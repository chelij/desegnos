use desegni_core::{Appearance, Command, Project, SiteContent, Viewport, compile_website};
use desegni_storage::{
    ProjectFile, create_project_folder, import_website_folder, is_project_folder, load_project,
    project_in_folder, sanitize_project_name,
};
use std::{collections::BTreeMap, fs, path::PathBuf};
use uuid::Uuid;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("desegnos-project-folder-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn project() -> Project {
    let mut project = Project::new(
        "My website".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap();
    project.pages[0].content = Some(SiteContent {
        html: "<main><h1>Home</h1></main>".into(),
        css: "h1 { color: blue; }".into(),
        javascript: "document.title = 'Saved';".into(),
    });
    project
}
fn managed(temp: &Temp, project: &Project) -> ProjectFile {
    let path = create_project_folder(&temp.0, &project.name, project.id).unwrap();
    ProjectFile::acquire(&path).unwrap()
}
fn fixture(root: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    let files = BTreeMap::from([
        ("index.html".into(), br#"<!doctype html><html><head><title>Imported studio</title><link rel="stylesheet" href="css/site.css"><script src="main.js" defer></script></head><body><h1>Welcome</h1><a href="about.html#team">Team</a><img src="pictures/pixel.png" srcset="pictures/pixel.png 1x, pictures/pixel.png 2x"></body></html>"#.to_vec()),
        ("about.html".into(), br#"<html><head><title>About</title></head><body><h1 id="team">Team</h1><a href="/index.html">Home</a></body></html>"#.to_vec()),
        ("css/site.css".into(), br#"@import 'colours.css'; body { background: url('../pictures/pixel.png'); color: #245583; }"#.to_vec()),
        ("css/colours.css".into(), b"h1 { color: purple; }".to_vec()),
        ("main.js".into(), b"document.querySelector('h1').dataset.ready = 'yes';".to_vec()),
        ("pictures/pixel.png".into(), {
            use base64::{Engine, engine::general_purpose::STANDARD};
            STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=").unwrap()
        }),
    ]);
    for (p, bytes) in &files {
        let path = root.join(p);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    files
}

#[test]
fn portable_names_and_numbered_collision_reservations() {
    for (source, expected) in [
        ("..", "Website"),
        ("  My project. ", "My project"),
        ("a/b:c*?\"<>|\\", "a_b_c_______"),
        ("NUL.txt", "_NUL.txt"),
        ("COM1", "_COM1"),
        ("lpt²", "_lpt²"),
        ("Hello\nworld", "Hello_world"),
        ("我的网站", "我的网站"),
    ] {
        assert_eq!(sanitize_project_name(source), expected, "{source}");
    }
    assert!(sanitize_project_name(&"é".repeat(200)).len() <= 180);
    let temp = Temp::new();
    let first = create_project_folder(&temp.0, "My Project", Uuid::new_v4()).unwrap();
    let second = create_project_folder(&temp.0, "My Project", Uuid::new_v4()).unwrap();
    let third = create_project_folder(&temp.0, "my project", Uuid::new_v4()).unwrap();
    assert_eq!(first, temp.0.join("My Project/My Project.desegnos"));
    assert_eq!(second, temp.0.join("My Project 2/My Project 2.desegnos"));
    assert_eq!(third, temp.0.join("my project 3/my project 3.desegnos"));
    assert!(is_project_folder(&first));
}

#[test]
fn save_commits_full_archive_and_exact_export_and_updates_both() {
    let temp = Temp::new();
    let mut project = project();
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    assert_eq!(load_project(file.path()).unwrap(), project);
    for (p, bytes) in compile_website(&project).unwrap().files {
        assert_eq!(fs::read(root.join(p)).unwrap(), bytes);
    }
    project = project
        .apply(
            project.revision,
            Command::RenameProject {
                name: "Renamed website".into(),
            },
        )
        .unwrap();
    project.pages[0].content.as_mut().unwrap().html = "<h1>Changed</h1>".into();
    file.save(&project).unwrap();
    assert_eq!(load_project(file.path()).unwrap(), project);
    assert!(
        fs::read_to_string(root.join("index.html"))
            .unwrap()
            .contains("Changed")
    );
    assert!(
        file.path()
            .with_file_name("My website.desegnos.previous")
            .exists()
    );
}

#[test]
fn blank_project_also_writes_a_live_index_without_changing_its_document() {
    let temp = Temp::new();
    let mut project = project();
    project.pages[0].content = None;
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    assert!(file.path().parent().unwrap().join("index.html").is_file());
    assert_eq!(load_project(file.path()).unwrap(), project);
    assert!(file.site_warnings().is_empty());
}

#[test]
fn stale_owned_files_are_removed_but_foreign_files_and_modified_outputs_survive() {
    let temp = Temp::new();
    let mut project = project();
    project = project
        .apply(
            0,
            Command::AddPage {
                name: "About".into(),
                path: "/about".into(),
            },
        )
        .unwrap();
    project.pages[1].content = Some(SiteContent {
        html: "<h1>About</h1>".into(),
        css: String::new(),
        javascript: String::new(),
    });
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    fs::write(root.join("about/keep.txt"), "Foreign file").unwrap();
    fs::write(root.join("notes.txt"), "Private notes").unwrap();
    fs::write(root.join("styles.css"), "User-edited styles").unwrap();
    project.pages.pop();
    project.pages[0].content.as_mut().unwrap().css.clear();
    project.revision += 1;
    file.save(&project).unwrap();
    assert!(!root.join("about/index.html").exists());
    assert_eq!(
        fs::read_to_string(root.join("about/keep.txt")).unwrap(),
        "Foreign file"
    );
    assert_eq!(
        fs::read_to_string(root.join("notes.txt")).unwrap(),
        "Private notes"
    );
    assert_eq!(
        fs::read_to_string(root.join("styles.css")).unwrap(),
        "User-edited styles"
    );
}

#[test]
fn foreign_collisions_and_external_edits_warn_without_losing_the_project_save() {
    let temp = Temp::new();
    let mut project = project();
    let file = managed(&temp, &project);
    let root = file.path().parent().unwrap();
    fs::write(root.join("index.html"), "Original user file").unwrap();
    file.save(&project).unwrap();
    assert_eq!(load_project(file.path()).unwrap(), project);
    assert_eq!(
        fs::read_to_string(root.join("index.html")).unwrap(),
        "Original user file"
    );
    assert!(
        file.outside_edits()
            .changed_paths
            .contains(&"index.html".to_owned())
    );
    fs::remove_file(root.join("index.html")).unwrap();
    file.save(&project).unwrap();
    fs::write(root.join("index.html"), "Edited outside Desegnos").unwrap();
    project.revision += 1;
    file.save(&project).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("index.html")).unwrap(),
        "Edited outside Desegnos"
    );
    assert_eq!(load_project(file.path()).unwrap(), project);
    assert!(
        file.outside_edits()
            .changed_paths
            .contains(&"index.html".to_owned())
    );
}

#[test]
fn real_site_write_failure_never_rolls_back_the_archive() {
    let temp = Temp::new();
    let mut project = project();
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    fs::remove_file(root.join("index.html")).unwrap();
    fs::create_dir(root.join("index.html")).unwrap();
    project.revision += 1;
    project.name = "Saved despite failure".into();
    file.save(&project).unwrap();
    assert_eq!(load_project(file.path()).unwrap(), project);
    assert!(
        file.site_warnings()
            .iter()
            .any(|s| s.contains("could not be synced"))
    );
    assert!(!fs::read_dir(root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".desegnos-site-stage-")
    }));
}

#[test]
fn interrupted_site_publication_is_recognized_and_converges_on_retry() {
    let temp = Temp::new();
    let project = project();
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    let path = root.join(".desegnos-site.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let partial = b"A partially published next revision";
    manifest["pending"] = serde_json::json!({ "index.html": desegni_core::digest(partial), "stale/index.html": desegni_core::digest(b"old") });
    fs::write(root.join("index.html"), partial).unwrap();
    fs::create_dir(root.join("stale")).unwrap();
    fs::write(root.join("stale/index.html"), b"old").unwrap();
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    file.save(&project).unwrap();
    assert_eq!(
        fs::read(root.join("index.html")).unwrap(),
        compile_website(&project).unwrap().files["index.html"]
    );
    assert!(!root.join("stale/index.html").exists());
    assert!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(path).unwrap()).unwrap()["pending"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn import_plain_folder_preserves_original_bytes_pages_styles_scripts_and_pictures() {
    let temp = Temp::new();
    let originals = fixture(&temp.0);
    let file = import_website_folder(&temp.0).unwrap();
    assert!(is_project_folder(file.path()));
    let mut project = file.load().unwrap();
    assert_eq!(project.pages.len(), 2);
    assert_eq!(project.pictures.len(), 1);
    let home = project
        .page(project.home_page_id)
        .unwrap()
        .content
        .as_ref()
        .unwrap();
    assert!(home.html.contains("data-desegnos-id="));
    assert!(home.html.contains("href=\"about.html#team\""));
    assert!(home.html.contains("src=\"main.js\""));
    assert!(home.javascript.is_empty());
    assert_eq!(compile_website(&project).unwrap().files, originals);
    let rendered = project
        .website_canvas_content(project.page(project.home_page_id).unwrap())
        .unwrap()
        .unwrap();
    assert!(rendered.html.contains("color: purple"));
    assert!(rendered.html.contains("/pictures/pixel.png"));
    assert!(rendered.html.contains("/about#team"));
    for (p, bytes) in &originals {
        assert_eq!(fs::read(temp.0.join(p)).unwrap(), *bytes, "source {p}");
        assert_eq!(
            fs::read(temp.0.join(".desegnos-original").join(p)).unwrap(),
            *bytes,
            "backup {p}"
        );
    }
    project.revision += 1;
    file.save(&project).unwrap();
    assert!(
        !file
            .site_warnings()
            .iter()
            .any(|s| s.contains("could not be synced")),
        "{:?}",
        file.site_warnings()
    );
    for (p, bytes) in compile_website(&project).unwrap().files {
        assert_eq!(fs::read(temp.0.join(p)).unwrap(), bytes);
    }
    project.revision += 1;
    file.save(&project).unwrap();
    for (p, bytes) in &originals {
        assert_eq!(
            fs::read(temp.0.join(".desegnos-original").join(p)).unwrap(),
            *bytes
        );
    }
    assert_eq!(
        fs::read(temp.0.join("about.html")).unwrap(),
        originals["about.html"]
    );
    fs::write(temp.0.join("index.html"), b"Owner edited this file").unwrap();
    project.revision += 1;
    file.save(&project).unwrap();
    assert_eq!(
        fs::read(temp.0.join("index.html")).unwrap(),
        b"Owner edited this file"
    );
    assert!(
        file.outside_edits()
            .changed_paths
            .contains(&"index.html".to_owned())
    );
}

#[test]
fn import_rejects_empty_website_and_respects_an_existing_original_backup() {
    let temp = Temp::new();
    fs::write(temp.0.join("notes.txt"), "Keep").unwrap();
    assert!(import_website_folder(&temp.0).is_err());
    assert!(!temp.0.join(".desegnos-original").exists());
    fixture(&temp.0);
    fs::create_dir(temp.0.join(".desegnos-original")).unwrap();
    fs::write(
        temp.0.join(".desegnos-original/index.html"),
        "Earlier backup",
    )
    .unwrap();
    assert!(import_website_folder(&temp.0).is_err());
    assert_eq!(
        fs::read_to_string(temp.0.join(".desegnos-original/index.html")).unwrap(),
        "Earlier backup"
    );
    assert!(project_in_folder(&temp.0).unwrap().is_none());
}

#[test]
fn interrupted_import_resumes_its_reserved_identity_without_replacing_originals() {
    let temp = Temp::new();
    let originals = fixture(&temp.0);
    let file = import_website_folder(&temp.0).unwrap();
    let identity = file.load().unwrap().id;
    let path = file.path().to_owned();
    drop(file);
    // Simulate interruption after publishing the backup and marker, before the archive.
    fs::remove_file(path).unwrap();
    let resumed = import_website_folder(&temp.0).unwrap();
    assert_eq!(resumed.load().unwrap().id, identity);
    for (path, bytes) in originals {
        assert_eq!(fs::read(temp.0.join(&path)).unwrap(), bytes);
        assert_eq!(
            fs::read(temp.0.join(".desegnos-original").join(path)).unwrap(),
            bytes
        );
    }
}

#[test]
fn replacing_a_managed_archive_keeps_its_backup_and_syncs_the_selected_project() {
    let temp = Temp::new();
    let original = project();
    let file = managed(&temp, &original);
    file.save(&original).unwrap();
    let stamp = file.stamp().unwrap().unwrap();
    let mut replacement = project();
    replacement.pages[0].content.as_mut().unwrap().html = "<h1>Replacement</h1>".into();
    file.save_replacing(&replacement, &stamp, true).unwrap();
    let root = file.path().parent().unwrap();
    assert!(file.site_warnings().is_empty());
    assert_eq!(load_project(file.path()).unwrap(), replacement);
    assert_eq!(
        load_project(&root.join("My website.desegnos.previous")).unwrap(),
        original
    );
    assert!(
        fs::read_to_string(root.join("index.html"))
            .unwrap()
            .contains("Replacement")
    );
}

#[test]
fn a_changed_original_backup_blocks_site_writes_without_blocking_archive_saves() {
    let temp = Temp::new();
    fixture(&temp.0);
    let file = import_website_folder(&temp.0).unwrap();
    let mut project = file.load().unwrap();
    project.revision += 1;
    let before = fs::read(temp.0.join("index.html")).unwrap();
    fs::write(
        temp.0.join(".desegnos-original/index.html"),
        "Changed backup",
    )
    .unwrap();
    file.save(&project).unwrap();
    assert_eq!(load_project(file.path()).unwrap(), project);
    assert_eq!(fs::read(temp.0.join("index.html")).unwrap(), before);
    assert!(
        file.site_warnings()
            .iter()
            .any(|s| s.contains("backup is missing or changed"))
    );
}

#[test]
fn opening_a_folder_finds_its_archive_and_uses_newest_with_stable_ties() {
    let temp = Temp::new();
    let project = project();
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    assert_eq!(
        project_in_folder(root).unwrap().as_deref(),
        Some(file.path())
    );
    let newer = ProjectFile::acquire(&root.join("Newer.desegni")).unwrap();
    newer.save(&project).unwrap();
    let time = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    fs::File::options()
        .write(true)
        .open(newer.path())
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(time))
        .unwrap();
    assert_eq!(
        project_in_folder(root).unwrap().as_deref(),
        Some(newer.path())
    );
}

#[test]
fn legacy_standalone_saves_do_not_claim_or_modify_the_parent_website() {
    let temp = Temp::new();
    let project = project();
    let file = ProjectFile::acquire(&temp.0.join("Legacy.desegni")).unwrap();
    fs::write(temp.0.join("index.html"), "Unrelated website").unwrap();
    file.save(&project).unwrap();
    assert!(!is_project_folder(file.path()));
    assert!(!temp.0.join(".desegnos-site.json").exists());
    assert_eq!(
        fs::read_to_string(temp.0.join("index.html")).unwrap(),
        "Unrelated website"
    );
}

#[cfg(unix)]
#[test]
fn symlink_site_paths_cannot_overwrite_or_delete_files_outside_the_project() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let mut project = project();
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    let external = temp.0.join("outside");
    fs::write(&external, "Foreign data").unwrap();
    fs::remove_file(root.join("index.html")).unwrap();
    symlink(&external, root.join("index.html")).unwrap();
    project.revision += 1;
    file.save(&project).unwrap();
    assert_eq!(fs::read_to_string(&external).unwrap(), "Foreign data");
    assert_eq!(load_project(file.path()).unwrap(), project);
    assert!(
        file.site_warnings()
            .iter()
            .any(|s| s.contains("symbolic link"))
    );
}

#[test]
fn outside_edits_are_structured_and_stale_paths_survive_repeated_saves() {
    let temp = Temp::new();
    let mut project = project();
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    fs::write(root.join("index.html"), b"outside home").unwrap();
    fs::write(root.join("styles.css"), b"outside styles").unwrap();
    project.pages[0].content.as_mut().unwrap().css.clear();
    for _ in 0..2 {
        file.save(&project).unwrap();
        let outside = file.outside_edits();
        assert_eq!(outside.changed_paths, ["index.html"]);
        assert_eq!(outside.kept_stale_paths, ["styles.css"]);
        assert!(!outside.backed_up);
        assert!(file.site_warnings().is_empty());
        assert_eq!(fs::read(root.join("index.html")).unwrap(), b"outside home");
        assert_eq!(
            fs::read(root.join("styles.css")).unwrap(),
            b"outside styles"
        );
    }
}

#[test]
fn overwrite_backs_up_exact_bytes_syncs_and_updates_manifest() {
    let temp = Temp::new();
    let mut project = project()
        .apply(
            0,
            Command::AddPage {
                name: "About".into(),
                path: "/about".into(),
            },
        )
        .unwrap();
    project.pages[1].content = project.pages[0].content.clone();
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    let outside = BTreeMap::from([
        ("index.html", b"outside home\0\xff".to_vec()),
        ("styles.css", b"outside styles".to_vec()),
        ("about/index.html", b"nested outside page".to_vec()),
    ]);
    for (path, bytes) in &outside {
        fs::write(root.join(path), bytes).unwrap();
    }
    project.pages[0].content.as_mut().unwrap().css.clear();
    project.pages.pop();
    file.save(&project).unwrap();
    file.overwrite_site(&project).unwrap();
    assert!(file.outside_edits().backed_up);
    let backups: Vec<_> = fs::read_dir(root.join(".desegnos-outside-edits"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    for (path, bytes) in &outside {
        assert_eq!(fs::read(backups[0].join(path)).unwrap(), *bytes);
    }
    assert!(!root.join("styles.css").exists());
    assert!(!root.join("about/index.html").exists());
    let desired = compile_website(&project).unwrap().files;
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".desegnos-site.json")).unwrap()).unwrap();
    assert_eq!(manifest["files"].as_object().unwrap().len(), desired.len());
    for (path, bytes) in desired {
        assert_eq!(fs::read(root.join(&path)).unwrap(), bytes);
        assert_eq!(manifest["files"][&path], desegni_core::digest(&bytes));
    }
    file.save(&project).unwrap();
    assert_eq!(file.outside_edits(), Default::default());
    assert_eq!(
        fs::read(backups[0].join("index.html")).unwrap(),
        outside["index.html"]
    );
}

