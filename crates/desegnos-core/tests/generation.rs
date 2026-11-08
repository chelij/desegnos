//! Generation reply validation and application.
use desegnos_core::{
    Appearance, BoundaryKind, Command, Element, ElementKind, ElementOperation, GenerationCandidate,
    GenerationCapabilities, PageSize, PendingRequest, PlayAction, Pointer, PointerSource,
    PointerTarget, Project, Rect, ReferenceItem, ReferenceKind, RequestTarget, Session, Viewport,
    apply_generation_output, empty_element,
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
                boundary_retry_order: Vec::new(),
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

    let mut classified = project.clone();
    classified.pointers[0].play_comment = classified.pointers[0].comment.clone();
    let classified_snapshot = classified.prepare_requirements().unwrap();
    let restated = candidate(&classified_snapshot, target, page, pointer);
    assert!(restated.validate(&classified_snapshot, &classified).is_ok());
}

#[test]
fn applying_a_generation_reply_is_one_undo_step() {
    let (project, page, target, _) = setup();
    let pointer = project.pointers[0].id;
    let snapshot = project.prepare_requirements().unwrap();
    let mut session = Session::new(project).unwrap();
    let before = session.history().position;
    session
        .apply_generation(
            &snapshot,
            &candidate(&snapshot, target, page, pointer),
            &[0],
            &BTreeMap::new(),
        )
        .unwrap();
    assert_eq!(session.history().position, before + 1);
    assert!(session.project().element(target).unwrap().1.children.len() == 1);
    assert!(!session.project().element_has_pending_request(target));
    session.undo().unwrap();
    let (_, undone) = session.project().element(target).unwrap();
    assert!(undone.children.is_empty());
    assert!(session.project().element_has_pending_request(target));
    assert!(session.project().pointers[0].play.is_none());
    session.redo().unwrap();
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
    assert_strict_schema(&desegnos_core::candidate_schema(), "schema");
    assert_strict_schema(&desegnos_core::result_schema(), "schema");
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

#[test]
fn set_behaviour_validates_scope_and_creates_updates_removes_in_one_undo_step() {
    for destination in ["layer", "subtree", "boundary"] {
        let (mut project, page, target, outsider) = setup();
        let child = element(
            Uuid::new_v4(),
            ElementKind::Text,
            rect(0.0, 0.0, 20.0, 20.0),
        );
        let child_id = child.id;
        project.pages[0].elements[0].children.push(child);
        let behaviour_target = match destination {
            "boundary" => None,
            "subtree" => Some(child_id),
            _ => Some(target),
        };
        let mut pointer_id = None;
        for words in ["Follow the mouse", "Rotate forever", ""] {
            let snapshot = project.prepare_requirements().unwrap();
            let mut reply = candidate(&snapshot, target, page, project.pointers[0].id);
            reply.pointer_classifications.retain(|classification| {
                project.pointers.iter().any(|pointer| {
                    pointer.id == classification.pointer_id
                        && desegnos_core::pointer_needs_classifying(pointer)
                })
            });
            reply.requests[0].operations = vec![desegnos_core::ElementOperation::SetBehaviour {
                element_id: behaviour_target,
                behaviour: words.into(),
            }];
            reply.validate(&snapshot, &project).unwrap();
            let mut escaped = reply.clone();
            escaped.requests[0].operations = vec![desegnos_core::ElementOperation::SetBehaviour {
                element_id: Some(outsider),
                behaviour: "Move".into(),
            }];
            // A pre-existing Change pointer to the boundary grants its layers, just as for Update.
            if destination != "boundary" || words == "Follow the mouse" {
                assert!(escaped.validate(&snapshot, &project).is_err());
            }
            let mut session = Session::new(project.clone()).unwrap();
            let before = session.history().position;
            session
                .apply_generation(&snapshot, &reply, &[0], &BTreeMap::new())
                .unwrap();
            assert_eq!(session.history().position, before + 1);
            let applied = session.project().clone();
            session.undo().unwrap();
            assert_eq!(session.project().pointers, project.pointers);
            let own: Vec<_> = applied
                .pointers
                .iter()
                .filter(|p| p.is_behaviour())
                .collect();
            if words.is_empty() {
                assert!(own.is_empty());
            } else {
                assert_eq!(own.len(), 1);
                assert_eq!(own[0].comment, words);
                assert_eq!(own[0].kind, desegnos_core::PointerKind::Change);
                assert!(own[0].wiring.is_none() && own[0].play.is_none());
                if let Some(id) = pointer_id {
                    assert_eq!(own[0].id, id);
                }
                pointer_id = Some(own[0].id);
            }
            project = applied;
            // Keep the same request active for the next edit.
            project.requests = session.project().requests.clone();
        }
    }
}

#[test]
fn mouse_following_and_scrolling_requests_apply_as_finite_designs_with_behaviours() {
    let (mut project, page, target, other) = setup();
    project.requests[0].text = "Make this point to mouse pointer in a website".into();
    let mut scrolling = project.requests[0].clone();
    scrolling.id = Uuid::new_v4();
    scrolling.text = "Looping scrolling text of random AI chat nonsense".into();
    scrolling.targets = vec![RequestTarget::Element {
        page,
        size: None,
        id: other,
    }];
    project.requests.push(scrolling);
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = candidate(&snapshot, target, page, project.pointers[0].id);
    reply.requests[0].request_id = Some(project.requests[0].id);
    reply.requests[0].operations = serde_json::from_value(json!([
        {"kind":"replaceChildren","children":[{
            "kind":"text","rect":rect(10.0, 10.0, 80.0, 40.0),"text":"→"
        }]},
        {"kind":"setBehaviour","elementId":target,"behaviour":"Rotate the arrow to point at the mouse pointer."}
    ])).unwrap();
    let mut second = reply.requests[0].clone();
    second.request_id = Some(project.requests[1].id);
    second.element_id = Some(other);
    second.operations = serde_json::from_value(json!([
        {"kind":"update","patches":[{"elementId":other,"text":"Thinking about thinking…"}]},
        {"kind":"setBehaviour","elementId":other,"behaviour":"Scroll the text left continuously in a seamless loop."}
    ])).unwrap();
    reply.requests.push(second);
    let reply = GenerationCandidate::from_json(&serde_json::to_string(&reply).unwrap()).unwrap();
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert!(applied.requests.is_empty());
    assert_eq!(applied.element(target).unwrap().1.children[0].text, "→");
    assert_eq!(
        applied.element(other).unwrap().1.text,
        "Thinking about thinking…"
    );
    assert_eq!(
        applied
            .pointers
            .iter()
            .filter(|pointer| pointer.is_behaviour())
            .count(),
        2
    );
    assert_eq!(applied.element(target).unwrap().1.meta.requests.len(), 1);
    assert_eq!(applied.element(other).unwrap().1.meta.requests.len(), 1);
}

#[test]
fn set_behaviour_rejects_boundary_without_a_boundary_request_and_oversized_text() {
    let (mut project, page, target, _) = setup();
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = candidate(&snapshot, target, page, project.pointers[0].id);
    reply.requests[0].operations = vec![desegnos_core::ElementOperation::SetBehaviour {
        element_id: Some(target),
        behaviour: "x".repeat(desegnos_core::limits::MAX_PROMPT_CHARS + 1),
    }];
    assert!(
        reply
            .validate(&snapshot, &project)
            .unwrap_err()
            .to_string()
            .contains("Pointer comment")
    );
    project.requests[0].targets = vec![RequestTarget::Reference {
        id: project.reference[0].id,
    }];
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = candidate(&snapshot, target, page, project.pointers[0].id);
    reply.requests[0].request_id = Some(project.requests[0].id);
    reply.requests[0].element_id = None;
    reply.requests[0].operations = vec![desegnos_core::ElementOperation::SetBehaviour {
        element_id: None,
        behaviour: "Animate forever".into(),
    }];
    assert!(
        reply
            .validate(&snapshot, &project)
            .unwrap_err()
            .to_string()
            .contains("boundary behaviour")
    );
}

#[test]
fn a_layer_keeps_one_pending_request() {
    let (project, page, target, _) = setup();
    let ask = |text: &str| PendingRequest {
        boundary_retry_order: Vec::new(),
        id: Uuid::new_v4(),
        text: text.into(),
        attachments: vec![],
        targets: vec![RequestTarget::Element {
            page,
            size: None,
            id: target,
        }],
        area: None,
        requested_at: "2026-10-31T00:00:00Z".into(),
    };
    let project = apply(
        &project,
        Command::AddRequest {
            request: ask("Make it red"),
        },
    );
    assert_eq!(project.requests.len(), 1);
    assert_eq!(project.requests[0].text, "Make it red");

    let mut saved = project.clone();
    saved.requests.insert(0, ask("An older request"));
    let session = Session::new(saved).unwrap();
    assert_eq!(session.project().requests.len(), 1);
    assert_eq!(session.project().requests[0].text, "Make it red");
}

fn add_request(project: &Project, text: &str, targets: Vec<RequestTarget>) -> Project {
    apply(
        project,
        Command::AddRequest {
            request: PendingRequest {
                boundary_retry_order: Vec::new(),
                id: Uuid::new_v4(),
                text: text.into(),
                attachments: vec![],
                targets,
                area: None,
                requested_at: "2026-11-01T00:00:00Z".into(),
            },
        },
    )
}

// A drawn request area over an empty page has only a boundary target: no layer exists to scope
// it to, so the whole pending request must still reach generation.
#[test]
fn a_boundary_only_request_on_an_empty_page_prepares_one_request_and_page() {
    let start = Project::new(
        "Blank".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    let page = start.home_page_id;
    let project = add_request(
        &start,
        "Build this page from the project brief.",
        vec![RequestTarget::Boundary { page, size: None }],
    );

    let snapshot = project.prepare_requirements().unwrap();
    assert_eq!(snapshot.request_count, 1);
    assert_eq!(snapshot.page_count, 1);
    assert_eq!(
        snapshot.request_ids().unwrap(),
        vec![project.requests[0].id]
    );
    snapshot.validate_current(&project).unwrap();
}

#[test]
fn a_layer_request_and_a_boundary_request_on_a_blank_page_both_travel() {
    let (project, _, target, _) = setup();
    let about = Uuid::new_v4();
    let project = apply(
        &project,
        Command::AddCanvasPage {
            name: "About".into(),
            path: "/about".into(),
            size: PageSize {
                id: about,
                label: "Desktop".into(),
                width: 800,
                height: 600,
                kind: BoundaryKind::Page,
                x: 0.0,
                y: 1000.0,
            },
            moves: vec![],
            reference: vec![],
        },
    );
    let blank_page = project.pages.last().unwrap().id;
    assert_eq!(blank_page, project.pages[1].id);
    assert!(project.page(blank_page).unwrap().elements.is_empty());
    let project = add_request(
        &project,
        "Build this page from the project brief.",
        vec![RequestTarget::Boundary {
            page: blank_page,
            size: None,
        }],
    );

    let snapshot = project.prepare_requirements().unwrap();
    assert_eq!(snapshot.request_count, 2);
    assert_eq!(snapshot.page_count, 2);
    snapshot.validate_current(&project).unwrap();

    // The targeted path stays scoped to the layers it was asked about.
    let targeted = project
        .prepare_targeted_requirements(GenerationCapabilities::default(), &[target], &[])
        .unwrap();
    assert_eq!(targeted.request_count, 1);
    assert_eq!(targeted.page_count, 1);
}

/// A reply to a boundary-only request: the page an area or a blank boundary asked for.
fn boundary_candidate(
    snapshot: &desegnos_core::RequirementsSnapshot,
    request_id: Uuid,
    operations: serde_json::Value,
) -> GenerationCandidate {
    serde_json::from_value(json!({
        "jobId": snapshot.job_id,
        "requirementsSha256": snapshot.sha256,
        "summary": "Built the page",
        "requests": [{
            "requestId": request_id,
            "elementId": null,
            "status": "implemented",
            "explanation": "Built the page",
            "operations": operations
        }],
        "images": [],
        "pointerClassifications": [],
        "decisionProposals": [],
        "criticism": []
    }))
    .unwrap()
}

/// One layer inserted straight into a boundary, below its current height.
fn below_the_fold(page: Uuid, y: f64, height: f64, text: &str) -> serde_json::Value {
    json!({
        "kind": "insertInBoundary",
        "pageId": page,
        "sizeId": null,
        "element": {
            "kind": "text", "rect": rect(0.0, y, 800.0, height), "text": text
        }
    })
}

fn blank_page_request(output: Option<desegnos_core::OutputType>) -> (Project, Uuid, Uuid) {
    let start = Project::new(
        "Blank".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    let page = start.home_page_id;
    let start = match output {
        Some(output) => apply(
            &start,
            Command::SetPageOutput {
                page_id: page,
                output,
            },
        ),
        None => start,
    };
    let project = add_request(
        &start,
        "Build this page from the project brief.",
        vec![RequestTarget::Boundary { page, size: None }],
    );
    let request = project.requests[0].id;
    (project, page, request)
}

#[test]
fn a_website_page_grows_downward_to_fit_inserted_layers() {
    let (project, page, request) = blank_page_request(None);
    let snapshot = project.prepare_requirements().unwrap();
    // The second layer sits below the first, so the boundary keeps the lowest edge.
    let reply = boundary_candidate(
        &snapshot,
        request,
        json!([
            below_the_fold(page, 620.0, 100.0, "Features"),
            below_the_fold(page, 1250.5, 40.0, "Footer"),
        ]),
    );
    reply.validate(&snapshot, &project).unwrap();
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    let size = applied.page(page).unwrap().base_size();
    assert_eq!(size.height, 1291);
    assert_eq!(size.width, 800);
    assert_eq!(applied.page(page).unwrap().elements.len(), 2);
    assert_eq!(applied.revision, project.revision + 1);

    // Growing the boundary is part of the same undo step as the new layers.
    let mut session = Session::new(project.clone()).unwrap();
    session
        .apply_generation(&snapshot, &reply, &[], &BTreeMap::new())
        .unwrap();
    assert_eq!(
        session.project().page(page).unwrap().base_size().height,
        1291
    );
    session.undo().unwrap();
    assert_eq!(
        session.project().page(page).unwrap().base_size().height,
        600
    );
    assert!(session.project().page(page).unwrap().elements.is_empty());
    session.redo().unwrap();
    assert_eq!(
        session.project().page(page).unwrap().base_size().height,
        1291
    );
}

// A model can echo the boundary's page id as the layer a request anchors on. A page is not a
// layer, so a boundary-only request drops that anchor instead of rejecting a complete reply.
#[test]
fn a_boundary_request_answered_with_its_page_id_as_the_layer_is_normalized_and_applied() {
    let (project, page, request) = blank_page_request(None);
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([below_the_fold(page, 620.0, 100.0, "Features")]),
    );
    reply.requests[0].element_id = Some(page);
    reply.normalize_boundary_anchors(&project);
    assert_eq!(reply.requests[0].element_id, None);

    reply.validate(&snapshot, &project).unwrap();
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert_eq!(applied.page(page).unwrap().base_size().height, 720);
    assert_eq!(applied.page(page).unwrap().elements.len(), 1);
}

#[test]
fn a_boundary_request_answered_with_an_unrelated_layer_is_still_rejected() {
    let (project, page, request) = blank_page_request(None);
    let unrelated = Uuid::new_v4();
    let project = apply(
        &project,
        Command::InsertElements {
            page_id: page,
            parent_id: None,
            index: 0,
            elements: vec![element(
                unrelated,
                ElementKind::Box,
                rect(0.0, 0.0, 100.0, 60.0),
            )],
            picture_data_uri: None,
            size_id: None,
        },
    );
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([below_the_fold(page, 620.0, 100.0, "Features")]),
    );
    reply.requests[0].element_id = Some(unrelated);
    reply.normalize_boundary_anchors(&project);
    assert_eq!(reply.requests[0].element_id, Some(unrelated));

    let error = reply.validate(&snapshot, &project).unwrap_err().to_string();
    assert!(error.contains("a layer it does not target"), "{error}");
}

// A model can also reach for `insert` and name the page it may fill as the parent. A page is
// not a layer, so that insert becomes a boundary insert instead of rejecting a complete reply.
#[test]
fn a_boundary_request_answered_with_insert_on_its_page_id_becomes_a_boundary_insert() {
    let (project, page, request) = blank_page_request(None);
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([
            {
                "kind": "insert", "index": null, "parentId": page,
                "element": {"kind": "text", "rect": rect(0.0, 620.0, 800.0, 100.0), "text": "Features"}
            },
            {
                "kind": "insert", "index": 0, "parentId": page,
                "element": {"kind": "text", "rect": rect(0.0, 720.0, 800.0, 40.0), "text": "Footer"}
            },
        ]),
    );
    assert!(
        reply.requests[0]
            .operations
            .iter()
            .all(|operation| matches!(operation, ElementOperation::Insert { .. }))
    );
    // Validation reads the reply the way apply does, so it accepts the page-id insert too.
    reply.validate(&snapshot, &project).unwrap();

    reply.normalize_boundary_anchors(&project);
    assert_eq!(reply.requests[0].operations.len(), 2);
    for operation in &reply.requests[0].operations {
        match operation {
            ElementOperation::InsertInBoundary {
                page_id, size_id, ..
            } => {
                assert_eq!(*page_id, page);
                assert!(size_id.is_none());
            }
            other => panic!("expected a boundary insert, got {other:?}"),
        }
    }

    reply.validate(&snapshot, &project).unwrap();
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert_eq!(applied.page(page).unwrap().base_size().height, 760);
    assert_eq!(applied.page(page).unwrap().elements.len(), 2);
}

