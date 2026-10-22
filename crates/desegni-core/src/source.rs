//! Only editor ID attributes are inserted; authored formatting and code stay intact.
//! Parsers inspect source; they never execute it.
use crate::{Error, Project, SiteContent};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use swc_common::{DUMMY_SP, FileName, SourceMap, Span, Spanned};
use swc_html_ast::{Child, DocumentMode, Element, Namespace};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageSourceEdit {
    pub html: Option<String>,
    pub css: Option<String>,
    pub javascript: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDiagnostic {
    pub language: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEditError {
    pub message: String,
    pub diagnostics: Vec<SourceDiagnostic>,
}
pub type SourceEditResult<T> = std::result::Result<T, SourceEditError>;
impl SourceEditError {
    pub fn general(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            message: message.clone(),
            diagnostics: vec![SourceDiagnostic {
                language: "project".into(),
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 1,
                message,
            }],
        }
    }
    fn from_diagnostics(diagnostics: Vec<SourceDiagnostic>) -> Self {
        let first = &diagnostics[0];
        Self {
            message: format!("{} line {}: {}", first.language, first.line, first.message),
            diagnostics,
        }
    }
}
impl From<Error> for SourceEditError {
    fn from(error: Error) -> Self {
        Self::general(error.0)
    }
}
impl std::fmt::Display for SourceEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for SourceEditError {}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementSourceRange {
    pub id: String,
    pub selector: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_column: usize,
    pub end_column: usize,
    pub implicit: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSourceMap {
    pub project_id: Uuid,
    pub page_id: Uuid,
    pub revision: u64,
    pub source_sha256: String,
    pub html_sha256: String,
    pub ranges: Vec<ElementSourceRange>,
}

struct Positions<'a> {
    text: &'a str,
    lines: Vec<usize>,
    unicode: Vec<(usize, usize)>,
}
impl<'a> Positions<'a> {
    fn new(text: &'a str) -> Self {
        let mut lines = vec![0];
        lines.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        let mut delta = 0;
        let unicode = text
            .char_indices()
            .filter_map(|(offset, character)| {
                if character.len_utf8() > 1 {
                    delta += character.len_utf8() - character.len_utf16();
                    Some((offset + character.len_utf8(), delta))
                } else {
                    None
                }
            })
            .collect();
        Self {
            text,
            lines,
            unicode,
        }
    }
    fn utf16(&self, offset: usize) -> usize {
        let index = self.unicode.partition_point(|(end, _)| *end <= offset);
        offset
            - self
                .unicode
                .get(index.wrapping_sub(1))
                .map_or(0, |(_, delta)| *delta)
    }
    fn get(&self, offset: usize) -> (usize, usize) {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.lines.partition_point(|start| *start <= offset) - 1;
        (
            line + 1,
            self.utf16(offset) - self.utf16(self.lines[line]) + 1,
        )
    }
}
fn position(text: &str, offset: usize) -> (usize, usize) {
    Positions::new(text).get(offset)
}

fn diagnostic(
    text: &str,
    language: &str,
    span: Span,
    base: u32,
    message: impl Into<String>,
) -> SourceDiagnostic {
    let (line, column) = position(text, span.lo.0.saturating_sub(base) as usize);
    let (end_line, end_column) = position(text, span.hi.0.saturating_sub(base) as usize);
    SourceDiagnostic {
        language: language.into(),
        line,
        column,
        end_line,
        end_column,
        message: message.into(),
    }
}
fn parse_html(text: &str) -> (Vec<Child>, Vec<SourceDiagnostic>, u32) {
    let map = SourceMap::default();
    let file = map.new_source_file(FileName::Custom("page.html".into()).into(), text.to_owned());
    let mut errors = vec![];
    let config = swc_html_parser::parser::ParserConfig {
        scripting_enabled: true,
        ..Default::default()
    };
    let full = text.to_ascii_lowercase().contains("<html")
        || text.to_ascii_lowercase().contains("<!doctype");
    let result = if full {
        swc_html_parser::parse_file_as_document(&file, config, &mut errors)
            .map(|document| document.children)
    } else {
        let context = Element {
            span: DUMMY_SP,
            tag_name: "body".into(),
            namespace: Namespace::HTML,
            attributes: vec![],
            children: vec![],
            content: None,
            is_self_closing: false,
        };
        swc_html_parser::parse_file_as_document_fragment(
            &file,
            &context,
            DocumentMode::NoQuirks,
            None,
            config,
            &mut errors,
        )
        .map(|fragment| fragment.children)
    };
    let children = match result {
        Ok(children) => children,
        Err(error) => {
            errors.push(error);
            vec![]
        }
    };
    let diagnostics = errors
        .into_iter()
        .filter(|error| {
            !matches!(
                error.kind(),
                swc_html_parser::error::ErrorKind::StartTagWithoutDoctype
                    | swc_html_parser::error::ErrorKind::EndTagSeenWithoutDoctype
                    | swc_html_parser::error::ErrorKind::NonSpaceCharacterWithoutDoctype
                    | swc_html_parser::error::ErrorKind::EofWithoutDoctype
            )
        })
        .take(64)
        .map(|error| {
            let message = error.message().to_string();
            diagnostic(
                text,
                "html",
                error.into_inner().0,
                file.start_pos.0,
                message,
            )
        })
        .collect();
    (children, diagnostics, file.start_pos.0)
}
fn validate_javascript(
    text: &str,
    language: &str,
    offset: usize,
    source: &str,
    diagnostics: &mut Vec<SourceDiagnostic>,
    module: bool,
) {
    let map = SourceMap::default();
    let file = map.new_source_file(FileName::Custom("page.js".into()).into(), text.to_owned());
    let mut errors = vec![];
    let result = if module {
        swc_ecma_parser::parse_file_as_module(
            &file,
            swc_ecma_parser::Syntax::Es(Default::default()),
            swc_ecma_ast::EsVersion::latest(),
            None,
            &mut errors,
        )
        .map(|_| ())
    } else {
        swc_ecma_parser::parse_file_as_script(
            &file,
            swc_ecma_parser::Syntax::Es(Default::default()),
            swc_ecma_ast::EsVersion::latest(),
            None,
            &mut errors,
        )
        .map(|_| ())
    };
    if let Err(error) = result {
        errors.push(error);
    }
    for error in errors.into_iter().take(64 - diagnostics.len().min(64)) {
        let span = Span::new(
            swc_common::BytePos(error.span().lo.0.saturating_sub(file.start_pos.0) + offset as u32),
            swc_common::BytePos(error.span().hi.0.saturating_sub(file.start_pos.0) + offset as u32),
        );
        diagnostics.push(diagnostic(
            source,
            language,
            span,
            0,
            error.kind().msg().to_string(),
        ));
    }
}
fn check_css_tokens<'i>(
    input: &mut cssparser::Parser<'i, '_>,
    depth: usize,
) -> std::result::Result<(), cssparser::ParseError<'i, &'static str>> {
    if depth > 128 {
        return Err(input.new_custom_error("CSS nesting is too deep."));
    }
    while !input.is_exhausted() {
        let start = input.position();
        let token = input.next_including_whitespace_and_comments()?.clone();
        if token.is_parse_error() {
            return Err(input.new_custom_error("Invalid CSS token."));
        }
        let closing = match token {
            cssparser::Token::Function(_) | cssparser::Token::ParenthesisBlock => Some(')'),
            cssparser::Token::SquareBracketBlock => Some(']'),
            cssparser::Token::CurlyBracketBlock => Some('}'),
            cssparser::Token::QuotedString(_) => {
                let raw = input.slice_from(start);
                let quote = raw.chars().next().unwrap_or(' ');
                let escaped = raw[..raw.len().saturating_sub(1)]
                    .bytes()
                    .rev()
                    .take_while(|b| *b == b'\\')
                    .count()
                    % 2
                    != 0;
                if raw.len() < 2 || !raw.ends_with(quote) || escaped {
                    return Err(input.new_custom_error("Close this CSS string."));
                }
                None
            }
            _ => None,
        };
        if let Some(closing) = closing {
            input.parse_nested_block(|nested| check_css_tokens(nested, depth + 1))?;
            if !input.slice_from(start).ends_with(closing) {
                return Err(input.new_custom_error("Close this CSS block."));
            }
        }
    }
    Ok(())
}