#[test]
fn imported_snapshot_marks_current_bytes_owned_but_not_later_edits() {
    let temp = Temp::new();
    let project = project();
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    let imported = BTreeMap::from([
        ("index.html".into(), b"<h1>Imported home</h1>".to_vec()),
        ("nested/new.html".into(), b"<h1>New</h1>".to_vec()),
    ]);
    fs::create_dir(root.join("nested")).unwrap();
    for (path, bytes) in &imported {
        fs::write(root.join(path), bytes).unwrap();
    }
    file.save(&project).unwrap();
    file.mark_imported_site_owned(&imported).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".desegnos-site.json")).unwrap()).unwrap();
    for (path, bytes) in &imported {
        assert_eq!(manifest["files"][path], desegni_core::digest(bytes));
    }
    file.save(&project).unwrap();
    assert!(file.outside_edits().changed_paths.is_empty());
    assert!(file.outside_edits().kept_stale_paths.is_empty());
    assert!(!root.join("nested/new.html").exists());
    // A disk change after snapshot capture must never acquire import ownership.
    fs::write(root.join("index.html"), b"Later outside edit").unwrap();
    file.mark_imported_site_owned(&imported).unwrap();
    file.save(&project).unwrap();
    assert_eq!(file.outside_edits().changed_paths, ["index.html"]);
    assert_eq!(
        fs::read(root.join("index.html")).unwrap(),
        b"Later outside edit"
    );
}

