//! Stage 1 of framed-document editing: pages, editable site HTML files and frame requests.
use base64::{Engine, engine::general_purpose::STANDARD};
use desegnos_core::*;
use std::collections::BTreeMap;
use uuid::Uuid;

const IFRAME: &str = "iframesite";
const PAGE_IFRAME: &str = "iframepage";
const SITE: &str = "play/solopong/index.html";
const SITE_HTML: &str = "<!doctype html>\n<html><head><title>Solopong</title></head><body><main><h1>Original framed heading</h1></main></body></html>";
const FRAMED_PAGE_HTML: &str =
    "<!doctype html>\n<html><body><main><h1>Framed page heading</h1></main></body></html>";

fn project(
    mode: GenerationMode,
    iframes: &[(&str, &str)],
    site: bool,
    framed_page: bool,
) -> Project {
    let mut project = Project::new(
        "Framed documents".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    project.generation_mode = mode;
    let home = project.home_page_id;
    if framed_page {
        project.pages.push(Page {
            id: Uuid::new_v4(),
            name: "Framed".into(),
            path: "/framed".into(),
            included: true,
            viewport: project.viewport,
            appearance: project.appearance,
            regions: vec![],
            content: Some(SiteContent {
                html: FRAMED_PAGE_HTML.into(),
                css: String::new(),
                javascript: String::new(),
            }),
        });
    }
    let frames: String = iframes
        .iter()
        .map(|(id, src)| format!("<iframe data-desegnos-id=\"{id}\" src=\"{src}\"></iframe>"))
        .collect();
    project.pages[0].content = Some(SiteContent {
        html: format!("<!doctype html>\n<html><body>{frames}</body></html>"),
        css: String::new(),
        javascript: String::new(),
    });
    let mut page_files = BTreeMap::from([(home, "index.html".to_string())]);
    if framed_page {
        page_files.insert(project.pages[1].id, "framed/index.html".into());
    }
    let mut files = BTreeMap::new();
    if site {
        files.insert(SITE.into(), STANDARD.encode(SITE_HTML).into());
    }
    project.website = Some(ImportedWebsite {
        page_files,
        files,
        framework: None,
        skipped: vec![],
    });
    project
}

fn host_rect() -> Rect {
    Rect {
        x: 120.0,
        y: 80.0,
        width: 480.0,
        height: 320.0,
    }
}

fn frame_rect() -> Rect {
    Rect {
        x: 16.0,
        y: 16.0,
        width: 320.0,
        height: 180.0,
    }
}

fn framed_request(project: Project, iframe_id: &str, document: DocumentTarget) -> (Project, Uuid) {
    let page_id = project.home_page_id;
    let project = project
        .apply(
            project.revision,
            Command::AddRegion {
                page_id,
                rect: host_rect(),
            },
        )
        .unwrap();
    let region_id = project.pages[0].regions.last().unwrap().id;
    let project = project
        .apply(
            project.revision,
            Command::SetRegionFrameTarget {
                page_id,
                region_id,
                frame_target: Some(FrameTarget {
                    page_id,
                    iframe_id: iframe_id.into(),
                    document,
                    rect: frame_rect(),
                }),
            },
        )
        .unwrap();
    let project = project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id,
                region_id,
                rect: host_rect(),
                prompt: "Update the framed heading.".into(),
            },
        )
        .unwrap();
    (project, region_id)
}

fn reply(
    project: &Project,
    snapshot: &RequirementsSnapshot,
    pages: Vec<GeneratedPage>,
    documents: Vec<GeneratedDocument>,
) -> GenerationCandidate {
    GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Updated the framed request.".into(),
        regions: project
            .pages
            .iter()
            .flat_map(|page| page.regions.iter())
            .filter(|region| !region.completed && !region.prompt.trim().is_empty())
            .map(|region| RegionOutcome {
                region_id: region.id,
                status: RegionStatus::Implemented,
                explanation: "Changed the requested framed content.".into(),
            })
            .collect(),
        pages,
        documents,
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],
        outside_changes: vec![],
    }
}

fn site_content(html: &str) -> SiteContent {
    SiteContent {
        html: html.into(),
        css: String::new(),
        javascript: String::new(),
    }
}

