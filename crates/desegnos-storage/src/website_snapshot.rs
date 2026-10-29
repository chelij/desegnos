//! Read a local website folder or ZIP into a bounded set of files that can be served read-only
//! for rendering. Nothing here writes outside the returned list or follows symlinks.
use desegnos_core::{Error, Result};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use zip::ZipArchive;

const MAX_ENTRIES: usize = 50_000;
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
    /// Exact relative file name, including its original case and extension.
    pub file: String,
}

fn io(error: impl std::fmt::Display) -> Error {
    Error(error.to_string())
}

fn extension(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}

fn priority(name: &str) -> Option<u8> {
    Some(match extension(name).as_str() {
        "html" | "htm" => 0,
        "css" => 1,
        "svg" | "png" | "jpg" | "jpeg" | "webp" | "gif" | "avif" | "ico" | "bmp" => 2,
        "woff" | "woff2" | "ttf" | "otf" | "eot" => 3,
        "js" | "mjs" | "cjs" => 4,
        _ => return None,
    })
}

fn download(name: &str) -> bool {
    matches!(
        extension(name).as_str(),
        "mp3" | "mp4" | "webm" | "ogg" | "wav" | "pdf" | "txt" | "json" | "xml"
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
                || part
                    .chars()
                    .any(|character| character.is_control() || "<>\"|*:?".contains(character))
        })
    {
        return None;
    }
    Some(name.to_owned())
}

fn index_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "index.html" | "index.htm" | "default.html" | "default.htm" | "home.html" | "home.htm"
    )
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

fn decode_html(bytes: &[u8]) -> String {
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

fn page_title(name: &str, doc: &dom_query::Document) -> String {
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
    // URL paths escape spaces/non-ASCII file names. Decode before matching the inventory.
    let path = resolved.path().as_bytes();
    let mut decoded = Vec::new();
    let mut i = 0;
    while i < path.len() {
        if path[i] == b'%' && i + 2 < path.len() {
            let byte =
                u8::from_str_radix(std::str::from_utf8(&path[i + 1..i + 3]).ok()?, 16).ok()?;
            decoded.push(byte);
            i += 3;
        } else {
            decoded.push(path[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(decoded)
        .ok()?
        .trim_start_matches('/')
        .to_ascii_lowercase();
    if let Some(file) = files.get(&path) {
        return Some(file.clone());
    }
    for index in [
        "index.html",
        "index.htm",
        "default.html",
        "default.htm",
        "home.html",
        "home.htm",
    ] {
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
                    route: page_route(name).unwrap(),
                    title: page_title(name, &doc),
                    file: name.clone(),
                },
            );
        }
    }
    let home = [
        "index.html",
        "index.htm",
        "default.html",
        "default.htm",
        "home.html",
        "home.htm",
    ]
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
        if frames.contains_key(home) {
            if let Some(page) = pages.get_mut(&target) {
                page.route = "/".into();
            }
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

struct Candidate {
    name: String,
    size: u64,
    index: usize,
}

fn select_files(mut candidates: Vec<Candidate>, skipped: &mut Vec<String>) -> Vec<Candidate> {
    let downloads = candidates
        .iter()
        .filter(|file| download(&file.name))
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
    let mut total = 0;
    let mut count = 0;
    let mut omitted = 0;
    let mut names = BTreeSet::new();
    candidates.retain(|file| {
        if !names.insert(file.name.clone()) {
            return false;
        }
        if count >= MAX_FILES || file.size > MAX_FILE_BYTES || file.size > MAX_TOTAL_BYTES - total {
            omitted += 1;
            return false;
        }
        count += 1;
        total += file.size;
        true
    });
    if omitted > 0 {
        skipped.push(format!("Left out {omitted} rendering files to keep the snapshot within 1,500 files, 128 MiB total and 8 MiB per file."));
    }
    candidates
}

fn inventory_folder(
    root: &Path,
    directory: &Path,
    candidates: &mut Vec<Candidate>,
    skipped: &mut Vec<String>,
    entries: &mut usize,
) -> Result<()> {
    for child in fs::read_dir(directory).map_err(io)? {
        let child = child.map_err(io)?;
        *entries += 1;
        if *entries > MAX_ENTRIES {
            return Err(Error(
                "The website has more than 50,000 entries. Choose the site folder itself.".into(),
            ));
        }
        let path = child.path();
        let relative = path.strip_prefix(root).map_err(io)?;
        let Some(name) = safe_name(&relative.to_string_lossy()) else {
            skipped.push(format!("{}: unsafe name", relative.display()));
            continue;
        };
        if excluded(&name) {
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
                index: 0,
            });
        }
    }
    Ok(())
}

/// Read only the selected rendering files after inventorying a folder or ZIP.
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
    let mut candidates = Vec::new();
    let mut skipped = Vec::new();
    let mut files = Vec::new();
    if metadata.is_dir() {
        inventory_folder(path, path, &mut candidates, &mut skipped, &mut 0)?;
        for file in select_files(candidates, &mut skipped) {
            let mut bytes = Vec::new();
            fs::File::open(path.join(&file.name))
                .map_err(io)?
                .take(file.size + 1)
                .read_to_end(&mut bytes)
                .map_err(io)?;
            if bytes.len() as u64 != file.size {
                return Err(Error(
                    "An imported file changed size. Try importing again.".into(),
                ));
            }
            files.push((file.name, bytes));
        }
    } else if metadata.is_file()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        let mut archive = ZipArchive::new(fs::File::open(path).map_err(io)?).map_err(io)?;
        if archive.len() > MAX_ENTRIES {
            return Err(Error(
                "The website ZIP has more than 50,000 entries. Choose the site folder itself."
                    .into(),
            ));
        }
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(io)?;
            if entry.is_dir() {
                continue;
            }
            let name = super::identity_source::safe_archive_name(entry.name())
                .and_then(|name| safe_name(&name));
            let Some(name) = name.filter(|_| entry.enclosed_name().is_some()) else {
                skipped.push(format!("{}: unsafe path", entry.name()));
                continue;
            };
            if excluded(&name) {
                continue;
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000))
            {
                skipped.push(format!("{name}: symbolic link or special file"));
                continue;
            }
            candidates.push(Candidate {
                name,
                size: entry.size(),
                index,
            });
        }
        for file in select_files(candidates, &mut skipped) {
            let mut bytes = Vec::new();
            archive
                .by_index(file.index)
                .map_err(io)?
                .take(file.size + 1)
                .read_to_end(&mut bytes)
                .map_err(io)?;
            if bytes.len() as u64 != file.size {
                return Err(Error("A ZIP entry has an invalid size.".into()));
            }
            files.push((file.name, bytes));
        }
    } else {
        return Err(Error(
            "Choose a website folder or a .zip website archive.".into(),
        ));
    }
    strip_site_root(&mut files);
    let pages = snapshot_pages(&files);
    if pages.is_empty() {
        return Err(Error(
            "That folder or archive has no HTML page to render.".into(),
        ));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(WebsiteSnapshot {
        name,
        files,
        pages,
        skipped,
    })
}

