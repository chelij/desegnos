//! One element change across pages and framed site documents: matching, one history step and
//! text that only changes where it was identical.
use base64::{Engine, engine::general_purpose::STANDARD};
use desegni_core::*;
use std::collections::BTreeMap;

const SITE: &str = "shared/index.html";
const HEADER: &str = "<header class=\"site\"><nav class=\"main\"><a class=\"link\" href=\"/\">Home</a><a class=\"link\" href=\"/about\">About</a></nav></header>";

fn content(html: &str) -> SiteContent {
    SiteContent {
        html: html.into(),
        css: String::new(),
        javascript: String::new(),
    }
}

/// A site with three pages and one framed site file that share the same header.
fn project() -> Project {
    let mut project = Project::new(
        "Across pages".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    let home = project.home_page_id;
    project.pages[0].content = Some(content(&format!("{HEADER}<main><h1>Home</h1></main>")));
    project = project
        .apply(
            project.revision,
            Command::AddPage {
                name: "Project one".into(),
                path: "/projects/one".into(),
            },
        )
        .unwrap();
    project.pages[1].content = Some(content(&format!("{HEADER}<main><h1>One</h1></main>")));
    project = project
        .apply(
            project.revision,
            Command::AddPage {
                name: "About".into(),
                path: "/about".into(),
            },
        )
        .unwrap();
    project.pages[2].content = Some(content(&format!(
        "<div id=\"app\">{HEADER}</div><main><h1>About</h1></main>"
    )));
    project.website = Some(ImportedWebsite {
        page_files: BTreeMap::from([(home, "index.html".into())]),
        files: BTreeMap::from([(
            SITE.into(),
            STANDARD
                .encode(format!("<!doctype html>\n<html><body>{HEADER}</body></html>"))
                .into(),
        )]),
        framework: None,
        skipped: vec![],
    });
    project
}

fn link(project: &Project, page: usize, position: usize) -> ElementTarget {
    ElementTarget {
        id: String::new(),
        selector: format!(
            "body > {}{}nav:nth-of-type(1) > a:nth-of-type({})",
            if page == 2 { "div:nth-of-type(1) > " } else { "" },
            "header:nth-of-type(1) > ",
            position + 1
        ),
        tag: "a".into(),
        label: "Home".into(),
        source_sha256: project.pages[page].content.as_ref().unwrap().sha256().unwrap(),
    }
}

fn page_html(project: &Project, index: usize) -> String {
    project.pages[index].content.as_ref().unwrap().html.clone()
}

fn style_edit(style: &str) -> ElementEdit {
    ElementEdit::Update {
        texts: vec![],
        attributes: BTreeMap::new(),
        style: Some(style.into()),
    }
}

fn text_edit(text: &str) -> ElementEdit {
    ElementEdit::Update {
        texts: vec![TextEdit {
            index: 0,
            text: text.into(),
        }],
        attributes: BTreeMap::new(),
        style: None,
    }
}

#[test]
fn matches_a_shared_header_on_pages_and_framed_site_files() {
    let project = project();
    let matches = project
        .element_matches(&DocumentTarget::page(project.home_page_id), &link(&project, 0, 0))
        .unwrap();
    let documents = matches
        .iter()
        .map(|item| item.document.clone())
        .collect::<Vec<_>>();
    assert_eq!(documents.len(), 3, "two pages and the framed file: {documents:?}");
    assert!(
        matches
            .iter()
            .any(|item| item.document == DocumentTarget::SiteFile { path: SITE.into() })
    );
    assert!(
        matches.iter().all(|item| item.identical_html),
        "identical markup is preferred and flagged"
    );
    assert_eq!(
        matches.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(),
        ["About", "Project one", SITE],
        "pages beside the reference come before other folders"
    );
}

#[test]
fn a_style_change_reaches_every_matched_page_in_one_history_step() {
    let project = project();
    let mut session = Session::new(project.clone()).unwrap();
    let before = session.history();
    let mut writes = 0;
    let changes = session
        .edit_element_across_documents(
            project.revision,
            &AcrossPagesEdit {
                document: DocumentTarget::page(project.home_page_id),
                target: link(&project, 0, 0),
                edit: style_edit("color: red !important;"),
                documents: vec![],
            },
            |_| {
                writes += 1;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(changes.len(), 4, "home, two pages and the framed file");
    assert_eq!(writes, 1, "the whole change is one durable write");
    assert_eq!(session.history().2, before.2 + 1, "one history step");
    for change in &changes {
        let html = session.project().document_html(&change.document).unwrap();
        assert_eq!(html.matches("color: red").count(), 1, "{html}");
    }
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(
        session
            .project()
            .document_html(&DocumentTarget::page(project.home_page_id))
            .unwrap(),
        page_html(&project, 0)
    );
    assert_eq!(
        session
            .project()
            .document_html(&DocumentTarget::SiteFile { path: SITE.into() })
            .unwrap(),
        project.site_document_html(SITE).unwrap()
    );
}

#[test]
fn a_style_change_keeps_each_pages_own_inline_declarations() {
    let mut project = project();
    project.pages[2].content = Some(content(&format!(
        "<div id=\"app\">{}</div>",
        HEADER.replace("<nav class=\"main\">", "<nav class=\"main\" style=\"display: flex;\">")
    )));
    let (next, _) = project
        .edit_element_across(
            project.revision,
            &AcrossPagesEdit {
                document: DocumentTarget::page(project.home_page_id),
                target: link(&project, 0, 0),
                edit: style_edit("color: red !important;"),
                documents: vec![],
            },
        )
        .unwrap();
    let about = next.document_html(&DocumentTarget::page(project.pages[2].id)).unwrap();
    assert!(about.contains("display: flex"), "the other page keeps its own style: {about}");
    assert!(about.contains("color: red"), "{about}");
    let home = next.document_html(&DocumentTarget::page(project.home_page_id)).unwrap();
    assert_eq!(home.matches("color: red").count(), 1, "{home}");
}

#[test]
fn text_changes_only_where_the_old_text_was_identical() {
    let mut project = project();
    project.pages[1].content = Some(content(&format!(
        "<header class=\"site\"><nav class=\"main\"><a class=\"link\" href=\"/\">Start</a><a class=\"link\" href=\"/about\">About</a></nav></header><main><h1>One</h1></main>"
    )));
    let (next, changes) = project
        .edit_element_across(
            project.revision,
            &AcrossPagesEdit {
                document: DocumentTarget::page(project.home_page_id),
                target: link(&project, 0, 0),
                edit: text_edit("Start"),
                documents: vec![],
            },
        )
        .unwrap();
    assert!(
        !changes
            .iter()
            .any(|change| change.document == DocumentTarget::page(project.pages[1].id)),
        "the page whose old text differed is left alone: {changes:?}"
    );
    assert_eq!(
        next.document_html(&DocumentTarget::page(project.pages[1].id)).unwrap(),
        page_html(&project, 1)
    );
    for document in [
        DocumentTarget::page(project.home_page_id),
        DocumentTarget::page(project.pages[2].id),
        DocumentTarget::SiteFile { path: SITE.into() },
    ] {
        assert!(
            next.document_html(&document).unwrap().contains(">Start</a>"),
            "{document:?}"
        );
    }
}

#[test]
fn unticked_documents_stay_unchanged() {
    let project = project();
    let keep = DocumentTarget::page(project.pages[2].id);
    let (next, changes) = project
        .edit_element_across(
            project.revision,
            &AcrossPagesEdit {
                document: DocumentTarget::page(project.home_page_id),
                target: link(&project, 0, 0),
                edit: ElementEdit::Update {
                    texts: vec![],
                    attributes: BTreeMap::from([("title".into(), Some("All pages".into()))]),
                    style: None,
                },
                documents: vec![keep.clone()],
            },
        )
        .unwrap();
    assert_eq!(changes.len(), 2, "the reference and the ticked page");
    assert_eq!(changes[0].document, DocumentTarget::page(project.home_page_id));
    assert_eq!(changes[1].document, keep);
    assert_eq!(
        next.document_html(&DocumentTarget::SiteFile { path: SITE.into() }).unwrap(),
        project.site_document_html(SITE).unwrap()
    );
    assert_eq!(
        next.document_html(&DocumentTarget::page(project.pages[1].id)).unwrap(),
        page_html(&project, 1)
    );
}

#[test]
fn duplicate_and_delete_reach_matched_elements() {
    let project = project();
    let (duplicated, changes) = project
        .edit_element_across(
            project.revision,
            &AcrossPagesEdit {
                document: DocumentTarget::page(project.home_page_id),
                target: link(&project, 0, 0),
                edit: ElementEdit::Duplicate,
                documents: vec![],
            },
        )
        .unwrap();
    assert_eq!(changes.len(), 4);
    assert_eq!(
        duplicated
            .document_html(&DocumentTarget::page(project.home_page_id))
            .unwrap()
            .matches("class=\"link\"")
            .count(),
        3,
        "a copy beside the original"
    );
    let reference = changes[0].target.clone();
    let (removed, changes) = duplicated
        .edit_element_across(
            duplicated.revision,
            &AcrossPagesEdit {
                document: DocumentTarget::page(project.home_page_id),
                target: reference,
                edit: ElementEdit::Remove,
                documents: vec![],
            },
        )
        .unwrap();
    assert_eq!(changes.len(), 4);
    assert!(changes.iter().all(|change| change.removed));
    assert_eq!(
        removed
            .document_html(&DocumentTarget::page(project.home_page_id))
            .unwrap()
            .matches("class=\"link\"")
            .count(),
        2
    );
}

#[test]
fn an_extra_wrapper_above_the_shared_layout_still_matches_but_another_layout_does_not() {
    let project = project();
    let matches = project
        .element_matches(&DocumentTarget::page(project.pages[2].id), &link(&project, 2, 0))
        .unwrap();
    assert!(
        matches
            .iter()
            .any(|item| item.document == DocumentTarget::page(project.home_page_id)),
        "the wrapper is above the shared header: {matches:?}"
    );

    let mut footer = project.clone();
    footer.pages[0].content = Some(content(
        "<header class=\"site\"><nav class=\"main\"><a class=\"link\" href=\"/\">Home</a></nav></header><footer><nav class=\"main\"><a class=\"link\" href=\"/\">Elsewhere</a></nav></footer>",
    ));
    let footer_target = ElementTarget {
        id: String::new(),
        selector: "body > footer:nth-of-type(1) > nav:nth-of-type(1) > a:nth-of-type(1)".into(),
        tag: "a".into(),
        label: String::new(),
        source_sha256: footer.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    };
    let matches = footer
        .element_matches(&DocumentTarget::page(footer.home_page_id), &footer_target)
        .unwrap();
    assert!(
        matches.is_empty(),
        "nothing shares the footer > nav.main position: {matches:?}"
    );
}

#[test]
fn a_duplicate_keeps_anchor_ids_unique() {
    let mut project = Project::new(
        "Duplicate".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    project.pages[0].content = Some(content(
        "<section id=\"intro\"><a id=\"top\" href=\"#top\">Home</a></section>",
    ));
    let target = ElementTarget {
        id: String::new(),
        selector: "body > section:nth-of-type(1)".into(),
        tag: "section".into(),
        label: String::new(),
        source_sha256: project.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    };
    let (next, _) = project
        .edit_element(project.revision, project.home_page_id, &target, &ElementEdit::Duplicate)
        .unwrap();
    let html = next.document_html(&DocumentTarget::page(project.home_page_id)).unwrap();
    assert_eq!(html.matches("id=\"intro\"").count(), 1, "{html}");
    assert_eq!(html.matches("id=\"top\"").count(), 1, "{html}");
    assert_eq!(html.matches("id=\"desegnos-copy-").count(), 2, "{html}");
    assert_eq!(html.matches("href=\"#top\"").count(), 1, "{html}");
    assert_eq!(html.matches("href=\"#desegnos-copy-").count(), 1, "the copy points at its own anchor: {html}");
}

#[test]
fn a_framed_site_document_matches_the_pages_that_share_its_layout() {
    let project = project();
    let document = DocumentTarget::SiteFile { path: SITE.into() };
    let reference = ElementTarget {
        id: String::new(),
        selector: "body > header:nth-of-type(1) > nav:nth-of-type(1) > a:nth-of-type(1)".into(),
        tag: "a".into(),
        label: String::new(),
        source_sha256: project.document_hash(&document).unwrap(),
    };
    let matches = project.element_matches(&document, &reference).unwrap();
    assert_eq!(matches.len(), 3, "every page with the shared header: {matches:?}");
    assert!(matches
        .iter()
        .all(|item| item.document != document && item.identical_html));
    let (next, changes) = project
        .edit_element_across(
            project.revision,
            &AcrossPagesEdit {
                document,
                target: reference,
                edit: style_edit("font-weight: 700 !important;"),
                documents: vec![],
            },
        )
        .unwrap();
    assert_eq!(changes.len(), 4);
    for change in changes {
        assert!(next
            .document_html(&change.document)
            .unwrap()
            .contains("font-weight: 700"));
    }
}
