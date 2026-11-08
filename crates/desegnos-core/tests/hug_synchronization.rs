use desegnos_core::{
    Appearance, BoundaryKind, Command, ElementKind, ElementMove, ElementOverride, Project, Rect,
    SizingMode, Viewport, empty_element,
};

#[test]
fn canvas_edits_preserve_fixed_geometry_and_synchronize_hug() {
    let mut project = Project::new(
        "Canvas edits".into(),
        Viewport {
            width: 1080,
            height: 1080,
        },
        Appearance::Light,
    )
    .unwrap();
    project.pages[0].sizes[0].kind = BoundaryKind::Screen;
    let page_id = project.pages[0].id;
    project.pages[0].elements = (0..640)
        .map(|i| {
            empty_element(
                ElementKind::Box,
                Rect {
                    x: f64::from(i % 32 * 32),
                    y: f64::from(i / 32 * 32),
                    width: 24.0,
                    height: 24.0,
                },
            )
        })
        .collect();
    project.validate().unwrap();
    for count in [1, 640] {
        let moves: Vec<_> = project.pages[0]
            .elements
            .iter()
            .take(count)
            .enumerate()
            .map(|(index, element)| ElementMove {
                id: element.id,
                to_page_id: page_id,
                to_parent_id: None,
                index,
                rect: Rect {
                    x: element.rect.x + 8.0,
                    ..element.rect
                },
                size_id: None,
                all_sizes: false,
            })
            .collect();
        for _ in 0..2 {
            let changed = project
                .apply(
                    project.revision,
                    Command::MoveElements {
                        moves: moves.clone(),
                    },
                )
                .unwrap();
            assert_eq!(changed.pages[0].elements.len(), 640);
            assert_eq!(changed.pages[0].elements[0].rect.x, 8.0);
        }
    }
    let mut copies = project.pages[0].elements[..320].to_vec();
    for element in &mut copies {
        element.id = uuid::Uuid::new_v4();
    }
    for _ in 0..2 {
        let changed = project
            .apply(
                project.revision,
                Command::InsertElements {
                    page_id,
                    parent_id: None,
                    index: 640,
                    elements: copies.clone(),
                    picture_data_uri: None,
                    size_id: None,
                },
            )
            .unwrap();
        assert_eq!(changed.pages[0].elements.len(), 960);
    }
    let mut child = empty_element(
        ElementKind::Box,
        Rect {
            x: 20.0,
            y: 10.0,
            width: 40.0,
            height: 30.0,
        },
    );
    let mut parent = empty_element(
        ElementKind::Box,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        },
    );
    parent.children.push(child.clone());
    parent.style.sizing.width = SizingMode::Fit;
    parent.style.sizing.height = SizingMode::Fit;
    child.id = uuid::Uuid::new_v4();
    child.children = vec![parent.clone()];
    project.pages[0].elements = vec![child];
    let changed = project
        .apply(
            project.revision,
            Command::SetBrief {
                brief: project.brief.clone(),
            },
        )
        .unwrap();
    assert_eq!(changed.pages[0].elements[0].children[0].rect.width, 60.0);
    assert_eq!(
        changed.pages[0].elements[0].rect.width, 40.0,
        "fixed ancestor stays fixed"
    );
    let style = parent.style.clone();
    parent.style.sizing.width = SizingMode::Fixed;
    parent.style.sizing.height = SizingMode::Fixed;
    let size = project.pages[0].sizes[0].id;
    parent.overrides.insert(
        size,
        ElementOverride {
            style: Some(style),
            ..Default::default()
        },
    );
    project.pages[0].elements = vec![parent];
    let changed = project
        .apply(
            project.revision,
            Command::SetBrief {
                brief: project.brief.clone(),
            },
        )
        .unwrap();
    assert_eq!(changed.pages[0].elements[0].rect.width, 200.0);
    let rect = changed.pages[0].elements[0].overrides[&size].rect.unwrap();
    assert_eq!((rect.width, rect.height), (60.0, 40.0));
}