#[test]
fn reimporting_the_project_site_marks_its_current_bytes_owned() {
    let temp = Temp::new();
    let region = Uuid::new_v4();
    let project = desegni_core::import_website_files(
        "Imported studio",
        &BTreeMap::from([
            (
                "index.html".into(),
                format!(
                    "<!doctype html><html><head><title>Studio</title></head><body><main><h1>Home</h1></main><script type=\"application/x-desegnos-region\" data-desegnos-script=\"{region}\">document.title = 'Region';</script></body></html>"
                )
                .into_bytes(),
            ),
            ("styles.css".into(), b"h1 { color: blue; }".to_vec()),
        ]),
    )
    .unwrap();
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    // The saved page carries the region runtime script and still parses on re-import.
    let saved = fs::read_to_string(root.join("index.html")).unwrap();
    assert!(saved.contains("<script defer src=\"desegnos-regions-"));
    let files = desegni_storage::project_folder::read_website_files(root)
        .unwrap()
        .files;
    project.review_website_import(&files).unwrap();
    file.mark_imported_site_owned(&files).unwrap();
    file.save(&project).unwrap();
    assert_eq!(file.outside_edits(), Default::default());
    assert!(file.site_warnings().is_empty());
    // A later edit that was not imported still counts as an outside change.
    fs::write(root.join("styles.css"), b"h1 { color: red; }").unwrap();
    file.save(&project).unwrap();
    assert_eq!(file.outside_edits().changed_paths, ["styles.css"]);
    assert_eq!(
        fs::read(root.join("styles.css")).unwrap(),
        b"h1 { color: red; }"
    );
}

