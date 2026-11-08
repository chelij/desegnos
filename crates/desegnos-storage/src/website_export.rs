//! Bounded, staged website output. Destination names are always host-generated.
use desegnos_core::{Error, Result, WebsiteExport, digest};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

#[derive(Debug)]
pub struct WrittenWebsiteExport {
    pub path: PathBuf,
    pub page_count: usize,
    pub file_count: usize,
    pub warnings: Vec<String>,
    hashes: BTreeMap<String, String>,
    #[cfg(unix)]
    directory_identity: (u64, u64),
}
impl WrittenWebsiteExport {
    /// Re-check the actual output before granting native folder-opening authority.
    pub fn verify(&self) -> Result<()> {
        let metadata = fs::symlink_metadata(&self.path).map_err(io)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || self.path.canonicalize().map_err(io)? != self.path
        {
            return Err(Error(
                "The export folder was moved or replaced. Export the website again.".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if (metadata.dev(), metadata.ino()) != self.directory_identity {
                return Err(Error(
                    "The export folder was replaced. Export the website again.".into(),
                ));
            }
        }
        for (relative, expected) in &self.hashes {
            let path = checked_path(&self.path, relative)?;
            let metadata = fs::symlink_metadata(&path).map_err(io)?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > MAX_BYTES as u64
            {
                return Err(Error(
                    "An exported file was replaced. Export the website again.".into(),
                ));
            }
            let mut input = crate::source_files::open_unchanged(&path, &metadata)?;
            let bytes =
                crate::source_files::read_exact_bounded(&mut input, metadata.len(), MAX_BYTES)?;
            if digest(&bytes) != *expected {
                return Err(Error("The exported files have changed. Export the website again before opening them here.".into()));
            }
        }
        Ok(())
    }
}
const MAX_BYTES: usize = desegnos_core::limits::MAX_EXPORT_BYTES;
fn io(error: impl std::fmt::Display) -> Error {
    Error(format!("The website export could not be written: {error}"))
}
pub(crate) fn validate(export: &WebsiteExport) -> Result<()> {
    if export.files.is_empty()
        || export.files.len() > desegnos_core::limits::MAX_EXPORT_FILES
        || export.files.values().map(Vec::len).sum::<usize>() > MAX_BYTES
    {
        return Err(Error(
            "The compiled website is empty, incomplete or too large to export.".into(),
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    for path in export.files.keys() {
        if crate::source_files::safe_archive_name(path).as_deref() != Some(path.as_str())
            || !names.insert(path.to_lowercase())
        {
            return Err(Error(
                "The compiled website contains an unsafe or duplicate output path.".into(),
            ));
        }
    }
    Ok(())
}
pub(crate) fn checked_path(root: &Path, relative: &str) -> Result<PathBuf> {
    if crate::source_files::safe_archive_name(relative).as_deref() != Some(relative) {
        return Err(Error("Unsafe export path.".into()));
    }
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(name) = component else {
            return Err(Error("Unsafe export path.".into()));
        };
        path.push(name);
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && metadata.file_type().is_symlink()
        {
            return Err(Error(
                "An exported path was replaced by a symbolic link.".into(),
            ));
        }
    }
    Ok(path)
}
/// Read one file back from an export folder, for pointer clash checks. The path must be a safe
/// relative name that resolves to an ordinary file inside the folder.
fn read_export_file(root: &Path, relative: &str) -> Result<Vec<u8>> {
    let path = checked_path(root, relative)?;
    let metadata = fs::symlink_metadata(&path).map_err(io)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_BYTES as u64
    {
        return Err(Error(
            "An exported file is missing, replaced or too large to check.".into(),
        ));
    }
    let mut input = crate::source_files::open_unchanged(&path, &metadata)?;
    crate::source_files::read_exact_bounded(&mut input, metadata.len(), MAX_BYTES)
}

/// Every HTML, CSS and JavaScript file of an export folder, up to two folders deep, keyed by
/// relative path. A file that is unreadable, replaced or larger than `max_file_bytes` fails the
/// whole read, so a pointer clash check never reports code as missing because it was skipped.
pub fn read_export_folder(root: &Path, max_file_bytes: usize) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut paths = Vec::new();
    collect_text_files(root, "", &mut paths, 0)?;
    let mut files = BTreeMap::new();
    for path in paths {
        let bytes = read_export_file(root, &path)?;
        if bytes.len() > max_file_bytes {
            return Err(Error(format!(
                "The exported file {path} is too large to check."
            )));
        }
        files.insert(path, bytes);
    }
    Ok(files)
}

fn collect_text_files(
    root: &Path,
    prefix: &str,
    out: &mut Vec<String>,
    depth: usize,
) -> Result<()> {
    if depth > 2 {
        return Err(Error(
            "The export folder is too deeply nested to check.".into(),
        ));
    }
    let directory = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix)
    };
    let entries = fs::read_dir(&directory)
        .map_err(|error| Error(format!("The export folder could not be read: {error}")))?;
    for entry in entries {
        let entry = entry.map_err(io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.contains(['/', '\\', '\0']) {
            continue;
        }
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let metadata = fs::symlink_metadata(entry.path()).map_err(io)?;
        if metadata.file_type().is_symlink() {
            return Err(Error("The export folder contains a symbolic link.".into()));
        }
        if metadata.is_dir() {
            collect_text_files(root, &relative, out, depth + 1)?;
        } else if metadata.is_file()
            && matches!(
                name.rsplit('.').next().unwrap_or_default(),
                "html" | "css" | "js"
            )
        {
            out.push(relative);
        }
    }
    Ok(())
}

/// Write an export into an existing private folder, used by preview exports. Every path is
/// created fresh; an existing file or symbolic link is never replaced.
pub fn write_export_into(root: &Path, export: &WebsiteExport) -> Result<()> {
    validate(export)?;
    let metadata = fs::symlink_metadata(root).map_err(io)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error("Choose a real folder for this export.".into()));
    }
    for (relative, bytes) in &export.files {
        let path = checked_path(root, relative)?;
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory).map_err(io)?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(io)?;
        file.write_all(bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
    }
    Ok(())
}

