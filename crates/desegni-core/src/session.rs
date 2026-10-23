use crate::{
    Command, Error, GenerationApplicationStatus, GenerationCandidate, Project,
    RequirementsSnapshot, Result, ensure,
};

pub struct Session {
    project: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    user_revision_floor: u64,
    history_id: uuid::Uuid,
    history_start: usize,
    history_version: u64,
    element_steps: Option<ElementSteps>,
    /// The request whose text is being typed, and when it last changed: a burst of typing in
    /// one box is one history step, not one per letter.
    typing: Option<(uuid::Uuid, std::time::Instant)>,
}

/// Typing that pauses longer than this starts a new history step.
const TYPING_PAUSE: std::time::Duration = std::time::Duration::from_secs(2);

/// Repeated "undo this element" presses walk further back through one element's versions.
struct ElementSteps {
    document: crate::DocumentTarget,
    element_id: String,
    /// Newest first; index 0 is the version the walk started from.
    versions: Vec<String>,
    index: usize,
    revision_after: u64,
}

/// The selected element's outer HTML, or None when that document has no such element.
fn element_version(
    project: &Project,
    document: &crate::DocumentTarget,
    element_id: &str,
) -> Option<String> {
    let html = project.document_html(document).ok()?;
    let doc = project.parse_document_target(document, &html).ok()?;
    let nodes = doc.select(&format!(
        "[{}=\"{}\"]",
        crate::ELEMENT_ID_ATTRIBUTE,
        element_id
    ));
    (nodes.nodes().len() == 1).then(|| nodes.nodes()[0].html().to_string())
}

