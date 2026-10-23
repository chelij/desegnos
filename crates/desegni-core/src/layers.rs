use crate::{
    ElementStyleEdit, ElementTarget, Error, Project, Rect, Result,
    editing::{body, bounded, check_value, ids, locate, tracked},
    ensure,
    model::validate_text,
};
use cssparser::{Delimiter, Parser, ParserInput, Token};
use dom_query::{Document, NodeRef, Selection};
use html5ever::{ParseOpts, QualName, tendril::TendrilSink, tree_builder::TreeBuilderOpts};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const MAX_LAYER_CLIPBOARD_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_LAYER_STYLE_EDITS: usize = 512;
pub const MAX_LAYER_STYLE_BYTES: usize = 65_536;
const MAX_LAYER_STYLES_BYTES: usize = 2 * 1024 * 1024;
const HTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const MATH_NAMESPACE: &str = "http://www.w3.org/1998/Math/MathML";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LayerDirection {
    Forward,
    Backward,
    Front,
    Back,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LayerTarget {
    Request { region_id: Uuid },
    Element { target: ElementTarget },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum LayerPseudo {
    Before,
    After,
    Marker,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerPseudoStyle {
    pub pseudo: LayerPseudo,
    pub style: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerVisualStyle {
    pub path: Vec<usize>,
    pub style: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pseudo_styles: Vec<LayerPseudoStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerElementContext {
    pub namespace: String,
    pub parent_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LayerClipboardContent {
    Request {
        rect: Rect,
        prompt: String,
        #[serde(default, skip_serializing_if = "crate::RegionShape::is_rect")]
        shape: crate::RegionShape,
    },
    Element {
        html: String,
        context: LayerElementContext,
        visual_styles: Vec<LayerVisualStyle>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerClipboard {
    pub format: String,
    pub version: u32,
    pub content: LayerClipboardContent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerPasteSelection {
    pub element: Option<ElementTarget>,
    pub region_id: Option<Uuid>,
}

#[derive(Clone)]
struct Declaration {
    name: String,
    value: String,
    important: bool,
}

fn consume_css<'i>(
    parser: &mut Parser<'i, '_>,
) -> std::result::Result<(), cssparser::ParseError<'i, ()>> {
    consume_css_depth(parser, 0)
}

fn consume_css_depth<'i>(
    parser: &mut Parser<'i, '_>,
    depth: usize,
) -> std::result::Result<(), cssparser::ParseError<'i, ()>> {
    if depth > 64 {
        return Err(parser.new_custom_error(()));
    }
    while !parser.is_exhausted() {
        match parser.next_including_whitespace_and_comments()?.clone() {
            Token::BadString(_)
            | Token::BadUrl(_)
            | Token::CloseParenthesis
            | Token::CloseSquareBracket
            | Token::CloseCurlyBracket => return Err(parser.new_custom_error(())),
            Token::Function(_)
            | Token::ParenthesisBlock
            | Token::SquareBracketBlock
            | Token::CurlyBracketBlock => {
                parser.parse_nested_block(|parser| consume_css_depth(parser, depth + 1))?
            }
            _ => {}
        }
    }
    Ok(())
}

fn declarations(style: &str) -> Result<Vec<Declaration>> {
    check_value(style, MAX_LAYER_STYLE_BYTES)?;
    let mut input = ParserInput::new(style);
    let mut parser = Parser::new(&mut input);
    let mut result = Vec::new();
    while !parser.is_exhausted() {
        if parser.try_parse(|parser| parser.expect_semicolon()).is_ok() {
            continue;
        }
        let declaration = parser
            .parse_until_before(Delimiter::Semicolon, |parser| {
                let name = parser.expect_ident_cloned()?.to_string();
                parser.expect_colon()?;
                let start = parser.position();
                let mut significant = Vec::new();
                while !parser.is_exhausted() {
                    let position = parser.position();
                    let token = parser.next_including_whitespace_and_comments()?.clone();
                    match token {
                        Token::WhiteSpace(_) | Token::Comment(_) => {}
                        Token::BadString(_)
                        | Token::BadUrl(_)
                        | Token::CloseParenthesis
                        | Token::CloseSquareBracket
                        | Token::CloseCurlyBracket => return Err(parser.new_custom_error(())),
                        Token::Function(_)
                        | Token::ParenthesisBlock
                        | Token::SquareBracketBlock
                        | Token::CurlyBracketBlock => {
                            parser.parse_nested_block(consume_css)?;
                            significant.push((position, false, false));
                        }
                        Token::Delim('!') => significant.push((position, true, false)),
                        Token::Ident(value) if value.eq_ignore_ascii_case("important") => {
                            significant.push((position, false, true))
                        }
                        _ => significant.push((position, false, false)),
                    }
                }
                let important = significant.len() >= 2
                    && significant[significant.len() - 2].1
                    && significant.last().unwrap().2;
                let end = if important {
                    significant[significant.len() - 2].0
                } else {
                    parser.position()
                };
                let value = parser.slice(start..end).trim().to_string();
                if value.is_empty() {
                    return Err(parser.new_custom_error(()));
                }
                Ok(Declaration {
                    name: if name.starts_with("--") {
                        name
                    } else {
                        name.to_ascii_lowercase()
                    },
                    value,
                    important,
                })
            })
            .map_err(|_: cssparser::ParseError<'_, ()>| {
                Error("This layer contains malformed inline CSS.".into())
            })?;
        result.push(declaration);
    }
    Ok(result)
}

fn render_style(declarations: &[Declaration]) -> String {
    let mut result = String::new();
    for declaration in declarations {
        cssparser::serialize_identifier(&declaration.name, &mut result)
            .expect("String writes cannot fail");
        result.push_str(": ");
        result.push_str(&declaration.value);
        if declaration.important {
            result.push_str(" !important");
        }
        result.push(';');
    }
    result
}

fn visual_style(original: &str, snapshot: &str) -> Result<String> {
    let (mut computed, mut important): (Vec<_>, Vec<_>) = declarations(snapshot)?
        .into_iter()
        .partition(|declaration| !declaration.important);
    important.extend(
        declarations(original)?
            .into_iter()
            .filter(|declaration| declaration.important),
    );
    computed.extend(important);
    // Priority permits normal declarations first, but shorthand/longhand order
    // within each priority must survive. Keep the last exact declaration so an
    // already-baked important snapshot can be merged and validated repeatedly.
    let mut seen = BTreeSet::new();
    computed.reverse();
    computed.retain(|declaration| {
        seen.insert((
            declaration.name.clone(),
            declaration.value.clone(),
            declaration.important,
        ))
    });
    computed.reverse();
    let result = render_style(&computed);
    check_value(&result, MAX_LAYER_STYLE_BYTES)?;
    Ok(result)
}

fn pseudo_bytes(styles: &[LayerPseudoStyle]) -> Result<usize> {
    ensure(
        styles.len() <= 3,
        "Only before, after, and marker styles can be copied.",
    )?;
    let mut seen = BTreeSet::new();
    let mut bytes = 0_usize;
    for style in styles {
        ensure(
            seen.insert(style.pseudo),
            "A pseudo-element style was supplied twice.",
        )?;
        let declarations = declarations(&style.style)?;
        ensure(
            !declarations.is_empty(),
            "A copied pseudo-element has no visual styles.",
        )?;
        if style.pseudo != LayerPseudo::Marker {
            ensure(
                declarations
                    .iter()
                    .rev()
                    .find(|declaration| declaration.name == "content")
                    .is_some_and(|declaration| {
                        !matches!(declaration.value.as_str(), "none" | "normal")
                    }),
                "Copy only visible before and after decorations.",
            )?;
        }
        bytes += style.style.len();
    }
    Ok(bytes)
}

fn pseudo_stylesheet(
    root: NodeRef<'_>,
    styles: &[LayerVisualStyle],
    replacements: &BTreeMap<String, String>,
    original: &str,
) -> Result<Option<String>> {
    if styles.iter().all(|style| style.pseudo_styles.is_empty()) {
        return Ok(None);
    }
    let nodes = element_paths(root)?;
    let mut rules = String::new();
    for style in styles {
        if style.pseudo_styles.is_empty() {
            continue;
        }
        let node = nodes
            .iter()
            .find(|(path, _)| *path == style.path)
            .map(|(_, node)| *node)
            .ok_or_else(|| Error("A copied decoration has no matching element.".into()))?;
        let id = node
            .attr("id")
            .ok_or_else(|| Error("A copied decoration has no unique element ID.".into()))?;
        for pseudo in &style.pseudo_styles {
            let name = match pseudo.pseudo {
                LayerPseudo::Before => "before",
                LayerPseudo::After => "after",
                LayerPseudo::Marker => "marker",
            };
            let mut declarations = declarations(&rewrite_urls(&pseudo.style, replacements)?)?;
            for declaration in &mut declarations {
                declaration.important = true;
            }
            rules.push('#');
            cssparser::serialize_identifier(&id, &mut rules).expect("String writes cannot fail");
            rules.push_str(&format!("::{name}{{{}}}\n", render_style(&declarations)));
        }
    }
    let layer = loop {
        let candidate = format!("desegnos_paste_{}", Uuid::new_v4().simple());
        if !original.contains(&candidate) {
            break candidate;
        }
    };
    let bom = usize::from(original.starts_with('\u{feff}')) * '\u{feff}'.len_utf8();
    let mut input = ParserInput::new(&original[bom..]);
    let mut parser = Parser::new(&mut input);
    let charset_end = if parser
        .try_parse(|parser| {
            match parser.next()? {
                Token::AtKeyword(name) if name.eq_ignore_ascii_case("charset") => {}
                _ => return Err(parser.new_custom_error::<(), ()>(())),
            }
            parser.expect_string()?;
            parser.expect_semicolon()?;
            Ok(parser.position().byte_index())
        })
        .is_ok()
    {
        bom + parser.position().byte_index()
    } else {
        bom
    };
    let (prefix, remainder) = original.split_at(charset_end);
    Ok(Some(format!(
        "{prefix}\n@layer {layer};\n{remainder}\n@layer {layer}{{\n{rules}}}\n"
    )))
}

pub(crate) fn apply_layer_styles(
    doc: &Document,
    selected: NodeRef<'_>,
    target: &ElementTarget,
    styles: &[ElementStyleEdit],
) -> Result<()> {
    ensure(target.selector != "body", "Select a layer inside the page.")?;
    ensure(
        !styles.is_empty() && styles.len() <= MAX_LAYER_STYLE_EDITS,
        "A layer arrangement supports between 1 and 512 style changes.",
    )?;
    let parent = selected
        .parent()
        .ok_or_else(|| Error("This layer has no editable parent.".into()))?;
    let mut nodes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut bytes = 0_usize;
    for style in styles {
        ensure(
            style.pseudo_styles.is_empty(),
            "Pseudo-element styles can only be supplied when copying a layer.",
        )?;
        ensure(
            !style.target.id.is_empty() && !target.id.is_empty()
                || style.target.source_sha256 == target.source_sha256,
            "A layer changed. Select it again.",
        )?;
        let node = locate(doc, &style.target)?;
        ensure(seen.insert(node.id), "A layer style was supplied twice.")?;
        ensure(
            node.id == parent.id || node.parent().is_some_and(|value| value.id == parent.id),
            "Only this layer, its siblings, and their parent may be arranged together.",
        )?;
        check_value(&style.style, MAX_LAYER_STYLE_BYTES)?;
        declarations(&style.style)?;
        bytes += style.style.len();
        ensure(
            bytes <= MAX_LAYER_STYLES_BYTES,
            "The layer styles exceed the 2 MiB limit.",
        )?;
        nodes.push((node, &style.style));
    }
    ensure(
        seen.contains(&selected.id),
        "The selected layer must be included in its arrangement.",
    )?;
    for (node, style) in nodes {
        if style.is_empty() {
            node.remove_attr("style");
        } else {
            node.set_attr("style", style);
        }
    }
    Ok(())
}

fn element_paths(root: NodeRef<'_>) -> Result<Vec<(Vec<usize>, NodeRef<'_>)>> {
    let mut result = Vec::new();
    let mut pending = vec![(vec![], root)];
    while let Some((path, node)) = pending.pop() {
        ensure(
            result.len() < MAX_LAYER_STYLE_EDITS && path.len() <= 64,
            "This layer is too large to copy. Copy a smaller visual layer.",
        )?;
        for (index, child) in node.element_children().into_iter().enumerate().rev() {
            let mut child_path = path.clone();
            child_path.push(index);
            pending.push((child_path, child));
        }
        result.push((path, node));
    }
    Ok(result)
}

fn static_subtree(root: NodeRef<'_>) -> Result<()> {
    for (_, node) in element_paths(root)? {
        ensure(
            !matches!(
                node.node_name().as_deref(),
                Some("script" | "style" | "iframe" | "object" | "embed")
            ),
            "This layer contains an embedded script, stylesheet, or document. Copy a smaller visual layer with static content.",
        )?;
    }
    Ok(())
}

fn context_name(context: &LayerElementContext) -> Result<QualName> {
    ensure(
        [HTML_NAMESPACE, SVG_NAMESPACE, MATH_NAMESPACE].contains(&context.namespace.as_str())
            && !context.parent_tag.is_empty()
            && context.parent_tag.len() <= 80
            && context
                .parent_tag
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || value == b'-'),
        "The copied layer has an invalid HTML context.",
    )?;
    Ok(QualName::new(
        None,
        context.namespace.as_str().into(),
        context.parent_tag.as_str().into(),
    ))
}

fn fragment(html: &str, context: QualName) -> Result<Document> {
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
    .one(html);
    bounded(&fragment)?;
    Ok(fragment)
}

fn fragment_root(doc: &Document) -> Result<NodeRef<'_>> {
    let container = doc
        .root()
        .first_child()
        .ok_or_else(|| Error("The clipboard has no layer.".into()))?;
    let children = container
        .children()
        .into_iter()
        .filter(|node| node.is_element() || node.is_text() && !node.text().trim().is_empty())
        .collect::<Vec<_>>();
    ensure(
        children.len() == 1 && children[0].is_element(),
        "The clipboard must contain exactly one visual layer.",
    )?;
    Ok(children[0])
}

fn qual_name(node: NodeRef<'_>) -> Result<QualName> {
    node.query(|node| node.as_element().map(|element| element.name.clone()))
        .flatten()
        .ok_or_else(|| Error("This layer has an invalid element context.".into()))
}

fn unique_ids(doc: &Document) -> Result<()> {
    ensure(
        ids(doc).values().all(|count| *count == 1),
        "This source contains duplicate element IDs. Make the IDs unique before copying or pasting layers.",
    )
}

impl LayerClipboard {
    pub fn from_json(source: &str) -> Result<Self> {
        check_value(source, MAX_LAYER_CLIPBOARD_BYTES)?;
        let clipboard: Self = serde_json::from_str(source).map_err(|_| {
            Error("The clipboard does not contain a supported Desegnos layer.".into())
        })?;
        clipboard.validate()?;
        Ok(clipboard)
    }

    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|error| Error(error.to_string()))
    }

    pub fn validate(&self) -> Result<()> {
        ensure(
            self.format == "desegnos-layer" && self.version == 1,
            "The clipboard layer format or version is unsupported.",
        )?;
        match &self.content {
            LayerClipboardContent::Request {
                rect,
                prompt,
                shape,
            } => {
                shape.validate()?;
                rect.validate(crate::Viewport {
                    width: 1,
                    height: 1,
                })?;
                validate_text(prompt, "Copied request", crate::MAX_PROMPT_CHARS, false)?;
            }
            LayerClipboardContent::Element {
                html,
                context,
                visual_styles,
            } => {
                check_value(html, crate::MAX_SITE_SOURCE_BYTES)?;
                let doc = fragment(html, context_name(context)?)?;
                let root = fragment_root(&doc)?;
                ensure(
                    root.node_name().as_deref() != Some("body"),
                    "Copy an item inside the page, not the page itself.",
                )?;
                ensure(
                    root.try_html().is_some_and(|value| value.as_ref() == html),
                    "The clipboard markup would be repaired by the browser. Copy a valid visual layer.",
                )?;
                unique_ids(&doc)?;
                static_subtree(root)?;
                let nodes = element_paths(root)?;
                ensure(
                    visual_styles.len() == nodes.len(),
                    "The copied visual styles must cover the complete layer.",
                )?;
                let mut seen = BTreeSet::new();
                let mut bytes = 0_usize;
                for style in visual_styles {
                    ensure(
                        seen.insert(&style.path),
                        "A copied visual style was supplied twice.",
                    )?;
                    let node = nodes
                        .iter()
                        .find(|(path, _)| *path == style.path)
                        .map(|(_, node)| *node)
                        .ok_or_else(|| {
                            Error("A copied visual style refers outside its layer.".into())
                        })?;
                    bytes += style.style.len() + pseudo_bytes(&style.pseudo_styles)?;
                    ensure(
                        bytes <= MAX_LAYER_STYLES_BYTES,
                        "The copied styles exceed the 2 MiB limit.",
                    )?;
                    let inline = node
                        .attr("style")
                        .map(|value| value.to_string())
                        .unwrap_or_default();
                    ensure(
                        visual_style(&inline, &style.style)? == inline,
                        "The copied visual styles disagree with its HTML.",
                    )?;
                }
            }
        }
        let bytes = serde_json::to_vec(self).map_err(|error| Error(error.to_string()))?;
        ensure(
            bytes.len() <= MAX_LAYER_CLIPBOARD_BYTES,
            "The copied layer exceeds the 8 MiB clipboard limit.",
        )
    }
}

