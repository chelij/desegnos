use crate::generation_apply::{
    OperationImages, apply_operations_with_sources, apply_page_operations,
    apply_pointer_classifications, apply_request, boundary_coverage_message,
    normalize_raw_page_patches, operation_kind, pending_for, untouched_boundaries,
};
use crate::generation_scope::scope_for;
use crate::limits::{
    MAX_DECISION_PROPOSALS, MAX_DESIGN_CRITICISMS, MAX_EXPLANATION_CHARS, MAX_GENERATED_IMAGES,
    MAX_GENERATION_BYTES, MAX_GENERATION_REQUESTS, MAX_POINTER_CLASSIFICATIONS, MAX_PROMPT_CHARS,
    MAX_REQUEST_OPERATIONS,
};
use crate::schema_conformance::{conform_to_candidate_schema, drop_null_optional_draft_fields};
use crate::{
    Error, Result, element_edit::ElementPatch, elements::ElementKind, elements::ElementLayout,
    elements::ElementStyle, model::PlayAction, model::Project, model::Scope,
    requirements::RequirementsSnapshot, validation::digest, validation::ensure,
    validation::validate_text,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

fn default_style() -> ElementStyle {
    crate::element_edit::empty_element(
        ElementKind::Box,
        crate::model::Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        },
    )
    .style
}

/// A new element the AI wants in a layer. The core assigns its id.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ElementDraft {
    /// Flat kind and its kind-specific fields, matching the editor's element type.
    #[serde(flatten)]
    pub kind: ElementKind,
    pub rect: crate::model::Rect,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "default_style")]
    pub style: ElementStyle,
    #[serde(default)]
    pub text: String,
    /// An existing picture in the project, used when duplicating a layer.
    #[serde(default)]
    pub image: Option<Uuid>,
    /// A key from the reply's `images` list. The picture is made when the provider supports it.
    #[serde(default)]
    pub image_request: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "ElementLayout::free")]
    pub layout: ElementLayout,
    #[serde(default)]
    pub children: Vec<ElementDraft>,
}

fn default_true() -> bool {
    true
}

/// A patch to one existing element in a requested layer's subtree.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementUpdate {
    pub element_id: Uuid,
    #[serde(default)]
    pub rect: Option<crate::model::Rect>,
    #[serde(default)]
    pub rotation: Option<f64>,
    #[serde(default)]
    pub style: Option<ElementStyle>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub layout: Option<ElementLayout>,
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub locked: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub input_type: Option<crate::elements::InputType>,
    #[serde(default)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    /// Change only this linked size, leaving the other sizes as they are.
    #[serde(default)]
    pub size_id: Option<Uuid>,
}

impl ElementUpdate {
    pub(crate) fn patch(self) -> ElementPatch {
        ElementPatch {
            id: self.element_id,
            rect: self.rect,
            rotation: self.rotation,
            style: self.style,
            text: self.text,
            image: None,
            url: self.url.map(Some),
            layout: self.layout,
            visible: self.visible,
            locked: self.locked,
            name: self.name.map(Some),
            linked_name: None,
            input_type: self.input_type,
            options: self.options,
            language: self.language,
            source: self.source,
            picture_data_uri: None,
            size_id: self.size_id,
            all_sizes: false,
            dismissed: None,
        }
    }
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ElementOperation {
    /// Edit a scoped boundary; null leaves a property unchanged.
    SetPage {
        page_id: Uuid,
        name: Option<String>,
        path: Option<String>,
        background: Option<String>,
    },
    /// Replace the requested layer's children with a new element tree.
    ReplaceChildren { children: Vec<ElementDraft> },
    /// Plain-language behaviour compiled to JavaScript at export; null targets the request boundary.
    SetBehaviour {
        element_id: Option<Uuid>,
        behaviour: String,
    },
    /// Change existing elements in the layer's subtree.
    Update { patches: Vec<ElementUpdate> },
    /// Add one new element under an existing box in the layer's subtree.
    Insert {
        parent_id: Uuid,
        #[serde(default)]
        index: Option<usize>,
        element: Box<ElementDraft>,
    },
    /// Remove one element from the layer's subtree.
    Remove { element_id: Uuid },
    /// Add a new layer at the root of a target boundary, inside the request's area when it has
    /// one. This is how an area over empty space inside a boundary fills it.
    InsertInBoundary {
        page_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_id: Option<Uuid>,
        element: Box<ElementDraft>,
    },
    /// Add a reference item outside every boundary, inside the request's area when it has one.
    AddReference { item: ReferenceDraft },
}

/// A reference item the AI may add outside every boundary.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceDraft {
    pub rect: crate::model::Rect,
    pub kind: crate::model::ReferenceKind,
}

