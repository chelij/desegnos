//! Read a local website folder or ZIP into a bounded set of files that can be served read-only
//! for rendering. Nothing here writes outside the returned list or follows symlinks.
use desegnos_core::{Error, Result};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Seek},
    path::{Path, PathBuf},
};
use zip::ZipArchive;

const MAX_ENTRIES: usize = 4_000;
const MAX_FILES: usize = 1_500;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

/// One renderable website snapshot: relative file names and the pages they serve.
#[derive(Debug, Clone, Default)]
pub struct WebsiteSnapshot {
    pub name: String,
    pub files: Vec<(String, Vec<u8>)>,
    pub pages: Vec<WebsitePage>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsitePage {
    /// The URL path the page is served at, e.g. `/` or `/about`.
    pub route: String,
    pub title: String,
}

fn io(error: impl std::fmt::Display) -> Error {
    Error(error.to_string())
}

fn extension(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}

fn web_file(name: &str) -> bool {
    matches!(
        extension(name).as_str(),
        "html" | "htm" | "css" | "js" | "mjs" | "cjs" | "json" | "xml" | "svg" | "png" | "jpg"
            | "jpeg" | "webp" | "gif" | "avif" | "ico" | "bmp" | "woff" | "woff2" | "ttf" | "otf"
            | "eot" | "mp4" | "webm" | "ogg" | "mp3" | "wav" | "pdf" | "txt"
    )
}

fn excluded(name: &str) -> bool {
    name.split('/').any(|part| {
        part.starts_with('.')
            || matches!(
                part,
                "node_modules" | "__MACOSX" | "$RECYCLE.BIN" | "System Volume Information"
            )
    })
}

fn safe_name(name: &str) -> Option<String> {
    let name = name.replace('\\', "/");
    let name = name.trim_start_matches('/');
    if name.is_empty()
        || name.contains("//")
        || name.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with(['.', ' '])
                || part.chars().any(|character| character.is_control() || "<>\"|*:?".contains(character))
        })
    {
        return None;
    }
    Some(name.to_owned())
}

fn page_route(name: &str) -> Option<String> {
    if !matches!(extension(name).as_str(), "html" | "htm") {
        return None;
    }
    let name = name.trim_start_matches('/');
    if name == "index.html" || name == "index.htm" {
        return Some("/".into());
    }
    if let Some(directory) = name.strip_suffix("/index.html").or_else(|| name.strip_suffix("/index.htm")) {
        return Some(format!("/{directory}/"));
    }
    let without = name
        .strip_suffix(".html")
        .or_else(|| name.strip_suffix(".htm"))
        .unwrap_or(name);
    Some(format!("/{without}"))
}

fn page_title(name: &str, bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    desegnos_core::identity_source_page(name, &text)
        .map(|page| page.title)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| {
            name.trim_end_matches(".html")
                .trim_end_matches(".htm")
                .trim_matches('/')
                .rsplit('/')
                .next()
                .filter(|part| !part.is_empty() && *part != "index")
                .map(|part| part.replace(['-', '_'], " "))
                .unwrap_or_else(|| "Home".into())
        })
}

/// Read a folder or ZIP without changing it. A folder with one obvious site subfolder
/// (`dist`, `build`, `public`, `_site`, `out`) is served from that subfolder.
pub fn collect_website_snapshot(path: &Path) -> Result<WebsiteSnapshot> {
    let metadata = fs::symlink_metadata(path).map_err(io)?;
    if metadata.file_type().is_symlink() {
        return Err(Error(
            "Choose the original website folder; symbolic links are excluded from imports.".into(),
        ));
    }
    let stem = if metadata.is_file() {
        path.file_stem()
    } else {
        path.file_name()
    };
    let name = stem
        .and_then(|name| name.to_str())
        .unwrap_or("Imported website")
        .chars()
        .take(120)
        .collect();
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    if metadata.is_dir() {
        let root = path.canonicalize().map_err(io)?;
        let mut entries = 0;
        let mut total = 0_u64;
        collect_folder(&root, &root, &mut files, &mut skipped, &mut entries, &mut total)?;
        strip_site_root(&mut files);
    } else if metadata.is_file() {
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        {
            return Err(Error("Choose a website folder or a .zip website archive.".into()));
        }
        if metadata.len() > MAX_TOTAL_BYTES {
            return Err(Error("The website ZIP exceeds the 128 MiB limit.".into()));
        }
        let file = fs::File::open(path).map_err(io)?;
        collect_zip(file, &mut files, &mut skipped)?;
        strip_site_root(&mut files);
    } else {
        return Err(Error("Choose a website folder or a .zip website archive.".into()));
    }
    if files.iter().all(|(name, _)| !matches!(extension(name).as_str(), "html" | "htm")) {
        return Err(Error(
            "That folder or archive has no HTML page to render.".into(),
        ));
    }
    let mut pages = files
        .iter()
        .filter_map(|(name, bytes)| {
            page_route(name).map(|route| WebsitePage {
                title: page_title(name, bytes),
                route,
            })
        })
        .collect::<Vec<_>>();
    pages.sort_by(|first, second| {
        (first.route != "/", &first.route).cmp(&(second.route != "/", &second.route))
    });
    pages.dedup_by(|first, second| first.route == second.route);
    files.sort_by(|first, second| first.0.cmp(&second.0));
    Ok(WebsiteSnapshot {
        name,
        files,
        pages,
        skipped,
    })
}

