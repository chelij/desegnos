//! Export plans, reply checks, direct rendering, records and website import.
use desegnos_core::{
    Appearance, BoundaryKind, Command, Element, ElementKind, ExportRecord, ExportReply,
    ExportedPage, ExportedPointer, InputType, OutputType, PageSize, Pointer, PointerSource,
    PointerTarget, Project, Rect, ReferenceKind, Session, Viewport, WebCapture, WebNode, WebScript,
    WebStyles, direct_export_bundle, empty_element, import_website, parse_export_reply, plan_export,
    validate_export_reply,
};
use std::collections::BTreeMap;
use uuid::Uuid;

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn project() -> Project {
    Project::new(
        "Export".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap()
}

fn apply(project: &Project, command: Command) -> Project {
    project.apply(project.revision, command).unwrap()
}

fn element(id: Uuid, kind: ElementKind, rect: Rect) -> Element {
    let mut element = empty_element(kind, rect);
    element.id = id;
    element
}

// ---------------------------------------------------------------------------
// Export reply validation
// ---------------------------------------------------------------------------

fn webpage_project() -> (Project, Uuid) {
    let project = project();
    let page = project.home_page_id;
    let element_id = Uuid::new_v4();
    let project = apply(
        &project,
        Command::InsertElements {
            page_id: page,
            parent_id: None,
            index: 0,
            elements: vec![element(element_id, ElementKind::Text, rect(0.0, 0.0, 200.0, 40.0))],
            picture_data_uri: None,
            size_id: None,
        },
    );
    let pointer_id = Uuid::new_v4();
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: Pointer {
                id: pointer_id,
                kind: desegnos_core::PointerKind::Change,
                from: PointerSource::Element {
                    page_id: page,
                    element_id,
                    size_id: None,
                },
                to: PointerTarget::Boundary {
                    page_id: page,
                    size_id: None,
                },
                comment: "Opens this boundary".into(),
                wiring: None,
                play: None,
                play_comment: String::new(),
            },
        },
    );
    (project, pointer_id)
}

fn reply(plan: &desegnos_core::ExportPlan, implemented_pointer: Uuid) -> ExportReply {
    ExportReply {
        job_id: plan.job_id,
        requirements_sha256: plan.sha256.clone(),
        summary: "Built the page".into(),
        pages: Vec::new(),
        shared: desegnos_core::ExportReplyShared {
            css: String::new(),
            js: format!("/* desegnos-pointer:{implemented_pointer} */\nconsole.log('ready');\n/* /desegnos-pointer:{implemented_pointer} */"),
        },
        pointers: plan
            .pointers
            .iter()
            .map(|pointer| desegnos_core::ExportReplyPointer {
                pointer_id: pointer.id,
                implemented: pointer.id == implemented_pointer,
            })
            .collect(),
    }
}

#[test]
fn excluded_boundaries_can_be_previewed_without_joining_the_full_export() {
    let mut project = project();
    let excluded = project.pages[0].id;
    project.pages[0].included = false;
    assert!(plan_export(&project, &[], Uuid::new_v4()).is_err());
    let preview = plan_export(&project, &[excluded], Uuid::new_v4()).unwrap();
    assert_eq!(preview.pages[0].page_id, excluded);
    assert!(!project.pages[0].included);
}

#[test]
fn boundary_background_updates_validate_hex_and_leave_editor_theme_alone() {
    let project = project();
    let command = |background: &str| Command::UpdatePage {
        page_id: project.pages[0].id, name: "Home".into(), path: "/".into(),
        background: background.into(), included: true,
    };
    let changed = apply(&project, command("#123"));
    assert_eq!(changed.pages[0].background, "#112233");
    assert_eq!(changed.appearance, project.appearance);
    assert!(project.apply(project.revision, command("red")).is_err());
    assert!(project.apply(project.revision, command("#ffffff;display:none")).is_err());
}