#[test]
fn frame_targets_resolve_pages_site_files_external_and_unresolvable_sources() {
    let project = project(
        GenerationMode::Strict,
        &[
            (IFRAME, SITE),
            (PAGE_IFRAME, "framed/index.html"),
            ("iframeroute", "/framed"),
            ("iframeextern", "https://example.com/embed"),
            ("iframemissing", "play/missing/index.html"),
        ],
        true,
        true,
    );
    let targets = project.frame_targets(project.home_page_id).unwrap();
    assert_eq!(targets.len(), 5);
    assert_eq!(targets[0].iframe_id, IFRAME);
    assert_eq!(targets[0].src, SITE);
    assert_eq!(
        targets[0].source,
        FrameSource::SiteFile { path: SITE.into() }
    );
    assert_eq!(
        targets[0].document,
        Some(DocumentTarget::SiteFile { path: SITE.into() })
    );
    assert_eq!(
        targets[0].source_sha256.as_deref(),
        project
            .document_hash(&DocumentTarget::SiteFile { path: SITE.into() })
            .ok()
            .as_deref()
    );
    let framed_page = project.pages[1].id;
    assert_eq!(
        targets[1].source,
        FrameSource::Page {
            page_id: framed_page
        }
    );
    assert_eq!(
        targets[2].source,
        FrameSource::Page {
            page_id: framed_page
        }
    );
    assert_eq!(
        targets[3].source,
        FrameSource::External {
            url: "https://example.com/embed".into()
        }
    );
    assert!(matches!(
        targets[4].source,
        FrameSource::Unresolvable { .. }
    ));
    assert!(targets[3].document.is_none() && targets[4].document.is_none());
    assert_eq!(project.site_document_paths(), vec![SITE.to_string()]);
}

#[test]
fn site_file_edits_are_undoable_and_export_at_their_original_path() {
    let project = project(GenerationMode::Strict, &[(IFRAME, SITE)], true, false);
    let document = DocumentTarget::SiteFile { path: SITE.into() };
    let original = project.site_document_html(SITE).unwrap();
    let target = ElementTarget {
        id: String::new(),
        selector: "body > main:nth-of-type(1)".into(),
        tag: "main".into(),
        label: "Framed content".into(),
        source_sha256: project.document_hash(&document).unwrap(),
    };
    let mut session = Session::new(project.clone()).unwrap();
    let edited = session
        .edit_element_document(
            0,
            &document,
            &target,
            &ElementEdit::Replace {
                html: "<section><h1>Edited framed copy</h1></section>".into(),
                keep_style: false,
            },
            |_| Ok(()),
        )
        .unwrap()
        .expect("the framed root stays selected");
    assert!(!edited.id.is_empty());
    assert!(
        session
            .project()
            .site_document_html(SITE)
            .unwrap()
            .contains("Edited framed copy")
    );
    assert_ne!(
        session.project().site_document_html(SITE).unwrap(),
        original
    );
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(
        session.project().site_document_html(SITE).unwrap(),
        original
    );
    assert_eq!(
        session.project().pages[0].content.as_ref().unwrap().html,
        project.pages[0].content.as_ref().unwrap().html
    );
    session.redo(|_| Ok(())).unwrap();
    let exported = compile_website(session.project()).unwrap();
    let output = String::from_utf8(exported.files[SITE].clone()).unwrap();
    assert!(output.contains("Edited framed copy"));
    assert!(!output.contains(ELEMENT_ID_ATTRIBUTE));
    assert_eq!(
        session.project().pages[0].content.as_ref().unwrap().html,
        project.pages[0].content.as_ref().unwrap().html
    );
}

#[test]
fn framed_site_requirements_include_the_document_and_the_reply_updates_that_file() {
    let project = project(GenerationMode::Strict, &[(IFRAME, SITE)], true, false);
    let document = DocumentTarget::SiteFile { path: SITE.into() };
    let (project, region_id) = framed_request(project, IFRAME, document.clone());
    let snapshot = project.prepare_requirements().unwrap();
    assert!(snapshot.markdown.contains("## Framed document requests"));
    assert!(snapshot.markdown.contains(SITE));
    assert!(snapshot.markdown.contains("Original framed heading"));
    assert!(snapshot.markdown.contains("\"iframeId\": \"iframesite\""));
    assert!(
        snapshot
            .dependencies
            .contains_key(&format!("document:{}", document.key()))
    );

    let changed = SITE_HTML.replace("Original framed heading", "Updated framed heading");
    let candidate = reply(
        &project,
        &snapshot,
        vec![],
        vec![GeneratedDocument {
            document: document.clone(),
            content: site_content(&changed),
        }],
    );
    candidate.validate(&snapshot, &project).unwrap();
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    let text = applied.site_document_html(SITE).unwrap();
    assert!(text.contains("Updated framed heading"));
    assert!(text.contains(ELEMENT_ID_ATTRIBUTE));
    assert!(
        applied
            .pages
            .iter()
            .flat_map(|page| page.regions.iter())
            .any(|region| region.id == region_id && region.completed)
    );
    let exported = compile_website(&applied).unwrap();
    let output = String::from_utf8(exported.files[SITE].clone()).unwrap();
    assert!(output.contains("Updated framed heading"));
    assert!(!output.contains(ELEMENT_ID_ATTRIBUTE));
}

