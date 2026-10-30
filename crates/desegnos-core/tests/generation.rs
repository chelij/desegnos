//! Generation reply validation and application.
use desegnos_core::{
    Appearance, Command, Element, ElementKind, GenerationCandidate, PendingRequest, PlayAction,
    Pointer, PointerSource, PointerTarget, Project, Rect, ReferenceItem, ReferenceKind,
    RequestTarget, Session, Viewport, apply_generation_output, empty_element,
};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn apply(project: &Project, command: Command) -> Project {
    project.apply(project.revision, command).unwrap()
}

fn element(id: Uuid, kind: ElementKind, rect: Rect) -> Element {
    let mut element = empty_element(kind, rect);
    element.id = id;
    element
}

fn setup() -> (Project, Uuid, Uuid, Uuid) {
    let start = Project::new(
        "Generation".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    let page = start.home_page_id;
    let target = Uuid::new_v4();
    let outsider = Uuid::new_v4();
    let reference = Uuid::new_v4();
    let pointer_id = Uuid::new_v4();
    let project = apply(
        &start,
        Command::InsertElements {
            page_id: page,
            parent_id: None,
            index: 0,
            elements: vec![
                element(target, ElementKind::Box, rect(0.0, 0.0, 300.0, 200.0)),
                element(outsider, ElementKind::Text, rect(320.0, 0.0, 120.0, 24.0)),
            ],
            picture_data_uri: None,
            size_id: None,
        },
    );
    let project = apply(
        &project,
        Command::AddRequest {
            request: PendingRequest {
                id: Uuid::new_v4(),
                text: "Fill this box with a headline".into(),
                attachments: vec![],
                targets: vec![RequestTarget::Element {
                    page,
                    size: None,
                    id: target,
                }],
                area: None,
                requested_at: "2026-10-27T00:00:00Z".into(),
            },
        },
    );
    let project = apply(
        &project,
        Command::AddReferenceItem {
            item: ReferenceItem {
                id: reference,
                rect: rect(-320.0, 0.0, 200.0, 120.0),
                kind: ReferenceKind::Note {
                    text: "Read-only brand note".into(),
                },
            },
            picture_data_uri: None,
        },
    );
    let project = apply(
        &project,
        Command::AddPointer {
            pointer: Pointer {
                id: pointer_id,
                from: PointerSource::Element {
                    page_id: page,
                    element_id: target,
                    size_id: None,
                },
                to: PointerTarget::Reference { item_id: reference },
                comment: "Reads the note".into(),
                kind: desegnos_core::PointerKind::Reference,
                wiring: None,
                play: None,
                play_comment: String::new(),
            },
        },
    );
    (project, page, target, outsider)
}

fn candidate(
    snapshot: &desegnos_core::RequirementsSnapshot,
    target: Uuid,
    page: Uuid,
    pointer: Uuid,
) -> GenerationCandidate {
    serde_json::from_value(json!({
        "jobId": snapshot.job_id,
        "requirementsSha256": snapshot.sha256,
        "summary": "Filled the requested box",
        "requests": [{
            "elementId": target,
            "status": "implemented",
            "explanation": "Added a headline",
            "operations": [{
                "kind": "replaceChildren",
                "children": [{
                    "kind": "text",
                    "rect": rect(10.0, 10.0, 200.0, 40.0),
                    "text": "Hello"
                }]
            }]
        }],
        "images": [],
        "pointerClassifications": [{
            "pointerId": pointer,
            "comment": "Reads the note",
            "action": {"kind": "goTo", "boundary": page}
        }],
        "decisionProposals": [{
            "topic": "Heading colour",
            "value": "Navy",
            "scope": {"kind": "project"},
            "rationale": "Matches the brand"
        }],
        "criticism": []
    }))
    .unwrap()
}

#[test]
fn apply_generation_assigns_ids_records_history_clears_pending_and_classifies_play() {
    let (project, page, target, _) = setup();
    let pointer = project.pointers[0].id;
    let snapshot = project.prepare_requirements().unwrap();
    assert!(project.element_has_pending_request(target));
    let reply = candidate(&snapshot, target, page, pointer);
    reply.validate(&snapshot, &project).unwrap();

    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[0], &BTreeMap::new()).unwrap();
    let (_, target_element) = applied.element(target).unwrap();
    assert_eq!(target_element.children.len(), 1);
    let child = &target_element.children[0];
    assert!(!child.id.is_nil());
    assert_eq!(child.text, "Hello");
    assert_ne!(child.id, target);
    assert_ne!(child.id, project.pages[0].elements[1].id);
    assert!(!applied.element_has_pending_request(target));
    assert_eq!(target_element.meta.requests.len(), 1);
    assert_eq!(
        target_element.meta.requests[0].prompt,
        "Fill this box with a headline"
    );
    assert_eq!(
        target_element.meta.requests[0].outcome.as_deref(),
        Some("Added a headline")
    );
    assert!(
        target_element.meta.requests[0]
            .job_id
            .as_deref()
            .is_some_and(|id| id == reply.job_id.to_string())
    );
    assert!(!applied.element_has_pending_request(target));
    assert_eq!(applied.pending_request_count(), 0);
    assert!(applied.requests.is_empty());

    let pointer = applied
        .pointers
        .iter()
        .find(|item| item.id == pointer)
        .unwrap();
    assert_eq!(pointer.play, Some(PlayAction::GoTo { boundary: page }));
    assert_eq!(pointer.play_comment, "Reads the note");
    assert_eq!(applied.applied_job_ids, vec![reply.job_id]);
    assert_eq!(applied.decisions.len(), 1);
    assert_eq!(applied.decisions[0].value, "Navy");
    assert_eq!(applied.revision, project.revision + 1);
}

