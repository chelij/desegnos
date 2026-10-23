//! Format 23: framed requests gain their document and Flexible requests see the pages they cover.
use base64::{Engine, engine::general_purpose::STANDARD};
use desegni_core::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

fn content(html: &str) -> SiteContent {
    SiteContent {
        html: html.into(),
        css: String::new(),
        javascript: String::new(),
    }
}

fn page(project: &Project, name: &str, path: &str, html: &str) -> Page {
    Page {
        id: Uuid::new_v4(),
        name: name.into(),
        path: path.into(),
        included: true,
        viewport: project.viewport,
        appearance: project.appearance,
        regions: vec![],
        content: Some(content(html)),
    }
}

/// A home page whose iframe shows a project index page with sibling sub-pages.
fn page_project(mode: GenerationMode) -> (Project, Uuid, Uuid, Uuid) {
    let mut project = Project::new(
        "Projects".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    project.generation_mode = mode;
    let index = page(
        &project,
        "Projects",
        "/projects",
        "<!doctype html>\n<html><body><main><h1>All projects</h1></main></body></html>",
    );
    let atlas = page(
        &project,
        "Atlas",
        "/projects/atlas",
        "<!doctype html>\n<html><body><main><h1>Atlas case study</h1></main></body></html>",
    );
    let borealis = page(
        &project,
        "Borealis",
        "/projects/borealis",
        "<!doctype html>\n<html><body><main><h1>Borealis case study</h1></main></body></html>",
    );
    let about = page(
        &project,
        "About",
        "/about",
        "<!doctype html>\n<html><body><main><h1>About us</h1></main></body></html>",
    );
    let (index_id, atlas_id, borealis_id) = (index.id, atlas.id, borealis.id);
    project.pages.push(index);
    project.pages.push(atlas);
    project.pages.push(borealis);
    project.pages.push(about);
    project.pages[0].content = Some(content(
        "<!doctype html>\n<html><body><iframe data-desegnos-id=\"iframeproj\" name=\"project-view\" src=\"/projects\"></iframe></body></html>",
    ));
    (project, index_id, atlas_id, borealis_id)
}

const PROMPT: &str = "projects sub-pages have large empty space at the top can you remove";

/// A box drawn before framed editing existed: no frame target, only the host-page rect and the
/// measured iframe box recorded by the canvas.
fn pending_request(project: Project, prompt: &str) -> (Project, Uuid) {
    let home = project.home_page_id;
    let rect = Rect {
        x: 640.0,
        y: 16.0,
        width: 768.0,
        height: 88.0,
    };
    let project = project
        .apply(
            project.revision,
            Command::AddRegion {
                page_id: home,
                rect,
            },
        )
        .unwrap();
    let region_id = project.pages[0].regions[0].id;
    let project = project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id: home,
                region_id,
                rect,
                prompt: prompt.into(),
            },
        )
        .unwrap();
    let mut project = project;
    project.pages[0].regions[0].area_bounds = vec![TargetBounds {
        target: ElementTarget {
            id: "iframeproj".into(),
            selector: "body > iframe:nth-of-type(1)".into(),
            tag: "iframe".into(),
            label: "Project view".into(),
            source_sha256: String::new(),
        },
        rect: Rect {
            x: 400.0,
            y: 0.0,
            width: 800.0,
            height: 700.0,
        },
    }];
    (project, region_id)
}

fn implemented(project: &Project) -> Vec<RegionOutcome> {
    project
        .pages
        .iter()
        .flat_map(|page| page.regions.iter())
        .filter(|region| !region.prompt.trim().is_empty())
        .map(|region| RegionOutcome {
            region_id: region.id,
            status: RegionStatus::Implemented,
            explanation: "Removed the empty space.".into(),
        })
        .collect()
}

fn candidate(
    project: &Project,
    snapshot: &RequirementsSnapshot,
    pages: Vec<GeneratedPage>,
    documents: Vec<GeneratedDocument>,
) -> GenerationCandidate {
    GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Removed the empty space above the project pages.".into(),
        regions: implemented(project),
        pages,
        documents,
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],
    }
}