#[test]
fn a_boundary_request_answered_with_insert_on_an_unrelated_layer_is_still_rejected() {
    let (project, page, request) = blank_page_request(None);
    // A layer that is not in this boundary's scope is not what the request may fill.
    let unrelated = Uuid::new_v4();
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([{
            "kind": "insert", "index": null, "parentId": unrelated,
            "element": {"kind": "text", "rect": rect(0.0, 20.0, 100.0, 20.0), "text": "Inside"}
        }]),
    );
    reply.normalize_boundary_anchors(&project);
    assert!(matches!(
        reply.requests[0].operations.as_slice(),
        [ElementOperation::Insert { .. }]
    ));

    let error = reply.validate(&snapshot, &project).unwrap_err().to_string();
    assert!(error.contains("inserted outside its request"), "{error}");
    assert!(project.page(page).unwrap().elements.is_empty());
}

// A real reply built the Nightfall home page as four boundary inserts and described the whole
// page's interaction in one boundary behaviour. Applying it must turn that behaviour into a
// pointer the export plan asks for, or Preview reports 0 pointers and no script ships.
#[test]
fn a_boundary_behaviour_from_a_generation_reply_is_planned_for_export() {
    let mut project: Project =
        serde_json::from_str(include_str!("fixtures/nightfall/project.json")).unwrap();
    let page = project.pages[0].clone();
    let request = Uuid::new_v4();
    project.requests.push(PendingRequest {
        boundary_retry_order: Vec::new(),
        id: request,
        text: "Build the full home page and make every interactive part work.".into(),
        attachments: vec![],
        targets: vec![RequestTarget::Boundary {
            page: page.id,
            size: Some(page.base_size().id),
        }],
        area: None,
        requested_at: "2026-11-06T14:09:34.534Z".into(),
    });
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/nightfall/reply.json")).unwrap();
    reply["jobId"] = json!(snapshot.job_id);
    reply["requirementsSha256"] = json!(snapshot.sha256);
    reply["requests"][0]["requestId"] = json!(request);
    let behaviour = "Sticky navigation, theme toggle, new-moon countdown, finder filters, billing \
        toggle, booking total, keyboard carousel, single-open accordion and inline email check.";
    reply["requests"][0]["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind": "setBehaviour", "elementId": null, "behaviour": behaviour}));
    let mut candidate = GenerationCandidate::from_json(&reply.to_string()).unwrap();
    candidate.normalize_boundary_anchors(&project);
    candidate.validate(&snapshot, &project).unwrap();

    let applied =
        apply_generation_output(&project, &snapshot, &candidate, &[], &BTreeMap::new()).unwrap();
    let pointer = applied
        .pointers
        .iter()
        .find(|pointer| pointer.is_behaviour())
        .expect("the boundary behaviour became a pointer");
    assert_eq!(pointer.comment, behaviour);
    assert!(matches!(
        pointer.from,
        PointerSource::Boundary { page_id, .. } if page_id == page.id
    ));

    let plan = desegnos_core::plan_export(&applied, &[], Uuid::new_v4()).unwrap();
    assert_eq!(plan.request_count, 1);
    assert_eq!(plan.pointers.len(), 1);
    assert_eq!(plan.pointers[0].id, pointer.id);
    assert_eq!(plan.pointers[0].comment, behaviour);
}

