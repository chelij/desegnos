//! Readable generated code. The AI writes each page as one long line; after a generation
//! Desegnos lays the HTML and CSS out on separate lines. From requirements format 14 the
//! compact form is what the AI sees, so the layout costs no tokens.
//!
//! HTML only gains or loses whitespace-only text where it never renders: between block-level
//! elements inside a block container, never inside pre/textarea/script/style, never next to
//! inline content, and never around elements the page's CSS makes inline or whitespace-preserving.
//! CSS is reprinted by lightningcss. JavaScript is left exactly as written.
use crate::SiteContent;
use dom_query::{Document, NodeRef};
use std::collections::HashSet;

/// Elements that are block-level by default, plus head-type elements that never render.
const BLOCK: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "caption",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "li",
    "link",
    "main",
    "menu",
    "meta",
    "nav",
    "noscript",
    "ol",
    "p",
    "pre",
    "script",
    "section",
    "style",
    "summary",
    "table",
    "tbody",
    "td",
    "template",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "ul",
];
/// Containers whose contents are never touched.
const OPAQUE: &[&str] = &[
    "pre", "textarea", "script", "style", "template", "svg", "math", "noscript", "title",
];
/// A page this large is left compact so later edits stay within the editor's node budget.
const MAX_FORMATTED_NODES: usize = 15_000;

fn full_document(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.contains("<html") || lower.contains("<!doctype")
}

fn name(node: &NodeRef<'_>) -> String {
    node.node_name()
        .map(|name| name.to_ascii_lowercase())
        .unwrap_or_default()
}

fn whitespace(node: &NodeRef<'_>) -> bool {
    node.is_text() && node.text().chars().all(|c| c.is_ascii_whitespace())
}

