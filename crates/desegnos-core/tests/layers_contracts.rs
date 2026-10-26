use desegnos_core::*;
use dom_query::{Document, NodeRef};
use std::{cell::Cell, collections::BTreeSet};
use uuid::Uuid;

fn project(html: &str) -> Project {
    let mut project = Project::new(
        "Layer checks".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    if !html.is_empty() {
        project.pages[0].content = Some(SiteContent {
            html: html.into(),
            css: "p { color: navy; }".into(),
            javascript: "document.querySelector('p')?.addEventListener('click', () => {});".into(),
        });
    }
    project
}
fn selected(project: &Project, selector: &str, tag: &str) -> ElementTarget {
    ElementTarget {
        id: String::new(),
        selector: selector.into(),
        tag: tag.into(),
        label: "Layer".into(),
        source_sha256: project.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    }
}
fn document(project: &Project) -> Document {
    Document::from(format!(
        "<!doctype html><html><head></head><body>{}</body></html>",
        project.pages[0].content.as_ref().unwrap().html
    ))
}
fn node_target(node: NodeRef<'_>, project: &Project) -> ElementTarget {
    let tag = node.node_name().unwrap().to_ascii_lowercase();
    let mut parts = vec![];
    let mut current = node;
    while current.node_name().as_deref() != Some("body") {
        let name = current.node_name().unwrap().to_ascii_lowercase();
        let parent = current.parent().unwrap();
        let index = parent
            .element_children()
            .into_iter()
            .filter(|child| child.node_name().unwrap().eq_ignore_ascii_case(&name))
            .position(|child| child.id == current.id)
            .unwrap()
            + 1;
        parts.push(format!("{name}:nth-of-type({index})"));
        current = parent;
    }
    parts.push("body".into());
    parts.reverse();
    selected(project, &parts.join(" > "), &tag)
}
fn snapshot(project: &Project, selector: &str) -> Vec<ElementStyleEdit> {
    let doc = document(project);
    let selection = doc.select(selector);
    let root = selection.nodes()[0];
    std::iter::once(root)
        .chain(root.descendants_it().filter(|node| node.is_element()))
        .map(|node| {
            // The browser snapshot promotes every measured property so copied
            // appearance survives destination author rules.
            let mut style =
                "display: block !important; color: rgb(255, 0, 0) !important; translate: none !important;"
                    .to_string();
            if node.attr("id").as_deref() == Some("panel") {
                style.push_str("translate: 10px 20px !important;");
            }
            if node.attr("id").as_deref() == Some("shape") {
                style.push_str("clip-path: url(#crop) !important;");
            }
            ElementStyleEdit {
                target: node_target(node, project),
                style,
                pseudo_styles: vec![],
            }
        })
        .collect()
}
fn copy(project: &Project, selector: &str, tag: &str) -> LayerClipboard {
    project
        .copy_layer(
            project.home_page_id,
            &LayerTarget::Element {
                target: selected(project, selector, tag),
            },
            &snapshot(project, selector),
        )
        .unwrap()
}
fn request(
    project: &mut Project,
    element: Option<ElementTarget>,
    prompt: &str,
    rank: i32,
    completed: bool,
) -> Uuid {
    let id = Uuid::new_v4();
    project.pages[0].regions.push(Region {
        pages: vec![],
        shape: Default::default(),
        area_bounds: vec![],
        id,
        rect: Rect {
            x: 12.0,
            y: 24.0,
            width: 160.0,
            height: 80.0,
        },
        z_index: rank,
        prompt: prompt.into(),
        elements: element.into_iter().collect(),
        allow_target_removal: false,
        mobile_only: false,
        runtime_errors: vec![],
        whole_page: false,
        whole_page_disabled: false,
        attachments: vec![],
        frame_target: None,
        completed,
    });
    id
}

#[test]
fn a_layer_style_batch_is_one_durable_edit_and_preserves_pending_element_targets() {
    let mut p =
        project("<main><section><p>First</p><p>Second</p></section><aside>Outside</aside></main>");
    let first = selected(
        &p,
        "body > main:nth-of-type(1) > section:nth-of-type(1) > p:nth-of-type(1)",
        "p",
    );
    let second = selected(
        &p,
        "body > main:nth-of-type(1) > section:nth-of-type(1) > p:nth-of-type(2)",
        "p",
    );
    request(
        &mut p,
        Some(second.clone()),
        "Keep this written request",
        0,
        false,
    );
    let parent = selected(
        &p,
        "body > main:nth-of-type(1) > section:nth-of-type(1)",
        "section",
    );
    let change = ElementEdit::LayerStyles {
        styles: vec![
            ElementStyleEdit {
                target: first.clone(),
                style: "position: relative; z-index: 1;".into(),
                pseudo_styles: vec![],
            },
            ElementStyleEdit {
                target: second,
                style: "position: relative; z-index: 0;".into(),
                pseudo_styles: vec![],
            },
            ElementStyleEdit {
                target: parent,
                style: "isolation: isolate;".into(),
                pseudo_styles: vec![],
            },
        ],
    };
    let mut session = Session::new(p.clone()).unwrap();
    assert!(
        session
            .edit_element(p.revision, p.home_page_id, &first, &change, |_| Err(Error(
                "Disk full".into()
            )))
            .is_err()
    );
    assert_eq!(session.project(), &p);
    assert!(!session.can_undo());
    let writes = Cell::new(0);
    let result = session
        .edit_element(p.revision, p.home_page_id, &first, &change, |_| {
            writes.set(writes.get() + 1);
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(writes.get(), 1);
    let after = session.project().clone();
    assert_eq!(after.revision, p.revision + 1);
    assert_eq!(result.selector, first.selector);
    assert_ne!(result.source_sha256, first.source_sha256);
    assert_eq!(
        after.pages[0].content.as_ref().unwrap().css,
        p.pages[0].content.as_ref().unwrap().css
    );
    assert_eq!(
        after.pages[0].content.as_ref().unwrap().javascript,
        p.pages[0].content.as_ref().unwrap().javascript
    );
    let tracked = after.pages[0].regions[0].targets()[0];
    assert_eq!(tracked.source_sha256, result.source_sha256);
    assert!(tracked.selector.ends_with("p:nth-of-type(2)"));
    assert_eq!(
        after.pages[0].regions[0].prompt,
        "Keep this written request"
    );
    after.inspect_element(after.home_page_id, tracked).unwrap();
    after.prepare_requirements().unwrap();
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, p.pages);
    assert!(!session.can_undo());
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, after.pages);
}

#[test]
fn style_batches_reject_stale_duplicate_missing_selected_and_unrelated_targets_atomically() {
    let p = project(
        "<main><p><span>Selected child</span></p><p>Sibling</p><aside>Other sibling</aside></main><footer>Unrelated</footer>",
    );
    let selected = selected(&p, "body > main:nth-of-type(1) > p:nth-of-type(1)", "p");
    let correct = ElementStyleEdit {
        target: selected.clone(),
        style: "z-index: 2; position: relative;".into(),
        pseudo_styles: vec![],
    };
    for styles in [
        vec![],
        vec![correct.clone(), correct.clone()],
        vec![ElementStyleEdit {
            target: ElementTarget {
                source_sha256: "0".repeat(64),
                ..selected.clone()
            },
            ..correct.clone()
        }],
        vec![ElementStyleEdit {
            target: self::selected(&p, "body > main:nth-of-type(1) > p:nth-of-type(2)", "p"),
            ..correct.clone()
        }],
        vec![
            correct.clone(),
            ElementStyleEdit {
                target: self::selected(&p, "body > footer:nth-of-type(1)", "footer"),
                ..correct.clone()
            },
        ],
        vec![
            correct.clone(),
            ElementStyleEdit {
                target: self::selected(
                    &p,
                    "body > main:nth-of-type(1) > p:nth-of-type(1) > span:nth-of-type(1)",
                    "span",
                ),
                ..correct.clone()
            },
        ],
        vec![ElementStyleEdit {
            style: "not a declaration".into(),
            ..correct.clone()
        }],
        vec![ElementStyleEdit {
            style: format!("x: {}1{};", "calc(".repeat(130), ")".repeat(130)),
            ..correct.clone()
        }],
        vec![ElementStyleEdit {
            pseudo_styles: vec![LayerPseudoStyle {
                pseudo: LayerPseudo::Before,
                style: "content: 'Decoration';".into(),
            }],
            ..correct.clone()
        }],
        vec![ElementStyleEdit {
            style: "x".repeat(MAX_LAYER_STYLE_BYTES + 1),
            ..correct.clone()
        }],
        vec![correct; MAX_LAYER_STYLE_EDITS + 1],
    ] {
        let mut session = Session::new(p.clone()).unwrap();
        assert!(
            session
                .edit_element(
                    p.revision,
                    p.home_page_id,
                    &selected,
                    &ElementEdit::LayerStyles { styles },
                    |_| panic!("Invalid style batch must not reach durable storage")
                )
                .is_err()
        );
        assert_eq!(session.project(), &p);
        assert!(!session.can_undo());
    }
    let no_op = ElementEdit::LayerStyles {
        styles: vec![ElementStyleEdit {
            target: selected.clone(),
            style: String::new(),
            pseudo_styles: vec![],
        }],
    };
    let mut session = Session::new(p.clone()).unwrap();
    session
        .edit_element(p.revision, p.home_page_id, &selected, &no_op, |_| {
            panic!("No-op must not write")
        })
        .unwrap();
    assert_eq!(session.project(), &p);
}

#[test]
fn request_arrangements_keep_array_order_stable_ties_history_and_safe_integer_ranks() {
    let mut p = project("");
    let a = request(&mut p, None, "A", i32::MAX, false);
    let b = request(&mut p, None, "B", i32::MIN, false);
    let c = request(&mut p, None, "C", i32::MAX, false);
    let d = request(&mut p, None, "D", 0, false);
    let history = request(&mut p, None, "History", i32::MAX, true);
    let original_ids = p.pages[0]
        .regions
        .iter()
        .map(|region| region.id)
        .collect::<Vec<_>>();
    let moved = p
        .apply(
            p.revision,
            Command::ArrangeRegion {
                page_id: p.home_page_id,
                region_id: a,
                direction: LayerDirection::Forward,
            },
        )
        .unwrap();
    assert_eq!(
        moved.pages[0]
            .regions
            .iter()
            .map(|region| region.id)
            .collect::<Vec<_>>(),
        original_ids
    );
    assert_eq!(
        moved.pages[0]
            .regions
            .iter()
            .map(|region| region.z_index)
            .collect::<Vec<_>>(),
        vec![3, 0, 2, 1, i32::MAX]
    );
    for (before, after) in p.pages[0].regions.iter().zip(&moved.pages[0].regions) {
        assert_eq!(
            before,
            &Region {
                pages: vec![],
                z_index: before.z_index,
                ..after.clone()
            }
        );
    }
    assert_eq!(
        moved
            .apply(
                moved.revision,
                Command::ArrangeRegion {
                    page_id: p.home_page_id,
                    region_id: a,
                    direction: LayerDirection::Front
                }
            )
            .unwrap(),
        moved
    );
    assert_eq!(
        p.apply(
            p.revision,
            Command::ArrangeRegion {
                page_id: p.home_page_id,
                region_id: b,
                direction: LayerDirection::Back
            }
        )
        .unwrap(),
        p
    );
    assert!(
        p.apply(
            p.revision,
            Command::ArrangeRegion {
                page_id: p.home_page_id,
                region_id: history,
                direction: LayerDirection::Back
            }
        )
        .is_err()
    );
    assert!(
        p.apply(
            p.revision + 1,
            Command::ArrangeRegion {
                page_id: p.home_page_id,
                region_id: c,
                direction: LayerDirection::Back
            }
        )
        .is_err()
    );
    let mut session = Session::new(moved.clone()).unwrap();
    session
        .execute(
            moved.revision,
            Command::ArrangeRegion {
                page_id: p.home_page_id,
                region_id: a,
                direction: LayerDirection::Forward,
            },
            |_| panic!("End movement must not write"),
        )
        .unwrap();
    assert!(!session.can_undo());
    let command = Command::ArrangeRegion {
        page_id: p.home_page_id,
        region_id: d,
        direction: LayerDirection::Front,
    };
    assert!(
        session
            .execute(moved.revision, command.clone(), |_| Err(Error(
                "Disk full".into()
            )))
            .is_err()
    );
    assert_eq!(session.project(), &moved);
    session
        .execute(moved.revision, command, |_| Ok(()))
        .unwrap();
    let after = session.project().clone();
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, moved.pages);
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, after.pages);
}