// The model can echo the boundary's page id where null means the request boundary. A page is
// not a layer, so it means the same page-wide behaviour instead of rejecting the reply.
#[test]
fn a_boundary_behaviour_that_names_its_page_id_targets_the_boundary() {
    let (project, page, request) = blank_page_request(None);
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([{"kind": "setBehaviour", "elementId": page, "behaviour": "Tick every second"}]),
    );
    reply.validate(&snapshot, &project).unwrap();

    reply.normalize_boundary_anchors(&project);
    assert!(matches!(
        reply.requests[0].operations.as_slice(),
        [ElementOperation::SetBehaviour {
            element_id: Some(id),
            ..
        }] if *id == page
    ));
    reply.validate(&snapshot, &project).unwrap();
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    let pointer = applied
        .pointers
        .iter()
        .find(|pointer| pointer.is_behaviour())
        .expect("the boundary behaviour became a pointer");
    assert_eq!(pointer.comment, "Tick every second");
    assert!(matches!(pointer.from, PointerSource::Boundary { .. }));
    assert_eq!(
        desegnos_core::plan_export(&applied, &[], Uuid::new_v4())
            .unwrap()
            .pointers
            .len(),
        1
    );
}

#[test]
fn a_slide_or_image_boundary_still_rejects_a_layer_below_its_height() {
    for output in [
        desegnos_core::OutputType::Slide,
        desegnos_core::OutputType::Image,
    ] {
        let (project, page, request) = blank_page_request(Some(output));
        let snapshot = project.prepare_requirements().unwrap();
        let reply = boundary_candidate(
            &snapshot,
            request,
            json!([below_the_fold(page, 620.0, 100.0, "Overflow")]),
        );
        let error = reply.validate(&snapshot, &project).unwrap_err().to_string();
        assert!(
            error.contains("must stay inside its boundary"),
            "{output:?}: {error}"
        );
        // The message names the layer, its rect and the boundary it left.
        assert!(
            error.contains("Text at 0,620 sized 800x100"),
            "{output:?}: {error}"
        );
        assert!(error.contains("800x600"), "{output:?}: {error}");
        assert_eq!(project.page(page).unwrap().base_size().height, 600);
        assert!(project.page(page).unwrap().elements.is_empty());
    }
}