/// Elements whose whitespace may matter: made inline (inline-block, inline-flex, …) or
/// whitespace-preserving by a stylesheet rule or their own style attribute. None when a
/// relevant selector cannot be evaluated here, in which case the HTML is left as it is.
fn sensitive(doc: &Document, css: &str) -> Option<HashSet<dom_query::NodeId>> {
    use lightningcss::rules::{CssRule, CssRuleList};
    use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
    use lightningcss::traits::ToCss;
    fn risky(declarations: &str) -> bool {
        let compact: String = declarations
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        compact.contains("display:inline")
            || compact.contains("display:contents")
            || compact.contains("display:none")
            || compact.contains("float:left")
            || compact.contains("float:right")
            || compact.contains("float:inline")
            || compact.contains("position:absolute")
            || compact.contains("position:fixed")
            || compact.contains("white-space:pre")
            || compact.contains("white-space:break-spaces")
            || compact.contains("white-space-collapse:preserve")
            || compact.contains("white-space-collapse:break-spaces")
    }
    fn collect(list: &CssRuleList<'_>, selectors: &mut Vec<String>) -> Option<()> {
        for rule in &list.0 {
            match rule {
                CssRule::Style(style) => {
                    let declarations = style
                        .declarations
                        .to_css_string(PrinterOptions::default())
                        .ok()?;
                    if risky(&declarations) {
                        let text = style
                            .selectors
                            .to_css_string(PrinterOptions::default())
                            .ok()?;
                        selectors.extend(text.split(',').map(|value| value.trim().to_owned()));
                    }
                    collect(&style.rules, selectors)?;
                }
                CssRule::Media(media) => collect(&media.rules, selectors)?,
                CssRule::Supports(supports) => collect(&supports.rules, selectors)?,
                CssRule::LayerBlock(layer) => collect(&layer.rules, selectors)?,
                CssRule::Container(container) => collect(&container.rules, selectors)?,
                CssRule::Unknown(_) | CssRule::Ignored => {}
                _ => {}
            }
        }
        Some(())
    }
    let mut marked = HashSet::new();
    if !css.trim().is_empty() {
        let sheet = StyleSheet::parse(css, ParserOptions::default()).ok()?;
        let mut selectors = vec![];
        collect(&sheet.rules, &mut selectors)?;
        for selector in selectors {
            // Pseudo-element rules style generated boxes, not the element's own display.
            if selector.contains("::")
                || selector.contains(":before")
                || selector.contains(":after")
            {
                continue;
            }
            // Dropping state pseudo-classes only widens the match, which is the safe direction.
            let mut plain = String::new();
            let mut rest = selector.as_str();
            while let Some(index) = rest.find(':') {
                plain.push_str(&rest[..index]);
                rest = &rest[index + 1..];
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                    .unwrap_or(rest.len());
                rest = &rest[end..];
                if rest.starts_with('(') {
                    let mut depth = 0;
                    let mut close = rest.len();
                    for (offset, c) in rest.char_indices() {
                        match c {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    close = offset + 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    rest = &rest[close..];
                }
            }
            plain.push_str(rest);
            let plain = plain.trim();
            let plain = if plain.is_empty() || plain.ends_with(['>', '+', '~']) {
                "*"
            } else {
                plain
            };
            let matcher = dom_query::Matcher::new(plain).ok()?;
            marked.extend(
                doc.select_matcher(&matcher)
                    .nodes()
                    .iter()
                    .map(|node| node.id),
            );
        }
    }
    for node in doc.select("[style]").nodes() {
        if node.attr("style").is_some_and(|style| risky(&style)) {
            marked.insert(node.id);
        }
    }
    marked.extend(doc.select("[hidden]").nodes().iter().map(|node| node.id));
    Some(marked)
}

/// Head-type elements: block-level in the list above but never rendered, so they only count
/// as blocks inside containers that hold nothing else.
const UNRENDERED: &[&str] = &[
    "link", "meta", "script", "style", "template", "title", "noscript",
];

fn layout_container(node: &NodeRef<'_>, sensitive: &HashSet<dom_query::NodeId>) -> bool {
    let tag = name(node);
    BLOCK.contains(&tag.as_str())
        && !OPAQUE.contains(&tag.as_str())
        && !sensitive.contains(&node.id)
}

fn block(node: &NodeRef<'_>, sensitive: &HashSet<dom_query::NodeId>) -> bool {
    node.is_element() && BLOCK.contains(&name(node).as_str()) && !sensitive.contains(&node.id)
}

/// A rendered block box: whitespace beside it falls at the start or end of a line and never shows.
fn solid(node: Option<&NodeRef<'_>>, sensitive: &HashSet<dom_query::NodeId>) -> bool {
    node.is_some_and(|node| block(node, sensitive) && !UNRENDERED.contains(&name(node).as_str()))
}

/// Children are only blocks (including unrendered ones), comments and whitespace.
fn block_only(node: &NodeRef<'_>, sensitive: &HashSet<dom_query::NodeId>) -> bool {
    let children = node.children();
    children.iter().any(|child| child.is_element())
        && children
            .iter()
            .all(|child| child.is_comment() || whitespace(child) || block(child, sensitive))
}

/// Layout whitespace may sit between `before` and `after` (None is the container's edge).
fn gap(
    all_blocks: bool,
    before: Option<&NodeRef<'_>>,
    after: Option<&NodeRef<'_>>,
    sensitive: &HashSet<dom_query::NodeId>,
) -> bool {
    all_blocks || solid(before, sensitive) || solid(after, sensitive)
}

fn walk(
    doc: &Document,
    node: &NodeRef<'_>,
    depth: usize,
    layout: bool,
    sensitive: &HashSet<dom_query::NodeId>,
    changes: &mut usize,
) {
    let tag = name(node);
    if OPAQUE.contains(&tag.as_str()) {
        return;
    }
    if layout_container(node, sensitive) {
        let all_blocks = block_only(node, sensitive);
        // Remove whitespace-only text in layout positions, then put the layout back in each such gap.
        let children = node.children();
        for (index, child) in children.iter().enumerate() {
            if whitespace(child) {
                let before = index
                    .checked_sub(1)
                    .and_then(|i| children.get(i))
                    .filter(|n| !whitespace(n));
                let after = children.get(index + 1).filter(|n| !whitespace(n));
                if gap(all_blocks, before, after, sensitive) {
                    child.remove_from_parent();
                    *changes += 1;
                }
            }
        }
        if layout {
            let body = tag == "body";
            let indent = |level: usize| format!("\n{}", "  ".repeat(level));
            let children = node.children();
            for (index, child) in children.iter().enumerate() {
                // The page starts and ends at the body's edges; no blank line there.
                if !whitespace(child)
                    && !(body && index == 0)
                    && gap(
                        all_blocks,
                        index.checked_sub(1).and_then(|i| children.get(i)),
                        Some(child),
                        sensitive,
                    )
                {
                    child.insert_before(&doc.tree.new_text(indent(if body {
                        0
                    } else {
                        depth + 1
                    })));
                    *changes += 1;
                }
            }
            if !body
                && gap(all_blocks, children.last(), None, sensitive)
                && children.last().is_some_and(|last| !whitespace(last))
            {
                node.append_child(&doc.tree.new_text(indent(depth)));
                *changes += 1;
            }
        }
    }
    for child in node.children().iter().filter(|child| child.is_element()) {
        walk(
            doc,
            child,
            if tag == "body" { 0 } else { depth + 1 },
            layout,
            sensitive,
            changes,
        );
    }
}

fn arrange_html(html: &str, css: &str, layout: bool) -> Option<String> {
    let full = full_document(html);
    let doc = crate::editing::parse_document(html, full).ok()?;
    let sensitive = sensitive(&doc, css)?;
    let body = doc.select("body").nodes().first().cloned()?;
    let mut changes = 0;
    walk(&doc, &body, 0, layout, &sensitive, &mut changes);
    // Nothing to lay out: keep the source byte for byte rather than re-serializing it.
    if changes == 0 || doc.root().descendants_it().count() > MAX_FORMATTED_NODES {
        return None;
    }
    if full {
        crate::editing::serialized(&doc).ok()
    } else {
        Some(body.inner_html().to_string())
    }
}

fn print_css(css: &str, minify: bool) -> Option<String> {
    use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
    if css.trim().is_empty() {
        return None;
    }
    let sheet = StyleSheet::parse(css, ParserOptions::default()).ok()?;
    sheet
        .to_css(PrinterOptions {
            minify,
            ..PrinterOptions::default()
        })
        .ok()
        .map(|result| result.code)
}

/// The readable layout stored after a generation. Parts that cannot be laid out safely stay as written.
pub fn format_site_content(content: &SiteContent) -> SiteContent {
    SiteContent {
        html: arrange_html(&content.html, &content.css, true)
            .unwrap_or_else(|| content.html.clone()),
        css: print_css(&content.css, false).unwrap_or_else(|| content.css.clone()),
        javascript: content.javascript.clone(),
    }
}

/// The compact form sent to the AI from format 14: the same page without layout whitespace.
pub fn compact_site_content(content: &SiteContent) -> SiteContent {
    SiteContent {
        html: arrange_html(&content.html, &content.css, false)
            .unwrap_or_else(|| content.html.clone()),
        css: print_css(&content.css, true).unwrap_or_else(|| content.css.clone()),
        javascript: content.javascript.clone(),
    }
}

impl crate::Project {
    /// This project as the AI sees it from format 14: page sources in compact form.
    pub(crate) fn compact_pages(&self) -> crate::Project {
        let mut project = self.clone();
        for page in &mut project.pages {
            if let Some(content) = &mut page.content {
                *content = compact_site_content(content);
            }
        }
        project
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(html: &str, css: &str) -> SiteContent {
        SiteContent {
            html: html.into(),
            css: css.into(),
            javascript: "let a=1;".into(),
        }
    }

    #[test]
    fn block_layout_is_readable_reversible_and_idempotent() {
        let page = site(
            "<main data-desegnos-id=\"a1\"><section><h1>Hi <em>there</em></h1><p>One</p></section><ul><li>A</li><li>B</li></ul><pre>  keep\n  this</pre></main>",
            "main{padding:0;color:red}@media (max-width:600px){main{padding:4px}}",
        );
        let formatted = format_site_content(&page);
        assert_eq!(
            formatted.html,
            "<main data-desegnos-id=\"a1\">\n  <section>\n    <h1>Hi <em>there</em></h1>\n    <p>One</p>\n  </section>\n  <ul>\n    <li>A</li>\n    <li>B</li>\n  </ul>\n  <pre>  keep\n  this</pre>\n</main>"
        );
        assert!(formatted.css.contains('\n'));
        assert_eq!(formatted.javascript, page.javascript);
        assert_eq!(format_site_content(&formatted), formatted);
        assert_eq!(
            compact_site_content(&formatted),
            compact_site_content(&page)
        );
        assert_eq!(
            compact_site_content(&formatted).html,
            "<main data-desegnos-id=\"a1\"><section><h1>Hi <em>there</em></h1><p>One</p></section><ul><li>A</li><li>B</li></ul><pre>  keep\n  this</pre></main>"
        );
    }

    #[test]
    fn whitespace_that_could_render_is_left_alone() {
        // Inline-block items would gain gaps; inline runs and preserved whitespace never change.
        let page = site(
            "<nav><ul class=\"menu\"><li>A</li><li>B</li></ul></nav><div><span>x</span><b>y</b><div>z</div></div><div style=\"white-space:pre\"><p>a</p><p>b</p></div><p>a<span style=\"position:absolute\">b</span>c</p>",
            ".menu li:hover{display:inline-block}",
        );
        let html = format_site_content(&page).html;
        assert!(
            html.contains("<ul class=\"menu\"><li>A</li><li>B</li></ul>"),
            "{html}"
        );
        // Inline neighbours stay together; a block beside inline content may start a new line.
        assert!(
            html.contains("<div><span>x</span><b>y</b>\n  <div>z</div>\n</div>"),
            "{html}"
        );
        assert!(
            html.contains("<p>a<span style=\"position:absolute\">b</span>c</p>"),
            "{html}"
        );
        assert!(
            html.contains("<div style=\"white-space:pre\"><p>a</p><p>b</p></div>"),
            "{html}"
        );
    }

    #[test]
    #[ignore = "reads a real project journal named by DESEGNOS_FORMAT_SAMPLE"]
    fn real_page_round_trips() {
        let path = std::env::var("DESEGNOS_FORMAT_SAMPLE").unwrap();
        let journal: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for page in journal["project"]["pages"].as_array().unwrap() {
            let content: SiteContent = serde_json::from_value(page["content"].clone()).unwrap();
            let formatted = format_site_content(&content);
            let compact = compact_site_content(&content);
            println!(
                "html {} lines {} -> {} lines; css {} -> {} lines; compact html {} bytes (was {})",
                content.html.len(),
                content.html.lines().count(),
                formatted.html.lines().count(),
                content.css.lines().count(),
                formatted.css.lines().count(),
                compact.html.len(),
                content.html.len()
            );
            assert_eq!(format_site_content(&formatted), formatted);
            assert_eq!(compact_site_content(&formatted), compact);
            let text = |html: &str| {
                crate::editing::parse_document(html, full_document(html))
                    .unwrap()
                    .select("body")
                    .text()
                    .split_whitespace()
                    .collect::<String>()
            };
            assert_eq!(text(&formatted.html), text(&content.html));
            let mut lines: Vec<_> = formatted.html.lines().map(|line| line.len()).collect();
            lines.sort_unstable();
            println!(
                "longest lines: {:?}",
                &lines[lines.len().saturating_sub(5)..]
            );
            for marker in ["{", "!important", "desegnos-asset:", "@media", "@keyframes"] {
                assert_eq!(
                    formatted.css.matches(marker).count(),
                    content.css.matches(marker).count(),
                    "{marker}"
                );
            }
            if let Ok(out) = std::env::var("DESEGNOS_FORMAT_OUT") {
                std::fs::write(out, &formatted.html).unwrap();
            }
        }
    }
}
