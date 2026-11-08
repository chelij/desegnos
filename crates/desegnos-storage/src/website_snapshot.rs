//! Read a local website folder or ZIP into a bounded set of files that can be served read-only
//! for rendering. Nothing here writes outside the returned list or follows symlinks.
use crate::io;
use crate::source_files::{
    SourcePurpose, excessive_expansion, excluded as source_excluded, extension, is_download,
    open_unchanged, ordinary_zip_mode, priority, read_exact_bounded, safe_archive_name,
};
use desegnos_core::{Error, Result};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};
use zip::ZipArchive;

const MAX_ENTRIES: usize = 50_000;
const MAX_FILES: usize = 1_500;
const MAX_FILE_BYTES: u64 = desegnos_core::limits::MAX_WEBSITE_FILE_BYTES as u64;
const MAX_TOTAL_BYTES: u64 = crate::source_files::MAX_WEBSITE_BYTES as u64;

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
    /// Exact relative file name, including its original case and extension.
    pub file: String,
}

const INDEX_NAMES: [&str; 6] = [
    "index.html",
    "index.htm",
    "default.html",
    "default.htm",
    "home.html",
    "home.htm",
];
fn index_name(name: &str) -> bool {
    INDEX_NAMES.contains(&name.to_ascii_lowercase().as_str())
}

fn page_route(name: &str) -> Option<String> {
    if priority(name) != Some(0) {
        return None;
    }
    let (directory, leaf) = name.rsplit_once('/').unwrap_or(("", name));
    if index_name(leaf) {
        return Some(if directory.is_empty() {
            "/".into()
        } else {
            format!("/{directory}/")
        });
    }
    Some(format!("/{}", name.rsplit_once('.')?.0))
}

