//! Element fixes: when every request targets existing elements, only those elements and the
//! page stylesheet (reference only) are sent, and the AI returns replacements for just those elements. Desegnos
//! rebuilds complete pages from the reply, so every normal check and Undo applies unchanged.
use crate::{
    ComponentRequest, DecisionProposal, DesignCriticism, Error, GeneratedPage, GenerationCandidate,
    Page, Project, Region, RegionOutcome, RequirementsSnapshot, Result, SiteContent, editing,
    ensure,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ElementFixReply {
    job_id: Uuid,
    requirements_sha256: String,
    summary: String,
    regions: Vec<RegionOutcome>,
    pages: Vec<ElementPageFix>,
    needs: Vec<String>,
    component_requests: Vec<ComponentRequest>,
    decision_proposals: Vec<DecisionProposal>,
    criticism: Vec<DesignCriticism>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ElementPageFix {
    page_id: Uuid,
    elements: Vec<ElementReplacement>,
    #[serde(default)]
    css: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ElementReplacement {
    id: String,
    html: String,
}

fn targets(region: &Region) -> impl Iterator<Item = &crate::ElementTarget> {
    region.elements.iter()
}

/// Element fixes apply when every request is an ordinary fix of existing elements with stable IDs
/// on a page that already has a site. New areas, phone fixes and attachments need the full page.
pub(crate) fn eligible(requests: &[(&Page, &Region)]) -> bool {
    !requests.is_empty()
        && requests.iter().all(|(page, region)| {
            page.content.is_some()
                && !region.mobile_only
                && region.attachments.is_empty()
                && targets(region).next().is_some()
                && targets(region).all(|target| !target.id.is_empty())
        })
}

/// One short line per element on the page (ID, tag.classes, own text), so the AI can ask for
/// the ones it needs.
const MAX_OUTLINE_ENTRIES: usize = 400;
const MAX_CONTEXT_ELEMENTS: usize = 32;
const CONTEXT_HEADING: &str = "\n## Requested context\n\n";

fn outline(doc: &dom_query::Document) -> Vec<String> {
    doc.select(&format!("body [{}]", crate::ELEMENT_ID_ATTRIBUTE))
        .nodes()
        .iter()
        .take(MAX_OUTLINE_ENTRIES)
        .map(|node| {
            let own: String = node
                .children()
                .into_iter()
                .filter(|child| child.is_text())
                .map(|child| child.text().to_string())
                .collect::<Vec<_>>()
                .join(" ");
            let text: String = own
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(40)
                .collect();
            let mut line = format!(
                "{} {}",
                node.attr(crate::ELEMENT_ID_ATTRIBUTE).unwrap_or_default(),
                node.node_name().unwrap_or_default()
            );
            if let Some(class) = node.attr("class") {
                for name in class.split_whitespace() {
                    line.push('.');
                    line.push_str(name);
                }
            }
            if !text.is_empty() {
                line.push_str(&format!(" {text:?}"));
            }
            line
        })
        .collect()
}

impl Project {
    fn referenced(&self, html: String) -> String {
        self.generation_source_with_asset_references(&SiteContent {
            html,
            css: String::new(),
            javascript: String::new(),
        })
        .html
    }

    /// The selected elements (outer HTML with editor IDs), the page stylesheet and a page outline.
    /// Requirements format 18 adds bounded read-only script context to this envelope.
    pub(crate) fn element_fix_source(
        &self,
        page: &Page,
        regions: &[&Region],
    ) -> Result<serde_json::Value> {
        let content = page
            .content
            .as_ref()
            .ok_or_else(|| Error("This page has no site to fix.".into()))?;
        let doc = editing::parse(&content.html)?;
        let mut seen = BTreeSet::new();
        let mut elements = vec![];
        for target in regions.iter().flat_map(|region| targets(region)) {
            if !seen.insert(target.id.clone()) {
                continue;
            }
            let node = editing::locate(&doc, target)?;
            elements.push(serde_json::json!({"id":target.id,"tag":target.tag,"html":self.referenced(node.html().to_string())}));
        }
        let css = self
            .generation_source_with_asset_references(&SiteContent {
                html: String::new(),
                css: content.css.clone(),
                javascript: String::new(),
            })
            .css;
        Ok(
            serde_json::json!({"pageId": page.id, "elements": elements, "css": css, "outline": outline(&doc)}),
        )
    }
}

impl RequirementsSnapshot {
    pub fn is_element_fix(&self) -> bool {
        self.format_version().is_ok() && self.markdown.contains("\n## Selected elements\n")
    }

    /// True for the one follow-up that carries elements the AI asked for.
    pub fn has_requested_context(&self) -> bool {
        self.markdown.contains(CONTEXT_HEADING)
    }

    /// When an element-fix reply asks for other elements, the follow-up requirements: the same
    /// frozen file plus those elements' HTML. None when the reply is a final result.
    pub fn context_follow_up(
        &self,
        text: &str,
        project: &Project,
    ) -> Result<Option<RequirementsSnapshot>> {
        if !self.is_element_fix() || self.has_requested_context() {
            return Ok(None);
        }
        let compacted = project.compact_pages();
        let project = &compacted;
        let Ok(reply) = serde_json::from_str::<ElementFixReply>(text) else {
            return Ok(None);
        };
        if reply.needs.is_empty() {
            return Ok(None);
        }
        ensure(
            reply.needs.len() <= MAX_CONTEXT_ELEMENTS,
            "The AI asked for too many other elements.",
        )?;
        ensure(
            reply.pages.is_empty()
                && reply.job_id == self.job_id
                && reply.requirements_sha256 == self.sha256,
            "The AI context request did not match this job.",
        )?;
        let pages: BTreeSet<Uuid> = self
            .target_ids()?
            .into_iter()
            .map(|(page, _)| page)
            .collect();
        let mut found = vec![];
        for id in &reply.needs {
            let element = pages.iter().filter(|_| crate::stable_targets::valid_id(id)).find_map(|page_id| {
                let content = project.page(*page_id).ok()?.content.as_ref()?;
                let doc = editing::parse(&content.html).ok()?;
                let nodes = doc.select(&format!("[{}=\"{}\"]", crate::ELEMENT_ID_ATTRIBUTE, id));
                (nodes.nodes().len() == 1).then(|| serde_json::json!({"pageId":page_id,"id":id,"html":project.referenced(nodes.nodes()[0].html().to_string())}))
            });
            found.push(element.unwrap_or_else(|| serde_json::json!({"id":id,"missing":true})));
        }
        let mut markdown = self.markdown.clone();
        markdown.push_str(CONTEXT_HEADING);
        markdown.push_str("You asked for these elements. They are reference only: change only the selected elements. Return the final result now, with needs empty.\n\n");
        markdown.push_str(&crate::requirements::fenced_json(&found)?);
        ensure(
            markdown.len() <= crate::MAX_REQUIREMENTS_BYTES,
            "The follow-up context exceeds its size limit.",
        )?;
        let mut follow_up = self.clone();
        follow_up.sha256 = crate::digest(markdown.as_bytes());
        follow_up.markdown = markdown;
        Ok(Some(follow_up))
    }

    /// Ties a follow-up reply back to the frozen requirements it extends.
    pub fn adopt_follow_up_reply(
        &self,
        follow_up: &RequirementsSnapshot,
        text: &str,
    ) -> Result<String> {
        let mut value: serde_json::Value = serde_json::from_str(text)
            .map_err(|_| Error("The AI reply did not match the element fix format.".into()))?;
        ensure(
            value["requirementsSha256"] == follow_up.sha256.as_str()
                && follow_up.markdown.starts_with(&self.markdown),
            "The AI reply did not match these requirements.",
        )?;
        value["requirementsSha256"] = self.sha256.clone().into();
        serde_json::to_string(&value).map_err(|e| Error(e.to_string()))
    }
}

impl GenerationCandidate {
    /// Reads a provider reply for these requirements. Element-fix replies are expanded into
    /// complete pages from the project they were prepared against.
    pub fn from_reply(
        text: &str,
        snapshot: &RequirementsSnapshot,
        project: &Project,
    ) -> Result<Self> {
        let mut candidate = Self::read_reply(text, snapshot, project)?;
        {
            candidate.note_unavailable_components();
        }
        Ok(candidate)
    }

    /// A finished area may still lack a service it needs. Say so in the suggestions the user sees.
    fn note_unavailable_components(&mut self) {
        let mut notes = Vec::new();
        for request in &self.component_requests {
            if request.resolution != crate::ComponentResolution::Unavailable {
                continue;
            }
            let message: String = format!(
                "{} isn't connected yet: {}",
                request.capability.trim(),
                request.reason.trim()
            )
            .chars()
            .take(crate::generation::MAX_EXPLANATION_CHARS)
            .collect();
            if !self.criticism.iter().any(|item| item.message == message) {
                notes.push(DesignCriticism {
                    region_id: Some(request.region_id),
                    message,
                    suggestion: "A clearly marked placeholder was left in its place. Connect the service later, or ask for something that works without it.".into(),
                });
            }
        }
        for note in notes {
            if self.criticism.len() < crate::MAX_DESIGN_CRITICISMS {
                self.criticism.push(note);
            }
        }
    }

    fn read_reply(text: &str, snapshot: &RequirementsSnapshot, project: &Project) -> Result<Self> {
        if !snapshot.is_element_fix() {
            return Self::from_json(text);
        }
        // The AI saw compact sources (format 14); its replacements go into that same form.
        let compacted = project.compact_pages();
        let project = &compacted;
        ensure(
            text.len() <= crate::MAX_GENERATION_BYTES,
            "The AI reply is too large.",
        )?;
        let reply: ElementFixReply = match serde_json::from_str(text) {
            Ok(reply) => reply,
            // Image preparation hands on an already expanded result; it is validated the same way.
            Err(error) => {
                return Self::from_json(text).map_err(|_| {
                    let detail: String = error.to_string().chars().take(160).collect();
                    Error(format!(
                        "The AI reply did not match the element fix format ({detail})."
                    ))
                });
            }
        };
        ensure(
            reply.needs.is_empty(),
            "The AI still needed more of the page. Your page is unchanged. Select the related elements too and try again.",
        )?;
        let allowed: BTreeSet<(Uuid, String)> = snapshot
            .target_ids()?
            .into_iter()
            .filter_map(|(page_id, region_id)| {
                project
                    .pages
                    .iter()
                    .find(|page| page.id == page_id)
                    .and_then(|page| page.regions.iter().find(|region| region.id == region_id))
                    .map(|region| (page_id, region))
            })
            .flat_map(|(page_id, region)| {
                targets(region).map(move |target| (page_id, target.id.clone()))
            })
            .collect();
        let mut pages = Vec::with_capacity(reply.pages.len());
        for fix in reply.pages {
            let page = project.page(fix.page_id)?;
            let content = page
                .content
                .as_ref()
                .ok_or_else(|| Error("The AI changed a page that has no site.".into()))?;
            let doc = editing::parse(&content.html)?;
            let mut done = BTreeSet::new();
            for element in &fix.elements {
                ensure(
                    allowed.contains(&(fix.page_id, element.id.clone()))
                        && done.insert(element.id.clone()),
                    "The AI changed an element that was not selected. Your page is unchanged.",
                )?;
                editing::check_value(&element.html, crate::MAX_SITE_SOURCE_BYTES)?;
                let nodes = doc.select(&format!(
                    "[{}=\"{}\"]",
                    crate::ELEMENT_ID_ATTRIBUTE,
                    element.id
                ));
                ensure(nodes.nodes().len() == 1, "This element was removed.")?;
                nodes.nodes()[0].replace_with_html(element.html.as_str());
            }
            editing::bounded(&doc)?;
            pages.push(GeneratedPage {
                page_id: fix.page_id,
                content: SiteContent {
                    // Page sources are body fragments; serializing the whole document here would
                    // differ from the reply that was retained for review and from every other
                    // generation path, which keeps the fragment form.
                    html: editing::body(&doc)?.inner_html().to_string(),
                    css: if fix.css.is_empty() {
                        content.css.clone()
                    } else {
                        format!("{}\n{}", content.css, fix.css)
                    },
                    javascript: content.javascript.clone(),
                },
            });
        }
        Ok(GenerationCandidate {
            job_id: reply.job_id,
            requirements_sha256: reply.requirements_sha256,
            summary: reply.summary,
            regions: reply.regions,
            pages,
            component_requests: reply.component_requests,
            decision_proposals: reply.decision_proposals,
            criticism: reply.criticism,

            documents: vec![],
        })
    }
}

/// The result schema matching these requirements.
pub fn result_schema(snapshot: &RequirementsSnapshot) -> serde_json::Value {
    if snapshot.is_element_fix() {
        let mut schema = element_fix_schema();
        {
            let page = &mut schema["properties"]["pages"]["items"];
            page["required"].as_array_mut().unwrap().push("css".into());
            page["properties"]["css"] = serde_json::json!({"type":"string","description":"Additional CSS only. Every selector must be the selected element's own [data-desegnos-id=\"ID\"], optionally with hover/focus/active/disabled/checked or before/after. No descendant or sibling selectors. Media queries are allowed; all their rules obey the same scope. Empty when no new rules are needed."});
        }
        if snapshot.has_requested_context() {
            schema["properties"]["needs"]["maxItems"] = 0.into();
        }
        {
            independent_result_schema(&mut schema);
        }
        schema
    } else {
        let mut schema = crate::candidate_schema();
        {
            independent_result_schema(&mut schema);
        }
        schema
    }
}

fn independent_result_schema(schema: &mut serde_json::Value) {
    schema["properties"]["regions"]["description"] = serde_json::json!(
        "Deliver every implementable region independently. Conflicts must not withhold ready regions."
    );
    schema["properties"]["pages"]["description"] = serde_json::json!(
        "Include exactly the pages with implemented regions, preserving conflicting regions unchanged or as the specified local placeholder. Omit pages containing only conflicts."
    );
}

pub fn element_fix_schema() -> serde_json::Value {
    use serde_json::json;
    let mut schema = crate::candidate_schema();
    let source = json!({"type":"string","description":format!("At most {} UTF-8 bytes, enforced by Desegnos before acceptance.", crate::MAX_SITE_SOURCE_BYTES)});
    let element = json!({"type":"object","additionalProperties":false,"required":["id","html"],"properties":{
        "id":{"type":"string","description":"The data-desegnos-id of a selected element."},
        "html":source
    }});
    schema["properties"]["pages"]["items"] = json!({"type":"object","additionalProperties":false,"required":["pageId","elements"],"properties":{
        "pageId":schema["properties"]["pages"]["items"]["properties"]["pageId"].clone(),
        "elements":{"type":"array","maxItems":crate::MAX_GENERATION_REGIONS * 8,"items":element}
    }});
    schema["properties"]["needs"] = json!({"type":"array","maxItems":MAX_CONTEXT_ELEMENTS,"items":{"type":"string"},
        "description":"IDs from the page outline whose HTML you need before you can finish. Leave pages empty when asking. Empty for a final result."});
    schema["required"]
        .as_array_mut()
        .expect("required is a list")
        .push("needs".into());
    schema
}