#[test]
fn export_reply_is_checked_against_the_frozen_plan() {
    let (project, pointer_id) = webpage_project();
    let plan = plan_export(&project, &[], Uuid::new_v4()).unwrap();
    assert_eq!(plan.pages.len(), 1);
    assert_eq!(plan.pointers.len(), 1);
    let valid = reply(&plan, pointer_id);
    validate_export_reply(&plan, &valid).unwrap();

    // Fenced JSON is accepted; the same reply parsed through the public entry point.
    let text = format!("```json\n{}\n```", serde_json::to_string(&valid).unwrap());
    let parsed = parse_export_reply(&plan, &text).unwrap();
    assert_eq!(parsed.job_id, plan.job_id);

    let mut wrong_job = valid.clone();
    wrong_job.job_id = Uuid::new_v4();
    assert!(parse_export_reply(&plan, &serde_json::to_string(&wrong_job).unwrap()).is_err());

    let mut extra_page = valid.clone();
    extra_page.pages.push(desegnos_core::ExportReplyPage {
        page_id: Uuid::new_v4(),
        color_scheme: "dark".into(),
    });
    assert!(parse_export_reply(&plan, &serde_json::to_string(&extra_page).unwrap()).is_err());

    let mut css_reply = valid.clone();
    css_reply.shared.css = "body { color: red; }".into();
    assert!(validate_export_reply(&plan, &css_reply).is_err());

    let mut unmarked = valid.clone();
    unmarked.shared.js = "console.log('unowned');".into();
    assert!(validate_export_reply(&plan, &unmarked).is_err());

    let mut bad_js = valid.clone();
    bad_js.shared.js = format!("/* desegnos-pointer:{pointer_id} */const = ;/* /desegnos-pointer:{pointer_id} */");
    assert!(validate_export_reply(&plan, &bad_js).is_err());

    let mut missing_pointer = valid.clone();
    missing_pointer.pointers.clear();
    assert!(parse_export_reply(&plan, &serde_json::to_string(&missing_pointer).unwrap()).is_err());

    let mut extra_pointer = valid.clone();
    extra_pointer.pointers.push(desegnos_core::ExportReplyPointer {
        pointer_id: Uuid::new_v4(),
        implemented: true,
    });
    assert!(parse_export_reply(&plan, &serde_json::to_string(&extra_pointer).unwrap()).is_err());
}

// ---------------------------------------------------------------------------
// Export records stay out of undo history
// ---------------------------------------------------------------------------

#[test]
fn recording_an_export_does_not_add_an_undo_step_and_survives_undo() {
    let (project, pointer_id) = webpage_project();
    let page = project.home_page_id;
    let mut session = Session::new(project).unwrap();
    let record = ExportRecord {
        id: Uuid::new_v4(),
        project_id: session.project().id,
        version: 4,
        destination: "/tmp/desegnos-export".into(),
        behaviours: BTreeMap::new(),
        exported_at: "2026-10-27T00:00:00Z".into(),
        pages: vec![ExportedPage {
            page_id: page,
            path: "/".into(),
            output: OutputType::Webpage,
        }],
        pointers: BTreeMap::from([(
            pointer_id,
            ExportedPointer {
                fingerprint: "c".repeat(64),
                comment: "Opens this boundary".into(),
            },
        )]),
    };
    let before = session.history().2;
    session.record_export(record, |_| Ok(())).unwrap();
    assert_eq!(session.history().2, before, "no history step was added");
    assert!(!session.can_undo());
    assert_eq!(session.project().last_export.as_ref().unwrap().version, 4);
    let wired = session
        .project()
        .pointers
        .iter()
        .find(|pointer| pointer.id == pointer_id)
        .unwrap();
    assert_eq!(
        wired.wiring.as_ref().unwrap().fingerprint,
        "c".repeat(64)
    );

    // A real edit can be undone; the export metadata is carried forward.
    let revision = session.project().revision;
    session
        .execute(
            revision,
            Command::RenameProject {
                name: "Renamed".into(),
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(session.history().2, before + 1);
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().name, "Export");
    assert_eq!(session.project().last_export.as_ref().unwrap().version, 4);
    assert!(session.project().pointers[0].wiring.is_some());
}

// ---------------------------------------------------------------------------
// Direct slide and image export
// ---------------------------------------------------------------------------

fn direct_project() -> Project {
    let project = project();
    let home = project.home_page_id;
    let project = apply(
        &project,
        Command::SetPageOutput {
            page_id: home,
            output: OutputType::Slide,
        },
    );
    let project = apply(
        &project,
        Command::InsertElements {
            page_id: home,
            parent_id: None,
            index: 0,
            elements: vec![element(Uuid::new_v4(), ElementKind::Text, rect(80.0, 80.0, 400.0, 60.0))],
            picture_data_uri: None,
            size_id: None,
        },
    );
    let project = apply(
        &project,
        Command::AddCanvasPage {
            name: "Image".into(),
            path: "/hero".into(),
            size: PageSize {
                id: Uuid::new_v4(),
                label: "Desktop".into(),
                width: 640,
                height: 480,
                kind: BoundaryKind::Page,
                x: 2000.0,
                y: 0.0,
            },
            moves: vec![],
            reference: vec![],
        },
    );
    let hero = project.pages.iter().find(|page| page.path == "/hero").unwrap().id;
    let project = apply(
        &project,
        Command::SetPageOutput {
            page_id: hero,
            output: OutputType::Image,
        },
    );
    apply(
        &project,
        Command::InsertElements {
            page_id: hero,
            parent_id: None,
            index: 0,
            elements: vec![
                element(Uuid::new_v4(), ElementKind::Box, rect(40.0, 40.0, 240.0, 160.0)),
                element(Uuid::new_v4(), ElementKind::Text, rect(60.0, 60.0, 200.0, 40.0)),
            ],
            picture_data_uri: None,
            size_id: None,
        },
    )
}

#[test]
fn slides_render_a_valid_pdf_and_images_render_png_and_svg() {
    let project = direct_project();
    let bundle = direct_export_bundle(&project, &[]).unwrap();
    assert_eq!(bundle.page_count(), 2);

    let pdf = bundle.files.get("slides.pdf").expect("a slide deck");
    assert!(pdf.starts_with(b"%PDF-1."));
    assert!(pdf.windows(6).any(|window| window == b"%%EOF\n"));
    assert!(
        pdf.windows(b"/Type /Page".len())
            .any(|window| window == b"/Type /Page")
    );

    let png = bundle.files.get("image.png").expect("a PNG image");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(&png[12..16], b"IHDR");
    assert!(png.ends_with(b"IEND\xaeB`\x82"));

    let svg = bundle.files.get("image.svg").expect("a vector image");
    let svg = std::str::from_utf8(svg).unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.contains("width=\"640\""));
    assert!(svg.contains("height=\"480\""));
}