#[test]
fn framed_page_requirements_include_its_source_and_the_reply_updates_that_page() {
    let project = project(
        GenerationMode::Strict,
        &[(PAGE_IFRAME, "framed/index.html")],
        true,
        true,
    );
    let framed_page = project.pages[1].id;
    let document = DocumentTarget::Page {
        page_id: framed_page,
    };
    let (project, _) = framed_request(project, PAGE_IFRAME, document);
    let snapshot = project.prepare_requirements().unwrap();
    assert!(snapshot.markdown.contains("Framed page heading"));
    assert!(
        snapshot
            .dependencies
            .contains_key(&format!("document:page:{framed_page}"))
    );

    let candidate = reply(
        &project,
        &snapshot,
        vec![GeneratedPage {
            page_id: framed_page,
            content: site_content(
                "<!doctype html>\n<html><body><main><h1>Framed page updated</h1></main></body></html>",
            ),
        }],
        vec![],
    );
    candidate.validate(&snapshot, &project).unwrap();
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    assert!(
        applied
            .page(framed_page)
            .unwrap()
            .content
            .as_ref()
            .unwrap()
            .html
            .contains("Framed page updated")
    );
    assert_eq!(
        applied.pages[0].content.as_ref().unwrap().html,
        project.pages[0].content.as_ref().unwrap().html
    );
}

#[test]
fn flexible_framed_generation_may_change_the_host_page_and_the_framed_file() {
    let project = project(GenerationMode::Fuzzy, &[(IFRAME, SITE)], true, false);
    let document = DocumentTarget::SiteFile { path: SITE.into() };
    let (project, _) = framed_request(project, IFRAME, document.clone());
    let snapshot = project.prepare_requirements().unwrap();
    assert!(
        snapshot
            .markdown
            .contains("you may also change the host page's HTML, CSS and JavaScript")
    );
    assert!(
        snapshot
            .markdown
            .contains("In Flexible mode the framed document is the anchor")
    );
    assert!(
        !snapshot.markdown.contains(
            "In Flexible mode you may also change the host page's iframe element and CSS"
        )
    );
    assert!(
        result_schema(&snapshot)["properties"]
            .get("outsideChanges")
            .is_some()
    );
    let home = project.home_page_id;
    let mut host = project.pages[0].content.clone().unwrap();
    host.html = host
        .html
        .replace("<iframe ", "<iframe class=\"framed\" ")
        .replace("</body>", "<div class=\"tooltip-controller\"></div></body>");
    host.css = "iframe { border: 0; width: 100%; }".into();
    host.javascript = "document.documentElement.dataset.framedHost = 'ready';".into();
    let changed = SITE_HTML.replace("Original framed heading", "Flexible framed heading");
    let mut candidate = reply(
        &project,
        &snapshot,
        vec![GeneratedPage {
            page_id: home,
            content: host,
        }],
        vec![GeneratedDocument {
            document: document.clone(),
            content: site_content(&changed),
        }],
    );
    candidate.outside_changes = vec![OutsideChange {
        document: "Home".into(),
        description: "Added the tooltip controller and its styles.".into(),
    }];
    candidate.validate(&snapshot, &project).unwrap();
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    let page = applied.page(home).unwrap();
    let host = page.content.as_ref().unwrap();
    assert!(host.html.contains("class=\"framed\""));
    assert!(host.html.contains("tooltip-controller"));
    assert!(host.css.contains("width: 100%"));
    assert!(host.javascript.contains("framedHost"));
    assert!(
        applied
            .site_document_html(SITE)
            .unwrap()
            .contains("Flexible framed heading")
    );
}

