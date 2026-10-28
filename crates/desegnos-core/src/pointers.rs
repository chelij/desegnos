//! Pointers: intent and permission made durable.
//!
//! The comment is the specification. A change pointer grants its source request permission to
//! edit its target layer; a reference pointer only lets the request read it as context. Simple
//! comments are classified into a Play action by generation.
//! Clashes are derived from the project alone, so the canvas and the AI conversation agree.
use crate::{Pointer, PointerSource, PointerTarget, Project};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PointerState {
    /// A line with no comment: it only shows its target.
    Unwired,
    /// Its comment has no classification or generated code yet, or changed since either.
    NeedsGenerating,
    /// Its comment is new or changed since the last export, so the next export rebuilds it.
    NeedsExporting,
    /// The code the last export made for it changed outside the pointer.
    CodeChanged,
    /// Its generated code still matches the fingerprint recorded with it, or it has been
    /// classified as a simple Play action.
    Wired,
    /// Two pointers from the same source mark the same element with different comments.
    Contradicts,
    /// One endpoint no longer exists.
    Broken,
}

/// One pointer's derived state, recomputed whenever project content changes.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PointerStateView {
    pub id: Uuid,
    pub state: PointerState,
    /// The other pointers this one contradicts, in project order.
    pub clash_with: Vec<Uuid>,
}

/// A pointer's comment still needs an AI classification.
pub fn pointer_needs_classifying(pointer: &Pointer) -> bool {
    !pointer.comment.trim().is_empty() && pointer.play_comment != pointer.comment
}

/// `None` for a pointer that is already wired or classified for its current comment.
pub fn pointer_needs_generating(pointer: &Pointer) -> bool {
    pointer_needs_classifying(pointer)
        && pointer
            .wiring
            .as_ref()
            .is_none_or(|wiring| wiring.generated_comment != pointer.comment)
}

/// The comment, endpoints and kind may change without the id, so generation treats them as
/// context.
pub fn pointer_context_value(project: &Project, pointer: &Pointer) -> serde_json::Value {
    serde_json::json!({
        "id": pointer.id,
        "from": pointer.from,
        "to": pointer.to,
        "comment": pointer.comment,
        "kind": pointer.kind,
        "target": pointer_target_value(project, &pointer.to, pointer.kind),
        "action": pointer.play,
    })
}

/// Whether this pointer lets a request change its target.
pub fn pointer_grants_change(pointer: &Pointer) -> bool {
    pointer.kind == crate::PointerKind::Change
}

/// A precise, human-readable description of what a pointer may read or change.
pub fn pointer_target_description(
    project: &Project,
    target: &PointerTarget,
    kind: crate::PointerKind,
) -> String {
    let permission = match kind {
        crate::PointerKind::Change => "may be changed within this project",
        crate::PointerKind::Reference => "may be read as context, not changed",
    };
    match target {
        PointerTarget::Element {
            page_id, element_id, ..
        } => {
            let Some(page) = project.pages.iter().find(|page| page.id == *page_id) else {
                return "A layer on a page that no longer exists.".into();
            };
            let Some(element) = crate::elements::find_in(&page.elements, *element_id) else {
                return format!(
                    "A layer on {} ({}) that no longer exists.",
                    page.name, page.path
                );
            };
            format!(
                "layer \"{}\" on page {} ({}). The layer {permission}.",
                short(&element.label(), 400),
                page.name,
                page.path
            )
        }
        PointerTarget::Boundary { page_id, .. } => project
            .pages
            .iter()
            .find(|page| page.id == *page_id)
            .map(|page| {
                format!(
                    "page {} ({}). Any of its layers {permission}.",
                    page.name, page.path
                )
            })
            .unwrap_or_else(|| "A page that no longer exists.".into()),
        PointerTarget::Reference { item_id } => project
            .reference
            .iter()
            .find(|item| item.id == *item_id)
            .map(|item| format!("reference item {} (read only).", reference_text(&item.kind)))
            .unwrap_or_else(|| "A reference item that no longer exists.".into()),
        PointerTarget::Url { url } => {
            format!("external URL {url} (read only).")
        }
    }
}

