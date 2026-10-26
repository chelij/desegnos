//! A result may be applied on top of the edits the user made while its request was running.
use base64::{Engine, engine::general_purpose::STANDARD};
use desegnos_core::*;
use std::collections::BTreeMap;
use uuid::Uuid;

const HTML: &str =
    "<main>\n  <h1>Studio</h1>\n  <p class=\"intro\">We design calm interfaces.</p>\n</main>";

fn content(html: &str) -> SiteContent {
    SiteContent {
        html: html.into(),
        css: String::new(),
        javascript: String::new(),
    }
}

/// One box request on a page that already has accepted source.
fn project() -> Project {
    let mut project = Project::new(
        "Edits during generation".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    project.generation_mode = GenerationMode::Strict;
    let home = project.home_page_id;
    project = project
        .apply(
            project.revision,
            Command::AddRegion {
                page_id: home,
                rect: Rect {
                    x: 24.0,
                    y: 32.0,
                    width: 520.0,
                    height: 160.0,
                },
            },
        )
        .unwrap();
    let region = project.pages[0].regions[0].id;
    project = project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id: home,
                region_id: region,
                rect: project.pages[0].regions[0].rect,
                prompt: "Add a primary call to action below the intro.".into(),
            },
        )
        .unwrap();
    project.pages[0].content = Some(SiteContent {
        html: HTML.into(),
        css: "main { padding: 32px; }".into(),
        javascript: String::new(),
    });
    project
}

fn element(project: &Project, page_id: Uuid, suffix: &str) -> ElementTarget {
    let page = project.page(page_id).unwrap();
    let map = project.page_source_map(page_id).unwrap();
    let range = map
        .ranges
        .iter()
        .find(|range| range.selector.ends_with(suffix))
        .unwrap();
    ElementTarget {
        id: range.id.clone(),
        tag: suffix.split(':').next().unwrap().into(),
        label: "Selected".into(),
        selector: range.selector.clone(),
        source_sha256: page.content.as_ref().unwrap().sha256().unwrap(),
    }
}

fn candidate(
    project: &Project,
    snapshot: &RequirementsSnapshot,
    html: &str,
) -> GenerationCandidate {
    GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Added a primary call to action.".into(),
        regions: project
            .pages
            .iter()
            .flat_map(|page| {
                page.regions
                    .iter()
                    .filter(|region| !region.prompt.trim().is_empty())
                    .map(|region| RegionOutcome {
                        region_id: region.id,
                        status: RegionStatus::Implemented,
                        explanation: "Added a native button.".into(),
                    })
            })
            .collect(),
        pages: vec![GeneratedPage {
            page_id: project.home_page_id,
            content: SiteContent {
                html: html.into(),
                css: "main { padding: 32px; }".into(),
                javascript: String::new(),
            },
        }],
        documents: vec![],
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],
        outside_changes: vec![],
    }
}

fn added_area(project: &Project) -> String {
    let region = project.pages[0].regions[0].id;
    format!(
        "<main>\n  <h1>Studio</h1>\n  <p class=\"intro\">We design calm interfaces.</p>\n  <section data-desegnos-region=\"{region}\"><button class=\"cta\">Contact us</button></section>\n</main>"
    )
}

