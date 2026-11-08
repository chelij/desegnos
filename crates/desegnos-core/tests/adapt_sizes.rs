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
        )
        .unwrap();
    assert!(!session.project().pages[0].elements[0].overrides[&id].automatic);
    session.undo().unwrap();
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
        )
        .unwrap();
    assert!(session.project().pages[0].elements[0].overrides[&id].automatic);
    session.undo().unwrap();
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

#[test]
fn inline_rows_wrap_only_when_the_next_item_does_not_fit() {
    let (mut p, id) = project();
    let widths = [180.0, 60.0, 80.0, 80.0, 170.0];
    let positions = [48.0, 900.0, 980.0, 1080.0, 1190.0];
    p.pages[0].elements = widths
        .into_iter()
        .zip(positions)
        .map(|(w, x)| {
            let mut e = empty_element(ElementKind::Link, rect(x, 20.0, w, 40.0));
            e.text = "Link".into();
            e
        })
        .collect();
    adapt_size(&mut p.pages[0], id).unwrap();
    let phone = resolve_for_size(&p, &p.pages[0], id);
    assert_eq!(phone[0].rect.y, phone[2].rect.y);
    assert!(phone[3].rect.y >= phone[0].rect.y + 40.0);
    assert_eq!(phone[3].rect.y, phone[4].rect.y);
    p.pages[0].size_mut(id).unwrap().width = 820;
    adapt_size(&mut p.pages[0], id).unwrap();
    let tablet = resolve_for_size(&p, &p.pages[0], id);
    assert!(tablet.iter().all(|e| e.rect.y == tablet[0].rect.y));
}

#[test]
fn repeated_tiles_use_two_phone_columns_and_four_tablet_columns() {
    let (mut p, id) = project();
    let mut parent = empty_element(ElementKind::Group, rect(0.0, 0.0, 620.0, 110.0));
    parent.children = (0..4)
        .map(|i| {
            let mut tile = empty_element(
                ElementKind::Group,
                rect(f64::from(i) * 160.0, 0.0, 140.0, 110.0),
            );
            let mut label = text(12.0, 12.0, 116.0, 40.0);
            label.text = "12".into();
            tile.children = vec![
                empty_element(ElementKind::Box, rect(0.0, 0.0, 140.0, 110.0)),
                label,
            ];
            tile
        })
        .collect();
    p.pages[0].elements = vec![parent];
    adapt_size(&mut p.pages[0], id).unwrap();
    let phone = resolve_for_size(&p, &p.pages[0], id);
    let tiles = &phone[0].children;
    assert_eq!(tiles[0].rect.y, tiles[1].rect.y);
    assert!(tiles[2].rect.y >= tiles[0].rect.y + tiles[0].rect.height);
    assert_eq!(tiles[2].rect.y, tiles[3].rect.y);
    assert_eq!(tiles[0].rect.width, tiles[3].rect.width);
    for tile in tiles {
        assert_eq!(tile.children[0].rect.width, tile.rect.width);
        assert!(tile.children[0].rect.height <= tile.rect.height);
    }
    p.pages[0].size_mut(id).unwrap().width = 820;
    adapt_size(&mut p.pages[0], id).unwrap();
    let tablet = resolve_for_size(&p, &p.pages[0], id);
    assert!(
        tablet[0]
            .children
            .iter()
            .all(|e| e.rect.y == tablet[0].children[0].rect.y)
    );
}

#[test]
fn wrapped_price_rows_keep_the_parent_content_inset() {
    let (mut p, id) = project();
    let mut card = empty_element(ElementKind::Group, rect(0.0, 0.0, 380.0, 400.0));
    let mut title = text(32.0, 32.0, 316.0, 20.0);
    title.text = "GA PASS".into();
    let mut price = text(32.0, 70.0, 200.0, 70.0);
    price.text = "$39".into();
    price.style.text_style.size = 54.0;
    let mut suffix = text(196.0, 106.0, 100.0, 20.0);
    suffix.text = "/ MO".into();
    let mut body = text(32.0, 150.0, 316.0, 40.0);
    body.text = "Every stage".into();
    card.children = vec![title, price, suffix, body];
    p.pages[0].elements = vec![card];
    adapt_size(&mut p.pages[0], id).unwrap();
    let tree = resolve_for_size(&p, &p.pages[0], id);
    let children = &tree[0].children;
    assert_eq!(children[0].rect.x, children[1].rect.x);
    assert_eq!(children[0].rect.x, children[3].rect.x);
    assert!(children[1].rect.x >= 20.0);
    assert!(children[2].rect.x >= children[0].rect.x);
}