fn validate_css(
    text: &str,
    language: &str,
    offset: usize,
    source: &str,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if diagnostics.len() >= 64 {
        return;
    }
    let mut input = cssparser::ParserInput::new(text);
    if let Err(error) = check_css_tokens(&mut cssparser::Parser::new(&mut input), 0) {
        let line = error.location.line as usize + 1;
        let column = error.location.column as usize;
        let base_line = position(source, offset).0;
        let base_column = if line == 1 {
            position(source, offset).1 - 1
        } else {
            0
        };
        diagnostics.push(SourceDiagnostic {
            language: language.into(),
            line: base_line + line - 1,
            column: column + base_column,
            end_line: base_line + line - 1,
            end_column: column + base_column + 1,
            message: "Close the CSS string or block and correct invalid tokens.".into(),
        });
        return;
    }
    if let Err(error) = lightningcss::stylesheet::StyleSheet::parse(
        text,
        lightningcss::stylesheet::ParserOptions::default(),
    ) {
        let (line, column) = error
            .loc
            .as_ref()
            .map_or((1, 1), |loc| (loc.line as usize + 1, loc.column as usize));
        let mut lines = text.split_inclusive('\n');
        let byte_offset: usize = lines
            .by_ref()
            .take(line.saturating_sub(1))
            .map(str::len)
            .sum();
        let line_text = lines.next().unwrap_or("");
        let mut units = 0;
        let mut within = line_text.len();
        for (index, character) in line_text.char_indices() {
            if units >= column.saturating_sub(1) {
                within = index;
                break;
            }
            units += character.len_utf16();
        }
        let at = (offset + byte_offset + within).min(source.len());
        let (line, column) = position(source, at);
        diagnostics.push(SourceDiagnostic {
            language: language.into(),
            line,
            column,
            end_line: line,
            end_column: column + 1,
            message: format!("{}", error.kind),
        });
    }
}

