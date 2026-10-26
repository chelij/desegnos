use desegnos_core::*;
use desegnos_storage::{Journal, ProjectFile, load_project};
use std::{fs, sync::Arc};

const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jMZkAAAAASUVORK5CYII=";

#[test]
fn inline_pictures_deduplicate_share_history_and_survive_save_recovery_export() {
    let root =
        std::env::temp_dir().join(format!("desegnos-picture-store-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let mut old = Project::new(
        "Pictures".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    old.pages[0].content = Some(SiteContent {
        html: format!("<h1>Pictures</h1><img src='{PNG}'><img src='{PNG}'>"),
        css: format!("body{{background:url({PNG})}}"),
        javascript: String::new(),
    });
    let project = Session::new(old.clone()).unwrap().project().clone();
    assert_eq!(project.pictures.len(), 1);
    let id = *project.pictures.keys().next().unwrap();
    assert!(
        !project.pages[0]
            .content
            .as_ref()
            .unwrap()
            .html
            .contains("base64")
    );
    let resolved = project
        .resolve_site_content(project.pages[0].content.as_ref().unwrap())
        .unwrap();
    assert_eq!(resolved.html.matches(PNG).count(), 2);
    assert_eq!(resolved.css, old.pages[0].content.as_ref().unwrap().css);
    let mut again = project.clone();
    again.store_inline_images().unwrap();
    assert_eq!(again, project);
    let picture = project.pictures[&id].clone();
    let mut session = Session::new(project).unwrap();
    let journal = Journal::new(&root, session.project().id).unwrap();
    session
        .execute(
            0,
            Command::RenameProject {
                name: "Changed".into(),
            },
            |p| journal.append(p),
        )
        .unwrap();
    assert!(Arc::ptr_eq(&picture, &session.project().pictures[&id]));
    session.undo(|_| Ok(())).unwrap();
    assert!(Arc::ptr_eq(&picture, &session.project().pictures[&id]));
    session.redo(|_| Ok(())).unwrap();
    let path = root.join("site.desegnos");
    ProjectFile::acquire(&path)
        .unwrap()
        .save(session.project())
        .unwrap();
    assert_eq!(load_project(&path).unwrap(), *session.project());
    assert_eq!(
        Journal::recover(journal.path()).unwrap().unwrap().pictures[&id],
        picture
    );
    let exported = compile_website(session.project()).unwrap();
    assert_eq!(
        exported
            .files
            .keys()
            .filter(|path| path.starts_with("assets/"))
            .count(),
        1
    );
    assert!(
        exported
            .files
            .values()
            .all(|bytes| !String::from_utf8_lossy(bytes).contains(PROJECT_ASSET_SCHEME))
    );
    let mut invalid = session.project().clone();
    invalid.pictures.clear();
    assert!(invalid.validate().is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn placed_imported_pictures_outlive_the_import_catalogue() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut project = Project::new(
        "Imported picture".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    let id = uuid::Uuid::new_v4();
    project.imported_identity = Some(ImportedIdentity {
        pages: vec![],
        source_name: "Brand".into(),
        source_sha256: digest(b"source"),
        summary: "Imported brand".into(),
        assets: vec![IdentityAsset {
            id,
            name: "logo.png".into(),
            mime: "image/png".into(),
            data_uri: PNG.into(),
            sha256: digest(&STANDARD.decode(PNG.split_once(',').unwrap().1).unwrap()),
        }],
        photos: vec![],
        content: vec![],
    });
    project.pages[0].content = Some(SiteContent {
        html: format!("<img src='{}'>", project_asset_reference(id)),
        css: String::new(),
        javascript: String::new(),
    });
    project.store_inline_images().unwrap();
    project.imported_identity = None;
    project.validate().unwrap();
    assert!(
        project
            .resolve_site_content(project.pages[0].content.as_ref().unwrap())
            .unwrap()
            .html
            .contains(PNG)
    );
    assert!(
        compile_website(&project)
            .unwrap()
            .files
            .keys()
            .any(|name| name.ends_with(".png"))
    );
}

#[test]
fn a_broken_inline_picture_stays_in_the_page_instead_of_blocking_the_project() {
    // Truncated Base64 (length not a multiple of four) from an incomplete import.
    let broken = &PNG[..PNG.len() - 3];
    let mut old = Project::new(
        "Broken".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    old.pages[0].content = Some(SiteContent {
        html: format!("<img src=\"{broken}\"><img src=\"{PNG}\">"),
        css: String::new(),
        javascript: String::new(),
    });
    let project = Session::new(old).unwrap().project().clone();
    let html = &project.pages[0].content.as_ref().unwrap().html;
    assert!(html.contains(broken) && !html.contains(PNG));
    assert_eq!(project.pictures.len(), 1);
    project.validate().unwrap();
}
