//! One element change carried to the same element on other pages and framed site files.
//!
//! Matching is structural: the same tag and class list, reached through the same ancestors.
//! Identical markup is preferred, so a shared header wins over a lookalike deep in the page.
use crate::{DocumentTarget, ElementEdit, ElementTarget, Error, Project, Result, TextEdit, ensure};
use dom_query::NodeRef;
use serde::{Deserialize, Serialize};

/// One change across many documents stays bounded like a batch edit.
pub const MAX_ACROSS_PAGES_DOCUMENTS: usize = 200;

/// Another document that contains the selected element.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcrossPagesMatch {
    pub document: DocumentTarget,
    pub target: ElementTarget,
    pub label: String,
    /// The element in the other document had the same markup before this change.
    pub identical_html: bool,
}

/// The element change and the documents the user kept ticked for it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcrossPagesEdit {
    pub document: DocumentTarget,
    pub target: ElementTarget,
    pub edit: ElementEdit,
    /// Empty applies to every match; otherwise only the listed documents change.
    #[serde(default)]
    pub documents: Vec<DocumentTarget>,
}

/// One document the change reached.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcrossPagesChange {
    pub document: DocumentTarget,
    pub target: ElementTarget,
    pub label: String,
    pub removed: bool,
}

/// Tag, class list and ancestor chain of one element, without its identity.
struct ElementShape {
    tag: String,
    classes: String,
    /// Nearest ancestor first, body excluded.
    ancestors: Vec<String>,
    html: String,
}

/// The selected element's text blocks and inline style before the change.
struct Reference {
    texts: Vec<String>,
    style: String,
}

fn classes(node: NodeRef<'_>) -> String {
    let mut names: Vec<_> = node
        .attr("class")
        .map(|value| value.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names.join(" ")
}

fn segment(node: NodeRef<'_>) -> Option<String> {
    Some(format!(
        "{}|{}",
        node.node_name()?.to_ascii_lowercase(),
        classes(node)
    ))
}

fn ancestors(node: NodeRef<'_>) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.node_name().as_deref() {
            Some("body") | Some("html") | None => break,
            Some(_) => chain.push(segment(parent).unwrap_or_default()),
        }
        current = parent.parent();
    }
    chain
}

fn shape(node: NodeRef<'_>) -> Option<ElementShape> {
    let tag = node.node_name()?.to_ascii_lowercase();
    if matches!(
        tag.as_str(),
        "script" | "style" | "link" | "meta" | "template" | "noscript" | "source" | "track"
    ) {
        return None;
    }
    Some(ElementShape {
        tag,
        classes: classes(node),
        ancestors: ancestors(node),
        html: node.html().to_string(),
    })
}

/// The same tag, class list and shared layout position. Extra wrappers above the shared
/// layout are allowed on either side; the nearest ancestors still have to line up.
fn matches_shape(reference: &ElementShape, candidate: &ElementShape) -> bool {
    let shared = reference.ancestors.len().min(candidate.ancestors.len());
    reference.tag == candidate.tag
        && reference.classes == candidate.classes
        && reference.ancestors[..shared] == candidate.ancestors[..shared]
}

fn folder(project: &Project, document: &DocumentTarget) -> String {
    let path = match document {
        DocumentTarget::Page { page_id } => project
            .page(*page_id)
            .ok()
            .map(|page| crate::export::output_path(&page.path)),
        DocumentTarget::SiteFile { path } => Some(path.clone()),
    };
    path.map(|path| {
        path.rsplit_once('/')
            .map_or(String::new(), |(folder, _)| folder.to_string())
    })
    .unwrap_or_default()
}

impl Project {
    pub(crate) fn document_label(&self, document: &DocumentTarget) -> String {
        match document {
            DocumentTarget::Page { page_id } => self
                .page(*page_id)
                .map(|page| page.name.clone())
                .unwrap_or_default(),
            DocumentTarget::SiteFile { path } => path.clone(),
        }
    }

