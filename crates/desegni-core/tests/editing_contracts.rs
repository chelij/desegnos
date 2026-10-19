mod support;
use desegni_core::*;
use std::collections::BTreeMap;
use support::without_editor_ids;

fn project(html: &str) -> Project {
    let mut p = Project::new(
        "Direct editing".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap();
    p.pages[0].content = Some(SiteContent {
        html: html.into(),
        css: "/* exact stylesheet */\nbutton {color:rebeccapurple}".into(),
        javascript: "// exact script\nconst label = '日本語';".into(),
    });
    p
}
fn selected(p: &Project, path: &str, tag: &str) -> ElementTarget {
    ElementTarget {
        id: String::new(),
        selector: path.into(),
        tag: tag.into(),
        label: "Selected".into(),
        source_sha256: p.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    }
}
fn update() -> ElementEdit {
    ElementEdit::Update {
        texts: vec![],
        attributes: BTreeMap::new(),
        style: None,
    }
}
#[test]
fn direct_text_preserves_nested_markup_other_elements_css_and_javascript() {
    let p = project("<main><button><span>Go</span><em>now</em></button><p>Untouched</p></main>");
    let t = selected(
        &p,
        "body > main:nth-of-type(1) > button:nth-of-type(1)",
        "button",
    );
    let d = p.inspect_element(p.home_page_id, &t).unwrap();
    assert_eq!(d.texts.len(), 2);
    let mut change = update();
    if let ElementEdit::Update { texts, .. } = &mut change {
        texts.push(TextEdit {
            index: d.texts[0].index,
            text: "Create <this> & café".into(),
        });
    }
    let (next, t) = p.edit_element(0, p.home_page_id, &t, &change).unwrap();
    let c = next.pages[0].content.as_ref().unwrap();
    assert!(
        without_editor_ids(&c.html)
            .contains("<span>Create &lt;this&gt; &amp; café</span><em>now</em>")
    );
    assert!(without_editor_ids(&c.html).contains("<p>Untouched</p>"));
    assert_eq!(c.css, p.pages[0].content.as_ref().unwrap().css);
    assert_eq!(
        c.javascript,
        p.pages[0].content.as_ref().unwrap().javascript
    );
    next.inspect_element(next.home_page_id, &t.unwrap())
        .unwrap();
}
#[test]
fn direct_edits_reject_stale_hash_revision_missing_target_and_invalid_attributes() {
    let p = project("<p>Text</p>");
    let mut t = selected(&p, "body > p:nth-of-type(1)", "p");
    assert!(p.edit_element(1, p.home_page_id, &t, &update()).is_err());
    t.source_sha256 = "0".repeat(64);
    assert!(p.inspect_element(p.home_page_id, &t).is_err());
    let t = selected(&p, "body > p:nth-of-type(4294967295)", "p");
    assert!(p.edit_element(0, p.home_page_id, &t, &update()).is_err());
    let t = selected(&p, "body > p:nth-of-type(1)", "p");
    let change = ElementEdit::Update {
        texts: vec![],
        attributes: BTreeMap::from([("onclick".into(), Some("doSomething()".into()))]),
        style: None,
    };
    assert!(p.edit_element(0, p.home_page_id, &t, &change).is_err());
}
#[test]
fn no_op_keeps_original_html_bytes_history_and_durable_write_count() {
    let p = project("<P title='odd quotes'>Text</P>");
    let t = selected(&p, "body > p:nth-of-type(1)", "p");
    let mut s = Session::new(p.clone()).unwrap();
    s.edit_element(0, p.home_page_id, &t, &update(), |_| {
        panic!("No-op must not write")
    })
    .unwrap();
    assert_eq!(s.project(), &p);
    assert!(!s.can_undo());
}
#[test]
fn replace_and_reorder_table_rows_use_the_actual_parent_context() {
    let p = project("<table><tr><td>First</td></tr><tr><td>Second</td></tr></table>");
    let t = selected(
        &p,
        "body > table:nth-of-type(1) > tbody:nth-of-type(1) > tr:nth-of-type(1)",
        "tr",
    );
    let (p, t) = p
        .edit_element(
            0,
            p.home_page_id,
            &t,
            &ElementEdit::Replace {
                html: "<tr><td>Replacement</td></tr>".into(),
                keep_style: true,
            },
        )
        .unwrap();
    let t = t.unwrap();
    assert!(
        without_editor_ids(&p.pages[0].content.as_ref().unwrap().html)
            .contains("<tbody><tr><td>Replacement</td></tr>")
    );
    let (p, t) = p
        .edit_element(
            p.revision,
            p.home_page_id,
            &t,
            &ElementEdit::Reorder {
                direction: ElementOrder::After,
            },
        )
        .unwrap();
    assert!(t.unwrap().selector.ends_with("tr:nth-of-type(2)"));
    assert!(
        without_editor_ids(&p.pages[0].content.as_ref().unwrap().html)
            .contains("<td>Second</td></tr><tr><td>Replacement</td>")
    );
}
#[test]
fn svg_replacement_retains_namespace_and_html_round_trip() {
    let p = project("<svg viewBox='0 0 100 100'><circle cx='20' cy='20' r='10'></circle></svg>");
    let t = selected(
        &p,
        "body > svg:nth-of-type(1) > circle:nth-of-type(1)",
        "circle",
    );
    let (p, t) = p
        .edit_element(
            0,
            p.home_page_id,
            &t,
            &ElementEdit::Replace {
                html: "<rect x='10' y='10' width='30' height='40'></rect>".into(),
                keep_style: true,
            },
        )
        .unwrap();
    assert_eq!(t.unwrap().tag, "rect");
    assert!(
        without_editor_ids(&p.pages[0].content.as_ref().unwrap().html)
            .contains("viewBox=\"0 0 100 100\"")
    );
}
#[test]
fn source_normalization_preserves_merged_html_body_attributes_and_comments() {
    let p = project(
        "<html class='site'><body class='page' data-name='x'><!-- comment --><p>Text</p></body></html>",
    );
    let t = selected(&p, "body > p:nth-of-type(1)", "p");
    let mut change = update();
    if let ElementEdit::Update { style, .. } = &mut change {
        *style = Some("color: red".into());
    }
    let (p, t) = p.edit_element(0, p.home_page_id, &t, &change).unwrap();
    let html = &p.pages[0].content.as_ref().unwrap().html;
    assert!(html.contains("<html class=\"site\">"));
    assert!(html.contains("<body class=\"page\" data-name=\"x\">"));
    assert!(html.contains("<!-- comment -->"));
    p.inspect_element(p.home_page_id, &t.unwrap()).unwrap();
}
#[test]
fn pending_filled_prompts_follow_their_actual_nodes_after_reorder() {
    let mut p = project("<main><p>First</p><p>Second</p></main>");
    let t = selected(&p, "body > main:nth-of-type(1) > p:nth-of-type(1)", "p");
    let page = p.home_page_id;
    p = p
        .apply(
            0,
            Command::AddElementRegion {
                page_id: page,
                rect: Rect {
                    x: 0.,
                    y: 0.,
                    width: 100.,
                    height: 40.,
                },
                element: t.clone(),
            },
        )
        .unwrap();
    let r = p.pages[0].regions[0].clone();
    p = p
        .apply(
            p.revision,
            Command::UpdateRegion {
                page_id: page,
                region_id: r.id,
                rect: r.rect,
                prompt: "Keep this written request".into(),
            },
        )
        .unwrap();
    let (p, _) = p
        .edit_element(
            p.revision,
            page,
            &t,
            &ElementEdit::Reorder {
                direction: ElementOrder::After,
            },
        )
        .unwrap();
    let r = &p.pages[0].regions[0];
    assert_eq!(r.prompt, "Keep this written request");
    assert!(!r.completed);
    assert!(r.targets()[0].selector.ends_with("p:nth-of-type(2)"));
    p.prepare_requirements().unwrap();
    let removed = p
        .edit_element(p.revision, page, r.targets()[0], &ElementEdit::Remove)
        .unwrap()
        .0;
    assert!(removed.prepare_requirements().is_err());
    assert_eq!(removed.pages[0].regions[0].prompt, r.prompt);
}
#[test]
fn deletion_archives_empty_prompts_but_preserves_their_stable_records() {
    let p = project("<p>Text</p>");
    let t = selected(&p, "body > p:nth-of-type(1)", "p");
    let page = p.home_page_id;
    let p = p
        .apply(
            0,
            Command::AddElementRegion {
                page_id: page,
                rect: Rect {
                    x: 0.,
                    y: 0.,
                    width: 100.,
                    height: 40.,
                },
                element: t.clone(),
            },
        )
        .unwrap();
    let id = p.pages[0].regions[0].id;
    let (p, t) = p
        .edit_element(p.revision, page, &t, &ElementEdit::Remove)
        .unwrap();
    assert!(t.is_none());
    assert_eq!(p.pages[0].regions[0].id, id);
    assert!(p.pages[0].regions[0].completed);
    p.validate().unwrap();
}
#[test]
fn direct_edits_are_atomic_and_undo_redo_restore_exact_sources() {
    let p = project("<p>Text</p>");
    let t = selected(&p, "body > p:nth-of-type(1)", "p");
    let mut s = Session::new(p.clone()).unwrap();
    let mut change = update();
    if let ElementEdit::Update { style, .. } = &mut change {
        *style = Some("translate: 20px 30px; transform: rotate(5deg)".into());
    }
    assert!(
        s.edit_element(0, p.home_page_id, &t, &change, |_| Err(Error(
            "Disk full".into()
        )))
        .is_err()
    );
    assert_eq!(s.project(), &p);
    assert!(!s.can_undo());
    s.edit_element(0, p.home_page_id, &t, &change, |_| Ok(()))
        .unwrap();
    let after = s.project().pages.clone();
    s.undo(|_| Ok(())).unwrap();
    assert_eq!(s.project().pages, p.pages);
    s.redo(|_| Ok(())).unwrap();
    assert_eq!(s.project().pages, after);
}
#[test]
fn replacing_body_multiple_elements_and_duplicate_ids_fails_without_edits() {
    let p = project("<p id='a'>A</p><p id='b'>B</p>");
    let body = selected(&p, "body", "body");
    assert!(
        p.edit_element(0, p.home_page_id, &body, &ElementEdit::Remove)
            .is_err()
    );
    let t = selected(&p, "body > p:nth-of-type(1)", "p");
    for html in ["<p>One</p><p>Two</p>", "<button id='b'>Duplicate</button>"] {
        assert!(
            p.edit_element(
                0,
                p.home_page_id,
                &t,
                &ElementEdit::Replace {
                    html: html.into(),
                    keep_style: true
                }
            )
            .is_err()
        );
    }
}

#[test]
fn derived_positions_are_durable_source_bound_and_do_not_add_undo_steps() {
    let p = project("<p>Text</p>");
    let t = selected(&p, "body > p:nth-of-type(1)", "p");
    let page = p.home_page_id;
    let rect = Rect {
        x: 0.,
        y: 0.,
        width: 100.,
        height: 40.,
    };
    let mut p = p
        .apply(
            0,
            Command::AddElementRegion {
                page_id: page,
                rect,
                element: t.clone(),
            },
        )
        .unwrap();
    p.pages[0].regions[0].prompt = "Keep this item".into();
    let id = p.pages[0].regions[0].id;
    let mut s = Session::new(p.clone()).unwrap();
    let change = ElementEdit::Update {
        texts: vec![],
        attributes: BTreeMap::new(),
        style: Some("translate:20px 30px".into()),
    };
    let target = s
        .edit_element(p.revision, page, &t, &change, |_| Ok(()))
        .unwrap()
        .unwrap();
    let before = s.project().clone();
    let bounds = ElementBounds {
        region_id: id,
        target,
        rect: Rect {
            x: 20.,
            y: 30.,
            ..rect
        },
    };
    assert!(
        s.refresh_element_bounds(page, p.viewport, std::slice::from_ref(&bounds), |_| Err(
            Error("Disk full".into())
        ))
        .is_err()
    );
    assert_eq!(s.project(), &before);
    s.refresh_element_bounds(page, p.viewport, std::slice::from_ref(&bounds), |_| Ok(()))
        .unwrap();
    let measured = s.project().clone();
    assert_eq!(measured.pages[0].regions[0].rect, bounds.rect);
    let snapshot = measured.prepare_requirements().unwrap();
    assert!(snapshot.markdown.contains("Keep this item"));
    s.refresh_element_bounds(page, p.viewport, std::slice::from_ref(&bounds), |_| {
        panic!("No-op must not write")
    })
    .unwrap();
    assert_eq!(s.project(), &measured);
    let mut stale = bounds.clone();
    stale.target.id = "000000000000".into();
    assert!(
        s.refresh_element_bounds(page, p.viewport, &[stale], |_| Ok(()))
            .is_err()
    );
    assert!(
        s.refresh_element_bounds(page, p.viewport, &[bounds.clone(), bounds], |_| Ok(()))
            .is_err()
    );
    s.undo(|_| Ok(())).unwrap();
    assert_eq!(s.project().pages, p.pages);
    assert!(!s.can_undo());
    s.redo(|_| Ok(())).unwrap();
    assert_eq!(s.project().pages, measured.pages);
}
