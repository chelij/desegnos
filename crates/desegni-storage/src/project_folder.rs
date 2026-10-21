//! Project folders own only the site files recorded here. Archives remain independently durable.
use crate::{ProjectFile, io, website_export};
use desegni_core::{Error, Project, Result, SiteContent, compile_website, digest};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

const MANIFEST: &str = ".desegnos-site.json";
const MAX_SITE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SiteManifest {
    version: u32,
    project_id: Uuid,
    project_file: String,
    files: BTreeMap<String, String>,
    /// A write-ahead record: retries recognize files published before an interrupted save.
    #[serde(default)]
    pending: BTreeMap<String, String>,
    /// Imported files may be replaced once, after their immutable original backup is verified.
    #[serde(default)]
    originals: BTreeMap<String, String>,
}

pub fn sanitize_project_name(name: &str) -> String {
    let mut clean: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(80)
        .collect();
    clean = clean.trim_matches([' ', '.']).to_owned();
    if clean.is_empty() {
        clean = "Website".into();
    }
    let stem = clean.split('.').next().unwrap_or("").to_uppercase();
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || (stem.len() >= 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&&stem[3..]))
        || clean.to_lowercase().starts_with(".desegnos")
    {
        clean.insert(0, '_');
    }
    // Bound bytes as well as characters for common filesystem component limits.
    while clean.len() > 180 {
        clean.pop();
    }
    clean.trim_end_matches([' ', '.']).to_owned()
}

/// Reserve with create_dir, so simultaneous creates and case-insensitive collisions converge.
pub fn create_project_folder(parent: &Path, name: &str, project_id: Uuid) -> Result<PathBuf> {
    if !parent.is_absolute() || parent.components().any(|c| c == Component::ParentDir) {
        return Err(Error("Choose an absolute project location.".into()));
    }
    fs::create_dir_all(parent).map_err(io)?;
    let parent = parent.canonicalize().map_err(io)?;
    let name = sanitize_project_name(name);
    for number in 1..10_000 {
        let name = if number == 1 {
            name.clone()
        } else {
            format!("{name} {number}")
        };
        // Use portable collision rules even on a case-sensitive host.
        if fs::read_dir(&parent).map_err(io)?.any(|entry| {
            entry.is_ok_and(|e| {
                e.file_name().to_string_lossy().to_lowercase() == name.to_lowercase()
            })
        }) {
            continue;
        }
        let folder = parent.join(&name);
        match fs::create_dir(&folder) {
            Ok(()) => {
                let path = folder.join(format!("{name}.desegnos"));
                if let Err(error) = manage_project_folder(&path, project_id, BTreeMap::new()) {
                    let _ = fs::remove_dir(&folder);
                    return Err(error);
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io(error)),
        }
    }
    Err(Error(
        "Too many projects share this name. Choose another name.".into(),
    ))
}

fn valid_site_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.ends_with(['.', ' '])
        })
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && !path
            .split('/')
            .any(|part| part.to_lowercase().starts_with(".desegnos"))
        && ![".desegnos", ".desegni", ".lock", ".previous"]
            .iter()
            .any(|ext| path.to_lowercase().ends_with(ext))
}

fn read_manifest(root: &Path) -> Result<SiteManifest> {
    let path = website_export::checked_path(root, MANIFEST)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(Error("The site manifest is too large.".into()));
    }
    let manifest: SiteManifest = serde_json::from_slice(&bytes).map_err(io)?;
    if manifest.version != 1
        || manifest.files.len() + manifest.pending.len() + manifest.originals.len() > 30_000
        || manifest
            .files
            .iter()
            .chain(&manifest.pending)
            .chain(&manifest.originals)
            .any(|(p, hash)| {
                !valid_site_path(p)
                    || hash.len() != 64
                    || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            })
    {
        return Err(Error(
            "The site manifest is invalid. Its original has been preserved.".into(),
        ));
    }
    Ok(manifest)
}

fn write_manifest(root: &Path, manifest: &SiteManifest) -> Result<()> {
    let path = website_export::checked_path(root, MANIFEST)?;
    crate::atomic_write(
        &path,
        |file| serde_json::to_writer(file, manifest).map_err(io),
        false,
    )
}

pub fn is_project_folder(path: &Path) -> bool {
    path.parent()
        .and_then(|root| read_manifest(root).ok())
        .is_some_and(|m| {
            path.file_name()
                .is_some_and(|name| name == m.project_file.as_str())
        })
}

