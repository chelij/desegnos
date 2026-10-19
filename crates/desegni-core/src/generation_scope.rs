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
        for page in &self.pages {
            let original = project.page(page.page_id)?;
            let regions: Vec<_> = original
                .regions
                .iter()
                .filter(|region| targets.contains(&(page.page_id, region.id)))
                .collect();
            // Rectangular requests on an existing page have no trustworthy mapping
            // to old DOM nodes. Do not invent one from labels or model claims.
            let targeted = regions.iter().all(|region| region.element.is_some());
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