#[test]
fn an_edit_to_another_element_is_merged_with_the_generated_area() {
    let project = project();
    let snapshot = project.prepare_requirements().unwrap();
    let reply = candidate(&project, &snapshot, &added_area(&project));
    reply.validate(&snapshot, &project).unwrap();
    let paragraph = element(&project, project.home_page_id, "p:nth-of-type(1)");
    let (edited, _) = project
        .edit_element(
            project.revision,
            project.home_page_id,
            &paragraph,
            &ElementEdit::Update {
                texts: vec![],
                attributes: Default::default(),
                style: Some("letter-spacing:0.02em".into()),
            },
        )
        .unwrap();
    assert!(snapshot.validate_current(&edited).is_err());
    let mut session = Session::new(edited.clone()).unwrap();
    session
        .apply_generation(&snapshot, &reply, &[], |_| Ok(()))
        .unwrap();
    let applied = session.project().clone();
    let html = &applied.pages[0].content.as_ref().unwrap().html;
    assert!(html.contains("letter-spacing:0.02em"), "{html}");
    assert!(html.contains("data-desegnos-region"), "{html}");
    assert!(html.contains("Contact us"), "{html}");
    assert!(!html.contains("<<<<<<<"));
    assert!(applied.applied_job_ids.contains(&reply.job_id));
    assert!(applied.pages[0].regions[0].completed);
    // One undo step returns the pages to how the user left them, edits included.
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, edited.pages);
    assert_eq!(
        session
            .project()
            .latest_generation
            .as_ref()
            .map(|dossier| dossier.application_status),
        Some(GenerationApplicationStatus::Undone)
    );
}

#[test]
fn changing_the_text_the_generation_rewrites_stays_stale() {
    let mut project = project();
    let home = project.home_page_id;
    project = project
        .apply(
            project.revision,
            Command::SetRegionWholePage {
                page_id: home,
                region_id: project.pages[0].regions[0].id,
                whole_page: true,
            },
        )
        .unwrap();
    let snapshot = project.prepare_requirements().unwrap();
    let reply = candidate(
        &project,
        &snapshot,
        "<main>\n  <h1>Studio</h1>\n  <p class=\"intro\">We craft calm interfaces.</p>\n</main>",
    );
    reply.validate(&snapshot, &project).unwrap();
    let paragraph = element(&project, home, "p:nth-of-type(1)");
    let (edited, _) = project
        .edit_element(
            project.revision,
            home,
            &paragraph,
            &ElementEdit::Update {
                texts: vec![TextEdit {
                    index: 0,
                    text: "We make calm interfaces.".into(),
                }],
                attributes: Default::default(),
                style: None,
            },
        )
        .unwrap();
    let error = edited.apply_generation(&snapshot, &reply, &[]).unwrap_err();
    assert!(
        error.0.contains("Existing page source changed"),
        "{}",
        error.0
    );
    assert!(!edited.applied_job_ids.contains(&reply.job_id));
}

#[test]
fn a_changed_request_keeps_the_result_stale() {
    let project = project();
    let snapshot = project.prepare_requirements().unwrap();
    let reply = candidate(&project, &snapshot, &added_area(&project));
    let mut changed = project.clone();
    changed.revision += 1;
    changed.pages[0].regions[0]
        .prompt
        .push_str(" Use a round button.");
    let error = changed
        .apply_generation(&snapshot, &reply, &[])
        .unwrap_err();
    assert!(error.0.contains("A requested area changed"), "{}", error.0);
}

/// A page whose elements already carry editor IDs, as an edited page always does.
const ANCHORED_HTML: &str = "<main data-desegnos-id=\"studiomain\">\n  <h1 data-desegnos-id=\"studiohead\">Studio</h1>\n  <p data-desegnos-id=\"intropara1\" class=\"intro\">We design calm interfaces.</p>\n</main>";

/// One request anchored to the intro paragraph, with its box measured from that element.
fn anchored_project() -> Project {
    let mut project = Project::new(
        "Anchored request".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    project.generation_mode = GenerationMode::Strict;
    let home = project.home_page_id;
    project.pages[0].content = Some(content(ANCHORED_HTML));
    let paragraph = element(&project, home, "p:nth-of-type(1)");
    let bounds = vec![TargetBounds {
        target: paragraph.clone(),
        rect: Rect {
            x: 24.0,
            y: 96.0,
            width: 480.0,
            height: 64.0,
        },
    }];
    project = project
        .apply(
            project.revision,
            Command::AddElementRegions {
                page_id: home,
                elements: vec![paragraph],
                bounds,
                allow_target_removal: false,
            },
        )
        .unwrap();
    let region = project.pages[0].regions[0].id;
    project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id: home,
                region_id: region,
                rect: project.pages[0].regions[0].rect,
                prompt: "Restyle the intro paragraph.".into(),
            },
        )
        .unwrap()
}