/// Explicit folder selection adopts an existing archive without taking ownership of other files.
pub fn manage_project_folder(
    path: &Path,
    project_id: Uuid,
    originals: BTreeMap<String, String>,
) -> Result<()> {
    let root = path
        .parent()
        .ok_or_else(|| Error("Choose a project folder.".into()))?;
    website_export::parent_directory(root)?;
    if fs::symlink_metadata(root.join(MANIFEST)).is_ok() {
        let manifest = read_manifest(root)?;
        if manifest.project_id != project_id
            || path
                .file_name()
                .is_none_or(|name| name != manifest.project_file.as_str())
        {
            return Err(Error(
                "This folder belongs to another Desegnos project. Its files were preserved.".into(),
            ));
        }
        return Ok(());
    }
    let project_file = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error("Invalid project filename.".into()))?
        .to_owned();
    let manifest = SiteManifest {
        version: 1,
        project_id,
        project_file,
        files: BTreeMap::new(),
        pending: BTreeMap::new(),
        originals,
    };
    // No-replace publication avoids taking over a concurrently adopted folder.
    let json = serde_json::to_vec(&manifest).map_err(io)?;
    let temporary = root.join(format!(".desegnos-manifest-{}", Uuid::new_v4()));
    let result = (|| {
        crate::atomic_write(
            &temporary,
            |file| std::io::Write::write_all(file, &json).map_err(io),
            false,
        )?;
        crate::commit_new_file(&temporary, &root.join(MANIFEST))?;
        crate::sync_parent(&root.join(MANIFEST))
    })();
    let _ = fs::remove_file(temporary);
    result
}

fn file_hash(root: &Path, relative: &str) -> Result<Option<String>> {
    let path = website_export::checked_path(root, relative)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(e)),
    };
    if !metadata.is_file() || metadata.len() > MAX_SITE_BYTES {
        return Err(Error(format!(
            "{relative} is not a regular, bounded site file."
        )));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io)?
        .take(MAX_SITE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_SITE_BYTES {
        return Err(Error("A site file is too large.".into()));
    }
    Ok(Some(digest(&bytes)))
}