#[test]
fn a_request_clipboard_is_portable_fresh_and_does_not_copy_element_or_history_metadata() {
    let mut p = project("<p>Original element</p>");
    let element = selected(&p, "body > p:nth-of-type(1)", "p");
    let id = request(
        &mut p,
        Some(element),
        "Exact request: café, 日本語, <button>",
        i32::MAX,
        true,
    );
    let before = p.clone();
    let clipboard = p
        .copy_layer(p.home_page_id, &LayerTarget::Request { region_id: id }, &[])
        .unwrap();
    assert_eq!(p, before);
    let encoded = clipboard.to_json().unwrap();
    let clipboard = LayerClipboard::from_json(&encoded).unwrap();
    let mut destination = project("");
    request(
        &mut destination,
        None,
        "Top at integer limit",
        i32::MAX,
        false,
    );
    let (pasted, selection) = destination
        .paste_layer(
            destination.revision,
            destination.home_page_id,
            &clipboard,
            None,
        )
        .unwrap();
    let new_id = selection.region_id.unwrap();
    assert_ne!(new_id, id);
    assert!(selection.element.is_none());
    let new = pasted.pages[0].regions.last().unwrap();
    assert_eq!(new.id, new_id);
    assert_eq!(
        new.rect,
        Rect {
            x: 36.0,
            y: 48.0,
            width: 160.0,
            height: 80.0
        }
    );
    assert_eq!(new.prompt, before.pages[0].regions[0].prompt);
    assert!(!new.completed);
    assert!(new.elements.is_empty());
    assert_eq!(new.z_index, 1);
    assert!(pasted.pages[0].content.is_none());
    let mut session = Session::new(destination.clone()).unwrap();
    assert!(
        session
            .paste_layer(
                destination.revision,
                destination.home_page_id,
                &clipboard,
                None,
                |_| Err(Error("Disk full".into()))
            )
            .is_err()
    );
    assert_eq!(session.project(), &destination);
    session
        .paste_layer(
            destination.revision,
            destination.home_page_id,
            &clipboard,
            None,
            |_| Ok(()),
        )
        .unwrap();
    let after = session.project().clone();
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, destination.pages);
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, after.pages);
}

