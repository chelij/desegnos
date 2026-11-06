use crate::{
    Element, ElementKind, ElementLayout, ElementMeta, ElementPatch, ElementStyle, Error, Page,
    PlayAction, Project, RequestRecord, RequirementsSnapshot, Result, Scope, digest, ensure,
    model::validate_text,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const MAX_GENERATION_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_REQUIREMENTS_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_GENERATION_REQUESTS: usize = 512;
pub const MAX_GENERATED_IMAGES: usize = 8;
pub const MAX_POINTER_CLASSIFICATIONS: usize = 1000;
pub const MAX_DECISION_PROPOSALS: usize = 128;
pub const MAX_DESIGN_CRITICISMS: usize = 512;
pub const MAX_APPLIED_JOBS: usize = 10_000;
pub(crate) const MAX_EXPLANATION_CHARS: usize = 4096;

fn default_style() -> ElementStyle {
    crate::empty_element(
        ElementKind::Box,
        crate::Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        },
    )
    .style
}

/// A new element the AI wants in a layer. The core assigns its id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ElementDraft {
    /// Flat kind and its kind-specific fields, matching the editor's element type.
    #[serde(flatten)]
    pub kind: ElementKind,
    pub rect: crate::Rect,
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementUpdate {
    pub element_id: Uuid,
    #[serde(default)]
    pub rect: Option<crate::Rect>,
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
    pub input_type: Option<crate::InputType>,
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
    fn patch(self) -> ElementPatch {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ElementOperation {
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceDraft {
    pub rect: crate::Rect,
    pub kind: crate::ReferenceKind,
}

/// A picture the AI asks for. Generated when the provider supports it; otherwise a Box
/// placeholder carries the prompt as its text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RequestStatus {
    Implemented,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointerClassification {
    pub pointer_id: Uuid,
    pub comment: String,
    pub action: ClassifiedAction,
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
    #[serde(default, deserialize_with = "crate::required_nullable")]
    pub element_id: Option<Uuid>,
    pub message: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

impl GenerationCandidate {
    pub fn from_json(source: &str) -> Result<Self> {
        ensure(
            source.len() <= MAX_GENERATION_BYTES,
            "The generation result exceeds the 8 MiB response limit.",
        )?;
        let mut value: serde_json::Value = crate::parse_provider_json(source)
            .map_err(|error| Error(format!("The generation result is invalid: {error}")))?;
        drop_null_optional_draft_fields(&mut value);
        conform_to_candidate_schema(&mut value);
        let candidate: Self = serde_json::from_value(value)
            .map_err(|error| Error(format!("The generation result is invalid: {error}")))?;
        candidate.validate_bounds()?;
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
                            crate::RequestTarget::Boundary { page, .. } => Some(*page),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            if outcome.element_id.is_some_and(|id| boundary_pages.contains(&id)) {
                outcome.element_id = None;
            }
            // `insert` names a parent layer, and a page is not one, so an insert into the
            // request's own boundary is what the model meant.
            outcome.operations = std::mem::take(&mut outcome.operations)
                .into_iter()
                .map(|operation| match operation {
                    ElementOperation::Insert {
                        parent_id,
                        element,
                        ..
                    } if boundary_pages.contains(&parent_id) => ElementOperation::InsertInBoundary {
                        page_id: parent_id,
                        size_id: None,
                        element,
                    },
                    // The same echo in a behaviour: null is how it targets the request boundary,
                    // and a page id is not a layer, so it means the same boundary behaviour.
                    ElementOperation::SetBehaviour {
                        element_id,
                        behaviour,
                    } if element_id.is_some_and(|id| boundary_pages.contains(&id)) => {
                        ElementOperation::SetBehaviour {
                            element_id: None,
                            behaviour,
                        }
                    }
                    other => other,
                })
                .collect();
        }
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
            !self.requests.is_empty() && self.requests.len() <= MAX_GENERATION_REQUESTS,
            "A generation result must account for between 1 and 512 requests.",
        )?;
        ensure(
            self.images.len() <= MAX_GENERATED_IMAGES,
            "The generation result exceeds the generated image limit.",
        )?;
        ensure(
            self.pointer_classifications.len() <= MAX_POINTER_CLASSIFICATIONS,
            "The generation result exceeds the pointer classification limit.",
        )?;
        ensure(
            self.decision_proposals.len() <= MAX_DECISION_PROPOSALS,
            "The generation result exceeds the decision proposal limit.",
        )?;
        ensure(
            self.criticism.len() <= MAX_DESIGN_CRITICISMS,
            "The generation result exceeds the design feedback limit.",
        )?;
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
        for request in &self.requests {
            validate_text(
                &request.explanation,
                "Request outcome explanation",
                MAX_EXPLANATION_CHARS,
                true,
            )?;
            ensure(
                request.operations.len() <= 2048,
                "A request supports at most 2,048 operations.",
            )?;
            validate_operations(&request.operations, &image_keys)?;
        }
        for classification in &self.pointer_classifications {
            validate_text(&classification.comment, "Pointer comment", 16_384, false)?;
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

    /// Check the reply against the frozen request: identity, coverage, operation scope,
    /// pointer coverage and bounded text.
    pub fn validate(&self, snapshot: &RequirementsSnapshot, project: &Project) -> Result<()> {
        self.check(snapshot, project, &mut None)
    }

    /// Which existing layer a rejected reply is about, when its failure names one.
    pub fn rejected_element(
        &self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
    ) -> Option<Uuid> {
        let mut rejected = None;
        let _ = self.check(snapshot, project, &mut rejected);
        rejected
    }

    fn check(
        &self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
        rejected: &mut Option<Uuid>,
    ) -> Result<()> {
        self.validate_bounds()?;
        snapshot.validate_current(project)?;
        self.validate_identity(snapshot)?;
        if let Some(capability) = snapshot
            .capabilities
            .as_ref()
            .and_then(|capabilities| capabilities.image_generation.as_ref())
        {
            ensure(
                self.images.len() <= usize::from(capability.max_images),
                "The generation result asks for more pictures than this provider can make.",
            )?;
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
                        crate::RequestTarget::Element { id, .. } => *id == element_id,
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
                            crate::RequestTarget::Element { id, .. } => id == element_id,
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
                        || (request
                            .targets
                            .iter()
                            .any(|target| matches!(target, crate::RequestTarget::Element { .. }))
                            && request.targets.iter().all(|target| match target {
                                crate::RequestTarget::Element { id, .. } => {
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
        let classifying: BTreeSet<Uuid> = project
            .pointers
            .iter()
            .filter(|pointer| crate::pointer_needs_classifying(pointer))
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
        // Run the operations on a copy so in-scope edits, ids, images and positions are checked
        // by the same code that later applies them.
        let mut copy = project.clone();
        copy.store_request_pictures(&request_ids)?;
        let generated = BTreeMap::new();
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
                apply_request(&mut copy, page_id, outcome, self, &generated, rejected)?;
            }
        }
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

    pub fn is_ready(&self) -> bool {
        !self.requests.is_empty()
            && self
                .requests
                .iter()
                .all(|request| request.status == RequestStatus::Implemented)
    }

    pub fn can_apply(&self, _snapshot: &RequirementsSnapshot) -> bool {
        self.requests
            .iter()
            .any(|request| request.status == RequestStatus::Implemented)
    }

    pub fn ensure_ready(&self) -> Result<()> {
        self.validate_bounds()?;
        if let Some(request) = self
            .requests
            .iter()
            .find(|request| request.status == RequestStatus::Conflict)
        {
            return Err(Error(format!(
                "Resolve the conflict for request {} before applying: {}",
                request
                    .request_id
                    .or(request.element_id)
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                request.explanation
            )));
        }
        ensure(
            self.is_ready(),
            "Resolve every request before applying this generation.",
        )
    }

    pub fn implemented_count(&self) -> usize {
        self.requests
            .iter()
            .filter(|request| request.status == RequestStatus::Implemented)
            .count()
    }
}

fn validate_operations(
    operations: &[ElementOperation],
    image_keys: &BTreeSet<String>,
) -> Result<()> {
    let mut new_ids = BTreeSet::new();
    for operation in operations {
        match operation {
            ElementOperation::ReplaceChildren { children } => {
                for child in children {
                    validate_draft(child, image_keys, &mut new_ids)?;
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
                crate::validate_text(behaviour, "Pointer comment", crate::MAX_PROMPT_CHARS, false)?;
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
                validate_draft(element, image_keys, &mut new_ids)?;
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
                validate_draft(element, image_keys, &mut new_ids)?;
            }
            ElementOperation::AddReference { item } => {
                ensure(
                    !matches!(item.kind, crate::ReferenceKind::Image { .. }),
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

fn validate_draft(
    draft: &ElementDraft,
    image_keys: &BTreeSet<String>,
    new_ids: &mut BTreeSet<String>,
) -> Result<()> {
    let _ = new_ids;
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
        validate_draft(child, image_keys, new_ids)?;
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

fn build_element(
    draft: &ElementDraft,
    generated: &BTreeMap<String, Uuid>,
    image_prompts: &BTreeMap<String, String>,
) -> Result<Element> {
    let requested_picture = draft
        .image_request
        .as_ref()
        .and_then(|key| generated.get(&key.to_lowercase()).copied());
    let mut placeholder_text = None;
    let mut kind = draft.kind.clone();
    if matches!(draft.kind, ElementKind::Image)
        && requested_picture.is_none()
        && draft.image.is_none()
    {
        // No picture generation available: keep the prompt visible in a Box instead.
        let prompt = draft
            .image_request
            .as_ref()
            .and_then(|key| image_prompts.get(&key.to_lowercase()))
            .cloned()
            .unwrap_or_else(|| "Picture".into());
        placeholder_text = Some(prompt);
        kind = ElementKind::Box;
    }
    let mut element = Element {
        id: Uuid::new_v4(),
        kind,
        name: draft.name.clone(),
        link: None,
        rect: draft.rect,
        rotation: draft.rotation,
        visible: draft.visible,
        locked: draft.locked,
        style: draft.style.clone(),
        text: placeholder_text
            .clone()
            .unwrap_or_else(|| draft.text.clone()),
        image: if matches!(draft.kind, ElementKind::Image) {
            requested_picture.or(draft.image)
        } else {
            None
        },
        url: draft.url.clone(),
        layout: draft.layout.clone(),
        overrides: BTreeMap::new(),
        children: vec![],
        meta: ElementMeta::default(),
    };
    if placeholder_text.is_none() {
        for child in &draft.children {
            element
                .children
                .push(build_element(child, generated, image_prompts)?);
        }
    }
    Ok(element)
}

fn element_page_and_parent(
    project: &Project,
    id: Uuid,
) -> Option<(Uuid, Option<Uuid>, crate::Rect)> {
    for page in &project.pages {
        let elements = crate::resolve_elements(project, &page.elements);
        if let Some((parent, rect)) = find_parent_rect(&elements, id, None, page_rect(page)) {
            return Some((page.id, parent, rect));
        }
    }
    None
}

fn page_rect(page: &Page) -> crate::Rect {
    let base = page.base_size();
    crate::Rect {
        x: 0.0,
        y: 0.0,
        width: base.width as f64,
        height: base.height as f64,
    }
}

fn find_parent_rect(
    elements: &[Element],
    id: Uuid,
    parent: Option<Uuid>,
    parent_rect: crate::Rect,
) -> Option<(Option<Uuid>, crate::Rect)> {
    for element in elements {
        if element.id == id {
            return Some((parent, parent_rect));
        }
        if let Some(found) = find_parent_rect(&element.children, id, Some(element.id), element.rect)
        {
            return Some(found);
        }
    }
    None
}

/// How far outside a box a rectangle may sit before it counts as leaving the box.
const FIT_TOLERANCE: f64 = 0.5;

fn fits(parent: &crate::Rect, child: &crate::Rect) -> bool {
    child.x >= -FIT_TOLERANCE
        && child.y >= -FIT_TOLERANCE
        && child.x + child.width <= parent.width + FIT_TOLERANCE
        && child.y + child.height <= parent.height + FIT_TOLERANCE
}

/// How a rejected layer is named in a message: its name when it has one, its kind, and its box.
fn describe(name: Option<&str>, kind: &ElementKind, rect: &crate::Rect) -> String {
    let label = match name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => format!("{name} ({})", kind.label()),
        None => kind.label().to_string(),
    };
    format!(
        "{label} at {},{} sized {}x{}",
        rect.x, rect.y, rect.width, rect.height
    )
}

fn describe_element(element: &Element) -> String {
    describe(element.name.as_deref(), &element.kind, &element.rect)
}

/// Record the layer a rejection is about, so it reaches the validation event.
fn reject(rejected: &mut Option<Uuid>, element: Option<Uuid>, message: String) -> Error {
    if element.is_some() {
        *rejected = element;
    }
    Error(message)
}

/// What one reply outcome may change. Element targets grant their subtree plus what their
/// change pointers grant; a boundary target grants every layer inside it; a change pointer target
/// grants what that pointer points at. Reference pointers grant reads only.
struct RequestScope {
    behaviour_page: Option<Uuid>,
    allowed: BTreeSet<Uuid>,
    boundary_pages: BTreeSet<Uuid>,
    area: Option<crate::RequestArea>,
}

fn scope_for(
    project: &Project,
    request: Option<&crate::PendingRequest>,
    anchor_page: Option<Uuid>,
) -> RequestScope {
    let mut allowed = BTreeSet::new();
    let mut boundary_pages = BTreeSet::new();
    if let Some(request) = request {
        for target in &request.targets {
            match target {
                crate::RequestTarget::Element { page, id, .. } => {
                    let (elements, pages) = project.editable_element_ids(*page, *id);
                    allowed.extend(elements);
                    boundary_pages.extend(pages);
                }
                crate::RequestTarget::Boundary { page, .. } => {
                    boundary_pages.insert(*page);
                    if let Some(page) = project.pages.iter().find(|item| item.id == *page) {
                        for element in &page.elements {
                            allowed.extend(project.element_subtree_ids(element.id));
                            let (_, pages) = project.editable_element_ids(page.id, element.id);
                            boundary_pages.extend(pages);
                        }
                    }
                }
                crate::RequestTarget::Pointer { id } => {
                    if let Some(pointer) = project
                        .pointers
                        .iter()
                        .find(|pointer| pointer.id == *id)
                        .filter(|pointer| crate::pointer_grants_change(pointer))
                    {
                        match &pointer.to {
                            crate::PointerTarget::Element {
                                page_id,
                                element_id,
                                ..
                            } => {
                                allowed.extend(project.element_subtree_ids(*element_id));
                                let (_, pages) =
                                    project.editable_element_ids(*page_id, *element_id);
                                boundary_pages.extend(pages);
                            }
                            crate::PointerTarget::Boundary { page_id, .. } => {
                                boundary_pages.insert(*page_id);
                            }
                            crate::PointerTarget::Reference { .. }
                            | crate::PointerTarget::Url { .. } => {}
                        }
                    }
                }
                crate::RequestTarget::Reference { .. } => {}
            }
        }
    } else if let Some(page_id) = anchor_page {
        boundary_pages.insert(page_id);
    }
    let mut request_pages = BTreeSet::new();
    if let Some(request) = request {
        request_pages.extend(request.targets.iter().filter_map(|target| target.page()));
        request_pages.extend(request.area.as_ref().and_then(|area| area.page));
    } else {
        request_pages.extend(anchor_page);
    }
    RequestScope {
        behaviour_page: if request_pages.len() == 1 {
            request_pages.first().copied()
        } else {
            None
        },
        allowed,
        boundary_pages,
        area: request.and_then(|request| request.area.clone()),
    }
}

fn shape_contains(shape: &crate::RequestShape, point: crate::RequestPoint) -> bool {
    match shape {
        crate::RequestShape::Rect { rect } => {
            point.x >= rect.x
                && point.x <= rect.x + rect.width
                && point.y >= rect.y
                && point.y <= rect.y + rect.height
        }
        crate::RequestShape::Ellipse { rect } => {
            let rx = rect.width / 2.0;
            let ry = rect.height / 2.0;
            if rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let dx = (point.x - (rect.x + rx)) / rx;
            let dy = (point.y - (rect.y + ry)) / ry;
            dx * dx + dy * dy <= 1.0
        }
        crate::RequestShape::Polygon { points } | crate::RequestShape::Freehand { points } => {
            // Even-odd ray casting, with boundary points counted as inside.
            let mut inside = false;
            let mut previous = points.last().expect("a shape has points");
            for current in points {
                let on_edge = point_segment_distance(point, previous, current) <= 0.5;
                if on_edge {
                    return true;
                }
                if (current.y > point.y) != (previous.y > point.y) {
                    let x = (previous.x - current.x) * (point.y - current.y)
                        / (previous.y - current.y)
                        + current.x;
                    if point.x < x {
                        inside = !inside;
                    }
                }
                previous = current;
            }
            inside
        }
    }
}

fn point_segment_distance(
    point: crate::RequestPoint,
    start: &crate::RequestPoint,
    end: &crate::RequestPoint,
) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = dx * dx + dy * dy;
    if length == 0.0 {
        return ((point.x - start.x).powi(2) + (point.y - start.y).powi(2)).sqrt();
    }
    let t = (((point.x - start.x) * dx + (point.y - start.y) * dy) / length).clamp(0.0, 1.0);
    ((point.x - (start.x + t * dx)).powi(2) + (point.y - (start.y + t * dy)).powi(2)).sqrt()
}

/// A requested area is the real outline; new content must sit inside it.
fn area_allows(area: &crate::RequestArea, rect: &crate::Rect) -> bool {
    let corners = [
        (rect.x, rect.y),
        (rect.x + rect.width, rect.y),
        (rect.x + rect.width, rect.y + rect.height),
        (rect.x, rect.y + rect.height),
        (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
    ];
    corners
        .iter()
        .all(|(x, y)| shape_contains(&area.shape, crate::RequestPoint { x: *x, y: *y }))
}

fn collect_history_ids(element: &Element, ids: &mut BTreeSet<Uuid>) {
    ids.insert(element.id);
    for child in &element.children {
        collect_history_ids(child, ids);
    }
}

fn apply_operations(
    project: &mut Project,
    scope: &RequestScope,
    anchor: Option<Uuid>,
    operations: &[ElementOperation],
    candidate: &GenerationCandidate,
    generated: &BTreeMap<String, Uuid>,
    rejected: &mut Option<Uuid>,
) -> Result<BTreeSet<Uuid>> {
    let images: BTreeMap<String, String> = candidate
        .images
        .iter()
        .map(|image| (image.key.to_lowercase(), image.prompt.clone()))
        .collect();
    let mut touched = BTreeSet::new();
    // Behaviours this reply already wrote; later ones on the same source add to them.
    let mut behaviour_sources: Vec<crate::PointerSource> = Vec::new();
    for operation in operations {
        match operation {
            ElementOperation::ReplaceChildren { children } => {
                let target = anchor
                    .filter(|id| scope.allowed.contains(id))
                    .ok_or_else(|| {
                        Error("The generation result changed a layer outside its request.".into())
                    })?;
                let mut built = Vec::with_capacity(children.len());
                for child in children {
                    let element = build_element(child, generated, &images)?;
                    built.push(element);
                }
                let parent_rect = project
                    .element(target)
                    .map(|(_, element)| element.rect)
                    .expect("the anchor exists");
                for element in &built {
                    if !fits(&parent_rect, &element.rect) {
                        return Err(reject(
                            rejected,
                            Some(target),
                            format!(
                                "A generated element must stay inside its frame: {} does not fit its {}x{} frame.",
                                describe_element(element),
                                parent_rect.width,
                                parent_rect.height
                            ),
                        ));
                    }
                }
                for element in &built {
                    collect_history_ids(element, &mut touched);
                }
                touched.insert(target);
                project
                    .editable_element_mut(target)
                    .expect("the target exists")
                    .children = built;
            }
            ElementOperation::SetBehaviour {
                element_id,
                behaviour,
            } => {
                let (from, to) = if let Some(id) = element_id {
                    ensure(
                        scope.allowed.contains(id),
                        "The generation result changed a layer outside its request.",
                    )?;
                    let (page, _) = project
                        .element(*id)
                        .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                    touched.insert(*id);
                    (
                        crate::PointerSource::Element {
                            page_id: page.id,
                            element_id: *id,
                            size_id: None,
                        },
                        crate::PointerTarget::Element {
                            page_id: page.id,
                            element_id: *id,
                            size_id: None,
                        },
                    )
                } else {
                    let page_id = scope.behaviour_page.ok_or_else(|| {
                        Error("A boundary behaviour needs a request in one boundary.".into())
                    })?;
                    (
                        crate::PointerSource::Boundary {
                            page_id,
                            size_id: None,
                        },
                        crate::PointerTarget::Boundary {
                            page_id,
                            size_id: None,
                        },
                    )
                };
                let existing = project
                    .pointers
                    .iter()
                    .position(|p| p.is_behaviour() && p.from == from);
                let seen = behaviour_sources.contains(&from);
                if !behaviour.is_empty() && !seen {
                    behaviour_sources.push(from.clone());
                }
                if behaviour.is_empty() {
                    if let Some(index) = existing {
                        project.pointers.remove(index);
                    }
                } else if let Some(index) = existing {
                    let comment = &mut project.pointers[index].comment;
                    if seen {
                        comment.push_str("\n\n");
                        comment.push_str(behaviour);
                    } else {
                        *comment = behaviour.clone();
                    }
                } else {
                    project.pointers.push(crate::Pointer {
                        id: Uuid::new_v4(),
                        from,
                        to,
                        comment: behaviour.clone(),
                        kind: crate::PointerKind::Change,
                        wiring: None,
                        play: None,
                        play_comment: String::new(),
                    });
                }
            }
            ElementOperation::Update { patches } => {
                for patch in patches {
                    ensure(
                        scope.allowed.contains(&patch.element_id),
                        "The generation result changed a layer outside its request.",
                    )?;
                    let (owner, current) = project
                        .element(patch.element_id)
                        .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                    let owner_page = owner.id;
                    let sizes: BTreeSet<Uuid> = project
                        .page(owner_page)?
                        .sizes
                        .iter()
                        .map(|size| size.id)
                        .collect();
                    ensure(
                        patch.size_id.is_none_or(|size_id| sizes.contains(&size_id)),
                        "The generation result changed a size this boundary does not have.",
                    )?;
                    let (_, _, parent_rect) = element_page_and_parent(project, patch.element_id)
                        .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                    if let Some(rect) = &patch.rect
                        && !fits(&parent_rect, rect)
                    {
                        return Err(reject(
                            rejected,
                            Some(patch.element_id),
                            format!(
                                "A generated element must stay inside its parent: {} does not fit its {}x{} parent.",
                                describe(current.name.as_deref(), &current.kind, rect),
                                parent_rect.width,
                                parent_rect.height
                            ),
                        ));
                    }
                    let target = crate::elements::size_target_for_patch(
                        project,
                        project.page(owner_page)?,
                        &patch.clone().patch(),
                    )?;
                    let element = project
                        .editable_element_mut(patch.element_id)
                        .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                    crate::elements::apply_sized_patch(element, patch.clone().patch(), target)?;
                    touched.insert(patch.element_id);
                }
            }
            ElementOperation::Insert {
                parent_id,
                index,
                element,
            } => {
                ensure(
                    scope.allowed.contains(parent_id),
                    "The generation result inserted outside its request.",
                )?;
                let parent_rect = project
                    .element(*parent_id)
                    .map(|(_, parent)| parent.rect)
                    .ok_or_else(|| Error("This box no longer exists.".into()))?;
                let built = build_element(element, generated, &images)?;
                if !fits(&parent_rect, &built.rect) {
                    return Err(reject(
                        rejected,
                        Some(*parent_id),
                        format!(
                            "A generated element must stay inside its parent: {} does not fit its {}x{} parent.",
                            describe_element(&built),
                            parent_rect.width,
                            parent_rect.height
                        ),
                    ));
                }
                let parent = project
                    .editable_element_mut(*parent_id)
                    .ok_or_else(|| Error("This box no longer exists.".into()))?;
                ensure(
                    parent.kind.holds_children(),
                    "Only a box or a group can hold elements.",
                )?;
                let at = index
                    .unwrap_or(parent.children.len())
                    .min(parent.children.len());
                collect_history_ids(&built, &mut touched);
                parent.children.insert(at, built);
            }
            ElementOperation::Remove { element_id } => {
                ensure(
                    scope.allowed.contains(element_id),
                    "The generation result removed a layer outside its request.",
                )?;
                ensure(
                    project.remove_editable_element(*element_id),
                    "This layer no longer exists.",
                )?;
            }
            ElementOperation::InsertInBoundary {
                page_id,
                size_id,
                element,
            } => {
                let boundary = scope
                    .area
                    .as_ref()
                    .filter(|area| area.page == Some(*page_id))
                    .is_some()
                    || scope.boundary_pages.contains(page_id);
                ensure(
                    boundary,
                    "The generation result inserted into a boundary outside its request.",
                )?;
                let page = project.page(*page_id)?;
                ensure(
                    size_id.is_none_or(|size| page.sizes.iter().any(|item| item.id == size)),
                    "The generation result inserted into a size this boundary does not have.",
                )?;
                let output = page.output;
                let size = match size_id {
                    Some(size) => page.sizes.iter().find(|item| item.id == *size),
                    None => page.sizes.first(),
                }
                .ok_or_else(|| Error("This boundary has no size.".into()))?
                .clone();
                let built = build_element(element, generated, &images)?;
                let right = built.rect.x + built.rect.width;
                let bottom = built.rect.y + built.rect.height;
                let outside_width =
                    built.rect.x < 0.0 || right > f64::from(size.width) + FIT_TOLERANCE;
                let outside_height = bottom > f64::from(size.height) + FIT_TOLERANCE;
                // A website page scrolls: a layer below the boundary grows it instead of being
                // rejected. Every other output keeps its fixed height.
                let grows = outside_height
                    && !outside_width
                    && built.rect.y >= 0.0
                    && output == crate::OutputType::Webpage;
                if outside_width || built.rect.y < 0.0 || (outside_height && !grows) {
                    return Err(reject(
                        rejected,
                        None,
                        format!(
                            "A generated element must stay inside its boundary: {} does not fit its {}x{} boundary.",
                            describe_element(&built),
                            size.width,
                            size.height
                        ),
                    ));
                }
                if grows {
                    // Grow to the lowest edge of the new layer, rounded up, within a size's limit.
                    let height = bottom.ceil();
                    ensure(
                        height <= f64::from(crate::MAX_VIEWPORT_SIDE),
                        format!(
                            "A generated element must stay inside its boundary: {} needs a boundary {} pixels high, and a boundary may be at most {} pixels high.",
                            describe_element(&built),
                            height,
                            crate::MAX_VIEWPORT_SIDE
                        ),
                    )?;
                    project
                        .page_mut(*page_id)?
                        .sizes
                        .iter_mut()
                        .find(|item| item.id == size.id)
                        .expect("the size exists")
                        .height = height as u32;
                }
                if let Some(area) = &scope.area
                    && !area_allows(area, &built.rect)
                {
                    return Err(reject(
                        rejected,
                        None,
                        format!(
                            "A generated element must stay inside the request area: {} is outside it.",
                            describe_element(&built)
                        ),
                    ));
                }
                collect_history_ids(&built, &mut touched);
                project.page_mut(*page_id)?.elements.push(built);
            }
            ElementOperation::AddReference { item } => {
                let area = scope
                    .area
                    .as_ref()
                    .filter(|area| area.page.is_none())
                    .ok_or_else(|| {
                        Error("Reference items may only be added through an area outside every boundary.".into())
                    })?;
                if !area_allows(area, &item.rect) {
                    return Err(reject(
                        rejected,
                        None,
                        format!(
                            "A generated reference item must stay inside the request area: the item at {},{} sized {}x{} is outside it.",
                            item.rect.x, item.rect.y, item.rect.width, item.rect.height
                        ),
                    ));
                }
                ensure(
                    !matches!(item.kind, crate::ReferenceKind::Image { .. }),
                    "A generated reference item cannot use picture bytes.",
                )?;
                project.add_reference_item(
                    crate::ReferenceItem {
                        id: Uuid::new_v4(),
                        rect: item.rect,
                        kind: item.kind.clone(),
                    },
                    None,
                )?;
            }
        }
    }
    Ok(touched)
}

fn record_request_history(
    project: &mut Project,
    ids: &BTreeSet<Uuid>,
    request: &crate::PendingRequest,
    outcome: &str,
    job_id: Uuid,
) {
    for id in ids {
        let Some(element) = project.element_mut(*id) else {
            continue;
        };
        if element.meta.requests.len() >= crate::MAX_ELEMENT_REQUESTS {
            element.meta.requests.remove(0);
        }
        element.meta.requests.push(RequestRecord {
            id: Uuid::new_v4(),
            prompt: request.text.clone(),
            attachments: request.attachments.clone(),
            created_at: request.requested_at.clone(),
            outcome: Some(outcome.to_string()),
            job_id: Some(job_id.to_string()),
        });
    }
}

fn apply_request(
    project: &mut Project,
    page_id: Option<Uuid>,
    request: &RequestOutcome,
    candidate: &GenerationCandidate,
    generated: &BTreeMap<String, Uuid>,
    rejected: &mut Option<Uuid>,
) -> Result<()> {
    if !matches!(request.status, RequestStatus::Implemented) {
        return Ok(());
    }
    // The reply may name the pending request; older replies name one layer. Either way the
    // request is read before it is changed so its text can become layer history.
    let pending = request
        .request_id
        .and_then(|id| project.requests.iter().find(|item| item.id == id))
        .or_else(|| {
            request.element_id.and_then(|id| {
                project.requests.iter().find(|item| {
                    item.targets.iter().any(|target| match target {
                        crate::RequestTarget::Element { id: target_id, .. } => *target_id == id,
                        _ => false,
                    })
                })
            })
        })
        .cloned();
    let anchor_page = page_id.or_else(|| {
        request
            .element_id
            .and_then(|id| project.element(id).map(|(page, _)| page.id))
    });
    let scope = scope_for(project, pending.as_ref(), anchor_page);
    let touched = apply_operations(
        project,
        &scope,
        request.element_id,
        &request.operations,
        candidate,
        generated,
        rejected,
    )?;
    if let Some(pending) = &pending {
        let outcome = if request.explanation.trim().is_empty() {
            candidate.summary.clone()
        } else {
            request.explanation.clone()
        };
        record_request_history(project, &touched, pending, &outcome, candidate.job_id);
        // A pending request is fulfilled once its reply is applied.
        project.requests.retain(|item| item.id != pending.id);
    }
    Ok(())
}

/// Apply one validated generation reply as one project state: element operations, pointer
/// classifications, accepted decisions and the applied job id.
pub fn apply_generation_output(
    project: &Project,
    snapshot: &RequirementsSnapshot,
    candidate: &GenerationCandidate,
    accepted_decision_indices: &[usize],
    generated_pictures: &BTreeMap<String, String>,
) -> Result<Project> {
    candidate.validate(snapshot, project)?;
    let mut next = project.clone();
    next.store_request_pictures(&snapshot.request_ids()?)?;
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or_else(|| Error("Project revision limit reached.".into()))?;
    ensure(
        !next.applied_job_ids.contains(&candidate.job_id),
        "This generation job has already been applied.",
    )?;
    let mut generated = BTreeMap::new();
    for image in &candidate.images {
        let Some(uri) = generated_pictures.get(&image.key.to_lowercase()) else {
            continue;
        };
        let picture = crate::project_assets::store_picture_uri(uri)?;
        next.pictures
            .insert(picture, std::sync::Arc::from(uri.as_str()));
        generated.insert(image.key.to_lowercase(), picture);
    }
    let targets = snapshot.target_ids()?;
    for request in &candidate.requests {
        if request.status != RequestStatus::Implemented {
            continue;
        }
        let page_id = request
            .request_id
            .and_then(|id| next.requests.iter().find(|pending| pending.id == id))
            .and_then(|pending| pending.targets.iter().find_map(|target| target.page()))
            .or_else(|| {
                request
                    .element_id
                    .and_then(|id| next.element(id).map(|(page, _)| page.id))
            });
        apply_request(
            &mut next, page_id, request, candidate, &generated, &mut None,
        )?;
    }
    let _ = targets;
    for classification in &candidate.pointer_classifications {
        let pointer = next
            .pointers
            .iter_mut()
            .find(|pointer| pointer.id == classification.pointer_id)
            .ok_or_else(|| Error("This pointer no longer exists.".into()))?;
        if !crate::pointer_needs_classifying(pointer) {
            continue;
        }
        ensure(
            pointer.comment == classification.comment,
            "A pointer comment changed before this result was applied.",
        )?;
        pointer.play = classification.action.play_action();
        pointer.play_comment = classification.comment.clone();
    }
    for index in accepted_decision_indices {
        let proposal = candidate
            .decision_proposals
            .get(*index)
            .ok_or_else(|| Error("An accepted decision is not in this result.".into()))?;
        if let Some(existing) = next.decisions.iter_mut().find(|decision| {
            decision.scope == proposal.scope
                && decision.topic.eq_ignore_ascii_case(proposal.topic.trim())
        }) {
            existing.value = proposal.value.trim().into();
            existing.accepted_at_revision = next.revision;
        } else {
            next.decisions.push(crate::Decision {
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
    if let Some(dossier) = &mut next.latest_generation
        && dossier.job_id == candidate.job_id
    {
        dossier.application_status = crate::GenerationApplicationStatus::Applied;
        dossier.applied_at_revision = Some(next.revision);
        dossier.accepted_decision_indices = accepted_decision_indices.to_vec();
    }
    // AI layers arrive without names; give them the same numbered default names as hand edits.
    let mut used = crate::elements::used_default_names(&next);
    for page in &mut next.pages {
        crate::elements::assign_default_names(&mut page.elements, &mut used);
    }
    next.validate()?;
    Ok(next)
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

    fn scopes_overlap(&self, first: &Scope, second: &Scope) -> bool {
        match (first, second) {
            (Scope::Project, _) | (_, Scope::Project) => true,
            (Scope::Page(first), Scope::Page(second))
            | (Scope::Element(first), Scope::Element(second)) => first == second,
            (Scope::Page(page), Scope::Element(element))
            | (Scope::Element(element), Scope::Page(page)) => self
                .element(*element)
                .is_some_and(|(owner, _)| owner.id == *page),
        }
    }
}

fn same_topic(first: &str, second: &str) -> bool {
    first.trim().to_lowercase() == second.trim().to_lowercase()
}

fn uuid_schema() -> serde_json::Value {
    serde_json::json!({"type":"string","pattern":"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"})
}

fn nullable<T: serde::Serialize>(schema: T) -> serde_json::Value {
    serde_json::json!({"anyOf":[schema,{"type":"null"}]})
}

/// Strict structured output requires every draft property, so an omitted optional kind field
/// arrives as null. The flattened element kind cannot tell a null member from an unknown one,
/// so remove those nulls before deserialising; every other optional field is an `Option` or has
/// a default, both of which already read null as absent.
fn drop_null_optional_draft_fields(value: &mut serde_json::Value) {
    const NULL_AS_ABSENT: [&str; 4] = ["inputType", "options", "language", "source"];
    match value {
        serde_json::Value::Object(object) => {
            object.retain(|key, member| {
                !(member.is_null() && NULL_AS_ABSENT.contains(&key.as_str()))
            });
            for member in object.values_mut() {
                drop_null_optional_draft_fields(member);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                drop_null_optional_draft_fields(item);
            }
        }
        _ => {}
    }
}

/// Repair the small, different drift a provider without an enforced schema introduces, guided by
/// the same schema the request carried. The pass only removes what the schema forbids and reads a
/// null as empty where the schema requires a string and allows one; it never invents a field,
/// never coerces another type, and leaves a value that already conforms untouched. Whatever it
/// cannot repair stays as it is, for deserialisation and validation to report.
fn conform_to_candidate_schema(value: &mut serde_json::Value) {
    let schema = candidate_schema();
    conform_value(value, &schema, &schema);
}

fn conform_value(
    value: &mut serde_json::Value,
    schema: &serde_json::Value,
    root: &serde_json::Value,
) {
    let schema = dereference(schema, root);
    if let Some(branches) = union_branches(schema) {
        if let Some(branch) = matching_branch(value, branches, root) {
            conform_value(value, branch, root);
        }
        return;
    }
    if value.is_null() {
        if schema_accepts_null(schema) {
            return;
        }
        if matches!(schema.get("type"), Some(serde_json::Value::String(kind)) if kind == "string")
            && empty_string_is_allowed(schema)
        {
            *value = serde_json::Value::String(String::new());
        }
        return;
    }
    if !value_fits_type(value, schema) {
        return;
    }
    match value {
        serde_json::Value::Object(object) => {
            let properties = schema
                .get("properties")
                .and_then(serde_json::Value::as_object);
            if schema
                .get("additionalProperties")
                .and_then(serde_json::Value::as_bool)
                == Some(false)
                && let Some(properties) = properties
            {
                object.retain(|key, _| properties.contains_key(key));
            }
            for (key, member) in object.iter_mut() {
                if let Some(property) = properties.and_then(|properties| properties.get(key)) {
                    conform_value(member, property, root);
                }
            }
        }
        serde_json::Value::Array(items) => {
            if let Some(item_schema) = schema.get("items") {
                for item in items {
                    conform_value(item, item_schema, root);
                }
            }
        }
        _ => {}
    }
}

fn dereference<'a>(
    mut schema: &'a serde_json::Value,
    root: &'a serde_json::Value,
) -> &'a serde_json::Value {
    // The candidate schema only uses flat root-relative definitions, and element drafts refer to
    // themselves through them, so one lookup per level is enough.
    while let Some(name) = schema
        .get("$ref")
        .and_then(serde_json::Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
    {
        let Some(target) = root.get("$defs").and_then(|defs| defs.get(name)) else {
            break;
        };
        schema = target;
    }
    schema
}

fn union_branches(schema: &serde_json::Value) -> Option<&[serde_json::Value]> {
    ["anyOf", "oneOf"].iter().find_map(|keyword| {
        schema
            .get(*keyword)
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
    })
}

/// Choose the union branch a value belongs to, or none when the choice is ambiguous so the value
/// is left untouched. A null belongs to a null branch, a value that fits exactly one branch type
/// belongs to that branch, and object branches are told apart by their single-valued enum or const
/// discriminator, such as `kind`.
fn matching_branch<'a>(
    value: &serde_json::Value,
    branches: &'a [serde_json::Value],
    root: &serde_json::Value,
) -> Option<&'a serde_json::Value> {
    if value.is_null() {
        if branches
            .iter()
            .any(|branch| schema_accepts_null(dereference(branch, root)))
        {
            return None;
        }
        return single(branches);
    }
    let typed: Vec<&serde_json::Value> = branches
        .iter()
        .filter(|branch| value_fits_type(value, dereference(branch, root)))
        .collect();
    if let [only] = typed.as_slice() {
        return Some(only);
    }
    if value.is_object() {
        let discriminated: Vec<&serde_json::Value> = typed
            .iter()
            .copied()
            .filter(|branch| discriminator_matches(value, dereference(branch, root)))
            .collect();
        if let [only] = discriminated.as_slice() {
            return Some(only);
        }
    }
    None
}

fn single(branches: &[serde_json::Value]) -> Option<&serde_json::Value> {
    match branches {
        [only] => Some(only),
        _ => None,
    }
}

fn schema_accepts_null(schema: &serde_json::Value) -> bool {
    match schema.get("type") {
        None => true,
        Some(serde_json::Value::String(kind)) => kind == "null",
        Some(serde_json::Value::Array(kinds)) => {
            kinds.iter().any(|kind| kind.as_str() == Some("null"))
        }
        _ => false,
    }
}

fn value_fits_type(value: &serde_json::Value, schema: &serde_json::Value) -> bool {
    fn fits(value: &serde_json::Value, kind: &str) -> bool {
        match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        }
    }
    match schema.get("type") {
        None => true,
        Some(serde_json::Value::String(kind)) => fits(value, kind),
        Some(serde_json::Value::Array(kinds)) => kinds
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(|kind| fits(value, kind)),
        _ => true,
    }
}

/// An empty string is only substituted where the schema does not rule it out: a const, an enum
/// without `""`, a positive minimum length or any pattern can all forbid it.
fn empty_string_is_allowed(schema: &serde_json::Value) -> bool {
    if let Some(constant) = schema.get("const") {
        return constant.as_str() == Some("");
    }
    if let Some(values) = schema.get("enum").and_then(serde_json::Value::as_array) {
        return values.iter().any(|value| value.as_str() == Some(""));
    }
    if schema.get("minLength").and_then(serde_json::Value::as_u64) > Some(0) {
        return false;
    }
    schema.get("pattern").is_none()
}

fn discriminator_matches(value: &serde_json::Value, branch: &serde_json::Value) -> bool {
    let (Some(object), Some(properties)) = (
        value.as_object(),
        branch
            .get("properties")
            .and_then(serde_json::Value::as_object),
    ) else {
        return false;
    };
    let mut declared = 0;
    let mut matched = 0;
    for (key, property) in properties {
        let Some(expected) = discriminator_value(property) else {
            continue;
        };
        declared += 1;
        if object.get(key) == Some(expected) {
            matched += 1;
        }
    }
    declared > 0 && declared == matched
}

fn discriminator_value(schema: &serde_json::Value) -> Option<&serde_json::Value> {
    if let Some(constant) = schema.get("const") {
        return Some(constant);
    }
    match schema
        .get("enum")
        .and_then(serde_json::Value::as_array)?
        .as_slice()
    {
        [only] => Some(only),
        _ => None,
    }
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
    let uuid = uuid_schema();
    let colour = json!({"type":"string","description":"#rgb, #rrggbb or #rrggbbaa; transparent is allowed."});
    let stroke = object(
        json!({"color":nullable(colour.clone()),"width":{"type":"number","minimum":0,"maximum":256}}),
    );
    let shadow = object(
        json!({"x":{"type":"number"},"y":{"type":"number"},"blur":{"type":"number","minimum":0,"maximum":4096},"color":nullable(colour.clone())}),
    );
    let text_style = object(json!({
        "fontFamily":text(200),"size":{"type":"number","minimum":1,"maximum":512},
        "weight":{"type":"integer","minimum":1,"maximum":1000},"italic":{"type":"boolean"},
        "lineHeight":{"type":"number","minimum":0,"maximum":10},
        "letterSpacing":{"type":"number","minimum":-100,"maximum":100},
        "color":colour.clone(),"align":{"type":"string","enum":["left","center","right"]}
    }));
    let style = object(json!({
        "fill":nullable(colour),"stroke":stroke,"radius":{"type":"number","minimum":0},
        "opacity":{"type":"number","minimum":0,"maximum":1},"shadow":shadow,"textStyle":text_style
    }));
    let rect = object(
        json!({"x":{"type":"number"},"y":{"type":"number"},"width":{"type":"number","minimum":0},"height":{"type":"number","minimum":0}}),
    );
    let layout = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["free"]}})),
        object(json!({"kind":{"type":"string","enum":["stack"]},"direction":{"type":"string","enum":["row","column"]},"gap":{"type":"number"},"padding":{"type":"number"},"align":{"type":"string","enum":["start","center","end","stretch"]},"justify":{"type":"string","enum":["start","center","end","spaceBetween"]},"wrap":{"type":"boolean"}})),
        object(json!({"kind":{"type":"string","enum":["grid"]},"columns":{"type":"integer","minimum":1,"maximum":64},"gap":{"type":"number"},"padding":{"type":"number"}}))
    ]});
    let kind = json!({"type":"string","enum":["box","group","text","ellipse","line","image","button","link","input","textarea","select","checkbox","radio","video","audio","embed","code"]});
    let input_type = json!({"type":"string","enum":["text","email","password","number","search","tel","url","date","time","color","range","file"]});
    // Strict structured output requires every property of every object, so a field the reply may
    // omit is required and reads null as omitted. The element kind fields stay nullable because
    // the flattened kind decides which of them applies.
    let draft = object(json!({
        "kind":kind,"rect":rect,"name":nullable(text(120)),
        "rotation":{"type":"number"},"visible":{"type":"boolean"},"locked":{"type":"boolean"},
        "style":style,"text":text(16_384),"image":nullable(uuid.clone()),
        "imageRequest":nullable(text(120)),"url":nullable(text(2048)),"layout":layout,
        "inputType":nullable(input_type.clone()),
        "options":nullable(json!({"type":"array","minItems":1,"maxItems":100,"items":text(120)})),
        "language":nullable(text(40)),"source":nullable(text(200_000)),
        "children":{"type":"array","maxItems":512,"items":{"$ref":"#/$defs/element"}}
    }));
    let update = object(json!({
        "elementId":uuid.clone(),"rect":nullable(rect.clone()),"rotation":nullable(json!({"type":"number"})),
        "style":nullable(style),"text":nullable(text(16_384)),"url":nullable(text(2048)),
        "layout":nullable(layout),"visible":nullable(json!({"type":"boolean"})),
        "locked":nullable(json!({"type":"boolean"})),"name":nullable(text(120)),
        "inputType":nullable(input_type),
        "options":nullable(json!({"type":"array","maxItems":100,"items":text(120)})),
        "language":nullable(text(40)),"source":nullable(text(200_000)),
        "sizeId":{"anyOf":[uuid.clone(),{"type":"null"}],"description":"Change only this linked size of the boundary; null keeps the base values every size inherits."}
    }));
    let reference_kind = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["note"]},"text":text(16_384)})),
        object(json!({"kind":{"type":"string","enum":["link"]},"url":text(2048),"title":text(120)})),
        object(json!({"kind":{"type":"string","enum":["swatch"]},"color":text(32)}))
    ]});
    let operation = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["replaceChildren"]},"children":{"type":"array","maxItems":512,"items":{"$ref":"#/$defs/element"}}})),
        object(json!({"kind":{"type":"string","enum":["update"]},"patches":{"type":"array","minItems":1,"maxItems":2048,"items":update}})),
        object(json!({"kind":{"type":"string","enum":["insert"]},"parentId":uuid.clone(),"index":nullable(json!({"type":"integer","minimum":0})),"element":{"$ref":"#/$defs/element"}})),
        object(json!({"kind":{"type":"string","enum":["remove"]},"elementId":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["setBehaviour"]},"elementId":nullable(uuid.clone()),"behaviour":text(crate::MAX_PROMPT_CHARS)})),
        object(json!({"kind":{"type":"string","enum":["insertInBoundary"]},"pageId":uuid.clone(),"sizeId":nullable(uuid.clone()),"element":{"$ref":"#/$defs/element"}})),
        object(json!({"kind":{"type":"string","enum":["addReference"]},"item":object(json!({"rect":rect,"kind":reference_kind}))}))
    ]});
    let action = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["goTo"]},"boundary":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["show"]},"element":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["hide"]},"element":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["toggle"]},"element":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["swap"]},"element":uuid.clone(),"withElement":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["exportOnly"]}}))
    ]});
    let scope = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["project"]}})),
        object(json!({"kind":{"type":"string","enum":["page"]},"id":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["element"]},"id":uuid.clone()}))
    ]});
    let schema = object(json!({
        "jobId":uuid,
        "requirementsSha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
        "summary":text(16_384),
        "requests":array(object(json!({
            "requestId":nullable(uuid.clone()),
            "elementId":nullable(uuid.clone()),
            "status":{"type":"string","enum":["implemented","conflict"]},
            "explanation":text(MAX_EXPLANATION_CHARS),
            "operations":array(operation,2048)
        })),MAX_GENERATION_REQUESTS),
        "images":array(object(json!({
            "key":text(120),"prompt":text(4000),
            "width":{"type":"integer","minimum":16,"maximum":4096},
            "height":{"type":"integer","minimum":16,"maximum":4096}
        })),MAX_GENERATED_IMAGES),
        "pointerClassifications":array(object(json!({
            "pointerId":uuid.clone(),"comment":text(16_384),"action":action
        })),MAX_POINTER_CLASSIFICATIONS),
        "decisionProposals":array(object(json!({
            "topic":text(120),"value":text(16_384),"scope":scope,"rationale":text(MAX_EXPLANATION_CHARS)
        })),MAX_DECISION_PROPOSALS),
        "criticism":array(object(json!({
            "elementId":nullable(uuid.clone()),
            "message":text(MAX_EXPLANATION_CHARS),
            "suggestion":text(MAX_EXPLANATION_CHARS)
        })),MAX_DESIGN_CRITICISMS)
    }));
    let mut schema = schema;
    schema["$defs"] = json!({"element": draft});
    schema
}