fn collect_folder(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
    skipped: &mut Vec<String>,
    entries: &mut usize,
    total: &mut u64,
) -> Result<()> {
    let mut children = fs::read_dir(directory)
        .map_err(io)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(io)?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        *entries += 1;
        if *entries > MAX_ENTRIES {
            return Err(Error(
                "The website has more than 4,000 entries. Choose the site folder itself.".into(),
            ));
        }
        let path = child.path();
        let relative = path.strip_prefix(root).map_err(io)?;
        let Some(name) = safe_name(&relative.to_string_lossy()) else {
            skipped.push(format!("{}: unsafe or unsupported name", relative.display()));
            continue;
        };
        if excluded(&name) {
            skipped.push(format!("{name}: hidden or dependency folder"));
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(io)?;
        if metadata.file_type().is_symlink() {
            skipped.push(format!("{name}: symbolic link"));
            continue;
        }
        if metadata.is_dir() {
            collect_folder(root, &path, files, skipped, entries, total)?;
            continue;
        }
        if !metadata.is_file() || !web_file(&name) {
            skipped.push(format!("{name}: not a web file"));
            continue;
        }
        if files.len() >= MAX_FILES {
            return Err(Error("The website has more than 1,500 web files.".into()));
        }
        if metadata.len() > MAX_FILE_BYTES {
            skipped.push(format!("{name}: larger than 8 MiB"));
            continue;
        }
        *total = total.checked_add(metadata.len()).ok_or_else(|| Error("Website size overflow.".into()))?;
        if *total > MAX_TOTAL_BYTES {
            return Err(Error("The website exceeds the 128 MiB import limit.".into()));
        }
        let bytes = fs::read(&path).map_err(io)?;
        files.push((name, bytes));
    }
    Ok(())
}

fn collect_zip<R: Read + Seek>(reader: R, files: &mut Vec<(String, Vec<u8>)>, skipped: &mut Vec<String>) -> Result<()> {
    let mut archive = ZipArchive::new(reader).map_err(|_| Error("The ZIP could not be opened.".into()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(Error("The website ZIP has more than 4,000 entries.".into()));
    }
    let mut total = 0_u64;
    let mut names = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| Error("The ZIP contains an unreadable entry.".into()))?;
        let Some(name) = super::identity_source::safe_archive_name(entry.name()) else {
            skipped.push(format!("{}: unsafe path", entry.name()));
            continue;
        };
        if entry.enclosed_name().is_none() {
            skipped.push(format!("{name}: unsafe path"));
            continue;
        }
        let Some(name) = safe_name(&name) else {
            skipped.push(format!("{name}: unsafe path"));
            continue;
        };
        if entry
            .unix_mode()
            .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000))
        {
            skipped.push(format!("{name}: symbolic link or special file"));
            continue;
        }
        if entry.is_dir() || excluded(&name) || !web_file(&name) {
            continue;
        }
        if files.len() >= MAX_FILES {
            return Err(Error("The website ZIP has more than 1,500 web files.".into()));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| Error("Website ZIP size overflow.".into()))?;
        if total > MAX_TOTAL_BYTES {
            return Err(Error("The website ZIP exceeds the 128 MiB expanded limit.".into()));
        }
        if entry.size() > MAX_FILE_BYTES {
            skipped.push(format!("{name}: larger than 8 MiB"));
            continue;
        }
        if !names.insert(name.clone()) {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(io)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            skipped.push(format!("{name}: larger than 8 MiB"));
            continue;
        }
        files.push((name, bytes));
    }
    Ok(())
}

/// If the site lives in one obvious build folder, serve that folder as the site root.
fn strip_site_root(files: &mut Vec<(String, Vec<u8>)>) {
    let has_root_index = files.iter().any(|(name, _)| name == "index.html" || name == "index.htm");
    if has_root_index {
        return;
    }
    let mut candidates = BTreeSet::new();
    for (name, _) in files.iter() {
        if let Some((first, _rest)) = name.split_once('/') {
            candidates.insert(first.to_owned());
        }
    }
    let obvious = ["dist", "build", "public", "_site", "out", "site"];
    let root = candidates
        .iter()
        .find(|name| obvious.contains(&name.as_str()))
        .cloned()
        .or_else(|| (candidates.len() == 1).then(|| candidates.iter().next().cloned()).flatten());
    let Some(root) = root else { return };
    let prefix = format!("{root}/");
    if !files.iter().any(|(name, _)| name.starts_with(&prefix) && (name.ends_with("/index.html") || name == "index.html")) {
        return;
    }
    let mut stripped = Vec::new();
    for (name, bytes) in files.drain(..) {
        if let Some(name) = name.strip_prefix(&prefix) {
            stripped.push((name.to_owned(), bytes));
        }
    }
    *files = stripped;
}

/// Write the snapshot into a private temporary folder for the local static server.
pub fn write_website_snapshot(folder: &Path, snapshot: &WebsiteSnapshot) -> Result<()> {
    let root = folder.canonicalize().unwrap_or_else(|_| folder.to_path_buf());
    for (name, bytes) in &snapshot.files {
        let Some(name) = safe_name(name) else {
            return Err(Error("An imported file has an unsafe name.".into()));
        };
        let mut path = root.clone();
        for part in name.split('/') {
            path.push(part);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        fs::write(&path, bytes).map_err(io)?;
    }
    Ok(())
}

/// A private folder for one import; the caller removes it when another source is chosen.
pub fn new_import_folder() -> Result<PathBuf> {
    let folder = std::env::temp_dir().join(format!(
        "desegnos-website-import-{}",
        uuid::Uuid::new_v4()
    ));
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&folder).map_err(io)?;
    Ok(folder)
}