pub(crate) fn parent_directory(parent: &Path) -> Result<PathBuf> {
    if !parent.is_absolute()
        || parent
            .components()
            .any(|p| matches!(p, Component::ParentDir))
    {
        return Err(Error("Choose an absolute destination folder.".into()));
    }
    for ancestor in parent.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(io)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error(
                "Choose a real destination folder rather than a symbolic link.".into(),
            ));
        }
    }
    parent.canonicalize().map_err(io)
}
fn export_folder_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    let clean = clean.trim_matches('-');
    if clean.is_empty() { "desegnos" } else { clean }.to_owned()
}

/// Choose the first free readable name and rebuild destination-dependent files on a race.
pub fn write_export_named(
    parent: &Path,
    name: &str,
    mut build: impl FnMut(&Path) -> Result<WebsiteExport>,
) -> Result<WrittenWebsiteExport> {
    let parent = parent_directory(parent)?;
    let base = export_folder_name(name);
    let mut index = 0_u64;
    loop {
        index = index
            .checked_add(1)
            .ok_or_else(|| Error("Too many exports share this name.".into()))?;
        let name = if index == 1 {
            base.clone()
        } else {
            format!("{base}-{index}")
        };
        let path = parent.join(&name);
        match fs::symlink_metadata(&path) {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io(error)),
        }
        let export = build(&path)?;
        match write_export_at(&parent, &name, &export) {
            Ok(None) => continue,
            Ok(Some(written)) => return Ok(written),
            Err(error) => return Err(error),
        }
    }
}

