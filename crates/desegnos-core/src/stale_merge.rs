//! Applying a generation result on top of edits the user made while its request was running.
//!
//! Every stale check and the apply itself go through this module, so a result that only overlaps
//! unrelated edits is merged instead of being reported as stale. Only frozen page and document
//! sources may be merged, together with the measured geometry a page refresh rewrites on its own:
//! a changed request, decision, route or note keeps the existing stale result, and a conflicting
//! merge is never accepted.

use crate::{
    DocumentTarget, Error, GenerationCandidate, Project, RequirementsSnapshot, Result, SiteContent,
    ensure,
};
use uuid::Uuid;

/// The region-root marker of a generated area.
const MARKER: &str = "data-desegnos-region";

/// A candidate resolved against the current project.
pub struct ResolvedGeneration {
    /// The current project with every frozen source restored, so the reply can be validated and
    /// applied against the exact snapshot it answered.
    pub base: Project,
    /// The candidate to apply, with the user's later edits merged into the pages and documents
    /// this generation changed.
    pub candidate: GenerationCandidate,
    /// Whether an edit made during generation was carried into the candidate.
    pub merged: bool,
}

impl RequirementsSnapshot {
    /// The current project with every frozen page and document source put back exactly as this
    /// request captured it.
    fn with_base_sources(&self, project: &Project) -> Result<Project> {
        let mut base = project.clone();
        for (key, content) in &self.base_sources {
            if !self.dependencies.contains_key(key) {
                continue;
            }
            if let Some(page_id) = key.strip_prefix("source:") {
                let page_id = Uuid::parse_str(page_id)
                    .map_err(|_| Error("Invalid page source dependency.".into()))?;
                base.pages
                    .iter_mut()
                    .find(|page| page.id == page_id)
                    .ok_or_else(|| Error("This page no longer exists.".into()))?
                    .content = content.clone();
                continue;
            }
            let Some(key) = key.strip_prefix("document:") else {
                continue;
            };
            match DocumentTarget::from_key(key)? {
                DocumentTarget::Page { page_id } => {
                    base.pages
                        .iter_mut()
                        .find(|page| page.id == page_id)
                        .ok_or_else(|| Error("This page no longer exists.".into()))?
                        .content = content.clone();
                }
                DocumentTarget::SiteFile { path } => {
                    let content = content.as_ref().ok_or_else(|| {
                        Error("A framed document was removed. Generate new requirements.".into())
                    })?;
                    base.set_site_document_html(&path, &content.html)?;
                }
            }
        }
        Ok(base)
    }

    /// `project` with the frozen measured geometry of every requested area put back.
    ///
    /// A page measurement rewrites geometry the user never set: an element-anchored request's box
    /// is the union of its measured element positions, a shaped or framed area carries the
    /// measured boxes of the elements inside it, and a framed request's box is mapped from its
    /// iframe. Only those fields are restored, so every other change to a request still stays
    /// stale; a free box keeps its own rect, which is the user's request.
    fn with_frozen_geometry(&self, project: &Project) -> Project {
        let mut base = project.clone();
        for page in &mut base.pages {
            for region in &mut page.regions {
                let Some(frozen) = self
                    .base_geometry
                    .get(&format!("region:{}:{}", page.id, region.id))
                else {
                    continue;
                };
                if region.has_targets() {
                    region.rect = frozen.rect;
                } else {
                    region.area_bounds = frozen.area_bounds.clone();
                }
                if let (Some(frame), Some(rect)) = (&mut region.frame_target, frozen.frame_rect) {
                    frame.rect = rect;
                }
            }
        }
        base
    }
}

