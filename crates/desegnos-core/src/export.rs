//! Export: what ships, what the AI is asked for, and how its reply is checked.
//!
//! Slides and images render directly from the resolved element tree. Websites and app screens
//! travel as one frozen request per export: the element tree for every size, pointers with
//! comments, referenced material, linked definitions and the brief. The reply is strict JSON of
//! plain files, validated here before anything is written. Pointer fingerprints are SHA-256
//! digests over each pointer's marked fragments, so later code changes can be spotted.
use crate::{
    Element, Error, ExportRecord, ExportedPage, ExportedPointer, GenerationMode, Page,
    Pointer, PointerSource, PointerTarget, Project, Result, digest, element_subtree, ensure,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Largest single exported file.
pub const MAX_EXPORT_FILE_BYTES: usize = 4 * 1024 * 1024;
/// Largest complete export, including pictures.
pub const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;
/// Most files one export may write.
pub const MAX_EXPORT_FILES: usize = 2_000;
/// The schema version of the exported manifest.
pub const EXPORT_MANIFEST_VERSION: u32 = 1;
pub const EXPORT_MANIFEST_FILE: &str = "desegnos-export.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsiteExport {
    pub files: BTreeMap<String, Vec<u8>>,
    pub page_count: usize,
    pub warnings: Vec<String>,
}

/// Compile the design into ordinary website files. Element designs carry no HTML; the AI export
/// job builds HTML, CSS and JavaScript from the canvas and the direct renderer writes slides
/// and images, so this older seam reports itself instead of producing partial files.
pub fn compile_website(_project: &Project) -> Result<WebsiteExport> {
    Err(Error("Export is being rebuilt.".into()))
}

/// One boundary the export ships, and the files it will write.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportPagePlan {
    pub page_id: Uuid,
    pub name: String,
    pub path: String,
    pub output: crate::OutputType,
    /// The HTML file, for AI pages: `index.html` at the root, `<slug>.html` otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    pub size_ids: Vec<Uuid>,
}

/// One pointer the reply must answer for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportPointerPlan {
    pub id: Uuid,
    pub comment: String,
}

/// A picture that travels beside the request, never inside it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportAssetPlan {
    pub picture_id: Uuid,
    pub path: String,
    pub name: String,
}

/// What an export job asks for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExportPlanKind {
    /// Build websites and app screens.
    Code,
    /// Rewrite one pointer's comment to describe the marked code that already exists.
    Comment { pointer_id: Uuid },
}