pub fn decode_html(bytes: &[u8]) -> String {
    use encoding_rs::{Encoding, WINDOWS_1252};
    if let Some((encoding, length)) = Encoding::for_bom(bytes) {
        return encoding.decode(&bytes[length..]).0.into_owned();
    }
    // Charset declarations are ASCII even when the surrounding document is not UTF-8.
    let preview = String::from_utf8_lossy(bytes);
    let doc = dom_query::Document::from(preview.as_ref());
    for node in doc.select("meta").nodes().iter() {
        let label = node
            .attr("charset")
            .map(|value| value.to_string())
            .or_else(|| {
                if !node
                    .attr("http-equiv")
                    .is_some_and(|v| v.eq_ignore_ascii_case("content-type"))
                {
                    return None;
                }
                let content = node.attr("content")?;
                let lower = content.to_ascii_lowercase();
                let start = lower.find("charset")? + 7;
                Some(
                    content[start..]
                        .trim_start()
                        .strip_prefix('=')?
                        .trim()
                        .trim_matches(['\'', '"'])
                        .split([';', ' ', '\'', '"'])
                        .next()?
                        .to_owned(),
                )
            });
        if let Some(encoding) = label.and_then(|label| Encoding::for_label(label.trim().as_bytes()))
        {
            return encoding.decode(bytes).0.into_owned();
        }
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .unwrap_or_else(|_| WINDOWS_1252.decode(bytes).0.into_owned())
}

pub fn page_title(name: &str, html: &str) -> String {
    let doc = dom_query::Document::from(html);
    let title = doc.select("title").first().text().trim().to_owned();
    if !title.is_empty() {
        return title;
    }
    let leaf = name.rsplit('/').next().unwrap_or(name);
    if index_name(leaf) {
        "Home".into()
    } else {
        leaf.rsplit_once('.')
            .map_or(leaf, |(stem, _)| stem)
            .replace(['-', '_'], " ")
    }
}

pub fn page_file(route: &str) -> String {
    let route = route.trim_start_matches('/');
    if route.is_empty() || route.ends_with('/') {
        format!("{route}index.html")
    } else if route.ends_with(".html") || route.ends_with(".htm") {
        route.to_owned()
    } else {
        format!("{route}.html")
    }
}

pub fn decode_url_path(path: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(path.len());
    let mut chars = path.bytes();
    while let Some(byte) = chars.next() {
        bytes.push(if byte == b'%' {
            let high = (chars.next()? as char).to_digit(16)?;
            let low = (chars.next()? as char).to_digit(16)?;
            (high * 16 + low) as u8
        } else {
            byte
        });
    }
    let value = String::from_utf8(bytes).ok()?;
    if value.contains('\\') || value.chars().any(char::is_control) {
        return None;
    }
    Some(value)
}

fn resolve_link(from: &str, target: &str, files: &BTreeMap<String, String>) -> Option<String> {
    let mut base = url::Url::parse("https://snapshot.invalid/").ok()?;
    base.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .extend(from.split('/'));
    let resolved = base.join(&target.replace('\\', "/")).ok()?;
    if resolved.origin() != base.origin() {
        return None;
    }
    let path = decode_url_path(resolved.path())?
        .trim_start_matches('/')
        .to_ascii_lowercase();
    if let Some(file) = files.get(&path) {
        return Some(file.clone());
    }
    for index in INDEX_NAMES {
        let candidate = format!(
            "{}{index}",
            if path.is_empty() {
                String::new()
            } else {
                format!("{}/", path.trim_end_matches('/'))
            }
        );
        if let Some(file) = files.get(&candidate) {
            return Some(file.clone());
        }
    }
    None
}

fn snapshot_pages(files: &[(String, Vec<u8>)]) -> Vec<WebsitePage> {
    let names = files
        .iter()
        .filter(|(name, _)| priority(name) == Some(0))
        .map(|(name, _)| (name.to_ascii_lowercase(), name.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut pages = BTreeMap::new();
    let mut links = BTreeMap::new();
    let mut frames = BTreeMap::new();
    for (name, bytes) in files.iter().filter(|(name, _)| priority(name) == Some(0)) {
        let text = decode_html(bytes);
        let doc = dom_query::Document::from(text.as_str());
        let targets = doc
            .select("a[href], frame[src], iframe[src]")
            .nodes()
            .iter()
            .filter_map(|node| {
                let target = node.attr("href").or_else(|| node.attr("src"))?;
                resolve_link(name, &target, &names)
            })
            .collect::<Vec<_>>();
        links.insert(name.clone(), targets);
        if doc.select("frameset").length() > 0 {
            let frame_nodes = doc.select("frame[src]");
            let nodes = frame_nodes.nodes();
            let main = nodes
                .iter()
                .find(|node| {
                    node.attr("name").is_some_and(|v| {
                        matches!(v.to_ascii_lowercase().as_str(), "main" | "content" | "body")
                    })
                })
                .or_else(|| nodes.last());
            if let Some(target) = main
                .and_then(|node| node.attr("src"))
                .and_then(|src| resolve_link(name, &src, &names))
            {
                frames.insert(name.clone(), target);
            }
        } else {
            pages.insert(
                name.clone(),
                WebsitePage {
                    route: match page_route(name) {
                        Some(route) => route,
                        None => continue,
                    },
                    title: page_title(name, &text),
                    file: name.clone(),
                },
            );
        }
    }
    let home = INDEX_NAMES
        .iter()
        .find_map(|name| names.get(*name))
        .cloned()
        .or_else(|| pages.keys().next().cloned());
    if let Some(home) = &home {
        let mut target = home.clone();
        let mut seen = BTreeSet::new();
        while seen.insert(target.clone()) {
            let Some(next) = frames.get(&target) else {
                break;
            };
            target = next.clone();
        }
        if frames.contains_key(home)
            && let Some(page) = pages.get_mut(&target)
        {
            page.route = "/".into();
        }
    }
    let mut queue = VecDeque::from_iter(home);
    let mut visited = BTreeSet::new();
    let mut ordered = Vec::new();
    while let Some(file) = queue.pop_front() {
        if !visited.insert(file.clone()) {
            continue;
        }
        if let Some(page) = pages.remove(&file) {
            ordered.push(page);
        }
        if let Some(targets) = links.get(&file) {
            queue.extend(targets.iter().cloned());
        }
    }
    let mut unreachable = pages.into_values().collect::<Vec<_>>();
    unreachable.sort_by_key(|page| (page.file.to_ascii_lowercase(), page.file.clone()));
    ordered.extend(unreachable);
    // The promoted frame is home even if another frame appears first in the document.
    ordered.sort_by_key(|page| page.route != "/");
    let mut routes = BTreeSet::new();
    ordered.retain(|page| routes.insert(page.route.to_ascii_lowercase()));
    ordered
}

struct Candidate<T> {
    name: String,
    size: u64,
    source: T,
}
impl<T> Candidate<T> {
    fn fits(&self, count: usize, total: u64) -> bool {
        count < MAX_FILES
            && self.size <= MAX_FILE_BYTES
            && self.size <= MAX_TOTAL_BYTES.saturating_sub(total)
    }
}

// Scan CSS tokens without treating URLs in comments or ordinary strings as dependencies.
use desegnos_core::css_urls as css_references;

fn render_references(name: &str, bytes: &[u8]) -> Vec<String> {
    if priority(name) == Some(1) {
        return css_references(&decode_html(bytes));
    }
    if priority(name) != Some(0) {
        return Vec::new();
    }
    let text = decode_html(bytes);
    let doc = dom_query::Document::from(text.as_str());
    let mut references = Vec::new();
    for node in doc.select("img, script, frame, iframe").nodes().iter() {
        if let Some(src) = node.attr("src") {
            references.push(src.to_string());
        }
    }
    for node in doc.select("img[srcset]").nodes().iter() {
        if let Some(srcset) = node.attr("srcset") {
            for candidate in srcset.split(',') {
                if let Some(src) = candidate.split_whitespace().next() {
                    references.push(src.to_owned());
                }
            }
        }
    }
    for node in doc.select("link[href]").nodes().iter() {
        if node.attr("rel").is_some_and(|rel| {
            rel.split_whitespace()
                .any(|v| v.eq_ignore_ascii_case("stylesheet"))
        }) && let Some(href) = node.attr("href")
        {
            references.push(href.to_string());
        }
    }
    for node in doc.select("[style]").nodes().iter() {
        if let Some(style) = node.attr("style") {
            references.extend(css_references(&style));
        }
    }
    for node in doc.select("style").nodes().iter() {
        references.extend(css_references(&node.text()));
    }
    references
}

fn select_files<T>(
    mut candidates: Vec<Candidate<T>>,
    skipped: &mut Vec<String>,
    mut read: impl FnMut(&Candidate<T>) -> Result<Vec<u8>>,
) -> Result<Vec<(String, Vec<u8>)>> {
    let downloads = candidates
        .iter()
        .filter(|file| is_download(&file.name) && priority(&file.name).is_none())
        .count();
    if downloads > 0 {
        skipped.push(format!(
            "Left out {downloads} audio, video, document or data files not needed to render pages."
        ));
    }
    candidates.retain(|file| priority(&file.name).is_some());
    candidates.sort_by_key(|file| {
        (
            priority(&file.name),
            file.name.to_ascii_lowercase(),
            file.name.clone(),
        )
    });
    candidates.dedup_by(|a, b| a.name == b.name);
    let mut inventory = candidates
        .iter()
        .map(|c| (c.name.clone(), Vec::new()))
        .collect();
    let prefix = strip_site_root(&mut inventory);
    candidates.retain(|c| c.name.starts_with(&prefix));
    let normalized = inventory
        .iter()
        .map(|(name, _)| (name.to_ascii_lowercase(), name.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut total = 0;
    let mut files = Vec::new();
    let mut selected = BTreeSet::new();
    let mut rejected = BTreeSet::new();
    // HTML and CSS must be available for discovery, but share all final snapshot limits.
    for file in candidates
        .iter()
        .filter(|c| matches!(priority(&c.name), Some(0 | 1)))
    {
        if !file.fits(files.len(), total) {
            rejected.insert(file.name.clone());
            continue;
        }
        let bytes = read(file)?;
        total += file.size;
        selected.insert(file.name.clone());
        files.push((file.name.clone(), bytes));
    }
    for (name, _) in &mut files {
        *name = name[prefix.len()..].to_owned();
    }
    let cached = files
        .iter()
        .enumerate()
        .map(|(i, (name, _))| (name.clone(), i))
        .collect::<BTreeMap<_, _>>();
    let mut queue = snapshot_pages(&files)
        .into_iter()
        .take(desegnos_core::limits::MAX_IMPORT_PAGES)
        .map(|page| page.file)
        .collect::<VecDeque<_>>();
    let mut needed = BTreeSet::new();
    while let Some(name) = queue.pop_front() {
        if !needed.insert(name.clone()) {
            continue;
        }
        if let Some(index) = cached.get(&name) {
            for reference in render_references(&name, &files[*index].1) {
                if let Some(target) = resolve_link(&name, &reference, &normalized) {
                    queue.push_back(target);
                }
            }
        }
    }
    candidates.sort_by_key(|file| {
        (
            !needed.contains(file.name.strip_prefix(&prefix).unwrap_or(&file.name)),
            priority(&file.name),
            file.name.to_ascii_lowercase(),
            file.name.clone(),
        )
    });
    for file in candidates {
        if selected.contains(&file.name) || rejected.contains(&file.name) {
            continue;
        }
        if !file.fits(files.len(), total) {
            rejected.insert(file.name);
            continue;
        }
        let bytes = read(&file)?;
        total += file.size;
        files.push((
            file.name
                .strip_prefix(&prefix)
                .unwrap_or(&file.name)
                .to_owned(),
            bytes,
        ));
    }
    if !rejected.is_empty() {
        skipped.push(format!("Left out {} rendering files to keep the snapshot within {MAX_FILES} files, {} MiB total and {} MiB per file.", rejected.len(), MAX_TOTAL_BYTES / 1024 / 1024, MAX_FILE_BYTES / 1024 / 1024));
    }
    Ok(files)
}

fn inventory_folder(
    root: &Path,
    directory: &Path,
    candidates: &mut Vec<Candidate<fs::Metadata>>,
    skipped: &mut Vec<String>,
    entries: &mut usize,
) -> Result<()> {
    for child in fs::read_dir(directory).map_err(io)? {
        let child = child.map_err(io)?;
        *entries += 1;
        if *entries > MAX_ENTRIES {
            return Err(Error(format!(
                "The website has more than {MAX_ENTRIES} entries. Choose the site folder itself."
            )));
        }
        let path = child.path();
        let relative = path.strip_prefix(root).map_err(io)?;
        let Some(name) = safe_archive_name(&relative.to_string_lossy()) else {
            skipped.push(format!("{}: unsafe name", relative.display()));
            continue;
        };
        if source_excluded(&name, SourcePurpose::Render) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(io)?;
        if metadata.file_type().is_symlink() {
            skipped.push(format!("{name}: symbolic link"));
            continue;
        }
        if metadata.is_dir() {
            inventory_folder(root, &path, candidates, skipped, entries)?;
        } else if metadata.is_file() {
            candidates.push(Candidate {
                name,
                size: metadata.len(),
                source: metadata,
            });
        }
    }
    Ok(())
}

pub fn collect_import_snapshot(path: &Path) -> Result<WebsiteSnapshot> {
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
    let mut skipped = Vec::new();
    let mut files;
    if metadata.is_dir() {
        let mut candidates = Vec::new();
        inventory_folder(path, path, &mut candidates, &mut skipped, &mut 0)?;
        files = select_files(candidates, &mut skipped, |file| {
            let mut input = open_unchanged(&path.join(&file.name), &file.source)?;
            read_exact_bounded(&mut input, file.size, MAX_FILE_BYTES as usize)
        })?;
    } else if metadata.is_file()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        let mut candidates = Vec::new();
        let mut archive = ZipArchive::new(fs::File::open(path).map_err(io)?).map_err(io)?;
        if archive.len() > MAX_ENTRIES {
            return Err(Error(format!(
                "The website ZIP has more than {MAX_ENTRIES} entries. Choose the site folder itself."
            )));
        }
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(io)?;
            if entry.is_dir() {
                continue;
            }
            let name = safe_archive_name(entry.name());
            let Some(name) = name.filter(|_| entry.enclosed_name().is_some()) else {
                skipped.push(format!("{}: unsafe path", entry.name()));
                continue;
            };
            if source_excluded(&name, SourcePurpose::Render) {
                continue;
            }
            if entry.encrypted()
                || !ordinary_zip_mode(entry.unix_mode())
                || excessive_expansion(entry.size(), entry.compressed_size())
            {
                skipped.push(format!(
                    "{name}: encrypted, special file or excessive expansion ratio"
                ));
                continue;
            }
            candidates.push(Candidate {
                name,
                size: entry.size(),
                source: index,
            });
        }
        files = select_files(candidates, &mut skipped, |file| {
            let mut entry = archive.by_index(file.source).map_err(io)?;
            read_exact_bounded(&mut entry, file.size, MAX_FILE_BYTES as usize)
        })?;
    } else {
        return Err(Error(
            "Choose a website folder or a .zip website archive.".into(),
        ));
    }
    let pages = snapshot_pages(&files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(WebsiteSnapshot {
        name,
        files,
        pages,
        skipped,
    })
}

/// Read only native-dialog-selected HTML files, with the normal snapshot limits.
pub fn collect_import_html(paths: &[PathBuf]) -> Result<WebsiteSnapshot> {
    if paths.is_empty() || paths.len() > MAX_FILES {
        return Err(Error(format!(
            "Choose between 1 and {MAX_FILES} HTML files."
        )));
    }
    let mut candidates = Vec::new();
    let mut selected = BTreeSet::new();
    for path in paths {
        let metadata = fs::symlink_metadata(path).map_err(io)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(safe_archive_name)
            .ok_or_else(|| Error("An HTML file has an unsafe name.".into()))?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || !matches!(extension(&name).as_str(), "html" | "htm")
        {
            return Err(Error(
                "Choose regular HTML files, without symbolic links.".into(),
            ));
        }
        if metadata.len() > desegnos_core::limits::MAX_IDENTITY_SOURCE_BYTES as u64 {
            return Err(Error(format!(
                "Each chosen file must be at most {} MiB.",
                desegnos_core::limits::MAX_IDENTITY_SOURCE_BYTES / 1024 / 1024
            )));
        }
        if !selected.insert(name.clone()) {
            return Err(Error("Choose HTML files with different names.".into()));
        }
        candidates.push(Candidate {
            name,
            size: metadata.len(),
            source: (path.clone(), metadata),
        });
    }
    let mut skipped = Vec::new();
    let files = select_files(candidates, &mut skipped, |file| {
        let mut input = open_unchanged(&file.source.0, &file.source.1)?;
        read_exact_bounded(&mut input, file.size, MAX_FILE_BYTES as usize)
    })?;
    Ok(WebsiteSnapshot {
        name: "Selected HTML files".into(),
        pages: snapshot_pages(&files),
        files,
        skipped,
    })
}

fn strip_site_root(files: &mut Vec<(String, Vec<u8>)>) -> String {
    if files
        .iter()
        .any(|(name, _)| !name.contains('/') && index_name(name))
    {
        return String::new();
    }
    let candidates = files
        .iter()
        .filter_map(|(name, _)| name.split_once('/').map(|(first, _)| first.to_owned()))
        .collect::<BTreeSet<_>>();
    let root = candidates
        .iter()
        .find(|name| {
            ["dist", "build", "public", "_site", "out", "site"]
                .contains(&name.to_ascii_lowercase().as_str())
        })
        .or_else(|| {
            if candidates.len() == 1 {
                candidates.first()
            } else {
                None
            }
        });
    let Some(root) = root else {
        return String::new();
    };
    let prefix = format!("{root}/");
    if !files.iter().any(|(name, _)| {
        name.strip_prefix(&prefix)
            .is_some_and(|rest| !rest.contains('/') && index_name(rest))
    }) {
        return String::new();
    }
    files.retain(|(name, _)| name.starts_with(&prefix));
    for (name, _) in files {
        *name = name[prefix.len()..].to_owned();
    }
    prefix
}

/// Write the snapshot into a private temporary folder for the local static server.
pub fn write_website_snapshot(folder: &Path, snapshot: &WebsiteSnapshot) -> Result<()> {
    let root = folder
        .canonicalize()
        .map_err(|error| Error(format!("The import folder could not be opened: {error}")))?;
    for (name, bytes) in &snapshot.files {
        let Some(name) = safe_archive_name(name) else {
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
    let folder =
        std::env::temp_dir().join(format!("desegnos-website-import-{}", uuid::Uuid::new_v4()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(new_import_folder().unwrap())
        }
        fn add(&self, name: &str, bytes: &[u8]) {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        fn snapshot(&self) -> WebsiteSnapshot {
            collect_import_snapshot(&self.0).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn website_snapshot_case_insensitive_home_and_directory_indexes() {
        for home in [
            "INDEX.HTM",
            "Index.HTML",
            "DEFAULT.HTM",
            "Default.HTML",
            "HOME.HTM",
            "Home.HTML",
        ] {
            let site = Fixture::new();
            site.add(home, b"<title>Home &amp; news</title>");
            site.add("Articles/INDEX.HTM", b"<title>Articles</title>");
            site.add("NEWS.HTM", b"<p>News</p>");
            let snapshot = site.snapshot();
            assert_eq!(snapshot.pages[0].route, "/");
            assert_eq!(snapshot.pages[0].file, home);
            assert_eq!(snapshot.pages[0].title, "Home & news");
            assert!(snapshot.pages.iter().any(|p| p.route == "/Articles/"));
            assert!(
                snapshot
                    .pages
                    .iter()
                    .any(|p| p.route == "/NEWS" && p.title == "NEWS")
            );
        }
    }

    #[test]
    fn website_snapshot_frameset_home_promotes_main_once() {
        for (frames, expected) in [
            (
                "<frame name='main' src='CONTENT.HTM'><frame src='menu.htm'>",
                "content.htm",
            ),
            (
                "<frame src='menu.htm'><frame src='content.htm'>",
                "content.htm",
            ),
            (
                "<frame name='BODY' src='content.htm'><frame src='menu.htm'>",
                "content.htm",
            ),
        ] {
            let site = Fixture::new();
            site.add(
                "INDEX.HTM",
                format!("<frameset>{frames}</frameset>").as_bytes(),
            );
            site.add(
                "content.htm",
                b"<title>Main content</title><a href='next.htm'>Next</a>",
            );
            site.add("menu.htm", b"<p>Menu</p>");
            site.add("next.htm", b"<p>Next</p>");
            site.add("other.htm", b"<frameset><frame src='next.htm'></frameset>");
            let snapshot = site.snapshot();
            assert_eq!(snapshot.pages[0].route, "/");
            assert_eq!(snapshot.pages[0].file, expected);
            assert_eq!(snapshot.pages.len(), 3);
            assert_eq!(
                snapshot.pages.iter().filter(|p| p.file == expected).count(),
                1
            );
            assert!(
                !snapshot
                    .pages
                    .iter()
                    .any(|p| p.file == "INDEX.HTM" || p.file == "other.htm")
            );
        }
    }

    #[test]
    fn website_snapshot_legacy_titles_and_encoding_precedence() {
        for prefix in [
            "",
            "<meta charset='windows-1252'>",
            "<meta http-equiv='Content-Type' content='text/html; charset=windows-1252'>",
        ] {
            let html = [
                prefix.as_bytes(),
                b"<title>Article 1 \x96 Caf\xe9 news</title>",
            ]
            .concat();
            let text = decode_html(&html);
            assert_eq!(page_title("ARTICLE.HTM", &text), "Article 1 – Café news");
        }
        assert_eq!(
            decode_html("<title>Café</title>".as_bytes()),
            "<title>Café</title>"
        );
        assert_eq!(
            decode_html(b"\xef\xbb\xbf<meta charset=windows-1252><title>Caf\xc3\xa9</title>"),
            "<meta charset=windows-1252><title>Café</title>"
        );
        assert_eq!(decode_html(&[0xff, 0xfe, b'A', 0]), "A");
        let site = Fixture::new();
        site.add("INDEX.HTM", b"<title>Article 1 - Caf\xe9 news</title>");
        assert_eq!(site.snapshot().pages[0].title, "Article 1 - Café news");
    }

    #[test]
    fn website_snapshot_breadth_first_links_and_true_total() {
        let site = Fixture::new();
        site.add("INDEX.HTM", b"<a href='z.HTM?view=1#top'>Z</a><iframe src='dir/'></iframe><a href='https://elsewhere.invalid/a.htm'>External</a>");
        site.add(
            "Z.htm",
            b"<a href='deep/../B.htm'>B</a><a href='INDEX.HTM'>Home</a>",
        );
        site.add(
            "dir/DEFAULT.HTML",
            b"<a href='../space%20page.htm'>Space</a>",
        );
        site.add("B.htm", b"<p>B</p>");
        site.add("space page.htm", b"<p>Space</p>");
        for index in 0..58 {
            site.add(&format!("a{index:02}.htm"), b"<p>Unreachable</p>");
        }
        let pages = site.snapshot().pages;
        assert_eq!(pages.len(), 63);
        assert_eq!(
            pages
                .iter()
                .take(5)
                .map(|p| p.file.as_str())
                .collect::<Vec<_>>(),
            [
                "INDEX.HTM",
                "Z.htm",
                "dir/DEFAULT.HTML",
                "B.htm",
                "space page.htm"
            ]
        );
        assert_eq!(pages[5].file, "a00.htm");
    }

    #[test]
    fn website_snapshot_priority_and_size_limits_skip_without_failing() {
        let mut candidates = vec![Candidate {
            name: "INDEX.HTM".into(),
            size: 20,
            source: 0,
        }];
        for (name, size) in [
            ("a.mp3", MAX_TOTAL_BYTES * 2),
            ("big.pdf", MAX_TOTAL_BYTES),
            ("z.css", 100),
            ("z.gif", 100),
            ("z.woff", 100),
            ("z.js", 100),
            ("huge.gif", MAX_FILE_BYTES + 1),
        ] {
            candidates.push(Candidate {
                name: name.into(),
                size,
                source: 0,
            });
        }
        for index in 0..20 {
            candidates.push(Candidate {
                name: format!("{index:02}.png"),
                size: MAX_FILE_BYTES,
                source: 0,
            });
        }
        let mut skipped = Vec::new();
        let selected = select_files(candidates, &mut skipped, |file| {
            Ok(vec![0; file.size as usize])
        })
        .unwrap();
        assert_eq!(selected[0].0, "INDEX.HTM");
        assert_eq!(selected[1].0, "z.css");
        assert!(selected.iter().map(|c| c.1.len() as u64).sum::<u64>() <= MAX_TOTAL_BYTES);
        assert!(selected.iter().any(|c| c.0 == "z.js"));
        assert!(
            !selected
                .iter()
                .any(|c| is_download(&c.0) || c.0 == "huge.gif")
        );
        assert_eq!(skipped.len(), 2);
    }

    #[test]
    fn website_snapshot_folder_and_zip_inventory_before_reading() {
        let site = Fixture::new();
        site.add("INDEX.HTM", b"<title>Home</title>");
        // More than the old 4,000-entry limit, with more than 1,500 rendering files.
        for index in 0..1800 {
            site.add(&format!("a{index:04}.GIF"), b"gif");
        }
        for index in 0..2200 {
            site.add(&format!("download{index:04}.MP3"), b"audio");
        }
        let folder = site.snapshot();
        assert_eq!(folder.files.len(), MAX_FILES);
        assert_eq!(folder.pages[0].file, "INDEX.HTM");
        assert_eq!(folder.skipped.len(), 2);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for entry in fs::read_dir(&site.0).unwrap() {
            let path = entry.unwrap().path();
            zip.start_file(
                path.file_name().unwrap().to_str().unwrap(),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(&fs::read(path).unwrap()).unwrap();
        }
        let archive = Fixture::new();
        archive.add("website.ZIP", &zip.finish().unwrap().into_inner());
        let zipped = collect_import_snapshot(&archive.0.join("website.ZIP")).unwrap();
        assert_eq!(zipped.files, folder.files);
        assert_eq!(zipped.pages, folder.pages);
        assert_eq!(zipped.skipped, folder.skipped);
    }

    #[test]
    fn website_snapshot_imported_pages_keep_alphabetically_last_assets() {
        let site = Fixture::new();
        let mut home = String::from(
            "<link rel='alternate STYLESHEET' href='/CSS/Main.CSS'>
            <script src='scripts/Z.JS'></script>
            <style>body { background: URL('images/z-style.gif') }</style>
            <div style=\"background: url(images/z-inline.gif)\"></div>
            <img srcset='images/z-small.gif 1x, images/z-large.gif 2x'>",
        );
        for index in (0..29).rev() {
            home.push_str(&format!("<a href='pages/z{index:02}.htm'>Page</a>"));
            site.add(
                &format!("old-site/pages/z{index:02}.htm"),
                format!("<img src='../IMAGES/Z{index:02}.GIF?version=1#image'>").as_bytes(),
            );
            site.add(&format!("old-site/images/z{index:02}.gif"), b"gif");
        }
        home.push_str("<iframe src='frame.htm'></iframe>");
        site.add("old-site/index.htm", home.as_bytes());
        site.add("old-site/frame.htm", b"<img src='images/z-frame.gif'>");
        for index in 0..371 {
            site.add(
                &format!("old-site/a{index:03}.htm"),
                b"<img src='images/zz-unused.gif'>",
            );
        }
        site.add(
            "old-site/css/main.css",
            b"@IMPORT 'nested/other.css'; body { background: url('../images/z-css.gif') }",
        );
        site.add(
            "old-site/css/nested/other.css",
            b"@import url(../main.css); @font-face { src: url('../../fonts/z.woff2') }",
        );
        site.add(
            "old-site/css/unused.css",
            b"body { background: url('../images/zz-unused.gif') }",
        );
        let assets = [
            "images/z-style.gif",
            "images/z-inline.gif",
            "images/z-small.gif",
            "images/z-large.gif",
            "images/z-css.gif",
            "images/z-frame.gif",
            "fonts/z.woff2",
            "scripts/z.js",
        ];
        for asset in assets {
            site.add(&format!("old-site/{asset}"), b"asset");
        }
        site.add("old-site/images/zz-unused.gif", b"unused");
        for index in 0..1800 {
            site.add(&format!("old-site/images/a{index:04}.gif"), b"gif");
        }
        let folder = site.snapshot();
        assert_eq!(folder.files.len(), MAX_FILES);
        assert_eq!(folder.pages.len(), 402);
        assert_eq!(folder.pages[1].file, "pages/z28.htm");
        assert_eq!(folder.pages[29].file, "pages/z00.htm");
        for page in folder
            .pages
            .iter()
            .take(desegnos_core::limits::MAX_IMPORT_PAGES)
            .skip(1)
        {
            let stem = page
                .file
                .strip_prefix("pages/")
                .unwrap()
                .strip_suffix(".htm")
                .unwrap();
            assert!(
                folder
                    .files
                    .iter()
                    .any(|(name, _)| name == &format!("images/{stem}.gif"))
            );
        }
        for asset in assets {
            assert!(
                folder.files.iter().any(|(name, _)| name == asset),
                "missing {asset}"
            );
        }
        assert!(
            !folder
                .files
                .iter()
                .any(|(name, _)| name == "images/zz-unused.gif")
        );

        let mut candidates = Vec::new();
        inventory_folder(&site.0, &site.0, &mut candidates, &mut Vec::new(), &mut 0).unwrap();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for candidate in candidates {
            zip.start_file(&candidate.name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&fs::read(site.0.join(candidate.name)).unwrap())
                .unwrap();
        }
        let archive = Fixture::new();
        archive.add("site.zip", &zip.finish().unwrap().into_inner());
        let zipped = collect_import_snapshot(&archive.0.join("site.zip")).unwrap();
        assert_eq!(zipped.files, folder.files);
        assert_eq!(zipped.pages, folder.pages);
        assert_eq!(zipped.skipped, folder.skipped);
    }

    #[test]
    fn website_snapshot_reads_each_selected_file_once() {
        let contents = [
            (
                "index.htm",
                "<link rel='stylesheet' href='main.css'><img src='z.gif'>",
            ),
            (
                "main.css",
                "@import 'main.css'; body { background: url(z.gif) }",
            ),
            ("z.gif", "gif"),
            ("unused.gif", "gif"),
        ];
        let candidates = contents
            .iter()
            .enumerate()
            .map(|(index, (name, bytes))| Candidate {
                name: (*name).into(),
                size: bytes.len() as u64,
                source: index,
            })
            .collect();
        let mut reads = BTreeSet::new();
        let files = select_files(candidates, &mut Vec::new(), |file| {
            assert!(reads.insert(file.name.clone()), "read twice: {}", file.name);
            Ok(contents[file.source].1.as_bytes().to_vec())
        })
        .unwrap();
        assert_eq!(files.len(), contents.len());
        assert_eq!(reads.len(), contents.len());
        assert_eq!(files[2].0, "z.gif");
    }

    #[test]
    fn website_snapshot_entry_limit_allows_exactly_fifty_thousand() {
        let site = Fixture::new();
        site.add("INDEX.HTM", b"<p>Home</p>");
        let mut candidates = Vec::new();
        let mut skipped = Vec::new();
        let mut entries = MAX_ENTRIES - 1;
        inventory_folder(
            &site.0,
            &site.0,
            &mut candidates,
            &mut skipped,
            &mut entries,
        )
        .unwrap();
        assert_eq!(entries, MAX_ENTRIES);
        let error = inventory_folder(
            &site.0,
            &site.0,
            &mut candidates,
            &mut skipped,
            &mut entries,
        )
        .unwrap_err();
        assert!(error.to_string().contains(&MAX_ENTRIES.to_string()));
        assert!(error.to_string().contains("site folder itself"));
    }

    #[test]
    fn website_snapshot_wrapped_uppercase_home() {
        let site = Fixture::new();
        site.add("old-site/HOME.HTM", b"<title>Old site</title>");
        site.add("old-site/about.HTM", b"<p>About</p>");
        assert_eq!(site.snapshot().pages[0].file, "HOME.HTM");
    }
}