#[test]
fn candidate_validation_rejects_scope_escape_duplicates_and_wrong_identity() {
    let (project, page, target, outsider) = setup();
    let pointer = project.pointers[0].id;
    let snapshot = project.prepare_requirements().unwrap();

    let mut escaped = candidate(&snapshot, target, page, pointer);
    escaped.requests[0].operations = vec![
        serde_json::from_value(json!({
            "kind": "update",
            "patches": [{"elementId": outsider, "text": "Hijacked"}]
        }))
        .unwrap(),
    ];
    let error = escaped.validate(&snapshot, &project).unwrap_err();
    assert!(
        error.to_string().contains("outside its request"),
        "unexpected error: {error}"
    );

    let mut wrong_job = candidate(&snapshot, target, page, pointer);
    wrong_job.job_id = Uuid::new_v4();
    assert!(wrong_job.validate(&snapshot, &project).is_err());

    let mut omitted = candidate(&snapshot, target, page, pointer);
    omitted.requests.clear();
    assert!(omitted.validate(&snapshot, &project).is_err());

    let mut unrequested_pointer = candidate(&snapshot, target, page, pointer);
    unrequested_pointer.pointer_classifications[0].pointer_id = Uuid::new_v4();
    assert!(unrequested_pointer.validate(&snapshot, &project).is_err());

    let mut mismatched_comment = candidate(&snapshot, target, page, pointer);
    mismatched_comment.pointer_classifications[0].comment = "Something else".into();
    assert!(mismatched_comment.validate(&snapshot, &project).is_err());
}

