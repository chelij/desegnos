use crate::{ElementTarget, Error, Project, Result, SiteContent, ensure};
use dom_query::{Document, NodeRef, Selection};
use html5ever::{ParseOpts, tendril::TendrilSink, tree_builder::TreeBuilderOpts};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const MAX_NODES: usize = 20_000;
const ATTRIBUTES: &[&str] = &[
    "href",
    "target",
    "rel",
    "src",
    "alt",
    "title",
    "placeholder",
    "value",
    "type",
    "aria-label",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextEdit {
    pub index: usize,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementStyleEdit {
    pub target: ElementTarget,
    pub style: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pseudo_styles: Vec<crate::LayerPseudoStyle>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ElementOrder {
    Before,
    After,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ElementEdit {
    Update {
        texts: Vec<TextEdit>,
        attributes: BTreeMap<String, Option<String>>,
        style: Option<String>,
    },
    Replace {
        html: String,
        keep_style: bool,
    },
    Reorder {
        direction: ElementOrder,
    },
    LayerStyles {
        styles: Vec<ElementStyleEdit>,
    },
    Remove,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementDetails {
    pub target: ElementTarget,
    pub texts: Vec<TextEdit>,
    pub texts_truncated: bool,
    pub attributes: BTreeMap<String, String>,
    pub inline_style: String,
    pub html: String,
    pub can_move: bool,
    pub can_move_before: bool,
    pub can_move_after: bool,
}

pub(crate) fn parse(html: &str) -> Result<Document> {
    let doc = Document::from(format!(
        "<!doctype html><html><head></head><body>{html}</body></html>"
    ));
    bounded(&doc)?;
    Ok(doc)
}
/// A page body fragment (the usual form) or, when `full`, a complete document as written.
pub(crate) fn parse_document(html: &str, full: bool) -> Result<Document> {
    if !full {
        return parse(html);
    }
    let doc = Document::from(html);
    bounded(&doc)?;
    Ok(doc)
}
pub(crate) fn bounded(doc: &Document) -> Result<()> {
    let mut pending = vec![(doc.root(), 0)];
    let mut count = 0;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        ensure(
            count <= MAX_NODES && depth <= 128,
            "This page is too complex for direct editing. Use a smaller page.",
        )?;
        pending.extend(node.children().into_iter().map(|child| (child, depth + 1)));
    }
    Ok(())
}
pub(crate) fn body(doc: &Document) -> Result<NodeRef<'_>> {
    doc.html_root()
        .element_children()
        .into_iter()
        .find(|node| node.node_name().as_deref() == Some("body"))
        .ok_or_else(|| Error("The page has no editable body.".into()))
}
pub(crate) fn locate<'a>(doc: &'a Document, target: &ElementTarget) -> Result<NodeRef<'a>> {
    target.validate()?;
    if !target.id.is_empty() {
        let nodes = doc.select(&format!(
            "[{}=\"{}\"]",
            crate::ELEMENT_ID_ATTRIBUTE,
            target.id
        ));
        ensure(nodes.nodes().len() == 1, "This element was removed.")?;
        return Ok(nodes.nodes()[0]);
    }
    let mut node = body(doc)?;
    // Legacy positional targets are only meaningful on a page without editor identities.
    ensure(
        doc.select(&format!("[{}]", crate::ELEMENT_ID_ATTRIBUTE))
            .nodes()
            .is_empty()
            || target.source_sha256.len() == 64,
        "This element was removed.",
    )?;
    for part in target.selector.split(" > ").skip(1) {
        let (tag, index) = part
            .split_once(":nth-of-type(")
            .ok_or_else(|| Error("Invalid element path.".into()))?;
        let index = index
            .trim_end_matches(')')
            .parse::<usize>()
            .map_err(|_| Error("Invalid element path.".into()))?;
        node = node
            .element_children()
            .into_iter()
            .filter(|child| {
                child
                    .node_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case(tag))
            })
            .nth(index - 1)
            .ok_or_else(|| Error("This element no longer exists. Select it again.".into()))?;
    }
    ensure(
        node.node_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(&target.tag)),
        "The selected element changed. Select it again.",
    )?;
    Ok(node)
}
pub(crate) fn selector(node: NodeRef<'_>, body: NodeRef<'_>) -> Option<String> {
    let mut node = node;
    let mut parts = vec![];
    while node.id != body.id {
        let tag = node.node_name()?.to_string();
        let parent = node.parent()?;
        let index = parent
            .element_children()
            .into_iter()
            .filter(|child| {
                child
                    .node_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&tag))
            })
            .position(|child| child.id == node.id)?
            + 1;
        parts.push(format!("{tag}:nth-of-type({index})"));
        node = parent;
        if parts.len() >= 64 {
            return None;
        }
    }
    parts.push("body".into());
    parts.reverse();
    Some(parts.join(" > "))
}