/// A frozen export job. The markdown and hashes are fixed when it is prepared, so a retry sends
/// exactly the same task; a changed design or destination makes it stale instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportPlan {
    pub job_id: Uuid,
    pub project_id: Uuid,
    pub base_revision: u64,
    pub kind: ExportPlanKind,
    pub pages: Vec<ExportPagePlan>,
    pub pointers: Vec<ExportPointerPlan>,
    pub assets: Vec<ExportAssetPlan>,
    pub markdown: String,
    pub sha256: String,
    pub schema: serde_json::Value,
    pub request_count: usize,
    pub page_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// One page's files as the AI returned them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportReplyPage {
    pub page_id: Uuid,
    pub path: String,
    pub html: String,
    pub css: String,
    pub js: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportReplyShared {
    #[serde(default)]
    pub css: String,
    #[serde(default)]
    pub js: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportReplyPointer {
    pub pointer_id: Uuid,
    pub implemented: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportReply {
    pub job_id: Uuid,
    pub requirements_sha256: String,
    pub summary: String,
    pub pages: Vec<ExportReplyPage>,
    #[serde(default)]
    pub shared: ExportReplyShared,
    pub pointers: Vec<ExportReplyPointer>,
}

/// The rendered and written result of an export.
#[derive(Debug, Clone, Default)]
pub struct ExportBundle {
    pub files: BTreeMap<String, Vec<u8>>,
    pub pages: Vec<ExportedPage>,
    pub pointers: BTreeMap<Uuid, ExportedPointer>,
    pub warnings: Vec<String>,
}

impl ExportBundle {
    /// Every exported page, in canvas order.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

/// Included boundaries in shipping order. Slides follow canvas order, or a chain of go-to
/// pointers between slide boundaries.
pub fn included_pages(project: &Project) -> Vec<&Page> {
    let mut pages: Vec<&Page> = project.pages.iter().filter(|page| page.included).collect();
    pages.sort_by(|first, second| {
        let one = first.base_size();
        let two = second.base_size();
        one.y
            .partial_cmp(&two.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(one.x.partial_cmp(&two.x).unwrap_or(std::cmp::Ordering::Equal))
            .then(first.name.cmp(&second.name))
    });
    pages
}

/// Slide order: canvas order, or following `go_to` pointers between slide boundaries.
pub fn slide_order(project: &Project) -> Vec<&Page> {
    let slides: Vec<&Page> = included_pages(project)
        .into_iter()
        .filter(|page| page.output == crate::OutputType::Slide)
        .collect();
    if slides.len() < 2 {
        return slides;
    }
    let ids: BTreeSet<Uuid> = slides.iter().map(|page| page.id).collect();
    let mut next: BTreeMap<Uuid, Uuid> = BTreeMap::new();
    let mut targets: BTreeSet<Uuid> = BTreeSet::new();
    for pointer in &project.pointers {
        if !matches!(pointer.play, Some(crate::PlayAction::GoTo { .. })) {
            continue;
        }
        let Some(crate::PlayAction::GoTo { boundary }) = pointer.play else {
            continue;
        };
        if !ids.contains(&boundary) {
            continue;
        }
        let source = match &pointer.from {
            PointerSource::Boundary { page_id, .. } => Some(*page_id),
            PointerSource::Element { page_id, .. } => Some(*page_id),
        };
        if let Some(source) = source
            && ids.contains(&source)
            && source != boundary
        {
            next.entry(source).or_insert(boundary);
            targets.insert(boundary);
        }
    }
    let mut order: Vec<&Page> = Vec::new();
    let mut seen: BTreeSet<Uuid> = BTreeSet::new();
    let starts: Vec<&Page> = slides
        .iter()
        .copied()
        .filter(|page| !targets.contains(&page.id))
        .collect();
    let starts = if starts.is_empty() { slides.clone() } else { starts };
    for start in starts {
        let mut current = Some(start);
        while let Some(page) = current {
            if !seen.insert(page.id) {
                break;
            }
            order.push(page);
            current = next
                .get(&page.id)
                .and_then(|id| slides.iter().copied().find(|page| page.id == *id));
        }
    }
    for page in slides {
        if seen.insert(page.id) {
            order.push(page);
        }
    }
    order
}

/// The file an AI page writes: `index.html` at the root, otherwise `<slug>.html` beside it.
pub fn page_file(page: &Page) -> String {
    if page.path == "/" {
        "index.html".into()
    } else {
        format!("{}.html", crate::file_stem(&page.path))
    }
}

/// Keep a file name unique inside one export folder by numbering later matches.
fn unique_file(used: &mut BTreeSet<String>, candidate: String) -> String {
    let candidate = candidate.to_lowercase();
    if used.insert(candidate.clone()) {
        return candidate;
    }
    let (stem, extension) = candidate
        .rsplit_once('.')
        .map(|(stem, extension)| (stem.to_owned(), format!(".{extension}")))
        .unwrap_or_else(|| (candidate.clone(), String::new()));
    for index in 2..10_000 {
        let next = format!("{stem}-{index}{extension}");
        if used.insert(next.clone()) {
            return next;
        }
    }
    candidate
}

/// Prepare an export of the AI-built boundaries among `page_ids`, or of every included
/// boundary when `page_ids` is empty. Direct outputs are included in the plan too; the caller
/// adds their rendered files when it writes.
pub fn plan_export(
    project: &Project,
    page_ids: &[Uuid],
    job_id: Uuid,
) -> Result<ExportPlan> {
    ensure(
        !job_id.is_nil(),
        "An export job needs a nonempty identifier.",
    )?;
    let chosen: Vec<&Page> = if page_ids.is_empty() {
        included_pages(project)
    } else {
        let mut chosen = Vec::new();
        for id in page_ids {
            let page = project.pages.iter().find(|page| page.id == *id).ok_or_else(|| {
                Error("A boundary in this export no longer exists.".into())
            })?;
            ensure(
                page.included,
                "Only included boundaries can be exported. Include this boundary first.",
            )?;
            chosen.push(page);
        }
        chosen
    };
    ensure(
        !chosen.is_empty(),
        "Include at least one boundary before exporting.",
    )?;
    let ai_pages: Vec<&Page> = chosen
        .iter()
        .copied()
        .filter(|page| page.output.needs_ai())
        .collect();
    // Direct-only exports need no job: a plan with no AI page is still valid for writing files.
    let mut pointers: Vec<ExportPointerPlan> = Vec::new();
    let mut assets: BTreeMap<Uuid, ExportAssetPlan> = BTreeMap::new();
    let mut pages_json: Vec<serde_json::Value> = Vec::new();
    let mut references_json: Vec<serde_json::Value> = Vec::new();
    let mut allowed_urls: BTreeSet<String> = BTreeSet::new();
    let ids: BTreeSet<Uuid> = ai_pages.iter().map(|page| page.id).collect();
    let mut linked_ids: BTreeSet<Uuid> = BTreeSet::new();
    for page in &ai_pages {
        let mut sizes = Vec::new();
        for size in &page.sizes {
            let elements = crate::resolve_for_size(project, page, size.id);
            collect_links(&elements, &mut linked_ids);
            collect_pictures(project, &elements, page, &mut assets);
            collect_attachments(&elements, &mut references_json, &mut allowed_urls);
            sizes.push(serde_json::json!({
                "sizeId": size.id,
                "label": size.label,
                "width": size.width,
                "height": size.height,
                "kind": size.kind,
                "elements": elements.iter().map(element_subtree).collect::<Vec<_>>(),
            }));
        }
        let touching: Vec<&Pointer> = project
            .pointers
            .iter()
            .filter(|pointer| pointer_touches(pointer, page.id))
            .collect();
        let mut pointer_json = Vec::new();
        for pointer in &touching {
            if pointer.comment.trim().is_empty() {
                continue;
            }
            if !pointers.iter().any(|item| item.id == pointer.id) {
                pointers.push(ExportPointerPlan {
                    id: pointer.id,
                    comment: pointer.comment.clone(),
                });
            }
            if let PointerTarget::Url { url } = &pointer.to {
                allowed_urls.insert(url.clone());
            }
            pointer_json.push(serde_json::json!({
                "pointerId": pointer.id,
                "comment": pointer.comment,
                "play": pointer.play,
                "classification": if pointer.play.is_some() { "play" } else { "exportOnly" },
                "from": pointer.from,
                "to": pointer.to,
                "target": crate::pointer_target_value(project, &pointer.to),
                "permission": crate::pointer_target_description(project, &pointer.to),
            }));
            collect_pointer_reference(project, pointer, &mut references_json, &mut allowed_urls);
        }
        pages_json.push(serde_json::json!({
            "pageId": page.id,
            "name": page.name,
            "path": page.path,
            "file": page_file(page),
            "output": page.output,
            "appearance": page.appearance,
            "sizes": sizes,
            "pointers": pointer_json,
        }));
    }
    let _ = ids;
    let linked: Vec<serde_json::Value> = project
        .linked
        .values()
        .filter(|definition| linked_ids.contains(&definition.id))
        .map(|definition| {
            serde_json::json!({
                "id": definition.id,
                "name": definition.name,
                "master": element_subtree(&definition.master),
            })
        })
        .collect();
    let asset_list: Vec<serde_json::Value> = assets
        .values()
        .map(|asset| serde_json::json!({"path": asset.path, "name": asset.name}))
        .collect();
    let references: Vec<serde_json::Value> = dedup_references(references_json);
    let identity = project.imported_identity.as_ref().map(|identity| {
        serde_json::json!({
            "sourceName": identity.source_name,
            "summary": identity.summary,
            "brandName": project.brief.brand_name,
        })
    });
    // File names are unique inside the export folder, whichever output a boundary writes.
    let mut used_files: BTreeSet<String> = BTreeSet::new();
    used_files.insert("slides.pdf".into());
    let planned_pages: Vec<ExportPagePlan> = chosen
        .iter()
        .map(|page| {
            let stem = crate::file_stem(&page.path);
            let file = match page.output {
                crate::OutputType::Webpage | crate::OutputType::AppScreen => {
                    unique_file(&mut used_files, if page.path == "/" {
                        "index.html".to_string()
                    } else {
                        format!("{stem}.html")
                    })
                }
                crate::OutputType::Image => {
                    let png = unique_file(&mut used_files, format!("{stem}.png"));
                    let svg_stem = png.trim_end_matches(".png").to_owned();
                    used_files.insert(format!("{svg_stem}.svg"));
                    png
                }
                crate::OutputType::Slide => String::new(),
            };
            ExportPagePlan {
                page_id: page.id,
                name: page.name.clone(),
                path: page.path.clone(),
                output: page.output,
                file,
                size_ids: page.sizes.iter().map(|size| size.id).collect(),
            }
        })
        .collect();
    // The design names the exact file each page writes, so cross-page links stay correct.
    for entry in &mut pages_json {
        let Some(page_id) = entry
            .get("pageId")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
        else {
            continue;
        };
        if let Some(planned) = planned_pages.iter().find(|page| page.page_id == page_id) {
            entry["file"] = serde_json::Value::String(planned.file.clone());
        }
    }
    let design = serde_json::json!({
        "project": {
            "name": project.name,
            "brief": project.brief,
            "identity": identity,
            "generationMode": match project.generation_mode {
                GenerationMode::Fuzzy => "flexible",
                GenerationMode::Strict => "precise",
            },
        },
        "pages": pages_json,
        "shared": {
            "linkedDefinitions": linked,
            "references": references,
            "assets": asset_list,
            "allowedExternalUrls": allowed_urls.iter().cloned().collect::<Vec<_>>(),
        },
    });
    let markdown = export_markdown(project.generation_mode, &design);
    let sha256 = digest(markdown.as_bytes());
    let schema = export_schema(job_id, &sha256);
    Ok(ExportPlan {
        job_id,
        project_id: project.id,
        base_revision: project.revision,
        kind: ExportPlanKind::Code,
        pages: planned_pages,
        request_count: ai_pages.len().max(1),
        page_count: ai_pages.len().max(1),
        pointers,
        assets: assets.into_values().collect(),
        markdown,
        sha256,
        schema,
        warnings: Vec::new(),
    })
}

/// Prepare the job that rewrites one pointer's comment to describe the marked code an outside
/// edit left in its place.
pub fn plan_comment(
    project: &Project,
    pointer_id: Uuid,
    marked_code: &str,
    job_id: Uuid,
) -> Result<ExportPlan> {
    let pointer = project
        .pointers
        .iter()
        .find(|pointer| pointer.id == pointer_id)
        .ok_or_else(|| Error("This pointer no longer exists.".into()))?;
    ensure(
        !job_id.is_nil(),
        "An export job needs a nonempty identifier.",
    )?;
    let source = crate::source_description(project, &pointer.from);
    let target = crate::pointer_target_description(project, &pointer.to);
    let design = serde_json::json!({
        "pointer": {
            "pointerId": pointer.id,
            "comment": pointer.comment,
            "from": source,
            "to": target,
            "markedCode": marked_code,
        },
    });
    let instructions = "Write the one-sentence comment this pointer should now carry so it \
describes the marked code exactly. Keep it in the same voice and vocabulary as the old \
comment. Do not ask for behaviour the code does not have, and do not add markup, prose \
around it or a trailing period when the old comment has none.";
    let markdown = format!(
        "{heading}## Generation setting\n```json\n{{\"generationMode\":\"{mode}\"}}\n```\n\n## Export task\n{instructions}\n\n## Marked code\n```json\n{design}\n```\n",
        heading = crate::current_requirements_heading(),
        mode = mode_name(project.generation_mode),
        design = serde_json::to_string_pretty(&design).map_err(|error| Error(error.to_string()))?,
    );
    let sha256 = digest(markdown.as_bytes());
    Ok(ExportPlan {
        job_id,
        project_id: project.id,
        base_revision: project.revision,
        kind: ExportPlanKind::Comment { pointer_id },
        pages: Vec::new(),
        pointers: vec![ExportPointerPlan {
            id: pointer.id,
            comment: pointer.comment.clone(),
        }],
        assets: Vec::new(),
        schema: comment_schema(job_id, &sha256),
        markdown,
        sha256,
        request_count: 1,
        page_count: 1,
        warnings: Vec::new(),
    })
}

fn mode_name(mode: GenerationMode) -> &'static str {
    match mode {
        GenerationMode::Fuzzy => "fuzzy",
        GenerationMode::Strict => "strict",
    }
}

fn export_markdown(mode: GenerationMode, design: &serde_json::Value) -> String {
    let instructions = "You are building the final, ordinary files a person can open without \
Desegnos, in the same folder. Follow the placed design closely: every size is a responsive \
breakpoint of the same page. Use semantic HTML, responsive CSS and plain JavaScript. No \
frameworks, no CDNs, no build step. Write only files the design needs. Pictures are already in \
assets/: reference them by the path given in shared.assets and never invent picture bytes. \
Each pointer in pages[].pointers is a behaviour described by its comment; implement it in the \
page that owns its source. Put data-desegnos-pointer=\"<pointerId>\" on every element that \
implements that pointer, and wrap that pointer's CSS or JavaScript in \
/* desegnos-pointer:<pointerId> */ and /* /desegnos-pointer:<pointerId> */ markers. Use the \
page file names given in pages[].file for links between pages. Links to allowedExternalUrls are \
the only external script or style sources permitted. Answer with the JSON object described by \
the reply schema and nothing else.";
    format!(
        "{heading}## Generation setting\n```json\n{{\"generationMode\":\"{mode}\"}}\n```\n\n## Export task\n{instructions}\n\n## Design\n```json\n{design}\n```\n",
        heading = crate::current_requirements_heading(),
        mode = mode_name(mode),
        design = serde_json::to_string_pretty(design).unwrap_or_else(|_| "{}".into()),
    )
}

/// The strict reply schema. The provider injects the frozen job id and hash as constants.
pub fn export_schema(job_id: Uuid, sha256: &str) -> serde_json::Value {
    let text = |max: usize| serde_json::json!({"type": "string", "description": format!("At most {max} Unicode characters, enforced by Desegnos.")});
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["jobId", "requirementsSha256", "summary", "pages", "shared", "pointers"],
        "properties": {
            "jobId": {"type": "string", "description": job_id.to_string()},
            "requirementsSha256": {"type": "string", "description": sha256},
            "summary": text(4000),
            "shared": {
                "type": "object",
                "additionalProperties": false,
                "required": ["css", "js"],
                "properties": {"css": text(1_000_000), "js": text(1_000_000)},
            },
            "pages": {
                "type": "array",
                "maxItems": 100,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["pageId", "path", "html", "css", "js"],
                    "properties": {
                        "pageId": {"type": "string"},
                        "path": {"type": "string", "description": "The exact path the request lists for this page."},
                        "html": text(1_000_000),
                        "css": text(1_000_000),
                        "js": text(1_000_000),
                    },
                },
            },
            "pointers": {
                "type": "array",
                "maxItems": 1000,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["pointerId", "implemented"],
                    "properties": {
                        "pointerId": {"type": "string"},
                        "implemented": {"type": "boolean"},
                    },
                },
            },
        },
    })
}

