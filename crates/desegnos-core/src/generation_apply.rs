use crate::generation::{
    ElementDraft, ElementOperation, GeneratedImage, GenerationCandidate, GenerationResult,
    PointerClassification, RequestOutcome, RequestStatus, ValidationEvent, ValidationEventKind,
    reject,
};
use crate::generation_scope::{RequestScope, area_allows, scope_for};
use crate::limits::MAX_EXPLANATION_CHARS;
use crate::{
    Error, Result, elements::Element, elements::ElementKind, elements::RequestRecord, model::Page,
    model::Project, model::Scope, requirements::RequirementsSnapshot, validation::ensure,
    validation::validate_text,
};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use uuid::Uuid;
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
        name: draft.name.clone(),
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
        ..crate::element_edit::empty_element(kind, draft.rect)
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
) -> Option<(Uuid, Option<Uuid>, crate::model::Rect)> {
    for page in &project.pages {
        let elements = crate::elements::resolve_elements(project, &page.elements);
        if let Some((parent, rect)) = find_parent_rect(&elements, id, None, page_rect(page)) {
            return Some((page.id, parent, rect));
        }
    }
    None
}

fn page_rect(page: &Page) -> crate::model::Rect {
    let base = page.base_size();
    crate::model::Rect {
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
    parent_rect: crate::model::Rect,
) -> Option<(Option<Uuid>, crate::model::Rect)> {
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

const FIT_TOLERANCE: f64 = 0.5;

fn fits(parent: &crate::model::Rect, child: &crate::model::Rect) -> bool {
    child.x >= -FIT_TOLERANCE
        && child.y >= -FIT_TOLERANCE
        && child.x + child.width <= parent.width + FIT_TOLERANCE
        && child.y + child.height <= parent.height + FIT_TOLERANCE
}

/// How a rejected layer is named in a message: its name when it has one, its kind, and its box.
fn describe(name: Option<&str>, kind: &ElementKind, rect: &crate::model::Rect) -> String {
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
) -> GenerationResult<BTreeSet<Uuid>> {
    apply_operations_with_sources(
        project,
        scope,
        anchor,
        operations,
        OperationImages {
            requests: &candidate.images,
            generated,
        },
        &mut Vec::new(),
    )
}

pub(crate) struct OperationImages<'a> {
    pub(crate) requests: &'a [GeneratedImage],
    pub(crate) generated: &'a BTreeMap<String, Uuid>,
}

pub(crate) fn apply_operations_with_sources(
    project: &mut Project,
    scope: &RequestScope,
    anchor: Option<Uuid>,
    operations: &[ElementOperation],
    image_sources: OperationImages<'_>,
    behaviour_sources: &mut Vec<crate::model::PointerSource>,
) -> GenerationResult<BTreeSet<Uuid>> {
    let images: BTreeMap<String, String> = image_sources
        .requests
        .iter()
        .map(|image| (image.key.to_lowercase(), image.prompt.clone()))
        .collect();
    let mut touched = BTreeSet::new();
    for operation in operations {
        let mut operation = operation.clone();
        resolve_operation_links(&mut operation, project, scope);
        match &operation {
            ElementOperation::SetPage {
                page_id,
                name,
                path,
                background,
            } => {
                ensure(
                    scope.boundary_pages.contains(page_id),
                    "The generation result changed a boundary outside its request.",
                )?;
                let page = project.page(*page_id)?;
                if let Some(path) = path {
                    crate::model::validate_path(path)?;
                    ensure(
                        !project.pages.iter().any(|page| {
                            page.id != *page_id && page.path.eq_ignore_ascii_case(path)
                        }),
                        "Two pages cannot use the same public path.",
                    )?;
                    if *page_id == project.home_page_id && page.path == "/" && path != "/" {
                        let text = scope.request_text.to_lowercase();
                        ensure(
                            text.contains(&path.to_lowercase())
                                && ["route", "path", "url"]
                                    .iter()
                                    .any(|word| text.contains(word)),
                            "The home page keeps / unless the request explicitly names its new route, path or URL.",
                        )?;
                    }
                }
                let page = project.page_mut(*page_id)?;
                if let Some(name) = name {
                    validate_text(name.trim(), "Page name", 120, true)?;
                    page.name = name.trim().into();
                }
                if let Some(path) = path {
                    page.path = path.clone();
                }
                if let Some(background) = background {
                    ensure(
                        crate::limits::is_hex_colour(background),
                        "Boundary background must be a hex colour.",
                    )?;
                    page.background = crate::model::normalized_swatch(background);
                }
            }
            ElementOperation::ReplaceChildren { children } => {
                let target = anchor
                    .filter(|id| scope.allowed.contains(id))
                    .ok_or_else(|| {
                        Error("The generation result changed a layer outside its request.".into())
                    })?;
                let mut built = Vec::with_capacity(children.len());
                for child in children {
                    let element = build_element(child, image_sources.generated, &images)?;
                    built.push(element);
                }
                let parent_rect = project
                    .element(target)
                    .map(|(_, element)| element.rect)
                    .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                for element in &built {
                    if !fits(&parent_rect, &element.rect) {
                        return Err(reject(
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
                let boundary = match element_id {
                    Some(id) if scope.boundary_pages.contains(id) => Some(*id),
                    None => scope.behaviour_page,
                    _ => None,
                };
                let (from, to) = if let Some(page_id) = boundary {
                    project.page(page_id)?;
                    (
                        crate::model::PointerSource::Boundary {
                            page_id,
                            size_id: None,
                        },
                        crate::model::PointerTarget::Boundary {
                            page_id,
                            size_id: None,
                        },
                    )
                } else if let Some(id) = element_id {
                    ensure(
                        scope.allowed.contains(id),
                        "The generation result changed a layer outside its request.",
                    )?;
                    let (page, _) = project
                        .element(*id)
                        .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                    touched.insert(*id);
                    (
                        crate::model::PointerSource::Element {
                            page_id: page.id,
                            element_id: *id,
                            size_id: None,
                        },
                        crate::model::PointerTarget::Element {
                            page_id: page.id,
                            element_id: *id,
                            size_id: None,
                        },
                    )
                } else {
                    return Err(Error(
                        "A boundary behaviour needs a request in one boundary.".into(),
                    )
                    .into());
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
                    project.pointers.push(crate::model::Pointer {
                        id: Uuid::new_v4(),
                        from,
                        to,
                        comment: behaviour.clone(),
                        kind: crate::model::PointerKind::Change,
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
                            Some(patch.element_id),
                            format!(
                                "A generated element must stay inside its parent: {} does not fit its {}x{} parent.",
                                describe(current.name.as_deref(), &current.kind, rect),
                                parent_rect.width,
                                parent_rect.height
                            ),
                        ));
                    }
                    let target = crate::element_edit::size_target_for_patch(
                        project,
                        project.page(owner_page)?,
                        &patch.clone().patch(),
                    )?;
                    let element = project
                        .editable_element_mut(patch.element_id)
                        .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                    crate::element_edit::apply_sized_patch(element, patch.clone().patch(), target)?;
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
                let built = build_element(element, image_sources.generated, &images)?;
                if !fits(&parent_rect, &built.rect) {
                    return Err(reject(
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
                let built = build_element(element, image_sources.generated, &images)?;
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
                    && output == crate::model::OutputType::Webpage;
                if outside_width || built.rect.y < 0.0 || (outside_height && !grows) {
                    return Err(reject(
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
                        height <= f64::from(crate::limits::MAX_VIEWPORT_SIDE),
                        format!(
                            "A generated element must stay inside its boundary: {} needs a boundary {} pixels high, and a boundary may be at most {} pixels high.",
                            describe_element(&built),
                            height,
                            crate::limits::MAX_VIEWPORT_SIDE
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
                        None,
                        format!(
                            "A generated reference item must stay inside the request area: the item at {},{} sized {}x{} is outside it.",
                            item.rect.x, item.rect.y, item.rect.width, item.rect.height
                        ),
                    ));
                }
                ensure(
                    !matches!(item.kind, crate::model::ReferenceKind::Image { .. }),
                    "A generated reference item cannot use picture bytes.",
                )?;
                project.add_reference_item(
                    crate::model::ReferenceItem {
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
    request: &crate::model::PendingRequest,
    outcome: &str,
    job_id: Uuid,
) {
    for id in ids {
        let Some(element) = project.element_mut(*id) else {
            continue;
        };
        if element.meta.requests.len() >= crate::limits::MAX_ELEMENT_REQUESTS {
            element.meta.requests.remove(0);
        }
        element.meta.requests.push(RequestRecord {
            id: Uuid::new_v4(),
            prompt: request.text.clone(),
            attachments: request.attachments.clone(),
            created_at: request.requested_at.clone(),
            outcome: Some(outcome.chars().take(MAX_EXPLANATION_CHARS).collect()),
            job_id: Some(job_id.to_string()),
        });
    }
}

pub(crate) fn pending_for<'a>(
    project: &'a Project,
    outcome: &RequestOutcome,
) -> Option<&'a crate::model::PendingRequest> {
    outcome
        .request_id
        .and_then(|id| project.requests.iter().find(|item| item.id == id))
        .or_else(|| {
            outcome.element_id.and_then(|id| {
                project.requests.iter().find(|item| {
                    item.targets.iter().any(|target| {
                        matches!(target,
                crate::model::RequestTarget::Element { id: target_id, .. } if *target_id == id)
                    })
                })
            })
        })
}

pub(crate) fn operation_kind(operation: &ElementOperation) -> &'static str {
    match operation {
        ElementOperation::SetPage { .. } => "setPage",
        ElementOperation::ReplaceChildren { .. } => "replaceChildren",
        ElementOperation::SetBehaviour { .. } => "setBehaviour",
        ElementOperation::Update { .. } => "update",
        ElementOperation::Insert { .. } => "insert",
        ElementOperation::Remove { .. } => "remove",
        ElementOperation::InsertInBoundary { .. } => "insertInBoundary",
        ElementOperation::AddReference { .. } => "addReference",
    }
}

pub(crate) fn untouched_boundaries(
    project: &Project,
    pending: &crate::model::PendingRequest,
    outcome: &RequestOutcome,
) -> Vec<Uuid> {
    let scope = scope_for(project, Some(pending), None);
    let owner = |id| project.element(id).map(|(page, _)| page.id);
    let mut received = BTreeSet::new();
    for operation in &outcome.operations {
        match operation {
            ElementOperation::SetPage { page_id, .. }
            | ElementOperation::InsertInBoundary { page_id, .. } => {
                received.insert(*page_id);
            }
            ElementOperation::SetBehaviour { element_id, .. } => {
                received.extend(
                    element_id
                        .and_then(|id| {
                            if scope.boundary_pages.contains(&id) {
                                Some(id)
                            } else {
                                owner(id)
                            }
                        })
                        .or_else(|| {
                            element_id
                                .is_none()
                                .then_some(scope.behaviour_page)
                                .flatten()
                        }),
                );
            }
            ElementOperation::ReplaceChildren { .. } => {
                received.extend(outcome.element_id.and_then(owner));
            }
            ElementOperation::Update { patches } => {
                received.extend(patches.iter().filter_map(|patch| owner(patch.element_id)));
            }
            ElementOperation::Insert { parent_id, .. } => {
                received.extend(owner(*parent_id));
            }
            ElementOperation::Remove { element_id } => {
                received.extend(owner(*element_id));
            }
            ElementOperation::AddReference { .. } => {}
        }
    }
    pending
        .targets
        .iter()
        .filter_map(|target| match target {
            crate::model::RequestTarget::Boundary { page, .. }
                if !received.contains(page)
                    && project
                        .page(*page)
                        .is_ok_and(|page| page.elements.is_empty()) =>
            {
                Some(*page)
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn boundary_coverage_message(
    project: &Project,
    pending: &crate::model::PendingRequest,
    untouched: &[Uuid],
) -> String {
    let names = untouched
        .iter()
        .filter_map(|id| project.page(*id).ok())
        .map(|page| page.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if pending.boundary_retry_order.is_empty() {
        format!(
            "No operations received for {names}. Kept a request for only these boundaries; Generate again to fill them."
        )
    } else {
        format!(
            "No operations received again for {names}. Stopped retrying these boundaries; the request was not completed."
        )
    }
}

pub(crate) fn normalize_raw_page_patches(
    value: &mut serde_json::Value,
    project: &Project,
    events: &mut Vec<ValidationEvent>,
) {
    use serde_json::{Value, json};
    let Some(outcomes) = value.get_mut("requests").and_then(Value::as_array_mut) else {
        return;
    };
    for outcome in outcomes {
        let id = |key| {
            outcome
                .get(key)
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
        };
        let request_id = id("requestId");
        let anchor = id("elementId");
        let pending = project.requests.iter().find(|request| Some(request.id) == request_id)
            .or_else(|| anchor.and_then(|id| project.requests.iter().find(|request| request.targets.iter()
                .any(|target| matches!(target, crate::model::RequestTarget::Element { id: target, .. } if *target == id)))));
        let scope = scope_for(
            project,
            pending,
            anchor.and_then(|id| project.element(id).map(|(page, _)| page.id)),
        );
        let Some(operations) = outcome.get_mut("operations").and_then(Value::as_array_mut) else {
            continue;
        };
        let mut repaired = Vec::new();
        for mut operation in std::mem::take(operations) {
            if operation.get("kind").and_then(Value::as_str) != Some("update") {
                repaired.push(operation);
                continue;
            }
            let Some(patches) = operation.get_mut("patches").and_then(Value::as_array_mut) else {
                repaired.push(operation);
                continue;
            };
            let had_patches = !patches.is_empty();
            let mut layers = Vec::new();
            for patch in std::mem::take(patches) {
                let page_id = patch
                    .get("elementId")
                    .and_then(Value::as_str)
                    .and_then(|id| Uuid::parse_str(id).ok());
                let Some(page_id) = page_id.filter(|id| project.page(*id).is_ok()) else {
                    layers.push(patch);
                    continue;
                };
                let background = patch
                    .get("style")
                    .and_then(|style| style.get("fill"))
                    .and_then(Value::as_str)
                    .filter(|fill| crate::limits::is_hex_colour(fill));
                let recovered = background.is_some();
                repaired.push(json!({"kind":"setPage","pageId":page_id,"name":null,"path":null,"background":background}));
                if scope.boundary_pages.contains(&page_id) && recovered {
                    events.push(page_patch_event(project, page_id, true));
                }
            }
            *patches = layers;
            if !patches.is_empty() || !had_patches {
                repaired.push(operation);
            }
        }
        *operations = repaired;
    }
}

fn page_patch_event(project: &Project, page_id: Uuid, recovered: bool) -> ValidationEvent {
    ValidationEvent {
        kind: ValidationEventKind::PagePatch,
        disposition: if recovered { "warning" } else { "dropped" }.into(),
        element_id: None,
        message: format!(
            "{} {}. Other layer patch fields were ignored; use setPage for boundary properties.",
            if recovered {
                "Recovered the page background for"
            } else {
                "Skipped a layer patch on boundary"
            },
            project
                .page(page_id)
                .map(|page| page.name.as_str())
                .unwrap_or("page")
        ),
    }
}

pub(crate) fn apply_page_operations(
    project: &mut Project,
    original: &Project,
    candidate: &GenerationCandidate,
) -> GenerationResult<()> {
    for outcome in &candidate.requests {
        if outcome.status != RequestStatus::Implemented {
            continue;
        }
        let pending = pending_for(original, outcome);
        let anchor_page = outcome
            .element_id
            .and_then(|id| original.element(id).map(|(page, _)| page.id));
        let scope = scope_for(original, pending, anchor_page);
        let operations: Vec<_> = outcome
            .operations
            .iter()
            .filter(|operation| matches!(operation, ElementOperation::SetPage { .. }))
            .cloned()
            .collect();
        apply_operations(
            project,
            &scope,
            outcome.element_id,
            &operations,
            candidate,
            &BTreeMap::new(),
        )?;
    }
    Ok(())
}

fn resolve_operation_links(
    operation: &mut ElementOperation,
    project: &Project,
    scope: &RequestScope,
) {
    fn resolve(url: &mut Option<String>, project: &Project, scope: &RequestScope) {
        let Some(value) = url.as_deref().map(str::trim) else {
            return;
        };
        if value.is_empty() || value.contains(":") || value.starts_with("//") {
            return;
        }
        let key = value.trim_end_matches('/');
        let unique = |ids: Vec<Uuid>| {
            let ids: BTreeSet<_> = ids.into_iter().collect();
            if ids.len() == 1 {
                ids.first().copied()
            } else {
                None
            }
        };
        let current_path = unique(
            project
                .pages
                .iter()
                .filter(|page| page.path.trim_end_matches('/').eq_ignore_ascii_case(key))
                .map(|page| page.id)
                .collect(),
        );
        let names: Vec<_> = project
            .pages
            .iter()
            .filter(|page| page.name.eq_ignore_ascii_case(key.trim_start_matches('/')))
            .map(|page| page.id)
            .collect();
        if current_path.is_none() && names.len() > 1 {
            return;
        }
        let current_name = unique(names);
        let original = unique(
            scope
                .original_routes
                .iter()
                .filter(|(_, name, path)| {
                    path.trim_end_matches('/').eq_ignore_ascii_case(key)
                        || name.eq_ignore_ascii_case(key.trim_start_matches('/'))
                })
                .map(|(id, _, _)| *id)
                .collect(),
        );
        if let Some(page) = current_path
            .or(current_name)
            .or(original)
            .and_then(|id| project.page(id).ok())
        {
            *url = Some(page.path.clone());
        }
    }
    fn draft(element: &mut ElementDraft, project: &Project, scope: &RequestScope) {
        resolve(&mut element.url, project, scope);
        for child in &mut element.children {
            draft(child, project, scope);
        }
    }
    match operation {
        ElementOperation::ReplaceChildren { children } => {
            for child in children {
                draft(child, project, scope);
            }
        }
        ElementOperation::Insert { element, .. }
        | ElementOperation::InsertInBoundary { element, .. } => draft(element, project, scope),
        ElementOperation::Update { patches } => {
            for patch in patches {
                resolve(&mut patch.url, project, scope);
            }
        }
        _ => {}
    }
}

pub(crate) fn apply_request(
    project: &mut Project,
    original: &Project,
    page_id: Option<Uuid>,
    request: &RequestOutcome,
    candidate: &GenerationCandidate,
    generated: &BTreeMap<String, Uuid>,
) -> GenerationResult<()> {
    if !matches!(request.status, RequestStatus::Implemented) {
        return Ok(());
    }
    // The reply may name the pending request; older replies name one layer. Either way the
    // request is read before it is changed so its text can become layer history.
    let pending = pending_for(original, request).cloned();
    let untouched = pending
        .as_ref()
        .map(|pending| untouched_boundaries(original, pending, request))
        .unwrap_or_default();
    let anchor_page = page_id.or_else(|| {
        request
            .element_id
            .and_then(|id| project.element(id).map(|(page, _)| page.id))
    });
    let scope = scope_for(original, pending.as_ref(), anchor_page);
    let operations: Vec<_> = request
        .operations
        .iter()
        .filter(|operation| !matches!(operation, ElementOperation::SetPage { .. }))
        .cloned()
        .collect();
    let touched = apply_operations(
        project,
        &scope,
        request.element_id,
        &operations,
        candidate,
        generated,
    )?;
    if let Some(pending) = &pending {
        let outcome = if request.explanation.trim().is_empty() {
            candidate.summary.clone()
        } else {
            request.explanation.clone()
        };
        let outcome = if untouched.is_empty() {
            outcome
        } else {
            format!(
                "{} {outcome}",
                boundary_coverage_message(project, pending, &untouched)
            )
        };
        record_request_history(project, &touched, pending, &outcome, candidate.job_id);
        project.requests.retain(|item| item.id != pending.id);
        if !untouched.is_empty() && pending.boundary_retry_order.is_empty() {
            let mut remaining = pending.clone();
            remaining.boundary_retry_order = pending.boundary_order();
            remaining.targets.retain(|target| {
                matches!(target,
                crate::model::RequestTarget::Boundary { page, .. } if untouched.contains(page))
            });
            // The narrowed targets define the follow-up; an old area must not grant more scope.
            remaining.area = None;
            project.requests.push(remaining);
        }
    }
    Ok(())
}

pub(crate) fn apply_pointer_classifications(
    project: &mut Project,
    classifications: &[PointerClassification],
) -> Result<()> {
    for classification in classifications {
        let pointer = project
            .pointers
            .iter_mut()
            .find(|pointer| pointer.id == classification.pointer_id)
            .ok_or_else(|| Error("This pointer no longer exists.".into()))?;
        if !crate::pointers::pointer_needs_classifying(pointer) {
            continue;
        }
        ensure(
            pointer.comment == classification.comment,
            "A pointer comment changed before this result was applied.",
        )?;
        pointer.play = classification.action.play_action();
        pointer.play_comment = classification.comment.clone();
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
    let mut candidate = candidate.clone();
    candidate.validate_reply_bounds()?;
    candidate.normalize(project, &mut Vec::new())?;
    candidate.validate(snapshot, project)?;
    let candidate = &candidate;
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
    apply_page_operations(&mut next, project, candidate)?;
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
        apply_request(&mut next, project, page_id, request, candidate, &generated)?;
    }
    apply_pointer_classifications(&mut next, &candidate.pointer_classifications)?;
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
            next.decisions.push(crate::model::Decision {
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
        dossier.application_status = crate::dossier::GenerationApplicationStatus::Applied;
        dossier.applied_at_revision = Some(next.revision);
        dossier.accepted_decision_indices = accepted_decision_indices.to_vec();
    }
    // AI layers arrive without names; give them the same numbered default names as hand edits.
    let mut used = crate::element_edit::used_default_names(&next);
    for page in &mut next.pages {
        crate::element_edit::assign_default_names(&mut page.elements, &mut used);
    }
    next.validate()?;
    crate::text_layout::sync_fit_rects(&mut next);
    crate::adapt::adapt_changed_pages(project, &mut next)?;
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