fn clear_found_ancestry(node: NodeRef<'_>) {
    let mut current = Some(node);
    while let Some(node) = current {
        node.remove_attr(crate::FOUND_CONTENT_ATTRIBUTE);
        current = node.parent();
    }
}
fn text_nodes(node: NodeRef<'_>) -> Vec<NodeRef<'_>> {
    let mut pending = vec![node];
    let mut result = vec![];
    while let Some(node) = pending.pop() {
        if matches!(
            node.node_name().as_deref(),
            Some("script" | "style" | "noscript" | "template" | "svg")
        ) {
            continue;
        }
        if node.is_text() {
            result.push(node);
        } else {
            pending.extend(node.children().into_iter().rev());
        }
    }
    result
}
pub(crate) fn target(node: NodeRef<'_>, body: NodeRef<'_>, hash: &str) -> Option<ElementTarget> {
    let id = node
        .attr(crate::ELEMENT_ID_ATTRIBUTE)
        .map(|v| v.to_string())
        .unwrap_or_default();
    let result = ElementTarget {
        id: id.clone(),
        selector: selector(node, body)?,
        tag: node.node_name()?.to_string(),
        label: text_nodes(node)
            .iter()
            .map(|node| node.text().to_string())
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(240)
            .collect(),
        source_sha256: if id.is_empty() {
            hash.into()
        } else {
            String::new()
        },
    };
    result.validate().ok()?;
    Some(result)
}
fn sibling<'a>(node: NodeRef<'a>, direction: &ElementOrder) -> Option<NodeRef<'a>> {
    let mut next = match direction {
        ElementOrder::Before => node.prev_sibling(),
        ElementOrder::After => node.next_sibling(),
    };
    while let Some(value) = next {
        if value.is_element() {
            return Some(value);
        }
        next = match direction {
            ElementOrder::Before => value.prev_sibling(),
            ElementOrder::After => value.next_sibling(),
        };
    }
    None
}
pub(crate) fn serialized(doc: &Document) -> Result<String> {
    doc.try_html()
        .map(|value| value.to_string())
        .ok_or_else(|| Error("The edited HTML could not be saved.".into()))
}
pub(crate) fn check_value(value: &str, max: usize) -> Result<()> {
    ensure(
        value.len() <= max && !value.contains('\0'),
        "This edit exceeds the supported text size or contains a null character.",
    )
}
pub(crate) fn ids(doc: &Document) -> BTreeMap<String, usize> {
    let mut ids = BTreeMap::new();
    for node in doc.root().descendants_it() {
        if let Some(id) = node.attr("id") {
            *ids.entry(id.to_string()).or_default() += 1;
        }
    }
    ids
}

pub(crate) fn tracked<'a>(
    doc: &'a Document,
    page: &crate::Page,
    hash: &str,
) -> Result<Vec<(Uuid, NodeRef<'a>)>> {
    page.regions
        .iter()
        .filter(|r| !r.completed)
        .flat_map(|r| r.targets().into_iter().map(move |t| (r.id, t)))
        .filter(|(_, target)| !target.id.is_empty() || target.source_sha256 == hash)
        .filter_map(|(id, target)| locate(doc, target).ok().map(|node| Ok((id, node))))
        .collect()
}