/// The measured box the anchored request moves to once the heading above it grows.
fn moved_bounds() -> Rect {
    Rect {
        x: 24.0,
        y: 168.0,
        width: 480.0,
        height: 64.0,
    }
}

/// A heading edit that pushes the anchored paragraph down, then the page measurement the
/// editor records for it. The user never touched the anchored request itself.
fn move_the_paragraph(project: &Project) -> Project {
    let home = project.home_page_id;
    let heading = element(project, home, "h1:nth-of-type(1)");
    let (edited, _) = project
        .edit_element(
            project.revision,
            home,
            &heading,
            &ElementEdit::Update {
                texts: vec![],
                attributes: Default::default(),
                style: Some("margin-bottom:72px".into()),
            },
        )
        .unwrap();
    let region = edited.pages[0].regions[0].id;
    let measured = ElementTargetBounds {
        region_id: region,
        bounds: vec![TargetBounds {
            target: element(&edited, home, "p:nth-of-type(1)"),
            rect: moved_bounds(),
        }],
    };
    let refreshed = edited
        .refresh_element_target_bounds(home, edited.pages[0].viewport, &[measured])
        .unwrap();
    assert_eq!(refreshed.pages[0].regions[0].rect, moved_bounds());
    refreshed
}

/// A reply for the anchored request: an element request may not add shared CSS.
fn anchored_reply(project: &Project, snapshot: &RequirementsSnapshot) -> GenerationCandidate {
    let mut reply = candidate(
        project,
        snapshot,
        "<main data-desegnos-id=\"studiomain\">\n  <h1 data-desegnos-id=\"studiohead\">Studio</h1>\n  <p data-desegnos-id=\"intropara1\" class=\"intro\">We design calm interfaces. Start a project today.</p>\n</main>",
    );
    reply.pages[0].content.css.clear();
    reply
}

#[test]
fn a_measured_element_move_does_not_make_an_anchored_request_stale() {
    let project = anchored_project();
    let snapshot = project.prepare_requirements().unwrap();
    let reply = anchored_reply(&project, &snapshot);
    reply.validate(&snapshot, &project).unwrap();
    let refreshed = move_the_paragraph(&project);
    // The page measurement changed the request's box even though the user changed no request.
    assert!(snapshot.validate_current(&refreshed).is_err());
    let applied = refreshed.apply_generation(&snapshot, &reply, &[]).unwrap();
    let html = &applied.pages[0].content.as_ref().unwrap().html;
    assert!(html.contains("margin-bottom:72px"), "{html}");
    assert!(html.contains("Start a project today."), "{html}");
    assert!(!html.contains("<<<<<<<"));
    assert!(applied.pages[0].regions[0].completed);
    // The applied project keeps the refreshed box the user sees, not the frozen one.
    assert_eq!(applied.pages[0].regions[0].rect, moved_bounds());
}

#[test]
fn a_changed_prompt_on_a_measured_request_stays_stale() {
    let project = anchored_project();
    let snapshot = project.prepare_requirements().unwrap();
    let reply = anchored_reply(&project, &snapshot);
    reply.validate(&snapshot, &project).unwrap();
    let refreshed = move_the_paragraph(&project);
    let mut changed = refreshed.clone();
    changed.revision += 1;
    changed.pages[0].regions[0]
        .prompt
        .push_str(" Use a round button.");
    let error = changed
        .apply_generation(&snapshot, &reply, &[])
        .unwrap_err();
    assert!(error.0.contains("A requested area changed"), "{}", error.0);
    assert!(!changed.applied_job_ids.contains(&reply.job_id));
}