#[test]
fn a_website_page_still_rejects_horizontal_and_upward_overflow() {
    let (project, page, request) = blank_page_request(None);
    let snapshot = project.prepare_requirements().unwrap();
    let wide = boundary_candidate(
        &snapshot,
        request,
        json!([{
            "kind": "insertInBoundary",
            "pageId": page,
            "sizeId": null,
            "element": {"kind": "text", "rect": rect(700.0, 10.0, 200.0, 20.0), "text": "Wide"}
        }]),
    );
    let error = wide.validate(&snapshot, &project).unwrap_err().to_string();
    assert!(error.contains("must stay inside its boundary"), "{error}");
    assert!(error.contains("Text at 700,10 sized 200x20"), "{error}");

    let above = boundary_candidate(
        &snapshot,
        request,
        json!([{
            "kind": "insertInBoundary",
            "pageId": page,
            "sizeId": null,
            "element": {"kind": "text", "rect": rect(0.0, -40.0, 200.0, 20.0), "text": "Above"}
        }]),
    );
    assert!(above.validate(&snapshot, &project).is_err());
}

#[test]
fn a_layer_that_leaves_its_parent_names_the_parent_in_the_rejection() {
    let (project, page, target, _) = setup();
    let pointer = project.pointers[0].id;
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = candidate(&snapshot, target, page, pointer);
    reply.requests[0].operations = vec![
        serde_json::from_value(json!({
            "kind": "insert",
            "parentId": target,
            "index": null,
            "element": {
                "kind": "box", "rect": rect(0.0, 180.0, 300.0, 100.0), "name": "Panel"
            }
        }))
        .unwrap(),
    ];
    let error = reply.validate(&snapshot, &project).unwrap_err().to_string();
    assert!(
        error.contains("Panel (Box) at 0,180 sized 300x100"),
        "{error}"
    );
    assert_eq!(
        reply.validate(&snapshot, &project).unwrap_err().element_id,
        Some(target)
    );
}

#[test]
fn the_requirements_tell_the_model_a_webpage_boundary_grows_downward() {
    let (project, ..) = blank_page_request(None);
    let snapshot = project.prepare_requirements().unwrap();
    assert!(
        snapshot
            .markdown
            .contains("A webpage boundary grows downward"),
        "the requirements must say a webpage boundary grows"
    );
}

#[test]
fn raw_control_characters_inside_reply_strings_are_repaired() {
    let job_id = Uuid::new_v4();
    let digest = "a".repeat(64);
    // Models emit literal newlines and tabs inside generated HTML text; strict JSON forbids them.
    let reply = format!(
        "{{\"jobId\":\"{job_id}\",\"requirementsSha256\":\"{digest}\",\
         \"summary\":\"Filled the box\",\
         \"requests\":[{{\"status\":\"implemented\",\
         \"explanation\":\"Wrote\n\t<div>hi</div>\",\"operations\":[]}}],\
         \"images\":[],\"pointerClassifications\":[],\"decisionProposals\":[],\
         \"criticism\":[]}}"
    );
    let candidate = GenerationCandidate::from_json(&reply).unwrap();
    assert_eq!(candidate.requests[0].explanation, "Wrote\n\t<div>hi</div>");

    // A reply that is still invalid reports the original strict error.
    let broken = reply.replacen("\"operations\":[]", "\"operations\":[,]", 1);
    let error = GenerationCandidate::from_json(&broken)
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with("The generation result is invalid: control character"),
        "{error}"
    );
}

// Without an enforced schema a model can copy `insert`'s index onto a boundary insert.
#[test]
fn a_boundary_insert_with_a_stray_index_still_parses() {
    let (project, page, request) = blank_page_request(None);
    let snapshot = project.prepare_requirements().unwrap();
    let mut operation = below_the_fold(page, 0.0, 40.0, "Hero");
    operation["index"] = json!(0);
    let reply = json!({
        "jobId": snapshot.job_id,
        "requirementsSha256": snapshot.sha256,
        "summary": "Built the page",
        "requests": [{
            "requestId": request, "elementId": null, "status": "implemented",
            "explanation": "Built the page", "operations": [operation]
        }],
        "images": [], "pointerClassifications": [], "decisionProposals": [], "criticism": []
    });
    let candidate = GenerationCandidate::from_json(&reply.to_string()).unwrap();
    candidate.validate(&snapshot, &project).unwrap();
}

