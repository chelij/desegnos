//! Structural scope checks; visual geometry remains a browser benchmark.
use crate::{GenerationCandidate, Project, RequirementsSnapshot, Result, ensure};
use dom_query::NodeRef;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const MARKER: &str = "data-desegnos-region";

pub(crate) const CSS_SCOPE_STATES: &[&str] = &[
    "::before",
    "::after",
    ":before",
    ":after",
    ":focus-visible",
    ":focus-within",
    ":disabled",
    ":checked",
    ":hover",
    ":focus",
    ":active",
];

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

// Keep diagnostics bounded and omit attribute values and page text from logs.
fn outside_area(node: NodeRef<'_>, attribute: Option<&str>) -> String {
    let name = node.node_name().unwrap_or_default();
    let detail = match attribute {
        Some(attribute) => format!("{name} {attribute}"),
        None => name.to_string(),
    };
    let mut short: String = detail.chars().take(96).collect();
    if detail.chars().count() > 96 {
        short.push('…');
    }
    format!(
        "The AI added or changed <{short}> outside the requested areas. Your page is unchanged."
    )
}

fn empty_layout(node: NodeRef<'_>) -> Result<()> {
    if node.is_comment() || node.is_text() && node.text().trim().is_empty() {
        return Ok(());
    }
    ensure(
        matches!(
            node.node_name().as_deref(),
            Some("body" | "div" | "main" | "section")
        ),
        outside_area(node, None),
    )?;
    for attr in node.attrs() {
        let name = attr.name.local.as_ref();
        let allowed = attr.name.ns.as_ref().is_empty()
            && (matches!(
                name,
                "class" | "id" | "style" | "aria-label" | "aria-labelledby"
            ) || name.starts_with("data-") && !name.starts_with("data-desegnos")
                || name == "role"
                    && matches!(
                        attr.value.trim(),
                        "main" | "region" | "presentation" | "none" | "group"
                    ));
        ensure(allowed, outside_area(node, Some(name)))?;
    }
    for child in node.children() {
        ensure(
            !child.is_text() || child.text().trim().is_empty(),
            outside_area(node, Some("text")),
        )?;
        empty_layout(child)?;
    }
    Ok(())
}