#[test]
fn image_export_names_use_page_names_and_remain_unique() {
    let mut project = direct_project();
    for page in &mut project.pages {
        page.output = OutputType::Image;
        page.name = "My Hero".into();
    }
    let plan = plan_export(&project, &[], Uuid::new_v4()).unwrap();
    assert_eq!(
        plan.pages
            .iter()
            .map(|page| page.file.as_str())
            .collect::<Vec<_>>(),
        vec!["my-hero.png", "my-hero-2.png"]
    );
    let bundle = direct_export_bundle(&project, &[]).unwrap();
    for name in [
        "my-hero.png",
        "my-hero-2.png",
        "my-hero.svg",
        "my-hero-2.svg",
    ] {
        assert!(bundle.files.contains_key(name), "missing {name}");
    }
    project.pages[0].output = OutputType::Webpage;
    project.pages[1].output = OutputType::Webpage;
    let plan = plan_export(&project, &[], Uuid::new_v4()).unwrap();
    assert_eq!(
        plan.pages
            .iter()
            .map(|page| page.file.as_str())
            .collect::<Vec<_>>(),
        vec!["index.html", "hero.html"]
    );
}

// ---------------------------------------------------------------------------
// Website capture conversion
// ---------------------------------------------------------------------------

fn styles(background: &str) -> WebStyles {
    WebStyles {
        background: background.into(),
        color: "#111111".into(),
        font_size: 20.0,
        ..WebStyles::default()
    }
}

fn node(tag: &str, rect: Rect, styles: WebStyles, children: Vec<WebNode>) -> WebNode {
    WebNode {
        tag: tag.into(),
        rect,
        text: None,
        src: None,
        poster: None,
        href: None,
        input_type: None,
        placeholder: None,
        value: None,
        alt: None,
        options: vec![],
        attributes: Default::default(),
        styles,
        children,
    }
}