/// Only called after the archive commits. Failure becomes a save warning, never a lost save.
pub(crate) fn sync_project_site(
    path: &Path,
    project: &Project,
    managed: bool,
) -> Result<Vec<String>> {
    let root = path
        .parent()
        .ok_or_else(|| Error("Missing project folder.".into()))?;
    // Legacy standalone archives deliberately keep saving in place without claiming their parent.
    match fs::symlink_metadata(root.join(MANIFEST)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !managed => {
            return Ok(vec![]);
        }
        Err(error) => return Err(io(error)),
        Ok(_) => {}
    }
    let mut manifest = read_manifest(root)?;
    if path
        .file_name()
        .is_none_or(|name| name != manifest.project_file.as_str())
    {
        return Err(Error(format!(
            "The website in this folder belongs to {}. Save a copy in its own folder to sync this project.",
            manifest.project_file
        )));
    }
    website_export::parent_directory(root)?;
    let lock_path = website_export::checked_path(root, ".desegnos-site.lock")?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(io)?;
    lock.try_lock().map_err(|_| {
        Error("Another window is updating the website. Save again to retry.".into())
    })?;
    // Reload after taking the shared folder lock.
    manifest = read_manifest(root)?;
    // An explicit archive replacement can change its identity. Its tracked website paths
    // remain ours; the replaced archive is already preserved in .previous.
    manifest.project_id = project.id;
    if !manifest.originals.is_empty() {
        website_export::parent_directory(&root.join(".desegnos-original"))?;
    }
    for (relative, expected) in &manifest.originals {
        if file_hash(&root.join(".desegnos-original"), relative)?.as_ref() != Some(expected) {
            return Err(Error(
                "The original website backup is missing or changed. The site was left untouched."
                    .into(),
            ));
        }
    }
    let export = if project
        .pages
        .iter()
        .any(|p| p.included && p.content.is_some())
    {
        compile_website(project)?
    } else {
        let mut blank = project.clone();
        for page in blank.pages.iter_mut().filter(|p| p.included) {
            page.content = Some(SiteContent {
                html: String::new(),
                css: String::new(),
                javascript: String::new(),
            });
        }
        compile_website(&blank)?
    };
    website_export::validate(&export)?;
    if export.files.keys().any(|p| !valid_site_path(p)) {
        return Err(Error(
            "A website path conflicts with private project files.".into(),
        ));
    }
    let desired: BTreeMap<_, _> = export
        .files
        .iter()
        .map(|(p, b)| (p.clone(), digest(b)))
        .collect();
    let stage = website_export::stage_files(
        root,
        &format!(".desegnos-site-stage-{}", Uuid::new_v4()),
        export.files,
    )?;
    let result = (|| {
        // Retain earlier pending hashes until each file is checked, for crash recovery.
        let previous_pending = manifest.pending.clone();
        let mut allowed = BTreeMap::new();
        let mut warnings = Vec::new();
        for (relative, hash) in &desired {
            let actual = file_hash(root, relative)?;
            let owned = actual.is_none()
                || actual.as_ref() == manifest.files.get(relative)
                || actual.as_ref() == previous_pending.get(relative)
                || actual.as_ref() == manifest.originals.get(relative);
            if owned {
                allowed.insert(relative.clone(), hash.clone());
            } else {
                warnings.push(format!("{relative} was kept because it was added or edited outside Desegnos. Move it aside, then save again to sync this page."));
            }
        }
        manifest.pending.extend(allowed.clone());
        write_manifest(root, &manifest)?;
        for (relative, hash) in &allowed {
            let source = website_export::checked_path(&stage, relative)?;
            let destination = website_export::checked_path(root, relative)?;
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(io)?;
            }
            // Recheck immediately before replacing; fresh names use no-replace publication.
            let actual = file_hash(root, relative)?;
            if actual.is_some()
                && actual.as_ref() != manifest.files.get(relative)
                && actual.as_ref() != previous_pending.get(relative)
                && actual.as_ref() != manifest.originals.get(relative)
            {
                return Err(Error(format!(
                    "{relative} changed while saving. Its bytes were preserved; save again to retry."
                )));
            }
            if actual.is_some() {
                fs::rename(source, &destination).map_err(io)?;
            } else {
                crate::commit_new_file(&source, &destination)?;
            }
            crate::sync_parent(&destination)?;
            manifest.files.insert(relative.clone(), hash.clone());
        }
        let stale: Vec<_> = manifest
            .files
            .keys()
            .chain(previous_pending.keys())
            .filter(|p| !desired.contains_key(*p))
            .cloned()
            .collect();
        for relative in stale {
            let actual = file_hash(root, &relative)?;
            if actual.is_some()
                && (actual.as_ref() == manifest.files.get(&relative)
                    || actual.as_ref() == previous_pending.get(&relative))
            {
                fs::remove_file(website_export::checked_path(root, &relative)?).map_err(io)?;
                crate::sync_parent(&root.join(&relative))?;
            } else if actual.is_some() {
                warnings.push(format!("{relative} is no longer part of this website, but was kept because it was edited outside Desegnos."));
            }
            manifest.files.remove(&relative);
        }
        manifest.pending.clear();
        // The backup grants overwrite authority for the first sync only. Restoring an
        // original file later is an external edit, even if its bytes match that backup.
        manifest.originals.clear();
        write_manifest(root, &manifest)?;
        Ok(warnings)
    })();
    let _ = fs::remove_dir_all(stage);
    result
}

/// Select only immediate archives; newest first, with deterministic tie breaking.
pub fn project_in_folder(folder: &Path) -> Result<Option<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(folder).map_err(io)? {
        let entry = entry.map_err(io)?;
        let metadata = entry.metadata().map_err(io)?;
        if entry.file_type().map_err(io)?.is_file() && crate::project_extension(&entry.path()) {
            paths.push((metadata.modified().map_err(io)?, entry.path()));
        }
    }
    paths.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(paths.into_iter().next().map(|(_, path)| path))
}