fn comment_schema(job_id: Uuid, sha256: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["jobId", "requirementsSha256", "comment"],
        "properties": {
            "jobId": {"type": "string", "description": job_id.to_string()},
            "requirementsSha256": {"type": "string", "description": sha256},
            "comment": {"type": "string", "maxLength": 2048},
        },
    })
}

/// The frozen provider request for a plan.
pub fn plan_snapshot(project: &Project, plan: &ExportPlan) -> crate::RequirementsSnapshot {
    let mut dependencies = BTreeMap::new();
    dependencies.insert(
        "export".into(),
        format!("{}:{}", project.id, project.revision),
    );
    crate::RequirementsSnapshot {
        job_id: plan.job_id,
        project_id: plan.project_id,
        base_revision: plan.base_revision,
        markdown: plan.markdown.clone(),
        sha256: plan.sha256.clone(),
        request_count: plan.request_count,
        page_count: plan.page_count,
        dependencies,
        dossier_context: None,
        request_history: String::new(),
        capabilities: Some(crate::GenerationCapabilities {
            web_search: false,
            image_downloads: 0,
            image_generation: None,
        }),
        reference_ids: Vec::new(),
        images: Vec::new(),
    }
}

/// The text of a reply, accepting a fenced JSON block around it.
fn reply_json(text: &str) -> Result<&str> {
    let text = text.trim();
    if text.starts_with('{') && text.ends_with('}') {
        return Ok(text);
    }
    let start = text.find('{').ok_or_else(|| {
        Error("The AI reply did not contain a JSON object. Try the export again.".into())
    })?;
    let end = text.rfind('}').ok_or_else(|| {
        Error("The AI reply did not contain a JSON object. Try the export again.".into())
    })?;
    ensure(
        end > start,
        "The AI reply did not contain a JSON object. Try the export again.",
    )?;
    Ok(&text[start..=end])
}