impl Project {
    pub(crate) fn parse_page_html(&self, html: &str) -> Result<Document> {
        parse_document(html, self.website.is_some())
    }
    pub fn inspect_element(
        &self,
        page_id: Uuid,
        selected: &ElementTarget,
    ) -> Result<ElementDetails> {
        self.validate()?;
        let content = self
            .page(page_id)?
            .content
            .as_ref()
            .ok_or_else(|| Error("Generate a page before selecting its elements.".into()))?;
        ensure(
            !selected.id.is_empty() || content.sha256()? == selected.source_sha256,
            "The page changed. Select this element again.",
        )?;
        let doc = self.parse_page_html(&content.html)?;
        let node = locate(&doc, selected)?;
        let text = text_nodes(node);
        let editable = text
            .iter()
            .enumerate()
            .filter(|(_, node)| !node.text().trim().is_empty())
            .collect::<Vec<_>>();
        Ok(ElementDetails {
            target: target(node, body(&doc)?, &content.sha256()?)
                .ok_or_else(|| Error("This item cannot be selected.".into()))?,
            texts: editable
                .iter()
                .take(64)
                .map(|(index, node)| TextEdit {
                    index: *index,
                    text: node.text().to_string(),
                })
                .collect(),
            texts_truncated: editable.len() > 64,
            attributes: ATTRIBUTES
                .iter()
                .filter_map(|name| {
                    node.attr(name)
                        .map(|value| ((*name).into(), value.to_string()))
                })
                .collect(),
            inline_style: node
                .attr("style")
                .map(|value| value.to_string())
                .unwrap_or_default(),
            html: node
                .try_html()
                .ok_or_else(|| Error("This element could not be read.".into()))?
                .to_string(),
            can_move: selected.selector != "body",
            can_move_before: sibling(node, &ElementOrder::Before).is_some(),
            can_move_after: sibling(node, &ElementOrder::After).is_some(),
        })
    }

