use desegnos_core::{Appearance, BoundaryKind, CanvasTemplate, OutputType, Project, SizePreset};

fn untitled(template: CanvasTemplate) -> Project {
    Project::untitled("Untitled canvas".into(), Appearance::Light, template).unwrap()
}

#[test]
fn blank_untitled_has_no_boundaries_until_the_first_one_becomes_home() {
    let project = untitled(CanvasTemplate::Blank);
    assert!(project.pages.is_empty());
    // The first boundary a blank project gets starts at the Desktop size.
    assert_eq!(
        SizePreset::Desktop.dimensions(0, 0),
        ("Desktop".into(), 1440, 900)
    );
}

#[test]
fn website_starts_with_linked_desktop_tablet_and_phone_sizes() {
    let project = untitled(CanvasTemplate::Website);
    assert_eq!(project.pages.len(), 1);
    let page = &project.pages[0];
    assert_eq!(page.name, "Page 1");
    assert_eq!(page.output, OutputType::Webpage);
    assert_eq!(page.path, "/");
    assert_eq!(page.id, project.home_page_id);
    let sizes: Vec<_> = page
        .sizes
        .iter()
        .map(|size| {
            (
                size.label.as_str(),
                size.width,
                size.height,
                size.kind,
                size.x,
            )
        })
        .collect();
    assert_eq!(
        sizes,
        vec![
            ("Desktop", 1440, 900, BoundaryKind::Page, 0.0),
            ("Tablet", 820, 1180, BoundaryKind::Page, 1600.0),
            ("Phone", 390, 844, BoundaryKind::Page, 2580.0),
        ]
    );
}

#[test]
fn app_template_creates_a_fixed_page_and_social_image_keeps_its_size() {
    let app = untitled(CanvasTemplate::AppScreen);
    assert_eq!(app.pages.len(), 1);
    assert_eq!(app.pages[0].name, "Page 1");
    assert_eq!(app.pages[0].output, OutputType::Webpage);
    assert_eq!(app.pages[0].base_size().width, 390);
    assert_eq!(app.pages[0].base_size().height, 844);
    assert_eq!(app.pages[0].base_size().kind, BoundaryKind::Screen);

    let image = untitled(CanvasTemplate::SocialImage);
    assert_eq!(image.pages.len(), 1);
    assert_eq!(image.pages[0].name, "Image 1");
    assert_eq!(image.pages[0].output, OutputType::Image);
    assert_eq!(image.pages[0].base_size().width, 1080);
    assert_eq!(image.pages[0].base_size().height, 1080);
    assert_eq!(image.pages[0].base_size().kind, BoundaryKind::Screen);
}

#[test]
fn slide_deck_starts_with_three_slides_side_by_side() {
    let project = untitled(CanvasTemplate::SlideDeck);
    assert_eq!(project.pages.len(), 3);
    assert_eq!(project.home_page_id, project.pages[0].id);
    let mut paths = std::collections::BTreeSet::new();
    for (index, page) in project.pages.iter().enumerate() {
        assert_eq!(page.name, format!("Slide {}", index + 1));
        assert_eq!(page.output, OutputType::Slide);
        assert!(paths.insert(page.path.clone()));
        assert_eq!(page.base_size().width, 1920);
        assert_eq!(page.base_size().height, 1080);
        assert_eq!(page.base_size().kind, BoundaryKind::Screen);
        assert_eq!(page.base_size().x, (index as u32 * 2080) as f64);
        assert_eq!(page.base_size().y, 0.0);
    }
}

#[test]
fn templates_do_not_add_undo_steps() {
    let session = desegnos_core::Session::new(untitled(CanvasTemplate::Website)).unwrap();
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert_eq!(session.project().revision, 0);
}

#[test]
fn template_kind_survives_deleting_every_boundary_and_serialization() {
    for template in [
        CanvasTemplate::Blank,
        CanvasTemplate::Website,
        CanvasTemplate::AppScreen,
        CanvasTemplate::SlideDeck,
        CanvasTemplate::SocialImage,
    ] {
        let mut project = untitled(template);
        for id in project.pages.iter().map(|page| page.id).collect::<Vec<_>>() {
            project = project
                .apply(
                    project.revision,
                    desegnos_core::Command::DeletePage { page_id: id },
                )
                .unwrap();
        }
        assert!(project.pages.is_empty());
        let reopened: Project =
            serde_json::from_slice(&serde_json::to_vec(&project).unwrap()).unwrap();
        reopened.validate().unwrap();
        assert_eq!(reopened.template, template);
    }
}