#[test]
fn a_request_drawn_before_framed_editing_gains_its_document_before_prepare() {
    let (project, index, _, _) = page_project(GenerationMode::Fuzzy);
    let (project, region_id) = pending_request(project, "Remove the empty space at the top");
    assert!(project.pages[0].regions[0].frame_target.is_none());
    let adopted = project.adopt_frame_targets().unwrap();
    let region = &adopted.pages[0].regions[0];
    assert_eq!(region.id, region_id);
    // The canvas keeps the host-page box; the frame target carries the document and inner box.
    assert_eq!(region.rect, project.pages[0].regions[0].rect);
    let frame = region.frame_target.as_ref().expect("a frame target");
    assert_eq!(frame.iframe_id, "iframeproj");
    assert_eq!(frame.document, DocumentTarget::page(index));
    assert!(
        frame.rect.width > region.rect.width,
        "the frame is drawn smaller than its document"
    );
    // Adopting again changes nothing.
    assert_eq!(adopted.adopt_frame_targets().unwrap(), adopted);

    let snapshot = adopted.prepare_requirements().unwrap();
    assert_eq!(snapshot.format_version().unwrap(), 23);
    assert!(snapshot.markdown.contains("## Framed document requests"));
    assert!(snapshot.markdown.contains("All projects"));
    assert!(
        snapshot
            .dependencies
            .contains_key(&format!("document:page:{index}"))
    );
    snapshot.validate_current(&adopted).unwrap();
}

#[test]
fn flexible_sub_page_request_includes_the_framed_and_sibling_pages_and_updates_several() {
    let (project, index, atlas, borealis) = page_project(GenerationMode::Fuzzy);
    let (project, _) = pending_request(project, PROMPT);
    let project = project.adopt_frame_targets().unwrap();
    let snapshot = project.prepare_requirements().unwrap();
    let included: Vec<_> = snapshot
        .documents
        .iter()
        .map(|document| document.document.clone())
        .collect();
    assert!(included.contains(&DocumentTarget::page(index)));
    assert!(included.contains(&DocumentTarget::page(atlas)));
    assert!(included.contains(&DocumentTarget::page(borealis)));
    // A page outside the project directory stays out.
    assert!(!included.contains(&DocumentTarget::page(project.pages[4].id)));
    assert!(
        snapshot
            .markdown
            .contains("## Editable documents for this request")
    );
    assert!(snapshot.markdown.contains("/projects/atlas"));
    assert!(snapshot.markdown.contains("/projects/borealis"));
    assert!(
        snapshot
            .markdown
            .contains("Do not report a document's source as unavailable")
    );
    snapshot.validate_current(&project).unwrap();

    let candidate = candidate(
        &project,
        &snapshot,
        vec![
            GeneratedPage {
                page_id: index,
                content: content(
                    "<!doctype html>\n<html><body><main><h1>All projects</h1></main></body></html>",
                ),
            },
            GeneratedPage {
                page_id: atlas,
                content: content(
                    "<!doctype html>\n<html><body><main><h1>Atlas without the gap</h1></main></body></html>",
                ),
            },
            GeneratedPage {
                page_id: borealis,
                content: content(
                    "<!doctype html>\n<html><body><main><h1>Borealis without the gap</h1></main></body></html>",
                ),
            },
        ],
        vec![],
    );
    candidate.validate(&snapshot, &project).unwrap();
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    assert_eq!(applied.revision, project.revision + 1);
    for page in [atlas, borealis] {
        assert!(
            applied
                .page(page)
                .unwrap()
                .content
                .as_ref()
                .unwrap()
                .html
                .contains("without the gap")
        );
    }
    // The unrelated page and the home page are untouched by one undoable change.
    assert_eq!(applied.pages[0].content, project.pages[0].content);
    assert_eq!(applied.pages[4].content, project.pages[4].content);
}