#[test]
fn precise_framed_generation_rejects_host_page_changes() {
    let project = project(GenerationMode::Strict, &[(IFRAME, SITE)], true, false);
    let document = DocumentTarget::SiteFile { path: SITE.into() };
    let (project, _) = framed_request(project, IFRAME, document.clone());
    let snapshot = project.prepare_requirements().unwrap();
    assert!(
        snapshot
            .markdown
            .contains("return the host page's HTML, CSS and JavaScript exactly as they are")
    );
    let home = project.home_page_id;
    let mut host = project.pages[0].content.clone().unwrap();
    host.html = host
        .html
        .replace("<iframe ", "<iframe class=\"framed\" ")
        .replace("</body>", "<div class=\"tooltip-controller\"></div></body>");
    host.css = "iframe { border: 0; }".into();
    host.javascript = "document.documentElement.dataset.framedHost = 'ready';".into();
    let changed = SITE_HTML.replace("Original framed heading", "Precise framed heading");
    let candidate = reply(
        &project,
        &snapshot,
        vec![GeneratedPage {
            page_id: home,
            content: host,
        }],
        vec![GeneratedDocument {
            document,
            content: site_content(&changed),
        }],
    );
    assert!(
        candidate.validate(&snapshot, &project).is_err(),
        "Precise framed requests change only the framed document"
    );
    assert!(
        project
            .apply_generation(&snapshot, &candidate, &[])
            .is_err(),
        "Apply must reject the same host change"
    );
}

#[test]
fn flexible_framed_generation_still_rejects_unlisted_pages() {
    let mut project = project(GenerationMode::Fuzzy, &[(IFRAME, SITE)], true, true);
    let other = project.pages[1].id;
    project.pages[1].name = "Archive".into();
    project.pages[1].path = "/archive".into();
    let document = DocumentTarget::SiteFile { path: SITE.into() };
    let (project, _) = framed_request(project, IFRAME, document.clone());
    let snapshot = project.prepare_requirements().unwrap();
    let candidate = reply(
        &project,
        &snapshot,
        vec![GeneratedPage {
            page_id: other,
            content: site_content(
                "<!doctype html>\n<html><body><main><h1>Unrequested page</h1></main></body></html>",
            ),
        }],
        vec![GeneratedDocument {
            document,
            content: site_content(SITE_HTML),
        }],
    );
    assert!(
        candidate.validate(&snapshot, &project).is_err(),
        "only the host page, the framed document and the listed editable documents may change"
    );
}

#[test]
fn flexible_framed_page_changes_are_not_reported_as_outside_changes() {
    let project = project(
        GenerationMode::Fuzzy,
        &[(PAGE_IFRAME, "framed/index.html")],
        true,
        true,
    );
    let framed_page = project.pages[1].id;
    let document = DocumentTarget::Page {
        page_id: framed_page,
    };
    let (project, _) = framed_request(project, PAGE_IFRAME, document);
    let snapshot = project.prepare_requirements().unwrap();
    let candidate = reply(
        &project,
        &snapshot,
        vec![GeneratedPage {
            page_id: framed_page,
            content: site_content(
                "<!doctype html>\n<html><body><main><h1>Framed page updated</h1></main></body></html>",
            ),
        }],
        vec![],
    );
    let (accepted, events) = recover_generation_reply(
        &serde_json::to_string(&candidate).unwrap(),
        &snapshot,
        &project,
    )
    .unwrap();
    accepted.validate(&snapshot, &project).unwrap();
    assert!(
        !events.iter().any(|event| event.kind == "outsideChanges"),
        "the framed document is the anchor, not an outside change: {events:?}"
    );
}

#[test]
fn an_authored_iframe_can_gain_its_editor_identity_for_a_framed_request() {
    let mut project = project(GenerationMode::Strict, &[], true, false);
    project.pages[0].content = Some(SiteContent {
        html: format!("<!doctype html>\n<html><body><iframe src=\"{SITE}\"></iframe></body></html>"),
        css: String::new(),
        javascript: String::new(),
    });
    let home = project.home_page_id;
    let hash = project
        .page(home)
        .unwrap()
        .content
        .as_ref()
        .unwrap()
        .sha256()
        .unwrap();
    let target = ElementTarget {
        id: String::new(),
        selector: "body > iframe:nth-of-type(1)".into(),
        tag: "iframe".into(),
        label: String::new(),
        source_sha256: hash,
    };
    let (identified, element) = project
        .ensure_element_id_in(&DocumentTarget::Page { page_id: home }, project.revision, &target)
        .unwrap();
    let element = element.expect("identified iframe");
    assert!(!element.id.is_empty(), "the iframe gains an editor ID");
    assert_eq!(element.tag, "iframe");
    let content = identified.pages[0].content.as_ref().unwrap();
    assert!(content.html.contains("data-desegnos-id="));
    // The bytes change only by editor metadata; a second call is a no-op.
    let (same, again) = identified
        .ensure_element_id_in(
            &DocumentTarget::Page { page_id: home },
            identified.revision,
            &element,
        )
        .unwrap();
    assert_eq!(again, Some(element));
    assert_eq!(same.revision, identified.revision);
    assert_eq!(same.pages[0].content, identified.pages[0].content);
}