pub fn parse_export_reply(plan: &ExportPlan, text: &str) -> Result<ExportReply> {
    let value: ExportReply = serde_json::from_str(reply_json(text)?).map_err(|error| {
        Error(format!(
            "The AI export reply did not match the expected files. {error}"
        ))
    })?;
    validate_export_reply(plan, &value)?;
    Ok(value)
}

/// One rewritten pointer comment.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentReply {
    pub job_id: Uuid,
    pub requirements_sha256: String,
    pub comment: String,
}

pub fn parse_comment_reply(plan: &ExportPlan, text: &str) -> Result<String> {
    let value: CommentReply = serde_json::from_str(reply_json(text)?).map_err(|error| {
        Error(format!(
            "The AI comment reply did not match the expected shape. {error}"
        ))
    })?;
    ensure(
        value.job_id == plan.job_id && value.requirements_sha256 == plan.sha256,
        "The AI comment reply belongs to a different export.",
    )?;
    let comment = value.comment.trim();
    ensure(
        !comment.is_empty() && comment.chars().count() <= 2048 && !comment.contains('\0'),
        "The AI returned an empty or oversized comment.",
    )?;
    Ok(comment.into())
}

/// Validate a reply against the frozen plan: every page once, parseable files, no unexpected
/// external script or style sources, and within the size caps.
pub fn validate_export_reply(plan: &ExportPlan, reply: &ExportReply) -> Result<()> {
    ensure(
        reply.job_id == plan.job_id && reply.requirements_sha256 == plan.sha256,
        "The AI export reply belongs to a different export.",
    )?;
    ensure(
        reply.summary.chars().count() <= 4000 && !reply.summary.contains('\0'),
        "The AI export summary is too long.",
    )?;
    ensure(
        reply.shared.css.len() <= MAX_EXPORT_FILE_BYTES
            && reply.shared.js.len() <= MAX_EXPORT_FILE_BYTES,
        "The AI export shared files are too large.",
    )?;
    let expected: Vec<&ExportPagePlan> = plan
        .pages
        .iter()
        .filter(|page| page.output.needs_ai())
        .collect();
    ensure(
        reply.pages.len() == expected.len(),
        "The AI export reply did not cover every page exactly once.",
    )?;
    let mut seen = BTreeSet::new();
    let mut total = reply.shared.css.len() + reply.shared.js.len();
    for page in &reply.pages {
        let planned = expected
            .iter()
            .find(|planned| planned.page_id == page.page_id)
            .ok_or_else(|| Error("The AI export reply includes a page that was not asked for.".into()))?;
        ensure(
            seen.insert(page.page_id),
            "The AI export reply repeated a page.",
        )?;
        ensure(
            page.path == planned.path,
            "The AI export reply changed a page's public path.",
        )?;
        ensure(
            !page.html.trim().is_empty(),
            "The AI export reply left a page without HTML.",
        )?;
        for (label, text) in [
            ("HTML", &page.html),
            ("CSS", &page.css),
            ("JavaScript", &page.js),
        ] {
            ensure(
                text.len() <= MAX_EXPORT_FILE_BYTES,
                format!("A page's exported {label} is too large."),
            )?;
            ensure(
                !text.contains('\0'),
                format!("A page's exported {label} contains an invalid character."),
            )?;
            total += text.len();
        }
        check_html(&page.html, plan)?;
        check_css(&page.css, plan)?;
        check_script(&page.js, plan)?;
    }
    for page in expected {
        ensure(
            seen.contains(&page.page_id),
            "The AI export reply did not cover every page.",
        )?;
    }
    ensure(
        reply.pointers.len() == plan.pointers.len(),
        "The AI export reply did not answer for every pointer.",
    )?;
    let mut answered = BTreeSet::new();
    for pointer in &reply.pointers {
        ensure(
            plan.pointers.iter().any(|planned| planned.id == pointer.pointer_id),
            "The AI export reply answered for a pointer that was not in this export.",
        )?;
        ensure(
            answered.insert(pointer.pointer_id),
            "The AI export reply answered for a pointer twice.",
        )?;
    }
    check_css(&reply.shared.css, plan)?;
    check_script(&reply.shared.js, plan)?;
    ensure(
        total <= MAX_EXPORT_BYTES,
        "The AI export is too large to write.",
    )?;
    Ok(())
}