impl Project {
    pub fn copy_layer(
        &self,
        page_id: Uuid,
        selected: &LayerTarget,
        visual_styles: &[ElementStyleEdit],
    ) -> Result<LayerClipboard> {
        self.copy_layer_in(
            &crate::DocumentTarget::Page { page_id },
            selected,
            visual_styles,
        )
    }
    pub fn copy_layer_in(
        &self,
        document: &crate::DocumentTarget,
        selected: &LayerTarget,
        visual_styles: &[ElementStyleEdit],
    ) -> Result<LayerClipboard> {
        self.validate()?;
        let content = match selected {
            LayerTarget::Request { region_id } => {
                ensure(
                    visual_styles.is_empty(),
                    "A request copy cannot include site element styles.",
                )?;
                let crate::DocumentTarget::Page { page_id } = document else {
                    return Err(Error(
                        "Requests belong to a page, not a framed document.".into(),
                    ));
                };
                let page = self.page(*page_id)?;
                let region = page
                    .regions
                    .iter()
                    .find(|region| region.id == *region_id)
                    .ok_or_else(|| Error("This request no longer exists.".into()))?;
                LayerClipboardContent::Request {
                    rect: region.rect,
                    prompt: region.prompt.clone(),
                    shape: region.shape.clone(),
                }
            }
            LayerTarget::Element { target } => {
                ensure(
                    target.selector != "body",
                    "Copy an item inside the page, not the page itself.",
                )?;
                let source = self.document_content(document)?;
                ensure(
                    !target.id.is_empty() || self.document_hash(document)? == target.source_sha256,
                    "The document changed. Select this layer again.",
                )?;
                let doc = self.parse_document_target(document, &source.html)?;
                unique_ids(&doc)?;
                let root = locate(&doc, target)?;
                static_subtree(root)?;
                let parent = root
                    .parent()
                    .ok_or_else(|| Error("This layer has no editable parent.".into()))?;
                let parent_name = qual_name(parent)?;
                let nodes = element_paths(root)?;
                ensure(
                    visual_styles.len() == nodes.len(),
                    "Capture the current visual styles for this layer and every item inside it.",
                )?;
                let mut seen = BTreeSet::new();
                let mut snapshots = Vec::new();
                let mut bytes = 0_usize;
                for style in visual_styles {
                    ensure(
                        !style.target.id.is_empty() && !target.id.is_empty()
                            || style.target.source_sha256 == target.source_sha256,
                        "The page changed while copying this layer. Select it again.",
                    )?;
                    let node = locate(&doc, &style.target)?;
                    ensure(
                        seen.insert(node.id),
                        "A copied visual style was supplied twice.",
                    )?;
                    let path = nodes
                        .iter()
                        .find(|(_, descendant)| descendant.id == node.id)
                        .map(|(path, _)| path.clone())
                        .ok_or_else(|| {
                            Error("A copied visual style refers outside the selected layer.".into())
                        })?;
                    let original = node
                        .attr("style")
                        .map(|value| value.to_string())
                        .unwrap_or_default();
                    let baked = visual_style(&original, &style.style)?;
                    bytes += style.style.len() + pseudo_bytes(&style.pseudo_styles)?;
                    ensure(
                        bytes <= MAX_LAYER_STYLES_BYTES,
                        "The copied styles exceed the 2 MiB limit.",
                    )?;
                    if baked.is_empty() {
                        node.remove_attr("style");
                    } else {
                        node.set_attr("style", &baked);
                    }
                    snapshots.push(LayerVisualStyle {
                        path,
                        style: style.style.clone(),
                        pseudo_styles: style.pseudo_styles.clone(),
                    });
                }
                snapshots.sort_by(|left, right| left.path.cmp(&right.path));
                LayerClipboardContent::Element {
                    html: self
                        .resolve_site_content(&crate::SiteContent {
                            html: root
                                .try_html()
                                .ok_or_else(|| Error("This layer could not be copied.".into()))?
                                .to_string(),
                            css: String::new(),
                            javascript: String::new(),
                        })?
                        .html,
                    context: LayerElementContext {
                        namespace: parent_name.ns.to_string(),
                        parent_tag: parent_name.local.to_string(),
                    },
                    visual_styles: snapshots,
                }
            }
        };
        let clipboard = LayerClipboard {
            format: "desegnos-layer".into(),
            version: 1,
            content,
        };
        clipboard.validate()?;
        Ok(clipboard)
    }