#[test]
fn all_important_browser_snapshots_validate_serialize_and_copy_without_style_growth() {
    let source = project(
        "<section id='card'><button type='button'>Continue<span>Next</span></button></section>",
    );
    let before = source.clone();
    let computed = concat!(
        "display: block !important; position: relative !important; box-sizing: border-box !important;",
        "width: 240px !important; height: 80px !important; min-width: 0px !important; max-width: none !important;",
        "color: rgb(20, 30, 40) !important; background-color: rgb(245, 245, 245) !important;",
        "border-top-width: 1px !important; border-right-width: 1px !important; border-bottom-width: 1px !important; border-left-width: 1px !important;",
        "border-top-style: solid !important; border-right-style: solid !important; border-bottom-style: solid !important; border-left-style: solid !important;",
        "border-top-color: rgb(100, 100, 100) !important; border-right-color: rgb(100, 100, 100) !important; border-bottom-color: rgb(100, 100, 100) !important; border-left-color: rgb(100, 100, 100) !important;",
        "padding-top: 8px !important; padding-right: 12px !important; padding-bottom: 8px !important; padding-left: 12px !important;",
        "margin-top: 0px !important; margin-right: 0px !important; margin-bottom: 0px !important; margin-left: 0px !important;",
        "font-family: system-ui !important; font-size: 16px !important; font-weight: 400 !important; line-height: 24px !important;",
        "opacity: 1 !important; z-index: auto !important; transform: none !important; translate: none !important;",
        "animation-name: none !important; animation-play-state: running !important; transition-property: all !important; transition-duration: 0s !important;",
        "visibility: visible !important; pointer-events: auto !important; --Brand: #224466 !important;"
    );
    let capture = |source: &Project| {
        let mut styles = snapshot(source, "body > section:nth-of-type(1)");
        for style in &mut styles {
            style.style = computed.into();
        }
        source
            .copy_layer(
                source.home_page_id,
                &LayerTarget::Element {
                    target: selected(source, "body > section:nth-of-type(1)", "section"),
                },
                &styles,
            )
            .unwrap()
    };
    let clipboard = capture(&source);
    assert_eq!(source, before);
    let LayerClipboardContent::Element { html, .. } = &clipboard.content else {
        panic!("Expected a complete visual element packet");
    };
    let baked = Document::from(format!("<body>{html}</body>"));
    for node in baked.select("section, button, span").nodes() {
        assert_eq!(node.attr("style").unwrap().matches("display:").count(), 1);
    }
    for _ in 0..3 {
        let json = clipboard.to_json().unwrap();
        assert_eq!(LayerClipboard::from_json(&json).unwrap(), clipboard);
        assert_eq!(capture(&project(html)).content, clipboard.content);
    }
    let destination = project("");
    let (pasted, selection) = destination
        .paste_layer(0, destination.home_page_id, &clipboard, None)
        .unwrap();
    pasted
        .inspect_element(pasted.home_page_id, &selection.element.unwrap())
        .unwrap();
    assert_eq!(document(&pasted).select("button > span").length(), 1);
}

