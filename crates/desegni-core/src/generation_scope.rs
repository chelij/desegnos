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
            if original.content.is_some() && !targeted {
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
        canonical(crate::editing::body(&before)?) == canonical(crate::editing::body(&after)?),
        "The AI changed content outside the selected elements. Your page is unchanged.",
    )
}