pub fn pointer_target_value(
    project: &Project,
    target: &PointerTarget,
    kind: crate::PointerKind,
) -> serde_json::Value {
    let editable = kind == crate::PointerKind::Change;
    match target {
        PointerTarget::Element {
            page_id,
            element_id,
            size_id,
        } => {
            let page = project.pages.iter().find(|page| page.id == *page_id);
            let element = page.and_then(|page| crate::elements::find_in(&page.elements, *element_id));
            serde_json::json!({
                "kind": "element",
                "pageId": page_id,
                "sizeId": size_id,
                "pageName": page.map(|page| page.name.clone()),
                "path": page.map(|page| page.path.clone()),
                "elementId": element_id,
                "label": element.map(|element| element.label()),
                "editable": editable,
            })
        }
        PointerTarget::Boundary { page_id, size_id } => {
            let page = project.pages.iter().find(|page| page.id == *page_id);
            serde_json::json!({
                "kind": "boundary",
                "pageId": page_id,
                "sizeId": size_id,
                "pageName": page.map(|page| page.name.clone()),
                "path": page.map(|page| page.path.clone()),
                "editable": editable,
            })
        }
        PointerTarget::Reference { item_id } => {
            let item = project.reference.iter().find(|item| item.id == *item_id);
            serde_json::json!({
                "kind": "reference",
                "itemId": item_id,
                "reference": item.map(|item| reference_text(&item.kind)),
                "editable": false,
            })
        }
        PointerTarget::Url { url } => serde_json::json!({
            "kind": "url",
            "url": url,
            "editable": false,
        }),
    }
}

pub fn reference_text(kind: &crate::ReferenceKind) -> String {
    match kind {
        crate::ReferenceKind::Element { element, .. } => format!("reference layer \"{}\"", element.label()),
        crate::ReferenceKind::Note { text } => format!("note \"{}\"", short(text, 400)),
        crate::ReferenceKind::Link { url, title } => {
            if title.trim().is_empty() {
                format!("link {url}")
            } else {
                format!("link \"{}\" ({url})", short(title, 200))
            }
        }
        crate::ReferenceKind::Image { name, .. } => {
            format!("picture \"{}\"", short(name, 200))
        }
        crate::ReferenceKind::Swatch { color } => format!("colour {color}"),
    }
}

fn short(value: &str, limit: usize) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() <= limit {
        return value;
    }
    let mut short: String = value.chars().take(limit).collect();
    short.push('…');
    short
}

/// The reference items a pointer reads: its target, or both ends when it points at one.
pub fn pointed_reference_ids(project: &Project, pointer: &Pointer) -> Vec<Uuid> {
    let mut ids = Vec::new();
    if let PointerTarget::Reference { item_id } = &pointer.to
        && project.reference.iter().any(|item| item.id == *item_id)
    {
        ids.push(*item_id);
    }
    ids
}

fn collect_ids(elements: &[crate::Element], ids: &mut BTreeSet<Uuid>) {
    for element in elements {
        ids.insert(element.id);
        collect_ids(&element.children, ids);
    }
}

impl Project {
    /// Element ids a request on this layer may change: its own subtree plus the targets of
    /// change pointers whose source is this layer or one of its descendants. Reference pointers
    /// only travel as context and grant nothing here.
    pub fn editable_element_ids(
        &self,
        page_id: Uuid,
        element_id: Uuid,
    ) -> (BTreeSet<Uuid>, BTreeSet<Uuid>) {
        let mut elements = self.element_subtree_ids(element_id);
        let mut pages = BTreeSet::new();
        for pointer in &self.pointers {
            let source_matches = match &pointer.from {
                PointerSource::Element {
                    page_id: source_page_id,
                    element_id: source_element,
                    ..
                } => *source_page_id == page_id && elements.contains(source_element),
                PointerSource::Boundary {
                    page_id: source_page_id,
                    ..
                } => *source_page_id == page_id,
            };
            if !source_matches || !pointer_grants_change(pointer) {
                continue;
            }
            match &pointer.to {
                PointerTarget::Element {
                    page_id: target_page,
                    element_id: target_element,
                    ..
                } => {
                    elements.extend(self.element_subtree_ids(*target_element));
                    pages.insert(*target_page);
                }
                PointerTarget::Boundary { page_id, .. } => {
                    pages.insert(*page_id);
                    if let Some(page) = self.pages.iter().find(|page| page.id == *page_id) {
                        for root in &page.elements {
                            collect_ids(std::slice::from_ref(root), &mut elements);
                        }
                    }
                }
                PointerTarget::Reference { .. } | PointerTarget::Url { .. } => {}
            }
        }
        (elements, pages)
    }