#[test]
fn bytes_matching_the_next_save_are_not_outside_edits() {
    let temp = Temp::new();
    let project = project();
    let file = managed(&temp, &project);
    let root = file.path().parent().unwrap();
    // Nothing is recorded yet, but the folder already holds exactly what Desegnos would write.
    for (path, bytes) in compile_website(&project).unwrap().files {
        fs::create_dir_all(root.join(&path).parent().unwrap()).unwrap();
        fs::write(root.join(&path), bytes).unwrap();
    }
    file.save(&project).unwrap();
    assert_eq!(file.outside_edits(), Default::default());
    assert!(file.site_warnings().is_empty());
}

#[cfg(unix)]
#[test]
fn unavailable_backup_preserves_outside_bytes_and_can_be_retried() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let project = project();
    let file = managed(&temp, &project);
    file.save(&project).unwrap();
    let root = file.path().parent().unwrap();
    fs::write(root.join("index.html"), b"Outside edits").unwrap();
    let foreign = temp.0.join("foreign");
    fs::create_dir(&foreign).unwrap();
    symlink(&foreign, root.join(".desegnos-outside-edits")).unwrap();
    assert!(file.overwrite_site(&project).is_err());
    assert_eq!(fs::read(root.join("index.html")).unwrap(), b"Outside edits");
    assert_eq!(fs::read_dir(&foreign).unwrap().count(), 0);
    fs::remove_file(root.join(".desegnos-outside-edits")).unwrap();
    file.overwrite_site(&project).unwrap();
    file.save(&project).unwrap();
    assert_eq!(file.outside_edits(), Default::default());
}