#[test]
fn copied_original_important_rules_keep_shorthand_order_with_mixed_snapshot_priorities() {
    let source = project(
        "<section style='border-left-color: blue !important; border: 2px solid red !important; color: navy !important; padding: 5px; --Ink: teal !important'>Content</section>",
    );
    let before = source.clone();
    let mut styles = snapshot(&source, "body > section:nth-of-type(1)");
    styles[0].style = concat!(
        "color: rgb(0, 0, 128) !important; display: block; border-left-color: rgb(255, 0, 0) !important;",
        "border-top-width: 2px !important; border-top-style: solid !important; border-top-color: rgb(255, 0, 0) !important;",
        "padding-left: 5px; --Ink: teal !important; translate: none !important;"
    )
    .into();
    let clipboard = source
        .copy_layer(
            source.home_page_id,
            &LayerTarget::Element {
                target: selected(&source, "body > section:nth-of-type(1)", "section"),
            },
            &styles,
        )
        .unwrap();
    assert_eq!(source, before);
    let LayerClipboardContent::Element { html, .. } = &clipboard.content else {
        panic!("Expected a complete visual element packet");
    };
    let doc = Document::from(format!("<body>{html}</body>"));
    let selection = doc.select("section");
    let inline = selection.attr("style").unwrap();
    assert!(
        inline.rfind("border-left-color: blue !important;").unwrap()
            < inline.rfind("border: 2px solid red !important;").unwrap()
    );
    assert!(inline.contains("color: navy !important;"));
    assert_eq!(inline.matches("--Ink:").count(), 1);
    assert_eq!(inline.matches("display:").count(), 1);
    for _ in 0..3 {
        clipboard.validate().unwrap();
        assert_eq!(
            LayerClipboard::from_json(&clipboard.to_json().unwrap()).unwrap(),
            clipboard
        );
    }
}