/// Write an export into a specific new folder name. The exporter derives the final folder before
/// writing, so the manifest inside it can name its own destination.
fn write_export_at(
    parent: &Path,
    name: &str,
    export: &WebsiteExport,
) -> Result<Option<WrittenWebsiteExport>> {
    validate(export)?;
    if name.is_empty()
        || name.len() > 128
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '\0'])
        || name.ends_with(['.', ' '])
    {
        return Err(Error(
            "An export folder name must be a safe, single name.".into(),
        ));
    }
    let parent = parent_directory(parent)?;
    let name = name.to_owned();
    let stage_name = format!(".desegnos-export-{}", Uuid::new_v4());
    if !write_at(&parent, &stage_name, &name, export)? {
        return Ok(None);
    }
    let path = parent.join(name);
    let metadata = fs::symlink_metadata(&path).map_err(io)?;
    let written = WrittenWebsiteExport {
        path,
        page_count: export.page_count,
        file_count: export.files.len(),
        warnings: export.warnings.clone(),
        hashes: export
            .files
            .iter()
            .map(|(p, b)| (p.clone(), digest(b)))
            .collect(),
        #[cfg(unix)]
        directory_identity: {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino())
        },
    };
    written.verify()?;
    Ok(Some(written))
}

#[cfg(unix)]
fn write_at(
    parent_path: &Path,
    stage_name: &str,
    name: &str,
    export: &WebsiteExport,
) -> Result<bool> {
    use rustix::fs::{
        AtFlags, Mode, OFlags, RenameFlags, mkdirat, open, openat, renameat_with, unlinkat,
    };
    let parent = open(
        parent_path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(io)?;
    mkdirat(&parent, stage_name, Mode::RWXU).map_err(io)?;
    let stage = match openat(
        &parent,
        stage_name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(stage) => stage,
        Err(error) => {
            if let Err(cleanup) = unlinkat(&parent, stage_name, AtFlags::REMOVEDIR) {
                return Err(Error(format!(
                    "{error}; export staging cleanup failed: {cleanup}"
                )));
            }
            return Err(io(error));
        }
    };
    let mut created_files = Vec::new();
    let mut directories = std::collections::BTreeSet::new();
    let mut published = false;
    let result = (|| {
        for (relative, bytes) in &export.files {
            let components: Vec<_> = relative.split('/').collect();
            let mut directory = stage.try_clone().map_err(io)?;
            let mut prefix = String::new();
            for component in &components[..components.len() - 1] {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(component);
                match mkdirat(&directory, *component, Mode::RWXU) {
                    Ok(()) => {
                        directories.insert(prefix.clone());
                    }
                    Err(rustix::io::Errno::EXIST) => {}
                    Err(e) => return Err(io(e)),
                }
                directory = openat(
                    &directory,
                    *component,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                    Mode::empty(),
                )
                .map_err(io)?;
            }
            let descriptor = openat(
                &directory,
                components[components.len() - 1],
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(io)?;
            created_files.push(relative.clone());
            let mut file = File::from(descriptor);
            file.write_all(bytes).map_err(io)?;
            file.sync_all().map_err(io)?;
            File::from(directory).sync_all().map_err(io)?;
        }
        File::from(stage.try_clone().map_err(io)?)
            .sync_all()
            .map_err(io)?;
        // Absolute identity is checked before publication, while every actual
        // write and rename remains anchored to the held parent descriptor.
        let current = open(
            parent_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(io)?;
        let before = rustix::fs::fstat(&parent).map_err(io)?;
        let now = rustix::fs::fstat(&current).map_err(io)?;
        if before.st_dev != now.st_dev || before.st_ino != now.st_ino {
            return Err(Error(
                "The destination folder changed while exporting. Choose it again.".into(),
            ));
        }
        match renameat_with(&parent, stage_name, &parent, name, RenameFlags::NOREPLACE) {
            Ok(()) => {}
            Err(error)
                if error == rustix::io::Errno::EXIST || error == rustix::io::Errno::NOTEMPTY =>
            {
                return Ok(false);
            }
            Err(error) => return Err(io(error)),
        }
        published = true;
        parent.try_clone().map(File::from).and_then(|file| file.sync_all())
            .map_err(|error| Error(format!("The complete website was written to {}, but its destination could not confirm durable storage: {error}", parent_path.join(name).display())))?;
        Ok(true)
    })();
    let mut cleanup_errors = Vec::new();
    if !published {
        // Cleanup uses descriptor-relative paths and never follows a replaced
        // parent or a symbolic link. Only entries created in our private stage.
        for path in created_files.iter().rev() {
            if let Err(error) = unlinkat(&stage, path.as_str(), AtFlags::empty()) {
                cleanup_errors.push(error.to_string());
            }
        }
        for path in directories.iter().rev() {
            if let Err(error) = unlinkat(&stage, path.as_str(), AtFlags::REMOVEDIR) {
                cleanup_errors.push(error.to_string());
            }
        }
        if let Err(error) = unlinkat(&parent, stage_name, AtFlags::REMOVEDIR) {
            cleanup_errors.push(error.to_string());
        }
    }
    if !cleanup_errors.is_empty() {
        return Err(Error(format!(
            "Export staging cleanup failed: {}",
            cleanup_errors.join("; ")
        )));
    }
    result
}

#[cfg(not(unix))]
fn write_at(parent: &Path, stage_name: &str, name: &str, export: &WebsiteExport) -> Result<bool> {
    // Windows directory rename refuses an existing destination. Fresh private
    // staging paths and create_new prevent replacement of any authored file.
    use std::fs::OpenOptions;
    let stage = parent.join(stage_name);
    fs::create_dir(&stage).map_err(io)?;
    let result = (|| {
        for (relative, bytes) in &export.files {
            let path = checked_path(&stage, relative)?;
            if let Some(directory) = path.parent() {
                fs::create_dir_all(directory).map_err(io)?;
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(io)?;
            file.write_all(bytes).map_err(io)?;
            file.sync_all().map_err(io)?;
        }
        match fs::rename(&stage, parent.join(name)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(io(error)),
        }
    })();
    if !matches!(result, Ok(true)) {
        if let Err(cleanup) = fs::remove_dir_all(stage) {
            return Err(Error(format!(
                "{}; export staging cleanup failed: {cleanup}",
                result
                    .as_ref()
                    .err()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "The export destination already exists".into())
            )));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> PathBuf {
        let root = std::env::temp_dir().join(format!("desegnos-website-export-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        root
    }
    fn website() -> WebsiteExport {
        WebsiteExport {
            files: BTreeMap::from([
                ("index.html".into(), b"<h1>Home</h1>".to_vec()),
                ("team/index.html".into(), b"<h1>Team</h1>".to_vec()),
                ("assets/image.png".into(), vec![1, 2, 3]),
            ]),
            page_count: 2,
            warnings: vec!["Actual warning".into()],
        }
    }
    #[test]
    fn website_export_writer_commits_exact_files_and_repeated_exports_keep_existing_data() {
        let root = directory();
        fs::write(root.join("keep.txt"), b"original").unwrap();
        let first = write_export_named(&root, "My site", |_| Ok(website())).unwrap();
        let second = write_export_named(&root, "My site", |_| Ok(website())).unwrap();
        assert_eq!(first.path, root.join("my-site"));
        assert_eq!(second.path, root.join("my-site-2"));
        let third = write_export_named(&root, "My site", |_| Ok(website())).unwrap();
        assert_eq!(third.path, root.join("my-site-3"));
        assert_eq!(fs::read(root.join("keep.txt")).unwrap(), b"original");
        assert_eq!(first.page_count, 2);
        assert_eq!(first.file_count, 3);
        first.verify().unwrap();
        second.verify().unwrap();
        for (name, bytes) in website().files {
            assert_eq!(fs::read(first.path.join(name)).unwrap(), bytes);
        }
        assert!(!fs::read_dir(&root).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".desegnos")
        }));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn website_export_readable_name_retries_publication_race_and_uses_first_gap() {
        let root = directory();
        fs::create_dir(root.join("my-site-3")).unwrap();
        let mut attempts = 0;
        let written = write_export_named(&root, "My site", |path| {
            attempts += 1;
            if attempts == 1 {
                fs::create_dir(path).unwrap();
            }
            let mut export = website();
            export.files.insert(
                "destination.txt".into(),
                path.to_string_lossy().as_bytes().to_vec(),
            );
            Ok(export)
        })
        .unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(written.path, root.join("my-site-2"));
        assert_eq!(
            fs::read(written.path.join("destination.txt")).unwrap(),
            written.path.to_string_lossy().as_bytes()
        );
        fs::remove_dir_all(&written.path).unwrap();
        let next = write_export_named(&root, "My site", |_| Ok(website())).unwrap();
        assert_eq!(next.path, root.join("my-site-2"));
        assert_eq!(export_folder_name("???"), "desegnos");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn website_export_writer_rejects_unsafe_empty_and_duplicate_paths_before_creating_anything() {
        let root = directory();
        for path in [
            "../escape",
            "/absolute",
            "assets/../../escape",
            "assets\\escape",
            "C:escape",
            "./escape",
            "empty//name",
        ] {
            let mut export = website();
            export.files.insert(path.into(), vec![0]);
            assert!(
                write_export_named(&root, "bad", |_| Ok(export.clone())).is_err(),
                "{path}"
            );
        }
        let mut export = website();
        export.files.insert("INDEX.HTML".into(), vec![0]);
        assert!(write_export_named(&root, "bad", |_| Ok(export.clone())).is_err());
        export.files.clear();
        assert!(write_export_named(&root, "bad", |_| Ok(export.clone())).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn website_export_writer_cleans_partial_stage_after_real_write_failure() {
        let root = directory();
        let mut export = website();
        export.files.insert("index.html/child".into(), vec![0]);
        assert!(write_export_named(&root, "bad", |_| Ok(export.clone())).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn website_export_receipt_rejects_modified_or_replaced_files() {
        let root = directory();
        let written = write_export_named(&root, "site", |_| Ok(website())).unwrap();
        fs::write(written.path.join("index.html"), b"changed").unwrap();
        assert!(written.verify().is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn website_export_writer_rejects_symlink_parent_and_receipt_symlink_resources() {
        use std::os::unix::fs::symlink;
        let root = directory();
        let real = root.join("real");
        fs::create_dir(&real).unwrap();
        let link = root.join("link");
        symlink(&real, &link).unwrap();
        assert!(write_export_named(&link, "bad", |_| Ok(website())).is_err());
        assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
        let written = write_export_named(&real, "site", |_| Ok(website())).unwrap();
        let index = written.path.join("index.html");
        fs::remove_file(&index).unwrap();
        symlink(root.join("private"), index).unwrap();
        assert!(written.verify().is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn website_export_atomic_publication_refuses_existing_empty_or_populated_destination() {
        let root = directory();
        fs::create_dir(root.join("existing")).unwrap();
        assert!(!write_at(&root, "stage-one", "existing", &website()).unwrap());
        assert!(root.join("existing").is_dir());
        assert!(!root.join("stage-one").exists());
        fs::write(root.join("existing/keep"), b"original").unwrap();
        assert!(!write_at(&root, "stage-two", "existing", &website()).unwrap());
        assert_eq!(fs::read(root.join("existing/keep")).unwrap(), b"original");
        assert!(!root.join("stage-two").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn website_export_atomic_publication_refuses_symlink_destination() {
        use std::os::unix::fs::symlink;
        let root = directory();
        let external = root.join("external");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("keep"), b"safe").unwrap();
        symlink(&external, root.join("existing")).unwrap();
        assert!(!write_at(&root, "stage", "existing", &website()).unwrap());
        assert_eq!(fs::read(external.join("keep")).unwrap(), b"safe");
        assert!(!root.join("stage").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
