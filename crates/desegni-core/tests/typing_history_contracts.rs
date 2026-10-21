use desegni_core::*;

#[test]
fn a_burst_of_typing_in_one_request_is_one_history_step() {
    let project = Project::new(
        "Typing".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    let home = project.home_page_id;
    let rect = Rect {
        x: 0.,
        y: 0.,
        width: 200.,
        height: 100.,
    };
    let mut session = Session::new(project).unwrap();
    let ok = |_: &Project| Ok(());
    let revision = session.project().revision;
    session
        .execute(
            revision,
            Command::AddRegion {
                page_id: home,
                rect,
            },
            ok,
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
                ok,
            )
            .unwrap();
    };
    for prompt in ["m", "ma", "mak", "make it calmer"] {
        typed(&mut session, prompt);
    }
    let prompt = |session: &Session| session.project().pages[0].regions[0].prompt.clone();
    assert_eq!(prompt(&session), "make it calmer");
    // One Undo removes the whole burst; the box itself is the step before.
    session.undo(ok).unwrap();
    assert_eq!(prompt(&session), "");
    assert_eq!(session.project().pages[0].regions.len(), 1);
    session.redo(ok).unwrap();
    assert_eq!(prompt(&session), "make it calmer");
    // After Redo, new typing starts a new step.
    typed(&mut session, "make it calmer!");
    session.undo(ok).unwrap();
    assert_eq!(prompt(&session), "make it calmer");
}
