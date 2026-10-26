use desegnos_core::*;
use std::collections::BTreeMap;

const HEADING: &str = "headingaaaa1";
const NOTE: &str = "noteaaaaaaa1";

fn session() -> Session {
    let mut project = Project::new(
        "Element undo".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap();
    project.pages[0].content = Some(SiteContent {
        html: format!(
            "<main><h1 data-desegnos-id=\"{HEADING}\">First</h1><p data-desegnos-id=\"{NOTE}\">Note</p></main>"
        ),
        css: "h1{color:black}".into(),
        javascript: String::new(),
    });
    Session::new(project).unwrap()
}

fn target(id: &str, tag: &str) -> ElementTarget {
    ElementTarget {
        id: id.into(),
        selector: format!("body > main:nth-of-type(1) > {tag}:nth-of-type(1)"),
        tag: tag.into(),
        label: "Selected".into(),
        source_sha256: String::new(),
    }
}

fn set_text(session: &mut Session, id: &str, tag: &str, text: &str) {
    let page = session.project().home_page_id;
    let change = ElementEdit::Update {
        texts: vec![TextEdit {
            index: 0,
            text: text.into(),
        }],
        attributes: BTreeMap::new(),
        style: None,
    };
    session
        .edit_element(
            session.project().revision,
            page,
            &target(id, tag),
            &change,
            |_| Ok(()),
        )
        .unwrap();
}

fn html(session: &Session) -> String {
    session.project().pages[0]
        .content
        .as_ref()
        .unwrap()
        .html
        .clone()
}

fn undo_heading(session: &mut Session) -> Result<Option<ElementTarget>> {
    let page = session.project().home_page_id;
    session.undo_element(
        session.project().revision,
        page,
        &target(HEADING, "h1"),
        |_| Ok(()),
    )
}

#[test]
fn each_press_steps_one_element_further_back_and_keeps_other_changes() {
    let mut s = session();
    set_text(&mut s, HEADING, "h1", "Second");
    set_text(&mut s, NOTE, "p", "Changed note");
    set_text(&mut s, HEADING, "h1", "Third");
    // Each press walks versions from separate edits, not one merged typing burst.
    s.end_history_step();
    set_text(&mut s, HEADING, "h1", "Fourth");

    undo_heading(&mut s).unwrap();
    assert!(html(&s).contains(">Third</h1>"), "{}", html(&s));
    assert!(
        html(&s).contains(">Changed note</p>"),
        "other elements keep their changes"
    );
    undo_heading(&mut s).unwrap();
    assert!(html(&s).contains(">Second</h1>"), "{}", html(&s));
    undo_heading(&mut s).unwrap();
    assert!(html(&s).contains(">First</h1>"), "{}", html(&s));
    assert!(html(&s).contains(">Changed note</p>"));
    assert!(html(&s).contains(&format!("data-desegnos-id=\"{HEADING}\"")));
    let error = undo_heading(&mut s).unwrap_err();
    assert_eq!(error.0, "This element has no earlier version.");
    assert_eq!(
        s.project().pages[0].content.as_ref().unwrap().css,
        "h1{color:black}"
    );
}

#[test]
fn element_undo_is_an_ordinary_edit_that_the_main_undo_reverts() {
    let mut s = session();
    set_text(&mut s, HEADING, "h1", "Second");
    set_text(&mut s, NOTE, "p", "Changed note");
    undo_heading(&mut s).unwrap();
    assert!(html(&s).contains(">First</h1>") && html(&s).contains(">Changed note</p>"));
    s.undo(|_| Ok(())).unwrap();
    assert!(html(&s).contains(">Second</h1>") && html(&s).contains(">Changed note</p>"));
    // After another edit a new walk starts from the current version.
    set_text(&mut s, HEADING, "h1", "Again");
    undo_heading(&mut s).unwrap();
    assert!(html(&s).contains(">Second</h1>"), "{}", html(&s));
}

#[test]
fn an_element_without_earlier_versions_says_so() {
    let mut s = session();
    set_text(&mut s, NOTE, "p", "Changed note");
    assert_eq!(
        undo_heading(&mut s).unwrap_err().0,
        "This element has no earlier version."
    );
}
