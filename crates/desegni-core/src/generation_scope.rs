//! Structural scope checks; visual geometry remains a browser benchmark.
use crate::{GenerationCandidate, Project, RequirementsSnapshot, Result, ensure};
use dom_query::NodeRef;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const MARKER: &str = "data-desegnos-region";

fn canonical(node: NodeRef<'_>) -> serde_json::Value {
    if node.is_comment() {
        return serde_json::Value::Null;
    }
    if node.is_text() {
        return serde_json::json!({"text":node.text().to_string()});
    }
    let attrs: BTreeMap<_, _> = node
        .attrs()
        .into_iter()
        .map(|attr| (format!("{:?}", attr.name), attr.value.to_string()))
        .collect();
    let children: Vec<_> = node
        .children()
        .into_iter()
        .map(canonical)
        .filter(|value| !value.is_null())
        .collect();
    serde_json::json!({"name":node.node_name().map(|value|value.to_string()),"attrs":attrs,"children":children})
}

fn empty_layout(node: NodeRef<'_>) -> bool {
    if node.is_comment() || node.is_text() && node.text().trim().is_empty() {
        return true;
    }
    matches!(
        node.node_name().as_deref(),
        Some("body" | "div" | "main" | "section")
    ) && !node.attrs().iter().any(|attr| {
        let name = attr.name.local.as_ref();
        name.starts_with("on")
            || matches!(name, "role" | "tabindex" | "contenteditable" | "aria-label")
    }) && node.children().into_iter().all(empty_layout)
}

