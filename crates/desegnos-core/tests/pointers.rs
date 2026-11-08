//! Pointer endpoints, clash states and export fingerprints.
use desegnos_core::{
    Appearance, Command, Element, ElementKind, ExportRecord, ExportedPage, ExportedPointer,
    Pointer, PointerSource, PointerState, PointerTarget, Project, Rect, ReferenceItem,
    ReferenceKind, Viewport, empty_element, pointer_fingerprints,
};
use std::collections::BTreeMap;
use uuid::Uuid;

fn project() -> Project {
    Project::new(
        "Pointers".into(),
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

fn element(id: Uuid, value: &str) -> Element {
    let mut element = empty_element(
        ElementKind::Text,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 24.0,
        },
    );
    element.id = id;
    element.text = value.into();
    element
}

fn pointer(
    id: Uuid,
    page: Uuid,
    element_id: Uuid,
    target: PointerTarget,
    comment: &str,
) -> Pointer {
    Pointer {
        id,
        from: PointerSource::Element {
            page_id: page,
            element_id,
            size_id: None,
        },
        to: target,
        comment: comment.into(),
        kind: desegnos_core::PointerKind::Reference,
        wiring: None,
        play: None,
        play_comment: String::new(),
    }
}

fn with_element() -> (Project, Uuid, Uuid) {
    let element_id = Uuid::new_v4();
    let start = project();
    let page = start.home_page_id;
    let project = apply(
        &start,
        Command::InsertElements {
            page_id: page,
            parent_id: None,
            index: 0,
            elements: vec![element(element_id, "Trigger")],
            picture_data_uri: None,
            size_id: None,
        },
    );
    (project, page, element_id)
}

#[test]
fn broken_detection_survives_deletion_and_reference_cards() {
    let (project, page, element_id) = with_element();
    let pointer_id = Uuid::new_v4();
    let link = ReferenceItem {
        id: Uuid::new_v4(),
        rect: Rect {
            x: -300.0,
            y: 0.0,
            width: 200.0,
            height: 140.0,
        },
        kind: ReferenceKind::Link {
            url: "https://example.com/read".into(),
            title: "Read".into(),
        },
    };
    let url_id = Uuid::new_v4();
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: pointer(
                pointer_id,
                page,
                element_id,
                PointerTarget::Boundary {
                    page_id: page,
                    size_id: None,
                },
                "Changes this boundary",
            ),
        },
    );
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: pointer(
                url_id,
                page,
                element_id,
                PointerTarget::Url {
                    url: "https://example.com/read".into(),
                },
                "Reads this page",
            ),
        },
    );
    assert!(broken_pointers(&project).contains(&url_id));

    let project = apply(
        &project,
        Command::AddReferenceItem {
            item: link,
            picture_data_uri: None,
        },
    );
    assert!(!broken_pointers(&project).contains(&url_id));
    assert!(
        !project
            .pointer_states()
            .iter()
            .find(|state| state.id == url_id)
            .unwrap()
            .state
            .eq(&PointerState::Broken)
    );

    let removed = apply(
        &project,
        Command::RemoveElements {
            page_id: page,
            ids: vec![element_id],
        },
    );
    let states = removed.pointer_states();
    assert_eq!(
        states
            .iter()
            .find(|state| state.id == pointer_id)
            .unwrap()
            .state,
        PointerState::Broken
    );
    assert_eq!(
        states
            .iter()
            .find(|state| state.id == url_id)
            .unwrap()
            .state,
        PointerState::Broken
    );
    assert_eq!(broken_pointers(&removed).len(), 2);
}

#[test]
fn pointers_from_one_source_with_different_comments_clash() {
    let (project, page, element_id) = with_element();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let same = Uuid::new_v4();
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: pointer(
                first,
                page,
                element_id,
                PointerTarget::Boundary {
                    page_id: page,
                    size_id: None,
                },
                "Show the menu",
            ),
        },
    );
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: pointer(
                second,
                page,
                element_id,
                PointerTarget::Boundary {
                    page_id: page,
                    size_id: None,
                },
                "Hide the menu",
            ),
        },
    );
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: pointer(
                same,
                page,
                element_id,
                PointerTarget::Boundary {
                    page_id: page,
                    size_id: None,
                },
                "Show the menu",
            ),
        },
    );
    let states = project.pointer_states();
    let first_state = states.iter().find(|state| state.id == first).unwrap();
    let second_state = states.iter().find(|state| state.id == second).unwrap();
    let same_state = states.iter().find(|state| state.id == same).unwrap();
    assert_eq!(first_state.state, PointerState::Contradicts);
    assert_eq!(second_state.state, PointerState::Contradicts);
    assert_eq!(same_state.state, PointerState::Contradicts);
    assert_eq!(first_state.clash_with, vec![second]);
    assert_eq!(same_state.clash_with, vec![second]);
    assert_eq!(second_state.clash_with, vec![first, same]);
    assert_eq!(
        states
            .iter()
            .filter(|state| state.state == PointerState::Contradicts)
            .count(),
        3
    );

    // Resolving the clash merges the comment and drops the other pointer.
    let resolved = apply(
        &project,
        Command::ResolvePointerClash {
            keep: first,
            drop: vec![second, same],
            merged_comment: Some("Toggles the menu".into()),
        },
    );
    assert_eq!(resolved.pointers.len(), 1);
    let kept = resolved
        .pointers
        .iter()
        .find(|item| item.id == first)
        .unwrap();
    assert_eq!(kept.comment, "Toggles the menu");
    assert!(
        resolved
            .pointer_states()
            .iter()
            .all(|state| state.state != PointerState::Contradicts)
    );
}

