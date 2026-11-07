use desegnos_core::*;
use serde_json::json;
use uuid::Uuid;
fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}
fn project() -> (Project, Uuid) {
    let mut p = Project::new(
        "Responsive".into(),
        Viewport {
            width: 1440,
            height: 300,
        },
        Appearance::Light,
    )
    .unwrap();
    let id = Uuid::new_v4();
    let mut size = p.pages[0].base_size().clone();
    size.id = id;
    size.width = 390;
    size.height = 300;
    p.pages[0].sizes.push(size);
    (p, id)
}
fn text(x: f64, y: f64, w: f64, h: f64) -> Element {
    let mut e = empty_element(ElementKind::Text, rect(x, y, w, h));
    e.text = "Readable text wraps onto several lines when the screen becomes narrow".into();
    e.style.text_style.size = 16.0;
    e
}
#[test]
fn overflowing_row_stacks_and_grows_parent_and_page() {
    let (mut p, id) = project();
    p.pages[0].size_mut(id).unwrap().height = 20;
    let mut parent = empty_element(ElementKind::Group, rect(0.0, 0.0, 1440.0, 100.0));
    parent.children = vec![
        text(20.0, 0.0, 600.0, 100.0),
        text(700.0, 0.0, 600.0, 100.0),
    ];
    p.pages[0].elements = vec![parent];
    adapt_size(&mut p.pages[0], id).unwrap();
    let e = resolve_for_size(&p, &p.pages[0], id);
    assert!(e[0].children[1].rect.y >= e[0].children[0].rect.y + e[0].children[0].rect.height);
    assert!(e[0].rect.height > 100.0);
    assert!(p.pages[0].size(id).unwrap().height >= e[0].rect.height.ceil() as u32);
    assert!(p.pages[0].size(id).unwrap().height > 20);
}
#[test]
fn fitting_row_stays_horizontal() {
    let (mut p, id) = project();
    let a = empty_element(ElementKind::Image, rect(0.0, 10.0, 80.0, 40.0));
    let b = empty_element(ElementKind::Image, rect(120.0, 10.0, 80.0, 40.0));
    p.pages[0].elements = vec![a, b];
    adapt_size(&mut p.pages[0], id).unwrap();
    let e = resolve_for_size(&p, &p.pages[0], id);
    assert_eq!(e[0].rect.y, e[1].rect.y);
    assert!(e[1].rect.x >= e[0].rect.x + e[0].rect.width);
}
#[test]
fn preserves_manual_override_on_readapt() {
    let (mut p, id) = project();
    let mut e = text(10.0, 10.0, 800.0, 50.0);
    let over = ElementOverride {
        rect: Some(rect(22.0, 44.0, 150.0, 60.0)),
        ..Default::default()
    };
    e.overrides.insert(id, over.clone());
    p.pages[0].elements = vec![e];
    adapt_size(&mut p.pages[0], id).unwrap();
    adapt_size(&mut p.pages[0], id).unwrap();
    assert_eq!(p.pages[0].elements[0].overrides[&id], over);
}
#[test]
fn editing_size_clears_automatic_and_undo_restores_it() {
    let (mut p, id) = project();
    p.pages[0].elements = vec![text(0.0, 0.0, 600.0, 100.0)];
    adapt_size(&mut p.pages[0], id).unwrap();
    let eid = p.pages[0].elements[0].id;
    let page_id = p.pages[0].id;
    let patch =
        serde_json::from_value(json!({"id":eid,"size_id":id,"rect":rect(20.0,30.0,200.0,100.0)}))
            .unwrap();
    let mut session = Session::new(p.clone()).unwrap();
    session
        .execute(
            p.revision,
            Command::UpdateElements {
                page_id,
                patches: vec![patch],
            },
            |_| Ok(()),
        )
        .unwrap();
    assert!(!session.project().pages[0].elements[0].overrides[&id].automatic);
    session.undo(|_| Ok(())).unwrap();
    assert!(session.project().pages[0].elements[0].overrides[&id].automatic);
}
#[test]
fn body_text_is_readable_and_repeated_adaptation_converges() {
    let (mut p, id) = project();
    p.pages[0].elements = vec![text(0.0, 0.0, 1000.0, 24.0)];
    adapt_size(&mut p.pages[0], id).unwrap();
    let e = resolve_for_size(&p, &p.pages[0], id);
    assert!(e[0].style.text_style.size >= 14.0);
    assert!(e[0].rect.height > 24.0);
    let before = p.clone();
    adapt_size(&mut p.pages[0], id).unwrap();
    assert_eq!(p, before);
}
#[test]
fn stack_rows_turn_into_columns_and_grids_drop_columns() {
    let (mut p, id) = project();
    let mut parent = empty_element(ElementKind::Box, rect(0.0, 0.0, 1440.0, 100.0));
    parent.children = vec![text(0.0, 0.0, 400.0, 100.0), text(400.0, 0.0, 400.0, 100.0)];
    parent.layout = ElementLayout::Stack {
        direction: StackDirection::Row,
        gap: 20.0,
        padding: 20.0,
        align: StackAlign::Start,
        justify: StackJustify::Start,
        wrap: false,
    };
    p.pages[0].elements = vec![parent];
    adapt_size(&mut p.pages[0], id).unwrap();
    let e = resolve_for_size(&p, &p.pages[0], id);
    assert!(matches!(
        e[0].layout,
        ElementLayout::Stack {
            direction: StackDirection::Column,
            ..
        }
    ));
    assert!(e[0].children[1].rect.y >= e[0].children[0].rect.y + e[0].children[0].rect.height);
    p.pages[0].elements[0].layout = ElementLayout::Grid {
        columns: 3,
        gap: 20.0,
        padding: 20.0,
    };
    adapt_size(&mut p.pages[0], id).unwrap();
    let e = resolve_for_size(&p, &p.pages[0], id);
    assert!(matches!(
        e[0].layout,
        ElementLayout::Grid { columns: 1, .. }
    ));
}
#[test]
fn base_edits_adapt_in_one_undo_step() {
    let (mut p, id) = project();
    p.pages[0].elements = vec![text(0.0, 0.0, 1000.0, 24.0)];
    let page_id = p.pages[0].id;
    let eid = p.pages[0].elements[0].id;
    let revision = p.revision;
    let mut session = Session::new(p.clone()).unwrap();
    let patch = serde_json::from_value(json!({"id":eid,"rect":rect(0.0,0.0,1200.0,24.0)})).unwrap();
    session
        .execute(
            revision,
            Command::UpdateElements {
                page_id,
                patches: vec![patch],
            },
            |_| Ok(()),
        )
        .unwrap();
    assert!(session.project().pages[0].elements[0].overrides[&id].automatic);
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, p.pages);
}
#[test]
fn backgrounds_do_not_join_rows_and_images_keep_aspect_ratio() {
    let (mut p, id) = project();
    p.pages[0].elements = vec![
        empty_element(ElementKind::Box, rect(0.0, 0.0, 1440.0, 300.0)),
        empty_element(ElementKind::Image, rect(0.0, 20.0, 1000.0, 500.0)),
        text(0.0, 550.0, 800.0, 40.0),
    ];
    adapt_size(&mut p.pages[0], id).unwrap();
    let e = resolve_for_size(&p, &p.pages[0], id);
    assert_eq!(e[0].rect.width, 390.0);
    assert_eq!(e[1].rect.width / e[1].rect.height, 2.0);
    assert!(e[2].rect.y >= e[1].rect.y + e[1].rect.height);
    assert!(p.pages[0].size(id).unwrap().height >= e[2].rect.y.ceil() as u32);
}
#[test]
fn shared_text_styles_are_measured_and_updates_readapt() {
    let (mut p, id) = project();
    let style = TextStyle {
        id: Uuid::new_v4(),
        name: "Body".into(),
        font_family: "Archivo".into(),
        size: 20.0,
        weight: 400,
        line_height: 1.4,
        letter_spacing: 0.0,
        italic: false,
    };
    let mut e = text(0.0, 0.0, 300.0, 30.0);
    e.style.references.text = Some(style.id);
    p.pages[0].elements = vec![e];
    p.styles.text.push(style.clone());
    p = p
        .apply(
            p.revision,
            Command::AdaptSize {
                page_id: p.pages[0].id,
                size_id: id,
            },
        )
        .unwrap();
    let first = resolve_for_size(&p, &p.pages[0], id)[0]
        .style
        .text_style
        .size;
    assert_eq!(first, 20.0);
    let mut style = style;
    style.size = 24.0;
    p = p
        .apply(p.revision, Command::UpdateTextStyle { style })
        .unwrap();
    assert_eq!(
        resolve_for_size(&p, &p.pages[0], id)[0]
            .style
            .text_style
            .size,
        24.0
    );
}