    pub fn paste_layer(
        &self,
        revision: u64,
        page_id: Uuid,
        clipboard: &LayerClipboard,
        insertion: Option<&ElementTarget>,
    ) -> Result<(Self, LayerPasteSelection)> {
        self.paste_layer_in(
            revision,
            &crate::DocumentTarget::Page { page_id },
            clipboard,
            insertion,
        )
    }
    pub fn paste_layer_in(
        &self,
        revision: u64,
        document: &crate::DocumentTarget,
        clipboard: &LayerClipboard,
        insertion: Option<&ElementTarget>,
    ) -> Result<(Self, LayerPasteSelection)> {
        self.validate()?;
        ensure(
            revision == self.revision,
            "The project changed. Try pasting again.",
        )?;
        clipboard.validate()?;
        let viewport = self.document_viewport(document)?;
        if let Some(insertion) = insertion {
            self.inspect_element_in(document, insertion)?;
        }
        let host_page_id = match document {
            crate::DocumentTarget::Page { page_id } => Some(*page_id),
            crate::DocumentTarget::SiteFile { .. } => None,
        };
        if let LayerClipboardContent::Request {
            rect,
            prompt,
            shape,
        } = &clipboard.content
        {
            let page_id = host_page_id
                .ok_or_else(|| Error("Requests belong to a page, not a framed document.".into()))?;
            let mut rect = *rect;
            rect.x += 24.0;
            rect.y += 24.0;
            rect.validate(viewport)?;
            let id = Uuid::new_v4();
            let mut next = self.clone();
            next.revision = revision
                .checked_add(1)
                .ok_or_else(|| Error("Project revision limit reached.".into()))?;
            let page = next
                .pages
                .iter_mut()
                .find(|page| page.id == page_id)
                .unwrap();
            let mut order = page
                .regions
                .iter()
                .enumerate()
                .filter(|(_, region)| !region.completed)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            order.sort_by_key(|index| page.regions[*index].z_index);
            order.push(page.regions.len());
            page.regions.push(crate::Region {
                id,
                rect,
                shape: shape.clone(),
                area_bounds: vec![],
                prompt: prompt.clone(),
                z_index: 0,
                element: None,
                frame_target: None,
                elements: vec![],
                allow_target_removal: false,
                mobile_only: false,
                runtime_errors: vec![],
                whole_page: false,
                whole_page_disabled: false,
                attachments: vec![],
                completed: false,
            });
            for (rank, index) in order.into_iter().enumerate() {
                page.regions[index].z_index = i32::try_from(rank)
                    .map_err(|_| Error("This page has too many request layers.".into()))?;
            }
            next.store_inline_images()?;
            next.validate()?;
            return Ok((
                next,
                LayerPasteSelection {
                    element: None,
                    region_id: Some(id),
                },
            ));
        }
        let LayerClipboardContent::Element {
            html,
            context,
            visual_styles,
        } = &clipboard.content
        else {
            unreachable!()
        };
        let source = match self.document_content(document) {
            Ok(content) => content,
            Err(_) if matches!(document, crate::DocumentTarget::Page { .. }) => {
                crate::SiteContent {
                    html: String::new(),
                    css: String::new(),
                    javascript: String::new(),
                }
            }
            Err(error) => return Err(error),
        };
        let doc = self.parse_document_target(document, &source.html)?;
        unique_ids(&doc)?;
        let body = body(&doc)?;
        let anchor = insertion.map(|target| locate(&doc, target)).transpose()?;
        let parent = match anchor {
            Some(node) if node.id != body.id => node
                .parent()
                .ok_or_else(|| Error("This insertion point has no parent.".into()))?,
            _ => body,
        };
        let source_fragment = fragment(html, context_name(context)?)?;
        let source_root = fragment_root(&source_fragment)?;
        let incoming = fragment(html, qual_name(parent)?)?;
        let root = fragment_root(&incoming)?;
        ensure(
            qual_name(root)? == qual_name(source_root)?
                && root.try_html().is_some_and(|value| value.as_ref() == html),
            "This layer cannot be pasted into that layout without changing its markup. Select a compatible sibling or paste on the empty page.",
        )?;
        let hash = self.document_hash(document).ok();
        let tracked = match (host_page_id, hash.as_deref()) {
            (Some(page_id), Some(hash)) => tracked(&doc, self.page(page_id)?, hash)?,
            _ => vec![],
        };
        let copied_nodes: Vec<_> = std::iter::once(root).chain(root.descendants_it()).collect();
        for node in copied_nodes {
            node.remove_attr(crate::ELEMENT_ID_ATTRIBUTE);
        }
        let replacements = rewrite_ids(root, &ids(&doc), visual_styles)?;
        let css = pseudo_stylesheet(
            root,
            visual_styles,
            &replacements,
            match document {
                crate::DocumentTarget::Page { page_id } => self
                    .page(*page_id)?
                    .content
                    .as_ref()
                    .map(|content| content.css.as_str())
                    .unwrap_or_default(),
                crate::DocumentTarget::SiteFile { .. } => "",
            },
        )?;
        offset_root(
            root,
            visual_styles
                .iter()
                .find(|style| style.path.is_empty())
                .ok_or_else(|| Error("The copied root style is missing.".into()))?,
        )?;
        Selection::from(parent).append_selection(&Selection::from(root));
        let pasted = parent
            .last_child()
            .ok_or_else(|| Error("The layer could not be inserted.".into()))?;
        if let Some(anchor) = anchor
            && anchor.id != body.id
        {
            pasted.remove_from_parent();
            anchor.insert_after(&pasted);
        }
        unique_ids(&doc)?;
        let (next, selected) =
            self.finish_document_edit(document, revision, &doc, Some(pasted), tracked, css)?;
        let selected =
            selected.ok_or_else(|| Error("The pasted layer has no selectable element.".into()))?;
        Ok((
            next,
            LayerPasteSelection {
                element: Some(selected),
                region_id: None,
            },
        ))
    }
}