    /// Pointers that touch a requested layer, in project order.
    pub fn pointers_touching(
        &self,
        page_id: Uuid,
        element_id: Uuid,
    ) -> Vec<&Pointer> {
        let ids = self.element_subtree_ids(element_id);
        self.pointers
            .iter()
            .filter(|pointer| {
                let from_matches = match &pointer.from {
                    PointerSource::Element {
                        page_id: source_page,
                        element_id: source_element,
                        ..
                    } => *source_page == page_id && ids.contains(source_element),
                    PointerSource::Boundary {
                        page_id: source_page,
                        ..
                    } => *source_page == page_id,
                };
                let to_matches = match &pointer.to {
                    PointerTarget::Element {
                        page_id: target_page,
                        element_id: target_element,
                        ..
                    } => *target_page == page_id && ids.contains(target_element),
                    PointerTarget::Boundary { page_id: target_page, .. } => *target_page == page_id,
                    PointerTarget::Reference { .. } | PointerTarget::Url { .. } => false,
                };
                from_matches || to_matches
            })
            .collect()
    }

    /// Derived pointer states. Broken wins, then contradictions between pointers from the same
    /// source, then code changed outside its pointer, then a comment the last export has not
    /// built, then a comment that still needs classifying, then a wired pointer.
    pub fn pointer_states(&self) -> Vec<PointerStateView> {
        self.pointer_states_with_fingerprints(&BTreeMap::new())
    }

    /// The same states, with the fingerprints recomputed from the last export's files. A
    /// pointer whose recomputed fingerprint differs from the recorded one has changed code.
    pub fn pointer_states_with_fingerprints(
        &self,
        current: &BTreeMap<Uuid, String>,
    ) -> Vec<PointerStateView> {
        if self.pointers.is_empty() {
            return Vec::new();
        }
        let exported = self.last_export.as_ref();
        let mut contradictory: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
        for (index, first) in self.pointers.iter().enumerate() {
            for second in self.pointers.iter().skip(index + 1) {
                if first.from != second.from
                    || first.comment.trim().is_empty()
                    || second.comment.trim().is_empty()
                    || first.comment.trim() == second.comment.trim()
                {
                    continue;
                }
                contradictory.entry(first.id).or_default().push(second.id);
                contradictory.entry(second.id).or_default().push(first.id);
            }
        }
        self.pointers
            .iter()
            .map(|pointer| {
                let clash_with = contradictory.get(&pointer.id).cloned().unwrap_or_default();
                let wired = pointer
                    .wiring
                    .as_ref()
                    .is_some_and(|wiring| wiring.generated_comment == pointer.comment);
                let classified = pointer.play_comment == pointer.comment && !pointer.comment.trim().is_empty();
                let recorded = exported.and_then(|record| record.pointers.get(&pointer.id));
                let code_changed = recorded.is_some_and(|recorded| {
                    current
                        .get(&pointer.id)
                        .is_some_and(|fingerprint| fingerprint != &recorded.fingerprint)
                });
                let needs_exporting = exported.is_some_and(|_| {
                    !pointer.comment.trim().is_empty()
                        && recorded.is_none_or(|recorded| recorded.comment != pointer.comment)
                });
                let state = if pointer.is_broken(self) {
                    PointerState::Broken
                } else if !clash_with.is_empty() {
                    PointerState::Contradicts
                } else if code_changed {
                    PointerState::CodeChanged
                } else if needs_exporting {
                    PointerState::NeedsExporting
                } else if wired || classified || recorded.is_some() {
                    PointerState::Wired
                } else if pointer.comment.trim().is_empty() {
                    PointerState::Unwired
                } else {
                    PointerState::NeedsGenerating
                };
                PointerStateView {
                    id: pointer.id,
                    state,
                    clash_with,
                }
            })
            .collect()
    }
}

pub fn source_description(project: &Project, source: &PointerSource) -> String {
    match source {
        PointerSource::Element {
            page_id, element_id, ..
        } => {
            let page = project.pages.iter().find(|page| page.id == *page_id);
            let element = page.and_then(|page| crate::elements::find_in(&page.elements, *element_id));
            match (page, element) {
                (Some(page), Some(element)) => format!(
                    "layer \"{}\" on page {} ({})",
                    short(&element.label(), 400),
                    page.name,
                    page.path
                ),
                (Some(page), None) => format!("a layer on page {} ({})", page.name, page.path),
                _ => "a layer that no longer exists".into(),
            }
        }
        PointerSource::Boundary { page_id, .. } => project
            .pages
            .iter()
            .find(|page| page.id == *page_id)
            .map(|page| format!("page boundary {} ({})", page.name, page.path))
            .unwrap_or_else(|| "a page boundary that no longer exists".into()),
    }
}