/// One observation about a reply: normalised, repaired or conflicted. Labels are fixed
/// vocabulary, never user text or selectors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationEvent {
    pub kind: String,
    pub disposition: String,
    pub element_id: Option<Uuid>,
    pub message: String,
}

/// A stable label for a validation failure message.
pub fn validation_failure_kind(message: &str) -> &'static str {
    let message = message.to_ascii_lowercase();
    if message.contains("image") {
        "image"
    } else if message.contains("pointer") {
        "pointer"
    } else if message.contains("layer") || message.contains("element") {
        "element"
    } else {
        "replyStructure"
    }
}

/// The provider reply schema for a frozen request.
pub fn result_schema(_snapshot: &RequirementsSnapshot) -> serde_json::Value {
    let mut schema = candidate_schema();
    schema["properties"]["requests"]["description"] = serde_json::json!(
        "Deliver every requested layer exactly once. A conflict must not withhold other ready layers."
    );
    schema["properties"]["pointerClassifications"]["description"] = serde_json::json!(
        "Classify every pointer listed under pointer comments to classify, and no other. Copy its current comment exactly. Leave empty when none are listed."
    );
    schema
}

#[cfg(test)]
mod conformance_tests {
    use super::*;
    use serde_json::{Value, json};

    const JOB: &str = "3d1f9c56-0000-4000-8000-000000000000";
    const SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const PAGE: &str = "3d1f9c56-0000-4000-8000-000000000001";