#[test]
fn states_distinguish_changed_code_changed_comments_and_wired_pointers() {
    let (project, page, element_id) = with_element();
    let pointer_id = Uuid::new_v4();
    let target = PointerTarget::Boundary {
        page_id: page,
        size_id: None,
    };
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: pointer(
                pointer_id,
                page,
                element_id,
                target.clone(),
                "Open the menu",
            ),
        },
    );
    let fingerprint = "a".repeat(64);
    let record = ExportRecord {
        id: Uuid::new_v4(),
        project_id: project.id,
        version: 1,
        destination: "/tmp/export".into(),
        behaviours: BTreeMap::new(),
        exported_at: "2026-10-27T00:00:00Z".into(),
        pages: vec![ExportedPage {
            page_id: page,
            path: "/".into(),
            output: desegnos_core::OutputType::Webpage,
        }],
        pointers: BTreeMap::from([(
            pointer_id,
            ExportedPointer {
                fingerprint: fingerprint.clone(),
                comment: "Open the menu".into(),
            },
        )]),
    };
    let project = apply(&project, Command::RecordExport { record });

    let state = |project: &Project, current: &BTreeMap<Uuid, String>| {
        project
            .pointer_states_with_fingerprints(current)
            .into_iter()
            .find(|state| state.id == pointer_id)
            .unwrap()
            .state
    };
    assert_eq!(state(&project, &BTreeMap::new()), PointerState::Wired);
    assert_eq!(
        state(&project, &BTreeMap::from([(pointer_id, "b".repeat(64))])),
        PointerState::CodeChanged
    );

    let changed = apply(
        &project,
        Command::UpdatePointer {
            pointer_id,
            from: None,
            to: None,
            comment: Some("Close the menu".into()),
            kind: None,
        },
    );
    assert_eq!(
        state(&changed, &BTreeMap::from([(pointer_id, fingerprint)])),
        PointerState::NeedsExporting
    );
}

#[test]
fn fingerprints_cover_marked_fragments_and_ignore_whitespace_edit_distance() {
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let files = BTreeMap::from([
        (
            "index.html".to_string(),
            format!(
                "<!doctype html><html><body><div data-desegnos-pointer=\"{first}\"><p>A</p></div><button data-desegnos-pointer=\"{second}\">B</button></body></html>"
            )
            .into_bytes(),
        ),
        (
            "style.css".to_string(),
            format!(
                "/* desegnos-pointer:{first} */\n.a {{ color: red; }}\n/* /desegnos-pointer:{first} */\n"
            )
            .into_bytes(),
        ),
    ]);
    let fingerprints = pointer_fingerprints(&files);
    assert!(fingerprints.contains_key(&first));
    assert!(fingerprints.contains_key(&second));
    assert_eq!(fingerprints[&first].len(), 64);

    let spaced = BTreeMap::from([
        (
            "index.html".to_string(),
            format!(
                "<!doctype html><html><body><div  data-desegnos-pointer=\"{first}\"><p>A</p></div><button data-desegnos-pointer=\"{second}\">B</button></body></html>"
            )
            .into_bytes(),
        ),
        (
            "style.css".to_string(),
            format!(
                "/* desegnos-pointer:{first} */\n.a   {{   color: red;   }}\n/* /desegnos-pointer:{first} */\n"
            )
            .into_bytes(),
        ),
    ]);
    assert_eq!(pointer_fingerprints(&spaced), fingerprints);

    let edited = BTreeMap::from([(
        "style.css".to_string(),
        format!(
            "/* desegnos-pointer:{first} */ .a {{ color: blue; }} /* /desegnos-pointer:{first} */"
        )
        .into_bytes(),
    )]);
    let edited = pointer_fingerprints(&edited);
    assert_ne!(edited[&first], fingerprints[&first]);
}