fn check_html(html: &str, plan: &ExportPlan) -> Result<()> {
    use swc_common::{FileName, SourceMap};
    let map = SourceMap::default();
    let file = map.new_source_file(FileName::Custom("page.html".into()).into(), html.to_owned());
    let mut errors = Vec::new();
    let config = swc_html_parser::parser::ParserConfig {
        scripting_enabled: true,
        ..Default::default()
    };
    let result = swc_html_parser::parse_file_as_document(&file, config, &mut errors);
    let fatal = errors.iter().any(|error| {
        !matches!(
            error.kind(),
            swc_html_parser::error::ErrorKind::StartTagWithoutDoctype
                | swc_html_parser::error::ErrorKind::EndTagSeenWithoutDoctype
                | swc_html_parser::error::ErrorKind::NonSpaceCharacterWithoutDoctype
                | swc_html_parser::error::ErrorKind::EofWithoutDoctype
        )
    });
    ensure(
        result.is_ok() && !fatal,
        "A page's exported HTML could not be parsed. Export again to rebuild it.",
    )?;
    check_no_external_sources(html, plan)
}

fn check_css(css: &str, plan: &ExportPlan) -> Result<()> {
    if css.trim().is_empty() {
        return Ok(());
    }
    lightningcss::stylesheet::StyleSheet::parse(
        css,
        lightningcss::stylesheet::ParserOptions::default(),
    )
    .map_err(|_| Error("A page's exported CSS could not be parsed.".into()))?;
    let lower = css.to_ascii_lowercase();
    for (label, needle) in [
        ("an imported stylesheet", "@import"),
        ("a remote asset", "url("),
    ] {
        let mut search = 0;
        while let Some(found) = lower[search..].find(needle) {
            let at = search + found;
            let end = lower.len().min(at + 512);
            if let Some(url) = first_external_url(&css[at..end])
                && !allowed(&url, plan)
            {
                return Err(Error(format!(
                    "The exported CSS loads {label} the design does not ask for: {url}"
                )));
            }
            search = at + needle.len();
        }
    }
    Ok(())
}

/// The first http(s) or protocol-relative URL in a fragment, without quotes or punctuation.
fn first_external_url(text: &str) -> Option<String> {
    let mut best: Option<usize> = None;
    for marker in ["http://", "https://", "//"] {
        if let Some(found) = text.find(marker) {
            best = Some(best.map_or(found, |current| current.min(found)));
        }
    }
    let start = best?;
    let tail = &text[start..];
    let url: String = tail
        .chars()
        .take_while(|character| {
            !character.is_whitespace()
                && !matches!(character, '"' | '\'' | ')' | ';' | '<' | '>' | '`')
        })
        .collect();
    (!url.is_empty()).then_some(url)
}

fn check_script(js: &str, plan: &ExportPlan) -> Result<()> {
    if js.trim().is_empty() {
        return Ok(());
    }
    for line in js.lines() {
        let line = line.trim();
        if (line.starts_with("import ") || line.starts_with("import("))
            && let Some(url) = first_external_url(line)
            && !allowed(&url, plan)
        {
            return Err(Error(format!(
                "The exported JavaScript imports an external script the design does not ask for: {url}"
            )));
        }
    }
    use swc_common::{FileName, SourceMap};
    let map = SourceMap::default();
    let file = map.new_source_file(FileName::Custom("page.js".into()).into(), js.to_owned());
    let mut errors = Vec::new();
    let result = swc_ecma_parser::parse_file_as_script(
        &file,
        swc_ecma_parser::Syntax::Es(Default::default()),
        swc_ecma_ast::EsVersion::latest(),
        None,
        &mut errors,
    );
    if result.is_ok() && errors.is_empty() {
        return Ok(());
    }
    let map = SourceMap::default();
    let file = map.new_source_file(FileName::Custom("page.js".into()).into(), js.to_owned());
    let mut module_errors = Vec::new();
    let module = swc_ecma_parser::parse_file_as_module(
        &file,
        swc_ecma_parser::Syntax::Es(Default::default()),
        swc_ecma_ast::EsVersion::latest(),
        None,
        &mut module_errors,
    );
    ensure(
        module.is_ok() && module_errors.is_empty(),
        "A page's exported JavaScript could not be parsed.",
    )
}

/// No external script or style may load unless a pointer or reference URL asks for it.
fn check_no_external_sources(html: &str, plan: &ExportPlan) -> Result<()> {
    let lower = html.to_ascii_lowercase();
    for (index, _) in lower.match_indices("<script") {
        let Some(end) = lower[index..].find('>') else {
            continue;
        };
        let tag = &html[index..index + end];
        if let Some(attribute) = attribute_value(tag, "src")
            && is_external(&attribute)
            && !allowed(&attribute, plan)
        {
            return Err(Error(format!(
                "The exported HTML loads an external script that the design does not ask for: {attribute}"
            )));
        }
    }
    for (index, _) in lower.match_indices("<link") {
        let Some(end) = lower[index..].find('>') else {
            continue;
        };
        let tag = &html[index..index + end];
        let is_stylesheet = attribute_value(tag, "rel")
            .is_some_and(|rel| rel.eq_ignore_ascii_case("stylesheet"));
        if is_stylesheet
            && let Some(attribute) = attribute_value(tag, "href")
            && is_external(&attribute)
            && !allowed(&attribute, plan)
        {
            return Err(Error(format!(
                "The exported HTML loads an external stylesheet that the design does not ask for: {attribute}"
            )));
        }
    }
    Ok(())
}

