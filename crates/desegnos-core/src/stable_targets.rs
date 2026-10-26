//! Editor identity is independent of source positions; exported sites never carry it.
use crate::{ElementTarget, Error, Project, Rect, Result, ensure};
use dom_query::{Document, NodeRef};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const ELEMENT_ID_ATTRIBUTE: &str = "data-desegnos-id";
pub(crate) fn valid_id(id: &str) -> bool {
    (8..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}
pub(crate) fn random_id(used: &mut BTreeSet<String>) -> String {
    loop {
        let id = Uuid::new_v4().simple().to_string()[..12].to_string();
        if used.insert(id.clone()) {
            return id;
        }
    }
}
pub(crate) fn validate_ids(doc: &Document) -> Result<()> {
    let mut used = BTreeSet::new();
    for node in doc.select(&format!("[{ELEMENT_ID_ATTRIBUTE}]")).nodes() {
        let id = node.attr(ELEMENT_ID_ATTRIBUTE).expect("selected attribute");
        ensure(
            valid_id(&id) && used.insert(id.to_string()),
            "Editor element IDs must be valid and unique within the page.",
        )?;
    }
    Ok(())
}
pub(crate) fn assign_document_ids(doc: &Document) -> Result<()> {
    validate_ids(doc)?;
    let mut used: BTreeSet<_> = doc
        .select(&format!("[{ELEMENT_ID_ATTRIBUTE}]"))
        .nodes()
        .iter()
        .filter_map(|n| n.attr(ELEMENT_ID_ATTRIBUTE).map(|v| v.to_string()))
        .collect();
    let body = crate::editing::body(doc)?;
    let nodes: Vec<_> = body.descendants_it().filter(|n| n.is_element()).collect();
    for node in nodes {
        if !node.has_attr(ELEMENT_ID_ATTRIBUTE) {
            node.set_attr(ELEMENT_ID_ATTRIBUTE, &random_id(&mut used));
        }
    }
    Ok(())
}
pub(crate) fn contains(outer: NodeRef<'_>, inner: NodeRef<'_>) -> bool {
    let mut current = Some(inner);
    while let Some(node) = current {
        if node.id == outer.id {
            return true;
        }
        current = node.parent();
    }
    false
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetBounds {
    pub target: ElementTarget,
    pub rect: Rect,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementTargetBounds {
    pub region_id: Uuid,
    pub bounds: Vec<TargetBounds>,
}
impl Project {
    pub fn delete_elements(
        &self,
        revision: u64,
        page_id: Uuid,
        targets: &[ElementTarget],
    ) -> Result<Self> {
        self.delete_elements_in(revision, &crate::DocumentTarget::Page { page_id }, targets)
    }
    /// Delete elements from a page or from an editable site HTML file shown by an iframe.
    pub fn delete_elements_in(
        &self,
        revision: u64,
        document: &crate::DocumentTarget,
        targets: &[ElementTarget],
    ) -> Result<Self> {
        ensure(
            revision == self.revision,
            "The project changed. Try the edit again.",
        )?;
        let canonical = self.canonical_targets_in(document, targets)?;
        let content = self.document_content(document)?;
        let doc = self.document_edit_document(document, &content.html)?;
        let tracked = match document {
            crate::DocumentTarget::Page { page_id } => {
                crate::editing::tracked(&doc, self.page(*page_id)?, &self.document_hash(document)?)?
            }
            crate::DocumentTarget::SiteFile { .. } => vec![],
        };
        let body = crate::editing::body(&doc)?;
        for target in canonical {
            let node = crate::editing::locate(&doc, &target)?;
            ensure(node.id != body.id, "Select items inside the page.")?;
            node.remove_from_parent();
        }
        self.finish_document_edit(document, revision, &doc, None, tracked, None)
            .map(|(project, _)| project)
    }
    /// The browser form an element edit uses for this document, unchanged for pages.
    pub(crate) fn document_edit_document(
        &self,
        document: &crate::DocumentTarget,
        html: &str,
    ) -> Result<dom_query::Document> {
        match document {
            crate::DocumentTarget::Page { .. } => crate::editing::parse(html),
            crate::DocumentTarget::SiteFile { .. } => self.parse_document_target(document, html),
        }
    }
    pub fn canonical_targets(
        &self,
        page_id: Uuid,
        targets: &[ElementTarget],
    ) -> Result<Vec<ElementTarget>> {
        self.canonical_targets_in(&crate::DocumentTarget::Page { page_id }, targets)
    }
    pub fn canonical_targets_in(
        &self,
        document: &crate::DocumentTarget,
        targets: &[ElementTarget],
    ) -> Result<Vec<ElementTarget>> {
        ensure(
            !targets.is_empty() && targets.len() <= crate::MAX_BATCH_ELEMENT_EDITS,
            "Select between one and 500 elements.",
        )?;
        let content = self.document_content(document)?;
        let hash = self.document_hash(document)?;
        let doc = self.document_edit_document(document, &content.html)?;
        let body = crate::editing::body(&doc)?;
        let mut nodes = Vec::new();
        for selected in targets {
            ensure(
                !selected.id.is_empty() || selected.source_sha256 == hash,
                "The document changed. Select this element again.",
            )?;
            let node = crate::editing::locate(&doc, selected)?;
            if !nodes
                .iter()
                .any(|existing: &NodeRef<'_>| existing.id == node.id)
            {
                nodes.push(node);
            }
        }
        let retained: Vec<_> = nodes
            .iter()
            .copied()
            .filter(|node| {
                !nodes
                    .iter()
                    .any(|outer| outer.id != node.id && contains(*outer, *node))
            })
            .collect();
        retained
            .into_iter()
            .map(|node| {
                crate::editing::target(node, body, &hash)
                    .ok_or_else(|| Error("This element cannot be selected.".into()))
            })
            .collect()
    }
    pub fn target_bounds_union(
        &self,
        page_id: Uuid,
        targets: &[ElementTarget],
        bounds: &[TargetBounds],
    ) -> Result<Rect> {
        self.target_bounds_union_in(&crate::DocumentTarget::Page { page_id }, targets, bounds)
    }
    pub fn target_bounds_union_in(
        &self,
        document: &crate::DocumentTarget,
        targets: &[ElementTarget],
        bounds: &[TargetBounds],
    ) -> Result<Rect> {
        ensure(
            !bounds.is_empty() && bounds.len() <= crate::MAX_BATCH_ELEMENT_EDITS,
            "Supply the selected element positions.",
        )?;
        let viewport = self.document_viewport(document)?;
        let content = self.document_content(document)?;
        let doc = self.document_edit_document(document, &content.html)?;
        let mut seen = BTreeSet::new();
        let mut rectangles = Vec::new();
        for bound in bounds {
            bound.rect.validate(viewport)?;
            let node = crate::editing::locate(&doc, &bound.target)?;
            ensure(
                seen.insert(node.id),
                "An element position was supplied twice.",
            )?;
            // Incoming nested selections may include descendants removed by canonicalization.
            ensure(
                targets.iter().any(|target| {
                    crate::editing::locate(&doc, target).is_ok_and(|outer| contains(outer, node))
                }),
                "A position refers outside the selected elements.",
            )?;
            if targets.iter().any(|target| {
                crate::editing::locate(&doc, target).is_ok_and(|outer| outer.id == node.id)
            }) {
                rectangles.push(bound.rect);
            }
        }
        ensure(
            rectangles.len() == targets.len(),
            "Refresh every selected element position.",
        )?;
        let left = rectangles.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
        let top = rectangles.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
        let right = rectangles
            .iter()
            .map(|r| r.x + r.width)
            .fold(f64::NEG_INFINITY, f64::max);
        let bottom = rectangles
            .iter()
            .map(|r| r.y + r.height)
            .fold(f64::NEG_INFINITY, f64::max);
        let result = Rect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        };
        result.validate(viewport)?;
        Ok(result)
    }
    pub fn refresh_element_target_bounds(
        &self,
        page_id: Uuid,
        viewport: crate::Viewport,
        bounds: &[ElementTargetBounds],
    ) -> Result<Self> {
        self.validate()?;
        let page = self.page(page_id)?;
        ensure(
            page.viewport == viewport,
            "The screen guide changed. Refresh the element positions.",
        )?;
        ensure(
            bounds.len() <= page.regions.len(),
            "Too many element positions were supplied.",
        )?;
        let mut seen = BTreeSet::new();
        let mut updates = Vec::new();
        for entry in bounds {
            ensure(
                seen.insert(entry.region_id),
                "A request position was supplied twice.",
            )?;
            let region = page
                .regions
                .iter()
                .find(|r| r.id == entry.region_id && !r.completed)
                .ok_or_else(|| Error("This request no longer exists.".into()))?;
            if !region.has_targets() {
                ensure(
                    entry.bounds.len() <= 10_000,
                    "An area supports at most 10,000 measured elements.",
                )?;
                let source = page
                    .content
                    .as_ref()
                    .ok_or_else(|| Error("The page has no elements to measure.".into()))?;
                let document = crate::editing::parse(&source.html)?;
                let mut ids = BTreeSet::new();
                for bound in &entry.bounds {
                    bound.rect.validate(viewport)?;
                    bound.target.validate()?;
                    ensure(
                        !bound.target.id.is_empty() && ids.insert(&bound.target.id),
                        "Measured elements need unique editor IDs.",
                    )?;
                    crate::editing::locate(&document, &bound.target)?;
                }
                updates.push((entry.region_id, region.rect, Some(entry.bounds.clone())));
                continue;
            }
            let targets: Vec<_> = region.targets().into_iter().cloned().collect();
            let rect = self.target_bounds_union(page_id, &targets, &entry.bounds)?;
            updates.push((entry.region_id, rect, None));
        }
        let mut next = self.clone();
        let page = next
            .pages
            .iter_mut()
            .find(|p| p.id == page_id)
            .expect("checked page");
        let mut changed = false;
        for (id, rect, area_bounds) in updates {
            let region = page
                .regions
                .iter_mut()
                .find(|r| r.id == id)
                .expect("checked request");
            changed |= region.rect != rect;
            region.rect = rect;
            if let Some(area_bounds) = area_bounds {
                changed |= region.area_bounds != area_bounds;
                region.area_bounds = area_bounds;
            }
        }
        if changed {
            next.revision = self
                .revision
                .checked_add(1)
                .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        }
        next.validate()?;
        Ok(next)
    }
}
