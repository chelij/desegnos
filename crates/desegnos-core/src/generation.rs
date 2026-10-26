use crate::{
    Decision, Error, GenerationApplicationStatus, GenerationDossier, MAX_PAGES, Project,
    RequirementsSnapshot, Result, Scope, digest, ensure, model::validate_text,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const MAX_GENERATION_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_REQUIREMENTS_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SITE_SOURCE_BYTES: usize = 1024 * 1024;
pub const MAX_SITE_CONTENT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PROJECT_SOURCE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_GENERATION_REGIONS: usize = 512;
pub const MAX_COMPONENT_REQUESTS: usize = 512;
pub const MAX_DECISION_PROPOSALS: usize = 128;
pub const MAX_DESIGN_CRITICISMS: usize = 512;
pub const MAX_APPLIED_JOBS: usize = 10_000;
pub(crate) const MAX_EXPLANATION_CHARS: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SiteContent {
    pub html: String,
    pub css: String,
    pub javascript: String,
}

impl SiteContent {
    pub fn validate(&self) -> Result<()> {
        for (label, source) in [
            ("HTML", &self.html),
            ("CSS", &self.css),
            ("JavaScript", &self.javascript),
        ] {
            ensure(
                source.len() <= MAX_SITE_SOURCE_BYTES,
                format!("Generated {label} exceeds the 1 MiB source limit."),
            )?;
            ensure(
                !source.contains('\0'),
                format!("Generated {label} cannot contain null characters."),
            )?;
        }
        ensure(
            self.byte_len() <= MAX_SITE_CONTENT_BYTES,
            "Generated page sources exceed the 2 MiB page limit.",
        )
    }

    pub fn byte_len(&self) -> usize {
        self.html.len() + self.css.len() + self.javascript.len()
    }

    pub fn sha256(&self) -> Result<String> {
        self.validate()?;
        Ok(digest(
            &serde_json::to_vec(self).map_err(|error| Error(error.to_string()))?,
        ))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RegionStatus {
    Implemented,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegionOutcome {
    pub region_id: Uuid,
    pub status: RegionStatus,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratedPage {
    pub page_id: Uuid,
    pub content: SiteContent,
}

/// New source for an editable site HTML file shown inside an iframe (`documents` in a reply).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratedDocument {
    pub document: crate::DocumentTarget,
    pub content: SiteContent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ComponentResolution {
    Native,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentRequest {
    pub region_id: Uuid,
    pub capability: String,
    pub resolution: ComponentResolution,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionProposal {
    pub topic: String,
    pub value: String,
    pub scope: Scope,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignCriticism {
    #[serde(deserialize_with = "crate::required_nullable")]
    pub region_id: Option<Uuid>,
    pub message: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationCandidate {
    pub job_id: Uuid,
    pub requirements_sha256: String,
    pub summary: String,
    pub regions: Vec<RegionOutcome>,
    pub pages: Vec<GeneratedPage>,
    /// Framed site HTML documents; page documents are returned in `pages`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<GeneratedDocument>,
    pub component_requests: Vec<ComponentRequest>,
    pub decision_proposals: Vec<DecisionProposal>,
    pub criticism: Vec<DesignCriticism>,
}

impl GenerationCandidate {
    pub fn from_json(source: &str) -> Result<Self> {
        ensure(
            source.len() <= MAX_GENERATION_BYTES,
            "The generation result exceeds the 8 MiB response limit.",
        )?;
        let candidate: Self = serde_json::from_str(source)
            .map_err(|error| Error(format!("The generation result is invalid: {error}")))?;
        candidate.validate_bounds()?;
        Ok(candidate)
    }

    pub(crate) fn validate_bounds(&self) -> Result<()> {
        ensure(
            !self.job_id.is_nil(),
            "The generation job identifier is required.",
        )?;
        ensure(
            self.requirements_sha256.len() == 64
                && self
                    .requirements_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "The requirements digest must be a SHA-256 hexadecimal digest.",
        )?;
        validate_text(&self.summary, "Generation summary", 16_384, true)?;
        ensure(
            !self.regions.is_empty() && self.regions.len() <= MAX_GENERATION_REGIONS,
            "A generation result must account for between 1 and 512 areas.",
        )?;
        ensure(
            self.pages.len() <= MAX_PAGES,
            "The generation result exceeds the page limit.",
        )?;
        ensure(
            self.component_requests.len() <= MAX_COMPONENT_REQUESTS,
            "The generation result exceeds the component request limit.",
        )?;
        ensure(
            self.decision_proposals.len() <= MAX_DECISION_PROPOSALS,
            "The generation result exceeds the decision proposal limit.",
        )?;
        ensure(
            self.criticism.len() <= MAX_DESIGN_CRITICISMS,
            "The generation result exceeds the design feedback limit.",
        )?;
        for region in &self.regions {
            validate_text(
                &region.explanation,
                "Area outcome explanation",
                MAX_EXPLANATION_CHARS,
                true,
            )?;
        }
        for page in &self.pages {
            ensure(
                !page.content.html.trim().is_empty(),
                "Generated page HTML is required.",
            )?;
            page.content.validate()?;
        }
        for request in &self.component_requests {
            validate_text(&request.capability, "Component capability", 1024, true)?;
            validate_text(
                &request.reason,
                "Component resolution reason",
                MAX_EXPLANATION_CHARS,
                true,
            )?;
        }
        for proposal in &self.decision_proposals {
            validate_text(&proposal.topic, "Decision topic", 120, true)?;
            validate_text(&proposal.value, "Decision value", 16_384, true)?;
            validate_text(
                &proposal.rationale,
                "Decision rationale",
                MAX_EXPLANATION_CHARS,
                true,
            )?;
        }
        for criticism in &self.criticism {
            validate_text(
                &criticism.message,
                "Design feedback",
                MAX_EXPLANATION_CHARS,
                true,
            )?;
            validate_text(
                &criticism.suggestion,
                "Design suggestion",
                MAX_EXPLANATION_CHARS,
                true,
            )?;
        }
        ensure(
            serde_json::to_vec(self)
                .map_err(|error| Error(error.to_string()))?
                .len()
                <= MAX_GENERATION_BYTES,
            "The generation result exceeds the 8 MiB response limit.",
        )
    }

    pub fn validate(&self, snapshot: &RequirementsSnapshot, project: &Project) -> Result<()> {
        self.validate_bounds()?;
        snapshot.validate_current(project)?;
        self.validate_identity(snapshot)?;
        let targets = snapshot.target_ids()?;
        let target_regions: BTreeSet<_> = targets.iter().map(|(_, region)| *region).collect();
        let target_pages: BTreeSet<_> = targets.iter().map(|(page, _)| *page).collect();
        let mut regions = BTreeSet::new();
        for region in &self.regions {
            ensure(
                target_regions.contains(&region.region_id) && regions.insert(region.region_id),
                "The generation result contains a duplicate or unrequested area.",
            )?;
        }
        ensure(
            regions == target_regions,
            "The generation result omitted a requested area.",
        )?;
        snapshot.format_version()?;

        // Framed requests edit their target document; only Flexible may also change the host page.
        let mut required_pages = BTreeSet::new();
        let mut allowed_pages: BTreeSet<Uuid> = target_pages.clone();
        let mut required_documents: BTreeSet<crate::DocumentTarget> = BTreeSet::new();
        let mut allowed_documents: BTreeSet<crate::DocumentTarget> = BTreeSet::new();
        {
            allowed_pages.clear();
            for (page_id, region_id) in &targets {
                let page = project.page(*page_id)?;
                let region = page
                    .regions
                    .iter()
                    .find(|region| region.id == *region_id)
                    .ok_or_else(|| Error("A requested area no longer exists.".into()))?;
                let implemented = self.regions.iter().any(|outcome| {
                    outcome.region_id == *region_id && outcome.status == RegionStatus::Implemented
                });
                if !implemented {
                    continue;
                }
                match &region.frame_target {
                    None => {
                        required_pages.insert(*page_id);
                        allowed_pages.insert(*page_id);
                    }
                    Some(frame) => match &frame.document {
                        crate::DocumentTarget::Page { page_id } => {
                            required_pages.insert(*page_id);
                            allowed_pages.insert(*page_id);
                        }
                        crate::DocumentTarget::SiteFile { .. } => {
                            required_documents.insert(frame.document.clone());
                        }
                    },
                }
                if region.frame_target.is_some() && snapshot.is_flexible() {
                    allowed_pages.insert(*page_id);
                }
            }
        }
        // Format 25: a sequential part may change only its own documents. The framed documents
        // are repeated in every later part as read-only reference.
        let reference_documents = snapshot.reference_documents();
        let owned_documents = snapshot.editable_documents();
        let owned_pages = snapshot.editable_pages();
        if snapshot
            .batch
            .as_ref()
            .is_some_and(|batch| batch.part.is_some())
        {
            allowed_pages.retain(|page_id| owned_pages.contains(page_id));
            allowed_documents.retain(|document| owned_documents.contains(document));
            required_pages.retain(|page_id| owned_pages.contains(page_id));
            required_documents.retain(|document| owned_documents.contains(document));
        }
        // Format 23: a Flexible request may also update the documents it was given.
        for included in &snapshot.documents {
            if reference_documents.contains(&included.document) {
                continue;
            }
            match &included.document {
                crate::DocumentTarget::Page { page_id } => {
                    allowed_pages.insert(*page_id);
                }
                crate::DocumentTarget::SiteFile { .. } => {
                    allowed_documents.insert(included.document.clone());
                }
            }
        }
        let mut pages = BTreeSet::new();
        for page in &self.pages {
            let allowed = { allowed_pages.contains(&page.page_id) };
            ensure(
                allowed && pages.insert(page.page_id),
                "The generation result contains a duplicate or unrequested page.",
            )?;
        }
        {
            ensure(
                required_pages.iter().all(|id| pages.contains(id)),
                "Return source for exactly the documents with implemented regions.",
            )?;
            let mut documents = BTreeSet::new();
            for document in &self.documents {
                ensure(
                    (required_documents.contains(&document.document)
                        || (allowed_documents.contains(&document.document)))
                        && documents.insert(document.document.clone()),
                    "The generation result contains a duplicate or unrequested framed document.",
                )?;
            }
            ensure(
                required_documents
                    .iter()
                    .all(|document| documents.contains(document)),
                "Return source for exactly the framed documents with implemented regions.",
            )?;
        }
        {
            if !snapshot.is_flexible() {
                self.validate_conflicting_regions(snapshot, project)?;
            }
            let requests = self.image_generation_requests()?;
            if !requests.is_empty() {
                ensure(
                    snapshot
                        .capabilities
                        .as_ref()
                        .is_some_and(|c| c.image_generation.is_some()),
                    "Image generation is unavailable for these requirements.",
                )?;
                for request in requests {
                    request.reference_image(snapshot, project)?;
                    request.mask_image(snapshot, project)?;
                }
            }
        }

        let mut capabilities = BTreeSet::new();
        for request in &self.component_requests {
            ensure(
                target_regions.contains(&request.region_id),
                "A component request refers to an unrequested area.",
            )?;
            ensure(
                capabilities.insert((request.region_id, request.capability.trim().to_lowercase())),
                "The generation result repeats a component capability for one area.",
            )?;
            // Since format 9 a draft finishes with a clearly marked placeholder for a missing
            // service (for example a CAPTCHA that needs its own keys).
        }
        let mut proposal_topics = BTreeSet::new();
        for proposal in &self.decision_proposals {
            project.validate_scope(&proposal.scope)?;
            ensure(
                match &proposal.scope {
                    Scope::Project => true,
                    Scope::Page(id) => target_pages.contains(id),
                    Scope::Region(id) => target_regions.contains(id),
                },
                "A proposed decision exceeds the requested page or area scope.",
            )?;
            ensure(
                proposal_topics.insert((
                    serde_json::to_string(&proposal.scope)
                        .map_err(|error| Error(error.to_string()))?,
                    proposal.topic.trim().to_lowercase(),
                )),
                "The generation result repeats a decision topic in one scope.",
            )?;
        }
        for criticism in &self.criticism {
            ensure(
                criticism
                    .region_id
                    .is_none_or(|id| target_regions.contains(&id)),
                "Design feedback refers to an unrequested area.",
            )?;
        }
        {
            self.validate_project_images(project)?;
        }
        {
            for page in &self.pages {
                crate::validate_page_source(&page.content)
                    .map_err(|error| Error(error.to_string()))?;
                page.content.found_sources()?;
            }
        }
        self.validate_scope(snapshot, project)
    }

    pub(crate) fn validate_identity(&self, snapshot: &RequirementsSnapshot) -> Result<()> {
        ensure(
            self.job_id == snapshot.job_id && self.requirements_sha256 == snapshot.sha256,
            "The generation result belongs to a different job or requirements document.",
        )
    }

    pub fn sha256(&self) -> Result<String> {
        self.validate_bounds()?;
        Ok(digest(
            &serde_json::to_vec(self).map_err(|error| Error(error.to_string()))?,
        ))
    }

    pub fn is_ready(&self) -> bool {
        !self.regions.is_empty()
            && self
                .regions
                .iter()
                .all(|region| region.status == RegionStatus::Implemented)
            && self
                .component_requests
                .iter()
                .all(|request| request.resolution == ComponentResolution::Native)
    }

    /// Apply ready areas from a validated current-format result.
    pub fn can_apply(&self, snapshot: &RequirementsSnapshot) -> bool {
        snapshot.format_version().is_ok()
            && self
                .regions
                .iter()
                .any(|region| region.status == RegionStatus::Implemented)
            && (!self.pages.is_empty() || !self.documents.is_empty())
    }

    pub fn ensure_ready(&self) -> Result<()> {
        self.validate_bounds()?;
        if let Some(region) = self
            .regions
            .iter()
            .find(|region| region.status == RegionStatus::Conflict)
        {
            return Err(Error(format!(
                "Resolve the conflict for area {} before applying: {}",
                region.region_id, region.explanation
            )));
        }
        ensure(
            self.is_ready(),
            "Resolve unavailable component requirements before applying this generation.",
        )
    }
}

/// Merge the independently validated results of a batched request into one candidate. A region
/// implemented by any part wins over a conflict; page and document updates are disjoint by
/// construction because every part may change only its own documents.
pub fn merge_document_batches(
    job_id: Uuid,
    requirements_sha256: &str,
    batches: &[GenerationCandidate],
    unresolved: &[(Uuid, String)],
) -> Result<GenerationCandidate> {
    ensure(
        !batches.is_empty(),
        "A batched result needs at least one part.",
    )?;
    let mut merged = GenerationCandidate {
        job_id,
        requirements_sha256: requirements_sha256.into(),
        summary: String::new(),
        regions: Vec::new(),
        pages: Vec::new(),
        documents: Vec::new(),
        component_requests: Vec::new(),
        decision_proposals: Vec::new(),
        criticism: Vec::new(),
    };
    let mut summaries: Vec<String> = Vec::new();
    for candidate in batches {
        if !candidate.summary.trim().is_empty() {
            summaries.push(candidate.summary.trim().to_string());
        }
        for outcome in &candidate.regions {
            match merged
                .regions
                .iter_mut()
                .find(|existing| existing.region_id == outcome.region_id)
            {
                Some(existing) if existing.status == RegionStatus::Implemented => {}
                Some(existing) if outcome.status == RegionStatus::Implemented => {
                    *existing = outcome.clone();
                }
                Some(_) => {}
                None => merged.regions.push(outcome.clone()),
            }
        }
        for page in &candidate.pages {
            if !merged
                .pages
                .iter()
                .any(|existing| existing.page_id == page.page_id)
            {
                merged.pages.push(page.clone());
            }
        }
        for document in &candidate.documents {
            if !merged
                .documents
                .iter()
                .any(|existing| existing.document == document.document)
            {
                merged.documents.push(document.clone());
            }
        }
        for request in &candidate.component_requests {
            if !merged.component_requests.iter().any(|existing| {
                existing.region_id == request.region_id
                    && existing
                        .capability
                        .eq_ignore_ascii_case(&request.capability)
            }) {
                merged.component_requests.push(request.clone());
            }
        }
        for proposal in &candidate.decision_proposals {
            if !merged.decision_proposals.iter().any(|existing| {
                existing.scope == proposal.scope
                    && existing
                        .topic
                        .trim()
                        .eq_ignore_ascii_case(&proposal.topic.trim())
            }) {
                merged.decision_proposals.push(proposal.clone());
            }
        }
        for criticism in &candidate.criticism {
            if !merged.criticism.iter().any(|existing| {
                existing.region_id == criticism.region_id && existing.message == criticism.message
            }) {
                merged.criticism.push(criticism.clone());
            }
        }
    }
    // Every requested area must be accounted for even when its part failed.
    for (region_id, explanation) in unresolved {
        if merged
            .regions
            .iter()
            .any(|existing| existing.region_id == *region_id)
        {
            continue;
        }
        merged.regions.push(RegionOutcome {
            region_id: *region_id,
            status: RegionStatus::Conflict,
            explanation: explanation.chars().take(MAX_EXPLANATION_CHARS).collect(),
        });
    }
    merged.summary = summaries
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(16_384)
        .collect();
    if merged.summary.is_empty() {
        merged.summary = "The parts of this request were applied separately.".into();
    }
    merged.validate_bounds()?;
    Ok(merged)
}

impl Project {
    pub fn compatible_decision_proposals(
        &self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
    ) -> Result<Vec<usize>> {
        candidate.validate(snapshot, self)?;
        let mut selected: Vec<usize> = Vec::new();
        for (index, proposal) in candidate.decision_proposals.iter().enumerate() {
            let compatible = |topic: &str, value: &str, scope: &Scope| {
                !same_topic(topic, &proposal.topic)
                    || !self.scopes_overlap(scope, &proposal.scope)
                    || value.trim() == proposal.value.trim()
            };
            if self
                .decisions
                .iter()
                .all(|existing| compatible(&existing.topic, &existing.value, &existing.scope))
                && selected.iter().all(|other| {
                    let other = &candidate.decision_proposals[*other];
                    compatible(&other.topic, &other.value, &other.scope)
                })
            {
                selected.push(index);
            }
        }
        Ok(selected)
    }

    pub fn apply_generation(
        &self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        accepted_decision_indices: &[usize],
    ) -> Result<Self> {
        self.validate()?;
        candidate.validate_bounds()?;
        candidate.validate_identity(snapshot)?;
        ensure(
            snapshot.project_id == self.id
                && snapshot.sha256 == digest(snapshot.markdown.as_bytes()),
            "The requirements snapshot is invalid for this project.",
        )?;
        let mut selected = BTreeSet::new();
        for index in accepted_decision_indices {
            ensure(
                *index < candidate.decision_proposals.len() && selected.insert(*index),
                "Selected decision indices must be valid and unique.",
            )?;
        }
        if self.applied_job_ids.contains(&candidate.job_id) {
            return Ok(self.clone());
        }
        // Edits made while the request was running are merged into the generated sources when
        // they do not touch the same text; the reply itself is validated against its frozen base.
        let resolved = crate::resolve_edits_during_generation(snapshot, candidate, self)?;
        candidate.validate(snapshot, &resolved.base)?;
        let generated = &resolved.candidate;
        {
            ensure(
                candidate.can_apply(snapshot),
                "There are no implemented regions to apply.",
            )?;
        }
        if let Some(recorded) = &self.latest_generation
            && recorded.job_id == candidate.job_id
        {
            ensure(
                recorded.candidate_sha256 == candidate.sha256()?,
                "This generation job already has a different recorded result.",
            )?;
        }
        for index in &selected {
            let proposal = &candidate.decision_proposals[*index];
            for existing in &self.decisions {
                ensure(
                    !same_topic(&existing.topic, &proposal.topic)
                        || !self.scopes_overlap(&existing.scope, &proposal.scope)
                        || existing.value.trim() == proposal.value.trim(),
                    format!(
                        "The proposed decision '{}' conflicts with an accepted decision. Keep it unselected or revise the project decision explicitly.",
                        proposal.topic
                    ),
                )?;
            }
            for other in selected.range(..*index) {
                let other = &candidate.decision_proposals[*other];
                ensure(
                    !same_topic(&other.topic, &proposal.topic)
                        || !self.scopes_overlap(&other.scope, &proposal.scope)
                        || other.value.trim() == proposal.value.trim(),
                    format!(
                        "Selected proposals for '{}' conflict across overlapping scopes.",
                        proposal.topic
                    ),
                )?;
            }
        }
        {
            ensure(
                candidate.image_generation_requests()?.is_empty(),
                "Images must be prepared before applying.",
            )?;
        }
        let mut next = self.clone();
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        let mut placed = BTreeMap::new();
        for page in &generated.pages {
            let target = next
                .pages
                .iter_mut()
                .find(|existing| existing.id == page.page_id)
                .ok_or_else(|| Error("A generated page no longer exists.".into()))?;
            let mut content = page.content.clone();
            // Attached pictures are stored with their requests and inserted here, after the AI
            // has placed them, scaled to the size they are shown at.
            let attachments: Vec<_> = target
                .regions
                .iter()
                .flat_map(|region| region.attachments.iter())
                .collect();
            if let Some(html) = crate::attachments::insert_attached_images(
                &content.html,
                &attachments,
                target.viewport.width,
                &mut placed,
            )? {
                content.html = html;
                content.validate()?;
            }
            content.html = crate::source::assign_editor_ids(&content.html)?;
            // The AI writes one long line; keep a readable layout (sent compact again from format 14).
            let formatted = crate::format_site_content(&content);
            if formatted.validate().is_ok() {
                content = formatted;
            }
            target.content = Some(content);
        }
        for page in &mut next.pages {
            for region in &mut page.regions {
                if candidate.regions.iter().any(|target| {
                    target.region_id == region.id
                        && (false || target.status == RegionStatus::Implemented)
                }) || (candidate
                    .pages
                    .iter()
                    .any(|target| target.page_id == page.id)
                    && region.has_targets()
                    && region.prompt.trim().is_empty())
                {
                    region.completed = true;
                    // Only the placed, display-sized copies are kept.
                    region
                        .attachments
                        .retain(|item| item.kind != crate::AttachmentKind::Image);
                }
            }
        }
        for document in &generated.documents {
            let crate::DocumentTarget::SiteFile { path } = &document.document else {
                return Err(Error(
                    "Page source belongs in pages, not in documents.".into(),
                ));
            };
            ensure(
                next.is_editable_site_file(path),
                "A framed document is no longer an editable site HTML file.",
            )?;
            let html = crate::source::assign_editor_ids(&document.content.html)?;
            crate::source::validate_imported_document_source(&html)
                .map_err(|error| Error(error.to_string()))?;
            next.set_site_document_html(path, &html)?;
        }
        next.pictures.extend(placed);
        for index in &selected {
            let proposal = &candidate.decision_proposals[*index];
            if !next.decisions.iter().any(|existing| {
                existing.scope == proposal.scope && same_topic(&existing.topic, &proposal.topic)
            }) {
                next.decisions.push(Decision {
                    id: Uuid::new_v4(),
                    topic: proposal.topic.trim().into(),
                    value: proposal.value.trim().into(),
                    scope: proposal.scope.clone(),
                    protected: false,
                    accepted_at_revision: next.revision,
                });
            }
        }
        next.applied_job_ids.push(candidate.job_id);
        let mut dossier = self
            .latest_generation
            .as_ref()
            .filter(|dossier| dossier.job_id == candidate.job_id)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| {
                GenerationDossier::from_candidate(snapshot, candidate, next.revision, 1)
            })?;
        dossier.application_status = GenerationApplicationStatus::Applied;
        dossier.applied_at_revision = Some(next.revision);
        dossier.accepted_decision_indices = selected.into_iter().collect();
        next.latest_generation = Some(dossier);
        next.store_inline_images()?;
        next.validate()?;
        Ok(next)
    }

    fn scopes_overlap(&self, first: &Scope, second: &Scope) -> bool {
        match (first, second) {
            (Scope::Project, _) | (_, Scope::Project) => true,
            (Scope::Page(first), Scope::Page(second))
            | (Scope::Region(first), Scope::Region(second)) => first == second,
            (Scope::Page(page), Scope::Region(region))
            | (Scope::Region(region), Scope::Page(page)) => self.pages.iter().any(|candidate| {
                candidate.id == *page
                    && candidate
                        .regions
                        .iter()
                        .any(|candidate| candidate.id == *region)
            }),
        }
    }
}

fn same_topic(first: &str, second: &str) -> bool {
    first.trim().to_lowercase() == second.trim().to_lowercase()
}

pub fn candidate_schema() -> serde_json::Value {
    use serde_json::json;
    fn object(properties: serde_json::Value) -> serde_json::Value {
        let required: Vec<_> = properties
            .as_object()
            .expect("schema properties are objects")
            .keys()
            .cloned()
            .collect();
        json!({"type":"object","additionalProperties":false,"required":required,"properties":properties})
    }
    fn text(maximum: usize) -> serde_json::Value {
        json!({"type":"string","description":format!("At most {maximum} Unicode characters, enforced by Desegnos before acceptance.")})
    }
    fn array(items: serde_json::Value, maximum: usize) -> serde_json::Value {
        json!({"type":"array","maxItems":maximum,"items":items})
    }
    let uuid = json!({"type":"string","pattern":"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"});
    let source = json!({"type":"string","description":format!("At most {MAX_SITE_SOURCE_BYTES} UTF-8 bytes, enforced by Desegnos before acceptance. Return complete source without null characters.")});
    let content = object(json!({"html":source,"css":source,"javascript":source}));
    let scope = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["project"]}})),
        object(json!({"kind":{"type":"string","enum":["page"]},"id":uuid})),
        object(json!({"kind":{"type":"string","enum":["region"]},"id":uuid}))
    ]});
    let schema = object(json!({
        "jobId":uuid,
        "requirementsSha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
        "summary":text(16_384),
        "regions":array(object(json!({
            "regionId":uuid,
            "status":{"type":"string","enum":["implemented","conflict"]},
            "explanation":text(MAX_EXPLANATION_CHARS)
        })),MAX_GENERATION_REGIONS),
        "pages":array(object(json!({"pageId":uuid,"content":content})),MAX_PAGES),
        "documents":{"type":"array","maxItems":MAX_GENERATION_REGIONS,"description":"New source for framed site HTML documents only. Page documents belong in pages.","items":object(json!({
            "document":{"anyOf":[
                object(json!({"kind":{"type":"string","enum":["siteFile"]},"path":{"type":"string"}})),
                object(json!({"kind":{"type":"string","enum":["page"]},"pageId":uuid}))
            ]},
            "content":content
        }))},
        "componentRequests":array(object(json!({
            "regionId":uuid,
            "capability":text(1024),
            "resolution":{"type":"string","enum":["native","unavailable"]},
            "reason":text(MAX_EXPLANATION_CHARS)
        })),MAX_COMPONENT_REQUESTS),
        "decisionProposals":array(object(json!({
            "topic":text(120),
            "value":text(16_384),
            "scope":scope,
            "rationale":text(MAX_EXPLANATION_CHARS)
        })),MAX_DECISION_PROPOSALS),
        "criticism":array(object(json!({
            "regionId":{"anyOf":[uuid,{"type":"null"}]},
            "message":text(MAX_EXPLANATION_CHARS),
            "suggestion":text(MAX_EXPLANATION_CHARS)
        })),MAX_DESIGN_CRITICISMS)
    }));

    schema
}
