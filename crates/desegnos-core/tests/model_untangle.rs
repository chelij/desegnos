use desegnos_core::{
    Appearance, Command, ElementKind, PendingRequest, Project, Rect, RequestHistoryOutcome,
    RequestTarget, Session, Viewport, earlier_requests, empty_element,
};
use uuid::Uuid;

fn project() -> Project {
    Project::new(
        "History".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap()
}

#[test]
fn request_history_uses_frozen_fields_even_when_markdown_changes() {
    let mut project = project();
    let element = empty_element(
        ElementKind::Box,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
    );
    let element_id = element.id;
    let page_id = project.pages[0].id;
    project.pages[0].elements.push(element);
    project.requests.push(PendingRequest {
        id: Uuid::new_v4(),
        text: "Use this exact text: ```json\n{}\n```".into(),
        targets: vec![RequestTarget::Element {
            page: page_id,
            id: element_id,
            size: None,
        }],
        attachments: vec![],
        area: None,
        requested_at: "2026-11-07T00:00:00Z".into(),
        boundary_retry_order: vec![],
    });
    let mut snapshot = project.prepare_requirements().unwrap();
    snapshot.markdown.clear();
    let records = earlier_requests(&snapshot, 10, RequestHistoryOutcome::Applied, "Done", &[]);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].element_id, element_id);
    assert_eq!(records[0].request, project.requests[0].text);
    assert_eq!(records[0].summary_targets, vec![(page_id, element_id)]);
}

#[test]
fn invalid_picture_preparation_returns_an_error() {
    assert!(Project::prepare_picture("data:image/png;base64,AAAA", 1440).is_err());
    assert!(
        desegnos_core::limits::decode_image_data_uri("data:image/png;base64,/9j/2Q==").is_err()
    );
}

#[test]
fn colour_scheme_handles_transparent_and_invalid_backgrounds() {
    let mut project = project();
    for background in ["transparent", "invalid", "#00", "é"] {
        project.pages[0].background = background.into();
        assert_eq!(project.pages[0].color_scheme(), "light");
    }
    project.pages[0].background = "#000".into();
    assert_eq!(project.pages[0].color_scheme(), "dark");
}

#[test]
fn failed_durable_write_preserves_history_and_project() {
    let mut session = Session::new(project()).unwrap();
    let before = session.project().clone();
    let history = session.history();
    let result = session.execute(
        before.revision,
        Command::RenameProject {
            name: "After".into(),
        },
        |_| Err(desegnos_core::Error("Cannot save".into())),
    );
    assert!(result.is_err());
    assert_eq!(session.project(), &before);
    assert_eq!(session.history(), history);
}

#[test]
fn restoring_a_version_carries_the_latest_export_metadata() {
    use desegnos_core::{ExportRecord, ExportedPage, OutputType};
    let original = project();
    let mut session = Session::new(original.clone()).unwrap();
    let record = ExportRecord {
        id: Uuid::new_v4(),
        project_id: original.id,
        version: 1,
        destination: "/tmp/desegnos-export".into(),
        exported_at: "2026-11-07T00:00:00Z".into(),
        pages: vec![ExportedPage {
            page_id: original.home_page_id,
            path: "/".into(),
            output: OutputType::Webpage,
        }],
        pointers: Default::default(),
        behaviours: Default::default(),
    };
    session.record_export(record.clone(), |_| Ok(())).unwrap();
    session.restore(original, |_| Ok(())).unwrap();
    assert_eq!(session.project().last_export.as_ref(), Some(&record));
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().last_export.as_ref(), Some(&record));
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().last_export.as_ref(), Some(&record));
}
