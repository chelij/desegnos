use crate::{
    Command, Error, ExportRecord, GenerationApplicationStatus, GenerationCandidate, Project,
    RequirementsSnapshot, Result, ensure,
};

pub struct Session {
    project: Project,
    undo: Vec<Project>,
    history_entries: Vec<HistoryEntry>,
    redo: Vec<Project>,
    user_revision_floor: u64,
    history_id: uuid::Uuid,
    history_start: usize,
    history_version: u64,
    /// The edit key whose burst is in progress, and when it last changed: a run of edits to
    /// one box, element, page or project setting is one history step, not one per keystroke.
    burst: Option<(String, std::time::Instant)>,
    /// Labels of the history steps committed since the last saved version.
    version_labels: Vec<String>,
    /// The label of the edit that is about to be committed.
    pending_label: Option<&'static str>,
}

/// Authoritative session step labels, retained across undo and redo.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HistoryEntry {
    pub label: String,
    pub at: u64,
    pub position: usize,
}
impl HistoryEntry {
    fn new(label: &str, position: usize) -> Self {
        Self {
            label: label.into(),
            position,
            at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }
}

/// Edits to the same thing that pause longer than this start a new history step.
const NEW_LAYER: &str = "new-layer:";
const STEP_PAUSE: std::time::Duration = std::time::Duration::from_secs(5);

impl Session {
    pub fn new(mut project: Project) -> Result<Self> {
        project.store_inline_images()?;
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
            pending_label: None,
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

    /// Session-local undo coordinates. Equal project contents remain distinct edits.
    pub fn history(&self) -> (uuid::Uuid, usize, usize, usize, u64) {
        let position = self.history_start + self.undo.len();
        (
            self.history_id,
            self.history_start,
            position,
            position + self.redo.len(),
            self.history_version,
        )
    }

    pub fn history_entries(&self) -> &[HistoryEntry] {
        &self.history_entries
    }

    pub fn execute(
        &mut self,
        revision: u64,
        command: Command,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        self.assert_revision(revision)?;
        let key = self.burst_key(&command);
        let label = command.version_label();
        let candidate = self.project.apply(self.project.revision, command)?;
        if candidate.revision == self.project.revision {
            return Ok(());
        }
        durable(&candidate)?;
        self.pending_label = Some(label);
        self.commit_or_merge(candidate, key);
        Ok(())
    }

    /// The labels of the history steps since the last saved version, oldest first.
    pub fn take_version_labels(&mut self) -> Vec<String> {
        std::mem::take(&mut self.version_labels)
    }

    /// Edits that land on the same thing in quick succession become one history step.
    fn burst_key(&self, command: &Command) -> Option<String> {
        match command {
            Command::NudgeElements { moves, .. } => {
                let mut targets: Vec<_> = moves
                    .iter()
                    .map(|movement| {
                        format!(
                            "{}:{:?}:{}",
                            movement.id, movement.size_id, movement.all_sizes
                        )
                    })
                    .collect();
                targets.sort();
                Some(format!("nudge:{}", targets.join(",")))
            }
            // Only the text of a request changed (its targets and attachments stayed put).
            Command::UpdateRequest { request } => self
                .project
                .requests
                .iter()
                .find(|existing| existing.id == request.id)
                .filter(|existing| {
                    existing.attachments == request.attachments
                        && existing.targets == request.targets
                })
                .map(|existing| format!("request:{}", existing.id)),
            Command::UpdatePage { page_id, .. } => Some(format!("page:{page_id}")),
            Command::UpdatePageSize {
                page_id, size_id, ..
            } => Some(format!("size:{page_id}:{size_id}")),
            // Only the comment changed; endpoints stayed put.
            Command::UpdatePointer {
                pointer_id,
                from: None,
                to: None,
                ..
            } => Some(format!("pointer-comment:{pointer_id}")),
            Command::RenameProject { .. } => Some("project-name".into()),
            Command::SetGridSettings { .. } => Some("grid".into()),
            // Placing a layer and typing its first text is one step, however long the typing.
            Command::InsertElements { elements, .. } if elements.len() == 1 => {
                Some(format!("{NEW_LAYER}{}", elements[0].id))
            }
            Command::UpdateElements { patches, .. }
                if patches.len() == 1 && patches[0].text.is_some() =>
            {
                Some(format!("{NEW_LAYER}{}", patches[0].id))
            }
            _ => None,
        }
    }

    /// Record a finished export without adding an undo step: it is metadata about the design,
    /// not a design change. Undo and Redo carry the newest export record forward.
    pub fn record_export(
        &mut self,
        record: ExportRecord,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        let candidate = self
            .project
            .apply(self.project.revision, Command::RecordExport { record })?;
        if candidate.revision == self.project.revision {
            return Ok(());
        }
        durable(&candidate)?;
        self.project = candidate;
        self.user_revision_floor = self.project.revision;
        self.history_version = self.project.revision;
        self.burst = None;
        self.pending_label = None;
        Ok(())
    }

    pub fn record_generation(
        &mut self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        attempt_ordinal: usize,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        let next = self
            .project
            .record_generation(snapshot, candidate, attempt_ordinal)?;
        if next.revision == self.project.revision {
            return Ok(());
        }
        durable(&next)?;
        self.project = next;
        self.version_labels.push("AI filled frames".into());
        // A recorded Generate result ends any step in progress.
        self.end_history_step();
        Ok(())
    }

    /// Apply one validated generation reply as one undo step.
    pub fn apply_generation(
        &mut self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        accepted_decision_indices: &[usize],
        generated_pictures: &std::collections::BTreeMap<String, String>,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        let next = crate::apply_generation_output(
            &self.project,
            snapshot,
            candidate,
            accepted_decision_indices,
            generated_pictures,
        )?;
        durable(&next)?;
        // Applying a reply publishes layers: it is one design edit and one undo step.
        self.pending_label = Some("AI updated layers");
        self.commit(next);
        Ok(())
    }

    pub fn import_context(
        &mut self,
        revision: u64,
        context: crate::ImportedContext,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        self.assert_revision(revision)?;
        let candidate = self
            .project
            .import_context(self.project.revision, context)?;
        durable(&candidate)?;
        self.pending_label = Some("Imported brand");
        self.commit(candidate);
        Ok(())
    }

    /// Commit the edit, or fold it into the step its burst key is still building, then make
    /// it the current burst. `commit` drops the burst, so the new key is set afterwards.
    fn commit_or_merge(&mut self, candidate: Project, key: Option<String>) {
        self.commit_or_merge_at(candidate, key, std::time::Instant::now());
    }

    fn commit_or_merge_at(
        &mut self,
        candidate: Project,
        key: Option<String>,
        now: std::time::Instant,
    ) {
        let first_text = key.as_deref().is_some_and(|key| key.starts_with(NEW_LAYER));
        let continues = key.is_some()
            && !self.undo.is_empty()
            && self.burst.as_ref().is_some_and(|(current, at)| {
                Some(current) == key.as_ref()
                    && (first_text
                        || now.duration_since(*at)
                            < if current.starts_with("nudge:") {
                                std::time::Duration::from_millis(500)
                            } else {
                                STEP_PAUSE
                            })
            });
        if continues {
            self.user_revision_floor = candidate.revision;
            self.history_version = candidate.revision;
            self.project = candidate;
            self.redo.clear();
        } else {
            self.commit(candidate);
        }
        // Only the first text joins the placing step; later edits are steps of their own.
        self.burst = if continues && first_text {
            None
        } else {
            key.map(|key| (key, now))
        };
        self.pending_label = None;
    }

    /// End the step in progress: the next edit starts a new one. Called when focus leaves an
    /// editor, a generation result lands, or the project is saved or switched.
    pub fn end_history_step(&mut self) {
        // Closing the editor of a just-placed layer still lets its first text join that step.
        if !self
            .burst
            .as_ref()
            .is_some_and(|(key, _)| key.starts_with(NEW_LAYER))
        {
            self.burst = None;
        }
    }

    fn commit(&mut self, candidate: Project) {
        self.burst = None;
        let position = self.history_start + self.undo.len() + 1;
        self.history_entries
            .retain(|entry| entry.position < position);
        self.history_entries.push(HistoryEntry::new(
            self.pending_label.unwrap_or("Changed the project"),
            position,
        ));
        if let Some(label) = self.pending_label.take() {
            self.version_labels.push(label.to_owned());
        }
        self.user_revision_floor = candidate.revision;
        self.history_version = candidate.revision;
        self.undo
            .push(std::mem::replace(&mut self.project, candidate));
        if self.undo.len() > 200 {
            self.undo.remove(0);
            self.history_start += 1;
            self.history_entries
                .retain(|entry| entry.position >= self.history_start);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, durable: impl FnOnce(&Project) -> Result<()>) -> Result<()> {
        let mut candidate = self
            .undo
            .last()
            .cloned()
            .ok_or_else(|| Error("There is no edit to undo.".into()))?;
        candidate.revision = self
            .project
            .revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        candidate.applied_job_ids = self.project.applied_job_ids.clone();
        let generation_was_applied =
            self.project
                .latest_generation
                .as_ref()
                .is_some_and(|dossier| {
                    dossier.application_status == GenerationApplicationStatus::Applied
                        && !self
                            .undo
                            .last()
                            .unwrap()
                            .applied_job_ids
                            .contains(&dossier.job_id)
                });
        candidate.latest_generation = self.project.latest_generation.clone();
        self.carry_export_metadata(&mut candidate);
        if generation_was_applied && let Some(dossier) = &mut candidate.latest_generation {
            dossier.application_status = GenerationApplicationStatus::Undone;
        }
        candidate.validate()?;
        durable(&candidate)?;
        self.burst = None;
        self.pending_label = None;
        self.version_labels.push("Undid a change".into());
        self.user_revision_floor = candidate.revision;
        self.undo.pop();
        self.redo
            .push(std::mem::replace(&mut self.project, candidate));
        Ok(())
    }

    pub fn redo(&mut self, durable: impl FnOnce(&Project) -> Result<()>) -> Result<()> {
        let mut candidate = self
            .redo
            .last()
            .cloned()
            .ok_or_else(|| Error("There is no edit to redo.".into()))?;
        candidate.revision = self
            .project
            .revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        candidate.applied_job_ids = self.project.applied_job_ids.clone();
        let generation_is_restored =
            self.project
                .latest_generation
                .as_ref()
                .is_some_and(|current| {
                    current.application_status == GenerationApplicationStatus::Undone
                        && candidate
                            .latest_generation
                            .as_ref()
                            .is_some_and(|restored| {
                                restored.application_status == GenerationApplicationStatus::Applied
                                    && restored.job_id == current.job_id
                                    && restored.candidate_sha256 == current.candidate_sha256
                            })
                });
        candidate.latest_generation = self.project.latest_generation.clone();
        self.carry_export_metadata(&mut candidate);
        if generation_is_restored && let Some(dossier) = &mut candidate.latest_generation {
            dossier.application_status = GenerationApplicationStatus::Applied;
            dossier.applied_at_revision = Some(candidate.revision);
        }
        candidate.validate()?;
        durable(&candidate)?;
        self.burst = None;
        self.pending_label = None;
        self.version_labels.push("Redid a change".into());
        self.user_revision_floor = candidate.revision;
        self.redo.pop();
        self.undo
            .push(std::mem::replace(&mut self.project, candidate));
        Ok(())
    }

    /// Export records and the wiring they stamped are metadata: a design Undo never rolls them
    /// back, so a clash check keeps comparing against the last real export.
    fn carry_export_metadata(&self, candidate: &mut Project) {
        candidate.last_export = self.project.last_export.clone();
        for pointer in &mut candidate.pointers {
            if let Some(current) = self
                .project
                .pointers
                .iter()
                .find(|item| item.id == pointer.id)
            {
                pointer.wiring = current.wiring.clone();
            }
        }
    }

    /// Make a saved version the current state as one undo step. The caller has already stored
    /// it as a new version, so history is never rewritten.
    pub fn restore(
        &mut self,
        project: Project,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        let mut candidate = project;
        candidate.revision = self
            .project
            .revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        candidate.applied_job_ids = self.project.applied_job_ids.clone();
        candidate.latest_generation = self.project.latest_generation.clone();
        candidate.validate()?;
        durable(&candidate)?;
        self.pending_label = Some("Restored a version");
        self.commit(candidate);
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

    fn key() -> Option<String> {
        Some("project-name".into())
    }

    #[test]
    fn history_labels_follow_native_steps_and_survive_save_undo_and_redo() {
        let mut session = session();
        session
            .execute(
                session.project().revision,
                Command::RenameProject { name: "One".into() },
                |_| Ok(()),
            )
            .unwrap();
        session
            .execute(
                session.project().revision,
                Command::RenameProject { name: "Two".into() },
                |_| Ok(()),
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
        session.undo(|_| Ok(())).unwrap();
        assert_eq!(session.history().2, 0);
        assert_eq!(session.history_entries()[1].label, first_label);
        session.redo(|_| Ok(())).unwrap();
        assert_eq!(session.history().2, 1);
        assert_eq!(session.history_entries()[1].label, first_label);
        session.undo(|_| Ok(())).unwrap();
        session
            .execute(
                session.project().revision,
                Command::SetGrid { enabled: false },
                |_| Ok(()),
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
                |_| Ok(()),
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
        session.undo(|_| Ok(())).unwrap();
        let undone = session
            .project()
            .page(page_id)
            .unwrap()
            .size(size_id)
            .cloned()
            .unwrap();
        assert_eq!(undone.x, 0.0);
        assert_eq!(undone.kind, crate::BoundaryKind::Page);
        session.redo(|_| Ok(())).unwrap();
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
        let position = session.history().2;
        session
            .execute(
                session.project().revision,
                Command::AddReferenceItem {
                    item,
                    picture_data_uri: None,
                },
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(session.history().2, position + 1, "one history step");
        assert_eq!(session.project().reference.len(), 1);
        session.undo(|_| Ok(())).unwrap();
        assert!(session.project().reference.is_empty());
        session.redo(|_| Ok(())).unwrap();
        assert_eq!(session.project().reference.len(), 1);
        let position = session.history().2;
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
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(session.history().2, position + 1, "one history step");
        assert_eq!(
            session.project().reference[0].kind,
            crate::ReferenceKind::Note {
                text: "edited reference".into()
            }
        );
        session.undo(|_| Ok(())).unwrap();
        assert_eq!(
            session.project().reference[0].kind,
            crate::ReferenceKind::Note {
                text: "reference".into()
            }
        );
        assert_eq!(session.project().reference[0].rect.x, 40.0);
        let position = session.history().2;
        session
            .execute(
                session.project().revision,
                Command::RemoveReferenceItems { ids: vec![item_id] },
                |_| Ok(()),
            )
            .unwrap();
        assert!(session.project().reference.is_empty());
        assert_eq!(session.history().2, position + 1, "one history step");
        session.undo(|_| Ok(())).unwrap();
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
        session.commit_or_merge_at(renamed(&session, "One"), key(), start);
        session.commit_or_merge_at(
            renamed(&session, "Two"),
            key(),
            start + STEP_PAUSE - std::time::Duration::from_millis(1),
        );
        assert_eq!(session.history().2, 1, "still the same history position");
        assert_eq!(session.undo.len(), 1);
        assert_eq!(session.project.name, "Two");
    }

    #[test]
    fn edits_after_the_step_gap_start_a_new_history_step() {
        let mut session = session();
        let start = std::time::Instant::now();
        session.commit_or_merge_at(renamed(&session, "One"), key(), start);
        let last = start + std::time::Duration::from_secs(4);
        session.commit_or_merge_at(renamed(&session, "Two"), key(), last);
        assert_eq!(session.history().2, 1);
        session.commit_or_merge_at(
            renamed(&session, "Three"),
            key(),
            last + STEP_PAUSE + std::time::Duration::from_millis(1),
        );
        assert_eq!(session.history().2, 2, "a pause committed a new step");
        assert_eq!(session.undo.len(), 2);
        session.undo(|_| Ok(())).unwrap();
        assert_eq!(session.project.name, "Two");
    }
}
