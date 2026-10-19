use crate::{
    Command, Error, GenerationApplicationStatus, GenerationCandidate, Project,
    RequirementsSnapshot, Result, ensure,
};

pub struct Session {
    project: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    user_revision_floor: u64,
}

impl Session {
    pub fn new(project: Project) -> Result<Self> {
        project.validate()?;
        let user_revision_floor = project.revision;
        Ok(Self {
            project,
            undo: vec![],
            redo: vec![],
            user_revision_floor,
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

    pub fn execute(
        &mut self,
        revision: u64,
        command: Command,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        self.assert_revision(revision)?;
        let candidate = self.project.apply(self.project.revision, command)?;
        if candidate.revision == self.project.revision {
            return Ok(());
        }
        durable(&candidate)?;
        self.commit(candidate);
        Ok(())
    }

    pub fn edit_element(
        &mut self,
        revision: u64,
        page_id: uuid::Uuid,
        target: &crate::ElementTarget,
        edit: &crate::ElementEdit,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<Option<crate::ElementTarget>> {
        self.assert_revision(revision)?;
        let (next, target) =
            self.project
                .edit_element(self.project.revision, page_id, target, edit)?;
        if next.revision != self.project.revision {
            durable(&next)?;
            self.commit(next);
        }
        Ok(target)
    }

    pub fn paste_layer(
        &mut self,
        revision: u64,
        page_id: uuid::Uuid,
        clipboard: &crate::LayerClipboard,
        insertion: Option<&crate::ElementTarget>,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<crate::LayerPasteSelection> {
        self.assert_revision(revision)?;
        let (next, selection) =
            self.project
                .paste_layer(self.project.revision, page_id, clipboard, insertion)?;
        durable(&next)?;
        self.commit(next);
        Ok(selection)
    }

    pub fn refresh_element_bounds(
        &mut self,
        page_id: uuid::Uuid,
        viewport: crate::Viewport,
        bounds: &[crate::ElementBounds],
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        let next = self
            .project
            .refresh_element_bounds(page_id, viewport, bounds)?;
        if next.revision != self.project.revision {
            durable(&next)?;
            self.project = next;
        }
        Ok(())
    }

    pub fn apply_generation(
        &mut self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        accepted_decision_indices: &[usize],
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        let next = self
            .project
            .apply_generation(snapshot, candidate, accepted_decision_indices)?;
        if next.revision == self.project.revision {
            return Ok(());
        }
        durable(&next)?;
        self.commit(next);
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
        Ok(())
    }

    fn commit(&mut self, candidate: Project) {
        self.user_revision_floor = candidate.revision;
        self.undo
            .push(std::mem::replace(&mut self.project, candidate));
        if self.undo.len() > 200 {
            self.undo.remove(0);
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
        if generation_was_applied && let Some(dossier) = &mut candidate.latest_generation {
            dossier.application_status = GenerationApplicationStatus::Undone;
        }
        candidate.validate()?;
        durable(&candidate)?;
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
        if generation_is_restored && let Some(dossier) = &mut candidate.latest_generation {
            dossier.application_status = GenerationApplicationStatus::Applied;
            dossier.applied_at_revision = Some(candidate.revision);
        }
        candidate.validate()?;
        durable(&candidate)?;
        self.user_revision_floor = candidate.revision;
        self.redo.pop();
        self.undo
            .push(std::mem::replace(&mut self.project, candidate));
        Ok(())
    }

    pub fn assert_revision(&self, revision: u64) -> Result<()> {
        ensure(
            revision >= self.user_revision_floor && revision <= self.project.revision,
            "The project changed before this operation could complete.",
        )
    }
}