/// Find the first canonical difference, including removed nodes and attributes.
fn outside_difference(before: NodeRef<'_>, after: NodeRef<'_>) -> Option<String> {
    if canonical(before) == canonical(after) {
        return None;
    }
    if before.node_name() != after.node_name() || before.is_text() || after.is_text() {
        return Some(outside_area(after, None));
    }
    for (node, other) in [(after, before), (before, after)] {
        for attr in node.attrs() {
            if !other
                .attrs()
                .iter()
                .any(|old| old.name == attr.name && old.value == attr.value)
            {
                return Some(outside_area(after, Some(attr.name.local.as_ref())));
            }
        }
    }
    fn children(node: NodeRef<'_>) -> Vec<NodeRef<'_>> {
        node.children()
            .into_iter()
            .filter(|child| !child.is_comment())
            .collect()
    }
    let old = children(before);
    let new = children(after);
    for index in 0..old.len().max(new.len()) {
        match (old.get(index), new.get(index)) {
            (Some(old), Some(new)) if old.is_text() || new.is_text() => {
                if canonical(*old) != canonical(*new) {
                    return Some(outside_area(after, Some("text")));
                }
            }
            (Some(old), Some(new)) => {
                if let Some(message) = outside_difference(*old, *new) {
                    return Some(message);
                }
            }
            (_, Some(node)) | (Some(node), _) => return Some(outside_area(*node, None)),
            _ => {}
        }
    }
    Some(outside_area(after, None))
}

impl GenerationCandidate {
    /// Only an implemented, frozen page request can authorize shared-code changes.
    pub(crate) fn implemented_whole_page(
        &self,
        snapshot: &RequirementsSnapshot,
        page: &crate::Page,
    ) -> bool {
        snapshot.target_ids().is_ok_and(|targets| {
            page.regions.iter().any(|region| {
                targets.contains(&(page.id, region.id))
                    && region.is_whole_page(page)
                    && self.regions.iter().any(|outcome| {
                        outcome.region_id == region.id
                            && outcome.status == crate::RegionStatus::Implemented
                    })
            })
        })
    }

    pub(crate) fn validate_conflicting_regions(
        &self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
    ) -> Result<()> {
        fn placeholder(node: NodeRef<'_>) -> bool {
            node.attr("data-desegnos-placeholder").as_deref() == Some("conflict")
                && node.text().chars().count() <= 160
                && node.children().iter().all(|child| child.is_text())
        }
        for generated in &self.pages {
            let page = project.page(generated.page_id)?;
            let whole_page = self.implemented_whole_page(snapshot, page);
            let conflicts: Vec<_> = page
                .regions
                .iter()
                .filter(|region| {
                    region.frame_target.is_none()
                        && !region.is_whole_page(page)
                        && self.regions.iter().any(|outcome| {
                            outcome.region_id == region.id
                                && outcome.status == crate::RegionStatus::Conflict
                        })
                })
                .collect();
            if conflicts.is_empty() {
                continue;
            }
            let after_content = crate::compact_site_content(&generated.content);
            let after = crate::editing::parse(&after_content.html)?;
            if let Some(source) = &page.content {
                let source = crate::compact_site_content(source);
                ensure(
                    whole_page
                        || (source.css == after_content.css
                            || crate::generation_recovery::validate_region_css(
                                &source.css,
                                &after_content.css,
                                self,
                                page,
                            )
                            .is_ok())
                            && source.javascript == after_content.javascript,
                    "A partial page must preserve shared CSS and JavaScript; use inline styles in ready regions.",
                )?;
                let before = crate::editing::parse(&source.html)?;
                let mut unbound_conflict = false;
                for region in conflicts {
                    if region.has_targets() {
                        for target in region.targets() {
                            ensure(
                                canonical(crate::editing::locate(&before, target)?)
                                    == canonical(crate::editing::locate(&after, target)?),
                                "A conflicted element must stay unchanged.",
                            )?;
                        }
                    } else {
                        let selector = format!("[{MARKER}=\"{}\"]", region.id);
                        let old = before.select(&selector);
                        let new = after.select(&selector);
                        if old.nodes().is_empty() {
                            ensure(
                                new.nodes().len() == 1 && placeholder(new.nodes()[0]),
                                "An unmapped conflicting area needs a short local placeholder.",
                            )?;
                            unbound_conflict = true;
                        } else {
                            ensure(
                                old.nodes().len() == 1
                                    && new.nodes().len() == 1
                                    && canonical(old.nodes()[0]) == canonical(new.nodes()[0]),
                                "A conflicted area must retain its unchanged region root.",
                            )?;
                        }
                    }
                }
                if unbound_conflict && !whole_page {
                    // No old DOM target maps to this rectangle. Preserve all existing content
                    // outside ready roots; only additive new areas or mapped replacements are safe.
                    for (_, id) in snapshot
                        .target_ids()?
                        .into_iter()
                        .filter(|(id, _)| *id == page.id)
                    {
                        let region = page
                            .regions
                            .iter()
                            .find(|region| region.id == id)
                            .expect("validated target");
                        let selector = format!("[{MARKER}=\"{id}\"]");
                        for doc in [&before, &after] {
                            for node in doc.select(&selector).nodes() {
                                node.remove_from_parent();
                            }
                            if self.regions.iter().any(|outcome| {
                                outcome.region_id == id
                                    && outcome.status == crate::RegionStatus::Implemented
                            }) {
                                for target in region.targets() {
                                    if let Ok(node) = crate::editing::locate(doc, target) {
                                        node.remove_from_parent();
                                    }
                                }
                            }
                        }
                    }
                    ensure(
                        canonical(crate::editing::body(&before)?)
                            == canonical(crate::editing::body(&after)?),
                        "An unmapped conflicting area requires preserving existing content outside ready roots.",
                    )?;
                }
            } else {
                for region in conflicts {
                    let nodes = after.select(&format!("[{MARKER}=\"{}\"]", region.id));
                    ensure(
                        nodes.nodes().len() == 1 && placeholder(nodes.nodes()[0]),
                        "A new conflicted area needs one short, clearly marked placeholder.",
                    )?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate_scope(
        &self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
    ) -> Result<()> {
        {
            self.validate_framed_documents(snapshot, project)?;
        }
        if snapshot.is_flexible() {
            for page in &self.pages {
                let doc = crate::editing::parse(&page.content.html)?;
                crate::stable_targets::validate_ids(&doc)?;
                let original = project.page(page.page_id)?;
                let before = crate::editing::parse(
                    original.content.as_ref().map_or("", |c| c.html.as_str()),
                )?;
                let mut seen = BTreeSet::new();
                for node in doc.select("[data-desegnos-region]").nodes() {
                    let id = node
                        .attr("data-desegnos-region")
                        .and_then(|id| Uuid::parse_str(&id).ok());
                    ensure(
                        id.is_some_and(|id| {
                            (original.regions.iter().any(|r| r.id == id)
                                || before
                                    .select(&format!("[data-desegnos-region=\"{id}\"]"))
                                    .nodes()
                                    .len()
                                    == 1)
                                && seen.insert(id)
                        }),
                        "The page contains invalid or duplicate area IDs.",
                    )?;
                }
            }
            return crate::region_scripts::validate(self, snapshot, project);
        }

        // From format 14 the AI sees compact page sources. Both sides are compared in that
        // form, so layout whitespace on either side never counts as a change.
        let mut candidate = self.clone();
        for page in &mut candidate.pages {
            page.content = crate::compact_site_content(&page.content);
        }
        let mut project = project.compact_pages();
        {
            crate::region_scripts::validate(&candidate, snapshot, &project)?;
            for page in &mut candidate.pages {
                page.content.html = crate::region_scripts::strip(&page.content.html)?;
            }
            for page in &mut project.pages {
                if let Some(content) = &mut page.content {
                    content.html = crate::region_scripts::strip(&content.html)?;
                }
            }
        }
        candidate.validate_scope_exact(snapshot, &project)
    }

    /// Framed site documents are complete HTML files; check them with the same parse and repair
    /// rules as pages. Fully scoped comparison inside a framed document is not part of this
    /// contract, so only structure, IDs and source rules are enforced here.
    pub(crate) fn validate_framed_documents(
        &self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
    ) -> Result<()> {
        snapshot.format_version()?;

        let mut framed = BTreeSet::new();
        for (page_id, region_id) in snapshot.target_ids()? {
            let page = project.page(page_id)?;
            let region = page
                .regions
                .iter()
                .find(|region| region.id == region_id)
                .ok_or_else(|| crate::Error("A requested area no longer exists.".into()))?;
            let implemented = self.regions.iter().any(|outcome| {
                outcome.region_id == region_id && outcome.status == crate::RegionStatus::Implemented
            });
            if implemented && let Some(frame) = &region.frame_target {
                framed.insert(frame.document.clone());
            }
        }
        {
            // Documents the request was given are editable even when no request is drawn on them.
            for included in &snapshot.documents {
                if let crate::DocumentTarget::SiteFile { .. } = included.document {
                    framed.insert(included.document.clone());
                }
            }
        }
        let mut seen = BTreeSet::new();
        for document in &self.documents {
            ensure(
                framed.contains(&document.document) && seen.insert(document.document.clone()),
                "The generation result contains a duplicate or unrequested framed document.",
            )?;
            crate::validate_imported_document_source(&document.content.html)
                .map_err(|error| crate::Error(error.to_string()))?;
            let doc = project.parse_document_target(&document.document, &document.content.html)?;
            crate::stable_targets::validate_ids(&doc)?;
        }
        let host_pages: BTreeSet<_> = snapshot
            .target_ids()?
            .into_iter()
            .map(|(page, _)| page)
            .collect();
        for page in &self.pages {
            if host_pages.contains(&page.page_id) {
                continue;
            }
            crate::validate_page_source(&page.content)
                .map_err(|error| crate::Error(error.to_string()))?;
            let doc = project.parse_page_html(&page.content.html)?;
            crate::stable_targets::validate_ids(&doc)?;
        }
        Ok(())
    }

    fn validate_scope_exact(
        &self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
    ) -> Result<()> {
        let targets = snapshot.target_ids()?;

        let host_pages: BTreeSet<_> = targets.iter().map(|(page, _)| *page).collect();
        for page in &self.pages {
            if !host_pages.contains(&page.page_id) {
                // A framed page document; validate_framed_documents covers its source rules.
                continue;
            }
            let original = project.page(page.page_id)?;
            {
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
            if self.implemented_whole_page(snapshot, original) {
                let implemented: Vec<_> = regions
                    .iter()
                    .copied()
                    .filter(|region| {
                        self.regions.iter().any(|outcome| {
                            outcome.region_id == region.id
                                && outcome.status == crate::RegionStatus::Implemented
                        })
                    })
                    .collect();
                validate_whole_page(project, original, &page.content, &implemented)?;
                continue;
            }
            let regions: Vec<_> = regions
                .into_iter()
                .filter(|region| !region.is_whole_page(original))
                .collect();
            if !regions.is_empty() && regions.iter().all(|region| region.mobile_only) {
                validate_phone_scope(original, &page.content)?;
                continue;
            }
            if original.content.is_some()
                && regions.iter().any(|region| !region.shape.is_rect())
                && regions
                    .iter()
                    .all(|region| region.has_targets() || !region.shape.is_rect())
            {
                validate_shaped_scope(original, &page.content, &regions, self)?;
                continue;
            }
            // Rectangular requests on an existing page have no trustworthy mapping
            // to old DOM nodes. Do not invent one from labels or model claims.
            let targeted = regions.iter().all(|region| region.has_targets());
            if regions.iter().any(|region| region.has_targets()) && original.content.is_some() {
                validate_stable_scope(original, &page.content, &regions, self)?;
                continue;
            }
            if original
                .content
                .as_ref()
                .is_some_and(|source| !source.html.trim().is_empty())
                && !targeted
            {
                {
                    let before =
                        crate::editing::parse(&original.content.as_ref().expect("source").html)?;
                    let after = crate::editing::parse(&page.content.html)?;
                    for region in &regions {
                        let selector = format!("[{MARKER}=\"{}\"]", region.id);
                        let old = before.select(&selector);
                        let new = after.select(&selector);
                        if let Some(old_id) = old
                            .nodes()
                            .first()
                            .and_then(|node| node.attr(crate::ELEMENT_ID_ATTRIBUTE))
                        {
                            ensure(
                                new.nodes().len() == 1
                                    && new.nodes()[0].attr(crate::ELEMENT_ID_ATTRIBUTE).as_deref()
                                        == Some(old_id.as_ref()),
                                format!(
                                    "Area {} must preserve editor element ID {} on its region root.",
                                    region.id, old_id
                                ),
                            )?;
                        }
                        for doc in [&before, &after] {
                            for node in doc.select(&format!("[{MARKER}=\"{}\"]", region.id)).nodes()
                            {
                                node.remove_from_parent();
                            }
                        }
                    }
                    if let Some(message) = outside_difference(before.root(), after.root()) {
                        return Err(crate::Error(message));
                    }
                }
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
                    outside_area(*node, Some(MARKER)),
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
                if let Some(message) = outside_difference(crate::editing::body(&before)?, remaining)
                {
                    return Err(crate::Error(message));
                }
            } else {
                empty_layout(remaining)?;
            }
        }
        Ok(())
    }
}

/// Full-page authority permits layout/code changes, never silent identity/content loss.
fn validate_whole_page(
    project: &Project,
    page: &crate::Page,
    content: &crate::SiteContent,
    regions: &[&crate::Region],
) -> Result<()> {
    let after = crate::editing::parse(&content.html)?;
    for region in regions.iter().filter(|region| !region.is_whole_page(page)) {
        if region.has_targets() {
            for target in region.targets() {
                ensure(
                    region.allow_target_removal || crate::editing::locate(&after, target).is_ok(),
                    "The AI removed a selected element without removal permission.",
                )?;
            }
        } else {
            ensure(
                after
                    .select(&format!("[{MARKER}=\"{}\"]", region.id))
                    .nodes()
                    .len()
                    == 1,
                "The AI did not identify each requested area. Your page is unchanged.",
            )?;
        }
    }
    let Some(source) = &page.content else {
        return Ok(());
    };
    let before = crate::editing::parse(&source.html)?;
    let removable = |node: NodeRef<'_>, id: &str| -> bool {
        regions.iter().any(|region| {
            (region.allow_target_removal
                && region.targets().iter().any(|target| {
                    crate::editing::locate(&before, target)
                        .is_ok_and(|root| crate::stable_targets::contains(root, node))
                }))
                || (region.is_whole_page(page) && {
                    // Do not guess which content a broad phrase such as "shorten the page" removes.
                    let prompt = region.prompt.to_lowercase();
                    prompt.split(['.', ';', '\n']).any(|instruction| {
                        let mut words = instruction.split_whitespace();
                        if !matches!(words.next(), Some("remove" | "delete")) {
                            return false;
                        }
                        let named = match words.next() {
                            Some("element") => words.next(),
                            named => named,
                        };
                        named.is_some_and(|named| named.trim_matches(['\"', '\'', '`', ',']) == id)
                    })
                })
        })
    };
    for node in before
        .select(&format!("[{}]", crate::ELEMENT_ID_ATTRIBUTE))
        .nodes()
    {
        let id = node.attr(crate::ELEMENT_ID_ATTRIBUTE).expect("selected ID");
        let retained = after.select(&format!("[{}=\"{}\"]", crate::ELEMENT_ID_ATTRIBUTE, id));
        ensure(
            !retained.nodes().is_empty() || removable(*node, &id),
            format!(
                "The AI removed editor element ID {id} without removal permission. Your page is unchanged."
            ),
        )?;
    }
    for decision in project.decisions.iter().filter(|d| d.protected) {
        match &decision.scope {
            crate::Scope::Region(id) => {
                let Some(region) = page.regions.iter().find(|r| r.id == *id) else {
                    continue;
                };
                let mut roots = before
                    .select(&format!("[{MARKER}=\"{id}\"]"))
                    .nodes()
                    .to_vec();
                for target in region.targets() {
                    if let Ok(root) = crate::editing::locate(&before, target) {
                        roots.push(root);
                    }
                }
                for root in roots {
                    let retained = if let Some(id) = root.attr(crate::ELEMENT_ID_ATTRIBUTE) {
                        after
                            .select(&format!("[{}=\"{}\"]", crate::ELEMENT_ID_ATTRIBUTE, id))
                            .nodes()
                            .to_vec()
                    } else {
                        after
                            .select(&format!("[{MARKER}=\"{id}\"]"))
                            .nodes()
                            .to_vec()
                    };
                    ensure(
                        retained.len() == 1 && canonical(root) == canonical(retained[0]),
                        "The AI changed protected content. Your page is unchanged.",
                    )?;
                }
            }
            crate::Scope::Page(id) if *id != page.id => continue,
            _ => {}
        }
        if !decision.value.is_empty() && source.html.contains(&decision.value) {
            ensure(
                content.html.contains(&decision.value),
                "The AI removed protected content. Your page is unchanged.",
            )?;
        }
    }
    // Decisions are structured authority. Region protection preserves that region's content;
    // page/project protected literal values already present in the source must remain present.
    // Semantic design decisions continue to be supplied to the model as accepted requirements.
    Ok(())
}

fn validate_shaped_scope(
    original: &crate::Page,
    candidate: &crate::SiteContent,
    regions: &[&crate::Region],
    current: &GenerationCandidate,
) -> Result<()> {
    let source = original.content.as_ref().expect("checked source");
    if source.css != candidate.css {
        crate::generation_recovery::validate_region_css(
            &source.css,
            &candidate.css,
            current,
            original,
        )?;
    }
    ensure(
        source.javascript == candidate.javascript,
        "The AI changed shared behavior outside the requested shapes. Your page is unchanged.",
    )?;
    let before = crate::editing::parse(&source.html)?;
    let after = crate::editing::parse(&candidate.html)?;
    let shared = |doc: &dom_query::Document| {
        doc.select("style, script, link")
            .nodes()
            .iter()
            .copied()
            .map(canonical)
            .collect::<Vec<_>>()
    };
    ensure(
        shared(&before) == shared(&after),
        "The AI changed embedded shared styling or scripts. Your page is unchanged.",
    )?;
    let handlers = |doc: &dom_query::Document| {
        doc.select("*")
            .nodes()
            .iter()
            .flat_map(|node| {
                let id = node
                    .attr(crate::ELEMENT_ID_ATTRIBUTE)
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| node.node_name().unwrap_or_default().to_string());
                node.attrs()
                    .into_iter()
                    .filter(|attr| attr.name.local.starts_with("on"))
                    .map(move |attr| {
                        (
                            (id.clone(), attr.name.local.to_string()),
                            attr.value.to_string(),
                        )
                    })
            })
            .collect::<BTreeMap<_, _>>()
    };
    let old_handlers = handlers(&before);
    ensure(
        handlers(&after)
            .iter()
            .all(|(key, value)| old_handlers.get(key) == Some(value)),
        "The AI changed shared script behavior through an event handler. Your page is unchanged.",
    )?;
    let mut editable: BTreeSet<String> = regions
        .iter()
        .flat_map(|region| region.elements_in_area())
        .map(|bound| bound.target.id.clone())
        .collect();
    let mut target_roots = Vec::new();
    let mut replacement_roots = Vec::new();
    for region in regions.iter().filter(|region| region.has_targets()) {
        for target in region.targets() {
            let root = crate::editing::locate(&before, target)?;
            let replacement = match crate::editing::locate(&after, target) {
                Ok(root) => Some(root),
                Err(_) if region.allow_target_removal => None,
                Err(error) => return Err(error),
            };
            target_roots.push(root);
            replacement_roots.push(replacement);
            editable.extend(
                before
                    .select(&format!("[{}]", crate::ELEMENT_ID_ATTRIBUTE))
                    .nodes()
                    .iter()
                    .filter(|node| crate::stable_targets::contains(root, **node))
                    .filter_map(|node| {
                        node.attr(crate::ELEMENT_ID_ATTRIBUTE)
                            .map(|id| id.to_string())
                    }),
            );
        }
    }
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
            .expect("selected attribute");
        let previous = before_ids
            .get(id.as_ref())
            .ok_or_else(|| crate::Error("The AI assigned an editor ID to a new element.".into()))?;
        let old_scope = target_roots
            .iter()
            .position(|root| crate::stable_targets::contains(*root, *previous));
        let new_scope = replacement_roots
            .iter()
            .position(|root| root.is_some_and(|root| crate::stable_targets::contains(root, *node)));
        ensure(
            old_scope == new_scope,
            "The AI moved content outside its selected element scope. Your page is unchanged.",
        )?;
    }
    let areas: BTreeSet<_> = regions
        .iter()
        .filter(|region| !region.has_targets())
        .map(|region| region.id.to_string())
        .collect();
    for id in &areas {
        let roots = after.select(&format!("[{MARKER}=\"{id}\"]"));
        ensure(
            roots.nodes().len() == 1,
            "The AI did not identify each requested shape exactly once. Your page is unchanged.",
        )?;
        ensure(
            roots.nodes()[0]
                .attr(crate::ELEMENT_ID_ATTRIBUTE)
                .is_none_or(|id| editable.contains(id.as_ref())),
            "The AI marked an element outside the requested shape as editable. Your page is unchanged.",
        )?;
        ensure(
            !replacement_roots
                .iter()
                .flatten()
                .any(|root| crate::stable_targets::contains(roots.nodes()[0], *root)),
            "The AI included a selected element inside a new area. Your page is unchanged.",
        )?;
    }
    // Remove only each in-scope element's own attributes/text. Its out-of-scope
    // descendants keep their exact content and order, even inside a large wrapper.
    fn masked(
        node: NodeRef<'_>,
        editable: &BTreeSet<String>,
        areas: &BTreeSet<String>,
        in_scope: bool,
    ) -> Vec<serde_json::Value> {
        if node.is_comment() {
            return vec![];
        }
        if node.is_text() {
            return if in_scope {
                vec![]
            } else {
                vec![canonical(node)]
            };
        }
        let id = node.attr(crate::ELEMENT_ID_ATTRIBUTE);
        let allowed = id.as_ref().is_some_and(|id| editable.contains(id.as_ref()))
            || id.is_none() && in_scope
            || node
                .attr(MARKER)
                .is_some_and(|area| areas.contains(area.as_ref()));
        let children: Vec<_> = node
            .children()
            .into_iter()
            .flat_map(|child| masked(child, editable, areas, allowed))
            .collect();
        if allowed {
            return children;
        }
        let attrs: BTreeMap<_, _> = node
            .attrs()
            .into_iter()
            .map(|attr| (format!("{:?}", attr.name), attr.value.to_string()))
            .collect();
        vec![
            serde_json::json!({"name":node.node_name().map(|name|name.to_string()),"attrs":attrs,"children":children}),
        ]
    }
    ensure(
        masked(before.root(), &editable, &BTreeSet::new(), false)
            == masked(after.root(), &editable, &areas, false),
        "The AI changed an element outside the requested shape. Your page is unchanged.",
    )
}

fn validate_stable_scope(
    original: &crate::Page,
    candidate: &crate::SiteContent,
    regions: &[&crate::Region],
    current: &GenerationCandidate,
) -> Result<()> {
    let source = original.content.as_ref().expect("checked source");
    if source.css != candidate.css {
        crate::generation_recovery::validate_region_css(
            &source.css,
            &candidate.css,
            current,
            original,
        )?;
    }
    ensure(
        source.javascript == candidate.javascript,
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
    if let Some(message) = outside_difference(before.root(), after.root()) {
        return Err(crate::Error(message));
    }
    Ok(())
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

pub(crate) fn validate_scoped_rules(css: &str, ids: &BTreeSet<(String, String)>) -> Result<()> {
    use lightningcss::rules::{CssRule, CssRuleList};
    use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
    use lightningcss::traits::ToCss;
    let sheet = StyleSheet::parse(css, ParserOptions::default())
        .map_err(|_| crate::Error("Scoped CSS could not be parsed.".into()))?;
    fn rules(list: &CssRuleList<'_>, ids: &BTreeSet<(String, String)>, depth: usize) -> Result<()> {
        ensure(
            depth <= 8 && list.0.len() <= 512,
            "Scoped CSS is too complex.",
        )?;
        for rule in &list.0 {
            match rule {
                CssRule::Style(style) => {
                    ensure(
                        style.rules.0.is_empty(),
                        "Nested CSS is outside the selected scope.",
                    )?;
                    let selectors = style
                        .selectors
                        .to_css_string(PrinterOptions::default())
                        .map_err(|_| {
                            crate::Error("Scoped CSS selectors could not be read.".into())
                        })?;
                    for selector in selectors.split(',').map(str::trim) {
                        let valid = ids.iter().any(|(attribute, id)| {
                            let quoted = format!("[{attribute}=\"{id}\"]");
                            let unquoted = format!("[{attribute}={id}]");
                            let Some(mut tail) = selector
                                .strip_prefix(&quoted)
                                .or_else(|| selector.strip_prefix(&unquoted))
                            else {
                                return false;
                            };
                            while !tail.is_empty() {
                                let Some(next) = CSS_SCOPE_STATES
                                    .iter()
                                    .find_map(|pseudo| tail.strip_prefix(pseudo))
                                else {
                                    return false;
                                };
                                tail = next;
                            }
                            true
                        });
                        ensure(
                            valid,
                            format!(
                                "CSS selector {selector} targets content outside the accepted areas."
                            ),
                        )?;
                    }
                }
                CssRule::Media(media) => rules(&media.rules, ids, depth + 1)?,
                _ => {
                    return Err(crate::Error(
                        "Global CSS rules are outside the selected scope.".into(),
                    ));
                }
            }
        }
        Ok(())
    }
    rules(&sheet.rules, ids, 0)
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

#[cfg(test)]
mod wrapper_tests {
    use super::*;

    fn fixture(
        wrapper: &str,
        tag: &str,
        text: &str,
    ) -> (Project, RequirementsSnapshot, GenerationCandidate) {
        let mut project: Project =
            serde_json::from_str(include_str!("../tests/fixtures/current-project.json")).unwrap();
        project.schema_version = crate::SCHEMA_VERSION;
        project.pages[0].content = None;
        let template = project.pages[0].regions[0].clone();
        project.pages[0].regions = (0..5)
            .map(|_| {
                let mut region = template.clone();
                region.id = Uuid::new_v4();
                region
            })
            .collect();
        project.generation_mode = crate::GenerationMode::Strict;
        let snapshot = project.prepare_requirements().unwrap();
        let roots: String = project.pages[0]
            .regions
            .iter()
            .map(|region| {
                format!(
                    "<section data-desegnos-region='{}'>Requested content</section>",
                    region.id
                )
            })
            .collect();
        let candidate = GenerationCandidate {
            job_id: snapshot.job_id,
            requirements_sha256: snapshot.sha256.clone(),
            summary: "Library".into(),
            regions: project.pages[0]
                .regions
                .iter()
                .map(|region| crate::RegionOutcome {
                    region_id: region.id,
                    status: crate::RegionStatus::Implemented,
                    explanation: "Implemented".into(),
                })
                .collect(),
            pages: vec![crate::GeneratedPage {
                page_id: project.home_page_id,
                content: crate::SiteContent {
                    html: format!("<{tag} {wrapper}>{text}{roots}</{tag}>"),
                    css: String::new(),
                    javascript: String::new(),
                },
            }],
            component_requests: vec![],
            decision_proposals: vec![],
            criticism: vec![],

            documents: vec![],
        };
        (project, snapshot, candidate)
    }

    #[test]
    fn five_roots_accept_accessible_layout_wrapper() {
        for role in ["main", "region", "presentation", "none", "group"] {
            let attrs = format!(
                "class='library-canvas' id='library' style='display:grid' data-library='books' aria-label=\"Cheli's Library\" aria-labelledby='title' role='{role}'"
            );
            let (project, snapshot, candidate) = fixture(&attrs, "main", "");
            candidate.validate(&snapshot, &project).unwrap();
            assert!(
                snapshot
                    .markdown
                    .contains("region roots may be wrapped only in plain body/div/main/section")
            );
        }
    }

    #[test]
    fn invalid_wrappers_name_the_offending_element_and_attribute() {
        for (attrs, tag, text, expected) in [
            ("", "main", "Unrequested text", "<main text>"),
            ("onclick='alert(1)'", "main", "", "<main onclick>"),
            ("tabindex='0'", "main", "", "<main tabindex>"),
            (
                "contenteditable='true'",
                "main",
                "",
                "<main contenteditable>",
            ),
            ("role='button'", "main", "", "<main role>"),
            (
                "data-desegnos-found='unrequested'",
                "main",
                "",
                "<main data-desegnos-found>",
            ),
            ("title='Unrequested'", "main", "", "<main title>"),
            ("", "nav", "", "<nav>"),
            ("", "header", "", "<header>"),
            ("", "footer", "", "<footer>"),
            ("", "article", "", "<article>"),
        ] {
            let (project, snapshot, candidate) = fixture(attrs, tag, text);
            let error = candidate.validate_scope(&snapshot, &project).unwrap_err();
            assert!(error.0.contains(expected), "{error:?}");
        }
    }

    #[test]
    fn existing_content_diagnostics_cover_changes_additions_and_removals() {
        for (before, after, expected) in [
            (
                "<main></main>",
                "<main aria-label='Changed'></main>",
                "<main aria-label>",
            ),
            ("<main title='Old'></main>", "<main></main>", "<main title>"),
            ("<main></main>", "<main><nav></nav></main>", "<nav>"),
            ("<main><nav></nav></main>", "<main></main>", "<nav>"),
            ("<main>Old</main>", "<main>Changed</main>", "<main text>"),
        ] {
            let before = crate::editing::parse(before).unwrap();
            let after = crate::editing::parse(after).unwrap();
            let message = outside_difference(before.root(), after.root()).unwrap();
            assert!(message.contains(expected), "{message}");
        }
    }

    #[test]
    fn diagnostics_truncate_untrusted_names_and_omit_values() {
        let attribute = format!("data-{}", "x".repeat(200));
        let document =
            crate::editing::parse(&format!("<main {attribute}='secret'></main>")).unwrap();
        let node = document.select("main").nodes()[0];
        let message = outside_area(node, Some(&attribute));
        assert!(message.contains('…'));
        assert!(!message.contains("secret"));
        assert!(message.chars().count() < 200);
    }
}