    fn candidate(operations: Value) -> Value {
        json!({
            "jobId": JOB,
            "requirementsSha256": SHA,
            "summary": "Built the page",
            "requests": [{
                "requestId": null,
                "elementId": null,
                "status": "implemented",
                "explanation": "Built the page",
                "operations": operations
            }],
            "images": [],
            "pointerClassifications": [],
            "decisionProposals": [],
            "criticism": []
        })
    }

    fn conformed(mut value: Value) -> Value {
        conform_to_candidate_schema(&mut value);
        value
    }

    fn element() -> Value {
        json!({
            "kind": "text",
            "rect": {"x": 0.0, "y": 0.0, "width": 400.0, "height": 40.0},
            "text": "Hero"
        })
    }

    #[test]
    fn unknown_keys_are_dropped_at_depth() {
        let mut operation = json!({
            "kind": "insert",
            "parentId": PAGE,
            "index": null,
            "element": element()
        });
        operation["element"]["style"] = json!({"opacity": 1.0, "borderWidth": 2});
        operation["position"] = json!(3);
        let value = conformed(candidate(json!([operation])));
        let operation = &value["requests"][0]["operations"][0];
        assert_eq!(operation["kind"], json!("insert"), "{operation}");
        assert!(operation.get("position").is_none(), "{operation}");
        assert!(
            operation["element"]["style"].get("borderWidth").is_none(),
            "{operation}"
        );
        assert_eq!(operation["element"]["style"]["opacity"], json!(1.0));
        assert_eq!(operation["element"]["text"], json!("Hero"));
    }

