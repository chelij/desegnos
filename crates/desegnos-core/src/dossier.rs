use crate::{
    DecisionProposal, DesignCriticism, Error, GenerationCandidate, MAX_DECISION_PROPOSALS,
    MAX_DESIGN_CRITICISMS, MAX_GENERATION_REQUESTS, Project, RequirementsSnapshot, Result, Scope,
    digest, ensure, model::validate_text,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GenerationApplicationStatus {
    Draft,
    Applied,
    Undone,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DossierTarget {
    pub page_id: Uuid,
    pub element_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationDossier {
    pub job_id: Uuid,
    pub attempt_ordinal: usize,
    pub base_revision: u64,
    pub recorded_at_revision: u64,
    pub requirements_sha256: String,
    pub candidate_sha256: String,
    pub summary: String,
    pub targets: Vec<DossierTarget>,
    pub decision_proposals: Vec<DecisionProposal>,
    pub criticism: Vec<DesignCriticism>,
    pub application_status: GenerationApplicationStatus,
    #[serde(deserialize_with = "crate::required_nullable")]
    pub applied_at_revision: Option<u64>,
    pub accepted_decision_indices: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationDossierContext {
    pub job_id: Uuid,
    pub attempt_ordinal: usize,
    pub base_revision: u64,
    pub recorded_at_revision: u64,
    #[serde(deserialize_with = "crate::required_nullable")]
    pub summary: Option<String>,
    pub targets: Vec<DossierTarget>,
    pub decision_proposals: Vec<DecisionProposal>,
    pub criticism: Vec<DesignCriticism>,
    pub application_status: GenerationApplicationStatus,
    #[serde(deserialize_with = "crate::required_nullable")]
    pub applied_at_revision: Option<u64>,
    pub accepted_decision_indices: Vec<usize>,
}

impl GenerationDossier {
    pub(crate) fn from_candidate(
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        recorded_at_revision: u64,
        attempt_ordinal: usize,
    ) -> Result<Self> {
        let criticism = {
            // Conflict explanations survive partial apply even when advisory feedback fills its budget.
            let mut notes: Vec<DesignCriticism> = candidate
                .requests
                .iter()
                .filter(|request| request.status == crate::RequestStatus::Conflict)
                .map(|request| DesignCriticism {
                    element_id: request.element_id,
                    message: request.explanation.clone(),
                    suggestion: "This request stays open. Update its details and generate again."
                        .into(),
                })
                .collect();
            for note in &candidate.criticism {
                if notes.len() < MAX_DESIGN_CRITICISMS
                    && !notes.iter().any(|existing| {
                        existing.element_id == note.element_id && existing.message == note.message
                    })
                {
                    notes.push(note.clone());
                }
            }
            notes
        };
        Ok(Self {
            job_id: candidate.job_id,
            attempt_ordinal,
            base_revision: snapshot.base_revision,
            recorded_at_revision,
            requirements_sha256: snapshot.sha256.clone(),
            candidate_sha256: candidate.sha256()?,
            summary: candidate.summary.clone(),
            targets: snapshot
                .target_ids()?
                .into_iter()
                .map(|(page_id, element_id)| DossierTarget {
                    page_id,
                    element_id,
                })
                .collect(),
            decision_proposals: candidate.decision_proposals.clone(),
            criticism,
            application_status: GenerationApplicationStatus::Draft,
            applied_at_revision: None,
            accepted_decision_indices: vec![],
        })
    }

    pub(crate) fn validate(&self, project: &Project) -> Result<()> {
        ensure(
            !self.job_id.is_nil()
                && self.attempt_ordinal > 0
                && self.base_revision <= self.recorded_at_revision
                && self.recorded_at_revision <= project.revision,
            "The generation notes have invalid job or revision metadata.",
        )?;
        for value in [&self.requirements_sha256, &self.candidate_sha256] {
            ensure(
                value.len() == 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "The generation notes have an invalid source digest.",
            )?;
        }
        validate_text(&self.summary, "Generation summary", 16_384, false)?;
        ensure(
            !self.targets.is_empty() && self.targets.len() <= MAX_GENERATION_REQUESTS,
            "The generation notes must identify between 1 and 512 requested layers.",
        )?;
        let mut targets = BTreeSet::new();
        let mut elements = BTreeSet::new();
        let mut pages = BTreeSet::new();
        for target in &self.targets {
            ensure(
                !target.page_id.is_nil()
                    && !target.element_id.is_nil()
                    && targets.insert((target.page_id, target.element_id))
                    && elements.insert(target.element_id),
                "The generation notes contain an invalid or repeated layer.",
            )?;
            pages.insert(target.page_id);
        }
        ensure(
            self.decision_proposals.len() <= MAX_DECISION_PROPOSALS
                && self.criticism.len() <= MAX_DESIGN_CRITICISMS,
            "The generation notes exceed their proposal or feedback limit.",
        )?;
        let mut topics = BTreeSet::new();
        for proposal in &self.decision_proposals {
            validate_text(&proposal.topic, "Decision topic", 120, true)?;
            validate_text(&proposal.value, "Decision value", 16_384, true)?;
            validate_text(&proposal.rationale, "Decision rationale", 4096, true)?;
            ensure(
                match &proposal.scope {
                    Scope::Project => true,
                    Scope::Page(id) => pages.contains(id),
                    Scope::Element(id) => elements.contains(id),
                },
                "A generation note proposes a decision outside its recorded scope.",
            )?;
            ensure(
                topics.insert((
                    serde_json::to_string(&proposal.scope)
                        .map_err(|error| Error(error.to_string()))?,
                    proposal.topic.trim().to_lowercase(),
                )),
                "The generation notes repeat a decision topic in one scope.",
            )?;
        }
        for criticism in &self.criticism {
            validate_text(&criticism.message, "Design feedback", 4096, true)?;
            validate_text(&criticism.suggestion, "Design suggestion", 4096, true)?;
            ensure(
                criticism.element_id.is_none_or(|id| elements.contains(&id)),
                "Design feedback refers outside its recorded scope.",
            )?;
        }
        let mut selected = BTreeSet::new();
        for index in &self.accepted_decision_indices {
            ensure(
                *index < self.decision_proposals.len() && selected.insert(*index),
                "The generation notes have invalid accepted proposal indices.",
            )?;
        }
        match self.application_status {
            GenerationApplicationStatus::Draft => ensure(
                self.applied_at_revision.is_none()
                    && self.accepted_decision_indices.is_empty()
                    && !project.applied_job_ids.contains(&self.job_id),
                "Draft generation notes cannot claim accepted choices or applied source.",
            ),
            GenerationApplicationStatus::Applied | GenerationApplicationStatus::Undone => ensure(
                self.applied_at_revision.is_some_and(|revision| {
                    revision >= self.recorded_at_revision && revision <= project.revision
                }) && project.applied_job_ids.contains(&self.job_id),
                "Applied generation notes must identify a recorded application.",
            ),
        }
    }

    fn relevant_context(&self, targets: &[(Uuid, Uuid)]) -> Option<GenerationDossierContext> {
        let included: Vec<_> = self
            .targets
            .iter()
            .filter(|target| targets.contains(&(target.page_id, target.element_id)))
            .cloned()
            .collect();
        let relevant = |scope: &Scope| match scope {
            Scope::Project => true,
            Scope::Page(id) => targets.iter().any(|(page, _)| page == id),
            Scope::Element(id) => targets.iter().any(|(_, element)| element == id),
        };
        let mut proposals = Vec::new();
        let mut accepted = Vec::new();
        for (index, proposal) in self.decision_proposals.iter().enumerate() {
            if relevant(&proposal.scope) {
                if self.accepted_decision_indices.contains(&index) {
                    accepted.push(proposals.len());
                }
                proposals.push(proposal.clone());
            }
        }
        let complete_scope = included.len() == self.targets.len();
        let criticism: Vec<_> = self
            .criticism
            .iter()
            .filter(|criticism| {
                criticism.element_id.map_or(complete_scope, |id| {
                    targets.iter().any(|(_, element)| *element == id)
                })
            })
            .cloned()
            .collect();
        if included.is_empty() && proposals.is_empty() && criticism.is_empty() {
            return None;
        }
        Some(GenerationDossierContext {
            job_id: self.job_id,
            attempt_ordinal: self.attempt_ordinal,
            base_revision: self.base_revision,
            recorded_at_revision: self.recorded_at_revision,
            summary: complete_scope.then(|| self.summary.clone()),
            targets: included,
            decision_proposals: proposals,
            criticism,
            application_status: self.application_status,
            applied_at_revision: self.applied_at_revision,
            accepted_decision_indices: accepted,
        })
    }
}

impl Project {
    pub fn record_generation(
        &self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        attempt_ordinal: usize,
    ) -> Result<Self> {
        self.validate()?;
        candidate.validate_bounds()?;
        candidate.validate_identity(snapshot)?;
        ensure(attempt_ordinal > 0, "A generation attempt starts at 1.")?;
        ensure(
            snapshot.project_id == self.id
                && snapshot.sha256 == digest(snapshot.markdown.as_bytes()),
            "The requirements snapshot is invalid for this project.",
        )?;
        if let Some(existing) = &self.latest_generation
            && existing.job_id == candidate.job_id
        {
            ensure(
                existing.requirements_sha256 == snapshot.sha256,
                "This generation job already has different recorded requirements.",
            )?;
            ensure(
                attempt_ordinal >= existing.attempt_ordinal,
                "A newer attempt of this generation is already recorded.",
            )?;
            if attempt_ordinal == existing.attempt_ordinal {
                ensure(
                    existing.candidate_sha256 == candidate.sha256()?,
                    "This generation attempt already has a different recorded result.",
                )?;
                return Ok(self.clone());
            }
            ensure(
                existing.application_status == GenerationApplicationStatus::Draft,
                "An applied generation cannot be replaced by another attempt.",
            )?;
        }
        ensure(
            !self.applied_job_ids.contains(&candidate.job_id),
            "This generation job has already been applied.",
        )?;
        candidate.validate(snapshot, self)?;
        let mut next = self.clone();
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        next.latest_generation = Some(GenerationDossier::from_candidate(
            snapshot,
            candidate,
            next.revision,
            attempt_ordinal,
        )?);
        next.validate()?;
        Ok(next)
    }

    pub(crate) fn dossier_context(
        &self,
        targets: &[(Uuid, Uuid)],
    ) -> Option<GenerationDossierContext> {
        self.latest_generation
            .as_ref()
            .and_then(|dossier| dossier.relevant_context(targets))
    }
}