/// A picture the AI asks for. Generated when the provider supports it; otherwise a Box
/// placeholder carries the prompt as its text.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratedImage {
    pub key: String,
    pub prompt: String,
    #[serde(default = "default_image_size")]
    pub width: u32,
    #[serde(default = "default_image_size")]
    pub height: u32,
}

fn default_image_size() -> u32 {
    1024
}

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RequestStatus {
    Implemented,
    Conflict,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestOutcome {
    /// The request this outcome answers. Absent on older replies, which name only a layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
    /// The layer this outcome anchors on. Absent for a request that only targets boundaries,
    /// reference items, pointers or an empty area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_id: Option<Uuid>,
    pub status: RequestStatus,
    pub explanation: String,
    #[serde(default)]
    pub operations: Vec<ElementOperation>,
}

/// The simple action the AI found in a pointer comment. `exportOnly` is stored as no Play
/// action; everything else runs visually in Play.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ClassifiedAction {
    GoTo { boundary: Uuid },
    Show { element: Uuid },
    Hide { element: Uuid },
    Toggle { element: Uuid },
    Swap { element: Uuid, with_element: Uuid },
    ExportOnly,
}

impl ClassifiedAction {
    pub fn play_action(&self) -> Option<PlayAction> {
        match self {
            Self::GoTo { boundary } => Some(PlayAction::GoTo {
                boundary: *boundary,
            }),
            Self::Show { element } => Some(PlayAction::Show { element: *element }),
            Self::Hide { element } => Some(PlayAction::Hide { element: *element }),
            Self::Toggle { element } => Some(PlayAction::Toggle { element: *element }),
            Self::Swap {
                element,
                with_element,
            } => Some(PlayAction::Swap {
                element: *element,
                with_element: *with_element,
            }),
            Self::ExportOnly => None,
        }
    }
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointerClassification {
    pub pointer_id: Uuid,
    pub comment: String,
    pub action: ClassifiedAction,
}

/// Text checks shared by an AI reply and the generation notes stored from it.
pub(crate) fn validate_summary_and_notes(
    summary: &str,
    proposals: &[DecisionProposal],
    criticism: &[DesignCriticism],
) -> Result<()> {
    validate_text(summary, "Generation summary", MAX_PROMPT_CHARS, false)?;
    for proposal in proposals {
        validate_text(&proposal.topic, "Decision topic", 120, true)?;
        validate_text(&proposal.value, "Decision value", MAX_PROMPT_CHARS, true)?;
        validate_text(
            &proposal.rationale,
            "Decision rationale",
            MAX_EXPLANATION_CHARS,
            true,
        )?;
    }
    for criticism in criticism {
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
    Ok(())
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionProposal {
    pub topic: String,
    pub value: String,
    pub scope: Scope,
    pub rationale: String,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignCriticism {
    #[serde(default, deserialize_with = "crate::validation::required_nullable")]
    pub element_id: Option<Uuid>,
    pub message: String,
    pub suggestion: String,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationCandidate {
    pub job_id: Uuid,
    pub requirements_sha256: String,
    pub summary: String,
    pub requests: Vec<RequestOutcome>,
    #[serde(default)]
    pub images: Vec<GeneratedImage>,
    #[serde(default)]
    pub pointer_classifications: Vec<PointerClassification>,
    #[serde(default)]
    pub decision_proposals: Vec<DecisionProposal>,
    #[serde(default)]
    pub criticism: Vec<DesignCriticism>,
}

#[derive(Clone, Copy)]
pub struct GenerationExpectations<'a> {
    pub job_id: Uuid,
    pub sha256: &'a str,
    pub project: Option<&'a Project>,
}

impl GenerationCandidate {
    pub fn from_json(source: &str) -> GenerationResult<Self> {
        Self::ingest(source, None, &mut Vec::new())
    }

    /// Repair page-targeted layer patches before typed style parsing, which otherwise requires
    /// a complete layer style even when the model supplied only a page background fill.
    pub fn from_json_for_project(
        source: &str,
        snapshot: &RequirementsSnapshot,
        project: &Project,
        events: &mut Vec<ValidationEvent>,
    ) -> GenerationResult<Self> {
        Self::ingest(
            source,
            Some(GenerationExpectations {
                job_id: snapshot.job_id,
                sha256: &snapshot.sha256,
                project: Some(project),
            }),
            events,
        )
    }

    pub fn ingest(
        source: &str,
        expectations: Option<GenerationExpectations<'_>>,
        events: &mut Vec<ValidationEvent>,
    ) -> GenerationResult<Self> {
        ensure(
            source.len() <= MAX_GENERATION_BYTES,
            format!(
                "The generation result exceeds the {} byte response limit.",
                MAX_GENERATION_BYTES
            ),
        )?;
        let mut value: serde_json::Value = crate::json_parse::parse_provider_json(source)
            .map_err(|error| Error(format!("The generation result is invalid: {error}")))?;
        let project = expectations.and_then(|expected| expected.project);
        if let Some(expected) = expectations {
            crate::json_parse::fill_reply_echoes(&mut value, expected.job_id, expected.sha256)?;
            if let Some(project) = expected.project {
                normalize_raw_page_patches(&mut value, project, events);
            }
        }
        conform_to_candidate_schema(&mut value);
        drop_null_optional_draft_fields(&mut value);
        let mut candidate: Self = serde_json::from_value(value)
            .map_err(|error| Error(format!("The generation result is invalid: {error}")))?;
        candidate.validate_reply_bounds()?;
        if let Some(project) = project {
            candidate.normalize_boundary_anchors(project);
        }
        Ok(candidate)
    }

    /// Drop what a reply echoed from a boundary target. A boundary is named by its page, but a
    /// page is not a layer: a model that answers a boundary-only request with that page id still
    /// answered the request, so an anchor on the page becomes absent and an insert whose parent
    /// is the page becomes a boundary insert before validation and apply read them.
    pub fn normalize_boundary_anchors(&mut self, project: &Project) {
        for outcome in &mut self.requests {
            let boundary_pages: BTreeSet<Uuid> = outcome
                .request_id
                .and_then(|request_id| {
                    project
                        .requests
                        .iter()
                        .find(|request| request.id == request_id)
                })
                .map(|request| {
                    request
                        .targets
                        .iter()
                        .filter_map(|target| match target {
                            crate::model::RequestTarget::Boundary { page, .. } => Some(*page),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            if outcome
                .element_id
                .is_some_and(|id| boundary_pages.contains(&id))
            {
                outcome.element_id = None;
            }
            // `insert` names a parent layer, and a page is not one, so an insert into the
            // request's own boundary is what the model meant.
            outcome.operations = std::mem::take(&mut outcome.operations)
                .into_iter()
                .map(|operation| match operation {
                    ElementOperation::Insert {
                        parent_id, element, ..
                    } if boundary_pages.contains(&parent_id) => {
                        ElementOperation::InsertInBoundary {
                            page_id: parent_id,
                            size_id: None,
                            element,
                        }
                    }
                    other => other,
                })
                .collect();
        }
    }

    pub(crate) fn validate_bounds(&self) -> GenerationResult<()> {
        self.validate_reply_bounds()?;
        let image_keys = self
            .images
            .iter()
            .map(|image| image.key.to_lowercase())
            .collect();
        for request in &self.requests {
            validate_operations(&request.operations, &image_keys).map_err(|error| {
                GenerationValidationError {
                    kind: ValidationEventKind::Element,
                    element_id: request.element_id,
                    message: error.0,
                }
            })?;
        }
        Ok(())
    }

    fn validate_images(&self) -> Result<()> {
        let mut image_keys = BTreeSet::new();
        for image in &self.images {
            ensure(
                !image.key.trim().is_empty()
                    && image.key.chars().count() <= 120
                    && image_keys.insert(image.key.to_lowercase()),
                "Generated image keys must be short, nonempty and unique.",
            )?;
            validate_text(&image.prompt, "Image prompt", 4000, true)?;
            ensure(
                (16..=4096).contains(&image.width) && (16..=4096).contains(&image.height),
                "A generated image size must be between 16 and 4,096 pixels.",
            )?;
        }
        Ok(())
    }

    pub(crate) fn validate_reply_bounds(&self) -> GenerationResult<()> {
        ensure(
            !self.job_id.is_nil(),
            "The generation job identifier is required.",
        )?;
        ensure(
            crate::limits::is_sha256_hex(&self.requirements_sha256),
            "The requirements digest must be a SHA-256 hexadecimal digest.",
        )?;
        ensure(
            !self.requests.is_empty() && self.requests.len() <= MAX_GENERATION_REQUESTS,
            format!(
                "A generation result must account for between 1 and {MAX_GENERATION_REQUESTS} requests."
            ),
        )?;
        ensure(
            self.images.len() <= MAX_GENERATED_IMAGES,
            "The generation result exceeds the generated image limit.",
        )
        .map_err(|error| GenerationValidationError {
            kind: ValidationEventKind::Image,
            element_id: None,
            message: error.0,
        })?;
        ensure(
            self.pointer_classifications.len() <= MAX_POINTER_CLASSIFICATIONS,
            "The generation result exceeds the pointer classification limit.",
        )
        .map_err(|error| GenerationValidationError {
            kind: ValidationEventKind::Pointer,
            element_id: None,
            message: error.0,
        })?;
        ensure(
            self.decision_proposals.len() <= MAX_DECISION_PROPOSALS,
            "The generation result exceeds the decision proposal limit.",
        )?;
        ensure(
            self.criticism.len() <= MAX_DESIGN_CRITICISMS,
            "The generation result exceeds the design feedback limit.",
        )?;
        self.validate_images()
            .map_err(|error| GenerationValidationError {
                kind: ValidationEventKind::Image,
                element_id: None,
                message: error.0,
            })?;
        for request in &self.requests {
            validate_text(
                &request.explanation,
                "Request outcome explanation",
                MAX_EXPLANATION_CHARS,
                false,
            )?;
            ensure(
                request.operations.len() <= MAX_REQUEST_OPERATIONS,
                format!("A request supports at most {MAX_REQUEST_OPERATIONS} operations."),
            )?;
        }
        for classification in &self.pointer_classifications {
            validate_text(
                &classification.comment,
                "Pointer comment",
                MAX_PROMPT_CHARS,
                false,
            )?;
        }
        validate_summary_and_notes(&self.summary, &self.decision_proposals, &self.criticism)?;
        ensure(
            serde_json::to_vec(self)
                .map_err(|error| Error(error.to_string()))?
                .len()
                <= MAX_GENERATION_BYTES,
            format!(
                "The generation result exceeds the {} byte response limit.",
                MAX_GENERATION_BYTES
            ),
        )
        .map_err(GenerationValidationError::from)
    }

    pub(crate) fn normalize(
        &mut self,
        project: &Project,
        events: &mut Vec<ValidationEvent>,
    ) -> Result<()> {
        let mut value = serde_json::to_value(&*self).map_err(|error| Error(error.to_string()))?;
        normalize_raw_page_patches(&mut value, project, events);
        *self = serde_json::from_value(value).map_err(|error| Error(error.to_string()))?;
        self.normalize_boundary_anchors(project);
        Ok(())
    }

    /// Validate page edits first, then keep other valid operations in order, rolling back each
    /// failed operation atomically. Links resolve against the accepted page routes.
    pub fn retain_valid_operations(
        &mut self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
        events: &mut Vec<ValidationEvent>,
    ) -> GenerationResult<()> {
        self.validate_reply_bounds()?;
        let initial_events = events.len();
        self.normalize(project, events)?;
        let source: Vec<_> = self
            .requests
            .iter()
            .map(|outcome| outcome.operations.clone())
            .collect();
        let mut accepted = vec![Vec::new(); self.requests.len()];
        let mut envelope = self.clone();
        for outcome in &mut envelope.requests {
            outcome.operations.clear();
        }
        envelope.validate(snapshot, project)?;
        let image_keys = self
            .images
            .iter()
            .map(|image| image.key.to_lowercase())
            .collect();
        let mut copy = project.clone();
        copy.store_request_pictures(&snapshot.request_ids()?)?;
        let mut retained_count = 0;
        let fulfilled: BTreeSet<_> = self
            .requests
            .iter()
            .filter(|outcome| outcome.status == RequestStatus::Implemented)
            .filter_map(|outcome| pending_for(project, outcome).map(|pending| pending.id))
            .collect();
        for page_phase in [true, false] {
            for index in 0..self.requests.len() {
                let outcome = &self.requests[index];
                if outcome.status != RequestStatus::Implemented {
                    continue;
                }
                let pending = pending_for(project, outcome);
                let anchor_page = outcome
                    .element_id
                    .and_then(|id| project.element(id).map(|(page, _)| page.id));
                let scope = scope_for(project, pending, anchor_page);
                let mut sources = Vec::new();
                for (ordinal, operation) in
                    source[index].iter().enumerate().filter(|(_, operation)| {
                        matches!(operation, ElementOperation::SetPage { .. }) == page_phase
                    })
                {
                    let mut trial = copy.clone();
                    let mut trial_sources = sources.clone();
                    let result = validate_operations(std::slice::from_ref(operation), &image_keys)
                        .map_err(GenerationValidationError::from)
                        .and_then(|()| {
                            apply_operations_with_sources(
                                &mut trial,
                                &scope,
                                outcome.element_id,
                                std::slice::from_ref(operation),
                                OperationImages {
                                    requests: &self.images,
                                    generated: &BTreeMap::new(),
                                },
                                &mut trial_sources,
                            )
                        })
                        .and_then(|_| {
                            // Applying the candidate consumes these requests. Their old targets must
                            // not make an otherwise valid remove look like a broken project.
                            let requests = std::mem::take(&mut trial.requests);
                            trial.requests = requests
                                .iter()
                                .filter(|request| !fulfilled.contains(&request.id))
                                .cloned()
                                .collect();
                            let pointers = trial.pointers.clone();
                            let result = apply_pointer_classifications(
                                &mut trial,
                                &self.pointer_classifications,
                            )
                            .and_then(|()| trial.validate());
                            trial.pointers = pointers;
                            trial.requests = requests;
                            result.map_err(GenerationValidationError::from)
                        });
                    match result {
                        Ok(()) => {
                            copy = trial;
                            sources = trial_sources;
                            accepted[index].push(operation.clone());
                            retained_count += 1;
                        }
                        Err(error) => events.push(ValidationEvent {
                            kind: ValidationEventKind::Operation,
                            disposition: "dropped".into(),
                            element_id: error.element_id,
                            message: format!(
                                "Request {} operation {} ({}) skipped: {error}",
                                index + 1,
                                ordinal + 1,
                                operation_kind(operation)
                            ),
                        }),
                    }
                }
            }
        }
        for (outcome, operations) in self.requests.iter_mut().zip(accepted) {
            outcome.operations = operations;
        }
        ensure(
            events.len() == initial_events
                || retained_count > 0
                || !self.pointer_classifications.is_empty()
                || !self.decision_proposals.is_empty(),
            format!(
                "No valid operations remain. {}",
                events
                    .iter()
                    .map(|event| event.message.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        )?;
        self.validate(snapshot, project)?;
        for outcome in &self.requests {
            if outcome.status == RequestStatus::Implemented
                && let Some(pending) = pending_for(project, outcome)
            {
                let untouched = untouched_boundaries(project, pending, outcome);
                if !untouched.is_empty() {
                    events.push(ValidationEvent {
                        kind: ValidationEventKind::BoundaryCoverage,
                        disposition: "warning".into(),
                        element_id: None,
                        message: boundary_coverage_message(project, pending, &untouched),
                    });
                }
            }
        }
        Ok(())
    }

    /// Check the reply against the frozen request: identity, coverage, operation scope,
    /// pointer coverage and bounded text.
    pub fn validate(
        &self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
    ) -> GenerationResult<()> {
        let mut candidate = self.clone();
        candidate.validate_reply_bounds()?;
        candidate.normalize(project, &mut Vec::new())?;
        candidate.check(snapshot, project)
    }

    fn validate_pointer_classifications(&self, project: &Project) -> Result<()> {
        let classifying: BTreeSet<Uuid> = project
            .pointers
            .iter()
            .filter(|pointer| crate::pointers::pointer_needs_classifying(pointer))
            .map(|pointer| pointer.id)
            .collect();
        let mut classified = BTreeSet::new();
        for classification in &self.pointer_classifications {
            // Restating a pointer already classified for its comment changes nothing; apply skips it.
            if !classifying.contains(&classification.pointer_id)
                && project.pointers.iter().any(|pointer| {
                    pointer.id == classification.pointer_id
                        && pointer.comment == classification.comment
                })
            {
                continue;
            }
            ensure(
                classifying.contains(&classification.pointer_id)
                    && classified.insert(classification.pointer_id),
                "The generation result contains a duplicate or unrequested pointer classification.",
            )?;
            let pointer = project
                .pointers
                .iter()
                .find(|pointer| pointer.id == classification.pointer_id)
                .expect("a classifying pointer exists");
            ensure(
                pointer.comment == classification.comment,
                "A pointer classification does not match the current comment.",
            )?;
            validate_classified_action(project, &classification.action)?;
        }
        ensure(
            classified == classifying,
            "The generation result omitted a pointer comment to classify.",
        )?;
        Ok(())
    }

    fn check(&self, snapshot: &RequirementsSnapshot, project: &Project) -> GenerationResult<()> {
        self.validate_bounds()?;
        snapshot.validate_current(project)?;
        self.validate_identity(snapshot)?;
        if let Some(capability) = snapshot
            .capabilities
            .as_ref()
            .and_then(|caps| caps.image_generation.as_ref())
        {
            ensure(
                self.images.len() <= usize::from(capability.max_images),
                "The generation result asks for more pictures than this provider can make.",
            )
            .map_err(|error| GenerationValidationError {
                kind: ValidationEventKind::Image,
                element_id: None,
                message: error.0,
            })?;
        }
        let targets = snapshot.target_ids()?;
        let target_elements: BTreeSet<_> = targets.iter().map(|(_, element)| *element).collect();
        let request_ids = snapshot.request_ids()?;
        let mut answered_requests = BTreeSet::new();
        let mut answered_elements = BTreeSet::new();
        for outcome in &self.requests {
            if let Some(request_id) = outcome.request_id {
                ensure(
                    request_ids.contains(&request_id) && answered_requests.insert(request_id),
                    "The generation result contains a duplicate or unrequested request.",
                )?;
                let pending = project
                    .requests
                    .iter()
                    .find(|request| request.id == request_id)
                    .expect("a requested request exists");
                if let Some(element_id) = outcome.element_id {
                    let anchored = pending.targets.iter().any(|target| match target {
                        crate::model::RequestTarget::Element { id, .. } => *id == element_id,
                        _ => false,
                    });
                    ensure(
                        anchored || target_elements.contains(&element_id),
                        "The generation result answered a request with a layer it does not target.",
                    )?;
                }
            } else {
                let element_id = outcome.element_id.ok_or_else(|| {
                    Error("A generation outcome names neither a request nor a layer.".into())
                })?;
                ensure(
                    target_elements.contains(&element_id) && answered_elements.insert(element_id),
                    "The generation result contains a duplicate or unrequested layer.",
                )?;
            }
        }
        // An element target is answered either directly or by the request that names it.
        for element_id in &target_elements {
            let covered = answered_elements.contains(element_id)
                || project.requests.iter().any(|request| {
                    answered_requests.contains(&request.id)
                        && request.targets.iter().any(|target| match target {
                            crate::model::RequestTarget::Element { id, .. } => id == element_id,
                            _ => false,
                        })
                });
            ensure(covered, "The generation result omitted a requested layer.")?;
        }
        ensure(
            project
                .requests
                .iter()
                .filter(|request| request_ids.contains(&request.id))
                .all(|request| {
                    answered_requests.contains(&request.id)
                        || (request.targets.iter().any(|target| {
                            matches!(target, crate::model::RequestTarget::Element { .. })
                        }) && request.targets.iter().all(|target| match target {
                            crate::model::RequestTarget::Element { id, .. } => {
                                answered_elements.contains(id)
                            }
                            _ => false,
                        }))
                }),
            "The generation result omitted a requested request.",
        )?;
        for proposal in &self.decision_proposals {
            project.validate_scope(&proposal.scope)?;
        }
        for criticism in &self.criticism {
            ensure(
                criticism
                    .element_id
                    .is_none_or(|id| target_elements.contains(&id)),
                "Design feedback refers to an unrequested layer.",
            )?;
        }
        self.validate_pointer_classifications(project)
            .map_err(|error| GenerationValidationError {
                kind: ValidationEventKind::Pointer,
                element_id: None,
                message: error.0,
            })?;
        // Run the operations on a copy so in-scope edits, ids, images and positions are checked
        // by the same code that later applies them.
        let mut copy = project.clone();
        copy.store_request_pictures(&request_ids)?;
        let generated = BTreeMap::new();
        apply_page_operations(&mut copy, project, self)?;
        for outcome in &self.requests {
            if outcome.status == RequestStatus::Implemented {
                let page_id = outcome
                    .request_id
                    .and_then(|id| project.requests.iter().find(|request| request.id == id))
                    .and_then(|request| request.targets.iter().find_map(|target| target.page()))
                    .or_else(|| {
                        outcome.element_id.and_then(|id| {
                            targets
                                .iter()
                                .find(|(_, element)| *element == id)
                                .map(|(page, _)| *page)
                        })
                    });
                apply_request(&mut copy, project, page_id, outcome, self, &generated)?;
            }
        }
        apply_pointer_classifications(&mut copy, &self.pointer_classifications)?;
        copy.validate()?;
        Ok(())
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

    pub fn can_apply(&self) -> bool {
        self.requests
            .iter()
            .any(|request| request.status == RequestStatus::Implemented)
    }
}

fn validate_operations(
    operations: &[ElementOperation],
    image_keys: &BTreeSet<String>,
) -> Result<()> {
    for operation in operations {
        match operation {
            ElementOperation::SetPage {
                page_id,
                name,
                path,
                background,
            } => {
                ensure(!page_id.is_nil(), "A page edit needs an existing boundary.")?;
                ensure(
                    name.is_some() || path.is_some() || background.is_some(),
                    "A page edit needs at least one property.",
                )?;
                if let Some(name) = name {
                    validate_text(name.trim(), "Page name", 120, true)?;
                }
                if let Some(path) = path {
                    crate::model::validate_path(path)?;
                }
                if let Some(background) = background {
                    ensure(
                        crate::limits::is_hex_colour(background),
                        "Boundary background must be a hex colour.",
                    )?;
                }
            }
            ElementOperation::ReplaceChildren { children } => {
                for child in children {
                    validate_draft(child, image_keys)?;
                }
            }
            ElementOperation::SetBehaviour {
                element_id,
                behaviour,
            } => {
                ensure(
                    element_id.is_none_or(|id| !id.is_nil()),
                    "A behaviour needs an existing element id.",
                )?;
                crate::validation::validate_text(
                    behaviour,
                    "Pointer comment",
                    crate::limits::MAX_PROMPT_CHARS,
                    false,
                )?;
            }
            ElementOperation::Update { patches } => {
                ensure(!patches.is_empty(), "An update needs at least one patch.")?;
                for patch in patches {
                    ensure(
                        !patch.element_id.is_nil(),
                        "An update needs an existing element id.",
                    )?;
                }
            }
            ElementOperation::Insert { element, .. } => {
                validate_draft(element, image_keys)?;
            }
            ElementOperation::Remove { element_id } => ensure(
                !element_id.is_nil(),
                "A remove operation needs an existing element id.",
            )?,
            ElementOperation::InsertInBoundary {
                page_id, element, ..
            } => {
                ensure(
                    !page_id.is_nil(),
                    "A boundary insert needs an existing boundary.",
                )?;
                validate_draft(element, image_keys)?;
            }
            ElementOperation::AddReference { item } => {
                ensure(
                    !matches!(item.kind, crate::model::ReferenceKind::Image { .. }),
                    "A generated reference item cannot use picture bytes.",
                )?;
                let rect = item.rect;
                ensure(
                    [rect.x, rect.y, rect.width, rect.height]
                        .iter()
                        .all(|value| value.is_finite())
                        && rect.width >= 1.0
                        && rect.height >= 1.0,
                    "A reference item needs finite coordinates and a positive size.",
                )?;
            }
        }
    }
    Ok(())
}

fn validate_draft(draft: &ElementDraft, image_keys: &BTreeSet<String>) -> Result<()> {
    ensure(
        [
            draft.rect.x,
            draft.rect.y,
            draft.rect.width,
            draft.rect.height,
        ]
        .iter()
        .all(|value| value.is_finite())
            && draft.rect.width >= 0.0
            && draft.rect.height >= 0.0,
        "A new element needs finite coordinates and a non-negative size.",
    )?;
    if let Some(request) = &draft.image_request {
        ensure(
            image_keys.contains(&request.to_lowercase()),
            "An image element refers to an image request that is not in the reply.",
        )?;
        ensure(
            matches!(draft.kind, ElementKind::Image),
            "Only an image element asks for a generated picture.",
        )?;
    }
    match draft.kind {
        ElementKind::Image => ensure(
            draft.image.is_some() || draft.image_request.is_some(),
            "Choose a picture for every image element.",
        )?,
        _ => ensure(
            draft.image.is_none() && draft.image_request.is_none(),
            "Only an image element uses a picture.",
        )?,
    }
    ensure(
        draft.kind.holds_children() || draft.children.is_empty(),
        "Only a box or a group can hold children.",
    )?;
    for child in &draft.children {
        validate_draft(child, image_keys)?;
    }
    Ok(())
}

fn validate_classified_action(project: &Project, action: &ClassifiedAction) -> Result<()> {
    match action {
        ClassifiedAction::GoTo { boundary } => ensure(
            project.pages.iter().any(|page| page.id == *boundary),
            "A go-to pointer names a boundary that does not exist.",
        ),
        ClassifiedAction::Show { element }
        | ClassifiedAction::Hide { element }
        | ClassifiedAction::Toggle { element } => ensure(
            project.element(*element).is_some(),
            "A Play action names a layer that does not exist.",
        ),
        ClassifiedAction::Swap {
            element,
            with_element,
        } => ensure(
            project.element(*element).is_some()
                && project.element(*with_element).is_some()
                && element != with_element,
            "A swap action needs two different existing layers.",
        ),
        ClassifiedAction::ExportOnly => Ok(()),
    }
}

pub(crate) fn reject(element_id: Option<Uuid>, message: String) -> GenerationValidationError {
    GenerationValidationError {
        kind: ValidationEventKind::Element,
        element_id,
        message,
    }
}

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ValidationEventKind {
    Operation,
    BoundaryCoverage,
    PagePatch,
    UnreadableReply,
    TelemetryStorage,
    ReplyStructure,
    Image,
    Pointer,
    Element,
}

impl ValidationEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Operation => "operation",
            Self::BoundaryCoverage => "boundaryCoverage",
            Self::PagePatch => "pagePatch",
            Self::UnreadableReply => "unreadableReply",
            Self::TelemetryStorage => "telemetryStorage",
            Self::ReplyStructure => "replyStructure",
            Self::Image => "image",
            Self::Pointer => "pointer",
            Self::Element => "element",
        }
    }
}

impl std::fmt::Display for ValidationEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One observation about a reply: normalised, repaired or conflicted. Labels are fixed
/// vocabulary, never user text or selectors.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[ts(rename = "GenerationValidationEvent")]
#[serde(rename_all = "camelCase")]
pub struct ValidationEvent {
    pub kind: ValidationEventKind,
    pub disposition: String,
    pub element_id: Option<Uuid>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct GenerationValidationError {
    pub kind: ValidationEventKind,
    pub element_id: Option<Uuid>,
    pub message: String,
}
impl std::fmt::Display for GenerationValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for GenerationValidationError {}
impl From<Error> for GenerationValidationError {
    fn from(error: Error) -> Self {
        Self {
            kind: ValidationEventKind::ReplyStructure,
            element_id: None,
            message: error.0,
        }
    }
}
impl From<GenerationValidationError> for Error {
    fn from(error: GenerationValidationError) -> Self {
        Self(error.message)
    }
}
pub(crate) type GenerationResult<T> = std::result::Result<T, GenerationValidationError>;
