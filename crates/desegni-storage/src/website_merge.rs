//! Bounded website input, shared by the native current-project import flow.
use crate::{Error, Result, io, project_folder};
use std::{collections::BTreeMap, fs::File, io::Read, path::PathBuf};

pub fn read_website_selection(paths: &[PathBuf]) -> Result<BTreeMap<String, Vec<u8>>> {
    if paths.len() == 1 && paths[0].is_dir() {
        let detected = crate::detect_website_folder(&paths[0])?;
        let root = detected
            .as_ref()
            .and_then(|site| site.output.as_ref())
            .unwrap_or(&paths[0]);
        return Ok(project_folder::read_website_files(root)?.files);
    }
    if paths.len() == 1
        && paths[0]
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        let mut zip = zip::ZipArchive::new(File::open(&paths[0]).map_err(io)?).map_err(io)?;
        if zip.len() > 10_000 {
            return Err(Error("The website has too many files.".into()));
        }
        let mut files = BTreeMap::new();
        let mut total = 0;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(io)?;
            let name = crate::identity_source::safe_archive_name(entry.name())
                .ok_or_else(|| Error("The ZIP contains an unsafe path.".into()))?;
            if entry.enclosed_name().is_none()
                || entry
                    .unix_mode()
                    .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000))
            {
                return Err(Error("The ZIP contains an unsafe path.".into()));
            }
            if entry.is_dir()
                || name.split('/').any(|part| {
                    part.starts_with('.')
                        || matches!(part, "__MACOSX" | "node_modules" | "target" | "vendor")
                })
            {
                continue;
            }
            if entry.size() > 128 * 1024 * 1024 || total + entry.size() > 128 * 1024 * 1024 {
                return Err(Error(
                    "The website exceeds the 128 MiB import limit.".into(),
                ));
            }
            total += entry.size();
            let expected = entry.size();
            let mut bytes = Vec::new();
            (&mut entry)
                .take(expected + 1)
                .read_to_end(&mut bytes)
                .map_err(io)?;
            if bytes.len() as u64 != expected || files.insert(name, bytes).is_some() {
                return Err(Error(
                    "The ZIP contains inconsistent or duplicate files.".into(),
                ));
            }
        }
        // Recognize a named archive wrapper without stripping a real route such as about/.
        let archive_name = paths[0]
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !files.keys().any(|p| !p.contains('/')) && !files.is_empty() {
            let prefix = format!(
                "{}/",
                files
                    .keys()
                    .next()
                    .expect("files")
                    .split('/')
                    .next()
                    .expect("path")
            );
            if prefix == format!("{archive_name}/") && files.keys().all(|p| p.starts_with(&prefix))
            {
                files = files
                    .into_iter()
                    .map(|(p, b)| (p[prefix.len()..].into(), b))
                    .collect();
            }
        }
        return Ok(files);
    }
    if paths.is_empty() {
        return Err(Error("Choose HTML files to import.".into()));
    }
    let mut root = paths[0]
        .parent()
        .ok_or_else(|| Error("Choose HTML files to import.".into()))?;
    while !paths.iter().all(|p| p.starts_with(root)) {
        root = root
            .parent()
            .ok_or_else(|| Error("Choose HTML files from one website.".into()))?;
    }
    // A single nested HTML selection can still belong to a site rooted beside index.html.
    if let Some(site_root) = root
        .ancestors()
        .skip(1)
        .find(|p| p.join("index.html").is_file())
    {
        root = site_root;
    }
    // Snapshot adjacent resources with the existing folder reader, then limit HTML to the selection.
    let mut files = project_folder::read_website_files(root)?.files;
    let selected: Vec<_> = paths
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        })
        .collect::<std::result::Result<_, _>>()
        .map_err(io)?;
    files.retain(|p, _| {
        !(p.to_ascii_lowercase().ends_with(".html") || p.to_ascii_lowercase().ends_with(".htm"))
            || selected.contains(p)
    });
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[test]
    fn zip_preserves_shared_assets_and_unwraps_one_common_folder() {
        let root = std::env::temp_dir().join(format!("website-selection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let archive = root.join("site.zip");
        let mut zip = ZipWriter::new(File::create(&archive).unwrap());
        for (name, bytes) in [
            (
                "site/index.html",
                "<!doctype html><html><body>Hello</body></html>",
            ),
            ("site/assets/site.js", "console.log('hello')"),
            ("site/assets/site.css", "body { color: red }"),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let files = read_website_selection(&[archive]).unwrap();
        assert!(files.contains_key("index.html"));
        assert!(files.contains_key("assets/site.js"));
        assert!(files.contains_key("assets/site.css"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selected_html_keeps_adjacent_resources_but_not_unselected_pages() {
        let root = std::env::temp_dir().join(format!("website-selection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("about.html"), "<h1>About</h1>").unwrap();
        std::fs::write(root.join("index.html"), "<h1>Home</h1>").unwrap();
        std::fs::write(root.join("shared.js"), "console.log('shared')").unwrap();
        let files = read_website_selection(&[root.join("about.html")]).unwrap();
        assert!(files.contains_key("about.html"));
        assert!(files.contains_key("shared.js"));
        assert!(!files.contains_key("index.html"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
