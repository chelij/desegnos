//! Named views: one undo step per command, validated names and cameras, and project storage.
use desegnos_core::{Appearance, Command, Project, Viewport};
use serde_json::json;

fn project() -> Project {
    Project::new(
        "Views".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap()
}

fn apply(project: &Project, command: Command) -> Project {
    project.apply(project.revision, command).unwrap()
}

fn add(project: &Project, name: &str, x: f64, y: f64, zoom: f64) -> Project {
    apply(
        project,
        Command::AddNamedView {
            name: name.into(),
            x,
            y,
            zoom,
        },
    )
}

#[test]
fn add_update_and_remove_a_named_view() {
    let start = project();
    assert!(start.views.is_empty());

    let added = add(&start, "  Header  ", 12.5, -40.0, 1.5);
    assert_eq!(added.revision, start.revision + 1);
    assert_eq!(added.views.len(), 1);
    let view = &added.views[0];
    assert_eq!(view.name, "Header");
    assert_eq!((view.x, view.y, view.zoom), (12.5, -40.0, 1.5));
    assert!(!view.id.is_nil());

    let renamed = apply(
        &added,
        Command::UpdateNamedView {
            view_id: view.id,
            name: Some("  Top of page  ".into()),
            x: None,
            y: None,
            zoom: None,
        },
    );
    assert_eq!(renamed.revision, added.revision + 1);
    assert_eq!(renamed.views[0].name, "Top of page");
    assert_eq!(
        (
            renamed.views[0].x,
            renamed.views[0].y,
            renamed.views[0].zoom
        ),
        (12.5, -40.0, 1.5)
    );

    let moved = apply(
        &renamed,
        Command::UpdateNamedView {
            view_id: view.id,
            name: None,
            x: Some(-320.0),
            y: Some(64.0),
            zoom: Some(0.5),
        },
    );
    assert_eq!(moved.views[0].name, "Top of page");
    assert_eq!(
        (moved.views[0].x, moved.views[0].y, moved.views[0].zoom),
        (-320.0, 64.0, 0.5)
    );

    let removed = apply(&moved, Command::RemoveNamedView { view_id: view.id });
    assert!(removed.views.is_empty());
    assert_eq!(removed.revision, moved.revision + 1);
}

#[test]
fn view_commands_reject_invalid_names_cameras_and_missing_views() {
    let start = project();
    let error = |command: Command| start.apply(start.revision, command).unwrap_err();

    for name in [String::new(), "   ".to_string(), "x".repeat(81)] {
        assert!(
            error(Command::AddNamedView {
                name,
                x: 0.0,
                y: 0.0,
                zoom: 1.0,
            })
            .0
            .contains("View name")
        );
    }
    for (x, y, zoom) in [
        (f64::NAN, 0.0, 1.0),
        (0.0, f64::INFINITY, 1.0),
        (0.0, 0.0, f64::NAN),
        (0.0, 0.0, 0.0),
        (0.0, 0.0, -1.0),
        (0.0, 0.0, 1001.0),
    ] {
        let message = error(Command::AddNamedView {
            name: "Broken".into(),
            x,
            y,
            zoom,
        })
        .0;
        assert!(
            message.contains("camera") || message.contains("zoom"),
            "{message}"
        );
    }

    let missing = uuid::Uuid::new_v4();
    assert!(
        error(Command::UpdateNamedView {
            view_id: missing,
            name: Some("Gone".into()),
            x: None,
            y: None,
            zoom: None,
        })
        .0
        .contains("no longer exists")
    );
    assert!(
        error(Command::RemoveNamedView { view_id: missing })
            .0
            .contains("no longer exists")
    );

    let added = add(&start, "One", 0.0, 0.0, 1.0);
    let id = added.views[0].id;
    assert!(
        added
            .apply(
                added.revision,
                Command::UpdateNamedView {
                    view_id: id,
                    name: Some("   ".into()),
                    x: None,
                    y: None,
                    zoom: None,
                },
            )
            .unwrap_err()
            .0
            .contains("View name")
    );
    assert!(
        added
            .apply(
                added.revision,
                Command::UpdateNamedView {
                    view_id: id,
                    name: Some("x".repeat(81)),
                    x: None,
                    y: None,
                    zoom: None,
                },
            )
            .unwrap_err()
            .0
            .contains("View name")
    );
    assert!(
        added
            .apply(
                added.revision,
                Command::UpdateNamedView {
                    view_id: id,
                    name: None,
                    x: Some(f64::NAN),
                    y: None,
                    zoom: None,
                },
            )
            .unwrap_err()
            .0
            .contains("camera")
    );
}

#[test]
fn a_project_holds_at_most_fifty_views() {
    let mut project = project();
    for index in 0..50 {
        project = add(
            &project,
            &format!("View {}", index + 1),
            index as f64,
            0.0,
            1.0,
        );
    }
    assert_eq!(project.views.len(), 50);

    let refused = project.apply(
        project.revision,
        Command::AddNamedView {
            name: "View 51".into(),
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        },
    );
    assert!(refused.unwrap_err().0.contains("at most 50 named views"));
    assert_eq!(project.views.len(), 50);
}

#[test]
fn views_round_trip_and_default_to_empty_when_missing() {
    let project = add(&project(), "Header", 10.0, 20.0, 2.0);
    let value = serde_json::to_value(&project).unwrap();
    assert_eq!(
        value["views"],
        json!([{
            "id": project.views[0].id,
            "name": "Header",
            "x": 10.0,
            "y": 20.0,
            "zoom": 2.0,
        }])
    );

    let mut without = value;
    without.as_object_mut().unwrap().remove("views");
    let parsed: Project = serde_json::from_value(without).unwrap();
    assert!(parsed.views.is_empty());
}
