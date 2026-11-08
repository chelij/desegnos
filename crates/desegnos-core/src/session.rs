use crate::dossier::HistoryTransition;
use crate::{
    Command, Error, ExportRecord, GenerationCandidate, Project, RequirementsSnapshot, Result,
    ensure,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionHistory {
    pub session_id: uuid::Uuid,
    pub start: usize,
    pub position: usize,
    pub end: usize,
    pub version: u64,
}

impl Project {
    fn carry_metadata(&mut self, current: &Project, transition: HistoryTransition) {
        self.carry_generation_metadata(current, transition);
        self.last_export = current.last_export.clone();
        for pointer in &mut self.pointers {
            if let Some(live) = current.pointers.iter().find(|live| live.id == pointer.id) {
                pointer.wiring = live.wiring.clone();
            }
        }
    }
}

pub struct Session {
    project: Project,
    undo: Vec<Project>,
    history_entries: Vec<HistoryEntry>,
    redo: Vec<Project>,
    user_revision_floor: u64,
    history_id: uuid::Uuid,
    history_start: usize,
    history_version: u64,
    burst: Option<(crate::commands::HistoryBurst, std::time::Instant)>,
    version_labels: Vec<String>,
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HistoryEntry {
    pub label: String,
    pub at: i64,
    pub position: usize,
}
impl HistoryEntry {
    fn new(label: &str, position: usize) -> Self {
        Self {
            label: label.into(),
            position,
            at: match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                Ok(duration) => duration.as_millis().min(i64::MAX as u128) as i64,
                Err(error) => -(error.duration().as_millis().min(i64::MAX as u128) as i64),
            },
        }
    }
}

const MAX_UNDO_STEPS: usize = 200;

impl Session {
    pub fn new(mut project: Project) -> Result<Self> {
        project.store_inline_images()?;
        project.keep_one_request_per_layer();
        project.validate()?;
        let user_revision_floor = project.revision;
        Ok(Self {
            project,
            undo: vec![],
            history_entries: vec![HistoryEntry::new("Opened project", 0)],
            redo: vec![],
            user_revision_floor,
            history_id: uuid::Uuid::new_v4(),
            history_start: 0,
            history_version: user_revision_floor,
            burst: None,
            version_labels: vec![],
        })
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn history(&self) -> SessionHistory {
        let position = self.history_start + self.undo.len();
        SessionHistory {
            session_id: self.history_id,
            start: self.history_start,
            position,
            end: position + self.redo.len(),
            version: self.history_version,
        }
    }

    pub fn history_entries(&self) -> &[HistoryEntry] {
        &self.history_entries
    }

    pub fn execute(&mut self, revision: u64, command: Command) -> Result<()> {
        self.assert_revision(revision)?;
        let policy = command.history(&self.project);
        let candidate = self.project.apply(self.project.revision, command)?;
        if candidate.revision == self.project.revision {
            return Ok(());
        }
        self.commit_or_merge(candidate, policy.burst, policy.label);
        Ok(())
    }

    pub fn take_version_labels(&mut self) -> Vec<String> {
        std::mem::take(&mut self.version_labels)
    }

    pub fn record_export(&mut self, record: ExportRecord) -> Result<()> {
        let candidate = self
            .project
            .apply(self.project.revision, Command::RecordExport { record })?;
        if candidate.revision == self.project.revision {
            return Ok(());
        }
        self.project = candidate;
        self.user_revision_floor = self.project.revision;
        self.history_version = self.project.revision;
        self.burst = None;
        Ok(())
    }

    pub fn record_generation(
        &mut self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        attempt_ordinal: usize,
    ) -> Result<()> {
        let next = self
            .project
            .record_generation(snapshot, candidate, attempt_ordinal)?;
        if next.revision == self.project.revision {
            return Ok(());
        }
        self.project = next;
        self.version_labels.push("AI filled frames".into());

        self.end_history_step();
        Ok(())
    }

    pub fn apply_generation(
        &mut self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        accepted_decision_indices: &[usize],
        generated_pictures: &std::collections::BTreeMap<String, String>,
    ) -> Result<()> {
        let next = crate::apply_generation_output(
            &self.project,
            snapshot,
            candidate,
            accepted_decision_indices,
            generated_pictures,
        )?;
        if next.revision == self.project.revision {
            return Ok(());
        }

        self.commit(next, "AI updated layers");
        Ok(())
    }

    pub fn import_context(&mut self, revision: u64, context: crate::ImportedContext) -> Result<()> {
        self.assert_revision(revision)?;
        let candidate = self
            .project
            .import_context(self.project.revision, context)?;
        if candidate.revision == self.project.revision {
            return Ok(());
        }
        self.commit(candidate, "Imported brand");
        Ok(())
    }

    fn commit_or_merge(
        &mut self,
        candidate: Project,
        key: Option<crate::commands::HistoryBurst>,
        label: &str,
    ) {
        self.commit_or_merge_at(candidate, key, std::time::Instant::now(), label);
    }

    fn commit_or_merge_at(
        &mut self,
        candidate: Project,
        key: Option<crate::commands::HistoryBurst>,
        now: std::time::Instant,
        label: &str,
    ) {
        let first_text = key.as_ref().is_some_and(|key| key.ends_after_merge());
        let continues = key.is_some()
            && !self.undo.is_empty()
            && self.burst.as_ref().is_some_and(|(current, at)| {
                Some(current) == key.as_ref()
                    && current
                        .pause()
                        .is_none_or(|pause| now.duration_since(*at) < pause)
            });
        if continues {
            self.user_revision_floor = candidate.revision;
            self.history_version = candidate.revision;
            self.project = candidate;
            self.redo.clear();
        } else {
            self.commit(candidate, label);
        }

        self.burst = if continues && first_text {
            None
        } else {
            key.map(|key| (key, now))
        };
    }

    pub fn end_history_step(&mut self) {
        if !self
            .burst
            .as_ref()
            .is_some_and(|(key, _)| key.survives_focus_change())
        {
            self.burst = None;
        }
    }

    fn commit(&mut self, candidate: Project, label: &str) {
        self.burst = None;
        let position = self.history_start + self.undo.len() + 1;
        self.history_entries
            .retain(|entry| entry.position < position);
        self.history_entries
            .push(HistoryEntry::new(label, position));
        self.version_labels.push(label.to_owned());
        self.user_revision_floor = candidate.revision;
        self.history_version = candidate.revision;
        self.undo
            .push(std::mem::replace(&mut self.project, candidate));
        if self.undo.len() > MAX_UNDO_STEPS {
            self.undo.remove(0);
            self.history_start += 1;
            self.history_entries
                .retain(|entry| entry.position >= self.history_start);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> Result<()> {
        let mut candidate = self
            .undo
            .last()
            .cloned()
            .ok_or_else(|| Error("There is no edit to undo.".into()))?;
        candidate.revision = crate::limits::next_revision(self.project.revision)?;
        candidate.carry_metadata(&self.project, HistoryTransition::Undo);
        candidate.validate()?;
        self.burst = None;
        self.version_labels.push("Undid a change".into());
        self.user_revision_floor = candidate.revision;
        self.undo.pop();
        self.redo
            .push(std::mem::replace(&mut self.project, candidate));
        Ok(())
    }

    pub fn redo(&mut self) -> Result<()> {
        let mut candidate = self
            .redo
            .last()
            .cloned()
            .ok_or_else(|| Error("There is no edit to redo.".into()))?;
        candidate.revision = crate::limits::next_revision(self.project.revision)?;
        candidate.carry_metadata(&self.project, HistoryTransition::Redo);
        candidate.validate()?;
        self.burst = None;
        self.version_labels.push("Redid a change".into());
        self.user_revision_floor = candidate.revision;
        self.redo.pop();
        self.undo
            .push(std::mem::replace(&mut self.project, candidate));
        Ok(())
    }

    pub fn restore(&mut self, project: Project) -> Result<()> {
        let mut candidate = project;
        candidate.keep_one_request_per_layer();
        candidate.revision = crate::limits::next_revision(self.project.revision)?;
        candidate.carry_metadata(&self.project, HistoryTransition::Restore);
        candidate.validate()?;
        self.commit(candidate, "Restored a version");
        Ok(())
    }

    pub fn assert_revision(&self, revision: u64) -> Result<()> {
        ensure(
            revision >= self.user_revision_floor && revision <= self.project.revision,
            "The project changed before this operation could complete.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session::new(
            Project::new(
                "Steps".into(),
                crate::Viewport {
                    width: 800,
                    height: 600,
                },
                crate::Appearance::Light,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn renamed(session: &Session, name: &str) -> Project {
        session
            .project
            .apply(
                session.project.revision,
                Command::RenameProject { name: name.into() },
            )
            .unwrap()
    }

    fn key() -> Option<crate::commands::HistoryBurst> {
        Some(crate::commands::HistoryBurst::Edit("project-name".into()))
    }

    #[test]
    fn history_labels_follow_native_steps_and_survive_save_undo_and_redo() {
        let mut session = session();
        session
            .execute(
                session.project().revision,
                Command::RenameProject { name: "One".into() },
            )
            .unwrap();
        session
            .execute(
                session.project().revision,
                Command::RenameProject { name: "Two".into() },
            )
            .unwrap();
        assert_eq!(
            session.history_entries().len(),
            2,
            "a burst has one authoritative label"
        );
        let first_label = session.history_entries()[1].label.clone();
        assert!(!first_label.is_empty());
        session.take_version_labels();
        session.undo().unwrap();
        assert_eq!(session.history().position, 0);
        assert_eq!(session.history_entries()[1].label, first_label);
        session.redo().unwrap();
        assert_eq!(session.history().position, 1);
        assert_eq!(session.history_entries()[1].label, first_label);
        session.undo().unwrap();
        session
            .execute(
                session.project().revision,
                Command::SetGrid { enabled: false },
            )
            .unwrap();
        assert_eq!(
            session.history_entries().len(),
            2,
            "a new edit replaces the abandoned redo label"
        );
        assert_ne!(session.history_entries()[1].label, first_label);
        assert!(!session.can_redo());
    }

    #[test]
    fn undo_restores_the_previous_canvas_placement() {
        let mut session = session();
        let page_id = session.project.home_page_id;
        let size_id = session.project().page(page_id).unwrap().base_size_id();
        let revision = session.project.revision;
        session
            .execute(
                revision,
                Command::SetCanvasLayout {
                    placements: vec![crate::CanvasPlacement {
                        page_id,
                        size_id,
                        x: 80.0,
                        y: 40.0,
                        kind: crate::BoundaryKind::Screen,
                    }],
                    reference: vec![],
                },
            )
            .unwrap();
        let placed = session
            .project()
            .page(page_id)
            .unwrap()
            .size(size_id)
            .cloned()
            .unwrap();
        assert_eq!(placed.x, 80.0);
        assert_eq!(placed.y, 40.0);
        assert_eq!(placed.kind, crate::BoundaryKind::Screen);
        session.undo().unwrap();
        let undone = session
            .project()
            .page(page_id)
            .unwrap()
            .size(size_id)
            .cloned()
            .unwrap();
        assert_eq!(undone.x, 0.0);
        assert_eq!(undone.kind, crate::BoundaryKind::Page);
        session.redo().unwrap();
        assert_eq!(
            session
                .project()
                .page(page_id)
                .unwrap()
                .size(size_id)
                .unwrap()
                .kind,
            crate::BoundaryKind::Screen
        );
    }

    #[test]
    fn reference_add_and_update_are_one_undo_step_each() {
        let mut session = session();
        let item = crate::ReferenceItem {
            id: uuid::Uuid::new_v4(),
            rect: crate::Rect {
                x: 40.0,
                y: -40.0,
                width: 200.0,
                height: 120.0,
            },
            kind: crate::ReferenceKind::Note {
                text: "reference".into(),
            },
        };
        let item_id = item.id;
        let position = session.history().position;
        session
            .execute(
                session.project().revision,
                Command::AddReferenceItem {
                    item,
                    picture_data_uri: None,
                },
            )
            .unwrap();
        assert_eq!(session.history().position, position + 1, "one history step");
        assert_eq!(session.project().reference.len(), 1);
        session.undo().unwrap();
        assert!(session.project().reference.is_empty());
        session.redo().unwrap();
        assert_eq!(session.project().reference.len(), 1);
        let position = session.history().position;
        session
            .execute(
                session.project().revision,
                Command::UpdateReferenceItem {
                    item_id,
                    rect: Some(crate::Rect {
                        x: 80.0,
                        y: -80.0,
                        width: 240.0,
                        height: 160.0,
                    }),
                    text: Some("edited reference".into()),
                    url: None,
                    title: None,
                    color: None,
                },
            )
            .unwrap();
        assert_eq!(session.history().position, position + 1, "one history step");
        assert_eq!(
            session.project().reference[0].kind,
            crate::ReferenceKind::Note {
                text: "edited reference".into()
            }
        );
        session.undo().unwrap();
        assert_eq!(
            session.project().reference[0].kind,
            crate::ReferenceKind::Note {
                text: "reference".into()
            }
        );
        assert_eq!(session.project().reference[0].rect.x, 40.0);
        let position = session.history().position;
        session
            .execute(
                session.project().revision,
                Command::RemoveReferenceItems { ids: vec![item_id] },
            )
            .unwrap();
        assert!(session.project().reference.is_empty());
        assert_eq!(session.history().position, position + 1, "one history step");
        session.undo().unwrap();
        assert_eq!(session.project().reference.len(), 1);
        assert_eq!(
            session.project().reference[0].kind,
            crate::ReferenceKind::Note {
                text: "reference".into()
            }
        );
    }

    #[test]
    fn edits_under_the_step_gap_fold_into_one_history_step() {
        let mut session = session();
        let start = std::time::Instant::now();
        session.commit_or_merge_at(
            renamed(&session, "One"),
            key(),
            start,
            "Renamed the project",
        );
        session.commit_or_merge_at(
            renamed(&session, "Two"),
            key(),
            start + std::time::Duration::from_secs(5) - std::time::Duration::from_millis(1),
            "Renamed the project",
        );
        assert_eq!(
            session.history().position,
            1,
            "still the same history position"
        );
        assert_eq!(session.undo.len(), 1);
        assert_eq!(session.project.name, "Two");
    }

    #[test]
    fn edits_after_the_step_gap_start_a_new_history_step() {
        let mut session = session();
        let start = std::time::Instant::now();
        session.commit_or_merge_at(
            renamed(&session, "One"),
            key(),
            start,
            "Renamed the project",
        );
        let last = start + std::time::Duration::from_secs(4);
        session.commit_or_merge_at(renamed(&session, "Two"), key(), last, "Renamed the project");
        assert_eq!(session.history().position, 1);
        session.commit_or_merge_at(
            renamed(&session, "Three"),
            key(),
            last + std::time::Duration::from_secs(5) + std::time::Duration::from_millis(1),
            "Renamed the project",
        );
        assert_eq!(
            session.history().position,
            2,
            "a pause committed a new step"
        );
        assert_eq!(session.undo.len(), 2);
        session.undo().unwrap();
        assert_eq!(session.project.name, "Two");
    }
}