impl GenerationCandidate {
    pub(crate) fn validate_scope(
        &self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
    ) -> Result<()> {
        if snapshot.format_version()? < 8 {
            return Ok(());
        }
        let targets = snapshot.target_ids()?;
        let stable = snapshot.format_version()? >= 10;
        for page in &self.pages {
            let original = project.page(page.page_id)?;
            if stable {
                let known = original
                    .content
                    .as_ref()
                    .map(|content| crate::editing::parse(&content.html))
                    .transpose()?;
                let before_ids: BTreeSet<_> = known
                    .as_ref()
                    .map(|doc| {
                        doc.select(&format!("[{}]", crate::ELEMENT_ID_ATTRIBUTE))
                            .nodes()
                            .iter()
                            .filter_map(|node| {
                                node.attr(crate::ELEMENT_ID_ATTRIBUTE)
                                    .map(|id| id.to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let incoming = crate::editing::parse(&page.content.html)?;
                crate::stable_targets::validate_ids(&incoming)?;
                for node in incoming
                    .select(&format!("[{}]", crate::ELEMENT_ID_ATTRIBUTE))
                    .nodes()
                {
                    ensure(
                        node.attr(crate::ELEMENT_ID_ATTRIBUTE)
                            .is_some_and(|id| before_ids.contains(id.as_ref())),
                        "The AI assigned an editor ID to a new element. Your page is unchanged.",
                    )?;
                }
            }
            let regions: Vec<_> = original
                .regions
                .iter()
                .filter(|region| targets.contains(&(page.page_id, region.id)))
                .collect();
            if !regions.is_empty() && regions.iter().all(|region| region.mobile_only) {
                validate_phone_scope(original, &page.content)?;
                continue;
            }
            // Rectangular requests on an existing page have no trustworthy mapping
            // to old DOM nodes. Do not invent one from labels or model claims.
            let targeted = regions.iter().all(|region| region.has_targets());
            if stable
                && regions.iter().any(|region| region.has_targets())
                && original.content.is_some()
            {
                validate_stable_scope(original, &page.content, &regions)?;
                continue;
            }
            if original
                .content
                .as_ref()
                .is_some_and(|source| !source.html.trim().is_empty())
                && !targeted
            {
                continue;
            }
            let document = crate::editing::parse(&page.content.html)?;
            let requested: BTreeSet<_> = regions.iter().map(|region| region.id).collect();
            let previous_markers: BTreeSet<_> = if let Some(source) = &original.content {
                crate::editing::parse(&source.html)?
                    .select(&format!("[{MARKER}]"))
                    .nodes()
                    .iter()
                    .filter_map(|node| node.attr(MARKER).map(|value| value.to_string()))
                    .collect()
            } else {
                BTreeSet::new()
            };
            let mut found = BTreeSet::new();
            for node in document.select(&format!("[{MARKER}]")).nodes() {
                let id = node
                    .attr(MARKER)
                    .and_then(|value| Uuid::parse_str(&value).ok());
                if node
                    .attr(MARKER)
                    .is_some_and(|value| previous_markers.contains(value.as_ref()))
                    && !id.is_some_and(|id| requested.contains(&id))
                {
                    continue;
                }
                ensure(
                    id.is_some_and(|id| requested.contains(&id) && found.insert(id)),
                    "The AI added content outside the requested areas. Your page is unchanged.",
                )?;
            }
            ensure(
                found == requested,
                "The AI did not identify each requested area. Your page is unchanged.",
            )?;
            for node in document.select(&format!("[{MARKER}]")).nodes() {
                if node
                    .attr(MARKER)
                    .and_then(|value| Uuid::parse_str(&value).ok())
                    .is_some_and(|id| requested.contains(&id))
                {
                    node.remove_from_parent();
                }
            }
            let remaining = crate::editing::body(&document)?;
            if let Some(source) = &original.content {
                let before = crate::editing::parse(&source.html)?;
                let nodes = regions
                    .iter()
                    .map(|region| {
                        crate::editing::locate(&before, region.element.as_ref().expect("targeted"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                for node in nodes {
                    node.remove_from_parent();
                }
                ensure(
                    canonical(crate::editing::body(&before)?) == canonical(remaining),
                    "The AI changed content outside the selected elements. Your page is unchanged.",
                )?;
            } else {
                ensure(
                    empty_layout(remaining),
                    "The AI added content outside the requested areas. Your page is unchanged.",
                )?;
            }
        }
        Ok(())
    }
}

fn validate_stable_scope(
    original: &crate::Page,
    candidate: &crate::SiteContent,
    regions: &[&crate::Region],
) -> Result<()> {
    let source = original.content.as_ref().expect("checked source");
    ensure(
        source.css == candidate.css && source.javascript == candidate.javascript,
        "The AI changed shared styling or behavior outside the selected elements. Your page is unchanged.",
    )?;
    let before = crate::editing::parse(&source.html)?;
    let after = crate::editing::parse(&candidate.html)?;
    crate::stable_targets::validate_ids(&before)?;
    crate::stable_targets::validate_ids(&after)?;
    let shared_code = |doc: &dom_query::Document| {
        doc.select("style, script, link")
            .nodes()
            .iter()
            .copied()
            .map(canonical)
            .collect::<Vec<_>>()
    };
    ensure(
        shared_code(&before) == shared_code(&after),
        "The AI changed embedded shared styling or scripts. Your page is unchanged.",
    )?;
    let handlers = |doc: &dom_query::Document| {
        let body = crate::editing::body(doc)?;
        let mut result = BTreeMap::new();
        for node in doc.select("*").nodes() {
            for attribute in node
                .attrs()
                .into_iter()
                .filter(|attribute| attribute.name.local.as_ref().starts_with("on"))
            {
                let key = node
                    .attr(crate::ELEMENT_ID_ATTRIBUTE)
                    .map(|id| id.to_string())
                    .or_else(|| crate::editing::selector(*node, body))
                    .unwrap_or_default();
                result.insert(
                    (key, attribute.name.local.to_string()),
                    attribute.value.to_string(),
                );
            }
        }
        Ok::<_, crate::Error>(result)
    };
    let previous_handlers = handlers(&before)?;
    ensure(
        handlers(&after)?
            .iter()
            .all(|(key, value)| previous_handlers.get(key) == Some(value)),
        "The AI changed shared script behavior through an event handler. Your page is unchanged.",
    )?;
    let new_areas: BTreeSet<_> = regions
        .iter()
        .filter(|region| !region.has_targets())
        .map(|region| region.id)
        .collect();
    let mut area_roots = Vec::new();
    let mut seen_areas = BTreeSet::new();
    for node in after.select(&format!("[{MARKER}]")).nodes() {
        if let Some(id) = node
            .attr(MARKER)
            .and_then(|value| Uuid::parse_str(&value).ok())
            .filter(|id| new_areas.contains(id))
        {
            ensure(
                seen_areas.insert(id),
                "The AI repeated a requested area. Your page is unchanged.",
            )?;
            area_roots.push(*node);
        }
    }
    ensure(
        seen_areas == new_areas,
        "The AI did not identify each requested area. Your page is unchanged.",
    )?;
    let targets: Vec<_> = regions
        .iter()
        .flat_map(|region| {
            region
                .targets()
                .into_iter()
                .map(|target| (target, region.allow_target_removal))
        })
        .collect();
    let old_roots = targets
        .iter()
        .map(|(target, _)| crate::editing::locate(&before, target))
        .collect::<Result<Vec<_>>>()?;
    let new_roots = targets
        .iter()
        .map(
            |(target, allowed)| match crate::editing::locate(&after, target) {
                Ok(node) => Ok(Some(node)),
                Err(_) if *allowed => Ok(None),
                Err(_) => Err(crate::Error(
                    "The AI removed a selected element. Your page is unchanged.".into(),
                )),
            },
        )
        .collect::<Result<Vec<_>>>()?;
    // Retained identities may not be smuggled in/out of another selected scope.
    let before_ids: BTreeMap<_, _> = before
        .select(&format!("[{}]", crate::ELEMENT_ID_ATTRIBUTE))
        .nodes()
        .iter()
        .filter_map(|node| {
            node.attr(crate::ELEMENT_ID_ATTRIBUTE)
                .map(|id| (id.to_string(), *node))
        })
        .collect();
    for node in after
        .select(&format!("[{}]", crate::ELEMENT_ID_ATTRIBUTE))
        .nodes()
    {
        let id = node
            .attr(crate::ELEMENT_ID_ATTRIBUTE)
            .expect("selected attr");
        let previous = before_ids.get(id.as_ref()).ok_or_else(|| {
            crate::Error(
                "The AI assigned an editor ID to a new element. Your page is unchanged.".into(),
            )
        })?;
        let old_scope = old_roots
            .iter()
            .position(|root| crate::stable_targets::contains(*root, *previous));
        let new_scope = new_roots
            .iter()
            .position(|root| root.is_some_and(|root| crate::stable_targets::contains(root, *node)));
        ensure(
            old_scope == new_scope,
            "The AI moved content outside its selected scope. Your page is unchanged.",
        )?;
    }
    for area in &area_roots {
        ensure(
            !new_roots
                .iter()
                .flatten()
                .any(|root| crate::stable_targets::contains(*area, *root)),
            "The AI included a selected element inside a new area. Your page is unchanged.",
        )?;
    }
    for root in old_roots {
        root.remove_from_parent();
    }
    for root in new_roots.into_iter().flatten() {
        root.remove_from_parent();
    }
    for root in area_roots {
        root.remove_from_parent();
    }
    ensure(
        canonical(before.root()) == canonical(after.root()),
        "The AI changed content outside the selected elements. Your page is unchanged.",
    )
}

/// Small-screen media queries only: every width limit is at most 820 px and none sets a minimum
/// above it, so desktop layouts can never match.
fn phone_query(query: &str) -> bool {
    let query = query.to_ascii_lowercase();
    let limit = |value: &str, unit: &str| -> Option<f64> {
        let number: f64 = value.parse().ok()?;
        Some(match unit {
            "px" => number,
            "em" | "rem" => number * 16.0,
            _ => return None,
        })
    };
    let mut maxima = Vec::new();
    let mut rest = query.as_str();
    while let Some(index) = rest.find("max-width") {
        rest = &rest[index + "max-width".len()..];
        let value: String = rest
            .trim_start_matches([':', ' '])
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let unit: String = rest
            .trim_start_matches([':', ' '])
            .chars()
            .skip(value.len())
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        maxima.push(limit(&value, &unit));
    }
    let mut range = query.as_str();
    while let Some(index) = range.find("width <") {
        range = &range[index + "width <".len()..];
        let tail = range.trim_start_matches(['=', ' ']);
        let value: String = tail
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let unit: String = tail
            .chars()
            .skip(value.len())
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        maxima.push(limit(&value, &unit));
    }
    !maxima.is_empty()
        && maxima
            .iter()
            .all(|value| value.is_some_and(|value| value <= 820.0))
        && !query.contains("orientation")
        && !query.contains("print")
}

/// Page CSS with its small-screen rules removed, printed minified so formatting differences don't matter.
fn desktop_css(css: &str) -> Result<String> {
    use lightningcss::rules::CssRule;
    use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
    use lightningcss::traits::ToCss;
    let mut sheet = StyleSheet::parse(css, ParserOptions::default()).map_err(|_| {
        crate::Error("The AI returned CSS that could not be read. Your page is unchanged.".into())
    })?;
    sheet.rules.0.retain(|rule| match rule {
        CssRule::Media(media) => !media
            .query
            .to_css_string(PrinterOptions::default())
            .is_ok_and(|query| phone_query(&query)),
        _ => true,
    });
    sheet
        .to_css(PrinterOptions {
            minify: true,
            ..PrinterOptions::default()
        })
        .map(|output| output.code)
        .map_err(|_| {
            crate::Error("The page CSS could not be compared. Your page is unchanged.".into())
        })
}

/// A phone fix keeps the page's HTML, JavaScript and desktop CSS identical.
fn validate_phone_scope(original: &crate::Page, candidate: &crate::SiteContent) -> Result<()> {
    let source = original
        .content
        .as_ref()
        .ok_or_else(|| crate::Error("Generate this page before fixing it for phones.".into()))?;
    ensure(
        canonical(crate::editing::parse(&source.html)?.root())
            == canonical(crate::editing::parse(&candidate.html)?.root()),
        "The AI changed the page content during a phone fix. Your page is unchanged.",
    )?;
    ensure(
        source.javascript == candidate.javascript,
        "The AI changed page behavior during a phone fix. Your page is unchanged.",
    )?;
    ensure(
        desktop_css(&source.css)? == desktop_css(&candidate.css)?,
        "The AI changed the desktop design during a phone fix. Your page is unchanged.",
    )
}

#[cfg(test)]
mod phone_tests {
    use super::*;

    #[test]
    fn only_small_screen_queries_count_as_phone_rules() {
        assert!(phone_query("(max-width: 767px)"));
        assert!(phone_query("screen and (max-width:600px)"));
        assert!(phone_query("(min-width: 400px) and (max-width: 47.5em)"));
        assert!(phone_query("(width < 768px)"));
        assert!(!phone_query("(max-width: 1200px)"));
        assert!(!phone_query("(min-width: 768px)"));
        assert!(!phone_query("print and (max-width: 600px)"));
        assert!(!phone_query(
            "(max-width: 600px) and (orientation: landscape)"
        ));
    }

    #[test]
    fn desktop_css_ignores_phone_rules_and_formatting() {
        let before = "body { color: red }\nh1 { font-size: 40px }";
        let after =
            "body{color:red}h1{font-size:40px}@media (max-width: 767px){h1{font-size:28px}}";
        assert_eq!(desktop_css(before).unwrap(), desktop_css(after).unwrap());
        let changed = "body{color:blue}@media (max-width: 767px){h1{font-size:28px}}";
        assert_ne!(desktop_css(before).unwrap(), desktop_css(changed).unwrap());
        let wide =
            "body{color:red}h1{font-size:40px}@media (max-width: 1400px){h1{font-size:28px}}";
        assert_ne!(desktop_css(before).unwrap(), desktop_css(wide).unwrap());
    }
}