fn attribute_value(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut search = 0;
    while let Some(found) = lower[search..].find(name) {
        let start = search + found;
        let after = start + name.len();
        let boundary = lower[..start]
            .chars()
            .next_back()
            .is_none_or(|character| character.is_whitespace() || character == '<');
        let rest = lower[after..].trim_start();
        if boundary && rest.starts_with('=') {
            let value = rest[1..].trim_start();
            let quote = value.chars().next();
            let result = match quote {
                Some('"') | Some('\'') => value[1..].split(quote.unwrap()).next(),
                _ => value.split_whitespace().next(),
            };
            return result.map(|value| value.trim().to_owned());
        }
        search = after;
    }
    None
}

fn is_external(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("//")
}

fn allowed(url: &str, plan: &ExportPlan) -> bool {
    let url = url.trim();
    plan.markdown.contains(url)
}

/// The complete file set for an AI-built export, including direct outputs and pictures.
pub fn ai_export_bundle(
    project: &Project,
    plan: &ExportPlan,
    reply: &ExportReply,
) -> Result<ExportBundle> {
    validate_export_reply(plan, reply)?;
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut warnings = plan.warnings.clone();
    for asset in &plan.assets {
        if let Some((_, bytes)) = project.picture_bytes(asset.picture_id) {
            files.insert(asset.path.clone(), bytes);
        } else {
            warnings.push(format!("The picture {} could not be added.", asset.name));
        }
    }
    let shared_css = if reply.shared.css.trim().is_empty() {
        None
    } else {
        files.insert("shared.css".into(), reply.shared.css.as_bytes().to_vec());
        Some("shared.css".to_owned())
    };
    let shared_js = if reply.shared.js.trim().is_empty() {
        None
    } else {
        files.insert("shared.js".into(), reply.shared.js.as_bytes().to_vec());
        Some("shared.js".to_owned())
    };
    for page in &reply.pages {
        let planned = plan
            .pages
            .iter()
            .find(|planned| planned.page_id == page.page_id)
            .ok_or_else(|| Error("The AI export reply includes an unknown page.".into()))?;
        let stem = planned.file.trim_end_matches(".html").to_owned();
        let css_file = format!("{stem}.css");
        let js_file = format!("{stem}.js");
        if !page.css.trim().is_empty() {
            files.insert(css_file.clone(), page.css.as_bytes().to_vec());
        }
        if !page.js.trim().is_empty() {
            files.insert(js_file.clone(), page.js.as_bytes().to_vec());
        }
        let html = inject_files(
            &page.html,
            shared_css.as_deref(),
            shared_js.as_deref(),
            (!page.css.trim().is_empty()).then_some(css_file.as_str()),
            (!page.js.trim().is_empty()).then_some(js_file.as_str()),
        );
        files.insert(planned.file.clone(), html.into_bytes());
    }
    add_direct_outputs(project, plan, &mut files)?;
    let pointers = mark_pointer_code(project, plan, reply, &files, &mut warnings);
    let pages = plan
        .pages
        .iter()
        .map(|page| ExportedPage {
            page_id: page.page_id,
            path: page.path.clone(),
            output: page.output,
        })
        .collect();
    check_bundle(&files)?;
    Ok(ExportBundle {
        files,
        pages,
        pointers,
        warnings,
    })
}

/// Slides and images, rendered without the AI.
pub fn direct_export_bundle(project: &Project, page_ids: &[Uuid]) -> Result<ExportBundle> {
    let mut plan = plan_export(project, page_ids, Uuid::new_v4())?;
    plan.pages.retain(|page| !page.output.needs_ai());
    ensure(
        !plan.pages.is_empty(),
        "Only slides and images can be exported without the AI.",
    )?;
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let warnings = Vec::new();
    add_direct_outputs(project, &plan, &mut files)?;
    check_bundle(&files)?;
    let pages = plan
        .pages
        .iter()
        .map(|page| ExportedPage {
            page_id: page.page_id,
            path: page.path.clone(),
            output: page.output,
        })
        .collect();
    Ok(ExportBundle {
        files,
        pages,
        pointers: BTreeMap::new(),
        warnings,
    })
}

fn add_direct_outputs(
    project: &Project,
    plan: &ExportPlan,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let requested: BTreeSet<Uuid> = plan.pages.iter().map(|page| page.page_id).collect();
    let slides: Vec<&Page> = slide_order(project)
        .into_iter()
        .filter(|page| requested.contains(&page.id))
        .collect();
    if !slides.is_empty() {
        let mut render = Vec::new();
        for page in &slides {
            render.push(crate::RenderPage::new(project, page, page.base_size_id())?);
        }
        files.insert("slides.pdf".into(), crate::slide_pdf(project, &render)?);
    }
    for page in plan.pages.iter().filter(|page| page.output == crate::OutputType::Image) {
        let source = project
            .pages
            .iter()
            .find(|source| source.id == page.page_id)
            .ok_or_else(|| Error("An image boundary in this export no longer exists.".into()))?;
        let png = if page.file.is_empty() {
            unique_file(&mut BTreeSet::new(), format!("{}.png", crate::file_stem(&page.path)))
        } else {
            page.file.clone()
        };
        let svg = format!("{}.svg", png.trim_end_matches(".png"));
        let elements = crate::resolve_for_size(project, source, source.base_size_id());
        if crate::is_vector_representable(&elements) {
            files.insert(
                svg,
                crate::boundary_svg(project, source, source.base_size_id())?.into_bytes(),
            );
        }
        files.insert(
            png,
            crate::boundary_png(project, source, source.base_size_id())?,
        );
    }
    Ok(())
}

fn check_bundle(files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    ensure(
        !files.is_empty(),
        "The export produced no files to write.",
    )?;
    ensure(
        files.len() <= MAX_EXPORT_FILES,
        "The export has too many files.",
    )?;
    ensure(
        files.values().map(Vec::len).sum::<usize>() <= MAX_EXPORT_BYTES,
        "The export is too large to write.",
    )?;
    for (path, bytes) in files {
        ensure(
            !path.is_empty()
                && !path.contains(['\\', ':', '\0'])
                && !path.starts_with('/')
                && path
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != ".."),
            "The export contains an unsafe file path.",
        )?;
        ensure(
            bytes.len() <= MAX_EXPORT_BYTES,
            "An exported file is too large.",
        )?;
    }
    Ok(())
}