pub fn validate_page_source(content: &SiteContent) -> SourceEditResult<()> {
    validate_page_source_for_website(content, false)
}
fn validate_page_source_for_website(content: &SiteContent, imported: bool) -> SourceEditResult<()> {
    content.validate()?;
    content.found_sources()?;
    // Bound the actual browser DOM before traversing a parser AST.
    crate::editing::parse(&content.html)?;
    let (children, mut diagnostics, base) = parse_html(&content.html);
    let mut pending: Vec<_> = children.iter().map(|node| (node, 0)).collect();
    let mut count = 0;
    while let Some((child, depth)) = pending.pop() {
        count += 1;
        if count > 20_000 || depth > 128 {
            return Err(SourceEditError::general(
                "This page is too complex to edit. Split it into smaller pages.",
            ));
        }
        let Child::Element(element) = child else {
            continue;
        };
        let tag = element.tag_name.as_ref();
        if tag == "script" {
            if element.attributes.iter().any(|attr| attr.name == "src") {
                if !imported {
                    diagnostics.push(diagnostic(&content.html,"html",element.span,base,"Remote and imported scripts are unavailable. Keep browser JavaScript in this page."));
                }
            } else if !element.attributes.iter().any(|attr| {
                attr.name == "type"
                    && attr
                        .value
                        .as_deref()
                        .is_some_and(|value| value.contains("json"))
            }) {
                for child in &element.children {
                    if let Child::Text(text) = child {
                        validate_javascript(
                            &text.data,
                            "html",
                            text.span.lo.0.saturating_sub(base) as usize,
                            &content.html,
                            &mut diagnostics,
                            imported
                                && element.attributes.iter().any(|attr| {
                                    attr.name == "type" && attr.value.as_deref() == Some("module")
                                }),
                        );
                    }
                }
            }
        }
        if tag == "style" {
            for child in &element.children {
                if let Child::Text(text) = child {
                    validate_css(
                        &text.data,
                        "html",
                        text.span.lo.0.saturating_sub(base) as usize,
                        &content.html,
                        &mut diagnostics,
                    );
                }
            }
        }
        for attr in &element.attributes {
            if let Some(value) = &attr.value {
                if matches!(
                    attr.name.as_ref(),
                    "href" | "src" | "action" | "formaction" | "xlink:href"
                ) {
                    let normalized = value
                        .chars()
                        .filter(|character| {
                            !character.is_ascii_control() && !character.is_ascii_whitespace()
                        })
                        .collect::<String>()
                        .to_ascii_lowercase();
                    if normalized.starts_with("javascript:")
                        || normalized.starts_with("vbscript:")
                        || normalized.starts_with("file:")
                        || normalized.starts_with("ipc:")
                        || normalized.starts_with("tauri:")
                    {
                        diagnostics.push(diagnostic(
                            &content.html,
                            "html",
                            attr.span,
                            base,
                            "Use an ordinary website address or page link.",
                        ));
                    }
                }
                if attr.name == "srcdoc" {
                    diagnostics.push(diagnostic(
                        &content.html,
                        "html",
                        attr.span,
                        base,
                        "Embedded page code is unavailable. Use another project page.",
                    ));
                }
            }
        }
        if diagnostics.len() >= 64 {
            diagnostics.truncate(64);
            break;
        }
        pending.extend(element.children.iter().map(|child| (child, depth + 1)));
        if let Some(fragment) = &element.content {
            pending.extend(fragment.children.iter().map(|child| (child, depth + 1)));
        }
    }
    validate_css(&content.css, "css", 0, &content.css, &mut diagnostics);
    validate_javascript(
        &content.javascript,
        "javascript",
        0,
        &content.javascript,
        &mut diagnostics,
        false,
    );
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(SourceEditError::from_diagnostics(diagnostics))
    }
}

