//! Golden fixture for the canvas's TypeScript copy of linked-element, size-override and shared-style
//! resolution. `tests/resolution.test.mjs` resolves the same project with the TypeScript code and
//! compares. Regenerate with `npm run generate:contract` (sets `UPDATE_GENERATED`).
use desegnos_core::{
    Appearance, CanvasTemplate, ColourStyle, Element, ElementKind, ElementLayout, ElementOverride,
    LinkedDefinition, Project, Rect, TextStyle, empty_element, resolve_for_size,
};
use serde_json::json;
use std::{fs, path::PathBuf};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn element(value: u128, kind: ElementKind, rect: Rect) -> Element {
    let mut element = empty_element(kind, rect);
    element.id = id(value);
    element
}

fn fixture_project() -> Project {
    let mut project = Project::untitled(
        "Resolution".into(),
        Appearance::Light,
        CanvasTemplate::Website,
    )
    .unwrap();
    let page = &mut project.pages[0];
    page.id = id(0x100);
    project.home_page_id = id(0x100);
    project.id = id(0x1);
    for (index, size) in page.sizes.iter_mut().enumerate() {
        size.id = id(0x200 + index as u128);
    }
    let (desktop, tablet, phone) = (id(0x200), id(0x201), id(0x202));

    project.styles.colours = vec![
        ColourStyle {
            id: id(0x300),
            name: "Brand".into(),
            value: "#112233".into(),
        },
        ColourStyle {
            id: id(0x301),
            name: "Ink".into(),
            value: "#445566".into(),
        },
    ];
    project.styles.text = vec![TextStyle {
        id: id(0x310),
        name: "Heading".into(),
        font_family: "Georgia".into(),
        size: 40.0,
        weight: 700,
        line_height: 1.1,
        letter_spacing: 0.5,
        italic: true,
    }];

    let mut heading = element(0x1, ElementKind::Text, rect(10.0, 20.0, 300.0, 60.0));
    heading.text = "Heading".into();
    heading.style.references.text = Some(id(0x310));
    heading.style.references.text_colour = Some(id(0x301));
    heading.overrides.insert(
        tablet,
        ElementOverride {
            rect: Some(rect(5.0, 6.0, 200.0, 50.0)),
            ..Default::default()
        },
    );

    let mut card = element(0x2, ElementKind::Box, rect(0.0, 100.0, 400.0, 200.0));
    card.style.references.fill = Some(id(0x300));
    card.style.references.stroke = Some(id(0x301));
    let mut tablet_style = card.style.clone();
    tablet_style.fill = Some("#ffffff".into());
    tablet_style.radius = 12.0;
    card.overrides.insert(
        tablet,
        ElementOverride {
            style: Some(tablet_style),
            ..Default::default()
        },
    );
    let mut note = element(0x3, ElementKind::Text, rect(4.0, 4.0, 100.0, 20.0));
    note.text = "Note".into();
    note.overrides.insert(
        phone,
        ElementOverride {
            hidden: true,
            ..Default::default()
        },
    );
    let grid = ElementLayout::Grid {
        columns: 2,
        gap: 8.0,
        padding: 24.0,
    };
    note.overrides.insert(
        tablet,
        ElementOverride {
            layout: Some(grid),
            ..Default::default()
        },
    );
    card.children.push(note);

    let mut master = element(0x10, ElementKind::Box, rect(0.0, 0.0, 120.0, 80.0));
    master.name = Some("Master".into());
    master.style.references.fill = Some(id(0x300));
    let mut label = element(0x11, ElementKind::Text, rect(8.0, 8.0, 80.0, 20.0));
    label.text = "Shared".into();
    master.children.push(label);
    project.linked.insert(
        id(0x400),
        LinkedDefinition {
            id: id(0x400),
            name: "Badge".into(),
            master,
        },
    );
    let mut first = element(0x20, ElementKind::Box, rect(30.0, 400.0, 10.0, 10.0));
    first.link = Some(id(0x400));
    first.name = Some("First badge".into());
    let mut second = element(0x21, ElementKind::Box, rect(200.0, 410.0, 10.0, 10.0));
    second.link = Some(id(0x400));
    second.visible = false;
    second.overrides.insert(
        phone,
        ElementOverride {
            rect: Some(rect(1.0, 2.0, 60.0, 40.0)),
            ..Default::default()
        },
    );

    project.pages[0].elements = vec![heading, card, first, second];
    let _ = desktop;
    project
}

fn golden() -> String {
    let project = fixture_project();
    let page = &project.pages[0];
    let cases: Vec<_> = page
        .sizes
        .iter()
        .map(|size| {
            json!({
                "pageId": page.id,
                "sizeId": size.id,
                "elements": resolve_for_size(&project, page, size.id),
            })
        })
        .collect();
    let text =
        serde_json::to_string_pretty(&json!({ "project": project, "cases": cases })).unwrap();
    format!("{text}\n")
}

#[test]
fn canvas_resolution_fixture_is_current() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/resolution.json");
    let text = golden();
    if fs::read_to_string(&path).ok().as_deref() == Some(&text) {
        return;
    }
    assert!(
        std::env::var_os("UPDATE_GENERATED").is_some(),
        "tests/fixtures/resolution.json is stale; run `npm run generate:contract`"
    );
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
}
