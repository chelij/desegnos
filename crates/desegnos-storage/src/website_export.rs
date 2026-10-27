//! Bounded, staged website output. Destination names are always host-generated.
use desegnos_core::{Error, Result, WebsiteExport, digest};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
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
            let mut bytes = Vec::new();
            File::open(path)
                .map_err(io)?
                .take(MAX_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(io)?;
            if digest(&bytes) != *expected {
                return Err(Error("The exported files have changed. Export the website again before opening them here.".into()));
            }
        }
        Ok(())
    }
}
const MAX_BYTES: usize = 128 * 1024 * 1024;
fn io(error: impl std::fmt::Display) -> Error {
    Error(format!("The website export could not be written: {error}"))
}
pub(crate) fn validate(export: &WebsiteExport) -> Result<()> {
    if export.files.is_empty()
        || export.files.len() > 10_000
        || export.files.values().map(Vec::len).sum::<usize>() > MAX_BYTES
    {
        return Err(Error(
            "The compiled website is empty, incomplete or too large to export.".into(),
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    for path in export.files.keys() {
        if path.is_empty()
            || path.contains(['\\', ':', '\0'])
            || Path::new(path)
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
            || path
                .split('/')
                .any(|s| s.is_empty() || s.ends_with(['.', ' ']))
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
pub fn read_export_file(root: &Path, relative: &str) -> Result<Vec<u8>> {
    let path = checked_path(root, relative)?;
    let metadata = fs::symlink_metadata(&path).map_err(io)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_BYTES as u64
    {
        return Err(Error(
            "An exported file is missing, replaced or too large to check.".into(),
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    Ok(bytes)
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
/// A new export folder name for this project, unique and safe on every platform.
pub fn export_folder_name(name: &str) -> String {
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
    format!(
        "{}-website-{}",
        if clean.is_empty() { "desegnos" } else { clean },
        Uuid::new_v4()
    )
}

/// A new folder is published only when every file is closed and synced. Existing
/// files or folders are never selected for replacement, even when empty.
pub fn write_website_export(
    parent: &Path,
    name: &str,
    export: &WebsiteExport,
) -> Result<WrittenWebsiteExport> {
    write_export_at(parent, &export_folder_name(name), export)
}

/// Write an export into a specific new folder name. The exporter derives the final folder before
/// writing, so the manifest inside it can name its own destination.
pub fn write_export_at(
    parent: &Path,
    name: &str,
    export: &WebsiteExport,
) -> Result<WrittenWebsiteExport> {
    validate(export)?;
    if name.is_empty()
        || name.len() > 128
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '\0'])
        || name.ends_with(['.', ' '])
    {
        return Err(Error("An export folder name must be a safe, single name.".into()));
    }
    let parent = parent_directory(parent)?;
    let name = name.to_owned();
    let stage_name = format!(".desegnos-export-{}", Uuid::new_v4());
    write_at(&parent, &stage_name, &name, export)?;
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
    Ok(written)
}

#[cfg(unix)]
fn write_at(
    parent_path: &Path,
    stage_name: &str,
    name: &str,
    export: &WebsiteExport,
) -> Result<()> {
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
            let _ = unlinkat(&parent, stage_name, AtFlags::REMOVEDIR);
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
        renameat_with(&parent, stage_name, &parent, name, RenameFlags::NOREPLACE).map_err(io)?;
        published = true;
        parent.try_clone().map(File::from).and_then(|file| file.sync_all())
            .map_err(|error| Error(format!("The complete website was written to {}, but its destination could not confirm durable storage: {error}", parent_path.join(name).display())))?;
        Ok(())
    })();
    if result.is_err() && !published {
        // Cleanup uses descriptor-relative paths and never follows a replaced
        // parent or a symbolic link. Only entries created in our private stage.
        for path in created_files.iter().rev() {
            let _ = unlinkat(&stage, path.as_str(), AtFlags::empty());
        }
        for path in directories.iter().rev() {
            let _ = unlinkat(&stage, path.as_str(), AtFlags::REMOVEDIR);
        }
        let _ = unlinkat(&parent, stage_name, AtFlags::REMOVEDIR);
    }
    result
}

#[cfg(not(unix))]
fn write_at(parent: &Path, stage_name: &str, name: &str, export: &WebsiteExport) -> Result<()> {
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
        fs::rename(&stage, parent.join(name)).map_err(io)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(stage);
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
        let first = write_website_export(&root, "My site", &website()).unwrap();
        let second = write_website_export(&root, "My site", &website()).unwrap();
        assert_ne!(first.path, second.path);
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
                write_website_export(&root, "bad", &export).is_err(),
                "{path}"
            );
        }
        let mut export = website();
        export.files.insert("INDEX.HTML".into(), vec![0]);
        assert!(write_website_export(&root, "bad", &export).is_err());
        export.files.clear();
        assert!(write_website_export(&root, "bad", &export).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn website_export_writer_cleans_partial_stage_after_real_write_failure() {
        let root = directory();
        let mut export = website();
        export.files.insert("index.html/child".into(), vec![0]);
        assert!(write_website_export(&root, "bad", &export).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn website_export_receipt_rejects_modified_or_replaced_files() {
        let root = directory();
        let written = write_website_export(&root, "site", &website()).unwrap();
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
        assert!(write_website_export(&link, "bad", &website()).is_err());
        assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
        let written = write_website_export(&real, "site", &website()).unwrap();
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
        assert!(write_at(&root, "stage-one", "existing", &website()).is_err());
        assert!(root.join("existing").is_dir());
        assert!(!root.join("stage-one").exists());
        fs::write(root.join("existing/keep"), b"original").unwrap();
        assert!(write_at(&root, "stage-two", "existing", &website()).is_err());
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
        assert!(write_at(&root, "stage", "existing", &website()).is_err());
        assert_eq!(fs::read(external.join("keep")).unwrap(), b"safe");
        assert!(!root.join("stage").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