/// The candidate to validate and apply on top of the current project. An unchanged project and a
/// mergeable set of source edits both succeed; every other change returns today's stale error.
pub fn resolve_edits_during_generation(
    snapshot: &RequirementsSnapshot,
    candidate: &GenerationCandidate,
    project: &Project,
) -> Result<ResolvedGeneration> {
    candidate.validate_identity(snapshot)?;
    let stale = match snapshot.validate_current(project) {
        Ok(()) => {
            return Ok(ResolvedGeneration {
                base: project.clone(),
                candidate: candidate.clone(),
                merged: false,
            });
        }
        Err(error) => error,
    };
    // The merge cannot loosen a changed request, decision, route or note: only restoring the
    // frozen sources and the measured geometry may make this snapshot current again.
    let Ok(base) = snapshot.with_base_sources(project) else {
        return Err(stale);
    };
    // A page measurement may also have refreshed the geometry this request never set, for example
    // after the user moved pictures on the page. Putting the frozen values back lets an untouched
    // request compare equal; anything else it changed still fails below.
    let base = if snapshot.validate_current(&base).is_ok() {
        base
    } else {
        let frozen = snapshot.with_frozen_geometry(&base);
        if snapshot.validate_current(&frozen).is_err() {
            return Err(stale);
        }
        frozen
    };
    let stale_error = || Error(stale.0.clone());
    let mut merged_candidate = candidate.clone();
    let mut merged = false;
    for page in &mut merged_candidate.pages {
        let Some(frozen) = snapshot
            .base_sources
            .get(&format!("source:{}", page.page_id))
        else {
            continue;
        };
        let Some(ours) = project.page(page.page_id)?.content.clone() else {
            continue;
        };
        let content =
            crate::merge_content(&frozen.clone().unwrap_or_else(empty), &ours, &page.content)
                .map_err(|_| stale_error())?;
        merged |= content != page.content;
        page.content = content;
    }
    for document in &mut merged_candidate.documents {
        let key = format!("document:{}", document.document.key());
        let Some(frozen) = snapshot.base_sources.get(&key) else {
            continue;
        };
        let Ok(ours) = project.document_content(&document.document) else {
            return Err(stale_error());
        };
        let content = crate::merge_content(
            &frozen.clone().unwrap_or_else(empty),
            &ours,
            &document.content,
        )
        .map_err(|_| stale_error())?;
        merged |= content != document.content;
        document.content = content;
    }
    if merged {
        validate_merged(snapshot, &base, candidate, &merged_candidate)
            .map_err(|_| stale_error())?;
    }
    Ok(ResolvedGeneration {
        base,
        candidate: merged_candidate,
        merged,
    })
}

fn empty() -> SiteContent {
    SiteContent {
        html: String::new(),
        css: String::new(),
        javascript: String::new(),
    }
}

/// The merged source must still be a complete page or document that contains the AI's work.
fn validate_merged(
    snapshot: &RequirementsSnapshot,
    base: &Project,
    original: &GenerationCandidate,
    merged: &GenerationCandidate,
) -> Result<()> {
    merged.validate_bounds()?;
    merged.validate_identity(snapshot)?;
    snapshot.format_version()?;
    for (index, page) in merged.pages.iter().enumerate() {
        if page.content == original.pages[index].content {
            continue;
        }
        page.content.validate()?;
        let after = base.parse_page_html(&page.content.html)?;
        crate::stable_targets::validate_ids(&after)?;
        {
            crate::validate_page_source(&page.content).map_err(|error| Error(error.to_string()))?;
            page.content.found_sources()?;
        }
        // The user's edits must not drop the area or element this reply implemented.
        let before = base.parse_page_html(&original.pages[index].content.html)?;
        for (page_id, region_id) in snapshot.target_ids()? {
            let implemented = original.regions.iter().any(|outcome| {
                outcome.region_id == region_id && outcome.status == crate::RegionStatus::Implemented
            });
            if page_id != page.page_id || !implemented {
                continue;
            }
            let Ok(source) = base.page(page_id) else {
                continue;
            };
            let Some(region) = source.regions.iter().find(|region| region.id == region_id) else {
                continue;
            };
            if region.frame_target.is_some() {
                continue;
            }
            if region.has_targets() {
                if !region.allow_target_removal {
                    for target in region.targets() {
                        ensure(
                            crate::editing::locate(&after, target).is_ok(),
                            "The merged page lost a generated element.",
                        )?;
                    }
                }
                continue;
            }
            let selector = format!("[{MARKER}=\"{region_id}\"]");
            if before.select(&selector).nodes().len() == 1 {
                ensure(
                    after.select(&selector).nodes().len() == 1,
                    "The merged page lost a generated area.",
                )?;
            }
        }
    }
    for (index, document) in merged.documents.iter().enumerate() {
        if document.content == original.documents[index].content {
            continue;
        }
        document.content.validate()?;
        crate::validate_imported_document_source(&document.content.html)
            .map_err(|error| Error(error.to_string()))?;
        let parsed = base.parse_document_target(&document.document, &document.content.html)?;
        crate::stable_targets::validate_ids(&parsed)?;
    }
    Ok(())
}