impl Session {
    pub fn new(mut project: Project) -> Result<Self> {
        project.store_inline_images()?;
        project.validate()?;
        let user_revision_floor = project.revision;
        Ok(Self {
            project,
            undo: vec![],
            redo: vec![],
            user_revision_floor,
            history_id: uuid::Uuid::new_v4(),
            history_start: 0,
            history_version: user_revision_floor,
            element_steps: None,
            typing: None,
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

    pub fn execute(
        &mut self,
        revision: u64,
        command: Command,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        self.assert_revision(revision)?;
        // Only the text of a request changed (its box stayed put).
        let typed = match &command {
            Command::UpdateRegion {
                region_id, rect, ..
            } => self
                .project
                .pages
                .iter()
                .flat_map(|page| &page.regions)
                .find(|region| region.id == *region_id)
                .filter(|region| region.rect == *rect)
                .map(|region| region.id),
            _ => None,
        };
        let candidate = self.project.apply(self.project.revision, command)?;
        if candidate.revision == self.project.revision {
            return Ok(());
        }
        durable(&candidate)?;
        let continues = typed.is_some()
            && !self.undo.is_empty()
            && self
                .typing
                .is_some_and(|(id, at)| Some(id) == typed && at.elapsed() < TYPING_PAUSE);
        if continues {
            self.user_revision_floor = candidate.revision;
            self.history_version = candidate.revision;
            self.project = candidate;
            self.redo.clear();
        } else {
            self.commit(candidate);
        }
        self.typing = typed.map(|id| (id, std::time::Instant::now()));
        Ok(())
    }

    pub fn import_website(
        &mut self,
        revision: u64,
        files: &std::collections::BTreeMap<String, Vec<u8>>,
        remove: &[uuid::Uuid],
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        self.assert_revision(revision)?;
        let candidate = self.project.import_website(revision, files, remove)?;
        if candidate.revision != self.project.revision {
            durable(&candidate)?;
            self.commit(candidate);
        }
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
        self.commit(candidate);
        Ok(())
    }

    pub fn edit_page_source(
        &mut self,
        revision: u64,
        page_id: uuid::Uuid,
        edit: &crate::PageSourceEdit,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> crate::SourceEditResult<()> {
        self.assert_revision(revision)?;
        let candidate = self.project.edit_page_source(revision, page_id, edit)?;
        if candidate.revision != self.project.revision {
            durable(&candidate)?;
            self.commit(candidate);
        }
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
        self.edit_element_document(
            revision,
            &crate::DocumentTarget::Page { page_id },
            target,
            edit,
            durable,
        )
    }

    /// Edit an element in the current page or in an editable framed site HTML file.
    pub fn edit_element_document(
        &mut self,
        revision: u64,
        document: &crate::DocumentTarget,
        target: &crate::ElementTarget,
        edit: &crate::ElementEdit,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<Option<crate::ElementTarget>> {
        self.assert_revision(revision)?;
        let (next, target) =
            self.project
                .edit_element_in(document, self.project.revision, target, edit)?;
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
        self.paste_layer_document(
            revision,
            &crate::DocumentTarget::Page { page_id },
            clipboard,
            insertion,
            durable,
        )
    }

    pub fn paste_layer_document(
        &mut self,
        revision: u64,
        document: &crate::DocumentTarget,
        clipboard: &crate::LayerClipboard,
        insertion: Option<&crate::ElementTarget>,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<crate::LayerPasteSelection> {
        self.assert_revision(revision)?;
        let (next, selection) =
            self.project
                .paste_layer_in(self.project.revision, document, clipboard, insertion)?;
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

    pub fn refresh_element_target_bounds(
        &mut self,
        page_id: uuid::Uuid,
        viewport: crate::Viewport,
        bounds: &[crate::ElementTargetBounds],
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        let next = self
            .project
            .refresh_element_target_bounds(page_id, viewport, bounds)?;
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

    pub fn apply_generation_draft(
        &mut self,
        snapshot: &RequirementsSnapshot,
        draft: &crate::GenerationDraft,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<()> {
        let next = self.project.apply_generation_draft(snapshot, draft)?;
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

    /// Steps one element back to its previous version while every other change stays. Each
    /// further press goes one version older. It is an ordinary edit, so Undo reverts it.
    /// Only the element's own HTML (inline styles included) is restored, not shared CSS.
    pub fn undo_element(
        &mut self,
        revision: u64,
        page_id: uuid::Uuid,
        target: &crate::ElementTarget,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<Option<crate::ElementTarget>> {
        self.undo_element_document(
            revision,
            &crate::DocumentTarget::Page { page_id },
            target,
            durable,
        )
    }

    pub fn undo_element_document(
        &mut self,
        revision: u64,
        document: &crate::DocumentTarget,
        target: &crate::ElementTarget,
        durable: impl FnOnce(&Project) -> Result<()>,
    ) -> Result<Option<crate::ElementTarget>> {
        self.assert_revision(revision)?;
        ensure(
            !target.id.is_empty(),
            "This element has no earlier version to go back to.",
        )?;
        let continuing = self.element_steps.as_ref().is_some_and(|steps| {
            steps.document == *document
                && steps.element_id == target.id
                && steps.revision_after == self.project.revision
        });
        if !continuing {
            let mut versions: Vec<String> = Vec::new();
            let mut last_html: Option<String> = None;
            for project in std::iter::once(&self.project).chain(self.undo.iter().rev()) {
                // Snapshots that left this document untouched need no parsing.
                let html = project.document_html(document).ok();
                if html.is_some() && html == last_html {
                    continue;
                }
                last_html = html;
                let Some(version) = element_version(project, document, &target.id) else {
                    break;
                };
                if versions.last() != Some(&version) {
                    versions.push(version);
                }
            }
            self.element_steps = Some(ElementSteps {
                document: document.clone(),
                element_id: target.id.clone(),
                versions,
                index: 0,
                revision_after: self.project.revision,
            });
        }
        let steps = self.element_steps.as_ref().expect("set above");
        let html = steps
            .versions
            .get(steps.index + 1)
            .cloned()
            .ok_or_else(|| Error("This element has no earlier version.".into()))?;
        let (next, element) = self.project.edit_element_in(
            document,
            self.project.revision,
            target,
            &crate::ElementEdit::Replace {
                html,
                keep_style: false,
            },
        )?;
        if next.revision == self.project.revision {
            // Equal after normalisation: move past it so the next press goes further back.
            if let Some(steps) = self.element_steps.as_mut() {
                steps.index += 1;
            }
            return Ok(element);
        }
        durable(&next)?;
        self.commit(next);
        let steps = self.element_steps.as_mut().expect("set above");
        steps.index += 1;
        steps.revision_after = self.project.revision;
        Ok(element)
    }

    fn commit(&mut self, candidate: Project) {
        self.typing = None;
        self.user_revision_floor = candidate.revision;
        self.history_version = candidate.revision;
        self.undo
            .push(std::mem::replace(&mut self.project, candidate));
        if self.undo.len() > 200 {
            self.undo.remove(0);
            self.history_start += 1;
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
        self.typing = None;
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
        self.typing = None;
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