fn inject_files(
    html: &str,
    shared_css: Option<&str>,
    shared_js: Option<&str>,
    css: Option<&str>,
    js: Option<&str>,
) -> String {
    let mut head = String::new();
    if let Some(shared) = shared_css {
        head.push_str(&format!(
            "<link rel=\"stylesheet\" href=\"{shared}\">"
        ));
    }
    if let Some(css) = css {
        head.push_str(&format!("<link rel=\"stylesheet\" href=\"{css}\">"));
    }
    let mut body = String::new();
    if let Some(shared) = shared_js {
        body.push_str(&format!("<script src=\"{shared}\" defer></script>"));
    }
    if let Some(js) = js {
        body.push_str(&format!("<script src=\"{js}\" defer></script>"));
    }
    if head.is_empty() && body.is_empty() {
        return html.to_owned();
    }
    let mut output = html.to_owned();
    if !head.is_empty() {
        let lower = output.to_ascii_lowercase();
        match lower.find("</head>") {
            Some(position) => output.insert_str(position, &head),
            None => {
                // A fragment without a head still gets its styles at the top.
                output.insert_str(0, &head);
            }
        }
    }
    if !body.is_empty() {
        let lower = output.to_ascii_lowercase();
        match lower.rfind("</body>") {
            Some(position) => output.insert_str(position, &body),
            None => output.push_str(&body),
        }
    }
    output
}

/// Record the code each implemented pointer produced and its fingerprint.
fn mark_pointer_code(
    project: &Project,
    plan: &ExportPlan,
    reply: &ExportReply,
    files: &BTreeMap<String, Vec<u8>>,
    warnings: &mut Vec<String>,
) -> BTreeMap<Uuid, ExportedPointer> {
    let fingerprints = pointer_fingerprints(files);
    let mut records = BTreeMap::new();
    for pointer in &plan.pointers {
        let implemented = reply
            .pointers
            .iter()
            .any(|answer| answer.pointer_id == pointer.id && answer.implemented);
        if !implemented {
            continue;
        }
        match fingerprints.get(&pointer.id) {
            Some(fingerprint) => {
                let _ = project;
                records.insert(
                    pointer.id,
                    ExportedPointer {
                        fingerprint: fingerprint.clone(),
                        comment: pointer.comment.clone(),
                    },
                );
            }
            None => warnings.push(
                "A pointer the AI marked implemented has no marked code, so it is not wired."
                    .into(),
            ),
        }
    }
    records
}