#[test]
fn website_captures_become_boundaries_elements_script_notes_and_pointers() {
    let project = project();
    let mut heading = node("h1", rect(40.0, 40.0, 400.0, 48.0), styles(""), vec![]);
    heading.text = Some("Hello imported site".into());
    let mut menu = node("button", rect(40.0, 120.0, 160.0, 40.0), styles("#eeeeee"), vec![]);
    menu.text = Some("Menu".into());
    menu.attributes.class_name = "menu-toggle".into();
    let mut disabled = node("input", rect(40.0, 200.0, 200.0, 40.0), styles(""), vec![]);
    disabled.input_type = Some("text".into());
    disabled.placeholder = Some("Email".into());
    let container = node(
        "div",
        rect(0.0, 0.0, 800.0, 600.0),
        styles("#ffffff"),
        vec![heading, menu, disabled],
    );
    let capture = WebCapture {
        url: "https://example.test/".into(),
        site: "Example".into(),
        route: "/".into(),
        title: "Example site".into(),
        width: 1440.0,
        height: 900.0,
        nodes: vec![container],
        scripts: vec![WebScript {
            src: Some("app.js".into()),
            text: None,
        }],
    };

    let imported = import_website(&[capture], &BTreeMap::new(), &project).unwrap();
    assert_eq!(imported.pages.len(), 1);
    let page = &imported.pages[0];
    assert_eq!(page.path, "/example");
    assert_eq!(page.output, OutputType::Webpage);
    assert_eq!(page.sizes.len(), 1);
    assert_eq!(page.sizes[0].width, 1440);
    assert!(!page.elements.is_empty());
    let boxed = page
        .elements
        .iter()
        .find(|element| element.kind == ElementKind::Box)
        .expect("the visible container stays a box");
    assert!(
        boxed
            .children
            .iter()
            .any(|element| element.kind == ElementKind::Text && element.text == "Hello imported site")
    );
    let button = boxed
        .children
        .iter()
        .find(|element| element.kind == ElementKind::Button && element.text == "Menu")
        .expect("a button becomes a button element");
    assert!(boxed.children.iter().any(|element| {
        matches!(
            element.kind,
            ElementKind::Input {
                input_type: InputType::Text
            }
        ) && element.text == "Email"
    }));

    assert_eq!(imported.reference.len(), 1);
    assert!(matches!(
        &imported.reference[0].kind,
        ReferenceKind::Note { text } if text == "Script: app.js"
    ));
    assert_eq!(imported.pointers.len(), 1);
    assert_eq!(imported.pointers[0].comment, "Toggles the navigation menu.");
    assert!(matches!(
        imported.pointers[0].from,
        PointerSource::Element { element_id, .. } if element_id == button.id
    ));
    assert!(matches!(
        imported.pointers[0].to,
        PointerTarget::Boundary { .. }
    ));

    // The conversion is pure and stable: importing the same capture gives the same page and
    // element ids.
    let again = import_website(&[import_website_capture()], &BTreeMap::new(), &project).unwrap();
    assert_eq!(again.pages[0].path, "/example");
    assert_eq!(again.pages[0].id, page.id);
    assert_eq!(again.pages[0].elements[0].id, page.elements[0].id);
}

fn import_website_capture() -> WebCapture {
    WebCapture {
        url: "https://example.test/".into(),
        site: "Example".into(),
        route: "/".into(),
        title: "Example site".into(),
        width: 1440.0,
        height: 900.0,
        nodes: vec![node(
            "div",
            rect(0.0, 0.0, 800.0, 600.0),
            styles("#ffffff"),
            vec![{
                let mut heading = node("h1", rect(40.0, 40.0, 400.0, 48.0), styles(""), vec![]);
                heading.text = Some("Hello imported site".into());
                heading
            }],
        )],
        scripts: vec![],
    }
}

// ---------------------------------------------------------------------------
// Import applied as one undo step
// ---------------------------------------------------------------------------