fn strip_site_root(files: &mut Vec<(String, Vec<u8>)>) {
    if files
        .iter()
        .any(|(name, _)| !name.contains('/') && index_name(name))
    {
        return;
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
    let Some(root) = root else { return };
    let prefix = format!("{root}/");
    if !files.iter().any(|(name, _)| {
        name.strip_prefix(&prefix)
            .is_some_and(|rest| !rest.contains('/') && index_name(rest))
    }) {
        return;
    }
    files.retain(|(name, _)| name.starts_with(&prefix));
    for (name, _) in files {
        *name = name[prefix.len()..].to_owned();
    }
}

/// Write the snapshot into a private temporary folder for the local static server.
pub fn write_website_snapshot(folder: &Path, snapshot: &WebsiteSnapshot) -> Result<()> {
    let root = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());
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
            collect_website_snapshot(&self.0).unwrap()
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
            let doc = dom_query::Document::from(text.as_str());
            assert_eq!(page_title("ARTICLE.HTM", &doc), "Article 1 – Café news");
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
            index: 0,
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
                index: 0,
            });
        }
        for index in 0..20 {
            candidates.push(Candidate {
                name: format!("{index:02}.png"),
                size: MAX_FILE_BYTES,
                index: 0,
            });
        }
        let mut skipped = Vec::new();
        let selected = select_files(candidates, &mut skipped);
        assert_eq!(selected[0].name, "INDEX.HTM");
        assert_eq!(selected[1].name, "z.css");
        assert!(selected.iter().map(|c| c.size).sum::<u64>() <= MAX_TOTAL_BYTES);
        assert!(selected.iter().any(|c| c.name == "z.js"));
        assert!(
            !selected
                .iter()
                .any(|c| download(&c.name) || c.name == "huge.gif")
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
        let zipped = collect_website_snapshot(&archive.0.join("website.ZIP")).unwrap();
        assert_eq!(zipped.files, folder.files);
        assert_eq!(zipped.pages, folder.pages);
        assert_eq!(zipped.skipped, folder.skipped);
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
        assert!(error.to_string().contains("50,000"));
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
