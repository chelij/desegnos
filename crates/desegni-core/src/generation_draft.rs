//! Retained provider source is data, never authority to bypass source or identity checks.
use crate::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationDraft {
    pub id: Uuid,
    pub page_id: Option<Uuid>,
    pub region_ids: Vec<Uuid>,
    /// Exact JSON page/replacement payload (or exact reply when it cannot be decoded).
    pub code: String,
    pub content: Option<SiteContent>,
    pub base_sha256: Option<String>,
    #[serde(default)]
    pub base_editor_ids: Vec<String>,
    pub reason: String,
    pub dismissed: bool,
    pub applied: bool,
}

fn source_hash(content: Option<&SiteContent>) -> Option<String> {
    content.map(|c| digest(&serde_json::to_vec(c).expect("source serializes")))
}

/// Called before recovery modifies the reply. The provider's 8 MiB envelope bounds each round.
pub fn retain_generation_drafts(
    text: &str,
    snapshot: &RequirementsSnapshot,
    project: &Project,
    recovered: Option<&GenerationCandidate>,
    events: &[ValidationEvent],
) -> Vec<GenerationDraft> {
    if text.len() > MAX_GENERATION_BYTES {
        return vec![];
    }
    let value = serde_json::from_str::<serde_json::Value>(text).ok();
    let targets = snapshot.target_ids().unwrap_or_default();
    let mut drafts = vec![];
    let mut reconstructed_bytes = 0;
    // An oversized/malformed page envelope remains one bounded, copy-only reply.
    if value
        .as_ref()
        .and_then(|v| v["pages"].as_array())
        .is_some_and(|pages| pages.len() > MAX_PAGES)
    {
        return vec![GenerationDraft {
            id: Uuid::new_v4(),
            page_id: None,
            region_ids: vec![],
            code: text.into(),
            content: None,
            base_sha256: None,
            base_editor_ids: vec![],
            reason: "The reply contains too many pages.".into(),
            dismissed: false,
            applied: false,
        }];
    }
    if let Some(pages) = value.as_ref().and_then(|v| v["pages"].as_array()) {
        for page in pages {
            let id = page["pageId"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok());
            let content = id.and_then(|id| draft_content(page, project.page(id).ok()?));
            let accepted = recovered.and_then(|c| c.pages.iter().find(|p| Some(p.page_id) == id));
            // Formatting alone is not lost work. Compare with the same normalisation as recovery.
            let unchanged = content.as_ref().zip(accepted).is_some_and(|(raw, kept)| {
                compact_site_content(raw) == compact_site_content(&kept.content)
            });
            if unchanged {
                continue;
            }
            let region_ids: Vec<_> = targets
                .iter()
                .filter(|(p, _)| Some(*p) == id)
                .map(|(_, r)| *r)
                .collect();
            let reason = events
                .iter()
                .find(|e| {
                    e.disposition != "normalised"
                        && (e.region_id.is_none()
                            || e.region_id.is_some_and(|r| region_ids.contains(&r)))
                })
                .map(|e| e.message.clone())
                .unwrap_or_default();
            let content = content.filter(|content| {
                reconstructed_bytes += content.byte_len();
                reconstructed_bytes <= 4 * 1024 * 1024
            });
            let base_editor_ids = content
                .as_ref()
                .and_then(|c| crate::editing::parse(&c.html).ok())
                .map(|doc| {
                    let old = id
                        .and_then(|id| project.page(id).ok())
                        .and_then(|p| p.content.as_ref())
                        .and_then(|c| crate::editing::parse(&c.html).ok());
                    doc.select(&format!("[{ELEMENT_ID_ATTRIBUTE}]"))
                        .nodes()
                        .iter()
                        .filter_map(|node| {
                            let identity = node.attr(ELEMENT_ID_ATTRIBUTE)?;
                            old.as_ref()
                                .is_some_and(|old| {
                                    old.select(&format!("[{ELEMENT_ID_ATTRIBUTE}=\"{identity}\"]"))
                                        .nodes()
                                        .len()
                                        == 1
                                })
                                .then(|| identity.to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();
            drafts.push(GenerationDraft {
                id: Uuid::new_v4(),
                page_id: id,
                region_ids,
                code: serde_json::to_string(page).expect("JSON serializes"),
                content,
                base_sha256: id
                    .and_then(|id| project.page(id).ok())
                    .and_then(|p| source_hash(p.content.as_ref())),
                base_editor_ids,
                reason: if reason.is_empty() {
                    "This page did not pass generation validation.".into()
                } else {
                    reason.chars().take(4096).collect()
                },
                dismissed: false,
                applied: false,
            });
        }
    }
    if drafts.is_empty() && recovered.is_none() {
        drafts.push(GenerationDraft {
            id: Uuid::new_v4(),
            page_id: None,
            region_ids: targets.iter().map(|(_, r)| *r).collect(),
            code: text.into(),
            content: None,
            base_sha256: None,
            base_editor_ids: vec![],
            reason: events
                .first()
                .map_or("The reply could not be decoded.".into(), |e| {
                    e.message.clone()
                }),
            dismissed: false,
            applied: false,
        });
    }
    drafts
}

// Keep original fragments in `code`; reconstruction is an optional bounded convenience.
fn draft_content(value: &serde_json::Value, page: &Page) -> Option<SiteContent> {
    let mut content = page.content.clone().unwrap_or(SiteContent {
        html: String::new(),
        css: String::new(),
        javascript: String::new(),
    });
    if value["content"].is_object() {
        for (name, target) in [
            ("html", &mut content.html),
            ("css", &mut content.css),
            ("javascript", &mut content.javascript),
        ] {
            if let Some(text) = value["content"][name].as_str().filter(|s| !s.is_empty()) {
                *target = text.into();
            }
        }
    } else {
        let doc = crate::editing::parse(&content.html).ok()?;
        for element in value["elements"].as_array()? {
            let id = element["id"].as_str()?;
            if !crate::stable_targets::valid_id(id) {
                return None;
            }
            let nodes = doc.select(&format!("[{ELEMENT_ID_ATTRIBUTE}=\"{id}\"]"));
            if nodes.nodes().len() != 1 {
                return None;
            }
            nodes.nodes()[0].replace_with_html(element["html"].as_str()?);
        }
        content.html = crate::editing::body(&doc).ok()?.inner_html().to_string();
        if let Some(css) = value["css"].as_str().filter(|s| !s.is_empty()) {
            content.css.push_str(css);
        }
    }
    Some(content)
}

impl GenerationDraft {
    /// Native callers use this immediately before Preview/Apply, including past jobs.
    pub fn check_source(&self, project: &Project) -> Result<()> {
        ensure(
            !self.applied && !self.dismissed,
            "This draft is no longer available.",
        )?;
        let page = project.page(self.page_id.ok_or_else(|| {
            Error("This reply cannot be reconstructed. Copy code to keep it.".into())
        })?)?;
        ensure(
            !self.region_ids.is_empty()
                && self
                    .region_ids
                    .iter()
                    .all(|id| page.regions.iter().any(|r| r.id == *id)),
            "The draft's requested areas no longer exist. Copy code to keep it.",
        )?;
        let content = self.content.as_ref().ok_or_else(|| {
            Error("This reply cannot be reconstructed. Copy code to keep it.".into())
        })?;
        validate_page_source(content).map_err(|e| Error(e.to_string()))?;
        crate::source::validate_draft_html(&content.html)?;
        let after = crate::editing::parse(&content.html)?;
        crate::stable_targets::validate_ids(&after)?;
        // Provider-assigned identities, duplicate region roots, and another page's region IDs
        // never become authoritative through an override.
        for node in after.select(&format!("[{ELEMENT_ID_ATTRIBUTE}]")).nodes() {
            let id = node.attr(ELEMENT_ID_ATTRIBUTE).expect("selected");
            ensure(
                self.base_editor_ids
                    .iter()
                    .any(|known| known == id.as_ref()),
                "The draft invents or changes an editor ID. Copy code to keep it.",
            )?;
        }
        let mut seen = std::collections::BTreeSet::new();
        for node in after.select("[data-desegnos-region]").nodes() {
            let id = node
                .attr("data-desegnos-region")
                .and_then(|s| Uuid::parse_str(&s).ok());
            ensure(
                id.is_some_and(|id| page.regions.iter().any(|r| r.id == id) && seen.insert(id)),
                "The draft contains invalid or duplicate area IDs. Copy code to keep it.",
            )?;
        }
        if content.javascript != page.content.as_ref().map_or("", |c| c.javascript.as_str()) {
            crate::source::validate_draft_javascript(&content.javascript)?;
        }
        for script in after.select("script").nodes() {
            if !script
                .attr("type")
                .is_some_and(|kind| kind.contains("json"))
            {
                crate::source::validate_draft_javascript(script.text().as_ref())?;
            }
        }
        for node in after.select("*").nodes() {
            for attr in node.attrs() {
                if attr.name.local.to_ascii_lowercase().starts_with("on") {
                    crate::source::validate_draft_javascript(&format!(
                        "(()=>{{ {} }});",
                        attr.value
                    ))?;
                }
            }
        }
        Ok(())
    }

    pub fn check(&self, project: &Project) -> Result<()> {
        self.check_source(project)?;
        let page = project.page(self.page_id.expect("checked"))?;
        let content = self.content.as_ref().expect("checked");
        ensure(
            source_hash(page.content.as_ref()) == self.base_sha256,
            "The page changed and this draft no longer fits. Preview or copy its code.",
        )?;
        // Also run the ordinary source transaction's target tracking and project validation.
        project
            .edit_page_source(
                project.revision,
                page.id,
                &PageSourceEdit {
                    html: Some(content.html.clone()),
                    css: Some(content.css.clone()),
                    javascript: Some(content.javascript.clone()),
                },
            )
            .map_err(|e| Error(e.to_string()))?;
        Ok(())
    }

    pub fn confirmation(&self, project: &Project) -> String {
        let protected = project
            .decisions
            .iter()
            .filter(|d| d.protected)
            .map(|d| d.topic.as_str())
            .collect::<Vec<_>>();
        let rule = if self.reason.contains("CSS") || self.reason.contains("stylesheet") {
            "Styles affect content outside the requested area."
        } else if self.reason.contains("JavaScript") || self.reason.contains("behavior") {
            "Behavior changes affect content outside the requested area."
        } else {
            "This output breaks the generation area rules shown above."
        };
        let mut text = format!("Replace this page with the retained draft? {rule}");
        if !protected.is_empty() {
            text.push_str(&format!(" Protected items: {}.", protected.join(", ")));
        }
        text
    }

    /// An automatic apply changes the page to the safe salvage, not to an unrelated edit.
    pub fn rebase_after_apply(&mut self, project: &Project) {
        self.base_sha256 = self
            .page_id
            .and_then(|id| project.page(id).ok())
            .and_then(|p| source_hash(p.content.as_ref()));
    }
}

impl Project {
    pub fn apply_generation_draft(
        &self,
        snapshot: &RequirementsSnapshot,
        draft: &GenerationDraft,
    ) -> Result<Self> {
        ensure(
            snapshot.project_id == self.id
                && snapshot.sha256 == digest(snapshot.markdown.as_bytes())
                && !draft.id.is_nil(),
            "The draft's frozen project identity is invalid.",
        )?;
        if self.applied_job_ids.contains(&draft.id) {
            return Ok(self.clone());
        }
        draft.check(self)?;
        let page_id = draft.page_id.expect("checked");
        let content = draft.content.as_ref().expect("checked");
        let mut next = self
            .edit_page_source(
                self.revision,
                page_id,
                &PageSourceEdit {
                    html: Some(content.html.clone()),
                    css: Some(content.css.clone()),
                    javascript: Some(content.javascript.clone()),
                },
            )
            .map_err(|e| Error(e.to_string()))?;
        // Even an identical source consumes the draft once and creates one history transaction.
        if next.revision == self.revision {
            next.revision = self
                .revision
                .checked_add(1)
                .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        }
        let candidate = GenerationCandidate {
            job_id: draft.id,
            requirements_sha256: snapshot.sha256.clone(),
            summary: "Applied retained AI draft".into(),
            regions: draft
                .region_ids
                .iter()
                .map(|id| RegionOutcome {
                    region_id: *id,
                    status: RegionStatus::Implemented,
                    explanation: draft.reason.chars().take(4096).collect(),
                })
                .collect(),
            pages: vec![GeneratedPage {
                page_id,
                content: content.clone(),
            }],
            component_requests: vec![],
            decision_proposals: vec![],
            criticism: vec![],
        };
        let mut dossier =
            GenerationDossier::from_candidate(snapshot, &candidate, next.revision, 1)?;
        dossier.application_status = GenerationApplicationStatus::Applied;
        dossier.applied_at_revision = Some(next.revision);
        next.latest_generation = Some(dossier);
        next.applied_job_ids.push(draft.id);
        next.validate()?;
        Ok(next)
    }
}
