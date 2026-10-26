use desegni_storage::{detect_website_folder, import_detected_website};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use uuid::Uuid;
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("desegnos-detection-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn write(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}
fn astro(root: &Path) {
    write(
        root,
        "package.json",
        r#"{"dependencies":{"astro":"*"},"scripts":{"build":"astro build"},"packageManager":"pnpm@11"}"#,
    );
    write(root, "src/pages/index.astro", "<h1>Source</h1>");
}
fn touch(path: &Path, time: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(time))
        .unwrap();
}
#[test]
fn plain_detection_uses_the_real_page_detector() {
    let temp = Temp::new();
    write(&temp.0, "index.html", "<h1>Home</h1>");
    write(&temp.0, "about.html", "<h1>About</h1>");
    write(&temp.0, "docs/notes.html", "<h1>Notes</h1>");
    write(&temp.0, "404.html", "<h1>Missing</h1>");
    let site = detect_website_folder(&temp.0).unwrap().unwrap();
    assert!(site.framework.is_none());
    assert!(site.fresh);
    assert_eq!(site.pages.len(), 2);
}
#[test]
fn astro_fresh_stale_and_missing_output() {
    let temp = Temp::new();
    astro(&temp.0);
    let missing = detect_website_folder(&temp.0).unwrap().unwrap();
    assert_eq!(missing.framework.as_deref(), Some("Astro"));
    assert!(!missing.fresh);
    assert!(missing.output.is_none());
    assert_eq!(missing.install_command.as_deref(), Some("pnpm install"));
    assert_eq!(missing.build_command.unwrap(), ["pnpm", "run", "build"]);
    write(&temp.0, "dist/index.html", "<h1>Built</h1>");
    let newer = SystemTime::now() + Duration::from_secs(2);
    touch(&temp.0.join("dist/index.html"), newer);
    assert!(detect_website_folder(&temp.0).unwrap().unwrap().fresh);
    touch(
        &temp.0.join("src/pages/index.astro"),
        newer + Duration::from_secs(2),
    );
    assert!(!detect_website_folder(&temp.0).unwrap().unwrap().fresh);
}
#[test]
fn framework_project_is_outside_source_and_output_and_keeps_all_source_bytes() {
    let temp = Temp::new();
    let source = temp.0.join("Source");
    fs::create_dir(&source).unwrap();
    astro(&source);
    write(&source, "dist/index.html", "<!doctype html><h1>Built</h1>");
    touch(
        &source.join("dist/index.html"),
        SystemTime::now() + Duration::from_secs(2),
    );
    let before = fs::read(source.join("dist/index.html")).unwrap();
    let file = import_detected_website(&source, &temp.0).unwrap();
    assert!(!file.path().starts_with(&source));
    assert!(!file.path().starts_with(source.join("dist")));
    let project = file.load().unwrap();
    assert_eq!(project.website.unwrap().framework.as_deref(), Some("Astro"));
    assert_eq!(fs::read(source.join("dist/index.html")).unwrap(), before);
    assert!(!source.join(".desegnos-original").exists());
    assert_eq!(fs::read_dir(&source).unwrap().count(), 3);
}
#[test]
fn base_href_root_assets_and_cdn_are_preserved_and_resolved_only_for_canvas() {
    let files = BTreeMap::from([
        ("index.html".into(), br#"<!doctype html><html><head><base href="/assets/"><link rel="stylesheet" href="site.css"><link rel="stylesheet" href="https://cdn.example.com/extra.css"></head><body><h1>Hello</h1><img src="/pixel.svg"><a href="../about.html">About</a></body></html>"#.to_vec()),
        ("about.html".into(), b"<!doctype html><h1>About</h1>".to_vec()),
        ("assets/site.css".into(), b"body{color:red}".to_vec()),
        ("pixel.svg".into(), br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#.to_vec()),
    ]);
    let project = desegni_core::import_website_files("Site", &files).unwrap();
    assert_eq!(
        desegni_core::compile_website(&project).unwrap().files,
        files
    );
    let canvas = project
        .website_canvas_content(project.page(project.home_page_id).unwrap())
        .unwrap()
        .unwrap();
    assert!(canvas.html.contains("body{color:red}"));
    assert!(canvas.html.contains("src=\"/pixel.svg\""));
    assert!(canvas.html.contains("https://cdn.example.com/extra.css"));
    assert!(canvas.html.contains("href=\"/about\""));
}
#[test]
#[ignore = "owner's read-only site; run explicitly"]
fn chelib_read_only_snapshot() {
    let Ok(path) = std::env::var("DESEGNOS_OWNER_SITE") else {
        return;
    };
    let path = Path::new(&path);
    if !path.is_dir() {
        return;
    }
    let site = detect_website_folder(path).unwrap().unwrap();
    println!(
        "chelib framework={:?}, fresh={}, output={:?}, pages={:?}",
        site.framework, site.fresh, site.output, site.pages
    );
    let output = site.output.unwrap();
    let desegni_storage::project_folder::WebsiteSnapshot { files, skipped } =
        desegni_storage::project_folder::read_website_files(&output).unwrap();
    println!("chelib skipped: {skipped:?}");
    let project = desegni_core::import_website_files("chelib", &files).unwrap();
    println!(
        "chelib loaded {} pages: {:?}",
        project.pages.len(),
        project
            .pages
            .iter()
            .map(|p| (&p.name, &p.path))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        desegni_core::compile_website(&project)
            .unwrap()
            .files
            .iter()
            .filter(|(p, _)| !p.ends_with(".html"))
            .count(),
        files.iter().filter(|(p, _)| !p.ends_with(".html")).count()
    );
}

#[test]
fn excess_and_oversized_pages_are_skipped_with_a_bounded_report() {
    let mut files = BTreeMap::new();
    for index in 0..101 {
        files.insert(
            format!("page-{index:03}.html"),
            b"<!doctype html><h1>Page</h1>".to_vec(),
        );
    }
    files.insert(
        "huge.html".into(),
        vec![b' '; desegni_core::MAX_SITE_SOURCE_BYTES + 1],
    );
    let project = desegni_core::import_website_files("Bounded", &files).unwrap();
    assert_eq!(project.pages.len(), 100);
    assert_eq!(project.website.as_ref().unwrap().skipped.len(), 2);
}
#[cfg(unix)]
#[test]
fn framework_destination_cannot_follow_a_symlink_back_into_source() {
    let temp = Temp::new();
    let source = temp.0.join("Source");
    fs::create_dir(&source).unwrap();
    astro(&source);
    write(&source, "dist/index.html", "<h1>Built</h1>");
    touch(
        &source.join("dist/index.html"),
        SystemTime::now() + Duration::from_secs(2),
    );
    let alias = temp.0.join("Alias");
    std::os::unix::fs::symlink(&source, &alias).unwrap();
    assert!(import_detected_website(&source, &alias.join("Projects")).is_err());
    assert!(!source.join("Projects").exists());
}

#[test]
fn similar_routes_and_reserved_names_keep_their_original_files() {
    let files = BTreeMap::from([
        ("index.htm".into(), b"<h1>Home</h1>".to_vec()),
        ("about.html".into(), b"<h1>About</h1>".to_vec()),
        ("about/index.html".into(), b"<h1>Other about</h1>".to_vec()),
        ("assets.html".into(), b"<h1>Assets</h1>".to_vec()),
    ]);
    let project = desegni_core::import_website_files("Site", &files).unwrap();
    assert_eq!(project.pages.len(), 4);
    project.validate().unwrap();
    assert_eq!(
        desegni_core::compile_website(&project).unwrap().files,
        files
    );
}

#[test]
fn normal_heading_and_source_edits_preserve_the_imported_document_head_and_scripts() {
    use desegni_core::{ElementEdit, ElementTarget, PageSourceEdit, TextEdit};
    let html = "<!doctype html><html><head><title>Original</title><meta name='viewport' content='width=device-width'><link rel='stylesheet' href='site.css'><script src='main.js' defer></script></head><body><h1>Heading</h1></body></html>";
    let files = BTreeMap::from([
        ("index.html".into(), html.as_bytes().to_vec()),
        ("site.css".into(), b"h1{color:red}".to_vec()),
        (
            "main.js".into(),
            b"document.body.dataset.ready='yes';".to_vec(),
        ),
    ]);
    let project = desegni_core::import_website_files("Site", &files).unwrap();
    let source = &project
        .page(project.home_page_id)
        .unwrap()
        .content
        .as_ref()
        .unwrap()
        .html;
    let id = source
        .split("<h1 data-desegnos-id=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let target = ElementTarget {
        id,
        selector: "body > h1:nth-of-type(1)".into(),
        tag: "h1".into(),
        label: "Heading".into(),
        source_sha256: String::new(),
    };
    let (edited, _) = project
        .edit_element(
            project.revision,
            project.home_page_id,
            &target,
            &ElementEdit::Update {
                texts: vec![TextEdit {
                    index: 0,
                    text: "Edited".into(),
                }],
                attributes: BTreeMap::new(),
                style: None,
            },
        )
        .unwrap();
    let html = &edited
        .page(edited.home_page_id)
        .unwrap()
        .content
        .as_ref()
        .unwrap()
        .html;
    let head_end = html.find("</head>").unwrap();
    assert!(html.find("<title").unwrap() < head_end);
    assert!(html.find("<script").unwrap() < head_end);
    assert!(html.contains("src=\"main.js\""));
    let typed = edited
        .edit_page_source(
            edited.revision,
            edited.home_page_id,
            &PageSourceEdit {
                html: Some(html.replace("Edited", "Typed")),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        desegni_core::compile_website(&typed).unwrap().files["index.html"]
            .windows(5)
            .any(|w| w == b"Typed")
    );
    assert!(
        desegni_core::validate_page_source(
            project
                .page(project.home_page_id)
                .unwrap()
                .content
                .as_ref()
                .unwrap()
        )
        .is_err()
    );
}