    /// Every document element editing can reach, in a stable order.
    pub(crate) fn editable_documents(&self) -> Vec<DocumentTarget> {
        let mut documents: Vec<_> = self
            .pages
            .iter()
            .filter(|page| page.content.is_some())
            .map(|page| DocumentTarget::Page { page_id: page.id })
            .collect();
        documents.extend(
            self.site_document_paths()
                .into_iter()
                .map(|path| DocumentTarget::SiteFile { path }),
        );
        documents
    }

    fn reference_shape(
        &self,
        document: &DocumentTarget,
        target: &ElementTarget,
    ) -> Result<ElementShape> {
        let content = self.document_content(document)?;
        let doc = self.parse_document_target(document, &content.html)?;
        let node = crate::editing::locate(&doc, target)?;
        ensure(
            node.id != crate::editing::body(&doc)?.id,
            "The page itself has no match on other pages.",
        )?;
        shape(node).ok_or_else(|| Error("This element cannot be matched across pages.".into()))
    }

    /// The selected element's text blocks and inline style, read once per change.
    fn reference(&self, document: &DocumentTarget, target: &ElementTarget) -> Result<Reference> {
        let content = self.document_content(document)?;
        let doc = self.parse_document_target(document, &content.html)?;
        let node = crate::editing::locate(&doc, target)?;
        Ok(Reference {
            texts: crate::editing::text_nodes(node)
                .iter()
                .map(|node| node.text().to_string())
                .collect(),
            style: node
                .attr("style")
                .map(|value| value.to_string())
                .unwrap_or_default(),
        })
    }