#[test]
fn a_moved_free_box_keeps_the_result_stale() {
    let project = project();
    let snapshot = project.prepare_requirements().unwrap();
    let reply = candidate(&project, &snapshot, &added_area(&project));
    let home = project.home_page_id;
    let region = project.pages[0].regions[0].id;
    let mut rect = project.pages[0].regions[0].rect;
    rect.y += 120.0;
    let moved = project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id: home,
                region_id: region,
                rect,
                prompt: project.pages[0].regions[0].prompt.clone(),
            },
        )
        .unwrap();
    let error = moved.apply_generation(&snapshot, &reply, &[]).unwrap_err();
    assert!(error.0.contains("A requested area changed"), "{}", error.0);
}

/// A home page whose iframe shows an imported site file the request may change.
fn framed_project() -> (Project, String) {
    const INDEX: &str = "projects/index.html";
    let mut project = Project::new(
        "Framed document".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    project.generation_mode = GenerationMode::Strict;
    let home = project.home_page_id;
    project.pages[0].content = Some(content(
        "<!doctype html>\n<html><body><iframe data-desegnos-id=\"iframeproj\" name=\"project-view\" src=\"projects/index.html\"></iframe></body></html>",
    ));
    project.website = Some(ImportedWebsite {
        page_files: BTreeMap::from([(home, "index.html".to_string())]),
        files: BTreeMap::from([
            (
                "index.html".to_string(),
                STANDARD
                    .encode("<!doctype html>\n<html><body><p>Imported home</p></body></html>")
                    .into(),
            ),
            (
                INDEX.to_string(),
                STANDARD
                    .encode(
                        "<!doctype html>\n<html><body>\n  <h1>Projects</h1>\n  <p class=\"intro\">Selected work.</p>\n</body></html>",
                    )
                    .into(),
            ),
        ]),
        framework: None,
        skipped: vec![],
    });
    let rect = Rect {
        x: 640.0,
        y: 16.0,
        width: 768.0,
        height: 88.0,
    };
    project = project
        .apply(
            project.revision,
            Command::AddRegion {
                page_id: home,
                rect,
            },
        )
        .unwrap();
    let region = project.pages[0].regions[0].id;
    project = project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id: home,
                region_id: region,
                rect,
                prompt: "Move the heading up.".into(),
            },
        )
        .unwrap();
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
    (project.adopt_frame_targets().unwrap(), INDEX.into())
}

#[test]
fn an_edit_to_a_framed_site_file_is_merged_with_the_generated_change() {
    let (project, path) = framed_project();
    let snapshot = project.prepare_requirements().unwrap();
    let target = DocumentTarget::SiteFile { path: path.clone() };
    let source = project.document_content(&target).unwrap().html;
    let mut reply = candidate(&project, &snapshot, "");
    reply.pages.clear();
    reply.documents = vec![GeneratedDocument {
        document: target.clone(),
        content: content(&source.replace("<h1>Projects</h1>", "<h1>Our work</h1>")),
    }];
    reply.validate(&snapshot, &project).unwrap();
    let paragraph = ElementTarget {
        id: String::new(),
        selector: "body > p:nth-of-type(1)".into(),
        tag: "p".into(),
        label: "Intro".into(),
        source_sha256: project.document_hash(&target).unwrap(),
    };
    let (edited, _) = project
        .edit_element_in(
            &target,
            project.revision,
            &paragraph,
            &ElementEdit::Update {
                texts: vec![],
                attributes: Default::default(),
                style: Some("letter-spacing:0.02em".into()),
            },
        )
        .unwrap();
    assert!(snapshot.validate_current(&edited).is_err());
    let applied = edited.apply_generation(&snapshot, &reply, &[]).unwrap();
    let html = applied.site_document_html(&path).unwrap();
    assert!(html.contains("Our work"), "{html}");
    assert!(html.contains("letter-spacing:0.02em"), "{html}");
    assert!(html.contains("Selected work."), "{html}");
}
