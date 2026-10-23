//! Recovery is confined to the current contract. Every reconstructed result still passes the
//! ordinary validators before it can be applied; provider output never becomes a new baseline.
use crate::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_VALIDATION_REPAIR_ROUNDS: usize = 1;
pub const REPAIR_HEADING: &str = "\n## Validation repair\n\n";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationEvent {
    pub kind: String,
    pub disposition: String,
    pub region_id: Option<Uuid>,
    pub message: String,
}

/// Fixed labels only: counter keys never contain user text, selectors or identifiers.
pub fn validation_failure_kind(message: &str) -> &'static str {
    let message = message.to_ascii_lowercase();
    if message.contains("css") || message.contains("stylesheet") {
        "cssScope"
    } else if message.contains("shared")
        && (message.contains("javascript") || message.contains("script"))
    {
        "sharedJavascript"
    } else if message.contains("javascript") || message.contains("script") {
        "sourceSyntax"
    } else if message.contains("editor id")
        || message.contains("element id")
        || message.contains("data-desegnos-id")
    {
        "elementIdentity"
    } else if message.contains("image") || message.contains("asset") {
        "imageSource"
    } else if message.contains("root") || message.contains("identify each") {
        "regionRoot"
    } else if message.contains("outside") || message.contains("scope") {
        "outsideScope"
    } else if message.contains("html") || message.contains("line ") {
        "sourceSyntax"
    } else {
        "candidateContract"
    }
}

fn event(
    events: &mut Vec<ValidationEvent>,
    kind: &str,
    disposition: &str,
    id: Option<Uuid>,
    message: String,
) {
    events.push(ValidationEvent {
        kind: kind.into(),
        disposition: disposition.into(),
        region_id: id,
        message,
    });
}