#[test]
fn visual_element_copy_paste_preserves_assets_important_styles_and_internal_references() {
    let source = project(
        r##"<section id="panel" style="color: red !important; translate: 10px 20px !important; border: 2px solid red !important"><label for="field">Name</label><input id="field" aria-describedby="desc" list="choices"><p id="desc">Description</p><datalist id="choices"><option value="One"></option></datalist><a href="#panel">Local</a><a href="https://example.com/#panel">External</a><img alt="Portable" src="data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxIiBoZWlnaHQ9IjEiPjwvc3ZnPg=="><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><defs><clipPath id="crop"><rect width="10" height="10"></rect></clipPath></defs><rect id="shape" clip-path="url(#crop)" style="clip-path: url(#crop)"></rect><use href="#shape" xlink:href="#shape"></use></svg></section>"##,
    );
    let before = source.clone();
    let clipboard = copy(&source, "body > section:nth-of-type(1)", "section");
    assert_eq!(source, before);
    let clipboard = LayerClipboard::from_json(&clipboard.to_json().unwrap()).unwrap();
    let destination = project("");
    let (pasted, selected) = destination
        .paste_layer(0, destination.home_page_id, &clipboard, None)
        .unwrap();
    let selected = selected.element.unwrap();
    pasted
        .inspect_element(pasted.home_page_id, &selected)
        .unwrap();
    let doc = document(&pasted);
    let root = doc.select("section");
    let new_panel = root.attr("id").unwrap();
    assert_ne!(new_panel.as_ref(), "panel");
    let inline = root.attr("style").unwrap();
    assert!(inline.contains("color: red !important"));
    assert!(inline.contains("border: 2px solid red !important"));
    assert!(inline.contains("translate: calc(10px + 24px) calc(20px + 24px) !important"));
    let field = doc.select("input").attr("id").unwrap();
    assert_eq!(doc.select("label").attr("for").unwrap(), field);
    assert_eq!(
        doc.select("input").attr("aria-describedby").unwrap(),
        doc.select("p").attr("id").unwrap()
    );
    assert_eq!(
        doc.select("input").attr("list").unwrap(),
        doc.select("datalist").attr("id").unwrap()
    );
    assert_eq!(
        doc.select("a").nodes()[0].attr("href").unwrap().as_ref(),
        format!("#{new_panel}")
    );
    assert_eq!(
        doc.select("a").nodes()[1].attr("href").unwrap().as_ref(),
        "https://example.com/#panel"
    );
    let clip = doc.select("clipPath").attr("id").unwrap();
    let shape = doc.select("svg > rect").attr("id").unwrap();
    assert!(
        doc.select("svg > rect")
            .attr("style")
            .unwrap()
            .contains(clip.as_ref())
    );
    assert!(
        doc.select("svg > rect")
            .attr("clip-path")
            .unwrap()
            .contains(clip.as_ref())
    );
    let use_attrs = doc.select("use").nodes()[0].attrs();
    assert_eq!(
        use_attrs
            .iter()
            .filter(|attr| attr.name.local.as_ref() == "href"
                && attr.value.as_ref() == format!("#{shape}"))
            .count(),
        2
    );
    assert!(
        doc.select("img")
            .attr("src")
            .unwrap()
            .starts_with("desegnos-asset:")
    );
    assert!(
        pasted.pages[0]
            .content
            .as_ref()
            .unwrap()
            .javascript
            .is_empty(),
        "Runtime listener closures are not copied"
    );
    assert!(pasted.pages[0].content.as_ref().unwrap().css.is_empty());
    let (twice, _) = pasted
        .paste_layer(pasted.revision, pasted.home_page_id, &clipboard, None)
        .unwrap();
    let doc = document(&twice);
    let ids = doc
        .select("[id]")
        .nodes()
        .iter()
        .map(|node| node.attr("id").unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), ids.iter().collect::<BTreeSet<_>>().len());
}

#[test]
fn element_paste_inserts_after_the_selected_sibling_and_remaps_existing_written_requests() {
    let clipboard = copy(
        &project("<p id='copied'>Copied</p>"),
        "body > p:nth-of-type(1)",
        "p",
    );
    let mut destination = project("<main><p>Anchor</p><p>Keep this request</p></main>");
    let anchor = selected(
        &destination,
        "body > main:nth-of-type(1) > p:nth-of-type(1)",
        "p",
    );
    let tracked = selected(
        &destination,
        "body > main:nth-of-type(1) > p:nth-of-type(2)",
        "p",
    );
    request(
        &mut destination,
        Some(tracked),
        "Change only this original item",
        1,
        false,
    );
    let mut session = Session::new(destination.clone()).unwrap();
    assert!(
        session
            .paste_layer(
                destination.revision,
                destination.home_page_id,
                &clipboard,
                Some(&anchor),
                |_| Err(Error("Disk full".into()))
            )
            .is_err()
    );
    assert_eq!(session.project(), &destination);
    assert!(!session.can_undo());
    let selection = session
        .paste_layer(
            destination.revision,
            destination.home_page_id,
            &clipboard,
            Some(&anchor),
            |_| Ok(()),
        )
        .unwrap();
    assert!(
        selection
            .element
            .unwrap()
            .selector
            .ends_with("p:nth-of-type(2)")
    );
    let after = session.project().clone();
    let tracked = after.pages[0].regions[0].targets()[0];
    assert!(tracked.selector.ends_with("p:nth-of-type(3)"));
    assert_eq!(
        after
            .inspect_element(after.home_page_id, tracked)
            .unwrap()
            .texts[0]
            .text,
        "Keep this request"
    );
    assert_eq!(
        after.pages[0].content.as_ref().unwrap().css,
        destination.pages[0].content.as_ref().unwrap().css
    );
    assert_eq!(
        after.pages[0].content.as_ref().unwrap().javascript,
        destination.pages[0].content.as_ref().unwrap().javascript
    );
    assert!(
        after
            .paste_layer(
                after.revision,
                after.home_page_id,
                &clipboard,
                Some(&anchor)
            )
            .is_err()
    );
    assert!(
        after
            .paste_layer(after.revision + 1, after.home_page_id, &clipboard, None)
            .is_err()
    );
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, destination.pages);
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, after.pages);
}