/// SHA-256 over each pointer's marked fragments, in stable file and source order. Covers CSS and
/// JavaScript marker blocks and HTML elements carrying `data-desegnos-pointer`.
pub fn pointer_fingerprints(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<Uuid, String> {
    let mut fragments: BTreeMap<Uuid, Vec<String>> = BTreeMap::new();
    for (path, bytes) in files {
        if !matches!(
            path.rsplit('.').next().unwrap_or_default(),
            "html" | "css" | "js" | "svg"
        ) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        if path.ends_with(".html") || path.ends_with(".svg") {
            for (id, fragment) in html_fragments(text) {
                fragments
                    .entry(id)
                    .or_default()
                    .push(format!("{path}\n{fragment}"));
            }
        }
        for (id, fragment) in marked_fragments(text) {
            fragments
                .entry(id)
                .or_default()
                .push(format!("{path}\n{fragment}"));
        }
    }
    let mut fingerprints = BTreeMap::new();
    for (id, fragments) in fragments {
        let mut bytes = Vec::new();
        for fragment in fragments {
            bytes.extend_from_slice(fragment.as_bytes());
            bytes.push(0);
        }
        fingerprints.insert(id, digest(&bytes));
    }
    fingerprints
}

fn normalize_fragment(fragment: &str) -> String {
    fragment.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The marker blocks that carry a pointer's CSS or JavaScript.
fn marked_fragments(text: &str) -> Vec<(Uuid, String)> {
    let mut rows = Vec::new();
    for (_, id, after_open) in find_markers(text) {
        let close = format!("/* /desegnos-pointer:{id} */");
        if let Some(end) = text[after_open..].find(&close) {
            let fragment = &text[after_open..after_open + end];
            rows.push((id, normalize_fragment(fragment)));
        }
    }
    rows
}

fn find_markers(text: &str) -> Vec<(usize, Uuid, usize)> {
    let mut rows = Vec::new();
    let needle = "/* desegnos-pointer:";
    let mut search = 0;
    while let Some(found) = text[search..].find(needle) {
        let start = search + found;
        let after = start + needle.len();
        let Some(end) = text[after..].find("*/") else {
            break;
        };
        let id = text[after..after + end].trim();
        if let Ok(id) = Uuid::parse_str(id) {
            rows.push((start, id, after + end + 2));
        }
        search = after + end + 2;
    }
    rows
}

/// Elements carrying `data-desegnos-pointer`, with their whole subtree.
fn html_fragments(html: &str) -> Vec<(Uuid, String)> {
    let mut rows = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut search = 0;
    while search < html.len() {
        let Some(found) = lower[search..].find("data-desegnos-pointer") else {
            break;
        };
        let at = search + found;
        search = at + "data-desegnos-pointer".len();
        let Some(value) = attribute_value(&html[at..], "data-desegnos-pointer") else {
            continue;
        };
        let Ok(id) = Uuid::parse_str(value.trim()) else {
            continue;
        };
        let Some(tag_start) = html[..at].rfind('<') else {
            continue;
        };
        let name: String = html[tag_start + 1..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if name.is_empty() || html[tag_start..].starts_with("</") {
            continue;
        }
        let Some(tag_end) = find_tag_end(html, at) else {
            continue;
        };
        let start_tag = &html[tag_start..tag_end];
        if start_tag.trim_end().ends_with("/>") || is_void(&name) {
            rows.push((id, normalize_fragment(start_tag)));
            search = search.max(tag_end);
            continue;
        }
        let Some(end) = matching_close(html, &lower, tag_end, &name) else {
            continue;
        };
        rows.push((id, normalize_fragment(&html[tag_start..end])));
        search = search.max(end);
    }
    rows
}

fn find_tag_end(html: &str, from: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, character) in html[from..].char_indices() {
        match character {
            '"' | '\'' if quote.is_none() => quote = Some(character),
            character if Some(character) == quote => quote = None,
            '>' if quote.is_none() => return Some(from + offset + 1),
            _ => {}
        }
    }
    None
}

fn matching_close(html: &str, lower: &str, from: usize, name: &str) -> Option<usize> {
    let open = format!("<{name}");
    let close = format!("</{name}");
    let mut depth = 1_i32;
    let mut search = from;
    while search < lower.len() {
        let next_open = lower[search..].find(&open).map(|offset| search + offset);
        let next_close = lower[search..].find(&close).map(|offset| search + offset);
        match (next_open, next_close) {
            (_, None) => return None,
            (Some(open_at), Some(close_at)) if open_at < close_at => {
                let after = open_at + open.len();
                let boundary = lower[after..]
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_whitespace() || character == '>' || character == '/');
                if boundary {
                    depth += 1;
                }
                search = after;
            }
            (_, Some(close_at)) => {
                depth -= 1;
                let end = find_tag_end(html, close_at).unwrap_or(close_at + close.len());
                if depth == 0 {
                    return Some(end);
                }
                search = end;
            }
        }
    }
    None
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn pointer_touches(pointer: &Pointer, page_id: Uuid) -> bool {
    let source = match &pointer.from {
        PointerSource::Element { page_id, .. } | PointerSource::Boundary { page_id, .. } => {
            *page_id
        }
    };
    let target = match &pointer.to {
        PointerTarget::Element { page_id, .. } | PointerTarget::Boundary { page_id, .. } => {
            Some(*page_id)
        }
        PointerTarget::Reference { .. } | PointerTarget::Url { .. } => None,
    };
    source == page_id || target == Some(page_id)
}

fn collect_links(elements: &[Element], ids: &mut BTreeSet<Uuid>) {
    for element in elements {
        if let Some(link) = element.link {
            ids.insert(link);
        }
        collect_links(&element.children, ids);
    }
}

fn collect_pictures(
    project: &Project,
    elements: &[Element],
    page: &Page,
    assets: &mut BTreeMap<Uuid, ExportAssetPlan>,
) {
    for element in elements {
        if let Some(picture) = element.image {
            let extension = project
                .picture_bytes(picture)
                .map(|(mime, _)| match mime.as_str() {
                    "image/png" => "png",
                    "image/jpeg" => "jpg",
                    "image/webp" => "webp",
                    "image/gif" => "gif",
                    "image/svg+xml" => "svg",
                    _ => "bin",
                })
                .unwrap_or("bin");
            let stem = crate::file_stem(&format!("{}-{}", page.name, element.label()));
            let path = format!(
                "assets/{stem}-{}.{extension}",
                &picture.simple().to_string()[..8]
            );
            assets.entry(picture).or_insert(ExportAssetPlan {
                picture_id: picture,
                path,
                name: element.label(),
            });
        }
        collect_pictures(project, &element.children, page, assets);
    }
}

fn collect_attachments(
    elements: &[Element],
    references: &mut Vec<serde_json::Value>,
    urls: &mut BTreeSet<String>,
) {
    for element in elements {
        let attached = element
            .meta
            .pending
            .iter()
            .flat_map(|pending| pending.attachments.iter())
            .chain(element.meta.context.iter());
        for attachment in attached {
            if let Some(url) = &attachment.url {
                urls.insert(url.clone());
                references.push(serde_json::json!({
                    "kind": "link",
                    "name": attachment.name,
                    "url": url,
                }));
            } else if let Some(text) = &attachment.text {
                references.push(serde_json::json!({
                    "kind": "note",
                    "name": attachment.name,
                    "text": text.chars().take(4000).collect::<String>(),
                }));
            }
        }
        collect_attachments(&element.children, references, urls);
    }
}

fn collect_pointer_reference(
    project: &Project,
    pointer: &Pointer,
    references: &mut Vec<serde_json::Value>,
    urls: &mut BTreeSet<String>,
) {
    match &pointer.to {
        PointerTarget::Reference { item_id } => {
            if let Some(item) = project.reference.iter().find(|item| item.id == *item_id) {
                match &item.kind {
                    crate::ReferenceKind::Note { text } => references.push(serde_json::json!({
                        "kind": "note",
                        "text": text.chars().take(4000).collect::<String>(),
                    })),
                    crate::ReferenceKind::Link { url, title } => {
                        urls.insert(url.clone());
                        references.push(serde_json::json!({
                            "kind": "link",
                            "title": title,
                            "url": url,
                        }));
                    }
                    crate::ReferenceKind::Swatch { color } => {
                        references.push(serde_json::json!({"kind": "swatch", "colour": color}))
                    }
                    crate::ReferenceKind::Image { name, .. } => {
                        references.push(serde_json::json!({"kind": "picture", "name": name}))
                    }
                }
            }
        }
        PointerTarget::Url { url } => {
            urls.insert(url.clone());
            references.push(serde_json::json!({"kind": "link", "url": url}));
        }
        _ => {}
    }
}

fn dedup_references(rows: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for row in rows {
        let key = row.to_string();
        if seen.insert(key) {
            result.push(row);
        }
    }
    result
}

/// The manifest an export folder carries: its version, destination and pointer fingerprints.
pub fn export_manifest(record: &ExportRecord) -> Vec<u8> {
    let value = serde_json::json!({
        "version": EXPORT_MANIFEST_VERSION,
        "projectId": record.project_id,
        "exportedVersion": record.version,
        "destination": record.destination,
        "exportedAt": record.exported_at,
        "pages": record.pages.iter().map(|page| serde_json::json!({
            "pageId": page.page_id,
            "path": page.path,
            "output": page.output,
        })).collect::<Vec<_>>(),
        "pointers": record.pointers.iter().map(|(id, pointer)| serde_json::json!({
            "pointerId": id,
            "fingerprint": pointer.fingerprint,
            "comment": pointer.comment,
        })).collect::<Vec<_>>(),
    });
    serde_json::to_vec_pretty(&value).unwrap_or_default()
}