    #[test]
    fn a_boundary_insert_with_a_stray_index_loses_it() {
        let value = conformed(candidate(json!([{
            "kind": "insertInBoundary",
            "pageId": PAGE,
            "sizeId": null,
            "index": 0,
            "element": element()
        }])));
        let operation = &value["requests"][0]["operations"][0];
        assert!(operation.get("index").is_none(), "{operation}");
        assert_eq!(operation["element"]["text"], json!("Hero"));
    }

    #[test]
    fn a_null_string_becomes_empty_where_the_schema_allows_it() {
        let mut value = candidate(json!([]));
        value["summary"] = Value::Null;
        value["requests"][0]["explanation"] = Value::Null;
        let value = conformed(value);
        assert_eq!(value["summary"], json!(""));
        assert_eq!(value["requests"][0]["explanation"], json!(""));
    }

    #[test]
    fn a_null_enum_member_is_left_for_validation() {
        let mut value = candidate(json!([]));
        value["requests"][0]["status"] = Value::Null;
        let value = conformed(value);
        assert_eq!(value["requests"][0]["status"], Value::Null);
        let error = serde_json::from_value::<GenerationCandidate>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid type: null"), "{error}");
    }

    #[test]
    fn a_conforming_candidate_is_byte_for_byte_unchanged() {
        let mut drafted = element();
        drafted["style"] = json!({"opacity": 1.0, "textStyle": {"size": 32.0, "align": "center"}});
        drafted["children"] = json!([]);
        let value = candidate(json!([
            {"kind": "insertInBoundary", "pageId": PAGE, "sizeId": null, "element": drafted},
            {"kind": "remove", "elementId": PAGE}
        ]));
        let before = serde_json::to_string(&value).unwrap();
        let after = conformed(value);
        assert_eq!(serde_json::to_string(&after).unwrap(), before);
    }

    #[test]
    fn a_type_mismatch_other_than_null_still_fails() {
        let mut value = candidate(json!([]));
        value["summary"] = json!(42);
        let value = conformed(value);
        assert_eq!(value["summary"], json!(42));
        let error = serde_json::from_value::<GenerationCandidate>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid type: integer"), "{error}");
    }
}