// A reply describes each interaction as its own boundary behaviour; all of them must reach export.
#[test]
fn several_boundary_behaviours_in_one_reply_all_survive() {
    let (project, _page, request) = blank_page_request(None);
    let snapshot = project.prepare_requirements().unwrap();
    let reply = boundary_candidate(
        &snapshot,
        request,
        json!([
            {"kind": "setBehaviour", "elementId": null, "behaviour": "Count down to the new moon."},
            {"kind": "setBehaviour", "elementId": null, "behaviour": "Open one FAQ answer at a time."}
        ]),
    );
    reply.validate(&snapshot, &project).unwrap();
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    let behaviours: Vec<_> = applied
        .pointers
        .iter()
        .filter(|p| p.is_behaviour())
        .collect();
    assert_eq!(behaviours.len(), 1);
    assert!(
        behaviours[0]
            .comment
            .contains("Count down to the new moon.")
    );
    assert!(
        behaviours[0]
            .comment
            .contains("Open one FAQ answer at a time.")
    );

    // A later generation still replaces the page's behaviour rather than piling onto it.
    let mut next = applied.clone();
    next.requests.push(PendingRequest {
        boundary_retry_order: Vec::new(),
        id: Uuid::new_v4(),
        text: "Rework the interactions.".into(),
        attachments: vec![],
        targets: project.requests[0].targets.clone(),
        area: None,
        requested_at: "2026-11-06T15:00:00.000Z".into(),
    });
    let second = next.prepare_requirements().unwrap();
    let reply = boundary_candidate(
        &second,
        next.requests.last().unwrap().id,
        json!([{"kind": "setBehaviour", "elementId": null, "behaviour": "Only the countdown."}]),
    );
    reply.validate(&second, &next).unwrap();
    let applied = apply_generation_output(&next, &second, &reply, &[], &BTreeMap::new()).unwrap();
    let behaviour = applied.pointers.iter().find(|p| p.is_behaviour()).unwrap();
    assert_eq!(behaviour.comment, "Only the countdown.");
}

#[test]
fn missing_host_echoes_and_summary_are_restored() {
    let job_id = Uuid::new_v4();
    let sha = "a".repeat(64);
    let parsed = parse_for_job(
        r#"{"requests":[{"status":"implemented","operations":[]}]}"#,
        job_id,
        &sha,
    )
    .unwrap();
    assert_eq!(parsed.job_id, job_id);
    assert_eq!(parsed.requirements_sha256, sha);
    assert_eq!(parsed.summary, "");
}

#[test]
fn supplied_foreign_or_malformed_host_echoes_are_rejected() {
    let job_id = Uuid::new_v4();
    let sha = "a".repeat(64);
    for (key, value) in [
        ("requirementsSha256", json!("b".repeat(64))),
        ("jobId", json!(Uuid::new_v4())),
        ("requirementsSha256", json!(null)),
        ("jobId", json!(42)),
    ] {
        let mut reply = json!({"requests": []});
        reply[key] = value;
        assert!(parse_for_job(&reply.to_string(), job_id, &sha).is_err());
    }
}

#[test]
fn missing_required_array_and_nullable_property_are_filled() {
    let reply = json!({"requests": [{"status":"implemented", "explanation":"Done"}]});
    let parsed = parse_for_job(&reply.to_string(), Uuid::new_v4(), &"a".repeat(64)).unwrap();
    assert!(parsed.requests[0].operations.is_empty());
    assert_eq!(parsed.requests[0].element_id, None);
}

#[test]
fn missing_required_number_is_not_invented() {
    let reply = json!({"requests": [{"status":"implemented", "operations":[{
        "kind":"insertInBoundary", "pageId":Uuid::new_v4(),
        "element":{"kind":"text", "rect":{"y":0,"width":200,"height":40}, "text":"Hello"}
    }]}]});
    let error = parse_for_job(&reply.to_string(), Uuid::new_v4(), &"a".repeat(64))
        .unwrap_err()
        .to_string();
    assert!(error.contains("missing field `x`"), "{error}");
}

fn multiple_blank_boundaries() -> (Project, Vec<Uuid>, Uuid) {
    let mut project = Project::new(
        "Festival".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    for (index, name) in ["Lineup", "Tickets"].iter().enumerate() {
        project = apply(
            &project,
            Command::AddCanvasPage {
                name: (*name).into(),
                path: format!("/{name}"),
                size: PageSize {
                    id: Uuid::new_v4(),
                    label: "Desktop".into(),
                    width: 800,
                    height: 600,
                    kind: BoundaryKind::Page,
                    x: 0.0,
                    y: 1000.0 * (index + 1) as f64,
                },
                moves: vec![],
                reference: vec![],
            },
        );
    }
    let pages: Vec<_> = project.pages.iter().map(|page| page.id).collect();
    project = add_request(
        &project,
        "Build page 1 Home, page 2 Lineup, page 3 Tickets.",
        pages
            .iter()
            .map(|page| RequestTarget::Boundary {
                page: *page,
                size: None,
            })
            .collect(),
    );
    let request = project.requests[0].id;
    (project, pages, request)
}

#[test]
fn page_id_behaviours_select_their_boundary_in_a_multi_boundary_request() {
    let (project, pages, request) = multiple_blank_boundaries();
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([
            {"kind":"setBehaviour", "elementId":pages[0], "behaviour":"Animate the home page"},
            {"kind":"setBehaviour", "elementId":pages[1], "behaviour":"Filter the lineup"}
        ]),
    );
    reply.normalize_boundary_anchors(&project);
    reply.validate(&snapshot, &project).unwrap();
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    for page_id in &pages[..2] {
        assert!(applied.pointers.iter().any(|pointer| matches!(pointer.from,
            PointerSource::Boundary { page_id: id, .. } if id == *page_id)));
    }
}

#[test]
fn one_invalid_operation_is_dropped_while_other_operations_apply() {
    let (project, pages, request) = multiple_blank_boundaries();
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([
            below_the_fold(pages[0], 0.0, 40.0, "Home"),
            {"kind":"setBehaviour", "elementId":null, "behaviour":"Ambiguous boundary"},
            below_the_fold(pages[1], 0.0, 40.0, "Lineup"),
            below_the_fold(pages[2], 0.0, 40.0, "Tickets")
        ]),
    );
    let mut events = vec![];
    reply
        .retain_valid_operations(&snapshot, &project, &mut events)
        .unwrap();
    assert_eq!(reply.requests[0].operations.len(), 3);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].disposition, "dropped");
    assert!(events[0].message.contains("one boundary"));
    assert!(reply.can_apply());
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    for page_id in pages {
        assert_eq!(applied.page(page_id).unwrap().elements.len(), 1);
    }
    assert!(applied.requests.is_empty());
}