#[test]
fn clipboard_context_supports_table_rows_and_svg_children_but_rejects_browser_repairs() {
    let table = project("<table><tr><td>Original</td></tr><tr><td>Anchor</td></tr></table>");
    let clipboard = copy(
        &table,
        "body > table:nth-of-type(1) > tbody:nth-of-type(1) > tr:nth-of-type(1)",
        "tr",
    );
    let anchor = selected(
        &table,
        "body > table:nth-of-type(1) > tbody:nth-of-type(1) > tr:nth-of-type(2)",
        "tr",
    );
    let (pasted, selection) = table
        .paste_layer(
            table.revision,
            table.home_page_id,
            &clipboard,
            Some(&anchor),
        )
        .unwrap();
    assert!(
        selection
            .element
            .unwrap()
            .selector
            .ends_with("tr:nth-of-type(3)")
    );
    assert_eq!(document(&pasted).select("td").nodes().len(), 3);
    let blank = project("");
    assert!(
        blank
            .paste_layer(0, blank.home_page_id, &clipboard, None)
            .is_err()
    );
    let div = copy(
        &project("<div>Block</div>"),
        "body > div:nth-of-type(1)",
        "div",
    );
    assert!(
        table
            .paste_layer(0, table.home_page_id, &div, Some(&anchor))
            .is_err()
    );
    let svg = project(
        "<svg><circle cx='5' cy='5' r='2'></circle><circle cx='10' cy='10' r='2'></circle></svg>",
    );
    let circle = copy(
        &svg,
        "body > svg:nth-of-type(1) > circle:nth-of-type(1)",
        "circle",
    );
    let anchor = selected(
        &svg,
        "body > svg:nth-of-type(1) > circle:nth-of-type(2)",
        "circle",
    );
    let (pasted, _) = svg
        .paste_layer(0, svg.home_page_id, &circle, Some(&anchor))
        .unwrap();
    assert_eq!(document(&pasted).select("svg > circle").nodes().len(), 3);
    assert!(
        blank
            .paste_layer(0, blank.home_page_id, &circle, None)
            .is_err()
    );
}