#[test]
fn precise_framed_request_includes_only_the_framed_document() {
    let (project, index, atlas, _) = page_project(GenerationMode::Strict);
    let (project, _) = pending_request(project, PROMPT);
    let project = project.adopt_frame_targets().unwrap();
    let snapshot = project.prepare_requirements().unwrap();
    assert_eq!(
        snapshot
            .documents
            .iter()
            .map(|document| document.document.clone())
            .collect::<Vec<_>>(),
        vec![DocumentTarget::page(index)]
    );
    assert!(
        !snapshot
            .markdown
            .contains("## Editable documents for this request")
    );
    assert!(
        !snapshot
            .dependencies
            .contains_key(&format!("source:{atlas}"))
    );
    let rejected = candidate(
        &project,
        &snapshot,
        vec![GeneratedPage {
            page_id: atlas,
            content: content(
                "<!doctype html>\n<html><body><main><h1>Atlas without the gap</h1></main></body></html>",
            ),
        }],
        vec![],
    );
    assert!(rejected.validate(&snapshot, &project).is_err());
}

#[test]
fn the_context_budget_keeps_the_framed_document_first_and_drops_large_siblings() {
    let (project, index, atlas, borealis) = page_project(GenerationMode::Fuzzy);
    let mut project = project;
    // Two siblings that cannot both fit in the 400 KiB document budget.
    let filler = "Tall case study text. ".repeat(13_000);
    for page_id in [atlas, borealis] {
        let page = project
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .unwrap();
        page.content = Some(content(&format!(
            "<!doctype html>\n<html><body><main><h1>Case study</h1><p>{filler}</p></main></body></html>"
        )));
    }
    let (project, _) = pending_request(project, PROMPT);
    let project = project.adopt_frame_targets().unwrap();
    let snapshot = project.prepare_requirements().unwrap();
    assert_eq!(
        snapshot.documents.first().unwrap().document,
        DocumentTarget::page(index)
    );
    let total: usize = snapshot
        .documents
        .iter()
        .map(|document| {
            project
                .document_content(&document.document)
                .unwrap()
                .byte_len()
        })
        .sum();
    assert!(
        total <= MAX_DOCUMENT_CONTEXT_BYTES,
        "the editable-document source exceeded its budget: {total}"
    );
    assert_eq!(
        snapshot.documents.len(),
        2,
        "one large sibling fits, the next does not"
    );
    snapshot.validate_current(&project).unwrap();
}