    pub fn edit_element(
        &self,
        revision: u64,
        page_id: Uuid,
        selected: &ElementTarget,
        edit: &ElementEdit,
    ) -> Result<(Self, Option<ElementTarget>)> {
        self.validate()?;
        ensure(
            revision == self.revision,
            "The project changed. Try the edit again.",
        )?;
        let page = self.page(page_id)?;
        let original = page
            .content
            .as_ref()
            .ok_or_else(|| Error("This page has no site to edit.".into()))?;
        ensure(
            !selected.id.is_empty() || original.sha256()? == selected.source_sha256,
            "The page changed. Select this element again.",
        )?;
        let doc = self.parse_page_html(&original.html)?;
        let node = locate(&doc, selected)?;
        let before = serialized(&doc)?;
        let before_ids = ids(&doc);
        let tracked = tracked(&doc, page, &selected.source_sha256)?;
        let mut retained = Some(node);
        match edit {
            ElementEdit::Update {
                texts,
                attributes,
                style,
            } => {
                ensure(
                    texts.len() <= 64 && attributes.len() <= ATTRIBUTES.len(),
                    "Too many element fields were supplied.",
                )?;
                let nodes = text_nodes(node);
                let mut indexes = BTreeSet::new();
                for text in texts {
                    check_value(&text.text, crate::MAX_SITE_SOURCE_BYTES)?;
                    ensure(
                        indexes.insert(text.index),
                        "A text block was supplied twice.",
                    )?;
                    let text_node = nodes.get(text.index).ok_or_else(|| {
                        Error("The text changed. Reopen the element editor.".into())
                    })?;
                    if text_node.text().as_ref() != text.text {
                        clear_found_ancestry(*text_node);
                        text_node.set_text(text.text.as_str());
                    }
                }
                for (name, value) in attributes {
                    ensure(
                        ATTRIBUTES.contains(&name.as_str()),
                        "This attribute cannot be edited with this control.",
                    )?;
                    if name == "src" && node.attr(name).as_deref() != value.as_deref() {
                        clear_found_ancestry(node);
                    }
                    if let Some(value) = value {
                        check_value(value, crate::MAX_SITE_SOURCE_BYTES)?;
                        if name == "href" {
                            let lower = value.trim().to_lowercase();
                            ensure(
                                !lower.contains(':')
                                    || ["http:", "https:", "mailto:", "tel:"]
                                        .iter()
                                        .any(|prefix| lower.starts_with(prefix)),
                                "Use a website address, email, telephone number or page link.",
                            )?;
                        }
                        if name == "src" {
                            ensure(
                                node.node_name().as_deref() == Some("img")
                                    && (value.starts_with("data:image/")
                                        || value.starts_with(crate::PROJECT_ASSET_SCHEME)
                                        || value.starts_with('/')
                                        || value.starts_with("https://")
                                        || value.starts_with("http://")),
                                "Choose an image file or a valid image address.",
                            )?;
                        }
                        node.set_attr(name, value);
                    } else {
                        node.remove_attr(name);
                    }
                }
                if let Some(style) = style {
                    check_value(style, 65_536)?;
                    if style.is_empty() {
                        node.remove_attr("style");
                    } else {
                        node.set_attr("style", style);
                    }
                }
            }
            ElementEdit::Replace { html, keep_style } => {
                ensure(
                    selected.selector != "body",
                    "Select an item inside the page to replace it.",
                )?;
                check_value(html, crate::MAX_SITE_SOURCE_BYTES)?;
                let parent = node
                    .parent()
                    .ok_or_else(|| Error("This element has no parent.".into()))?;
                let context = parent
                    .query(|value| value.as_element().map(|element| element.name.clone()))
                    .flatten()
                    .ok_or_else(|| Error("The replacement context is invalid.".into()))?;
                let fragment = html5ever::parse_fragment(
                    Document::fragment_sink(),
                    ParseOpts {
                        tree_builder: TreeBuilderOpts {
                            scripting_enabled: false,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    context,
                    vec![],
                    false,
                )
                .one(html.as_str());
                bounded(&fragment)?;
                let root = fragment
                    .root()
                    .first_child()
                    .ok_or_else(|| Error("Enter an element to replace this item.".into()))?;
                let children = root
                    .children()
                    .into_iter()
                    .filter(|child| {
                        child.is_element() || child.is_text() && !child.text().trim().is_empty()
                    })
                    .collect::<Vec<_>>();
                ensure(
                    children.len() == 1 && children[0].is_element(),
                    "Replacement must contain exactly one element.",
                )?;
                let replacement = children[0];
                if let Some(id) = node.attr(crate::ELEMENT_ID_ATTRIBUTE) {
                    replacement.set_attr(crate::ELEMENT_ID_ATTRIBUTE, &id);
                }
                if *keep_style {
                    for name in ["id", "class", "style"] {
                        if !replacement.has_attr(name)
                            && let Some(value) = node.attr(name)
                        {
                            replacement.set_attr(name, &value);
                        }
                    }
                }
                let previous = node.prev_sibling();
                Selection::from(node).replace_with_selection(&Selection::from(replacement));
                retained = match previous {
                    Some(previous) => previous.next_sibling(),
                    None => parent.first_child(),
                };
            }
            ElementEdit::Reorder { direction } => {
                ensure(
                    selected.selector != "body",
                    "The page itself cannot be reordered.",
                )?;
                let adjacent = sibling(node, direction).ok_or_else(|| {
                    Error("This item is already at that end of its layout.".into())
                })?;
                node.remove_from_parent();
                match direction {
                    ElementOrder::Before => adjacent.insert_before(&node),
                    ElementOrder::After => adjacent.insert_after(&node),
                }
            }
            ElementEdit::LayerStyles { styles } => {
                crate::layers::apply_layer_styles(&doc, node, selected, styles)?;
            }
            ElementEdit::Remove => {
                ensure(
                    selected.selector != "body",
                    "The page itself cannot be deleted. Select an item inside it.",
                )?;
                node.remove_from_parent();
                retained = None;
            }
        }
        bounded(&doc)?;
        let html = serialized(&doc)?;
        if html == before {
            return Ok((self.clone(), Some(selected.clone())));
        }
        for (id, count) in ids(&doc) {
            ensure(
                count <= before_ids.get(&id).copied().unwrap_or(1),
                "This replacement introduces a duplicate element ID. Use unique IDs.",
            )?;
        }
        self.finish_element_edit(revision, page_id, &doc, retained, tracked, None)
    }

    pub(crate) fn finish_element_edit(
        &self,
        revision: u64,
        page_id: Uuid,
        doc: &Document,
        retained: Option<NodeRef<'_>>,
        tracked: Vec<(Uuid, NodeRef<'_>)>,
        css: Option<String>,
    ) -> Result<(Self, Option<ElementTarget>)> {
        bounded(doc)?;
        crate::stable_targets::assign_document_ids(doc)?;
        let html = serialized(doc)?;
        ensure(
            serialized(&self.parse_page_html(&html)?)? == html,
            "This edit would be repaired differently by the browser. Choose valid markup for this layout.",
        )?;
        let original = self.page(page_id)?.content.as_ref();
        let content = SiteContent {
            html,
            css: css.unwrap_or_else(|| {
                original
                    .map(|content| content.css.clone())
                    .unwrap_or_default()
            }),
            javascript: original
                .map(|content| content.javascript.clone())
                .unwrap_or_default(),
        };
        content.validate()?;
        let hash = content.sha256()?;
        let body = body(doc)?;
        let mut next = self.clone();
        next.revision = revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        let page = next
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .unwrap();
        page.content = Some(content);
        let mut refreshed = BTreeMap::<Uuid, Vec<ElementTarget>>::new();
        for (id, node) in tracked {
            if let Some(target) = target(node, body, &hash) {
                refreshed.entry(id).or_default().push(target);
            }
        }
        for region in page
            .regions
            .iter_mut()
            .filter(|r| !r.completed && r.has_targets())
        {
            let previous: Vec<_> = region.targets().into_iter().cloned().collect();
            let actual = refreshed.remove(&region.id).unwrap_or_default();
            let lost = actual.is_empty();
            region.elements = previous
                .into_iter()
                .enumerate()
                .map(|(index, old)| {
                    if old.id.is_empty() {
                        actual.get(index).cloned().unwrap_or(old)
                    } else {
                        actual
                            .iter()
                            .find(|new| old.id == new.id)
                            .cloned()
                            .unwrap_or(old)
                    }
                })
                .collect();
            if region.prompt.trim().is_empty()
                && (lost
                    || region
                        .elements
                        .iter()
                        .all(|target| locate(doc, target).is_err()))
            {
                region.completed = true;
            }
            region.element = None;
        }
        let selected = retained.and_then(|node| target(node, body, &hash));
        next.store_inline_images()?;
        next.validate()?;
        Ok((next, selected))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementBounds {
    pub region_id: Uuid,
    pub target: ElementTarget,
    pub rect: crate::Rect,
}
impl Project {
    pub fn refresh_element_bounds(
        &self,
        page_id: Uuid,
        viewport: crate::Viewport,
        bounds: &[ElementBounds],
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
        let hash = page
            .content
            .as_ref()
            .ok_or_else(|| Error("This page has no site.".into()))?
            .sha256()?;
        let mut next = self.clone();
        let page = next
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .unwrap();
        let mut ids = BTreeSet::new();
        let mut changed = false;
        for value in bounds {
            ensure(
                ids.insert(value.region_id),
                "An element position was supplied twice.",
            )?;
            value.rect.validate(viewport)?;
            let region = page
                .regions
                .iter_mut()
                .find(|region| region.id == value.region_id)
                .ok_or_else(|| Error("This request no longer exists.".into()))?;
            ensure(
                !region.completed
                    && region.targets().len() == 1
                    && region.targets()[0].id == value.target.id
                    && (!value.target.id.is_empty() || value.target.source_sha256 == hash),
                "The page changed before its element positions were recorded.",
            )?;
            if region.rect != value.rect {
                region.rect = value.rect;
                changed = true;
            }
        }
        if !changed {
            return Ok(self.clone());
        }
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        next.store_inline_images()?;
        next.validate()?;
        Ok(next)
    }
}