#[test]
fn untouched_boundaries_get_one_narrowed_follow_up_then_stop_on_an_empty_answer() {
    let (project, pages, request) = multiple_blank_boundaries();
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([below_the_fold(pages[0], 0.0, 40.0, "Home")]),
    );
    let mut events = vec![];
    reply
        .retain_valid_operations(&snapshot, &project, &mut events)
        .unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.message.contains("Lineup") && event.message.contains("Tickets"))
    );
    let next = apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert_eq!(next.requests.len(), 1);
    assert_eq!(next.requests[0].boundary_retry_order, pages);
    assert_eq!(
        next.requests[0].targets,
        vec![
            RequestTarget::Boundary {
                page: pages[1],
                size: None
            },
            RequestTarget::Boundary {
                page: pages[2],
                size: None
            },
        ]
    );
    // The retry marker and original page numbering survive saving and reopening.
    let next: Project = serde_json::from_str(&serde_json::to_string(&next).unwrap()).unwrap();
    let retry_snapshot = next.prepare_requirements().unwrap();
    assert!(retry_snapshot.markdown.contains("boundaryOrder"));
    let mut empty = boundary_candidate(&retry_snapshot, request, json!([]));
    let mut events = vec![];
    empty
        .retain_valid_operations(&retry_snapshot, &next, &mut events)
        .unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.message.contains("Stopped retrying"))
    );
    let applied =
        apply_generation_output(&next, &retry_snapshot, &empty, &[], &BTreeMap::new()).unwrap();
    assert!(applied.requests.is_empty());
    assert_eq!(applied.page(pages[0]).unwrap().elements.len(), 1);
    assert!(applied.page(pages[1]).unwrap().elements.is_empty());
    assert!(applied.page(pages[2]).unwrap().elements.is_empty());
}

#[test]
fn a_follow_up_cannot_duplicate_content_on_an_already_completed_boundary() {
    let (project, pages, request) = multiple_blank_boundaries();
    let snapshot = project.prepare_requirements().unwrap();
    let first = boundary_candidate(
        &snapshot,
        request,
        json!([below_the_fold(pages[0], 0.0, 40.0, "Home")]),
    );
    let next = apply_generation_output(&project, &snapshot, &first, &[], &BTreeMap::new()).unwrap();
    let snapshot = next.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([
            below_the_fold(pages[0], 0.0, 40.0, "Duplicate home"),
            below_the_fold(pages[1], 0.0, 40.0, "Lineup"),
            below_the_fold(pages[2], 0.0, 40.0, "Tickets")
        ]),
    );
    let mut events = vec![];
    reply
        .retain_valid_operations(&snapshot, &next, &mut events)
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.disposition == "dropped")
            .count(),
        1
    );
    let applied = apply_generation_output(&next, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert_eq!(applied.page(pages[0]).unwrap().elements.len(), 1);
    assert!(applied.requests.is_empty());
}

#[test]
fn a_failed_multi_patch_update_rolls_back_all_its_patches() {
    // Use an existing scoped layer and a separate unknown patch target.
    let (project_with_layer, page, target, outsider) = setup();
    let snapshot = project_with_layer.prepare_requirements().unwrap();
    let mut reply = candidate(&snapshot, target, page, project_with_layer.pointers[0].id);
    reply.requests[0].operations = serde_json::from_value(json!([
        {"kind":"update", "patches":[{"elementId":target,"text":"Must roll back"},{"elementId":outsider,"text":"Out of scope"}]},
        {"kind":"setBehaviour", "elementId":target,"behaviour":"Animate"}
    ])).unwrap();
    let mut events = vec![];
    reply
        .retain_valid_operations(&snapshot, &project_with_layer, &mut events)
        .unwrap();
    let applied = apply_generation_output(
        &project_with_layer,
        &snapshot,
        &reply,
        &[],
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(
        applied.element(target).unwrap().1.text,
        project_with_layer.element(target).unwrap().1.text
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.disposition == "dropped")
            .count(),
        1
    );
}

#[test]
fn operations_depending_on_a_missing_parent_are_dropped_without_losing_other_content() {
    let (mut project, pages, request) = multiple_blank_boundaries();
    let parent = Uuid::new_v4();
    project = apply(
        &project,
        Command::InsertElements {
            page_id: pages[0],
            parent_id: None,
            index: 0,
            elements: vec![element(
                parent,
                ElementKind::Box,
                rect(0.0, 0.0, 800.0, 200.0),
            )],
            picture_data_uri: None,
            size_id: None,
        },
    );
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([
            {"kind":"remove", "elementId":parent},
            {"kind":"insert", "parentId":parent, "element":{"kind":"text","rect":rect(0.0,0.0,100.0,40.0),"text":"Missing parent"}},
            below_the_fold(pages[1],0.0,40.0,"Lineup")
        ]),
    );
    let mut events = vec![];
    reply
        .retain_valid_operations(&snapshot, &project, &mut events)
        .unwrap();
    assert_eq!(reply.requests[0].operations.len(), 2);
    assert!(
        events
            .iter()
            .any(|event| event.disposition == "dropped"
                && event.message.contains("no longer exists"))
    );
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert!(applied.element(parent).is_none());
    assert_eq!(applied.page(pages[1]).unwrap().elements.len(), 1);
}

#[test]
fn recovery_still_rejects_foreign_replies_and_replies_with_no_valid_operations() {
    let (project, pages, request) = multiple_blank_boundaries();
    let snapshot = project.prepare_requirements().unwrap();
    let mut wrong = boundary_candidate(
        &snapshot,
        request,
        json!([below_the_fold(pages[0], 0.0, 40.0, "Home")]),
    );
    wrong.job_id = Uuid::new_v4();
    assert!(
        wrong
            .retain_valid_operations(&snapshot, &project, &mut vec![])
            .is_err()
    );
    let mut invalid = boundary_candidate(
        &snapshot,
        request,
        json!([
            {"kind":"setBehaviour", "elementId":null, "behaviour":"Ambiguous"}
        ]),
    );
    let mut events = vec![];
    assert!(
        invalid
            .retain_valid_operations(&snapshot, &project, &mut events)
            .is_err()
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].disposition, "dropped");
}