fn site_project(mode: GenerationMode) -> (Project, String, String, String) {
    const INDEX: &str = "projects/index.html";
    const ATLAS: &str = "projects/atlas/index.html";
    const BOREALIS: &str = "projects/borealis/index.html";
    let mut project = Project::new(
        "Imported projects".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    project.generation_mode = mode;
    let home = project.home_page_id;
    project.pages[0].content = Some(content(
        "<!doctype html>\n<html><body><iframe data-desegnos-id=\"iframeproj\" name=\"project-view\" src=\"projects/index.html\"></iframe></body></html>",
    ));
    let files = BTreeMap::from([
        (
            "index.html".to_string(),
            STANDARD.encode(
                "<!doctype html>\n<html><body><p>Imported home</p></body></html>".as_bytes(),
            )
            .into(),
        ),
        (
            INDEX.to_string(),
            STANDARD
                .encode(
                    "<!doctype html>\n<html><head><link rel=\"stylesheet\" href=\"site.css\"></head><body><main><h1>Projects</h1></main></body></html>"
                        .as_bytes(),
                )
                .into(),
        ),
        (
            ATLAS.to_string(),
            STANDARD
                .encode(
                    "<!doctype html>\n<html><body><main><h1>Atlas</h1></main></body></html>"
                        .as_bytes(),
                )
                .into(),
        ),
        (
            BOREALIS.to_string(),
            STANDARD
                .encode(
                    "<!doctype html>\n<html><body><main><h1>Borealis</h1></main></body></html>"
                        .as_bytes(),
                )
                .into(),
        ),
        (
            "projects/site.css".to_string(),
            STANDARD.encode("main { padding-top: 120px; }".as_bytes()).into(),
        ),
    ]);
    project.website = Some(ImportedWebsite {
        page_files: BTreeMap::from([(home, "index.html".to_string())]),
        files,
        framework: None,
        skipped: vec![],
    });
    (
        project,
        INDEX.to_string(),
        ATLAS.to_string(),
        BOREALIS.to_string(),
    )
}

#[test]
fn imported_site_sub_pages_and_their_stylesheet_are_editable_in_flexible_mode() {
    let (project, index, atlas, borealis) = site_project(GenerationMode::Fuzzy);
    let (project, _) = pending_request(project, PROMPT);
    let project = project.adopt_frame_targets().unwrap();
    let snapshot = project.prepare_requirements().unwrap();
    let included: Vec<_> = snapshot
        .documents
        .iter()
        .map(|document| document.document.clone())
        .collect();
    for path in [&index, &atlas, &borealis] {
        assert!(
            included.contains(&DocumentTarget::SiteFile { path: path.clone() }),
            "{path} is editable"
        );
    }
    assert_eq!(snapshot.documents[0].reason, DocumentReason::Framed);
    assert!(snapshot.markdown.contains("projects/site.css"));
    assert!(snapshot.markdown.contains("padding-top: 120px"));
    snapshot.validate_current(&project).unwrap();

    let index_source = project.site_document_html(&index).unwrap();
    let changed = candidate(
        &project,
        &snapshot,
        vec![],
        vec![
            GeneratedDocument {
                document: DocumentTarget::SiteFile {
                    path: index.clone(),
                },
                content: content(&index_source),
            },
            GeneratedDocument {
                document: DocumentTarget::SiteFile {
                    path: atlas.clone(),
                },
                content: content(
                    "<!doctype html>\n<html><body><main><h1>Atlas without the gap</h1></main></body></html>",
                ),
            },
        ],
    );
    changed.validate(&snapshot, &project).unwrap();
    let applied = project.apply_generation(&snapshot, &changed, &[]).unwrap();
    assert!(
        applied
            .site_document_html(&atlas)
            .unwrap()
            .contains("without the gap")
    );
    // The framed index document was returned unchanged and keeps its content.
    assert!(
        applied
            .site_document_html(&index)
            .unwrap()
            .contains("<h1 data-desegnos-id")
            && applied
                .site_document_html(&index)
                .unwrap()
                .contains("Projects")
    );
}

// Keep the Arc import meaningful for older compilers that do not see it through BTreeMap.
#[allow(dead_code)]
fn stored(value: &str) -> Arc<str> {
    Arc::from(value)
}

#[test]
fn history_and_validation_fingerprints_cover_every_included_document() {
    let (project, index, atlas, _) = page_project(GenerationMode::Fuzzy);
    let (project, _) = pending_request(project, PROMPT);
    let project = project.adopt_frame_targets().unwrap();
    let snapshot = project.prepare_requirements().unwrap();
    for page_id in [index, atlas] {
        assert!(
            snapshot
                .dependencies
                .contains_key(&format!("document:page:{page_id}"))
        );
        // Page documents also keep their full source fingerprint for request history.
        assert!(
            snapshot
                .dependencies
                .contains_key(&format!("source:{page_id}"))
        );
    }
    let entries = earlier_requests(&snapshot, 10, "applied", "Tidied the gap", &[]);
    assert_eq!(entries.len(), 1);
    for page_id in [index, atlas] {
        assert!(
            entries[0]
                .document_fingerprints
                .contains_key(&DocumentTarget::page(page_id).key())
        );
    }

    // A hand edit to an included document is both stale for validation and reported to the AI.
    let mut edited = project.clone();
    edited
        .pages
        .iter_mut()
        .find(|page| page.id == atlas)
        .unwrap()
        .content = Some(content(
        "<!doctype html>\n<html><body><main><h1>Hand edited</h1></main></body></html>",
    ));
    assert!(snapshot.validate_current(&edited).is_err());
    let mut entries = entries;
    entries[0].job_id = Uuid::new_v4();
    let rendered = snapshot
        .clone()
        .with_earlier_requests(&edited, &entries, 7200)
        .unwrap();
    assert!(
        rendered
            .request_history
            .contains(&format!("\"documents\":[\"page:{atlas}\"]"))
    );
    snapshot.validate_current(&project).unwrap();
}
