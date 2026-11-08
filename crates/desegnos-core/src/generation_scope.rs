use crate::model::Project;
use std::collections::BTreeSet;
use uuid::Uuid;
/// What one reply outcome may change. Element targets grant their subtree plus what their
/// change pointers grant; a boundary target grants every layer inside it; a change pointer target
/// grants what that pointer points at. Reference pointers grant reads only.
pub(crate) struct RequestScope {
    pub(crate) original_routes: Vec<(Uuid, String, String)>,
    pub(crate) request_text: String,
    pub(crate) behaviour_page: Option<Uuid>,
    pub(crate) allowed: BTreeSet<Uuid>,
    pub(crate) boundary_pages: BTreeSet<Uuid>,
    pub(crate) area: Option<crate::model::RequestArea>,
}

pub(crate) fn scope_for(
    project: &Project,
    request: Option<&crate::model::PendingRequest>,
    anchor_page: Option<Uuid>,
) -> RequestScope {
    let mut allowed = BTreeSet::new();
    let mut boundary_pages = BTreeSet::new();
    if let Some(request) = request {
        for target in &request.targets {
            match target {
                crate::model::RequestTarget::Element { page, id, .. } => {
                    let (elements, pages) = project.editable_element_ids(*page, *id);
                    allowed.extend(elements);
                    boundary_pages.extend(pages);
                }
                crate::model::RequestTarget::Boundary { page, .. } => {
                    boundary_pages.insert(*page);
                    if let Some(page) = project.pages.iter().find(|item| item.id == *page) {
                        for element in &page.elements {
                            allowed.extend(project.element_subtree_ids(element.id));
                            let (_, pages) = project.editable_element_ids(page.id, element.id);
                            boundary_pages.extend(pages);
                        }
                    }
                }
                crate::model::RequestTarget::Pointer { id } => {
                    if let Some(pointer) = project
                        .pointers
                        .iter()
                        .find(|pointer| pointer.id == *id)
                        .filter(|pointer| crate::pointers::pointer_grants_change(pointer))
                    {
                        match &pointer.to {
                            crate::model::PointerTarget::Element {
                                page_id,
                                element_id,
                                ..
                            } => {
                                allowed.extend(project.element_subtree_ids(*element_id));
                                let (_, pages) =
                                    project.editable_element_ids(*page_id, *element_id);
                                boundary_pages.extend(pages);
                            }
                            crate::model::PointerTarget::Boundary { page_id, .. } => {
                                boundary_pages.insert(*page_id);
                            }
                            crate::model::PointerTarget::Reference { .. }
                            | crate::model::PointerTarget::Url { .. } => {}
                        }
                    }
                }
                crate::model::RequestTarget::Reference { .. } => {}
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
        original_routes: project
            .pages
            .iter()
            .map(|page| (page.id, page.name.clone(), page.path.clone()))
            .collect(),
        request_text: request
            .map(|request| request.text.clone())
            .unwrap_or_default(),
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

fn shape_contains(shape: &crate::model::RequestShape, point: crate::model::RequestPoint) -> bool {
    match shape {
        crate::model::RequestShape::Rect { rect } => {
            point.x >= rect.x
                && point.x <= rect.x + rect.width
                && point.y >= rect.y
                && point.y <= rect.y + rect.height
        }
        crate::model::RequestShape::Ellipse { rect } => {
            let rx = rect.width / 2.0;
            let ry = rect.height / 2.0;
            if rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let dx = (point.x - (rect.x + rx)) / rx;
            let dy = (point.y - (rect.y + ry)) / ry;
            dx * dx + dy * dy <= 1.0
        }
        crate::model::RequestShape::Polygon { points }
        | crate::model::RequestShape::Freehand { points } => {
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
    point: crate::model::RequestPoint,
    start: &crate::model::RequestPoint,
    end: &crate::model::RequestPoint,
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
pub(crate) fn area_allows(area: &crate::model::RequestArea, rect: &crate::model::Rect) -> bool {
    let corners = [
        (rect.x, rect.y),
        (rect.x + rect.width, rect.y),
        (rect.x + rect.width, rect.y + rect.height),
        (rect.x, rect.y + rect.height),
        (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
    ];
    corners
        .iter()
        .all(|(x, y)| shape_contains(&area.shape, crate::model::RequestPoint { x: *x, y: *y }))
}