#[test]
fn camel_case_svg_targets_keep_their_real_tag_and_selector_after_copy_and_direct_edits() {
    let p = project(
        "<svg><defs><clipPath id='clip'><rect></rect></clipPath><linearGradient id='gradient'><stop offset='0'></stop></linearGradient></defs><foreignObject><div>Native HTML inside SVG</div></foreignObject></svg>",
    );
    for (path, tag) in [
        (
            "body > svg:nth-of-type(1) > defs:nth-of-type(1) > clipPath:nth-of-type(1)",
            "clipPath",
        ),
        (
            "body > svg:nth-of-type(1) > defs:nth-of-type(1) > linearGradient:nth-of-type(1)",
            "linearGradient",
        ),
        (
            "body > svg:nth-of-type(1) > foreignObject:nth-of-type(1)",
            "foreignObject",
        ),
    ] {
        let target = selected(&p, path, tag);
        target.validate().unwrap();
        p.inspect_element(p.home_page_id, &target).unwrap();
        let (after, retained) = p
            .edit_element(
                p.revision,
                p.home_page_id,
                &target,
                &ElementEdit::LayerStyles {
                    styles: vec![ElementStyleEdit {
                        target: target.clone(),
                        style: "opacity: 0.9;".into(),
                        pseudo_styles: vec![],
                    }],
                },
            )
            .unwrap();
        let retained = retained.unwrap();
        assert_eq!(retained.tag, tag);
        assert_eq!(retained.selector, path);
        after
            .inspect_element(after.home_page_id, &retained)
            .unwrap();
    }
    let copied = copy(&p, "body > svg:nth-of-type(1)", "svg");
    let destination = project("");
    let (after, _) = destination
        .paste_layer(0, destination.home_page_id, &copied, None)
        .unwrap();
    let doc = document(&after);
    assert_eq!(
        doc.select("clipPath").nodes()[0].node_name().as_deref(),
        Some("clipPath")
    );
    assert_eq!(
        doc.select("linearGradient").nodes()[0]
            .node_name()
            .as_deref(),
        Some("linearGradient")
    );
    assert_eq!(
        doc.select("foreignObject").nodes()[0]
            .node_name()
            .as_deref(),
        Some("foreignObject")
    );
    let invalid = ElementTarget {
        tag: "clipPath[onclick]".into(),
        ..selected(
            &p,
            "body > svg:nth-of-type(1) > defs:nth-of-type(1) > clipPath:nth-of-type(1)",
            "clipPath",
        )
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn clipboard_rejects_incomplete_or_foreign_snapshots_duplicate_ids_and_active_embedded_content() {
    let p = project("<main><p>Text</p></main><aside>Outside</aside>");
    let target = selected(&p, "body > main:nth-of-type(1)", "main");
    let snapshot = snapshot(&p, &target.selector);
    for styles in [
        vec![],
        vec![snapshot[0].clone()],
        vec![snapshot[0].clone(), snapshot[0].clone()],
        vec![
            snapshot[0].clone(),
            ElementStyleEdit {
                target: selected(&p, "body > aside:nth-of-type(1)", "aside"),
                style: snapshot[1].style.clone(),
                pseudo_styles: vec![],
            },
        ],
        vec![
            snapshot[0].clone(),
            ElementStyleEdit {
                target: ElementTarget {
                    source_sha256: "0".repeat(64),
                    ..snapshot[1].target.clone()
                },
                style: snapshot[1].style.clone(),
                pseudo_styles: vec![],
            },
        ],
    ] {
        assert!(
            p.copy_layer(
                p.home_page_id,
                &LayerTarget::Element {
                    target: target.clone()
                },
                &styles
            )
            .is_err()
        );
    }
    assert!(
        p.copy_layer(
            p.home_page_id,
            &LayerTarget::Element {
                target: selected(&p, "body", "body")
            },
            &[]
        )
        .is_err()
    );
    let duplicates = project("<p id='same'>One</p><p id='same'>Two</p>");
    assert!(
        duplicates
            .copy_layer(
                duplicates.home_page_id,
                &LayerTarget::Element {
                    target: selected(&duplicates, "body > p:nth-of-type(1)", "p")
                },
                &self::snapshot(&duplicates, "body > p:nth-of-type(1)")
            )
            .is_err()
    );
    let clipboard = copy(&project("<p>Portable</p>"), "body > p:nth-of-type(1)", "p");
    assert!(
        duplicates
            .paste_layer(
                duplicates.revision,
                duplicates.home_page_id,
                &clipboard,
                None
            )
            .is_err()
    );
    for html in [
        "<main><script>window.run()</script></main>",
        "<main><style>body{display:none}</style></main>",
        "<main><iframe src='https://example.com/'></iframe></main>",
        "<main><object data='x'></object></main>",
        "<main><embed src='x'></main>",
        "<svg><style>rect{fill:red}</style><rect></rect></svg>",
    ] {
        let source = project(html);
        let tag = if html.starts_with("<svg") {
            "svg"
        } else {
            "main"
        };
        let path = format!("body > {tag}:nth-of-type(1)");
        let before = source.clone();
        let error = source
            .copy_layer(
                source.home_page_id,
                &LayerTarget::Element {
                    target: selected(&source, &path, tag),
                },
                &self::snapshot(&source, &path),
            )
            .unwrap_err();
        assert!(error.0.contains("Copy a smaller visual layer"));
        assert_eq!(source, before);
    }
}

#[test]
fn clipboard_format_bounds_and_snapshot_integrity_are_checked_before_any_edit() {
    let original = copy(&project("<p>Portable</p>"), "body > p:nth-of-type(1)", "p");
    let encoded = original.to_json().unwrap();
    assert!(LayerClipboard::from_json(&" ".repeat(MAX_LAYER_CLIPBOARD_BYTES + 1)).is_err());
    assert!(
        LayerClipboard::from_json(&encoded.replacen(
            "\"version\":1",
            "\"version\":1,\"version\":1",
            1
        ))
        .is_err()
    );
    for condition in [
        "format", "version", "style", "path", "missing", "html", "unknown",
    ] {
        let mut packet = serde_json::to_value(&original).unwrap();
        match condition {
            "format" => packet["format"] = "foreign".into(),
            "version" => packet["version"] = 2.into(),
            "style" => packet["content"]["visualStyles"][0]["style"] = "color: blue".into(),
            "path" => packet["content"]["visualStyles"][0]["path"] = serde_json::json!([99]),
            "missing" => packet["content"]["visualStyles"] = serde_json::json!([]),
            "html" => packet["content"]["html"] = "<p>One</p><p>Two</p>".into(),
            "unknown" => packet["content"]["invented"] = true.into(),
            _ => unreachable!(),
        }
        assert!(
            LayerClipboard::from_json(&packet.to_string()).is_err(),
            "{condition}"
        );
    }
    let mut invalid = original;
    if let LayerClipboardContent::Element { html, .. } = &mut invalid.content {
        *html = "x".repeat(MAX_SITE_SOURCE_BYTES + 1);
    }
    let destination = project("<p>Untouched</p>");
    let mut session = Session::new(destination.clone()).unwrap();
    assert!(
        session
            .paste_layer(
                destination.revision,
                destination.home_page_id,
                &invalid,
                None,
                |_| panic!("Malformed clipboard cannot reach durability")
            )
            .is_err()
    );
    assert_eq!(session.project(), &destination);
    assert!(!session.can_undo());
}

#[test]
fn copied_badges_decorations_and_markers_get_fresh_scoped_important_css_before_hashing() {
    let mut source = project(
        "<section id='badge'><span data-caption='Details'>Caption</span><ul><li>Item</li></ul><svg><defs><clipPath id='crop'><rect></rect></clipPath></defs></svg></section>",
    );
    source.pages[0].content.as_mut().unwrap().css =
        "#badge::before{content:'New';color:green} span::after{content:attr(data-caption)}".into();
    let source_before = source.clone();
    let mut styles = snapshot(&source, "body > section:nth-of-type(1)");
    styles[0].pseudo_styles = vec![
        LayerPseudoStyle {
            pseudo: LayerPseudo::Before,
            style: "content: 'New'; color: green;".into(),
        },
        LayerPseudoStyle {
            pseudo: LayerPseudo::After,
            style: "content: ''; background-image: url(#crop); width: 12px;".into(),
        },
    ];
    styles
        .iter_mut()
        .find(|style| style.target.tag == "span")
        .unwrap()
        .pseudo_styles = vec![LayerPseudoStyle {
        pseudo: LayerPseudo::After,
        style: "content: attr(data-caption); color: red;".into(),
    }];
    styles
        .iter_mut()
        .find(|style| style.target.tag == "li")
        .unwrap()
        .pseudo_styles = vec![LayerPseudoStyle {
        pseudo: LayerPseudo::Marker,
        style: "content: '• '; color: blue;".into(),
    }];
    let target = selected(&source, "body > section:nth-of-type(1)", "section");
    let clipboard = source
        .copy_layer(
            source.home_page_id,
            &LayerTarget::Element { target },
            &styles,
        )
        .unwrap();
    assert_eq!(source, source_before);
    let clipboard = LayerClipboard::from_json(&clipboard.to_json().unwrap()).unwrap();
    let mut destination =
        project("<main id='badge'>Existing badge</main><p id='existing'>Keep request</p>");
    let original_css = "@charset \"UTF-8\";\n@import url(\"https://example.com/existing.css\");\n@layer old { *::before { content: 'Destination' !important; color: black !important; } }\n#badge::before { content: 'Original' !important; }";
    destination.pages[0].content.as_mut().unwrap().css = original_css.into();
    let pending = selected(&destination, "body > p:nth-of-type(1)", "p");
    request(
        &mut destination,
        Some(pending),
        "Keep this exact request",
        0,
        false,
    );
    let mut session = Session::new(destination.clone()).unwrap();
    assert!(
        session
            .paste_layer(
                destination.revision,
                destination.home_page_id,
                &clipboard,
                None,
                |_| Err(Error("Disk full".into()))
            )
            .is_err()
    );
    assert_eq!(session.project(), &destination);
    let selection = session
        .paste_layer(
            destination.revision,
            destination.home_page_id,
            &clipboard,
            None,
            |_| Ok(()),
        )
        .unwrap()
        .element
        .unwrap();
    let after = session.project().clone();
    let content = after.pages[0].content.as_ref().unwrap();
    assert!(!selection.id.is_empty());
    after
        .inspect_element(after.home_page_id, &selection)
        .unwrap();
    assert!(!after.pages[0].regions[0].targets()[0].id.is_empty());
    let doc = document(&after);
    let root_id = doc.select("section").attr("id").unwrap();
    let span_id = doc.select("span").attr("id").unwrap();
    let marker_id = doc.select("li").attr("id").unwrap();
    let clip_id = doc.select("clipPath").attr("id").unwrap();
    for id in [&root_id, &span_id, &marker_id] {
        assert!(id.starts_with("desegnos-copy-"));
    }
    assert_eq!(doc.select("main").attr("id").unwrap().as_ref(), "badge");
    assert!(
        content
            .css
            .starts_with("@charset \"UTF-8\";\n@layer desegnos_paste_")
    );
    let first_layer = content
        .css
        .lines()
        .find(|line| line.starts_with("@layer desegnos_paste_") && line.ends_with(';'))
        .unwrap();
    assert!(content.css.find(first_layer).unwrap() < content.css.find("@import").unwrap());
    assert!(
        content
            .css
            .contains(original_css.strip_prefix("@charset \"UTF-8\";").unwrap())
    );
    let layer_name = first_layer
        .strip_prefix("@layer ")
        .unwrap()
        .strip_suffix(';')
        .unwrap();
    let rules = content
        .css
        .split_once(&format!("@layer {layer_name}{{"))
        .unwrap()
        .1;
    assert!(rules.contains(&format!(
        "#{root_id}::before{{content: 'New' !important;color: green !important;}}"
    )));
    assert!(rules.contains(&format!("#{root_id}::after{{")));
    assert!(rules.contains(&format!(
        "#{span_id}::after{{content: attr(data-caption) !important;color: red !important;}}"
    )));
    assert!(rules.contains(&format!("#{marker_id}::marker{{")));
    assert!(rules.contains(clip_id.as_ref()));
    assert!(!rules.contains("#badge"));
    assert!(!rules.contains("#existing"));
    assert!(!rules.contains("url(#crop)"));
    after
        .inspect_element(after.home_page_id, after.pages[0].regions[0].targets()[0])
        .unwrap();
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, destination.pages);
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, after.pages);
}

