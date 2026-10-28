//! Element commands, linked definitions, linked sizes and their undo behaviour.
use desegnos_core::{
    Appearance, Command, Element, ElementKind, ElementMove, ElementPatch, PageSize, Project, Rect,
    Session, Viewport, empty_element, nested_element_id, resolve_element, resolve_for_size,
};
use serde_json::json;
use uuid::Uuid;

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn project() -> Project {
    Project::new(
        "Test".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap()
}

fn apply(project: &Project, command: Command) -> desegnos_core::Result<Project> {
    project.apply(project.revision, command)
}

fn page_id(project: &Project) -> Uuid {
    project.home_page_id
}

fn base_size_id(project: &Project) -> Uuid {
    project.page(page_id(project)).unwrap().base_size_id()
}

fn element(id: Uuid, kind: ElementKind, rect: Rect) -> Element {
    let mut element = empty_element(kind, rect);
    element.id = id;
    element
}

fn text(id: Uuid, value: &str) -> Element {
    let mut element = element(id, ElementKind::Text, rect(10.0, 20.0, 120.0, 24.0));
    element.text = value.into();
    element
}

fn patch(id: Uuid) -> ElementPatch {
    serde_json::from_value(json!({ "id": id })).unwrap()
}

fn insert(project: &Project, elements: Vec<Element>) -> desegnos_core::Result<Project> {
    apply(
        project,
        Command::InsertElements {
            page_id: page_id(project),
            parent_id: None,
            index: 0,
            elements,
            picture_data_uri: None,
            size_id: None,
        },
    )
}

fn step(session: &Session) -> usize {
    session.history().2
}

// ---------------------------------------------------------------------------
// Element validation
// ---------------------------------------------------------------------------

#[test]
fn element_validation_rejects_bad_geometry_kind_content_and_duplicates() {
    let project = project();

    let mut broken = element(Uuid::new_v4(), ElementKind::Box, rect(0.0, 0.0, 10.0, 10.0));
    broken.rect.width = f64::NAN;
    assert!(insert(&project, vec![broken]).is_err());

    let image = element(Uuid::new_v4(), ElementKind::Image, rect(0.0, 0.0, 10.0, 10.0));
    assert!(insert(&project, vec![image]).is_err());

    let mut text_with_child = text(Uuid::new_v4(), "parent");
    text_with_child
        .children
        .push(text(Uuid::new_v4(), "child"));
    assert!(insert(&project, vec![text_with_child]).is_err());

    let select = element(
        Uuid::new_v4(),
        ElementKind::Select {
            options: vec!["One".into(), "one".into()],
        },
        rect(0.0, 0.0, 100.0, 32.0),
    );
    assert!(insert(&project, vec![select]).is_err());

    let id = Uuid::new_v4();
    let populated = insert(&project, vec![text(id, "one")]).unwrap();
    assert!(insert(&populated, vec![text(id, "again")]).is_err());

    let mut missing_link = text(Uuid::new_v4(), "linked");
    missing_link.link = Some(Uuid::new_v4());
    assert!(insert(&project, vec![missing_link]).is_err());
}

// ---------------------------------------------------------------------------
// One undo step per element command
// ---------------------------------------------------------------------------

#[test]
fn insert_update_move_reparent_and_remove_are_one_undo_step_each() {
    let mut session = Session::new(project()).unwrap();
    let page = page_id(session.project());
    let box_id = Uuid::new_v4();
    let text_id = Uuid::new_v4();
    let box_element = element(box_id, ElementKind::Box, rect(100.0, 50.0, 300.0, 200.0));
    let text_element = text(text_id, "Hello");

    let before = step(&session);
    session
        .execute(
            session.project().revision,
            Command::InsertElements {
                page_id: page,
                parent_id: None,
                index: 0,
                elements: vec![box_element, text_element],
                picture_data_uri: None,
                size_id: None,
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(step(&session), before + 1);
    assert_eq!(session.project().page(page).unwrap().elements.len(), 2);
    session.undo(|_| Ok(())).unwrap();
    assert!(session.project().page(page).unwrap().elements.is_empty());
    session.redo(|_| Ok(())).unwrap();

    // Update: rect and text in one patch and one history step.
    let before = step(&session);
    let mut update = patch(text_id);
    update.rect = Some(rect(30.0, 40.0, 200.0, 60.0));
    update.text = Some("Changed".into());
    session
        .execute(
            session.project().revision,
            Command::UpdateElements {
                page_id: page,
                patches: vec![update],
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(step(&session), before + 1);
    assert_eq!(session.project().element(text_id).unwrap().1.text, "Changed");
    assert_eq!(session.project().element(text_id).unwrap().1.rect.x, 30.0);
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().element(text_id).unwrap().1.text, "Hello");
    assert_eq!(session.project().element(text_id).unwrap().1.rect.x, 10.0);

    // Reparent into the box: the position becomes parent-relative.
    let before = step(&session);
    session
        .execute(
            session.project().revision,
            Command::MoveElements {
                moves: vec![ElementMove {
                    id: text_id,
                    to_page_id: page,
                    to_parent_id: Some(box_id),
                    index: 0,
                    rect: rect(12.0, 18.0, 120.0, 24.0),
                    size_id: None,
                    all_sizes: false,
                }],
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(step(&session), before + 1);
    let (_, moved) = session.project().element(text_id).unwrap();
    assert_eq!((moved.rect.x, moved.rect.y), (12.0, 18.0));
    assert_eq!(
        session.project().element(box_id).unwrap().1.children.len(),
        1
    );
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().element(text_id).unwrap().1.rect.x, 10.0);
    assert!(session.project().element(box_id).unwrap().1.children.is_empty());

    // Remove one layer.
    let before = step(&session);
    session
        .execute(
            session.project().revision,
            Command::RemoveElements {
                page_id: page,
                ids: vec![text_id],
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(step(&session), before + 1);
    assert!(session.project().element(text_id).is_none());
    session.undo(|_| Ok(())).unwrap();
    assert!(session.project().element(text_id).is_some());
}

#[test]
fn group_and_ungroup_are_one_undo_step_each() {
    let mut session = Session::new(insert(
        &project(),
        vec![
            element(Uuid::new_v4(), ElementKind::Box, rect(10.0, 10.0, 100.0, 80.0)),
            element(Uuid::new_v4(), ElementKind::Box, rect(150.0, 50.0, 60.0, 60.0)),
        ],
    )
    .unwrap())
    .unwrap();
    let page = page_id(session.project());
    let ids: Vec<Uuid> = session
        .project()
        .page(page)
        .unwrap()
        .elements
        .iter()
        .map(|element| element.id)
        .collect();

    let before = step(&session);
    session
        .execute(
            session.project().revision,
            Command::GroupElements {
                page_id: page,
                ids: ids.clone(),
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(step(&session), before + 1);
    let roots = &session.project().page(page).unwrap().elements;
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].kind, ElementKind::Group);
    assert_eq!(roots[0].rect, rect(10.0, 10.0, 200.0, 100.0));
    assert_eq!(roots[0].children.len(), 2);
    assert_eq!(roots[0].children[0].rect, rect(0.0, 0.0, 100.0, 80.0));
    assert_eq!(roots[0].children[1].rect, rect(140.0, 40.0, 60.0, 60.0));
    let group_id = roots[0].id;

    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().page(page).unwrap().elements.len(), 2);
    session.redo(|_| Ok(())).unwrap();

    let before = step(&session);
    session
        .execute(
            session.project().revision,
            Command::UngroupElements {
                page_id: page,
                id: group_id,
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(step(&session), before + 1);
    let roots = &session.project().page(page).unwrap().elements;
    assert_eq!(roots.len(), 2);
    assert_eq!(roots[0].rect, rect(10.0, 10.0, 100.0, 80.0));
    assert_eq!(roots[1].rect, rect(150.0, 50.0, 60.0, 60.0));
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().page(page).unwrap().elements.len(), 1);
    assert_eq!(
        session.project().page(page).unwrap().elements[0].kind,
        ElementKind::Group
    );
}

// ---------------------------------------------------------------------------
// Linked elements
// ---------------------------------------------------------------------------

fn linked_boxes(project: &Project) -> (Project, Uuid, Uuid) {
    let left = Uuid::new_v4();
    let right = Uuid::new_v4();
    let mut first = element(left, ElementKind::Box, rect(0.0, 0.0, 200.0, 100.0));
    first.children = vec![text(Uuid::new_v4(), "Shared")];
    let mut second = element(right, ElementKind::Box, rect(300.0, 0.0, 200.0, 100.0));
    second.children = vec![text(Uuid::new_v4(), "Other")];
    let project = insert(project, vec![first, second]).unwrap();
    let project = apply(
        &project,
        Command::LinkElements {
            ids: vec![left, right],
            source_id: Some(left),
        },
    )
    .unwrap();
    (project, left, right)
}

#[test]
fn nested_path_ids_are_stable_and_match_the_shared_algorithm() {
    let root = Uuid::parse_str("12345678-1234-4234-8234-1234567890ab").unwrap();
    assert_eq!(
        nested_element_id(root, &[0]).to_string(),
        "12345678-1234-4234-8234-12343e44d4c0"
    );
    assert_eq!(
        nested_element_id(root, &[1, 2]).to_string(),
        "12345678-1234-4234-8234-12344a75f0a3"
    );
    assert_eq!(
        nested_element_id(root, &[0, 1, 2]).to_string(),
        "12345678-1234-4234-8234-1234cc84f503"
    );
}

#[test]
fn link_requires_the_same_structure() {
    let project = project();
    let box_id = Uuid::new_v4();
    let text_id = Uuid::new_v4();
    let mut boxed = element(box_id, ElementKind::Box, rect(0.0, 0.0, 200.0, 100.0));
    boxed.children = vec![text(Uuid::new_v4(), "Inside")];
    let project = insert(&project, vec![boxed, text(text_id, "Loose")]).unwrap();
    let error = apply(
        &project,
        Command::LinkElements {
            ids: vec![box_id, text_id],
            source_id: None,
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("Different structure"),
        "unexpected error: {error}"
    );
}

#[test]
fn linked_copies_resolve_one_definition_and_edit_together() {
    let (project, left, right) = linked_boxes(&project());
    let definition = project.linked.values().next().cloned().unwrap();
    assert_eq!(project.linked.len(), 1);
    assert_eq!(definition.name, "Rectangle 1");

    let resolved_left = resolve_element(&project, project.element(left).unwrap().1);
    let resolved_right = resolve_element(&project, project.element(right).unwrap().1);
    assert_eq!(resolved_left.children[0].text, "Shared");
    assert_eq!(resolved_right.children[0].text, "Shared");
    assert_eq!(resolved_left.rect.x, 0.0);
    assert_eq!(resolved_right.rect.x, 300.0);
    assert_eq!(
        resolved_left.children[0].id,
        nested_element_id(left, &[0])
    );
    assert_eq!(
        resolved_right.children[0].id,
        nested_element_id(right, &[0])
    );
    assert_ne!(
        resolved_left.children[0].id,
        resolved_right.children[0].id,
        "each copy keeps its own nested ids"
    );

    // Editing a nested layer through one copy changes the shared definition, one undo step.
    let nested_id = nested_element_id(left, &[0]);
    let mut update = patch(nested_id);
    update.text = Some("Edited once".into());
    let page = page_id(&project);
    let step = project.revision;
    let next = project
        .apply(
            step,
            Command::UpdateElements {
                page_id: page,
                patches: vec![update],
            },
        )
        .unwrap();
    assert_eq!(
        next.linked[&definition.id].master.children[0].text,
        "Edited once"
    );
    assert_eq!(
        resolve_element(&next, next.element(right).unwrap().1).children[0].text,
        "Edited once"
    );
    assert_eq!(next.revision, step + 1);

    let location = desegnos_core::link_location(&next, nested_id).unwrap();
    assert_eq!(location.definition, definition.id);
    assert_eq!(location.instance, left);
    assert_eq!(location.path, vec![0]);
    assert!(!location.is_copy());
    assert!(next.element_subtree_ids(left).contains(&nested_id));

    // Unlinking one copy detaches it with the current content.
    let unlinked = apply(
        &next,
        Command::UnlinkElements {
            ids: vec![left],
        },
    )
    .unwrap();
    assert!(unlinked.element(left).unwrap().1.link.is_none());
    assert!(!unlinked.element(left).unwrap().1.children.is_empty());
    assert_eq!(
        resolve_element(&unlinked, unlinked.element(right).unwrap().1).children[0].text,
        "Edited once"
    );

    // Editing the detached copy no longer changes the other copy.
    let mut detach_update = patch(nested_id);
    detach_update.text = Some("Only left".into());
    let unlinked = apply(
        &unlinked,
        Command::UpdateElements {
            page_id: page,
            patches: vec![detach_update],
        },
    )
    .unwrap();
    assert_eq!(unlinked.element(nested_id).unwrap().1.text, "Only left");
    assert_eq!(
        resolve_element(&unlinked, unlinked.element(right).unwrap().1).children[0].text,
        "Edited once"
    );

    let error = apply(
        &unlinked,
        Command::RemoveLinkedDefinition {
            id: definition.id,
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("still used"));

    let unlinked = apply(
        &unlinked,
        Command::UnlinkElements {
            ids: vec![right],
        },
    )
    .unwrap();
    apply(
        &unlinked,
        Command::RemoveLinkedDefinition {
            id: definition.id,
        },
    )
    .unwrap();
}

#[test]
fn linking_creates_a_named_definition_when_unnamed() {
    let left = Uuid::new_v4();
    let right = Uuid::new_v4();
    // Built on the page directly, so both layers keep a missing name; insert would number them.
    let mut start = project();
    start.pages[0]
        .elements
        .push(element(left, ElementKind::Box, rect(0.0, 0.0, 50.0, 50.0)));
    start.pages[0]
        .elements
        .push(element(right, ElementKind::Box, rect(60.0, 0.0, 50.0, 50.0)));
    let project = apply(
        &start,
        Command::LinkElements {
            ids: vec![left, right],
            source_id: None,
        },
    )
    .unwrap();
    let definition = project.linked.values().next().unwrap();
    assert_eq!(definition.name, "Box");
    assert_eq!(project.element(left).unwrap().1.name, None);
    assert_eq!(project.element(right).unwrap().1.name, None);
}

// ---------------------------------------------------------------------------
// Linked sizes
// ---------------------------------------------------------------------------

fn project_with_phone() -> (Project, Uuid, Uuid, Uuid) {
    let element_id = Uuid::new_v4();
    let project = insert(&project(), vec![text(element_id, "Shared text")]).unwrap();
    let page = page_id(&project);
    let base = base_size_id(&project);
    let phone = Uuid::new_v4();
    let project = apply(
        &project,
        Command::AddPageSize {
            page_id: page,
            size: PageSize {
                id: phone,
                label: "Phone".into(),
                width: 390,
                height: 844,
                kind: desegnos_core::BoundaryKind::Page,
                x: 1200.0,
                y: 0.0,
            },
            moves: vec![],
            reference: vec![],
        },
    )
    .unwrap();
    (project, page, base, phone)
}

#[test]
fn size_override_edits_only_that_size_and_all_sizes_edits_clear_overrides() {
    let (project, page, base, phone) = project_with_phone();
    let element_id = project.page(page).unwrap().elements[0].id;

    let mut sized = patch(element_id);
    sized.rect = Some(rect(40.0, 60.0, 200.0, 30.0));
    sized.text = Some("Edited in phone".into());
    sized.size_id = Some(phone);
    let project = apply(
        &project,
        Command::UpdateElements {
            page_id: page,
            patches: vec![sized],
        },
    )
    .unwrap();
    let page_ref = project.page(page).unwrap();
    let phone_tree = resolve_for_size(&project, page_ref, phone);
    let base_tree = resolve_for_size(&project, page_ref, base);
    assert_eq!(phone_tree[0].rect, rect(40.0, 60.0, 200.0, 30.0));
    assert_eq!(base_tree[0].rect, rect(10.0, 20.0, 120.0, 24.0));
    assert_eq!(
        project.page(page).unwrap().elements[0].overrides[&phone].rect,
        Some(rect(40.0, 60.0, 200.0, 30.0))
    );
    assert_eq!(phone_tree[0].text, "Edited in phone");
    assert_eq!(base_tree[0].text, "Edited in phone", "text is shared");

    // Base-size edits change the inherited value and leave the phone override alone.
    let mut base_patch = patch(element_id);
    base_patch.rect = Some(rect(5.0, 6.0, 130.0, 26.0));
    let project = apply(
        &project,
        Command::UpdateElements {
            page_id: page,
            patches: vec![base_patch],
        },
    )
    .unwrap();
    let page_ref = project.page(page).unwrap();
    assert_eq!(resolve_for_size(&project, page_ref, base)[0].rect.x, 5.0);
    assert_eq!(resolve_for_size(&project, page_ref, phone)[0].rect.x, 40.0);

    // Double-click editing changes the base and clears every size's override.
    let mut all = patch(element_id);
    all.rect = Some(rect(70.0, 80.0, 140.0, 28.0));
    all.all_sizes = true;
    let project = apply(
        &project,
        Command::UpdateElements {
            page_id: page,
            patches: vec![all],
        },
    )
    .unwrap();
    let page_ref = project.page(page).unwrap();
    assert_eq!(
        resolve_for_size(&project, page_ref, base)[0].rect,
        rect(70.0, 80.0, 140.0, 28.0)
    );
    assert_eq!(
        resolve_for_size(&project, page_ref, phone)[0].rect,
        rect(70.0, 80.0, 140.0, 28.0)
    );
    assert!(
        !project.page(page).unwrap().elements[0]
            .overrides
            .contains_key(&phone),
        "the all-sizes edit removed the override"
    );
}

#[test]
fn new_layers_are_shared_and_can_still_be_hidden_per_size() {
    let (project, page, base, phone) = project_with_phone();
    let added = Uuid::new_v4();
    let project = apply(
        &project,
        Command::InsertElements {
            page_id: page,
            parent_id: None,
            index: 1,
            elements: vec![text(added, "Shared from phone")],
            picture_data_uri: None,
            size_id: Some(phone),
        },
    )
    .unwrap();
    let page_ref = project.page(page).unwrap();
    let base_element = resolve_for_size(&project, page_ref, base)
        .into_iter()
        .find(|element| element.id == added)
        .unwrap();
    let phone_element = resolve_for_size(&project, page_ref, phone)
        .into_iter()
        .find(|element| element.id == added)
        .unwrap();
    assert!(base_element.visible, "new elements are shared with the base size");
    assert!(phone_element.visible);
    assert!(!project.element(added).unwrap().1.overrides.contains_key(&base));

    // Explicit This size edits can still hide the element in one size without a ghost.
    let mut dismiss = patch(added);
    dismiss.visible = Some(false);
    dismiss.dismissed = Some(true);
    dismiss.size_id = Some(phone);
    let dismissed = apply(
        &project,
        Command::UpdateElements {
            page_id: page,
            patches: vec![dismiss],
        },
    )
    .unwrap();
    let page_ref = dismissed.page(page).unwrap();
    let phone_element = resolve_for_size(&dismissed, page_ref, phone)
        .into_iter()
        .find(|element| element.id == added)
        .unwrap();
    assert!(!phone_element.visible);
    let over = &dismissed.element(added).unwrap().1.overrides[&phone];
    assert!(over.hidden && over.dismissed);

    // Including it in the phone size clears the placeholder override.
    let mut include = patch(added);
    include.visible = Some(true);
    include.size_id = Some(phone);
    let included = apply(
        &dismissed,
        Command::UpdateElements {
            page_id: page,
            patches: vec![include],
        },
    )
    .unwrap();
    let page_ref = included.page(page).unwrap();
    let phone_element = resolve_for_size(&included, page_ref, phone)
        .into_iter()
        .find(|element| element.id == added)
        .unwrap();
    assert!(phone_element.visible);
    assert!(
        !included.element(added).unwrap().1.overrides.contains_key(&phone),
        "including removed the empty override"
    );

    // Dismissing in the phone size leaves the phone placeholder state on its own.
    let mut phone_dismiss = patch(added);
    phone_dismiss.visible = Some(false);
    phone_dismiss.dismissed = Some(true);
    phone_dismiss.size_id = Some(phone);
    let phone_dismissed = apply(
        &included,
        Command::UpdateElements {
            page_id: page,
            patches: vec![phone_dismiss],
        },
    )
    .unwrap();
    let over = &phone_dismissed.element(added).unwrap().1.overrides[&phone];
    assert!(over.hidden && over.dismissed);
}

#[test]
fn new_layers_scale_x_and_only_shrink_width_when_narrower_sizes_need_it() {
    let (project, page, base, phone) = project_with_phone();
    let wide = Uuid::new_v4();
    let small = Uuid::new_v4();
    let project = apply(
        &project,
        Command::InsertElements {
            page_id: page,
            parent_id: None,
            index: 1,
            elements: vec![
                element(wide, ElementKind::Box, rect(400.0, 20.0, 300.0, 40.0)),
                element(small, ElementKind::Box, rect(100.0, 80.0, 100.0, 40.0)),
            ],
            picture_data_uri: None,
            size_id: Some(base),
        },
    ).unwrap();
    let page_ref = project.page(page).unwrap();
    let desktop = resolve_for_size(&project, page_ref, base);
    let phone_tree = resolve_for_size(&project, page_ref, phone);
    let desktop_wide = desktop.iter().find(|element| element.id == wide).unwrap();
    let phone_wide = phone_tree.iter().find(|element| element.id == wide).unwrap();
    let phone_small = phone_tree.iter().find(|element| element.id == small).unwrap();
    assert_eq!(desktop_wide.rect, rect(400.0, 20.0, 300.0, 40.0));
    assert_eq!(phone_wide.rect, rect(195.0, 20.0, 195.0, 40.0));
    assert_eq!(phone_small.rect, rect(48.75, 80.0, 100.0, 40.0));
    assert!(phone_wide.visible && phone_small.visible && desktop_wide.visible);
}

#[test]
fn placing_text_and_typing_it_is_one_undo_step() {
    let mut session = Session::new(project()).unwrap();
    let id = Uuid::new_v4();
    let page = page_id(session.project());
    let before = step(&session);
    let revision = session.project().revision;
    session
        .execute(
            revision,
            Command::InsertElements {
                page_id: page,
                parent_id: None,
                index: 0,
                elements: vec![text(id, "")],
                picture_data_uri: None,
                size_id: None,
            },
            |_| Ok(()),
        )
        .unwrap();
    // The editor closing ends the step before its text arrives.
    session.end_history_step();
    let mut typed = patch(id);
    typed.text = Some("hello".into());
    let revision = session.project().revision;
    session
        .execute(
            revision,
            Command::UpdateElements {
                page_id: page,
                patches: vec![typed],
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(step(&session), before + 1);
    let mut edited = patch(id);
    edited.text = Some("hello again".into());
    let revision = session.project().revision;
    session
        .execute(
            revision,
            Command::UpdateElements {
                page_id: page,
                patches: vec![edited],
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(
        step(&session),
        before + 2,
        "later text edits are their own step"
    );
    session.undo(|_| Ok(())).unwrap();
    session.undo(|_| Ok(())).unwrap();
    assert!(session.project().page(page).unwrap().elements.is_empty());
}

#[test]
fn reference_round_trip_preserves_shapes_text_children_pictures_and_history() {
    use base64::Engine;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(2, 2)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
    );
    for kind in [
        ElementKind::Ellipse,
        ElementKind::Text,
        ElementKind::Group,
        ElementKind::Image,
        ElementKind::Line,
    ] {
        let mut project = project();
        let (picture, uri) = Project::prepare_picture(&uri, 2).unwrap();
        project.pictures.insert(picture, uri.into());
        let page = page_id(&project);
        let mut original = element(
            Uuid::new_v4(),
            kind.clone(),
            rect(
                10.0,
                20.0,
                90.0,
                if kind == ElementKind::Line { 0.0 } else { 60.0 },
            ),
        );
        original.name = Some("My ellipse is not a prompt".into());
        original.text = "hello".into();
        original.style.fill = Some("#123456".into());
        original.style.opacity = 0.7;
        original.rotation = 15.0;
        original.meta.requests.push(desegnos_core::RequestRecord {
            id: Uuid::new_v4(),
            prompt: "An earlier explicit request".into(),
            attachments: vec![],
            created_at: "test".into(),
            outcome: None,
            job_id: None,
        });
        if kind == ElementKind::Group {
            let child = text(Uuid::new_v4(), "Nested text");
            original.children.push(child);
        }
        if kind == ElementKind::Image {
            original.image = Some(picture);
        }
        let mut project = insert(&project, vec![original.clone()]).unwrap();
        project = apply(
            &project,
            Command::ElementToReference {
                page_id: page,
                element_id: original.id,
                rect: rect(-200.0, -100.0, 90.0, original.rect.height),
                size_id: None,
            },
        )
        .unwrap();
        assert!(project.page(page).unwrap().elements.is_empty());
        assert!(project.requests.is_empty());
        let stored = match &project.reference[0].kind {
            desegnos_core::ReferenceKind::Element { element, .. } => element,
            _ => panic!("a design layer must stay a design layer"),
        };
        assert_eq!(stored.as_ref(), &original);
        if kind == ElementKind::Image {
            assert!(project.pictures.contains_key(&picture));
        }
        let reopened: Project =
            serde_json::from_slice(&serde_json::to_vec(&project).unwrap()).unwrap();
        let restored = apply(
            &reopened,
            Command::ReferenceToElement {
                item_id: reopened.reference[0].id,
                page_id: page,
                rect: original.rect,
                size_id: None,
            },
        )
        .unwrap();
        assert_eq!(restored.page(page).unwrap().elements, vec![original]);
        assert!(restored.reference.is_empty());
        assert!(
            restored.requests.is_empty(),
            "moving a layer does not create an AI request"
        );
    }
}

#[test]
fn reference_keeps_linked_size_overrides_and_existing_requests() {
    use desegnos_core::{PendingRequest, ReferenceKind, RequestTarget};
    let (project, page, _, phone) = project_with_phone();
    let id = project.page(page).unwrap().elements[0].id;
    let mut edit = patch(id);
    edit.size_id = Some(phone);
    edit.rect = Some(rect(20.0, 30.0, 80.0, 40.0));
    let project = apply(
        &project,
        Command::UpdateElements {
            page_id: page,
            patches: vec![edit],
        },
    )
    .unwrap();
    let original = project.page(page).unwrap().elements[0].clone();
    let request = PendingRequest {
        id: Uuid::new_v4(),
        text: "Make the text larger".into(),
        attachments: vec![],
        targets: vec![RequestTarget::Element {
            page,
            size: Some(phone),
            id,
        }],
        area: None,
        requested_at: "test".into(),
    };
    let project = apply(
        &project,
        Command::AddRequest {
            request: request.clone(),
        },
    )
    .unwrap();
    let project = apply(
        &project,
        Command::ElementToReference {
            page_id: page,
            element_id: id,
            rect: rect(-200.0, 0.0, 80.0, 40.0),
            size_id: Some(phone),
        },
    )
    .unwrap();
    assert!(
        matches!(&project.reference[0].kind, ReferenceKind::Element { element, size } if element.as_ref() == &original && *size == Some(phone))
    );
    let restored = apply(
        &project,
        Command::ReferenceToElement {
            item_id: project.reference[0].id,
            page_id: page,
            rect: rect(20.0, 30.0, 80.0, 40.0),
            size_id: Some(phone),
        },
    )
    .unwrap();
    assert_eq!(restored.page(page).unwrap().elements[0], original);
    assert_eq!(restored.requests, vec![request]);
}

#[test]
fn reference_notes_create_only_draft_requests_using_note_content() {
    let project = project();
    let page = page_id(&project);
    let item = desegnos_core::ReferenceItem {
        id: Uuid::new_v4(),
        rect: rect(-200.0, 0.0, 100.0, 80.0),
        kind: desegnos_core::ReferenceKind::Note {
            text: "Use a calm blue background".into(),
        },
    };
    let project = apply(
        &project,
        Command::AddReferenceItem {
            item: item.clone(),
            picture_data_uri: None,
        },
    )
    .unwrap();
    let project = apply(
        &project,
        Command::ReferenceToElement {
            item_id: item.id,
            page_id: page,
            rect: rect(10.0, 20.0, 100.0, 80.0),
            size_id: None,
        },
    )
    .unwrap();
    assert_eq!(project.requests.len(), 1);
    assert_eq!(project.requests[0].text, "Use a calm blue background");
    assert!(project.latest_generation.is_none());
    assert!(project.applied_job_ids.is_empty());
}

#[test]
fn stored_reference_trees_still_validate_nested_identifiers() {
    let mut project = project();
    let mut group = element(
        Uuid::new_v4(),
        ElementKind::Group,
        rect(0.0, 0.0, 100.0, 80.0),
    );
    group.children = vec![text(group.id, "Duplicate id")];
    project.reference.push(desegnos_core::ReferenceItem {
        id: Uuid::new_v4(),
        rect: rect(-200.0, 0.0, 100.0, 80.0),
        kind: desegnos_core::ReferenceKind::Element {
            element: Box::new(group),
            size: None,
        },
    });
    assert!(project.validate().is_err());
}

#[test]
fn repeated_nudges_use_current_positions_and_share_one_undo_step() {
    let project = project();
    let page = page_id(&project);
    let layer = text(Uuid::new_v4(), "Nudge");
    let id = layer.id;
    let initial = layer.rect;
    let project = insert(&project, vec![layer]).unwrap();
    let mut session = Session::new(project).unwrap();
    for _ in 0..5 {
        session.execute(session.project().revision, Command::NudgeElements {
            moves: vec![ElementMove {
                id, to_page_id: page, to_parent_id: None, index: 0,
                rect: initial, size_id: None, all_sizes: true,
            }],
            dx: 1.0, dy: 0.0,
        }, |_| Ok(())).unwrap();
    }
    assert_eq!(session.project().element(id).unwrap().1.rect.x, initial.x + 5.0);
    assert_eq!(step(&session), 1);
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().element(id).unwrap().1.rect, initial);
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().element(id).unwrap().1.rect.x, initial.x + 5.0);
}

#[test]
fn stack_wraps_a_single_text_layer_and_converts_a_box_in_place() {
    let project = project();
    let page = page_id(&project);
    let layer = text(Uuid::new_v4(), "Stack");
    let id = layer.id;
    let project = insert(&project, vec![layer]).unwrap();
    let wrapped = apply(&project, Command::StackElements { page_id: page, ids: vec![id] }).unwrap();
    let container = &wrapped.page(page).unwrap().elements[0];
    assert_eq!(container.kind, ElementKind::Box);
    assert!(matches!(container.layout, desegnos_core::ElementLayout::Stack { .. }));
    assert_eq!(container.children[0].id, id);
    let box_id = Uuid::new_v4();
    let project = insert(&project, vec![element(box_id, ElementKind::Box, rect(0.0, 0.0, 200.0, 100.0))]).unwrap();
    let converted = apply(&project, Command::StackElements { page_id: page, ids: vec![box_id] }).unwrap();
    let container = converted.element(box_id).unwrap().1;
    assert!(container.children.is_empty());
    assert!(matches!(container.layout, desegnos_core::ElementLayout::Stack { .. }));
}