fn offset_root(root: NodeRef<'_>, snapshot: &LayerVisualStyle) -> Result<()> {
    let computed = declarations(&snapshot.style)?;
    let translate = computed
        .iter()
        .rev()
        .find(|declaration| declaration.name == "translate")
        .map(|declaration| declaration.value.as_str())
        .unwrap_or("none");
    let mut parts = Vec::new();
    if translate != "none" {
        let mut input = ParserInput::new(translate);
        let mut parser = Parser::new(&mut input);
        while !parser.is_exhausted() {
            parser.skip_whitespace();
            let start = parser.position();
            let token = parser
                .next()
                .map_err(|_| Error("The copied layer has an invalid translation.".into()))?
                .clone();
            if matches!(token, Token::Function(_) | Token::ParenthesisBlock) {
                parser
                    .parse_nested_block(consume_css)
                    .map_err(|_| Error("The copied layer has an invalid translation.".into()))?;
            }
            parts.push(parser.slice_from(start).to_string());
        }
        ensure(
            (1..=3).contains(&parts.len()),
            "The copied layer has an unsupported translation.",
        )?;
    }
    let x = parts.first().map(String::as_str).unwrap_or("0px");
    let y = parts.get(1).map(String::as_str).unwrap_or("0px");
    let z = parts
        .get(2)
        .map(|value| format!(" {value}"))
        .unwrap_or_default();
    let mut style = declarations(
        &root
            .attr("style")
            .map(|value| value.to_string())
            .unwrap_or_default(),
    )?;
    let important = style
        .iter()
        .any(|declaration| declaration.name == "translate" && declaration.important);
    style.retain(|declaration| declaration.name != "translate");
    style.push(Declaration {
        name: "translate".into(),
        value: format!("calc({x} + 24px) calc({y} + 24px){z}"),
        important,
    });
    let style = render_style(&style);
    check_value(&style, MAX_LAYER_STYLE_BYTES)?;
    root.set_attr("style", &style);
    Ok(())
}