/// Missing fields are resolved before typed parsing, without weakening the reply schema for
/// older snapshots. Empty strings mean unchanged only when there is a nonempty source.
pub fn recover_generation_reply(
    text: &str,
    snapshot: &RequirementsSnapshot,
    project: &Project,
) -> Result<(GenerationCandidate, Vec<ValidationEvent>)> {
    if snapshot.format_version()? < 17 {
        let mut candidate = GenerationCandidate::from_reply(text, snapshot, project)?;
        let mut events = vec![];
        for generated in &mut candidate.pages {
            generated.content.html = repair_page_metadata(
                &generated.content.html,
                project.page(generated.page_id)?,
                &mut events,
            )?;
        }
        candidate.validate(snapshot, project)?;
        if snapshot.is_flexible() {
            record_outside_changes(&candidate, snapshot, project, &mut events)?;
        }
        return Ok((candidate, events));
    }
    ensure(
        text.len() <= MAX_GENERATION_BYTES,
        "The AI reply exceeds the response limit.",
    )?;
    let mut value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| Error(format!("The reply is not valid JSON: {e}")))?;
    ensure(
        value.is_object(),
        "The reply must be a JSON object matching the supplied result schema.",
    )?;
    let mut events = vec![];
    let targets = snapshot.target_ids()?;
    let value_regions_implemented: std::collections::BTreeSet<_> = value["regions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["status"] == "implemented")
        .filter_map(|r| {
            r["regionId"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
        })
        .collect();
    if !snapshot.is_element_fix()
        && let Some(pages) = value["pages"].as_array_mut()
    {
        for generated in pages {
            let Some(page) = generated["pageId"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .and_then(|id| project.page(id).ok())
            else {
                continue;
            };
            let whole_page = snapshot.format_version()? >= 18
                && page.regions.iter().any(|region| {
                    targets.contains(&(page.id, region.id))
                        && region.is_whole_page(page)
                        && value_regions_implemented.contains(&region.id)
                });
            if generated["content"].is_null() {
                generated["content"] = serde_json::json!({});
            }
            if !generated["content"].is_object() {
                continue;
            }
            if let Some(source) = &page.content {
                for (field, old, label) in [
                    ("html", &source.html, "HTML"),
                    ("css", &source.css, "stylesheet"),
                    ("javascript", &source.javascript, "JavaScript"),
                ] {
                    if !old.is_empty()
                        && (generated["content"][field].is_null()
                            || (!whole_page && generated["content"][field].as_str() == Some("")))
                    {
                        generated["content"][field] = old.clone().into();
                        event(
                            &mut events,
                            "omittedField",
                            "normalised",
                            None,
                            format!("Kept the {label} unchanged (the AI omitted it)."),
                        );
                    } else if generated["content"][field].is_null() {
                        generated["content"][field] = "".into();
                    }
                }
            } else {
                for field in ["html", "css", "javascript"] {
                    if generated["content"][field].is_null() {
                        generated["content"][field] = "".into();
                    }
                }
            }
        }
    }
    // Omitted outcomes and pages mean unchanged, not permission to invent a replacement.
    if let Some(regions) = value["regions"].as_array_mut() {
        for (_, id) in &targets {
            if !regions
                .iter()
                .any(|r| r["regionId"].as_str() == Some(&id.to_string()))
            {
                regions.push(serde_json::json!({"regionId":id,"status":"implemented","explanation":"Kept unchanged (the AI omitted this area)."}));
                event(
                    &mut events,
                    "omittedRegion",
                    "normalised",
                    Some(*id),
                    "Kept the area unchanged (the AI omitted it).".into(),
                );
            }
        }
    }
    if !snapshot.is_element_fix() {
        let ready: Vec<_> = targets
            .iter()
            .filter(|(_, id)| {
                value["regions"].as_array().is_some_and(|rs| {
                    rs.iter()
                        .any(|r| r["regionId"] == id.to_string() && r["status"] == "implemented")
                })
            })
            .map(|(page, _)| *page)
            .collect();
        if let Some(pages) = value["pages"].as_array_mut() {
            for id in ready {
                if !pages.iter().any(|p| p["pageId"] == id.to_string())
                    && let Some(content) = &project.page(id)?.content
                {
                    pages.push(serde_json::json!({"pageId":id,"content":content}));
                    event(
                        &mut events,
                        "omittedPage",
                        "normalised",
                        None,
                        format!("Kept page {id} unchanged (the AI omitted it)."),
                    );
                }
            }
        }
    }
    if !snapshot.is_element_fix() && snapshot.format_version()? >= 18 {
        if let Some(pages) = value["pages"].as_array_mut() {
            for generated in pages {
                let Some(page) = generated["pageId"]
                    .as_str()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .and_then(|id| project.page(id).ok())
                else {
                    continue;
                };
                if let Some(html) = generated["content"]["html"].as_str() {
                    let repaired = if snapshot.is_flexible() {
                        repair_page_metadata(html, page, &mut events)
                    } else {
                        repair_mechanical_structure(
                            html,
                            page,
                            &targets,
                            &value_regions_implemented,
                            &mut events,
                        )
                    };
                    if let Ok(repaired) = repaired {
                        generated["content"]["html"] = repaired.into();
                    }
                }
            }
        }
    }
    if !snapshot.is_element_fix() {
        let mut failed_pages = vec![];
        let mut seen = std::collections::BTreeSet::new();
        if let Some(pages) = value["pages"].as_array_mut() {
            for page in pages.iter() {
                let id = page["pageId"]
                    .as_str()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .ok_or_else(|| Error("A generated page needs a valid pageId.".into()))?;
                ensure(
                    targets.iter().any(|(page, _)| *page == id) && seen.insert(id),
                    "The reply contains duplicate or unrequested pages.",
                )?;
                let checked = serde_json::from_value::<GeneratedPage>(page.clone())
                    .map_err(|error| Error(format!("Page {id}: {error}")))
                    .and_then(|page| {
                        page.content.validate()?;
                        ensure(
                            !page.content.html.trim().is_empty(),
                            "The generated page has no HTML.",
                        )
                    });
                if let Err(error) = checked {
                    failed_pages.push((id, error.to_string()));
                }
            }
            pages.retain(|page| {
                !failed_pages
                    .iter()
                    .any(|(id, _)| page["pageId"] == id.to_string())
            });
        }
        for (page, message) in failed_pages {
            for (_, id) in targets.iter().filter(|(id, _)| *id == page) {
                let reason = format!("Area {id}, page {page}: {message}");
                if let Some(region) = value["regions"]
                    .as_array_mut()
                    .and_then(|rs| rs.iter_mut().find(|r| r["regionId"] == id.to_string()))
                {
                    region["status"] = "conflict".into();
                    region["explanation"] = reason.chars().take(4096).collect::<String>().into();
                }
                event(&mut events, "pageSource", "conflicted", Some(*id), reason);
            }
        }
    }
    let text = serde_json::to_string(&value).map_err(|e| Error(e.to_string()))?;
    let mut candidate = GenerationCandidate::from_reply(&text, snapshot, project)?;
    for generated in &mut candidate.pages {
        generated.content.html = repair_page_metadata(
            &generated.content.html,
            project.page(generated.page_id)?,
            &mut events,
        )?;
    }
    candidate.validate_identity(snapshot)?;
    // Filter shared code even when HTML already validates. A model cannot authorize a
    // global rule by returning all requested regions as implemented.
    let original_candidate = candidate.clone();
    if !snapshot.is_flexible() {
        for (id, reason) in shared_dependencies(&candidate, snapshot, project)? {
            if let Some(region) = candidate
                .regions
                .iter_mut()
                .find(|r| r.region_id == id && r.status == RegionStatus::Implemented)
            {
                region.status = RegionStatus::Conflict;
                region.explanation = format!("Area {id}: {reason}").chars().take(4096).collect();
                event(
                    &mut events,
                    validation_failure_kind(&region.explanation),
                    "conflicted",
                    Some(id),
                    region.explanation.clone(),
                );
            }
        }
        for generated in &mut candidate.pages {
            if original_candidate.implemented_whole_page(snapshot, project.page(generated.page_id)?)
            {
                continue;
            }
            let old = page_source(project, generated.page_id);
            let (css, dropped) = scoped_stylesheet(
                &old.css,
                &generated.content.css,
                &original_candidate,
                &candidate.regions,
                generated.page_id,
                project.page(generated.page_id)?,
            )?;
            if !dropped && generated.content.css != css {
                event(
                    &mut events,
                    "formatting",
                    "normalised",
                    None,
                    format!(
                        "Normalised stylesheet formatting on page {}.",
                        generated.page_id
                    ),
                );
            }
            generated.content.css = css;
            if dropped {
                event(
                    &mut events,
                    "sharedCss",
                    "salvaged",
                    None,
                    format!(
                        "Dropped CSS outside accepted areas on page {}.",
                        generated.page_id
                    ),
                );
            }
            if generated.content.javascript != old.javascript {
                generated.content.javascript = old.javascript;
                event(
                    &mut events,
                    "sharedJavascript",
                    "salvaged",
                    None,
                    format!(
                        "Kept JavaScript unchanged on page {}; the change could affect other areas.",
                        generated.page_id
                    ),
                );
            }
        }
    }
    if snapshot.is_flexible() {
        candidate.validate(snapshot, project)?;
        record_outside_changes(&candidate, snapshot, project, &mut events)?;
        return Ok((candidate, events));
    }
    if !events
        .iter()
        .any(|e| e.kind == "omittedRegion" || e.kind == "omittedPage")
        && candidate.validate(snapshot, project).is_ok()
    {
        return Ok((candidate, events));
    }
    if let Err(error) = candidate.validate(snapshot, project) {
        event(
            &mut events,
            validation_failure_kind(&error.to_string()),
            "salvaged",
            None,
            error.to_string(),
        );
    }
    if candidate.pages.iter().any(|p| {
        project
            .page(p.page_id)
            .is_ok_and(|page| candidate.implemented_whole_page(snapshot, page))
    }) {
        candidate.validate(snapshot, project)?;
        return Ok((candidate, events));
    }
    salvage(candidate, snapshot, project, events)
}

/// Precise mode additionally repairs inert script placement when ownership is clear.
fn repair_mechanical_structure(
    html: &str,
    page: &Page,
    targets: &[(Uuid, Uuid)],
    implemented: &std::collections::BTreeSet<Uuid>,
    events: &mut Vec<ValidationEvent>,
) -> Result<String> {
    let html = repair_page_metadata(html, page, events)?;
    let doc = crate::editing::parse(&html)?;
    let before = crate::editing::parse(page.content.as_ref().map_or("", |c| c.html.as_str()))?;
    let mut changed = false;
    for region in page
        .regions
        .iter()
        .filter(|r| targets.contains(&(page.id, r.id)) && implemented.contains(&r.id))
    {
        let mut roots = if region.has_targets() {
            region
                .targets()
                .iter()
                .filter_map(|t| crate::editing::locate(&doc, t).ok())
                .collect::<Vec<_>>()
        } else {
            doc.select(&selector(region.id)).nodes().to_vec()
        };
        roots.dedup_by_key(|n| n.id);
        if roots.is_empty() {
            continue;
        }
        if roots.len() != 1 {
            continue;
        }
        let scripts = doc.select(&format!("script[data-desegnos-script=\"{}\"]", region.id));
        if scripts.nodes().len() != 1 {
            continue;
        }
        let script = scripts.nodes()[0];
        let root = roots[0];
        let unchanged = before
            .select(&format!("script[data-desegnos-script=\"{}\"]", region.id))
            .nodes()
            .iter()
            .any(|old| old.html() == script.html());
        if !unchanged
            && script.attr("type").as_deref() == Some(crate::region_scripts::SCRIPT_TYPE)
            && script.attr("src").is_none()
            && script.parent().is_some_and(|p| p.id != root.id)
            && !crate::stable_targets::contains(script, root)
        {
            // The explicit UUID and unique selected root establish ownership, even when
            // the model placed this inert source beside the root or in a nested wrapper.
            script.remove_from_parent();
            root.append_child(&script);
            changed = true;
            event(
                events,
                "regionRoot",
                "repaired",
                Some(region.id),
                "Moved the inert region script directly inside its uniquely owned root.".into(),
            );
        }
    }
    if changed {
        crate::editing::serialized(&doc)
    } else {
        Ok(html)
    }
}

/// Repair bookkeeping against the accepted source before any scope comparison. New copies
/// receive IDs on apply, after validation, so provider IDs never become authoritative.
pub(crate) fn repair_page_metadata(
    html: &str,
    page: &Page,
    events: &mut Vec<ValidationEvent>,
) -> Result<String> {
    use std::collections::BTreeMap;
    // Do not let DOM normalization hide malformed source while repairing metadata.
    crate::source::validate_html_before_repair(html)?;
    let doc = crate::editing::parse(html)?;
    let before = crate::editing::parse(page.content.as_ref().map_or("", |c| c.html.as_str()))?;
    let mut changed = false;
    // A pending request is editor state. Restore its marker on the retained element, or
    // restore its original subtree when the model removed that element altogether.
    for region in page.regions.iter().filter(|r| !r.completed) {
        let old = before.select(&selector(region.id));
        if old.nodes().len() != 1 || !doc.select(&selector(region.id)).nodes().is_empty() {
            continue;
        }
        let old = old.nodes()[0];
        let retained = old.attr(ELEMENT_ID_ATTRIBUTE).and_then(|id| {
            let nodes = doc.select(&format!("[{ELEMENT_ID_ATTRIBUTE}=\"{id}\"]"));
            (nodes.nodes().len() == 1).then(|| nodes.nodes()[0])
        });
        if let Some(node) = retained {
            node.set_attr("data-desegnos-region", &region.id.to_string());
        } else {
            // Prefer the original parent and next sibling to preserve document placement.
            let parent = old
                .parent()
                .and_then(|p| p.attr(ELEMENT_ID_ATTRIBUTE))
                .and_then(|id| {
                    let nodes = doc.select(&format!("[{ELEMENT_ID_ATTRIBUTE}=\"{id}\"]"));
                    (nodes.nodes().len() == 1).then(|| nodes.nodes()[0])
                })
                .unwrap_or(crate::editing::body(&doc)?);
            let next = old
                .next_element_sibling()
                .and_then(|node| node.attr(ELEMENT_ID_ATTRIBUTE))
                .and_then(|id| {
                    let nodes = doc.select(&format!("[{ELEMENT_ID_ATTRIBUTE}=\"{id}\"]"));
                    (nodes.nodes().len() == 1).then(|| nodes.nodes()[0])
                })
                .filter(|node| node.parent().is_some_and(|p| p.id == parent.id));
            if let Some(next) = next {
                next.before_html(old.html().to_string());
            } else {
                parent.append_html(old.html().to_string());
            }
        }
        changed = true;
        event(
            events,
            "regionRoot",
            "repaired",
            Some(region.id),
            "Restored a pending request marker.".into(),
        );
    }
    for attr in [ELEMENT_ID_ATTRIBUTE, "data-desegnos-region"] {
        let originals: BTreeMap<_, _> = before
            .select(&format!("[{attr}]"))
            .nodes()
            .iter()
            .filter_map(|node| node.attr(attr).map(|id| (id.to_string(), *node)))
            .collect();
        let mut groups = BTreeMap::<String, Vec<dom_query::NodeRef<'_>>>::new();
        for node in doc.select(&format!("[{attr}]")).nodes() {
            groups
                .entry(node.attr(attr).unwrap().to_string())
                .or_default()
                .push(*node);
        }
        for (id, nodes) in groups {
            let original = originals.get(&id).copied();
            let score = |node: dom_query::NodeRef<'_>| -> usize {
                original
                    .filter(|old| node.node_name() == old.node_name())
                    .map_or(0, |old| {
                        100 + usize::from(node.text() == old.text()) * 40
                            + usize::from(node.attr("class") == old.attr("class")) * 10
                            + usize::from(
                                node.parent().and_then(|p| p.attr(ELEMENT_ID_ATTRIBUTE))
                                    == old.parent().and_then(|p| p.attr(ELEMENT_ID_ATTRIBUTE)),
                            ) * 5
                            + usize::from(
                                node.prev_element_sibling()
                                    .and_then(|p| p.attr(ELEMENT_ID_ATTRIBUTE))
                                    == old
                                        .prev_element_sibling()
                                        .and_then(|p| p.attr(ELEMENT_ID_ATTRIBUTE)),
                            )
                    })
            };
            // max_by_key picks the last tie; reverse to retain document order on ties.
            let keep = (0..nodes.len())
                .rev()
                .max_by_key(|i| score(nodes[*i]))
                .unwrap();
            if attr == "data-desegnos-region"
                && nodes.len() > 1
                && nodes.iter().enumerate().any(|(i, node)| {
                    i != keep
                        && score(*node) == score(nodes[keep])
                        && node.html() != nodes[keep].html()
                })
            {
                // Distinct equally plausible roots do not establish one request owner.
                continue;
            }
            let valid = if attr == ELEMENT_ID_ATTRIBUTE {
                crate::stable_targets::valid_id(&id) && original.is_some()
            } else {
                Uuid::parse_str(&id).is_ok_and(|id| page.regions.iter().any(|r| r.id == id))
            };
            for (index, node) in nodes.iter().enumerate() {
                if index == keep && valid {
                    continue;
                }
                // Invalid region IDs are genuine integrity errors, not invented requests.
                if attr != ELEMENT_ID_ATTRIBUTE && !valid {
                    continue;
                }
                node.remove_attr(attr);
                changed = true;
                event(
                    events,
                    if attr == ELEMENT_ID_ATTRIBUTE {
                        "elementIdentity"
                    } else {
                        "regionRoot"
                    },
                    "repaired",
                    None,
                    if attr == ELEMENT_ID_ATTRIBUTE {
                        "Repaired an editor ID; new copies receive fresh unique IDs on apply."
                    } else {
                        "Removed a duplicate area marker from a copied element."
                    }
                    .into(),
                );
            }
        }
    }
    if changed {
        crate::editing::serialized(&doc)
    } else {
        Ok(html.into())
    }
}

fn record_outside_changes(
    candidate: &GenerationCandidate,
    snapshot: &RequirementsSnapshot,
    project: &Project,
    events: &mut Vec<ValidationEvent>,
) -> Result<()> {
    let targets = snapshot.target_ids()?;
    let mut labels = Vec::new();
    let mut add = |label: String| {
        if !labels.contains(&label) {
            labels.push(label);
        }
    };
    for generated in &candidate.pages {
        let page = project.page(generated.page_id)?;
        if candidate.implemented_whole_page(snapshot, page) {
            continue;
        }
        let old = compact_site_content(&page_source(project, page.id));
        let new = compact_site_content(&generated.content);
        if old.css != new.css {
            add("page styles".into());
        }
        let before = crate::editing::parse(&old.html)?;
        let after = crate::editing::parse(&new.html)?;
        let scripts = |doc: &dom_query::Document| {
            doc.select("script")
                .nodes()
                .iter()
                .map(|n| n.html().to_string())
                .collect::<Vec<_>>()
        };
        if old.javascript != new.javascript || scripts(&before) != scripts(&after) {
            add("page script".into());
        }
        for (index, region) in page.regions.iter().enumerate() {
            let selectors: Vec<_> = if region.has_targets() {
                region
                    .targets()
                    .iter()
                    .map(|t| format!("[{ELEMENT_ID_ATTRIBUTE}=\"{}\"]", t.id))
                    .collect()
            } else {
                vec![selector(region.id)]
            };
            if !targets.contains(&(page.id, region.id))
                && selectors
                    .iter()
                    .any(|s| before.select(s).html() != after.select(s).html())
            {
                add(format!("Box {}", index + 1));
            }
            // Compare the remaining page separately so named boxes are not also
            // redundantly reported as generic page content.
            {
                for s in selectors {
                    for doc in [&before, &after] {
                        for node in doc.select(&s).nodes() {
                            node.remove_from_parent();
                        }
                    }
                }
            }
        }
        for doc in [&before, &after] {
            for node in doc.select("script").nodes() {
                node.remove_from_parent();
            }
        }
        if before.root().html() != after.root().html() {
            add("page content".into());
        }
    }
    if !labels.is_empty() {
        event(
            events,
            "outsideChanges",
            "advisory",
            None,
            format!("Also changed: {}", labels.join(", ")),
        );
    }
    Ok(())
}

fn recovery_targets(region: &Region) -> Vec<&ElementTarget> {
    region
        .targets()
        .into_iter()
        .chain(region.elements_in_area().map(|bound| &bound.target))
        .collect()
}

fn selector(id: Uuid) -> String {
    format!("[data-desegnos-region=\"{id}\"]")
}
fn placeholder(id: Uuid) -> String {
    format!(
        "<section data-desegnos-region=\"{id}\" data-desegnos-placeholder=\"conflict\">Area needs a decision</section>"
    )
}

fn salvage(
    candidate: GenerationCandidate,
    snapshot: &RequirementsSnapshot,
    project: &Project,
    mut events: Vec<ValidationEvent>,
) -> Result<(GenerationCandidate, Vec<ValidationEvent>)> {
    // Preserve envelope checks (including duplicate/unrequested IDs) independently of source.
    let targets = snapshot.target_ids()?;
    ensure(
        candidate.regions.len() == targets.len()
            && candidate
                .regions
                .iter()
                .all(|r| targets.iter().filter(|(_, id)| *id == r.region_id).count() == 1)
            && candidate
                .regions
                .iter()
                .map(|r| r.region_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == targets.len(),
        "The reply contains duplicate or unrequested areas.",
    )?;
    ensure(
        candidate
            .pages
            .iter()
            .all(|p| targets.iter().any(|(id, _)| *id == p.page_id))
            && candidate
                .pages
                .iter()
                .map(|p| p.page_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == candidate.pages.len(),
        "The reply contains duplicate or unrequested pages.",
    )?;
    let mut result = candidate.clone();
    result.pages.clear();
    for outcome in &mut result.regions {
        outcome.status = RegionStatus::Conflict;
    }
    // Build a trustworthy starting page, with local placeholders only where no source exists.
    let mut baselines = std::collections::BTreeMap::new();
    for (page_id, _) in &targets {
        if baselines.contains_key(page_id) {
            continue;
        }
        let page = project.page(*page_id)?;
        let content = page.content.clone().unwrap_or(SiteContent {
            html: String::new(),
            css: String::new(),
            javascript: String::new(),
        });
        let doc = crate::editing::parse(&content.html)?;
        for (_, id) in targets.iter().filter(|(p, _)| p == page_id) {
            let region = page
                .regions
                .iter()
                .find(|r| r.id == *id)
                .expect("snapshot target");
            if recovery_targets(region).is_empty() && doc.select(&selector(*id)).nodes().is_empty()
            {
                crate::editing::body(&doc)?.append_html(placeholder(*id));
            }
        }
        baselines.insert(
            *page_id,
            SiteContent {
                html: crate::editing::serialized(&doc)?,
                ..content
            },
        );
    }
    for outcome in &candidate.regions {
        if outcome.status == RegionStatus::Conflict {
            continue;
        }
        let (page_id, _) = targets
            .iter()
            .find(|(_, id)| *id == outcome.region_id)
            .expect("checked target");
        let page = project.page(*page_id)?;
        let region = page
            .regions
            .iter()
            .find(|r| r.id == outcome.region_id)
            .expect("snapshot target");
        let attempt = (|| -> Result<GenerationCandidate> {
            let incoming = candidate
                .pages
                .iter()
                .find(|p| p.page_id == *page_id)
                .ok_or_else(|| Error("The AI omitted this area's page source.".into()))?;
            let source = baselines.get(page_id).expect("baseline");
            let before = crate::editing::parse(&source.html)?;
            let after = crate::editing::parse(&incoming.content.html)?;
            let omitted = events
                .iter()
                .any(|e| e.kind == "omittedRegion" && e.region_id == Some(region.id))
                || events.iter().any(|e| e.kind == "omittedPage")
                    && incoming.content == page.content.clone().unwrap_or_else(|| source.clone());
            if omitted
                && (!recovery_targets(region).is_empty()
                    || page.content.is_some()
                        && !crate::editing::parse(&page.content.as_ref().expect("source").html)?
                            .select(&selector(region.id))
                            .nodes()
                            .is_empty())
            {
                // The baseline already contains the unchanged region in its original position.
            } else if !recovery_targets(region).is_empty() {
                for target in recovery_targets(region) {
                    let old = crate::editing::locate(&before, target)?;
                    match crate::editing::locate(&after, target) {
                        Ok(new) => old.replace_with_html(new.html().to_string()),
                        Err(_) if region.allow_target_removal => old.remove_from_parent(),
                        Err(e) => return Err(Error(format!("Element {}: {e}", target.id))),
                    }
                }
            } else {
                let nodes = after.select(&selector(region.id));
                ensure(
                    nodes.nodes().len() == 1,
                    format!(
                        "Area {} needs exactly one {} root; found {}.",
                        region.id,
                        selector(region.id),
                        nodes.nodes().len()
                    ),
                )?;
                let old = before.select(&selector(region.id));
                ensure(
                    old.nodes().len() == 1,
                    "The original area has no unique root to replace.",
                )?;
                old.nodes()[0].replace_with_html(nodes.nodes()[0].html().to_string());
            }
            let mut trial = result.clone();
            trial
                .regions
                .iter_mut()
                .find(|r| r.region_id == region.id)
                .expect("outcome")
                .status = RegionStatus::Implemented;
            trial.pages.retain(|p| p.page_id != *page_id);
            trial.pages.push(GeneratedPage {
                page_id: *page_id,
                content: SiteContent {
                    html: crate::editing::serialized(&before)?,
                    ..source.clone()
                },
            });
            trial.validate(snapshot, project)?;
            Ok(trial)
        })();
        match attempt {
            Ok(trial) => {
                baselines.insert(
                    *page_id,
                    trial
                        .pages
                        .iter()
                        .find(|p| p.page_id == *page_id)
                        .expect("trial page")
                        .content
                        .clone(),
                );
                result = trial;
                event(
                    &mut events,
                    "regionValidation",
                    "salvaged",
                    Some(region.id),
                    format!("Kept the validated changes for area {}.", region.id),
                );
            }
            Err(error) => {
                let excerpt = candidate
                    .pages
                    .iter()
                    .find(|p| p.page_id == *page_id)
                    .and_then(|p| crate::editing::parse(&p.content.html).ok())
                    .map(|doc| {
                        doc.select(&selector(region.id))
                            .nodes()
                            .first()
                            .map(|n| n.html().to_string())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default()
                    .chars()
                    .take(180)
                    .collect::<String>();
                let message = format!(
                    "Area {} ({}): {}{}",
                    region.id,
                    selector(region.id),
                    error,
                    if excerpt.is_empty() {
                        String::new()
                    } else {
                        format!(" Excerpt: {excerpt}")
                    }
                );
                result
                    .regions
                    .iter_mut()
                    .find(|r| r.region_id == region.id)
                    .expect("outcome")
                    .explanation = message.chars().take(4096).collect();
                event(
                    &mut events,
                    validation_failure_kind(&message),
                    "conflicted",
                    Some(region.id),
                    message,
                );
            }
        }
    }
    // Shared code is never copied from an untrusted page as part of a region transplant.
    for incoming in &candidate.pages {
        if let Some(accepted) = result
            .pages
            .iter_mut()
            .find(|p| p.page_id == incoming.page_id)
        {
            let old = page_source(project, incoming.page_id);
            let (css, dropped) = scoped_stylesheet(
                &old.css,
                &incoming.content.css,
                &candidate,
                &result.regions,
                incoming.page_id,
                project.page(incoming.page_id)?,
            )?;
            accepted.content.css = css;
            if dropped {
                event(
                    &mut events,
                    "sharedCss",
                    "salvaged",
                    None,
                    format!(
                        "Dropped CSS outside accepted areas on page {}.",
                        incoming.page_id
                    ),
                );
            }
            if incoming.content.javascript != old.javascript {
                event(
                    &mut events,
                    "sharedJavascript",
                    "salvaged",
                    None,
                    format!(
                        "Kept JavaScript unchanged on page {}; the change could affect other areas.",
                        incoming.page_id
                    ),
                );
            }
        }
    }
    result.validate(snapshot, project)?;
    Ok((result, events))
}

fn page_source(project: &Project, page: Uuid) -> SiteContent {
    project
        .page(page)
        .ok()
        .and_then(|p| p.content.clone())
        .unwrap_or(SiteContent {
            html: String::new(),
            css: String::new(),
            javascript: String::new(),
        })
}

/// Parse rules with the same CSS parser and conservative selector policy as element fixes.
/// A region marker is treated as an additional selected root; descendants require their own ID.
fn scoped_stylesheet(
    before: &str,
    after: &str,
    candidate: &GenerationCandidate,
    outcomes: &[RegionOutcome],
    page: Uuid,
    original: &Page,
) -> Result<(String, bool)> {
    use lightningcss::{
        stylesheet::{ParserOptions, PrinterOptions, StyleSheet},
        traits::ToCss,
    };
    if before == after {
        return Ok((before.into(), false));
    }
    let old = StyleSheet::parse(before, ParserOptions::default())
        .map_err(|_| Error("The original stylesheet cannot be parsed.".into()))?;
    let Ok(new) = StyleSheet::parse(after, ParserOptions::default()) else {
        return Ok((before.into(), true));
    };
    let old_rules: Vec<_> = old
        .rules
        .0
        .iter()
        .filter_map(|r| r.to_css_string(PrinterOptions::default()).ok())
        .collect();
    let ids = css_scope_ids(candidate, outcomes, page, original);
    let mut css = before.to_string();
    let mut dropped = false;
    fn append_rules(
        list: &lightningcss::rules::CssRuleList<'_>,
        old: &[String],
        ids: &std::collections::BTreeSet<(String, String)>,
        css: &mut String,
        dropped: &mut bool,
    ) {
        for rule in &list.0 {
            let Ok(text) = rule.to_css_string(PrinterOptions::default()) else {
                *dropped = true;
                continue;
            };
            if old.contains(&text) {
                continue;
            }
            if crate::generation_scope::validate_scoped_rules(&text, ids).is_ok() {
                css.push('\n');
                css.push_str(&text);
            } else if let lightningcss::rules::CssRule::Media(media) = rule {
                let mut inner = String::new();
                append_rules(&media.rules, old, ids, &mut inner, dropped);
                if !inner.is_empty() {
                    if let Ok(query) = media.query.to_css_string(PrinterOptions::default()) {
                        css.push_str(&format!("\n@media {query}{{{inner}}}"));
                    } else {
                        *dropped = true;
                    }
                }
            } else {
                *dropped = true;
            }
        }
    }
    append_rules(&new.rules, &old_rules, &ids, &mut css, &mut dropped);
    if old_rules.iter().any(|old| {
        !new.rules
            .0
            .iter()
            .any(|r| r.to_css_string(PrinterOptions::default()).ok().as_ref() == Some(old))
    }) {
        dropped = true;
    }
    Ok((css, dropped))
}

fn css_rule_allowed(
    css: &str,
    candidate: &GenerationCandidate,
    outcomes: &[RegionOutcome],
    page: Uuid,
    original: &Page,
) -> bool {
    let ids = css_scope_ids(candidate, outcomes, page, original);
    crate::generation_scope::validate_scoped_rules(css, &ids).is_ok()
}

fn css_scope_ids(
    candidate: &GenerationCandidate,
    outcomes: &[RegionOutcome],
    page: Uuid,
    original: &Page,
) -> std::collections::BTreeSet<(String, String)> {
    let Some(content) = candidate.pages.iter().find(|p| p.page_id == page) else {
        return Default::default();
    };
    let Ok(doc) = crate::editing::parse(&content.content.html) else {
        return Default::default();
    };
    let mut ids = std::collections::BTreeSet::new();
    for outcome in outcomes
        .iter()
        .filter(|r| r.status == RegionStatus::Implemented)
    {
        for root in doc.select(&selector(outcome.region_id)).nodes() {
            ids.insert((
                "data-desegnos-region".to_string(),
                outcome.region_id.to_string(),
            ));
            if let Some(id) = root.attr(ELEMENT_ID_ATTRIBUTE) {
                ids.insert((ELEMENT_ID_ATTRIBUTE.into(), id.to_string()));
            }
            for node in doc
                .select(&format!(
                    "{} [{}]",
                    selector(outcome.region_id),
                    ELEMENT_ID_ATTRIBUTE
                ))
                .nodes()
            {
                if let Some(id) = node.attr(ELEMENT_ID_ATTRIBUTE) {
                    ids.insert((ELEMENT_ID_ATTRIBUTE.into(), id.to_string()));
                }
            }
        }
    }
    for outcome in outcomes
        .iter()
        .filter(|r| r.status == RegionStatus::Implemented)
    {
        if let Some(region) = original.regions.iter().find(|r| r.id == outcome.region_id) {
            for target in recovery_targets(region) {
                if let Ok(root) = crate::editing::locate(&doc, target) {
                    let mut pending = vec![root];
                    while let Some(node) = pending.pop() {
                        if let Some(id) = node.attr(ELEMENT_ID_ATTRIBUTE) {
                            ids.insert((ELEMENT_ID_ATTRIBUTE.into(), id.to_string()));
                        }
                        pending.extend(node.children());
                    }
                }
            }
        }
    }
    ids
}

pub(crate) fn validate_region_css(
    before: &str,
    after: &str,
    candidate: &GenerationCandidate,
    page: &Page,
) -> Result<()> {
    let (filtered, dropped) =
        scoped_stylesheet(before, after, candidate, &candidate.regions, page.id, page)?;
    ensure(
        !dropped
            && compact_site_content(&SiteContent {
                html: String::new(),
                css: filtered,
                javascript: String::new(),
            })
            .css == compact_site_content(&SiteContent {
                html: String::new(),
                css: after.into(),
                javascript: String::new(),
            })
            .css,
        "Shared CSS may only add rules for accepted areas.",
    )
}

impl RequirementsSnapshot {
    pub fn validation_repair(&self, violations: &[ValidationEvent]) -> Result<Option<Self>> {
        if self.format_version()? < 17
            || self.markdown.contains(REPAIR_HEADING)
            || MAX_VALIDATION_REPAIR_ROUNDS == 0
        {
            return Ok(None);
        }
        let failed: Vec<_> = violations
            .iter()
            .filter(|e| e.disposition == "conflicted")
            .collect();
        if failed.is_empty() {
            return Ok(None);
        }
        let mut next = self.clone();
        next.markdown.push_str(REPAIR_HEADING);
        next.markdown.push_str("Correct only the areas/pages listed below. Use the same result schema and the new requirements digest. Omit unchanged areas/elements. Preserve other content, IDs, CSS and JavaScript. Return the final corrected output now.\n\n");
        next.markdown
            .push_str(&crate::requirements::fenced_json(&failed)?);
        ensure(
            next.markdown.len() <= MAX_REQUIREMENTS_BYTES,
            "The repair context exceeds the size limit.",
        )?;
        next.sha256 = digest(next.markdown.as_bytes());
        Ok(Some(next))
    }
}

/// A repair cannot replace already accepted regions, even when it returns a complete page.
pub fn merge_validation_repair(
    first: &GenerationCandidate,
    second: &GenerationCandidate,
    snapshot: &RequirementsSnapshot,
    project: &Project,
) -> Result<GenerationCandidate> {
    first.validate(snapshot, project)?;
    second.validate(snapshot, project)?;
    let targets = snapshot.target_ids()?;
    let mut merged = first.clone();
    for outcome in &second.regions {
        if first
            .regions
            .iter()
            .any(|r| r.region_id == outcome.region_id && r.status == RegionStatus::Implemented)
        {
            continue;
        }
        if outcome.status == RegionStatus::Conflict {
            *merged
                .regions
                .iter_mut()
                .find(|r| r.region_id == outcome.region_id)
                .expect("validated outcome") = outcome.clone();
            continue;
        }
        let (page_id, _) = targets
            .iter()
            .find(|(_, id)| *id == outcome.region_id)
            .expect("validated target");
        let original = project.page(*page_id)?;
        let region = original
            .regions
            .iter()
            .find(|r| r.id == outcome.region_id)
            .expect("target");
        let attempt = (|| -> Result<GenerationCandidate> {
            let incoming = second
                .pages
                .iter()
                .find(|p| p.page_id == *page_id)
                .ok_or_else(|| Error("The repaired area's source is missing.".into()))?;
            let source = if let Some(page) = merged.pages.iter().find(|p| p.page_id == *page_id) {
                page.content.clone()
            } else {
                let mut content = page_source(project, *page_id);
                let doc = crate::editing::parse(&content.html)?;
                for (_, id) in targets.iter().filter(|(id, _)| id == page_id) {
                    let region = original
                        .regions
                        .iter()
                        .find(|r| r.id == *id)
                        .expect("target");
                    if recovery_targets(region).is_empty()
                        && doc.select(&selector(*id)).nodes().is_empty()
                    {
                        crate::editing::body(&doc)?.append_html(placeholder(*id));
                    }
                }
                content.html = crate::editing::serialized(&doc)?;
                content
            };
            let doc = crate::editing::parse(&source.html)?;
            let repaired = crate::editing::parse(&incoming.content.html)?;
            if !recovery_targets(region).is_empty() {
                for target in recovery_targets(region) {
                    let old = crate::editing::locate(&doc, target)?;
                    match crate::editing::locate(&repaired, target) {
                        Ok(new) => old.replace_with_html(new.html().to_string()),
                        Err(_) if region.allow_target_removal => old.remove_from_parent(),
                        Err(e) => return Err(e),
                    }
                }
            } else {
                let old = doc.select(&selector(region.id));
                let new = repaired.select(&selector(region.id));
                ensure(
                    old.nodes().len() == 1 && new.nodes().len() == 1,
                    format!("Repair area {} has no unique root.", region.id),
                )?;
                old.nodes()[0].replace_with_html(new.nodes()[0].html().to_string());
            }
            let mut trial = merged.clone();
            *trial
                .regions
                .iter_mut()
                .find(|r| r.region_id == outcome.region_id)
                .expect("outcome") = outcome.clone();
            trial.pages.retain(|p| p.page_id != *page_id);
            trial.pages.push(GeneratedPage {
                page_id: *page_id,
                content: SiteContent {
                    html: crate::editing::serialized(&doc)?,
                    ..source.clone()
                },
            });
            let (css, _) = scoped_stylesheet(
                &source.css,
                &incoming.content.css,
                &trial,
                &trial.regions,
                *page_id,
                original,
            )?;
            trial
                .pages
                .iter_mut()
                .find(|p| p.page_id == *page_id)
                .expect("page")
                .content
                .css = css;
            trial
                .component_requests
                .retain(|r| r.region_id != outcome.region_id);
            trial.component_requests.extend(
                second
                    .component_requests
                    .iter()
                    .filter(|r| r.region_id == outcome.region_id)
                    .cloned(),
            );
            trial.validate(snapshot, project)?;
            Ok(trial)
        })();
        match attempt {
            Ok(trial) => merged = trial,
            Err(error) => {
                merged
                    .regions
                    .iter_mut()
                    .find(|r| r.region_id == outcome.region_id)
                    .expect("outcome")
                    .explanation = format!("Area {}: {error}", outcome.region_id)
                    .chars()
                    .take(4096)
                    .collect();
            }
        }
    }
    merged.validate(snapshot, project)?;
    Ok(merged)
}

/// A malformed final reply still leaves explicit open requests, never unvalidated source.
pub fn conflicted_generation(
    snapshot: &RequirementsSnapshot,
    project: &Project,
    reason: &str,
) -> Result<GenerationCandidate> {
    let candidate = GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "These areas need a decision.".into(),
        regions: snapshot
            .target_ids()?
            .iter()
            .map(|(_, id)| RegionOutcome {
                region_id: *id,
                status: RegionStatus::Conflict,
                explanation: format!("Area {id}: {reason}").chars().take(4096).collect(),
            })
            .collect(),
        pages: vec![],
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],

        documents: vec![],
    };
    candidate.validate(snapshot, project)?;
    Ok(candidate)
}

// Conservatively treat a dropped rule matching an area's root, descendants or ancestors as
// required by that area. Keeping styled markup while discarding its dependencies is not salvage.
fn shared_dependencies(
    candidate: &GenerationCandidate,
    snapshot: &RequirementsSnapshot,
    project: &Project,
) -> Result<Vec<(Uuid, String)>> {
    use lightningcss::{
        rules::{CssRule, CssRuleList},
        stylesheet::{ParserOptions, PrinterOptions, StyleSheet},
        traits::ToCss,
    };
    let mut failures = vec![];
    for page in &candidate.pages {
        let original = project.page(page.page_id)?;
        if candidate.implemented_whole_page(snapshot, original) {
            continue;
        }
        let old = page_source(project, page.page_id);
        let doc = crate::editing::parse(&page.content.html)?;
        let old_doc = crate::editing::parse(&old.html)?;
        let mut roots = vec![];
        for outcome in candidate
            .regions
            .iter()
            .filter(|r| r.status == RegionStatus::Implemented)
        {
            let Some(region) = original.regions.iter().find(|r| r.id == outcome.region_id) else {
                continue;
            };
            let mut nodes = doc.select(&selector(region.id)).nodes().to_vec();
            for target in recovery_targets(region) {
                if let Ok(node) = crate::editing::locate(&doc, target) {
                    nodes.push(node);
                }
            }
            let mut old_roots = old_doc.select(&selector(region.id)).nodes().to_vec();
            for target in recovery_targets(region) {
                if let Ok(node) = crate::editing::locate(&old_doc, target) {
                    old_roots.push(node);
                }
            }
            let shared = |root: dom_query::NodeRef<'_>| {
                root.descendants_it()
                    .filter(|node| {
                        matches!(
                            node.node_name().as_deref(),
                            Some("style" | "script" | "link")
                        )
                    })
                    .map(|node| node.html().to_string())
                    .collect::<Vec<_>>()
            };
            let old_shared: Vec<_> = old_roots.iter().flat_map(|root| shared(*root)).collect();
            for root in &nodes {
                for node in root.descendants_it().filter(|node| {
                    matches!(
                        node.node_name().as_deref(),
                        Some("style" | "script" | "link")
                    )
                }) {
                    if snapshot.format_version()? >= 18
                        && node.node_name().as_deref() == Some("script")
                        && node.attr("data-desegnos-script").is_some()
                        && node.attr("type").as_deref() == Some(crate::region_scripts::SCRIPT_TYPE)
                    {
                        // Ownership and AST checks run in validate_scope, including salvage.
                        continue;
                    }
                    let html = node.html().to_string();
                    if old_shared.contains(&html) {
                        continue;
                    }
                    if node.node_name().as_deref() == Some("style")
                        && css_rule_allowed(
                            node.text().as_ref(),
                            candidate,
                            std::slice::from_ref(outcome),
                            page.page_id,
                            original,
                        )
                    {
                        continue;
                    }
                    failures.push((region.id, format!("New or changed <{}> code cannot affect other areas. Keep shared code unchanged and put scoped styling in the page's css field. Excerpt: {}", node.node_name().unwrap_or_default(), html.chars().take(180).collect::<String>())));
                }
            }
            roots.push((region.id, nodes));
            if page.content.javascript != old.javascript {
                failures.push((
                    region.id,
                    crate::region_scripts::SHARED_JAVASCRIPT_REASON.into(),
                ));
            }
        }
        if page.content.css == old.css {
            continue;
        }
        let sheet = match StyleSheet::parse(&page.content.css, ParserOptions::default()) {
            Ok(sheet) => sheet,
            Err(error) => {
                for (id, _) in &roots {
                    failures.push((
                        *id,
                        format!(
                            "The area's CSS cannot be parsed: {}. Excerpt: {}",
                            error.to_string().chars().take(180).collect::<String>(),
                            page.content.css.chars().take(180).collect::<String>()
                        ),
                    ));
                }
                continue;
            }
        };
        let old_rules = StyleSheet::parse(&old.css, ParserOptions::default())
            .ok()
            .map(|sheet| {
                sheet
                    .rules
                    .0
                    .iter()
                    .filter_map(|r| r.to_css_string(PrinterOptions::default()).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        fn inspect(
            list: &CssRuleList<'_>,
            selectors: &mut Vec<(String, String)>,
            old: &[String],
            ids: &std::collections::BTreeSet<(String, String)>,
        ) {
            for rule in &list.0 {
                let Ok(text) = rule.to_css_string(PrinterOptions::default()) else {
                    continue;
                };
                if old.contains(&text)
                    || crate::generation_scope::validate_scoped_rules(&text, ids).is_ok()
                {
                    continue;
                }
                match rule {
                    CssRule::Style(style) => {
                        if let Ok(selector) =
                            style.selectors.to_css_string(PrinterOptions::default())
                        {
                            selectors.push((selector, text));
                        }
                    }
                    CssRule::Media(media) => inspect(&media.rules, selectors, old, ids),
                    _ => selectors.push(("*".into(), text)),
                }
            }
        }
        let ids = css_scope_ids(candidate, &candidate.regions, page.page_id, original);
        let mut selectors = vec![];
        inspect(&sheet.rules, &mut selectors, &old_rules, &ids);
        for (selector, text) in selectors {
            let base_selectors = selector
                .split(',')
                .map(|selector| {
                    let mut base = selector.trim();
                    while let Some(next) = crate::generation_scope::CSS_SCOPE_STATES
                        .iter()
                        .find_map(|state| base.strip_suffix(state))
                    {
                        base = next;
                    }
                    base
                })
                .collect::<Vec<_>>()
                .join(",");
            let matches = doc.select(&base_selectors);
            for (id, nodes) in &roots {
                if nodes.iter().any(|root| {
                    matches.nodes().iter().any(|node| {
                        crate::stable_targets::contains(*node, *root)
                            || crate::stable_targets::contains(*root, *node)
                    })
                }) {
                    failures.push((*id, format!("CSS selector {selector} also affects content outside accepted areas. This area's styling depends on it; use region-scoped CSS instead. Excerpt: {}", text.chars().take(180).collect::<String>())));
                }
            }
        }
    }
    Ok(failures)
}