#[test]
fn only_change_pointers_grant_their_target_to_a_request() {
    let (project, page, source) = with_element();
    let target = Uuid::new_v4();
    let project = apply(
        &project,
        Command::InsertElements {
            page_id: page,
            parent_id: None,
            index: 1,
            elements: vec![element(target, "Target")],
            picture_data_uri: None,
            size_id: None,
        },
    );
    let pointer_id = Uuid::new_v4();
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: pointer(
                pointer_id,
                page,
                source,
                PointerTarget::Element {
                    page_id: page,
                    element_id: target,
                    size_id: None,
                },
                "Reads this layer",
            ),
        },
    );
    let (editable, _) = project.editable_element_ids(page, source);
    assert!(
        !editable.contains(&target),
        "a reference pointer reads its target without granting changes"
    );
    let project = apply(
        &project,
        Command::UpdatePointer {
            pointer_id,
            from: None,
            to: None,
            comment: None,
            kind: Some(desegnos_core::PointerKind::Change),
        },
    );
    let (editable, _) = project.editable_element_ids(page, source);
    assert!(
        editable.contains(&target),
        "a change pointer grants its target"
    );
}

#[test]
fn a_pointer_without_a_kind_deserialises_as_reference() {
    let parsed: Pointer = serde_json::from_value(serde_json::json!({
        "id": Uuid::new_v4(),
        "from": { "kind": "boundary", "pageId": Uuid::new_v4() },
        "to": { "kind": "url", "url": "https://example.com" },
        "comment": "Read this",
        "wiring": null,
    }))
    .unwrap();
    assert_eq!(parsed.kind, desegnos_core::PointerKind::Reference);
}

#[test]
fn behaviours_require_change_and_are_unique_per_source() {
    let (project, page, id) = with_element();
    let mut own = pointer(
        Uuid::new_v4(),
        page,
        id,
        PointerTarget::Element {
            page_id: page,
            element_id: id,
            size_id: None,
        },
        "Follow the mouse",
    );
    assert!(own.is_behaviour());
    assert!(own.validate().unwrap_err().to_string().contains("Change"));
    own.kind = desegnos_core::PointerKind::Change;
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: own.clone(),
        },
    );
    assert_eq!(
        project.pointer_states()[0].state,
        PointerState::NeedsExporting
    );
    assert!(!desegnos_core::pointer_needs_classifying(&own));
    assert!(
        project
            .apply(
                project.revision,
                Command::UpdatePointer {
                    pointer_id: own.id,
                    from: None,
                    to: None,
                    comment: None,
                    kind: Some(desegnos_core::PointerKind::Reference),
                }
            )
            .is_err()
    );
    own.id = Uuid::new_v4();
    assert!(
        project
            .apply(
                project.revision,
                Command::AddPointer {
                    pointer: own.clone()
                }
            )
            .unwrap_err()
            .to_string()
            .contains("at most one behaviour")
    );
    own.to = PointerTarget::Boundary {
        page_id: page,
        size_id: None,
    };
    assert!(!own.is_behaviour());
    own.comment = "Go to this boundary".into();
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: own.clone(),
        },
    );
    assert!(
        project
            .pointer_states()
            .iter()
            .all(|state| state.state != PointerState::Contradicts)
    );
    assert!(
        project
            .apply(
                project.revision,
                Command::UpdatePointer {
                    pointer_id: own.id,
                    from: None,
                    to: Some(PointerTarget::Element {
                        page_id: page,
                        element_id: id,
                        size_id: None
                    }),
                    comment: None,
                    kind: None,
                }
            )
            .is_err()
    );
    own.from = PointerSource::Boundary {
        page_id: page,
        size_id: None,
    };
    assert!(own.is_behaviour());
    own.id = Uuid::new_v4();
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: own.clone(),
        },
    );
    own.id = Uuid::new_v4();
    let mut duplicate = project.clone();
    duplicate.pointers.push(own.clone());
    assert!(
        duplicate
            .validate()
            .unwrap_err()
            .to_string()
            .contains("at most one behaviour")
    );
    assert!(
        project
            .apply(
                project.revision,
                Command::AddPointer {
                    pointer: own.clone()
                }
            )
            .is_err()
    );
    own.kind = desegnos_core::PointerKind::Reference;
    assert!(own.validate().is_err());
    own.to = PointerTarget::Boundary {
        page_id: page,
        size_id: Some(Uuid::new_v4()),
    };
    assert!(!own.is_behaviour());
}

fn broken_pointers(project: &Project) -> Vec<Uuid> {
    project
        .pointers
        .iter()
        .filter(|pointer| pointer.is_broken(project))
        .map(|pointer| pointer.id)
        .collect()
}