#[test]
fn field_labels_wrap_with_their_controls() {
    let (mut p, id) = project();
    let mut parent = empty_element(ElementKind::Group, rect(0.0, 0.0, 1440.0, 300.0));
    for x in [80.0, 520.0, 960.0] {
        let mut label = text(x, 130.0, 200.0, 18.0);
        label.text = "NAME".into();
        parent.children.push(label);
    }
    for x in [80.0, 520.0, 960.0] {
        parent.children.push(empty_element(
            ElementKind::Input {
                input_type: InputType::Text,
            },
            rect(x, 150.0, 400.0, 52.0),
        ));
    }
    p.pages[0].elements = vec![parent];
    for width in [390, 820] {
        p.pages[0].size_mut(id).unwrap().width = width;
        adapt_size(&mut p.pages[0], id).unwrap();
        let tree = resolve_for_size(&p, &p.pages[0], id);
        let fields = &tree[0].children;
        for i in 0..3 {
            assert_eq!(fields[i].rect.x, fields[i + 3].rect.x);
            assert_eq!(
                fields[i + 3].rect.y,
                fields[i].rect.y + fields[i].rect.height + 8.0
            );
        }
        if width == 390 {
            assert!(fields[1].rect.y > fields[3].rect.y);
        } else {
            assert_eq!(fields[0].rect.y, fields[2].rect.y);
        }
    }
}

#[test]
fn uneven_headliner_cards_share_a_tablet_grid_without_overlapping() {
    let (mut p, id) = project();
    p.pages[0].size_mut(id).unwrap().width = 820;
    let mut parent = empty_element(ElementKind::Group, rect(0.0, 0.0, 1280.0, 340.0));
    for (x, y, w, h) in [
        (0.0, 40.0, 320.0, 260.0),
        (340.0, 0.0, 600.0, 340.0),
        (960.0, 40.0, 320.0, 260.0),
    ] {
        let mut card = empty_element(ElementKind::Group, rect(x, y, w, h));
        let mut title = text(24.0, 24.0, w - 48.0, 40.0);
        title.text = "HEADLINER".into();
        card.children = vec![title];
        parent.children.push(card);
    }
    p.pages[0].elements = vec![parent];
    adapt_size(&mut p.pages[0], id).unwrap();
    let tree = resolve_for_size(&p, &p.pages[0], id);
    let cards = &tree[0].children;
    assert_eq!(cards[0].rect.y, cards[2].rect.y);
    assert_eq!(cards[0].rect.width, cards[1].rect.width);
    for pair in cards.windows(2) {
        assert!(pair[1].rect.x >= pair[0].rect.x + pair[0].rect.width);
    }
}

#[test]
fn adding_a_linked_size_adapts_without_resizing_the_base_in_one_undo_step() {
    let (mut p, phone_id) = project();
    let phone = p.pages[0].sizes.pop().unwrap();
    p.pages[0].elements = vec![text(20.0, 0.0, 1200.0, 24.0)];
    let before = p.pages[0].clone();
    let mut session = Session::new(p.clone()).unwrap();
    session
        .execute(
            p.revision,
            Command::AddPageSize {
                page_id: before.id,
                size: phone,
                moves: vec![],
                reference: vec![],
            },
        )
        .unwrap();
    let page = &session.project().pages[0];
    assert_eq!(page.base_size(), before.base_size());
    assert_eq!(page.elements[0].rect, before.elements[0].rect);
    assert!(page.elements[0].overrides[&phone_id].automatic);
    let phone_elements = resolve_for_size(session.project(), page, phone_id);
    assert!(phone_elements[0].rect.width < before.elements[0].rect.width);
    session.undo().unwrap();
    assert_eq!(session.project().pages[0], before);
}

#[test]
fn letter_by_letter_headline_stays_on_one_line_at_phone() {
    let (mut p, id) = project();
    p.pages[0].elements = "EMBER"
        .chars()
        .enumerate()
        .map(|(i, letter)| {
            let mut e = text(440.0 + i as f64 * 110.0, 230.0, 100.0, 200.0);
            e.text = letter.to_string();
            e.style.text_style.size = 170.0;
            e
        })
        .collect();
    adapt_size(&mut p.pages[0], id).unwrap();
    let phone = resolve_for_size(&p, &p.pages[0], id);
    let width = f64::from(p.pages[0].size(id).unwrap().width);
    assert!(
        phone.iter().all(|e| e.rect.y == phone[0].rect.y),
        "letters wrapped"
    );
    assert!(
        phone.iter().all(|e| e.rect.x + e.rect.width <= width),
        "letters overflow the phone"
    );
    assert!(
        phone
            .iter()
            .all(|e| e.style.text_style.size <= width * 0.16 + 0.01)
    );
}