#[test]
fn pseudo_snapshots_are_bounded_unique_and_only_capture_visible_decorations() {
    let p = project("<p>Layer</p>");
    let target = selected(&p, "body > p:nth-of-type(1)", "p");
    for pseudo_styles in [
        vec![LayerPseudoStyle {
            pseudo: LayerPseudo::Before,
            style: "content: none;".into(),
        }],
        vec![LayerPseudoStyle {
            pseudo: LayerPseudo::After,
            style: "color: red;".into(),
        }],
        vec![LayerPseudoStyle {
            pseudo: LayerPseudo::Marker,
            style: String::new(),
        }],
        vec![
            LayerPseudoStyle {
                pseudo: LayerPseudo::Before,
                style: "content: 'a';".into()
            };
            2
        ],
        vec![LayerPseudoStyle {
            pseudo: LayerPseudo::After,
            style: "x".repeat(MAX_LAYER_STYLE_BYTES + 1),
        }],
    ] {
        let styles = vec![ElementStyleEdit {
            target: target.clone(),
            style: "color: red;".into(),
            pseudo_styles,
        }];
        assert!(
            p.copy_layer(
                p.home_page_id,
                &LayerTarget::Element {
                    target: target.clone()
                },
                &styles
            )
            .is_err()
        );
    }
    let style: ElementStyleEdit =
        serde_json::from_value(serde_json::json!({"target":target,"style":"color: red;"})).unwrap();
    assert!(style.pseudo_styles.is_empty());
    assert!(
        serde_json::to_value(style)
            .unwrap()
            .get("pseudoStyles")
            .is_none()
    );
}