/// A bounded source snapshot, shared by import and the immutable original-site backup.
pub(crate) fn site_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
    entries: &mut usize,
    size: &mut u64,
    skipped: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(directory).map_err(io)? {
        let entry = entry.map_err(io)?;
        *entries += 1;
        if *entries > 10_000 {
            return Err(Error(
                "The website exceeds the 10,000-entry import limit.".into(),
            ));
        }
        let path = entry.path();
        let name = path
            .strip_prefix(root)
            .map_err(io)?
            .components()
            .map(|part| {
                part.as_os_str()
                    .to_str()
                    .ok_or_else(|| Error("The website contains a non-Unicode filename.".into()))
            })
            .collect::<Result<Vec<_>>>()?
            .join("/");
        if name.split('/').any(|part| {
            part.starts_with('.') || matches!(part, "node_modules" | "target" | "vendor")
        }) {
            if skipped.len() < 256 { skipped.push(name.clone()); }
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(io)?;
        if metadata.file_type().is_symlink() {
            if skipped.len() < 256 { skipped.push(name.clone()); }
            continue;
        }
        if metadata.is_dir() {
            if name.split('/').count() > 32 {
                return Err(Error("The website has too many nested folders.".into()));
            }
            site_files(root, &path, files, entries, size, skipped)?;
        } else if metadata.is_file() {
            let extension = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ![
                "html", "htm", "css", "js", "mjs", "png", "jpg", "jpeg", "webp", "gif", "svg",
                "ico", "avif", "woff", "woff2", "ttf", "otf", "json", "wasm", "xml", "webmanifest",
            ]
            .contains(&extension.as_str())
            {
                if skipped.len() < 256 { skipped.push(name.clone()); }
                continue;
            }
            if !valid_site_path(&name) {
                return Err(Error(format!("Unsupported website filename: {name}")));
            }
            *size += metadata.len();
            if *size > MAX_SITE_BYTES {
                return Err(Error(
                    "The website exceeds the 128 MiB import limit.".into(),
                ));
            }
            let canonical = path.canonicalize().map_err(io)?;
            if !canonical.starts_with(root) {
                return Err(Error("A website path changed during import.".into()));
            }
            let mut bytes = Vec::new();
            let mut source = crate::identity_source::open_unchanged(&path, &metadata)?;
            (&mut source)
                .take(metadata.len() + 1)
                .read_to_end(&mut bytes)
                .map_err(io)?;
            let after = source.metadata().map_err(io)?;
            if bytes.len() as u64 != metadata.len()
                || after.modified().ok() != metadata.modified().ok()
            {
                return Err(Error(
                    "A website file changed during import. Open it again.".into(),
                ));
            }
            files.insert(name, bytes);
        }
    }
    Ok(())
}

/// Read-only bounded snapshot, using exactly the same filtering as editable imports.
pub struct WebsiteSnapshot {
    pub files: BTreeMap<String, Vec<u8>>,
    pub skipped: Vec<String>,
}
pub fn read_website_files(folder: &Path) -> Result<WebsiteSnapshot> {
    let root = website_export::parent_directory(folder)?;
    let mut files = BTreeMap::new();
    let mut skipped = vec![];
    site_files(&root, &root, &mut files, &mut 0, &mut 0, &mut skipped)?;
    Ok(WebsiteSnapshot { files, skipped })
}

/// Import never overwrites the source: backup first, archive second; syncing waits for Save.
pub fn import_website_folder(folder: &Path) -> Result<ProjectFile> {
    let root = website_export::parent_directory(folder)?;
    if project_in_folder(&root)?.is_some() {
        return Err(Error(
            "This folder already contains a project. Open that project instead.".into(),
        ));
    }
    let name = sanitize_project_name(
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Website"),
    );
    let WebsiteSnapshot { files, skipped } = read_website_files(&root)?;
    let mut project = desegni_core::import_website_files(&name, &files)?;
    project.website.as_mut().expect("imported site").skipped.extend(skipped);
    let hashes: BTreeMap<_, _> = files.iter().map(|(p, b)| (p.clone(), digest(b))).collect();
    let backup = root.join(".desegnos-original");
    if fs::symlink_metadata(&backup).is_ok() {
        website_export::parent_directory(&backup)?;
        // An interrupted import may already have published the backup. Never replace it.
        for (p, hash) in &hashes {
            if file_hash(&backup, p)?.as_ref() != Some(hash) {
                return Err(Error("The existing .desegnos-original backup differs from this website. It has been preserved.".into()));
            }
        }
    } else {
        website_export::stage_files(&root, ".desegnos-original", files)?;
    }
    let path = root.join(format!("{name}.desegnos"));
    if fs::symlink_metadata(root.join(MANIFEST)).is_ok() {
        let previous = read_manifest(&root)?;
        if previous.project_file != format!("{name}.desegnos")
            || previous.originals != hashes
            || !previous.files.is_empty()
            || !previous.pending.is_empty()
        {
            return Err(Error(
                "An existing site manifest belongs to different work. Its files were preserved."
                    .into(),
            ));
        }
        // Resume an import interrupted after reserving the marker but before saving the archive.
        project.id = previous.project_id;
    }
    manage_project_folder(&path, project.id, hashes)?;
    let file = ProjectFile::acquire(&path)?;
    // Initial import commits only the document. The user's next Save syncs its website.
    file.save_archive(&project, false)
        .map_err(|e| Error(e.to_string()))?;
    Ok(file)
}
