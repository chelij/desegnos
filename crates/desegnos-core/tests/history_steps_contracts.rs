use desegnos_core::*;
use std::collections::BTreeMap;

const HEADING: &str = "headingaaaa1";
const NOTE: &str = "noteaaaaaaa1";

fn project() -> Project {
    let mut project = Project::new(
        "History steps".into(),
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
    project
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

fn remove(session: &mut Session, id: &str, tag: &str) {
    let page = session.project().home_page_id;
    session
        .edit_element(
            session.project().revision,
            page,
            &target(id, tag),
            &ElementEdit::Remove,
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

fn css(session: &Session) -> String {
    session.project().pages[0]
        .content
        .as_ref()
        .unwrap()
        .css
        .clone()
}

fn style(session: &mut Session, id: &str, tag: &str, style: &str) {
    let page = session.project().home_page_id;
    let change = ElementEdit::Update {
        texts: vec![],
        attributes: BTreeMap::new(),
        style: Some(style.into()),
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

#[test]
fn consecutive_text_edits_to_one_element_are_one_history_step() {
    let mut session = Session::new(project()).unwrap();
    set_text(&mut session, HEADING, "h1", "Second");
    let after_first = session.history().2;
    assert_eq!(after_first, 1);
    set_text(&mut session, HEADING, "h1", "Third");
    assert_eq!(
        session.history().2,
        after_first,
        "the second keystroke replaces the same step"
    );
    assert!(html(&session).contains(">Third</h1>"));
    session.undo(|_| Ok(())).unwrap();
    assert!(html(&session).contains(">First</h1>"), "{}", html(&session));
}

#[test]
fn text_and_style_edits_to_one_element_share_a_step() {
    let mut session = Session::new(project()).unwrap();
    set_text(&mut session, HEADING, "h1", "Second");
    style(&mut session, HEADING, "h1", "letter-spacing:0.02em;");
    assert_eq!(session.history().2, 1);
    assert!(html(&session).contains(">Second</h1>"));
    assert!(html(&session).contains("letter-spacing:0.02em"));
    session.undo(|_| Ok(())).unwrap();
    assert!(html(&session).contains(">First</h1>"));
    assert!(!html(&session).contains("letter-spacing"));
}

#[test]
fn edits_to_two_different_elements_are_two_history_steps() {
    let mut session = Session::new(project()).unwrap();
    set_text(&mut session, HEADING, "h1", "Second");
    set_text(&mut session, NOTE, "p", "Changed note");
    assert_eq!(session.history().2, 2);
    session.undo(|_| Ok(())).unwrap();
    assert!(html(&session).contains(">Second</h1>"));
    assert!(html(&session).contains(">Note</p>"));
    session.undo(|_| Ok(())).unwrap();
    assert!(html(&session).contains(">First</h1>"));
}

#[test]
fn alternating_prompt_typing_and_element_edits_stay_separate_steps() {
    let mut session = Session::new(project()).unwrap();
    let home = session.project().home_page_id;
    session
        .execute(
            0,
            Command::AddRegion {
                page_id: home,
                rect: Rect {
                    x: 0.,
                    y: 0.,
                    width: 200.,
                    height: 100.,
                },
            },
            |_| Ok(()),
        )
        .unwrap();
    let region_id = session.project().pages[0].regions[0].id;
    let rect = session.project().pages[0].regions[0].rect;
    let typed = |session: &mut Session, prompt: &str| {
        let revision = session.project().revision;
        session
            .execute(
                revision,
                Command::UpdateRegion {
                    page_id: home,
                    region_id,
                    rect,
                    prompt: prompt.into(),
                },
                |_| Ok(()),
            )
            .unwrap();
    };
    typed(&mut session, "make it calmer");
    set_text(&mut session, HEADING, "h1", "Second");
    typed(&mut session, "make it calmer still");
    assert_eq!(session.history().2, 4, "box, prompt, element, prompt");
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(
        session.project().pages[0].regions[0].prompt,
        "make it calmer"
    );
    assert!(html(&session).contains(">Second</h1>"));
    session.undo(|_| Ok(())).unwrap();
    assert!(html(&session).contains(">First</h1>"));
    assert_eq!(
        session.project().pages[0].regions[0].prompt,
        "make it calmer"
    );
}

#[test]
fn ending_a_step_splits_a_burst_of_typing() {
    let mut session = Session::new(project()).unwrap();
    let home = session.project().home_page_id;
    session
        .execute(
            0,
            Command::AddRegion {
                page_id: home,
                rect: Rect {
                    x: 0.,
                    y: 0.,
                    width: 200.,
                    height: 100.,
                },
            },
            |_| Ok(()),
        )
        .unwrap();
    let region_id = session.project().pages[0].regions[0].id;
    let rect = session.project().pages[0].regions[0].rect;
    let typed = |session: &mut Session, prompt: &str| {
        let revision = session.project().revision;
        session
            .execute(
                revision,
                Command::UpdateRegion {
                    page_id: home,
                    region_id,
                    rect,
                    prompt: prompt.into(),
                },
                |_| Ok(()),
            )
            .unwrap();
    };
    typed(&mut session, "make");
    session.end_history_step();
    typed(&mut session, "make it calmer");
    assert_eq!(session.history().2, 3, "box, first burst, second burst");
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages[0].regions[0].prompt, "make");
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages[0].regions[0].prompt, "");
}

#[test]
fn a_remove_is_always_its_own_step() {
    let mut session = Session::new(project()).unwrap();
    set_text(&mut session, HEADING, "h1", "Second");
    remove(&mut session, HEADING, "h1");
    assert_eq!(session.history().2, 2, "Remove does not join the text step");
    assert!(!html(&session).contains(">Second</h1>"));
    remove(&mut session, NOTE, "p");
    assert_eq!(session.history().2, 3);
    session.undo(|_| Ok(())).unwrap();
    assert!(html(&session).contains(">Note</p>"));
    session.undo(|_| Ok(())).unwrap();
    assert!(html(&session).contains(">Second</h1>"));
}

#[test]
fn page_source_edits_to_one_page_merge_into_one_step() {
    let mut session = Session::new(project()).unwrap();
    let page_id = session.project().home_page_id;
    session
        .edit_page_source(
            0,
            page_id,
            &PageSourceEdit {
                css: Some("h1{color:red}".into()),
                ..PageSourceEdit::default()
            },
            |_| Ok(()),
        )
        .unwrap();
    let revision = session.project().revision;
    session
        .edit_page_source(
            revision,
            page_id,
            &PageSourceEdit {
                css: Some("h1{color:blue}".into()),
                ..PageSourceEdit::default()
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(session.history().2, 1);
    assert_eq!(css(&session), "h1{color:blue}");
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(css(&session), "h1{color:black}");
}

#[test]
fn page_settings_tweaks_to_one_page_merge_into_one_step() {
    let mut session = Session::new(project()).unwrap();
    let page = session.project().pages[0].clone();
    let tweak = |width: u32| Command::UpdatePage {
        page_id: page.id,
        name: page.name.clone(),
        path: page.path.clone(),
        viewport: Viewport {
            width,
            height: page.viewport.height,
        },
        appearance: page.appearance,
        included: page.included,
    };
    session.execute(0, tweak(1200), |_| Ok(())).unwrap();
    let revision = session.project().revision;
    session.execute(revision, tweak(1000), |_| Ok(())).unwrap();
    assert_eq!(session.history().2, 1);
    assert_eq!(session.project().pages[0].viewport.width, 1000);
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages[0].viewport.width, 1440);
}