    fn match_in_document(
        &self,
        document: &DocumentTarget,
        reference: &ElementShape,
    ) -> Result<Option<AcrossPagesMatch>> {
        let content = self.document_content(document)?;
        let hash = self.document_hash(document)?;
        let doc = self.parse_document_target(document, &content.html)?;
        let body = crate::editing::body(&doc)?;
        let mut best: Option<(u8, NodeRef<'_>)> = None;
        for node in body.descendants_it().filter(|node| node.is_element()) {
            let Some(candidate) = shape(node) else {
                continue;
            };
            if !matches_shape(reference, &candidate) {
                continue;
            }
            let identical = candidate.html == reference.html;
            let same_depth = candidate.ancestors.len() == reference.ancestors.len();
            let score = u8::from(identical) * 2 + u8::from(same_depth);
            if best.is_none_or(|(current, _)| score > current) {
                best = Some((score, node));
            }
        }
        Ok(best.and_then(|(score, node)| {
            crate::editing::target(node, body, &hash).map(|target| AcrossPagesMatch {
                document: document.clone(),
                target,
                label: self.document_label(document),
                identical_html: score >= 2,
            })
        }))
    }

    /// The other pages and framed site files that contain this element. Identical markup and
    /// pages from the same folder are listed first; each other document appears once.
    pub fn element_matches(
        &self,
        document: &DocumentTarget,
        target: &ElementTarget,
    ) -> Result<Vec<AcrossPagesMatch>> {
        let reference = self.reference_shape(document, target)?;
        let home = folder(self, document);
        let mut matches = Vec::new();
        for other in self.editable_documents() {
            if other == *document {
                continue;
            }
            if matches.len() >= MAX_ACROSS_PAGES_DOCUMENTS {
                break;
            }
            if let Some(found) = self.match_in_document(&other, &reference)? {
                matches.push(found);
            }
        }
        matches.sort_by(|left, right| {
            right
                .identical_html
                .cmp(&left.identical_html)
                .then_with(|| {
                    u8::from(folder(self, &right.document) == home)
                        .cmp(&u8::from(folder(self, &left.document) == home))
                })
                .then_with(|| left.label.cmp(&right.label))
        });
        Ok(matches)
    }

    /// Keep a text change only where the other document had the same text before the edit,
    /// and merge a style change into each element's own inline style.
    fn adapted_edit(
        edit: &ElementEdit,
        reference: &Reference,
        current_texts: &[String],
        current_style: &str,
    ) -> Result<ElementEdit> {
        let ElementEdit::Update {
            texts,
            attributes,
            style,
        } = edit
        else {
            return Ok(edit.clone());
        };
        let texts = texts
            .iter()
            .filter(|text| reference.texts.get(text.index) == current_texts.get(text.index))
            .cloned()
            .collect::<Vec<TextEdit>>();
        let style = match style {
            Some(next) => Some(crate::layers::merge_style_changes(
                current_style,
                &reference.style,
                next,
            )?),
            None => None,
        };
        Ok(ElementEdit::Update {
            texts,
            attributes: attributes.clone(),
            style,
        })
    }

    /// Apply one element change to its own document and to the selected matching elements in
    /// other documents, as one revision and one history step.
    pub fn edit_element_across(
        &self,
        revision: u64,
        request: &AcrossPagesEdit,
    ) -> Result<(Self, Vec<AcrossPagesChange>)> {
        self.validate()?;
        ensure(
            revision == self.revision,
            "The project changed. Try the edit again.",
        )?;
        ensure(
            !matches!(request.edit, ElementEdit::LayerStyles { .. }),
            "Layering changes apply to the selected page only."
        )?;
        let matches = self.element_matches(&request.document, &request.target)?;
        let selected = if request.documents.is_empty() {
            matches
        } else {
            matches
                .into_iter()
                .filter(|item| request.documents.contains(&item.document))
                .collect()
        };
        ensure(
            selected.len() <= MAX_ACROSS_PAGES_DOCUMENTS,
            "Change this element on at most 200 other pages at once."
        )?;
        let reference = self.reference(&request.document, &request.target)?;
        let mut next = self.clone();
        next.revision = revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        let targets = std::iter::once((request.document.clone(), request.target.clone())).chain(
            selected
                .iter()
                .map(|item| (item.document.clone(), item.target.clone())),
        );
        let mut changes = Vec::new();
        for (document, target) in targets {
            if let Some(change) =
                self.edit_across_document(&request.edit, &reference, &document, &target, &mut next)?
            {
                changes.push(change);
            }
        }
        if changes.is_empty() {
            return Ok((self.clone(), changes));
        }
        next.store_inline_images()?;
        next.validate()?;
        Ok((next, changes))
    }

    fn edit_across_document(
        &self,
        edit: &ElementEdit,
        reference: &Reference,
        document: &DocumentTarget,
        target: &ElementTarget,
        next: &mut Project,
    ) -> Result<Option<AcrossPagesChange>> {
        let content = next.document_content(document)?;
        let hash = next.document_hash(document)?;
        let doc = next.parse_document_target(document, &content.html)?;
        let before = crate::editing::serialized(&doc)?;
        let before_ids = crate::editing::ids(&doc);
        let tracked = match document {
            DocumentTarget::Page { page_id } => {
                crate::editing::tracked(&doc, self.page(*page_id)?, &hash)?
            }
            DocumentTarget::SiteFile { .. } => vec![],
        };
        ensure(
            !target.id.is_empty() || hash == target.source_sha256,
            "The document changed. Select this element again."
        )?;
        let node = crate::editing::locate(&doc, target)?;
        let current_texts: Vec<_> = crate::editing::text_nodes(node)
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        let current_style = node
            .attr("style")
            .map(|value| value.to_string())
            .unwrap_or_default();
        let adapted = Self::adapted_edit(edit, reference, &current_texts, &current_style)?;
        let retained = crate::editing::apply_element_edit(&doc, node, target, &adapted)?;
        crate::editing::bounded(&doc)?;
        let html = crate::editing::serialized(&doc)?;
        if html == before {
            return Ok(None);
        }
        for (id, count) in crate::editing::ids(&doc) {
            ensure(
                count <= before_ids.get(&id).copied().unwrap_or(1),
                "This replacement introduces a duplicate element ID. Use unique IDs.",
            )?;
        }
        self.store_document_edit(document, &doc, None, tracked, None, next)?;
        let next_hash = next.document_hash(document)?;
        let body = crate::editing::body(&doc)?;
        let target = retained
            .and_then(|node| crate::editing::target(node, body, &next_hash))
            .unwrap_or_else(|| target.clone());
        Ok(Some(AcrossPagesChange {
            document: document.clone(),
            target,
            label: self.document_label(document),
            removed: matches!(adapted, ElementEdit::Remove),
        }))
    }
}