fn decoded_fragment(value: &str) -> Option<String> {
    let value = value.strip_prefix('#')?;
    let mut result = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            result.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            result.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(result).ok()
}

fn rewrite_ids(
    root: NodeRef<'_>,
    destination: &BTreeMap<String, usize>,
    styles: &[LayerVisualStyle],
) -> Result<BTreeMap<String, String>> {
    let nodes = element_paths(root)?;
    let mut replacements = BTreeMap::new();
    let mut used: BTreeSet<_> = destination.keys().cloned().collect();
    for (path, node) in &nodes {
        let original_id = node.attr("id");
        if original_id.is_some()
            || styles
                .iter()
                .any(|style| style.path == *path && !style.pseudo_styles.is_empty())
        {
            let new_id = loop {
                let candidate = format!("desegnos-copy-{}", Uuid::new_v4().simple());
                if used.insert(candidate.clone()) {
                    break candidate;
                }
            };
            if let Some(id) = original_id {
                replacements.insert(id.to_string(), new_id.clone());
            }
            node.set_attr("id", &new_id);
        }
    }
    for (_, node) in nodes {
        for attribute in node.attrs() {
            let name = attribute.name.local.as_ref();
            if name == "id" {
                continue;
            }
            let value = attribute.value.as_ref();
            let rewritten = if name == "href" {
                decoded_fragment(value)
                    .and_then(|id| replacements.get(&id))
                    .map(|id| format!("#{id}"))
                    .unwrap_or_else(|| value.to_string())
            } else if [
                "for",
                "form",
                "list",
                "headers",
                "itemref",
                "aria-activedescendant",
                "aria-labelledby",
                "aria-describedby",
                "aria-controls",
                "aria-owns",
                "aria-flowto",
                "aria-details",
                "aria-errormessage",
                "popovertarget",
                "commandfor",
            ]
            .contains(&name)
            {
                value
                    .split_ascii_whitespace()
                    .map(|id| replacements.get(id).map(String::as_str).unwrap_or(id))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else if name == "style"
                || [
                    "fill",
                    "stroke",
                    "filter",
                    "clip-path",
                    "mask",
                    "marker-start",
                    "marker-mid",
                    "marker-end",
                    "cursor",
                ]
                .contains(&name)
            {
                rewrite_urls(value, &replacements)?
            } else {
                continue;
            };
            if rewritten != value {
                node.update(|node| {
                    if let Some(element) = node.as_element_mut()
                        && let Some(original) = element
                            .attrs
                            .iter_mut()
                            .find(|original| original.name == attribute.name)
                    {
                        original.value = rewritten.as_str().into();
                    }
                });
            }
        }
    }
    Ok(replacements)
}

fn rewrite_urls(source: &str, replacements: &BTreeMap<String, String>) -> Result<String> {
    fn walk(
        parser: &mut Parser<'_, '_>,
        replacements: &BTreeMap<String, String>,
        edits: &mut Vec<(usize, usize, String)>,
        depth: usize,
    ) -> Result<()> {
        ensure(depth <= 64, "A copied CSS reference is nested too deeply.")?;
        while !parser.is_exhausted() {
            let start = parser.position().byte_index();
            let token = parser
                .next_including_whitespace_and_comments()
                .map_err(|_| Error("A copied CSS reference is malformed.".into()))?
                .clone();
            let url = match token {
                Token::UnquotedUrl(value) => Some(value.to_string()),
                Token::Function(name) if name.eq_ignore_ascii_case("url") => Some(
                    parser
                        .parse_nested_block(|parser| {
                            parser
                                .expect_string_cloned()
                                .map_err(Into::<cssparser::ParseError<'_, ()>>::into)
                        })
                        .map_err(|_| Error("A copied CSS URL is malformed.".into()))?
                        .to_string(),
                ),
                Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock => {
                    parser
                        .parse_nested_block(|parser| {
                            walk(parser, replacements, edits, depth + 1)
                                .map_err(|_| parser.new_custom_error::<(), ()>(()))
                        })
                        .map_err(|_| Error("A copied CSS reference is malformed.".into()))?;
                    None
                }
                _ => None,
            };
            if let Some(url) = url
                && let Some(id) = decoded_fragment(&url).and_then(|id| replacements.get(&id))
            {
                let mut replacement = "url(".to_string();
                cssparser::serialize_string(&format!("#{id}"), &mut replacement)
                    .expect("String writes cannot fail");
                replacement.push(')');
                edits.push((start, parser.position().byte_index(), replacement));
            }
        }
        Ok(())
    }
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let mut edits = Vec::new();
    walk(&mut parser, replacements, &mut edits, 0)?;
    let mut result = source.to_string();
    for (start, end, value) in edits.into_iter().rev() {
        result.replace_range(start..end, &value);
    }
    Ok(result)
}