#[test]
fn operation_bounds_errors_reach_recovery_instead_of_rejecting_during_json_parsing() {
    let (project, pages, request) = multiple_blank_boundaries();
    let snapshot = project.prepare_requirements().unwrap();
    let original = boundary_candidate(
        &snapshot,
        request,
        json!([
            below_the_fold(pages[0], 0.0, -40.0, "Invalid size"),
            below_the_fold(pages[1], 0.0, 40.0, "Lineup")
        ]),
    );
    let mut reply = parse_for_job(
        &serde_json::to_string(&original).unwrap(),
        snapshot.job_id,
        &snapshot.sha256,
    )
    .unwrap();
    let mut events = vec![];
    reply
        .retain_valid_operations(&snapshot, &project, &mut events)
        .unwrap();
    assert_eq!(reply.requests[0].operations.len(), 1);
    assert!(
        events
            .iter()
            .any(|event| event.disposition == "dropped" && event.message.contains("non-negative"))
    );
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert_eq!(applied.page(pages[1]).unwrap().elements.len(), 1);
}

fn set_page(
    page: Uuid,
    name: Option<&str>,
    path: Option<&str>,
    background: Option<&str>,
) -> serde_json::Value {
    json!({"kind":"setPage","pageId":page,"name":name,"path":path,"background":background})
}

#[test]
fn page_properties_and_content_apply_in_one_undo_step() {
    let (project, pages, request) = multiple_blank_boundaries();
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([
            set_page(pages[0], Some("Home"), Some("/"), Some("#05060b")),
            set_page(pages[1], Some("Lineup"), Some("/lineup"), None),
            set_page(pages[2], Some("Tickets"), Some("/tickets"), None),
            below_the_fold(pages[0], 0.0, 40.0, "Festival")
        ]),
    );
    reply
        .retain_valid_operations(&snapshot, &project, &mut vec![])
        .unwrap();
    let original_pages = project.pages.clone();
    let mut session = Session::new(project).unwrap();
    let before = session.history().position;
    session
        .apply_generation(&snapshot, &reply, &[], &BTreeMap::new())
        .unwrap();
    assert_eq!(session.history().position, before + 1);
    assert_eq!(
        session.project().page(pages[0]).unwrap().background,
        "#05060b"
    );
    assert_eq!(session.project().page(pages[0]).unwrap().path, "/");
    assert_eq!(session.project().page(pages[1]).unwrap().name, "Lineup");
    assert_eq!(session.project().page(pages[1]).unwrap().path, "/lineup");
    assert_eq!(session.project().page(pages[2]).unwrap().path, "/tickets");
    session.undo().unwrap();
    assert_eq!(session.project().pages, original_pages);
}

#[test]
fn invalid_page_edits_drop_individually_and_do_not_change_unscoped_pages() {
    let (mut project, pages, request) = multiple_blank_boundaries();
    project.requests[0].targets = vec![RequestTarget::Boundary {
        page: pages[0],
        size: None,
    }];
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([
            set_page(pages[1], Some("Forbidden"), None, None),
            set_page(pages[0], None, Some("relative"), None),
            set_page(
                pages[0],
                None,
                Some(&project.page(pages[1]).unwrap().path),
                None
            ),
            set_page(pages[0], Some("  "), None, None),
            set_page(pages[0], None, None, Some("red")),
            set_page(pages[0], None, Some("/home"), None),
            set_page(pages[0], None, None, Some("#abc")),
            below_the_fold(pages[0], 0.0, 40.0, "Home")
        ]),
    );
    let mut events = vec![];
    reply
        .retain_valid_operations(&snapshot, &project, &mut events)
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.disposition == "dropped")
            .count(),
        6
    );
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert_eq!(applied.page(pages[0]).unwrap().path, "/");
    assert_eq!(applied.page(pages[0]).unwrap().background, "#aabbcc");
    assert_eq!(
        applied.page(pages[1]).unwrap(),
        project.page(pages[1]).unwrap()
    );
}

#[test]
fn an_explicit_home_route_request_can_change_the_home_path() {
    let (mut project, pages, request) = multiple_blank_boundaries();
    project.requests[0].text = "Set the home page URL path to /home.".into();
    let snapshot = project.prepare_requirements().unwrap();
    let mut reply = boundary_candidate(
        &snapshot,
        request,
        json!([set_page(pages[0], None, Some("/home"), None)]),
    );
    reply
        .retain_valid_operations(&snapshot, &project, &mut vec![])
        .unwrap();
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert_eq!(applied.page(pages[0]).unwrap().path, "/home");
}

#[test]
fn a_minimal_page_fill_patch_is_repaired_before_typed_style_parsing() {
    let (project, page, request) = blank_page_request(None);
    let snapshot = project.prepare_requirements().unwrap();
    // The discarded fields deliberately cannot deserialize as layer fields.
    let text=json!({"requests":[{"requestId":request,"status":"implemented","operations":[
        {"kind":"update","patches":[{"elementId":page,"style":{"fill":"#05060b"},"rect":"not a layer rectangle","text":"ignore this"}]}
    ]}]}).to_string();
    let mut events = vec![];
    let mut reply =
        GenerationCandidate::from_json_for_project(&text, &snapshot, &project, &mut events)
            .unwrap();
    reply
        .retain_valid_operations(&snapshot, &project, &mut events)
        .unwrap();
    assert!(
        matches!(&reply.requests[0].operations[0],ElementOperation::SetPage{page_id,background:Some(fill),..} if *page_id==page && fill=="#05060b")
    );
    assert_eq!(
        events
            .iter()
            .filter(
                |event| event.kind == desegnos_core::ValidationEventKind::PagePatch
                    && event.disposition == "warning"
            )
            .count(),
        1
    );
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert_eq!(applied.page(page).unwrap().background, "#05060b");
    assert!(applied.page(page).unwrap().elements.is_empty());
}

#[test]
fn page_links_resolve_after_page_edits_even_in_a_later_request() {
    let (mut project, pages, _) = multiple_blank_boundaries();
    let old_path = project.page(pages[1]).unwrap().path.clone();
    let old_name = project.page(pages[1]).unwrap().name.clone();
    project.requests.clear();
    project = add_request(
        &project,
        "Build the home navigation.",
        vec![RequestTarget::Boundary {
            page: pages[0],
            size: None,
        }],
    );
    let home_request = project.requests.last().unwrap().id;
    project = add_request(
        &project,
        "Name the lineup page and set its path to /artists.",
        vec![RequestTarget::Boundary {
            page: pages[1],
            size: None,
        }],
    );
    let lineup_request = project.requests.last().unwrap().id;
    let snapshot = project.prepare_requirements().unwrap();
    let links: Vec<_> = ["Lineup", "/lineup", old_path.as_str(), old_name.as_str()]
        .iter()
        .enumerate()
        .map(|(i, url)| {
            json!({
                "kind":"button","rect":rect(0.0,i as f64*45.0,200.0,40.0),"text":"Lineup","url":url
            })
        })
        .collect();
    let mut reply = boundary_candidate(
        &snapshot,
        home_request,
        json!([
            {"kind":"insertInBoundary","pageId":pages[0],"element":{"kind":"box","rect":rect(0.0,0.0,800.0,200.0),"children":links}}
        ]),
    );
    reply.requests.extend(
        boundary_candidate(
            &snapshot,
            lineup_request,
            json!([
                set_page(pages[1], Some("Lineup"), Some("/artists"), None),
                below_the_fold(pages[1], 0.0, 40.0, "Artists")
            ]),
        )
        .requests,
    );
    reply
        .retain_valid_operations(&snapshot, &project, &mut vec![])
        .unwrap();
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    let nav = &applied.page(pages[0]).unwrap().elements[0];
    for element in &nav.children {
        assert_eq!(element.url.as_deref(), Some("/artists"));
    }
    let bundle = desegnos_core::direct_export_bundle(&applied, &[]).unwrap();
    let plan = desegnos_core::plan_export(&applied, &[], Uuid::new_v4()).unwrap();
    let home = &plan
        .pages
        .iter()
        .find(|page| page.page_id == pages[0])
        .unwrap()
        .file;
    let lineup = &plan
        .pages
        .iter()
        .find(|page| page.page_id == pages[1])
        .unwrap()
        .file;
    let html = String::from_utf8(bundle.files[home].clone()).unwrap();
    assert!(html.contains(&format!("href=\"{lineup}\"")), "{html}");
}