#[test]
fn applying_a_generation_reply_is_one_undo_step() {
    let (project, page, target, _) = setup();
    let pointer = project.pointers[0].id;
    let snapshot = project.prepare_requirements().unwrap();
    let mut session = Session::new(project).unwrap();
    let before = session.history().2;
    session
        .apply_generation(
            &snapshot,
            &candidate(&snapshot, target, page, pointer),
            &[0],
            &BTreeMap::new(),
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(session.history().2, before + 1);
    assert!(session.project().element(target).unwrap().1.children.len() == 1);
    assert!(!session.project().element_has_pending_request(target));
    session.undo(|_| Ok(())).unwrap();
    let (_, undone) = session.project().element(target).unwrap();
    assert!(undone.children.is_empty());
    let _ = undone;
    assert!(session.project().element_has_pending_request(target));
    assert!(session.project().pointers[0].play.is_none());
    session.redo(|_| Ok(())).unwrap();
    assert!(
        !session
            .project()
            .element(target)
            .unwrap()
            .1
            .children
            .is_empty()
    );
}

/// OpenAI Structured Outputs strict mode rejects `oneOf` and any object that does not require
/// every property. Walk the whole reply schema, including `$defs`, so a future branch cannot
/// reintroduce a construct the provider refuses.
fn assert_strict_schema(schema: &serde_json::Value, path: &str) {
    let object = schema
        .as_object()
        .unwrap_or_else(|| panic!("{path} is not a schema object"));
    for keyword in [
        "oneOf",
        "allOf",
        "not",
        "if",
        "then",
        "else",
        "patternProperties",
    ] {
        assert!(
            !object.contains_key(keyword),
            "{path} uses the unsupported keyword {keyword}"
        );
    }
    assert!(
        object.get("anyOf").is_none() || path != "schema",
        "the schema root must not use anyOf"
    );
    if object.get("type").and_then(serde_json::Value::as_str) == Some("object") {
        assert_eq!(
            object.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false)),
            "{path} must set additionalProperties: false"
        );
        let properties = object
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("{path} has no properties object"));
        let required: std::collections::BTreeSet<&str> = object
            .get("required")
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| panic!("{path} has no required list"))
            .iter()
            .map(|key| key.as_str().expect("required keys are strings"))
            .collect();
        let keys: std::collections::BTreeSet<&str> =
            properties.keys().map(String::as_str).collect();
        assert_eq!(required, keys, "{path} must require every property");
        for (name, child) in properties {
            assert_strict_schema(child, &format!("{path}.properties.{name}"));
        }
    }
    if let Some(items) = object.get("items") {
        assert_strict_schema(items, &format!("{path}.items"));
    }
    if let Some(branches) = object.get("anyOf").and_then(serde_json::Value::as_array) {
        for (index, branch) in branches.iter().enumerate() {
            assert_strict_schema(branch, &format!("{path}.anyOf[{index}]"));
        }
    }
    if let Some(definitions) = object.get("$defs").and_then(serde_json::Value::as_object) {
        for (name, child) in definitions {
            assert_strict_schema(child, &format!("{path}.$defs.{name}"));
        }
    }
}

#[test]
fn reply_schemas_conform_to_strict_structured_output() {
    let (project, ..) = setup();
    let snapshot = project.prepare_requirements().unwrap();
    assert_strict_schema(&desegnos_core::candidate_schema(), "schema");
    assert_strict_schema(&desegnos_core::result_schema(&snapshot), "schema");
}

