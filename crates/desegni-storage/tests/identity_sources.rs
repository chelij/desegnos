use base64::{Engine, engine::general_purpose::STANDARD};
use desegni_core::{Appearance, Project, SiteContent, Viewport, digest};
use desegni_storage::{ProjectFile, identity_source::*};
use std::{fs, io::Write, path::PathBuf};
use zip::{ZipWriter, write::SimpleFileOptions};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("desegnos-identity-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn zip(temp: &Temp, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = temp.0.join(name);
    let mut archive = ZipWriter::new(fs::File::create(&path).unwrap());
    for (name, bytes) in entries {
        archive
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap();
    path
}

#[test]
fn brand_import_reads_new_and_legacy_project_names_without_changing_either() {
    let temp = Temp::new();
    let legacy = temp.0.join("brand.desegni");
    let current = temp.0.join("brand.desegnos");
    let project = Project::new(
        "Brand source".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap();
    let file = ProjectFile::acquire(&legacy).unwrap();
    file.save(&project).unwrap();
    fs::copy(&legacy, &current).unwrap();
    let original = fs::read(&legacy).unwrap();
    let old_source = collect_identity_source(&legacy).unwrap();
    let new_source = collect_identity_source(&current).unwrap();
    assert_eq!(old_source.files, new_source.files);
    assert_eq!(old_source.sha256, new_source.sha256);
    assert_eq!(fs::read(&legacy).unwrap(), original);
    assert_eq!(fs::read(&current).unwrap(), original);
    assert!(!temp.0.join("brand.desegnos.lock").exists());
}

#[test]
fn folder_and_zip_collect_the_same_immutable_sources_with_stable_ids() {
    let folder = Temp::new();
    let original = [
        ("index.html", b"<h1>Acme</h1>".as_slice()),
        ("styles.css", b":root { --brand: #123456 }".as_slice()),
        ("brand/tokens.json", b"{\"color\":\"#123456\"}".as_slice()),
        (
            "brand/IDENTITY.md",
            b"Acme uses an assured, friendly voice.".as_slice(),
        ),
    ];
    for (name, bytes) in &original {
        folder.write(name, bytes);
    }
    let first = collect_identity_source(&folder.0).unwrap();
    assert_eq!(first, collect_identity_source(&folder.0).unwrap());
    assert_eq!(first.files.len(), 4);
    let archive = Temp::new();
    let zipped = collect_identity_source(&zip(&archive, "website.zip", &original)).unwrap();
    assert_eq!(first.files, zipped.files);
    assert_eq!(first.sha256, zipped.sha256);
    assert_eq!(first.id, zipped.id);
    for (name, bytes) in &original {
        assert_eq!(fs::read(folder.0.join(name)).unwrap(), *bytes);
    }
    folder.write("styles.css", ":root { --brand: #654321 }");
    let changed = collect_identity_source(&folder.0).unwrap();
    assert_ne!(first.sha256, changed.sha256);
    assert_ne!(first.id, changed.id);
}

#[test]
fn astro_site_reads_templates_and_ranks_brand_sources_before_incidental_code() {
    let folder = Temp::new();
    let entries = [
        ("src/pages/work.astro", "<h1>Our work</h1>"),
        (
            "src/pages/index.astro",
            "<h1>Welcome</h1><script src=\"/play/site-menu.js\"></script>",
        ),
        ("src/pages/about.astro", "<h1>About us</h1>"),
        ("src/layouts/Base.astro", "<html><slot /></html>"),
        ("src/styles/global.css", ":root{--brand:#16324f}"),
        (
            "tailwind.config.ts",
            "export default {theme:{colors:{brand:'#16324f'}}}",
        ),
        ("src/components/Header.vue", "<header>Studio</header>"),
        ("src/components/Footer.svelte", "<footer>Studio</footer>"),
        ("src/components/Work.jsx", "<div>Work</div>"),
        ("src/pages/contact.tsx", "<main>Contact</main>"),
        ("README.md", "# Studio"),
        ("DESIGN.md", "Our brand uses blue."),
        ("notes.mdx", "# Brand notes"),
        ("theme.toml", "color = '#16324f'"),
        ("theme.yaml", "color: '#16324f'"),
        ("theme.yml", "color: '#16324f'"),
        ("page.njk", "<h1>{{ title }}</h1>"),
        ("page.hbs", "<h1>{{ title }}</h1>"),
        ("page.liquid", "<h1>{{ title }}</h1>"),
        ("page.php", "<h1>Studio</h1>"),
        ("accent.scss", "$accent: blue;"),
        ("accent.sass", "$accent: blue"),
        ("accent.less", "@accent: blue;"),
        (
            "public/logo.svg",
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'><path d='M0 0H10V10Z'/></svg>",
        ),
        (
            "public/play/site-menu.js",
            "document.body.classList.toggle('menu')",
        ),
    ];
    let skipped = [
        "LICENSE",
        "LICENCE.txt",
        "public/fonts/DM-Sans-OFL.txt",
        "public/fonts/LICENSE.md",
        "node_modules/a/index.js",
        ".git/config",
        "dist/index.html",
        "build/site.css",
        "out/index.html",
        ".astro/cache.json",
        ".next/index.html",
        "package-lock.json",
        "pnpm-lock.yaml",
        "public/vendor.js",
        "public/js/menu.min.js",
        "public/play/solopong/game.js",
        "public/play/solopong/index.html",
        "public/play/solopong/styles.css",
        "public/apps/demo/app.tsx",
    ];
    for (name, text) in &entries {
        folder.write(name, text);
    }
    for name in skipped {
        folder.write(name, "unrelated content");
    }
    let source = collect_identity_source(&folder.0).unwrap();
    let names: Vec<_> = source.files.iter().map(|file| file.name.as_str()).collect();
    assert_eq!(&names[..2], ["src/styles/global.css", "tailwind.config.ts"]);
    assert!(
        names
            .iter()
            .position(|name| *name == "src/layouts/Base.astro")
            < names
                .iter()
                .position(|name| *name == "src/pages/about.astro")
    );
    assert!(
        names
            .iter()
            .position(|name| *name == "src/pages/about.astro")
            < names
                .iter()
                .position(|name| *name == "src/pages/work.astro")
    );
    for (name, text) in &entries {
        assert_eq!(
            source
                .files
                .iter()
                .find(|file| file.name == *name)
                .unwrap()
                .text
                .as_deref(),
            Some(*text)
        );
    }
    for name in skipped {
        assert!(!names.contains(&name), "included {name}");
    }
    assert_eq!(source, collect_identity_source(&folder.0).unwrap());
    let archive = Temp::new();
    let all: Vec<_> = entries
        .iter()
        .map(|(name, text)| (*name, text.as_bytes()))
        .chain(
            skipped
                .iter()
                .map(|name| (*name, b"unrelated content".as_slice())),
        )
        .collect();
    let zipped = collect_identity_source(&zip(&archive, "astro.zip", &all)).unwrap();
    assert_eq!(source.files, zipped.files);
    assert_eq!(source.sha256, zipped.sha256);
}

#[test]
fn built_site_is_readable_when_no_original_sources_are_available() {
    let folder = Temp::new();
    folder.write("dist/index.html", "<h1>Built site</h1>");
    let source = collect_identity_source(&folder.0).unwrap();
    assert_eq!(source.files[0].name, "dist/index.html");
    let archive = Temp::new();
    let zipped = collect_identity_source(&zip(
        &archive,
        "built.zip",
        &[("dist/index.html", b"<h1>Built site</h1>")],
    ))
    .unwrap();
    assert_eq!(source.files, zipped.files);
}

#[test]
fn project_archive_is_verified_and_read_without_writer_lock_or_changes() {
    let temp = Temp::new();
    let path = temp.0.join("website.desegni");
    let handle = ProjectFile::acquire(&path).unwrap();
    let mut project = Project::new(
        "Acme".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap();
    project.pages[0].content = Some(SiteContent {
        html: "<h1>Acme</h1>".into(),
        css: "h1{color:#123456}".into(),
        javascript: "console.log('website source is data');".into(),
    });
    handle.save(&project).unwrap();
    let before = fs::read(&path).unwrap();
    // Collection succeeds while the actual editor's writer lock is held.
    let source = collect_identity_source(&path).unwrap();
    assert!(
        source
            .files
            .iter()
            .any(|file| file.name == "PROJECT.md" && file.text.as_ref().unwrap().contains("Acme"))
    );
    assert!(
        source
            .files
            .iter()
            .any(|file| file.name.ends_with("index.html")
                && file.text.as_deref() == Some("<h1>Acme</h1>"))
    );
    assert!(
        source
            .files
            .iter()
            .any(|file| file.name.ends_with("styles.css"))
    );
    assert!(
        source
            .files
            .iter()
            .any(|file| file.name.ends_with("script.js"))
    );
    assert_eq!(before, fs::read(&path).unwrap());
    drop(handle);
    fs::remove_file(temp.0.join("website.desegni.lock")).unwrap();
    collect_identity_source(&path).unwrap();
    assert!(!temp.0.join("website.desegni.lock").exists());
    assert!(!temp.0.join("website.desegni.previous").exists());
}

#[test]
fn secrets_dependencies_hidden_files_and_configuration_are_excluded() {
    let folder = Temp::new();
    folder.write(
        "brand/tokens.json",
        "{\"palette\": [\"#112233\", \"#ffffff\"]}",
    );
    folder.write("index.html", "<h1>Welcome</h1>");
    folder.write(
        ".env",
        "OPENAI_API_KEY=sk-example-abcdefghijklmnopqrstuvwxyz",
    );
    folder.write(".claude/settings.json", "{\"private\":true}");
    folder.write("auth.json", "{\"user\":\"someone\"}");
    folder.write("config/server.json", "{\"internal\":true}");
    folder.write("node_modules/huge.js", "dependency content");
    folder.write("target/site.html", "build output");
    folder.write("package-lock.json", "lock data");
    folder.write("credentials.txt", "private account metadata");
    folder.write(
        "src/accidental.js",
        "const apiKey = 'sk-example-abcdefghijklmnopqrstuvwxyz';",
    );
    folder.write(
        "src/private.md",
        "-----BEGIN OPENSSH PRIVATE KEY-----\nsensitive\n-----END OPENSSH PRIVATE KEY-----",
    );
    folder.write(
        "src/access.ts",
        "const access_token = 'a-long-secret-without-a-provider-prefix';",
    );
    folder.write(
        "src/client.json",
        "{\"client\":{\"token\":\"opaque-nested-credential\"}}",
    );
    folder.write(
        "src/encrypted.md",
        "-----BEGIN ENCRYPTED PRIVATE KEY-----\nopaque\n-----END ENCRYPTED PRIVATE KEY-----",
    );
    let source = collect_identity_source(&folder.0).unwrap();
    assert_eq!(
        source
            .files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        ["brand/tokens.json", "index.html"]
    );
    let serialized = serde_json::to_string(&source).unwrap();
    assert!(!serialized.contains("sk-example-abcdefghijklmnopqrstuvwxyz"));
    assert!(!serialized.contains("a-long-secret-without-a-provider-prefix"));
    assert!(!serialized.contains("BEGIN OPENSSH"));
    assert!(!serialized.contains("opaque-nested-credential"));
    assert!(!serialized.contains("BEGIN ENCRYPTED"));
    assert!(
        source
            .skipped
            .iter()
            .any(|reason| reason.contains("accidental.js") && reason.contains("credential"))
    );
    assert!(
        source
            .skipped
            .iter()
            .any(|reason| reason.contains("node_modules"))
    );
    assert!(
        source
            .skipped
            .iter()
            .any(|reason| reason.contains(".claude"))
    );
}

#[test]
fn secret_placeholders_read_from_environment_are_allowed_but_never_evaluated() {
    let folder = Temp::new();
    let text = "const apiKey = process.env.OPENAI_API_KEY;\nconst access_token = import.meta.env.VITE_TOKEN;\n";
    folder.write("src/main.ts", text);
    let source = collect_identity_source(&folder.0).unwrap();
    assert_eq!(source.files[0].text.as_deref(), Some(text));
}

#[test]
fn oversized_and_non_utf8_sources_are_reported_without_truncation() {
    let folder = Temp::new();
    folder.write("index.html", "<h1>Welcome</h1>");
    folder.write("large.css", vec![b'a'; 256 * 1024 + 1]);
    folder.write("binary.js", [0xff, 0xfe, 0xfd]);
    folder.write("nul.ts", b"hello\0world");
    let source = collect_identity_source(&folder.0).unwrap();
    assert_eq!(source.files.len(), 1);
    for filename in ["large.css", "binary.js", "nul.ts"] {
        assert!(
            source
                .skipped
                .iter()
                .any(|reason| reason.contains(filename))
        );
    }
    let archive = Temp::new();
    let oversized = vec![b'a'; 256 * 1024 + 1];
    let zipped = collect_identity_source(&zip(
        &archive,
        "site.zip",
        &[
            ("large.css", &oversized),
            ("index.html", b"<h1>Welcome</h1>"),
        ],
    ))
    .unwrap();
    assert_eq!(source.files, zipped.files);
    assert!(
        zipped
            .skipped
            .iter()
            .any(|reason| reason.contains("large.css"))
    );
}

#[test]
fn text_and_file_budgets_are_bounded_and_report_every_omission() {
    let folder = Temp::new();
    for index in 0..140 {
        folder.write(
            &format!("page-{index:03}.html"),
            format!("<h1>Page {index}</h1>"),
        );
    }
    let source = collect_identity_source(&folder.0).unwrap();
    assert_eq!(source.files.len(), 128);
    assert_eq!(source.skipped.len(), 12);
    assert!(
        source
            .skipped
            .iter()
            .all(|reason| reason.contains("128 source file"))
    );
    let folder = Temp::new();
    for index in 0..24 {
        folder.write(&format!("page-{index:03}.html"), "x".repeat(200 * 1024));
    }
    folder.write(
        "z-brand/IDENTITY.md",
        "This important identity must survive the collection budget.",
    );
    let source = collect_identity_source(&folder.0).unwrap();
    assert!(
        source
            .files
            .iter()
            .any(|file| file.name.ends_with("IDENTITY.md"))
    );
    assert!(
        source
            .files
            .iter()
            .filter_map(|file| file.text.as_ref())
            .map(String::len)
            .sum::<usize>()
            <= 2 * 1024 * 1024
    );
    assert!(
        source
            .skipped
            .iter()
            .any(|reason| reason.contains("snapshot budget"))
    );
    assert!(source.skipped.iter().any(|reason| reason.contains("2 MiB")));
}

#[cfg(unix)]
#[test]
fn folder_and_file_symlinks_are_never_followed() {
    use std::os::unix::fs::symlink;
    let outside = Temp::new();
    outside.write("private.html", "outside source");
    let folder = Temp::new();
    folder.write("index.html", "<h1>Safe</h1>");
    symlink(outside.0.join("private.html"), folder.0.join("linked.html")).unwrap();
    symlink(&outside.0, folder.0.join("linked-directory")).unwrap();
    let source = collect_identity_source(&folder.0).unwrap();
    assert_eq!(source.files.len(), 1);
    assert_eq!(source.skipped.len(), 2);
    assert!(
        source
            .skipped
            .iter()
            .all(|reason| reason.contains("symbolic link"))
    );
    assert!(collect_identity_source(&folder.0.join("linked-directory")).is_err());
    assert!(collect_identity_source(&folder.0.join("linked.html")).is_err());
}

#[test]
fn zip_rejects_traversal_absolute_windows_and_duplicate_names() {
    for name in [
        "../outside.html",
        "/absolute.html",
        "C:/site/index.html",
        "a\\b.html",
        "site/../index.html",
        "site/./index.html",
        "site//index.html",
    ] {
        let temp = Temp::new();
        let path = zip(
            &temp,
            "bad.zip",
            &[("index.html", b"<h1>Safe</h1>"), (name, b"unsafe")],
        );
        assert!(collect_identity_source(&path).is_err(), "accepted {name}");
    }
    // zip 6 itself refuses duplicate names when creating an archive.
    let temp = Temp::new();
    let mut writer = ZipWriter::new(fs::File::create(temp.0.join("duplicate.zip")).unwrap());
    writer
        .start_file("same.html", SimpleFileOptions::default())
        .unwrap();
    assert!(
        writer
            .start_file("same.html", SimpleFileOptions::default())
            .is_err()
    );
}

#[test]
fn zip_rejects_symlinks_excessive_entries_expansion_and_bad_archives() {
    let temp = Temp::new();
    let path = temp.0.join("link.zip");
    let mut writer = ZipWriter::new(fs::File::create(&path).unwrap());
    writer
        .add_symlink(
            "linked.html",
            "../../outside.html",
            SimpleFileOptions::default(),
        )
        .unwrap();
    writer.finish().unwrap();
    assert!(
        collect_identity_source(&path)
            .unwrap_err()
            .to_string()
            .contains("unsafe")
    );
    let path = temp.0.join("entries.zip");
    let mut writer = ZipWriter::new(fs::File::create(&path).unwrap());
    for index in 0..4097 {
        writer
            .add_directory(format!("d{index}/"), SimpleFileOptions::default())
            .unwrap();
    }
    writer.finish().unwrap();
    assert!(
        collect_identity_source(&path)
            .unwrap_err()
            .to_string()
            .contains("4,096")
    );
    let expansion = vec![0_u8; 2 * 1024 * 1024];
    let path = zip(&temp, "expansion.zip", &[("bomb.html", &expansion)]);
    assert!(
        collect_identity_source(&path)
            .unwrap_err()
            .to_string()
            .contains("expansion ratio")
    );
    temp.write("invalid.zip", "not a zip");
    assert!(collect_identity_source(&temp.0.join("invalid.zip")).is_err());
}

#[test]
fn svg_is_source_text_and_only_passive_logo_svg_is_reusable() {
    let temp = Temp::new();
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><path fill=\"#123456\" d=\"M0 0H10V10Z\"/></svg>";
    temp.write("logo.svg", svg);
    temp.write(
        "logo-interactive.svg",
        "<svg onpointerdown=\"alert(1)\"><path/></svg>",
    );
    temp.write(
        "logo-external.svg",
        "<svg><style>.a{fill:url (https://example.com/x)}</style></svg>",
    );
    let source = collect_identity_source(&temp.0).unwrap();
    assert_eq!(source.files.len(), 3);
    let logo = source
        .files
        .iter()
        .find(|file| file.name == "logo.svg")
        .unwrap();
    assert_eq!(logo.text.as_deref(), Some(svg));
    let asset = logo.asset.as_ref().unwrap();
    assert_eq!(asset.mime, "image/svg+xml");
    assert_eq!(asset.sha256, digest(svg.as_bytes()));
    assert_eq!(
        STANDARD
            .decode(asset.data_uri.split_once(',').unwrap().1)
            .unwrap(),
        svg.as_bytes()
    );
    assert!(
        source
            .files
            .iter()
            .filter(|file| file.name != "logo.svg")
            .all(|file| file.asset.is_none())
    );
}

#[test]
fn real_desegnos_brand_folder_keeps_tokens_and_deduplicates_logo_sizes_read_only() {
    let Ok(root) = std::env::var("DESEGNOS_BRAND_FOLDER") else {
        return;
    };
    let root = std::path::Path::new(&root);
    if !root.is_dir() {
        return;
    }
    fn fingerprint(
        directory: &std::path::Path,
        result: &mut std::collections::BTreeMap<PathBuf, String>,
    ) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                fingerprint(&path, result);
            } else if metadata.is_file() {
                result.insert(path.clone(), digest(&fs::read(path).unwrap()));
            }
        }
    }
    let mut before = std::collections::BTreeMap::new();
    fingerprint(root, &mut before);
    let source = collect_identity_source(root).unwrap();
    for name in [
        "brand/IDENTITY.md",
        "brand/tokens.json",
        "brand/tokens.css",
        "brand/index.html",
    ] {
        assert!(
            source
                .files
                .iter()
                .any(|file| file.name == name && file.text.is_some()),
            "missing {name}"
        );
    }
    let assets = source
        .files
        .iter()
        .filter_map(|file| file.asset.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(assets.len(), 4);
    assert_eq!(
        assets
            .iter()
            .filter(|asset| asset.mime == "image/svg+xml")
            .count(),
        2
    );
    assert_eq!(
        assets
            .iter()
            .filter(|asset| asset.mime == "image/png")
            .count(),
        2
    );
    assert!(
        assets
            .iter()
            .filter(|asset| asset.mime == "image/png")
            .all(|asset| asset.name.ends_with("-128.png"))
    );
    assert!(
        assets
            .iter()
            .map(|asset| asset.data_uri.len())
            .sum::<usize>()
            <= 512 * 1024
    );
    assert!(
        source
            .skipped
            .iter()
            .any(|reason| reason.contains(".claude"))
    );
    assert!(
        source
            .skipped
            .iter()
            .any(|reason| reason.contains("__pycache__"))
    );
    let mut after = std::collections::BTreeMap::new();
    fingerprint(root, &mut after);
    assert_eq!(before, after);
    assert_eq!(source, collect_identity_source(root).unwrap());
}

#[test]
fn empty_or_only_excluded_sources_and_unsupported_selection_have_clear_errors() {
    let temp = Temp::new();
    assert!(
        collect_identity_source(&temp.0)
            .unwrap_err()
            .to_string()
            .contains("No usable identity")
    );
    temp.write(".env", "API_KEY=private");
    assert!(collect_identity_source(&temp.0).is_err());
    temp.write("one.html", "<h1>Site</h1>");
    assert!(
        collect_identity_source(&temp.0.join("one.html"))
            .unwrap_err()
            .to_string()
            .contains("website folder")
    );
}

#[test]
fn static_logo_styles_and_gradients_use_the_shared_portable_asset_validation() {
    let temp = Temp::new();
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg"><style>.logo{fill:url(&quot;#brand&quot;)}</style><defs><linearGradient id="brand"><stop stop-color="#16324F"/></linearGradient></defs><rect class="logo" width="20" height="20"/></svg>"##;
    temp.write("logo.svg", svg);
    let source = collect_identity_source(&temp.0).unwrap();
    let asset = source
        .files
        .iter()
        .find_map(|file| file.asset.as_ref())
        .unwrap();
    desegni_core::IdentityAsset {
        id: asset.id,
        name: asset.name.clone(),
        mime: asset.mime.clone(),
        data_uri: asset.data_uri.clone(),
        sha256: asset.sha256.clone(),
    }
    .validate()
    .unwrap();
    assert_eq!(
        STANDARD
            .decode(asset.data_uri.split_once(',').unwrap().1)
            .unwrap(),
        svg
    );
    assert_eq!(fs::read(temp.0.join("logo.svg")).unwrap(), svg);
}

#[test]
fn folder_pages_attribute_different_photos_and_shared_references() {
    let temp = Temp::new();
    temp.write(
        "src/pages/index.astro",
        "<h1>Welcome</h1><img src='/photos/home.png'>",
    );
    temp.write(
        "src/pages/projects.astro",
        "<h1>Projects</h1><img src=/photos/robot.png><img src='/photos/home.png'>",
    );
    for (name, color) in [
        ("home", [240, 30, 20]),
        ("robot", [20, 100, 230]),
        ("loose", [40, 140, 20]),
    ] {
        let image = image::RgbImage::from_pixel(320, 240, image::Rgb(color));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        temp.write(&format!("public/photos/{name}.png"), bytes.into_inner());
    }
    let source = collect_identity_source(&temp.0).unwrap();
    let photo = |name: &str| {
        &source
            .photos
            .iter()
            .find(|photo| photo.photo.name.ends_with(name))
            .unwrap()
            .photo
    };
    assert_eq!(
        photo("robot.png")
            .source_pages
            .iter()
            .map(|page| page.path.as_str())
            .collect::<Vec<_>>(),
        ["/projects"]
    );
    assert_eq!(photo("robot.png").source_pages[0].title, "Projects");
    assert_eq!(photo("home.png").source_pages.len(), 2);
    assert!(photo("loose.png").source_pages.is_empty());
    assert_eq!(source.pages.len(), 2);
    assert_eq!(
        source
            .pages
            .iter()
            .find(|page| page.path == "/projects")
            .unwrap()
            .photo_count,
        2
    );
    assert_eq!(source, collect_identity_source(&temp.0).unwrap());
}

#[test]
fn crawled_files_attribute_photos_to_their_original_url_and_page_title() {
    let image = image::RgbImage::from_pixel(320, 240, image::Rgb([20, 90, 160]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let source = identity_source_from_files("example.com", vec![
        ("pages/02-about.html".into(), b"<!-- Source URL: https://example.com/about -->\n<title>About our studio</title><img src='/photos/team.png'>".to_vec()),
        ("photos/team.png".into(), bytes.into_inner()),
    ], vec![]).unwrap();
    let page = &source.photos[0].photo.source_pages[0];
    assert_eq!(page.path, "/about");
    assert_eq!(page.title, "About our studio");
    assert_eq!(page.url.as_deref(), Some("https://example.com/about"));
    assert_eq!(source.pages[0].kind, "About");
}

#[test]
fn crawl_metadata_attributes_cdn_and_extensionless_images_after_safe_renaming() {
    let image = image::RgbImage::from_pixel(320, 240, image::Rgb([20, 90, 160]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let reference = desegni_core::SourcePageRef {
        path: "/about".into(),
        title: "About our studio".into(),
        url: Some("https://example.com/about".into()),
    };
    let source = identity_source_from_files_attributed("example.com", vec![
        ("pages/02-about.html".into(), b"<!-- Source URL: https://example.com/about -->\n<title>About our studio</title><img src='https://cdn.example.net/image?id=team'>".to_vec()),
        ("photos/verified-url-hash.png".into(), bytes.into_inner()),
    ], vec![], std::collections::BTreeMap::from([("photos/verified-url-hash.png".into(), vec![reference.clone()])])).unwrap();
    assert_eq!(source.photos[0].photo.source_pages, [reference]);
    assert_eq!(source.pages.len(), 1);
    assert_eq!(source.pages[0].photo_count, 1);
}

#[test]
fn project_snapshots_use_real_page_metadata_instead_of_snapshot_filenames() {
    let temp = Temp::new();
    let project = Project::new(
        "Studio".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap();
    let project = project
        .apply(
            0,
            desegni_core::Command::AddPage {
                name: "Our projects".into(),
                path: "/projects".into(),
            },
        )
        .unwrap();
    let path = temp.0.join("studio.desegnos");
    ProjectFile::acquire(&path).unwrap().save(&project).unwrap();
    let source = collect_identity_source(&path).unwrap();
    assert_eq!(
        source
            .pages
            .iter()
            .map(|page| page.path.as_str())
            .collect::<Vec<_>>(),
        ["/", "/projects"]
    );
    assert_eq!(source.pages[1].title, "Our projects");
    assert!(source.files.iter().any(|file| {
        file.source_page
            .as_ref()
            .is_some_and(|page| page.path == "/projects")
    }));
}

#[test]
fn astro_routes_exclude_notes_exports_errors_drafts_and_build_copies() {
    let temp = Temp::new();
    let fixture = [
        ("astro.config.mjs", "export default {}"),
        (
            "src/pages/index.astro",
            "<h1>AI guides<br>and <span>tools</span><br>from Cheli.</h1>",
        ),
        (
            "src/pages/projects/index.astro",
            "<title>Projects | Cheli</title>",
        ),
        ("src/pages/projects/papan.astro", "<h1>Papan</h1>"),
        (
            "src/pages/posts/[...slug].astro",
            "---\nconst posts = await getCollection('posts');\n---\n<h1>{post.title}</h1>",
        ),
        (
            "src/content/posts/local-ai.md",
            "---\ntitle: Local AI\ndraft: false\n---\n# Ignored heading",
        ),
        (
            "src/content/posts/hidden.mdx",
            "---\ntitle: Hidden\ndraft: true\n---",
        ),
        (
            "src/content/unrouted/notes.md",
            "---\ntitle: Not a page\n---",
        ),
        ("src/pages/404.astro", "<h1>Page not found.</h1>"),
        ("src/pages/500.astro", "<h1>Error</h1>"),
        (
            "src/pages/draft.astro",
            "---\ndraft: true\n---\n<h1>Draft</h1>",
        ),
        ("anti-slop/audit-001.md", "# Copy audit 001"),
        ("CLAUDE.md", "# Instructions"),
        ("README.md", "# Readme"),
        ("chelib-redesign/index.html", "<title>Old site</title>"),
        ("chelib-redesign/package.json", "{}"),
        ("dist/index.html", "<title>Duplicate</title>"),
        (
            "dist/projects/index.html",
            "<title>Duplicate projects</title>",
        ),
        ("public/copy.html", "<title>Copied page</title>"),
        (".hidden/index.html", "<title>Hidden</title>"),
    ];
    for (name, text) in fixture {
        temp.write(name, text);
    }
    let source = collect_identity_source(&temp.0).unwrap();
    let pages = |source: &IdentitySource| {
        source
            .pages
            .iter()
            .map(|p| (p.path.clone(), p.kind.clone(), p.title.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        pages(&source),
        [
            ("/", "Home", "AI guides and tools from Cheli."),
            ("/posts/local-ai", "Blog", "Local AI"),
            ("/projects", "Projects", "Projects"),
            ("/projects/papan", "Projects", "Papan"),
        ]
        .map(|(a, b, c)| (a.into(), b.into(), c.into()))
    );
    assert!(
        source
            .files
            .iter()
            .any(|f| f.name == "anti-slop/audit-001.md" && f.source_page.is_none())
    );
    let archive = Temp::new();
    let entries = fixture
        .iter()
        .map(|(name, text)| (*name, text.as_bytes()))
        .collect::<Vec<_>>();
    assert_eq!(
        pages(&collect_identity_source(&zip(&archive, "site.zip", &entries)).unwrap()),
        pages(&source)
    );
}

#[test]
fn plain_html_uses_sources_and_falls_back_to_build_html_even_with_notes() {
    let temp = Temp::new();
    for (name, text) in [
        ("index.html", "<title>Welcome — Studio</title>"),
        ("about/index.html", "<h1>About <span>us</span></h1>"),
        ("notes/notes.html", "<h1>Notes</h1>"),
        ("404.html", "<h1>Missing</h1>"),
        ("dist/index.html", "<title>Built copy</title>"),
        ("public/about.html", "<h1>Copy</h1>"),
        ("old/index.html", "<h1>Old</h1>"),
        ("old/old.desegnos", "archive marker"),
    ] {
        temp.write(name, text);
    }
    let source = collect_identity_source(&temp.0).unwrap();
    assert_eq!(
        source
            .pages
            .iter()
            .map(|p| (p.path.as_str(), p.title.as_str(), p.kind.as_str()))
            .collect::<Vec<_>>(),
        [("/", "Welcome", "Home"), ("/about", "About us", "About")]
    );
    let built = Temp::new();
    built.write("README.md", "# Build instructions");
    built.write("_site/index.html", "<title>Built site</title>");
    built.write("_site/contact.html", "<h1>Contact us</h1>");
    let source = collect_identity_source(&built.0).unwrap();
    assert_eq!(
        source
            .pages
            .iter()
            .map(|p| p.path.as_str())
            .collect::<Vec<_>>(),
        ["/", "/contact"]
    );
    let archive = Temp::new();
    let entries: &[(&str, &[u8])] = &[
        ("README.md", b"# Build instructions"),
        ("_site/index.html", b"<title>Built site</title>"),
        ("_site/contact.html", b"<h1>Contact us</h1>"),
    ];
    assert_eq!(
        collect_identity_source(&zip(&archive, "built.zip", entries))
            .unwrap()
            .pages,
        source.pages
    );
}

#[test]
fn page_titles_space_elements_and_prefer_frontmatter() {
    let page = desegni_core::identity_source_page(
        "src/pages/index.astro",
        "<h1>AI guides<br>and <span>tools</span>from Cheli.</h1>",
    )
    .unwrap();
    assert_eq!(page.title, "AI guides and tools from Cheli.");
    let page = desegni_core::identity_source_page(
        "src/pages/about.md",
        "---\ntitle: 'Our studio | Site'\n---\n<title>Browser title</title><h1>Heading</h1>",
    )
    .unwrap();
    assert_eq!(page.title, "Our studio");
    assert!(desegni_core::identity_source_page("CLAUDE.md", "# Notes").is_none());
    assert!(desegni_core::folder_source_page("photos/loose.jpg").is_none());
}

#[test]
fn framework_roots_and_special_route_files_are_respected() {
    for (config, files, expected) in [
        (
            "next.config.js",
            vec![
                ("app/(site)/page.tsx", "Home"),
                ("app/(site)/about/page.tsx", "About"),
                ("app/layout.tsx", "No"),
                ("pages/api/test.ts", "No"),
                ("pages/projects.tsx", "Projects"),
            ],
            vec!["/", "/about", "/projects"],
        ),
        (
            "nuxt.config.ts",
            vec![
                ("pages/index.vue", "Home"),
                ("pages/contact.vue", "Contact"),
            ],
            vec!["/", "/contact"],
        ),
        (
            "svelte.config.js",
            vec![
                ("src/routes/+page.svelte", "Home"),
                ("src/routes/about/+page.svelte", "About"),
                ("src/routes/+layout.svelte", "No"),
                ("src/routes/+error.svelte", "No"),
            ],
            vec!["/", "/about"],
        ),
        (
            "hugo.toml",
            vec![
                ("content/_index.md", "Home"),
                ("content/about.md", "About"),
                ("content/posts/one.md", "One"),
            ],
            vec!["/", "/about", "/posts/one"],
        ),
        (
            "gatsby-config.js",
            vec![
                ("src/pages/index.jsx", "Home"),
                ("src/pages/about.jsx", "About"),
            ],
            vec!["/", "/about"],
        ),
        (
            "vite.config.ts",
            vec![
                ("src/pages/index.html", "Home"),
                ("src/pages/about.html", "About"),
            ],
            vec!["/", "/about"],
        ),
        (
            ".eleventy.js",
            vec![
                ("index.njk", "Home"),
                ("about.md", "About"),
                ("_includes/layout.njk", "No"),
            ],
            vec!["/", "/about", "/outside"],
        ),
        (
            "eleventy.config.js",
            vec![
                ("src/pages/index.njk", "Home"),
                ("src/pages/about.md", "About"),
            ],
            vec!["/", "/about"],
        ),
    ] {
        let temp = Temp::new();
        temp.write(config, "export default {}");
        temp.write("outside.html", "<h1>Not in routing root</h1>");
        for (name, title) in files {
            temp.write(name, format!("<h1>{title}</h1>"));
        }
        let source = collect_identity_source(&temp.0).unwrap();
        assert_eq!(
            source
                .pages
                .iter()
                .map(|p| p.path.as_str())
                .collect::<Vec<_>>(),
            expected,
            "{config}"
        );
    }
}

#[test]
fn astro_script_object_titles_are_not_frontmatter_or_markup() {
    let text = "---\nconst steps = [{\n    title: 'Spin',\n    html: '<h1>Script text</h1>',\n}];\n---\n<h1>Wheely <span>Lucky</span>.</h1>";
    let page =
        desegni_core::identity_source_page("src/pages/projects/wheely-lucky.astro", text).unwrap();
    assert_eq!(page.title, "Wheely Lucky.");
}