fn authored_span(children: &[Child]) -> Option<Span> {
    let mut start = u32::MAX;
    let mut end = 0;
    let mut pending: Vec<_> = children.iter().collect();
    while let Some(child) = pending.pop() {
        let span = match child {
            Child::Element(element) => {
                pending.extend(&element.children);
                element.span
            }
            Child::Text(text) => text.span,
            Child::Comment(comment) => comment.span,
            Child::DocumentType(doctype) => doctype.span,
        };
        if !span.is_dummy() {
            start = start.min(span.lo.0);
            end = end.max(span.hi.0);
        }
    }
    (start != u32::MAX).then(|| Span::new(swc_common::BytePos(start), swc_common::BytePos(end)))
}

/// Overrides accept local DOM behavior, but never code loaders or reflection. Unknown
/// capabilities stay copy-only: arbitrary JavaScript cannot be proven safe by URL matching.
pub(crate) fn validate_draft_javascript(text: &str) -> crate::Result<()> {
    use serde_json::Value;
    use std::collections::BTreeSet;
    let map = SourceMap::default();
    let file = map.new_source_file(FileName::Custom("draft.js".into()).into(), text.to_owned());
    let mut errors = vec![];
    let script = swc_ecma_parser::parse_file_as_script(
        &file,
        swc_ecma_parser::Syntax::Es(Default::default()),
        swc_ecma_ast::EsVersion::latest(),
        None,
        &mut errors,
    )
    .map_err(|_| Error("The draft JavaScript cannot be parsed.".into()))?;
    crate::ensure(errors.is_empty(), "The draft JavaScript cannot be parsed.")?;
    let tree = serde_json::to_value(script).map_err(|e| Error(e.to_string()))?;
    let mut locals: BTreeSet<String> = [
        "document",
        "console",
        "Math",
        "JSON",
        "Number",
        "String",
        "Boolean",
        "Array",
        "parseInt",
        "parseFloat",
        "isNaN",
        "undefined",
        "NaN",
        "Infinity",
        "event",
        "alert",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    fn binding(value: &Value, locals: &mut BTreeSet<String>) {
        if value["type"] == "Identifier"
            && let Some(name) = value["value"].as_str()
        {
            locals.insert(name.into());
        }
        // Complex binding patterns stay outside this small override capability set.
    }
    let mut pending = vec![&tree];
    while let Some(value) = pending.pop() {
        if let Some(object) = value.as_object() {
            match value["type"].as_str().unwrap_or("") {
                "VariableDeclarator" => binding(&value["id"], &mut locals),
                "FunctionDeclaration" | "FunctionExpression" => {
                    binding(&value["identifier"], &mut locals)
                }
                _ => (),
            }
            if let Some(params) = value["params"].as_array() {
                for param in params {
                    binding(param.get("pat").unwrap_or(param), &mut locals);
                }
            }
            pending.extend(object.values());
        } else if let Some(values) = value.as_array() {
            pending.extend(values);
        }
    }
    let properties = [
        "querySelector",
        "querySelectorAll",
        "getElementById",
        "addEventListener",
        "removeEventListener",
        "preventDefault",
        "stopPropagation",
        "closest",
        "matches",
        "contains",
        "forEach",
        "map",
        "filter",
        "reduce",
        "find",
        "indexOf",
        "includes",
        "push",
        "pop",
        "join",
        "split",
        "trim",
        "toLowerCase",
        "toUpperCase",
        "classList",
        "add",
        "remove",
        "toggle",
        "replace",
        "style",
        "dataset",
        "hidden",
        "disabled",
        "checked",
        "value",
        "textContent",
        "innerText",
        "length",
        "body",
        "documentElement",
        "target",
        "currentTarget",
        "key",
        "code",
        "type",
        "detail",
        "focus",
        "blur",
        "showModal",
        "close",
        "log",
        "warn",
        "error",
        "min",
        "max",
        "round",
        "floor",
        "ceil",
        "abs",
        "random",
        "parse",
        "stringify",
    ];
    let mut pending = vec![&tree];
    while let Some(value) = pending.pop() {
        if let Some(object) = value.as_object() {
            let kind = value["type"].as_str().unwrap_or("");
            crate::ensure(
                !matches!(
                    kind,
                    "Import"
                        | "NewExpression"
                        | "TaggedTemplateExpression"
                        | "WithStatement"
                        | "Super"
                        | "MetaProperty"
                ),
                "The draft can load or construct executable code. Copy code to keep it.",
            )?;
            if kind == "CallExpression" {
                let callee = &value["callee"];
                crate::ensure(
                    callee["type"] == "MemberExpression"
                        || callee["type"] == "FunctionExpression"
                        || callee["type"] == "ArrowFunctionExpression"
                        || callee["type"] == "Identifier"
                            && callee["value"].as_str().is_some_and(|name| {
                                [
                                    "Number",
                                    "String",
                                    "Boolean",
                                    "parseInt",
                                    "parseFloat",
                                    "isNaN",
                                    "alert",
                                ]
                                .contains(&name)
                            }),
                    "The draft calls code that requires source review. Copy code to keep it.",
                )?;
            }
            if kind == "Identifier" {
                crate::ensure(
                    value["value"].as_str().is_some_and(|name| {
                        locals.contains(name)
                            && ![
                                "eval",
                                "Function",
                                "AsyncFunction",
                                "GeneratorFunction",
                                "fetch",
                                "XMLHttpRequest",
                                "WebSocket",
                                "Worker",
                                "SharedWorker",
                                "importScripts",
                                "setTimeout",
                                "setInterval",
                                "globalThis",
                                "window",
                                "self",
                                "top",
                                "parent",
                                "frames",
                                "opener",
                                "navigator",
                                "location",
                                "Reflect",
                                "Proxy",
                            ]
                            .contains(&name)
                    }),
                    "The draft uses script capabilities that require source review. Copy code to keep it.",
                )?;
            }
            if kind == "MemberExpression" || kind == "OptionalChainingExpression" {
                let property = &value["property"];
                let name = property["value"].as_str().unwrap_or("");
                let parent = value["object"]["property"]["value"].as_str().unwrap_or("");
                let data_property = matches!(parent, "dataset" | "style")
                    && !matches!(name, "constructor" | "prototype" | "__proto__");
                crate::ensure(
                    property["type"] == "Identifier"
                        && (properties.contains(&name) || data_property),
                    "The draft uses dynamic or executable script access. Copy code to keep it.",
                )?;
                pending.extend(
                    object
                        .iter()
                        .filter(|(key, _)| key.as_str() != "property")
                        .map(|(_, v)| v),
                );
            } else {
                pending.extend(object.values());
            }
        } else if let Some(values) = value.as_array() {
            pending.extend(values);
        }
    }
    Ok(())
}

/// Every source element must survive the browser parser with its identity intact.
pub(crate) fn validate_draft_html(html: &str) -> crate::Result<()> {
    let identified = assign_editor_ids(html)?;
    let (children, diagnostics, _) = parse_html(&identified);
    crate::ensure(
        diagnostics.is_empty(),
        "The draft HTML cannot round-trip through the parser.",
    )?;
    let doc = crate::editing::parse(&identified)?;
    let mut pending: Vec<_> = children.iter().collect();
    while let Some(child) = pending.pop() {
        if let Child::Element(element) = child {
            if let Some(id) = element
                .attributes
                .iter()
                .find(|a| a.name == crate::ELEMENT_ID_ATTRIBUTE)
                .and_then(|a| a.value.as_deref())
            {
                let nodes = doc.select(&format!("[{}=\"{id}\"]", crate::ELEMENT_ID_ATTRIBUTE));
                crate::ensure(
                    nodes.nodes().len() == 1
                        && nodes.nodes()[0].node_name().as_deref()
                            == Some(element.tag_name.as_ref()),
                    "The draft HTML cannot round-trip without losing element IDs.",
                )?;
            }
            pending.extend(&element.children);
            if let Some(fragment) = &element.content {
                pending.extend(&fragment.children);
            }
        }
    }
    Ok(())
}

/// Add identities without reformatting authored HTML or changing source coordinates elsewhere.
pub(crate) fn assign_editor_ids(html: &str) -> crate::Result<String> {
    let (children, _, base) = parse_html(html);
    let mut pending: Vec<_> = children.iter().collect();
    let mut used = std::collections::BTreeSet::new();
    let mut insertions = vec![];
    while let Some(child) = pending.pop() {
        let Child::Element(element) = child else {
            continue;
        };
        pending.extend(&element.children);
        if let Some(fragment) = &element.content {
            pending.extend(&fragment.children);
        }
        if let Some(attribute) = element
            .attributes
            .iter()
            .find(|a| a.name.as_ref() == crate::ELEMENT_ID_ATTRIBUTE)
        {
            let value = attribute.value.as_deref().unwrap_or("");
            crate::ensure(
                crate::stable_targets::valid_id(value) && used.insert(value.to_string()),
                "Editor element IDs must be valid and unique within the page.",
            )?;
        } else if !element.span.is_dummy() {
            let start = element.span.lo.0.saturating_sub(base) as usize;
            let slice = html
                .get(start..)
                .ok_or_else(|| Error("The element source position is invalid.".into()))?;
            if !slice.starts_with('<') {
                continue;
            }
            let end = slice
                .bytes()
                .enumerate()
                .skip(1)
                .find(|(_, b)| b.is_ascii_whitespace() || matches!(b, b'>' | b'/'))
                .map(|(i, _)| start + i)
                .ok_or_else(|| Error("The element opening tag is incomplete.".into()))?;
            insertions.push(end);
        }
    }
    insertions.sort_unstable();
    let mut result = html.to_owned();
    for offset in insertions.into_iter().rev() {
        let id = crate::stable_targets::random_id(&mut used);
        result.insert_str(
            offset,
            &format!(" {}=\"{}\"", crate::ELEMENT_ID_ATTRIBUTE, id),
        );
    }
    Ok(result)
}

impl Project {
    pub fn edit_page_source(
        &self,
        revision: u64,
        page_id: Uuid,
        edit: &PageSourceEdit,
    ) -> SourceEditResult<Self> {
        crate::ensure(
            revision == self.revision,
            "The page changed while you were editing. Keep your text and reload the latest source before saving.",
        )?;
        self.validate()?;
        let before = self.page(page_id)?;
        let old = before.content.clone().unwrap_or(SiteContent {
            html: String::new(),
            css: String::new(),
            javascript: String::new(),
        });
        let mut content = SiteContent {
            html: edit.html.clone().unwrap_or_else(|| old.html.clone()),
            css: edit.css.clone().unwrap_or_else(|| old.css.clone()),
            javascript: edit
                .javascript
                .clone()
                .unwrap_or_else(|| old.javascript.clone()),
        };
        if content == old {
            return Ok(self.clone());
        }
        validate_page_source_for_website(&content, self.website.is_some())?;
        content.html = assign_editor_ids(&content.html)?;
        let after = self.parse_page_html(&content.html)?;
        let body = crate::editing::body(&after)?;
        let new_ids = crate::editing::ids(&after);
        let original = self.parse_page_html(&old.html)?;
        let old_ids = crate::editing::ids(&original);
        for (id, count) in &new_ids {
            let allowed = old_ids.get(id).copied().unwrap_or(1);
            if *count > allowed {
                let (children, _, base) = parse_html(&content.html);
                let mut pending: Vec<_> = children.iter().collect();
                let mut spans = vec![];
                while let Some(child) = pending.pop() {
                    if let Child::Element(element) = child {
                        spans.extend(
                            element
                                .attributes
                                .iter()
                                .filter(|attr| {
                                    attr.name == "id" && attr.value.as_deref() == Some(id.as_str())
                                })
                                .map(|attr| attr.span),
                        );
                        pending.extend(&element.children);
                    }
                }
                spans.sort_by_key(|span| span.lo.0);
                let span = spans.get(allowed).copied().unwrap_or(DUMMY_SP);
                return Err(SourceEditError::from_diagnostics(vec![diagnostic(
                    &content.html,
                    "html",
                    span,
                    base,
                    "Use a unique ID for each element.",
                )]));
            }
        }
        let hash = content.sha256()?;
        let mut next = self.clone();
        next.revision = revision
            .checked_add(1)
            .ok_or_else(|| SourceEditError::general("Project revision limit reached."))?;
        let page = next
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .expect("page checked");
        for region in page.regions.iter_mut().filter(|region| !region.completed) {
            let previous: Vec<_> = region.targets().into_iter().cloned().collect();
            let mut refreshed = Vec::new();
            for target in &previous {
                if !target.id.is_empty() {
                    refreshed.push(
                        crate::editing::locate(&after, target)
                            .ok()
                            .and_then(|node| crate::editing::target(node, body, &hash))
                            .unwrap_or_else(|| target.clone()),
                    );
                    continue;
                }
                let old_node = crate::editing::locate(&original, target)?;
                let matches: Vec<_> = after
                    .select("*")
                    .nodes()
                    .iter()
                    .copied()
                    .filter(|node| {
                        if let Some(id) = old_node
                            .attr("id")
                            .filter(|id| old_ids.get(id.as_ref()) == Some(&1))
                        {
                            node.attr("id") == Some(id) && node.node_name() == old_node.node_name()
                        } else if let Some(id) = old_node.attr("data-desegnos-region") {
                            node.attr("data-desegnos-region") == Some(id)
                                && node.node_name() == old_node.node_name()
                        } else {
                            node.html() == old_node.html()
                        }
                    })
                    .collect();
                if matches.len() == 1 {
                    if let Some(target) = crate::editing::target(matches[0], body, &hash) {
                        refreshed.push(target);
                    }
                } else {
                    crate::ensure(
                        region.prompt.trim().is_empty(),
                        "This code edit would lose track of an item with a written AI request. Finish or remove that request first, or keep the item's unique ID.",
                    )?;
                    region.completed = true;
                }
            }
            region.elements = refreshed;
            region.element = None;
        }
        page.content = Some(content);
        next.store_inline_images()?;
        next.validate()?;
        Ok(next)
    }

    pub fn page_source_map(&self, page_id: Uuid) -> SourceEditResult<PageSourceMap> {
        let page = self.page(page_id)?;
        let empty = SiteContent {
            html: String::new(),
            css: String::new(),
            javascript: String::new(),
        };
        let content = page.content.as_ref().unwrap_or(&empty);
        let (children, _, base) = parse_html(&content.html);
        let positions = Positions::new(&content.html);
        let mut ranges = vec![];
        let mut pending = vec![(children.as_slice(), String::new(), 0)];
        let mut count = 0;
        while let Some((children, parent, depth)) = pending.pop() {
            let mut counts = BTreeMap::<String, usize>::new();
            for child in children {
                let Child::Element(element) = child else {
                    continue;
                };
                count += 1;
                crate::ensure(
                    count <= 20_000 && depth <= 128,
                    "This page is too complex to map.",
                )?;
                let tag = element.tag_name.to_string();
                let index = counts.entry(tag.clone()).or_default();
                *index += 1;
                let selector = if tag == "body" && parent.is_empty() {
                    "body".to_owned()
                } else if parent.is_empty() && tag == "html" {
                    String::new()
                } else {
                    format!(
                        "{} > {tag}:nth-of-type({index})",
                        if parent.is_empty() { "body" } else { &parent }
                    )
                };
                if !selector.is_empty() && tag != "head" {
                    let implicit = element.span.is_dummy();
                    let span = if implicit {
                        authored_span(&element.children).unwrap_or_else(|| {
                            Span::new(
                                swc_common::BytePos(base),
                                swc_common::BytePos(base + content.html.len() as u32),
                            )
                        })
                    } else {
                        element.span
                    };
                    let start = positions.get(span.lo.0.saturating_sub(base) as usize);
                    let end = positions.get(span.hi.0.saturating_sub(base) as usize);
                    ranges.push(ElementSourceRange {
                        id: element
                            .attributes
                            .iter()
                            .find(|a| a.name.as_ref() == crate::ELEMENT_ID_ATTRIBUTE)
                            .and_then(|a| a.value.as_ref())
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        selector: selector.clone(),
                        start_line: start.0,
                        end_line: end.0,
                        start_column: start.1,
                        end_column: end.1,
                        implicit,
                    });
                }
                if tag != "head" {
                    pending.push((&element.children, selector, depth + 1));
                }
            }
        }
        if !ranges.iter().any(|range| range.selector == "body") {
            let end = position(&content.html, content.html.len());
            ranges.push(ElementSourceRange {
                id: String::new(),
                selector: "body".into(),
                start_line: 1,
                end_line: end.0,
                start_column: 1,
                end_column: end.1,
                implicit: true,
            });
        }
        // Only selectors that resolve in the same browser DOM used by the canvas
        // are exposed; parser-inserted/template-only nodes cannot select another item.
        let doc = self.parse_page_html(&content.html)?;
        let body = crate::editing::body(&doc)?;
        let actual: std::collections::BTreeSet<_> = doc
            .select("*")
            .nodes()
            .iter()
            .filter_map(|node| crate::editing::selector(*node, body))
            .collect();
        ranges.retain(|range| actual.contains(&range.selector));
        Ok(PageSourceMap {
            project_id: self.id,
            page_id,
            revision: self.revision,
            source_sha256: content.sha256()?,
            html_sha256: crate::digest(content.html.as_bytes()),
            ranges,
        })
    }
}

/// Remove only the editor attributes, without serializing the authored document.
pub(crate) fn strip_editor_ids(html: &str) -> String {
    let (children, _, base) = parse_html(html);
    let mut pending: Vec<_> = children.iter().collect();
    let mut ranges = vec![];
    while let Some(child) = pending.pop() {
        let Child::Element(element) = child else {
            continue;
        };
        pending.extend(&element.children);
        if let Some(fragment) = &element.content {
            pending.extend(&fragment.children);
        }
        for attr in &element.attributes {
            if attr.name.as_ref() == crate::ELEMENT_ID_ATTRIBUTE && !attr.span.is_dummy() {
                let mut start = attr.span.lo.0.saturating_sub(base) as usize;
                if start > 0 && html.as_bytes()[start - 1] == b' ' {
                    start -= 1;
                }
                ranges.push((start, attr.span.hi.0.saturating_sub(base) as usize));
            }
        }
    }
    ranges.sort_unstable();
    ranges.dedup();
    let mut result = html.to_owned();
    for (start, end) in ranges.into_iter().rev() {
        if result.get(start..end).is_some() {
            result.replace_range(start..end, "");
        }
    }
    result
}
