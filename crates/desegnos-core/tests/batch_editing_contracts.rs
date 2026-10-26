use desegnos_core::*;
use std::collections::BTreeMap;

fn project(html: &str) -> Project {
    let mut p = Project::new(
        "Batch editing".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap();
    p.pages[0].content = Some(SiteContent {
        html: html.into(),
        css: String::new(),
        javascript: String::new(),
    });
    p
}

fn selected(p: &Project, path: &str, tag: &str) -> ElementTarget {
    ElementTarget {
        id: String::new(),
        selector: path.into(),
        tag: tag.into(),
        label: String::new(),
        source_sha256: p.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    }
}

fn style(style: &str) -> ElementEdit {
    ElementEdit::Update {
        texts: vec![],
        attributes: BTreeMap::new(),
        style: Some(style.into()),
    }
}

fn html(p: &Project) -> &str {
    &p.pages[0].content.as_ref().unwrap().html
}

#[test]
fn one_batch_is_one_history_step_and_changes_every_target() {
    let p = project("<p>One</p><p>Two</p><p>Three</p>");
    let targets = [
        selected(&p, "body > p:nth-of-type(1)", "p"),
        selected(&p, "body > p:nth-of-type(2)", "p"),
        selected(&p, "body > p:nth-of-type(3)", "p"),
    ];
    let edits = targets
        .iter()
        .map(|target| ElementBatchEdit {
            target: target.clone(),
            edit: style("color: red !important;"),
        })
        .collect::<Vec<_>>();
    let mut session = Session::new(p.clone()).unwrap();
    let before = session.history();
    let mut writes = 0;
    let next = session
        .edit_elements_document(0, &DocumentTarget::Page { page_id: p.home_page_id }, &edits, |_| {
            writes += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(writes, 1, "the whole batch is one durable write");
    let after = session.history();
    assert_eq!(after.2, before.2 + 1, "the whole batch is one history step");
    assert_eq!(next.len(), 3);
    assert!(next.iter().all(Option::is_some));
    assert_eq!(html(session.project()).matches("color: red").count(), 3);
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(html(session.project()), html(&p));
}

#[test]
fn batch_accepts_a_change_per_target() {
    let p = project("<h1>Title</h1><p>Body</p>");
    let edits = [
        ElementBatchEdit {
            target: selected(&p, "body > h1:nth-of-type(1)", "h1"),
            edit: style("font-size: 40px !important;"),
        },
        ElementBatchEdit {
            target: selected(&p, "body > p:nth-of-type(1)", "p"),
            edit: style("font-size: 18px !important;"),
        },
    ];
    let (next, targets) = p
        .edit_elements_in(
            &DocumentTarget::Page { page_id: p.home_page_id },
            p.revision,
            &edits,
        )
        .unwrap();
    let html = html(&next);
    assert!(html.contains("font-size: 40px"), "{html}");
    assert!(html.contains("font-size: 18px"), "{html}");
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().flatten().all(|target| !target.id.is_empty()));
}

#[test]
fn a_stale_target_rejects_the_whole_batch_atomically() {
    let p = project("<p>One</p><p>Two</p>");
    let edits = [
        ElementBatchEdit {
            target: selected(&p, "body > p:nth-of-type(1)", "p"),
            edit: style("color: red !important;"),
        },
        ElementBatchEdit {
            target: selected(&p, "body > p:nth-of-type(9)", "p"),
            edit: style("color: red !important;"),
        },
    ];
    let mut session = Session::new(p.clone()).unwrap();
    let error = session
        .edit_elements_document(0, &DocumentTarget::Page { page_id: p.home_page_id }, &edits, |_| {
            panic!("a rejected batch must not write")
        })
        .unwrap_err();
    assert!(!error.to_string().is_empty());
    assert_eq!(session.project(), &p);
    assert!(!session.can_undo());
    assert!(!html(session.project()).contains("color: red"));
}

#[test]
fn a_target_supplied_twice_rejects_the_batch() {
    let p = project("<p>One</p>");
    let target = selected(&p, "body > p:nth-of-type(1)", "p");
    let edits = [
        ElementBatchEdit {
            target: target.clone(),
            edit: style("color: red !important;"),
        },
        ElementBatchEdit {
            target,
            edit: style("color: blue !important;"),
        },
    ];
    assert!(
        p.edit_elements_in(
            &DocumentTarget::Page { page_id: p.home_page_id },
            p.revision,
            &edits
        )
        .is_err()
    );
}

#[test]
fn batches_over_the_selection_limit_are_rejected() {
    let p = project("<p>One</p>");
    let edit = ElementBatchEdit {
        target: selected(&p, "body > p:nth-of-type(1)", "p"),
        edit: style("color: red !important;"),
    };
    let edits = vec![edit; MAX_BATCH_ELEMENT_EDITS + 1];
    assert!(
        p.edit_elements_in(
            &DocumentTarget::Page { page_id: p.home_page_id },
            p.revision,
            &edits
        )
        .is_err()
    );
}