#[test]
fn strict_schema_nulls_read_as_omitted_optional_fields() {
    let (project, page, target, _) = setup();
    let pointer = project.pointers[0].id;
    let snapshot = project.prepare_requirements().unwrap();
    let reply = json!({
        "jobId": snapshot.job_id,
        "requirementsSha256": snapshot.sha256,
        "summary": "Filled the requested box",
        "requests": [{
            "requestId": null,
            "elementId": target,
            "status": "implemented",
            "explanation": "Added a headline",
            "operations": [
                {"kind": "update", "patches": [{
                    "elementId": target, "rect": null, "rotation": null, "style": null,
                    "text": null, "url": null, "layout": null, "visible": null,
                    "locked": null, "name": null, "inputType": null, "options": null,
                    "language": null, "source": null, "sizeId": null
                }]},
                {"kind": "replaceChildren", "children": [{
                    "kind": "text", "rect": rect(10.0, 10.0, 200.0, 40.0),
                    "name": null, "rotation": 0.0, "visible": true, "locked": false,
                    "text": "Hello", "image": null, "imageRequest": null, "url": null,
                    "layout": {"kind": "free"}, "inputType": null, "options": null,
                    "language": null, "source": null, "children": []
                }]},
                {"kind": "insert", "parentId": target, "index": null,
                 "element": {"kind": "box", "rect": rect(0.0, 0.0, 20.0, 20.0),
                    "name": null, "rotation": 0.0, "visible": true, "locked": false,
                    "text": "", "image": null, "imageRequest": null, "url": null,
                    "layout": {"kind": "free"}, "inputType": null, "options": null,
                    "language": null, "source": null, "children": []}},
                {"kind": "insertInBoundary", "pageId": page, "sizeId": null,
                 "element": {"kind": "text", "rect": rect(600.0, 500.0, 100.0, 20.0),
                    "name": null, "rotation": 0.0, "visible": true, "locked": false,
                    "text": "Aside", "image": null, "imageRequest": null, "url": null,
                    "layout": {"kind": "free"}, "inputType": null, "options": null,
                    "language": null, "source": null, "children": []}}
            ]
        }],
        "images": [],
        "pointerClassifications": [{
            "pointerId": pointer, "comment": "Reads the note",
            "action": {"kind": "goTo", "boundary": page}
        }],
        "decisionProposals": [],
        "criticism": [{
            "elementId": null, "message": "Looks sparse", "suggestion": "Add spacing"
        }]
    });
    let candidate = GenerationCandidate::from_json(&reply.to_string()).unwrap();
    assert_eq!(candidate.requests[0].request_id, None);
    match &candidate.requests[0].operations[0] {
        desegnos_core::ElementOperation::Update { patches } => {
            let patch = &patches[0];
            assert!(patch.rect.is_none());
            assert!(patch.rotation.is_none());
            assert!(patch.style.is_none());
            assert!(patch.name.is_none());
            assert!(patch.input_type.is_none());
            assert!(patch.size_id.is_none());
        }
        other => panic!("unexpected operation: {other:?}"),
    }
    match &candidate.requests[0].operations[1] {
        desegnos_core::ElementOperation::ReplaceChildren { children } => {
            let draft = &children[0];
            assert!(draft.name.is_none());
            assert!(draft.image.is_none());
            assert!(draft.url.is_none());
            assert!(draft.children.is_empty());
            assert!(draft.layout.is_free());
        }
        other => panic!("unexpected operation: {other:?}"),
    }
    match &candidate.requests[0].operations[2] {
        desegnos_core::ElementOperation::Insert { index, .. } => assert_eq!(*index, None),
        other => panic!("unexpected operation: {other:?}"),
    }
    match &candidate.requests[0].operations[3] {
        desegnos_core::ElementOperation::InsertInBoundary { size_id, .. } => {
            assert_eq!(*size_id, None)
        }
        other => panic!("unexpected operation: {other:?}"),
    }

    // An update patch whose fields all arrive as null changes nothing at all.
    let mut no_change = reply.clone();
    no_change["requests"][0]["operations"] = json!([{
        "kind": "update",
        "patches": [{
            "elementId": target, "rect": null, "rotation": null, "style": null,
            "text": null, "url": null, "layout": null, "visible": null,
            "locked": null, "name": null, "inputType": null, "options": null,
            "language": null, "source": null, "sizeId": null
        }]
    }]);
    let candidate = GenerationCandidate::from_json(&no_change.to_string()).unwrap();
    candidate.validate(&snapshot, &project).unwrap();
    let applied =
        apply_generation_output(&project, &snapshot, &candidate, &[], &BTreeMap::new()).unwrap();
    let (_, target_element) = applied.element(target).unwrap();
    assert_eq!(target_element.rect, rect(0.0, 0.0, 300.0, 200.0));
    assert_eq!(target_element.rotation, 0.0);
    assert_eq!(target_element.children.len(), 0);
}