#[test]
fn page_edit_schema_is_strict_and_has_nullable_unchanged_fields() {
    let schema = desegnos_core::candidate_schema();
    let operations = schema["properties"]["requests"]["items"]["properties"]["operations"]["items"]
        ["anyOf"]
        .as_array()
        .unwrap();
    let page = operations
        .iter()
        .find(|operation| operation["properties"]["kind"]["enum"] == json!(["setPage"]))
        .unwrap();
    assert_eq!(page["additionalProperties"], false);
    assert_eq!(page["required"].as_array().unwrap().len(), 5);
    for field in ["name", "path", "background"] {
        assert!(
            page["properties"][field]["anyOf"]
                .as_array()
                .unwrap()
                .iter()
                .any(|branch| branch["type"] == "null")
        );
    }
}

#[test]
fn page_patch_repair_does_not_bypass_scope_or_accept_non_colour_fills() {
    let (mut project, pages, request) = multiple_blank_boundaries();
    project.requests[0].targets = vec![RequestTarget::Boundary {
        page: pages[0],
        size: None,
    }];
    let snapshot = project.prepare_requirements().unwrap();
    let text=json!({"requests":[{"requestId":request,"status":"implemented","operations":[
        {"kind":"update","patches":[{"elementId":pages[1],"style":{"fill":"#000"}},{"elementId":pages[0],"style":{"fill":"linear-gradient(red,blue)"}}]},
        below_the_fold(pages[0],0.0,40.0,"Home")
    ]}]}).to_string();
    let mut events = vec![];
    let mut reply =
        GenerationCandidate::from_json_for_project(&text, &snapshot, &project, &mut events)
            .unwrap();
    reply
        .retain_valid_operations(&snapshot, &project, &mut events)
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.disposition == "dropped")
            .count(),
        2
    );
    let applied =
        apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
    assert_eq!(
        applied.page(pages[0]).unwrap().background,
        project.page(pages[0]).unwrap().background
    );
    assert_eq!(
        applied.page(pages[1]).unwrap(),
        project.page(pages[1]).unwrap()
    );
    assert_eq!(applied.page(pages[0]).unwrap().elements.len(), 1);
}

#[test]
fn page_names_resolve_in_updates_inserts_and_replacement_children() {
    for kind in ["update", "insert", "replaceChildren"] {
        let (mut project, pages, request) = multiple_blank_boundaries();
        let parent = Uuid::new_v4();
        let child = Uuid::new_v4();
        let mut box_element = element(parent, ElementKind::Box, rect(0.0, 0.0, 800.0, 200.0));
        box_element.children.push(element(
            child,
            ElementKind::Text,
            rect(0.0, 0.0, 200.0, 40.0),
        ));
        project = apply(
            &project,
            Command::InsertElements {
                page_id: pages[0],
                parent_id: None,
                index: 0,
                elements: vec![box_element],
                picture_data_uri: None,
                size_id: None,
            },
        );
        project.requests[0].targets.push(RequestTarget::Element {
            page: pages[0],
            size: None,
            id: parent,
        });
        let snapshot = project.prepare_requirements().unwrap();
        let link =
            json!({"kind":"text","rect":rect(0.0,50.0,200.0,40.0),"text":"Lineup","url":"Lineup"});
        let operation = match kind {
            "update" => json!({"kind":"update","patches":[{"elementId":child,"url":"Lineup"}]}),
            "insert" => json!({"kind":"insert","parentId":parent,"element":link}),
            _ => json!({"kind":"replaceChildren","children":[link]}),
        };
        let mut reply = boundary_candidate(
            &snapshot,
            request,
            json!([
                operation,
                set_page(pages[1], Some("Lineup"), Some("/lineup"), None)
            ]),
        );
        reply.requests[0].element_id = Some(parent);
        reply
            .retain_valid_operations(&snapshot, &project, &mut vec![])
            .unwrap();
        let applied =
            apply_generation_output(&project, &snapshot, &reply, &[], &BTreeMap::new()).unwrap();
        assert!(
            applied
                .element(parent)
                .unwrap()
                .1
                .children
                .iter()
                .any(|element| element.url.as_deref() == Some("/lineup")),
            "{kind}"
        );
    }
}

#[test]
fn generated_children_are_adapted_in_the_generation_undo_step() {
    let (mut project, page, target, _) = setup();
    let phone = Uuid::new_v4();
    let mut size = project.pages[0].base_size().clone();
    size.id = phone;
    size.width = 390;
    project.pages[0].sizes.push(size);
    let snapshot = project.prepare_requirements().unwrap();
    let reply = candidate(&snapshot, target, page, project.pointers[0].id);
    let mut session = Session::new(project.clone()).unwrap();
    session
        .apply_generation(&snapshot, &reply, &[], &BTreeMap::new())
        .unwrap();
    let child = &session.project().element(target).unwrap().1.children[0];
    assert!(child.overrides[&phone].automatic);
    assert!(
        child.overrides[&phone]
            .style
            .as_ref()
            .unwrap()
            .text_style
            .size
            >= 14.0
    );
    session.undo().unwrap();
    assert!(
        session
            .project()
            .element(target)
            .unwrap()
            .1
            .children
            .is_empty()
    );
    assert_eq!(session.project().pages, project.pages);
}

fn parse_for_job(
    source: &str,
    job_id: Uuid,
    sha256: &str,
) -> std::result::Result<GenerationCandidate, desegnos_core::GenerationValidationError> {
    GenerationCandidate::ingest(
        source,
        Some(desegnos_core::GenerationExpectations {
            job_id,
            sha256,
            project: None,
        }),
        &mut Vec::new(),
    )
}