#[test]
fn importing_a_website_command_is_one_undo_step() {
    let project = project();
    let capture = import_website_capture();
    let imported = import_website(&[capture], &BTreeMap::new(), &project).unwrap();
    let mut session = Session::new(project).unwrap();
    let before = session.history().2;
    session
        .execute(
            session.project().revision,
            Command::ImportWebsite {
                pages: imported.pages,
                reference: imported.reference,
                pointers: imported.pointers,
                pictures: BTreeMap::new(),
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(session.history().2, before + 1);
    assert_eq!(session.project().pages.len(), 2);
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages.len(), 1);
}

#[test]
fn phone_captures_never_give_text_a_layout_override() {
    let project = project();
    let capture = |width: f64| {
        let mut text = node(
            "p",
            rect(20.0, 20.0, width - 40.0, 40.0),
            styles(""),
            vec![],
        );
        text.text = Some("Plain paragraph".into());
        WebCapture {
            url: "https://example.test/".into(),
            site: "Example".into(),
            route: "/".into(),
            title: "Example site".into(),
            width,
            height: 900.0,
            nodes: vec![node(
                "body",
                rect(0.0, 0.0, width, 900.0),
                styles("#ffffff"),
                vec![text],
            )],
            scripts: vec![],
        }
    };
    let imported = import_website(
        &[capture(1440.0), capture(390.0)],
        &BTreeMap::new(),
        &project,
    )
    .unwrap();
    let page = &imported.pages[0];
    assert_eq!(page.sizes.len(), 2, "the phone capture adds a phone size");
    fn check(elements: &[desegnos_core::Element]) -> usize {
        elements
            .iter()
            .map(|element| {
                if !element.kind.holds_children() {
                    assert!(
                        element
                            .overrides
                            .values()
                            .all(|value| value.layout.is_none()),
                        "{:?} carries a layout override",
                        element.kind
                    );
                }
                usize::from(!element.overrides.is_empty()) + check(&element.children)
            })
            .sum()
    }
    assert!(
        check(&page.elements) > 0,
        "the phone size is stored as overrides"
    );
}

#[test]
fn links_to_page_files_point_at_the_imported_pages() {
    let project = project();
    let page = |url: &str, route: &str, href: &str| {
        let mut link = node("a", rect(20.0, 20.0, 120.0, 30.0), styles(""), vec![]);
        link.text = Some("Go".into());
        link.href = Some(href.into());
        WebCapture {
            url: url.into(),
            site: "Site".into(),
            route: route.into(),
            title: route.into(),
            width: 1440.0,
            height: 900.0,
            nodes: vec![node("body", rect(0.0, 0.0, 1440.0, 900.0), styles("#ffffff"), vec![link])],
            scripts: vec![],
        }
    };
    let home = page("http://127.0.0.1:9/t/", "/", "http://127.0.0.1:9/t/about.html");
    let about = page("http://127.0.0.1:9/t/about", "/about", "http://127.0.0.1:9/t/index.html");
    let imported = import_website(&[home, about], &BTreeMap::new(), &project).unwrap();
    fn link(elements: &[desegnos_core::Element]) -> Option<&desegnos_core::Element> {
        elements.iter().find_map(|element| (element.kind == ElementKind::Link).then_some(element).or_else(|| link(&element.children)))
    }
    let to_about = link(&imported.pages[0].elements).expect("the home page keeps its link");
    let to_home = link(&imported.pages[1].elements).expect("the about page keeps its link");
    assert_eq!(to_about.url.as_deref(), Some(imported.pages[1].path.as_str()));
    assert_eq!(to_home.url.as_deref(), Some(imported.pages[0].path.as_str()));
}

#[test]
fn old_site_links_on_the_original_domain_find_the_imported_pages() {
    let project = project();
    let capture = |url: &str, route: &str, href: Option<&str>| {
        let mut link = node("a", rect(20.0, 20.0, 120.0, 30.0), styles(""), vec![]);
        link.text = Some("Go".into());
        link.href = href.map(str::to_owned);
        WebCapture {
            url: url.into(),
            site: "Site".into(),
            route: route.into(),
            title: route.into(),
            width: 1440.0,
            height: 900.0,
            nodes: vec![node("body", rect(0.0, 0.0, 1440.0, 900.0), styles("#ffffff"), vec![link])],
            scripts: vec![],
        }
    };
    let home = capture(
        "http://127.0.0.1:9/token/",
        "/",
        Some("http://www.oldsite.example/pages/P002.HTM"),
    );
    let main = capture("http://127.0.0.1:9/token/main.htm", "/main.htm", None);
    let p002 = capture("http://127.0.0.1:9/token/pages/p002", "/pages/p002", Some("../MAIN.HTM"));
    let imported = import_website(&[home, main, p002], &BTreeMap::new(), &project).unwrap();
    fn link(elements: &[desegnos_core::Element]) -> Option<&desegnos_core::Element> {
        elements.iter().find_map(|element| {
            (element.kind == ElementKind::Link)
                .then_some(element)
                .or_else(|| link(&element.children))
        })
    }
    let to_p002 = link(&imported.pages[0].elements).expect("the home link is kept");
    let to_main = link(&imported.pages[2].elements).expect("the page link is kept");
    assert_eq!(to_p002.url.as_deref(), Some(imported.pages[2].path.as_str()));
    assert_eq!(to_main.url.as_deref(), Some(imported.pages[1].path.as_str()));
}

#[test]
fn public_import_links_keep_pointing_at_the_original_site() {
    let project = project();
    let capture = |url: &str, route: &str, href: &str| {
        let mut link = node("a", rect(20.0, 20.0, 120.0, 30.0), styles(""), vec![]);
        link.text = Some("Go".into());
        link.href = Some(href.into());
        WebCapture {
            url: url.into(),
            site: "Site".into(),
            route: route.into(),
            title: route.into(),
            width: 1440.0,
            height: 900.0,
            nodes: vec![node("body", rect(0.0, 0.0, 1440.0, 900.0), styles("#ffffff"), vec![link])],
            scripts: vec![],
        }
    };
    let href = "https://www.oldsite.example/pages/P002.HTM";
    let home = capture("https://oldsite.example/pages/p001", "/pages/p001", href);
    let p002 = capture("https://oldsite.example/pages/p002", "/pages/p002", "https://oldsite.example/");
    let imported = import_website(&[home, p002], &BTreeMap::new(), &project).unwrap();
    fn link(elements: &[desegnos_core::Element]) -> Option<&desegnos_core::Element> {
        elements.iter().find_map(|element| {
            (element.kind == ElementKind::Link)
                .then_some(element)
                .or_else(|| link(&element.children))
        })
    }
    let link = link(&imported.pages[0].elements).expect("the home link is kept");
    assert_eq!(link.url.as_deref(), Some(href));
}

#[test]
fn a_matching_path_on_another_domain_stays_external() {
    let project = project();
    let capture = |url: &str, route: &str, href: Option<&str>| {
        let mut link = node("a", rect(20.0, 20.0, 120.0, 30.0), styles(""), vec![]);
        link.text = Some("Go".into());
        link.href = href.map(str::to_owned);
        WebCapture {
            url: url.into(),
            site: "Site".into(),
            route: route.into(),
            title: route.into(),
            width: 1440.0,
            height: 900.0,
            nodes: vec![node("body", rect(0.0, 0.0, 1440.0, 900.0), styles("#ffffff"), vec![link])],
            scripts: vec![],
        }
    };
    let home = capture("http://127.0.0.1:9/token/", "/", Some("https://social.example/about"));
    let about = capture("http://127.0.0.1:9/token/about", "/about", None);
    let imported = import_website(&[home, about], &BTreeMap::new(), &project).unwrap();
    fn link(elements: &[desegnos_core::Element]) -> Option<&desegnos_core::Element> {
        elements.iter().find_map(|element| (element.kind == ElementKind::Link).then_some(element).or_else(|| link(&element.children)))
    }
    let social = link(&imported.pages[0].elements).expect("the social link is kept");
    assert_eq!(social.url.as_deref(), Some("https://social.example/about"));
}

#[test]
fn table_cells_keep_their_background_and_place_their_content() {
    let project = project();
    let mut link = node("a", rect(18.0, 60.0, 40.0, 16.0), styles(""), vec![]);
    link.text = Some("Home".into());
    let nav = node("td", rect(10.0, 50.0, 160.0, 200.0), styles("#ccccff"), vec![link]);
    let mut words = node("p", rect(180.0, 60.0, 500.0, 40.0), styles(""), vec![]);
    words.text = Some("Article text".into());
    let body = node("td", rect(170.0, 50.0, 600.0, 200.0), styles(""), vec![words]);
    let row = node("tr", rect(10.0, 50.0, 760.0, 200.0), styles(""), vec![nav, body]);
    let table = node("table", rect(10.0, 50.0, 760.0, 200.0), styles("#ffffff"), vec![row]);
    let capture = WebCapture {
        url: "http://127.0.0.1:9/token/".into(),
        site: "Old".into(),
        route: "/".into(),
        title: "Old".into(),
        width: 1440.0,
        height: 900.0,
        nodes: vec![node("body", rect(0.0, 0.0, 1440.0, 900.0), styles(""), vec![table])],
        scripts: vec![],
    };
    let imported = import_website(&[capture], &BTreeMap::new(), &project).unwrap();
    fn all(elements: &[desegnos_core::Element], origin: (f64, f64), out: &mut Vec<(desegnos_core::Element, f64, f64)>) {
        for element in elements {
            let at = (origin.0 + element.rect.x, origin.1 + element.rect.y);
            out.push((element.clone(), at.0, at.1));
            all(&element.children, at, out);
        }
    }
    let mut placed = Vec::new();
    all(&imported.pages[0].elements, (0.0, 0.0), &mut placed);
    assert!(placed.iter().any(|(element, ..)| element.style.fill.as_deref() == Some("#ccccff")), "the nav cell keeps its background");
    let text = placed.iter().find(|(element, ..)| element.text == "Article text").unwrap();
    assert_eq!((text.1, text.2), (180.0, 60.0), "the second cell's text keeps its page position");
    assert_eq!(text.0.style.sizing.width, desegnos_core::SizingMode::Fit, "a single line hugs its text");
}
