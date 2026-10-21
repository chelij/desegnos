pub mod website_detection;
pub use website_detection::{WebsiteFolder, detect_website_folder, import_detected_website};
use desegni_core::{
    DecodedProject, Error, Project, Result, SCHEMA_VERSION, decode_project_with_notes,
    decode_project_with_source_hash, digest,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub mod identity_source;
pub mod project_folder;
pub use identity_source::{
    IdentitySource, IdentitySourceAsset, IdentitySourceFile, collect_identity_source,
    identity_source_from_files, identity_source_from_files_attributed,
};
pub use project_folder::{
    create_project_folder, import_website_folder, is_project_folder, manage_project_folder,
    project_in_folder, sanitize_project_name,
};

const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MODEL_BYTES: u64 = 128 * 1024 * 1024;
pub const PROJECT_FILE_EXTENSIONS: [&str; 2] = ["desegnos", "desegni"];

fn project_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| PROJECT_FILE_EXTENSIONS.contains(&extension))
}

fn io(error: impl std::fmt::Display) -> Error {
    Error(error.to_string())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    schema_version: u32,
    project_id: Uuid,
    revision: u64,
    files: BTreeMap<String, String>,
}

/// The identity and exact manifest bytes read from a validated archive, before migration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    pub project_id: Uuid,
    pub revision: u64,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveFailureKind {
    Changed,
    DifferentProject,
    Unreadable,
    InvalidProject,
    Io,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveFailure {
    pub kind: SaveFailureKind,
    pub message: String,
    pub expected: Option<FileStamp>,
    pub actual: Option<FileStamp>,
}

impl std::fmt::Display for SaveFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SaveFailure {}

impl SaveFailure {
    fn new(kind: SaveFailureKind, message: impl Into<String>) -> Box<Self> {
        Box::new(Self {
            kind,
            message: message.into(),
            expected: None,
            actual: None,
        })
    }
}

pub struct ProjectFile {
    path: PathBuf,
    _lock: Arc<File>,
    stamp: Mutex<Option<FileStamp>>,
    site_warnings: Mutex<Vec<String>>,
    managed: AtomicBool,
}

impl ProjectFile {
    pub fn acquire(path: &Path) -> Result<Self> {
        if !project_extension(path) {
            return Err(Error(
                "Project files must use .desegnos (or the older .desegni extension).".into(),
            ));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = parent.canonicalize().map_err(io)?;
        let name = path
            .file_name()
            .ok_or_else(|| Error("Choose a project filename.".into()))?;
        let requested = parent.join(name);
        let path = match fs::symlink_metadata(&requested) {
            Ok(_) => {
                let path = requested.canonicalize().map_err(io)?;
                let metadata = path.metadata().map_err(io)?;
                if !metadata.is_file() || !project_extension(&path) {
                    return Err(Error(
                        "Choose a regular .desegnos or .desegni project file.".into(),
                    ));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.nlink() > 1 {
                        return Err(Error("Hard-linked project files cannot be opened for writing. Save a separate copy first.".into()));
                    }
                }
                path
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => requested,
            Err(error) => return Err(io(error)),
        };
        let name = path
            .file_name()
            .ok_or_else(|| Error("Choose a project filename.".into()))?;
        let lock_path = path.with_file_name(format!("{}.lock", name.to_string_lossy()));
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(&lock_path).map_err(io)?;
        lock.try_lock().map_err(|_| {
            Error("This project is already open for writing in another window.".into())
        })?;
        let managed = project_folder::is_project_folder(&path);
        Ok(Self {
            path,
            _lock: Arc::new(lock),
            stamp: Mutex::new(None),
            site_warnings: Mutex::new(vec![]),
            managed: AtomicBool::new(managed),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn stamp(&self) -> Result<Option<FileStamp>> {
        Ok(self.stamp.lock().map_err(io)?.clone())
    }

    /// Ordinary saves only replace the exact validated archive this handle opened or saved.
    pub fn save(&self, project: &Project) -> Result<()> {
        self.save_checked(project)
            .map_err(|error| Error(error.to_string()))
    }

    pub fn save_checked(&self, project: &Project) -> std::result::Result<(), Box<SaveFailure>> {
        self.save_archive(project, true)
    }

    fn save_archive(
        &self,
        project: &Project,
        sync_site: bool,
    ) -> std::result::Result<(), Box<SaveFailure>> {
        let mut stamp = self
            .stamp
            .lock()
            .map_err(|error| SaveFailure::new(SaveFailureKind::Io, error.to_string()))?;
        let next = self.save_exact(project, stamp.as_ref(), false)?;
        *stamp = Some(next);
        if sync_site {
            self.sync_site(project);
        }
        Ok(())
    }

    /// Only call after an explicit user choice bound to this observed, validated stamp.
    /// A second change on disk invalidates that choice, including a same-revision change.
    pub fn save_replacing(
        &self,
        project: &Project,
        observed: &FileStamp,
        allow_different_project: bool,
    ) -> std::result::Result<(), Box<SaveFailure>> {
        let mut stamp = self
            .stamp
            .lock()
            .map_err(|error| SaveFailure::new(SaveFailureKind::Io, error.to_string()))?;
        let next = self.save_exact(project, Some(observed), allow_different_project)?;
        *stamp = Some(next);
        self.sync_site(project);
        Ok(())
    }

    fn sync_site(&self, project: &Project) {
        let warnings = match project_folder::sync_project_site(
            &self.path,
            project,
            self.is_project_folder(),
        ) {
            Ok(warnings) => warnings,
            Err(error) => vec![format!("Website files could not be synced: {error}")],
        };
        if project_folder::is_project_folder(&self.path) {
            self.managed.store(true, Ordering::Relaxed);
        }
        if let Ok(mut saved) = self.site_warnings.lock() {
            *saved = warnings;
        }
    }

    pub fn site_warnings(&self) -> Vec<String> {
        self.site_warnings
            .lock()
            .map(|warnings| warnings.clone())
            .unwrap_or_default()
    }

    /// Cached so ordinary edits and workspace polling never read the site manifest.
    pub fn is_project_folder(&self) -> bool {
        self.managed.load(Ordering::Relaxed)
    }

    fn check_disk(
        &self,
        project_id: Uuid,
        expected: Option<&FileStamp>,
        allow_different_project: bool,
    ) -> std::result::Result<(), Box<SaveFailure>> {
        let actual = match fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && expected.is_none() => {
                return Ok(());
            }
            Err(_) => {
                return Err(SaveFailure::new(
                    SaveFailureKind::Unreadable,
                    "This file cannot be read. Save a copy with a new name.",
                ));
            }
            Ok(metadata) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.nlink() > 1 {
                        return Err(SaveFailure::new(
                            SaveFailureKind::Unreadable,
                            "This file has another linked name. Save a separate copy.",
                        ));
                    }
                }
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(SaveFailure::new(
                        SaveFailureKind::Unreadable,
                        "This file cannot be safely replaced. Save a copy with a new name.",
                    ));
                }
                read_project_document(&self.path).map_err(|_| SaveFailure::new(SaveFailureKind::Unreadable, "This file is not a readable Desegnos project. Choose a new name ending in .desegnos."))?.1
            }
        };
        if actual.project_id != project_id && !allow_different_project {
            return Err(Box::new(SaveFailure {
                kind: SaveFailureKind::DifferentProject,
                message: "This file is a different project. Save a copy with a new name.".into(),
                expected: expected.cloned(),
                actual: Some(actual),
            }));
        }
        if expected != Some(&actual) {
            return Err(Box::new(SaveFailure {
                kind: SaveFailureKind::Changed,
                message: "This file changed since you opened it.".into(),
                expected: expected.cloned(),
                actual: Some(actual),
            }));
        }
        Ok(())
    }

    fn save_exact(
        &self,
        project: &Project,
        expected: Option<&FileStamp>,
        allow_different_project: bool,
    ) -> std::result::Result<FileStamp, Box<SaveFailure>> {
        self.check_disk(project.id, expected, allow_different_project)?;
        self.write_project(project, expected.is_some(), || {
            self.check_disk(project.id, expected, allow_different_project)
        })
    }

    fn write_project(
        &self,
        project: &Project,
        replace_existing: bool,
        check: impl Fn() -> std::result::Result<(), Box<SaveFailure>>,
    ) -> std::result::Result<FileStamp, Box<SaveFailure>> {
        let operation = || -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
            project.validate()?;
            let json = serde_json::to_vec(project).map_err(io)?;
            let markdown = project.project_markdown()?.into_bytes();
            if json.len() as u64 + markdown.len() as u64 > MAX_MODEL_BYTES {
                return Err(Error("The project exceeds the 128 MiB model limit.".into()));
            }
            let manifest = Manifest {
                format: "desegni".into(),
                schema_version: SCHEMA_VERSION,
                project_id: project.id,
                revision: project.revision,
                files: BTreeMap::from([
                    ("project.json".into(), digest(&json)),
                    ("PROJECT.md".into(), digest(&markdown)),
                ]),
            };
            let manifest = serde_json::to_vec(&manifest).map_err(io)?;
            if json.len() as u64 + markdown.len() as u64 + manifest.len() as u64 > MAX_MODEL_BYTES {
                return Err(Error("The project exceeds the 128 MiB model limit.".into()));
            }
            Ok((json, markdown, manifest))
        };
        let (json, markdown, manifest) = operation().map_err(|error| {
            SaveFailure::new(SaveFailureKind::InvalidProject, error.to_string())
        })?;
        let next = FileStamp {
            project_id: project.id,
            revision: project.revision,
            manifest_sha256: digest(&manifest),
        };
        atomic_write_checked(
            &self.path,
            replace_existing,
            |file| {
                let mut archive = ZipWriter::new(file);
                let options = SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(0o600);
                for (name, data) in [
                    ("manifest.json", manifest.as_slice()),
                    ("project.json", json.as_slice()),
                    ("PROJECT.md", markdown.as_slice()),
                ] {
                    archive.start_file(name, options).map_err(io)?;
                    archive.write_all(data).map_err(io)?;
                }
                archive.finish().map_err(io)?;
                Ok(())
            },
            check,
        )?;
        Ok(next)
    }

    pub fn load(&self) -> Result<Project> {
        Ok(self.load_document()?.project)
    }

    pub fn load_document(&self) -> Result<DecodedProject> {
        let mut stamp = self.stamp.lock().map_err(io)?;
        let (document, observed) = read_project_document(&self.path)?;
        *stamp = Some(observed);
        Ok(document)
    }

    /// Stage an explicit reopen without reacquiring our own lock or rebasing the active file.
    /// The old handle and its save baseline remain valid if workspace activation fails.
    pub fn reopen(&self) -> Result<(Self, DecodedProject)> {
        let file = Self {
            path: self.path.clone(),
            _lock: Arc::clone(&self._lock),
            stamp: Mutex::new(None),
            site_warnings: Mutex::new(vec![]),
            managed: AtomicBool::new(project_folder::is_project_folder(&self.path)),
        };
        let document = file.load_document()?;
        Ok((file, document))
    }

    /// Read the recorded saved snapshot without accepting an external change as a new baseline.
    pub fn saved_document(&self) -> Result<DecodedProject> {
        let stamp = self.stamp.lock().map_err(io)?;
        let (document, actual) = read_project_document(&self.path)?;
        if stamp.as_ref() != Some(&actual) {
            return Err(Error("The saved file changed before its recovery state could be recorded. The project remains unsaved.".into()));
        }
        Ok(document)
    }
}

pub fn load_project(path: &Path) -> Result<Project> {
    Ok(load_project_document(path)?.project)
}

pub fn load_project_document(path: &Path) -> Result<DecodedProject> {
    Ok(read_project_document(path)?.0)
}

fn read_project_document(path: &Path) -> Result<(DecodedProject, FileStamp)> {
    let file = File::open(path).map_err(io)?;
    if file.metadata().map_err(io)?.len() > MAX_ARCHIVE_BYTES {
        return Err(Error("The archive exceeds the 2 GiB input limit.".into()));
    }
    let mut archive = ZipArchive::new(file).map_err(io)?;
    if archive.len() != 3 {
        return Err(Error(
            "The project archive contains unexpected or missing entries.".into(),
        ));
    }
    let mut entries = BTreeMap::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io)?;
        let name = entry.name().to_owned();
        if !["manifest.json", "project.json", "PROJECT.md"].contains(&name.as_str())
            || entry.is_dir()
            || entry.encrypted()
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            || entries.contains_key(&name)
        {
            return Err(Error(
                "The archive contains an unsafe, duplicate, or unsupported entry.".into(),
            ));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| Error("Archive size overflow.".into()))?;
        if total > MAX_MODEL_BYTES {
            return Err(Error("The archive exceeds the 128 MiB model limit.".into()));
        }
        let mut data = Vec::new();
        let remaining = MAX_MODEL_BYTES
            - entries
                .values()
                .map(|data: &Vec<u8>| data.len() as u64)
                .sum::<u64>();
        Read::by_ref(&mut entry)
            .take(remaining + 1)
            .read_to_end(&mut data)
            .map_err(io)?;
        if data.len() as u64 != entry.size() {
            return Err(Error(
                "An archive entry has an invalid expanded size.".into(),
            ));
        }
        if data.len() as u64 > remaining {
            return Err(Error("The archive exceeds the 128 MiB model limit.".into()));
        }
        entries.insert(name, data);
    }
    let manifest: Manifest = serde_json::from_slice(&entries["manifest.json"]).map_err(io)?;
    if manifest.format != "desegni"
        || ![1, 2, 3, 4, 5, 6, 7, SCHEMA_VERSION].contains(&manifest.schema_version)
    {
        return Err(Error(
            "This project format is unsupported. The original has not been changed.".into(),
        ));
    }
    if manifest.files.len() != 2 {
        return Err(Error("The project manifest is incomplete.".into()));
    }
    for name in ["project.json", "PROJECT.md"] {
        if manifest.files.get(name) != Some(&digest(&entries[name])) {
            return Err(Error(format!("{name} failed its integrity check.")));
        }
    }
    let document = decode_project_with_notes(&entries["project.json"], &entries["PROJECT.md"])?;
    if document.source_schema_version != manifest.schema_version
        || document.project.id != manifest.project_id
        || document.project.revision != manifest.revision
    {
        return Err(Error("The manifest and project disagree.".into()));
    }
    let stamp = FileStamp {
        project_id: manifest.project_id,
        revision: manifest.revision,
        manifest_sha256: digest(&entries["manifest.json"]),
    };
    Ok((document, stamp))
}

pub struct Journal {
    path: PathBuf,
    project_id: Uuid,
    last_revision: Mutex<Option<u64>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalEntry {
    sha256: String,
    project: Project,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredJournalEntry<'a> {
    sha256: String,
    #[serde(borrow)]
    project: &'a serde_json::value::RawValue,
}

impl Journal {
    pub fn new(directory: &Path, project_id: Uuid) -> Result<Self> {
        fs::create_dir_all(directory).map_err(io)?;
        Ok(Self {
            path: directory.join(format!("{project_id}-{}.journal", Uuid::new_v4())),
            project_id,
            last_revision: Mutex::new(None),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, project: &Project) -> Result<()> {
        project.validate()?;
        if project.id != self.project_id {
            return Err(Error(
                "This recovery journal belongs to another project.".into(),
            ));
        }
        let mut last_revision = self.last_revision.lock().map_err(io)?;
        if last_revision.is_some_and(|revision| project.revision <= revision) {
            return Err(Error(
                "Recovery transactions must advance the project revision.".into(),
            ));
        }
        let hash = digest(&serde_json::to_vec(project).map_err(io)?);
        let entry = JournalEntry {
            sha256: hash,
            project: project.clone(),
        };
        let mut bytes = serde_json::to_vec(&entry).map_err(io)?;
        if bytes.len() as u64 > MAX_MODEL_BYTES {
            return Err(Error(
                "The recovery entry exceeds the model size limit.".into(),
            ));
        }
        bytes.push(b'\n');
        // Each record is a full recovery snapshot, so atomic replacement avoids a partial tail poisoning a retry.
        atomic_write(&self.path, |file| file.write_all(&bytes).map_err(io), false)?;
        *last_revision = Some(project.revision);
        Ok(())
    }

    pub fn recover(path: &Path) -> Result<Option<Project>> {
        Ok(Self::recover_document(path)?.map(|document| document.project))
    }

    pub fn recover_document(path: &Path) -> Result<Option<DecodedProject>> {
        let file = File::open(path).map_err(io)?;
        if file.metadata().map_err(io)?.len() > MAX_MODEL_BYTES * 3 {
            return Err(Error(
                "The recovery journal exceeds the supported size.".into(),
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_MODEL_BYTES * 3 + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 > MAX_MODEL_BYTES * 3 {
            return Err(Error(
                "The recovery journal exceeds the supported size.".into(),
            ));
        }
        let mut previous: Option<DecodedProject> = None;
        for line in bytes.split_inclusive(|b| *b == b'\n') {
            if !line.ends_with(b"\n") {
                break;
            }
            if line.len() as u64 > MAX_MODEL_BYTES + 1 {
                return Err(Error(
                    "A recovery entry exceeds the model size limit.".into(),
                ));
            }
            let entry: StoredJournalEntry<'_> = serde_json::from_slice(line)
                .map_err(|e| Error(format!("A recovery entry is damaged: {e}")))?;
            let document =
                decode_project_with_source_hash(entry.project.get().as_bytes(), &entry.sha256)?;
            if let Some(old) = &previous
                && (old.project.id != document.project.id
                    || old.project.revision >= document.project.revision)
            {
                return Err(Error("Recovery revisions are inconsistent.".into()));
            }
            previous = Some(document);
        }
        Ok(previous)
    }
}

pub fn write_markdown(path: &Path, markdown: &str) -> Result<()> {
    atomic_write(
        path,
        |file| file.write_all(markdown.as_bytes()).map_err(io),
        true,
    )
}

fn atomic_write_checked(
    path: &Path,
    replace_existing: bool,
    write: impl FnOnce(&mut File) -> Result<()>,
    check: impl Fn() -> std::result::Result<(), Box<SaveFailure>>,
) -> std::result::Result<(), Box<SaveFailure>> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = parent.join(format!(".desegni-{}.tmp", Uuid::new_v4()));
    let io_failure = |error: Error| SaveFailure::new(SaveFailureKind::Io, error.to_string());
    let operation = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(io).map_err(io_failure)?;
        write(&mut file).map_err(io_failure)?;
        file.sync_all().map_err(io).map_err(io_failure)?;
        drop(file);
        check()?;
        if path.exists() {
            backup_file(path).map_err(io_failure)?;
        }
        check()?;
        let committed = if replace_existing {
            fs::rename(&temporary, path).map_err(io)
        } else {
            commit_new_file(&temporary, path)
        };
        if let Err(error) = committed {
            // In particular, a new destination may appear after the last check.
            check()?;
            return Err(io_failure(error));
        }
        sync_parent(path).map_err(io_failure)
    })();
    if operation.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    operation
}

fn commit_new_file(temporary: &Path, path: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        renameat_with(CWD, temporary, CWD, path, RenameFlags::NOREPLACE).map_err(io)?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Creating a hard link is atomic and refuses an existing destination.
        // Both paths are in the same directory/filesystem, as required above.
        fs::hard_link(temporary, path).map_err(io)?;
        fs::remove_file(temporary).map_err(io)?;
    }
    Ok(())
}

fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut File) -> Result<()>,
    backup: bool,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = parent.join(format!(".desegni-{}.tmp", Uuid::new_v4()));
    let operation = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(io)?;
        write(&mut file)?;
        file.sync_all().map_err(io)?;
        drop(file);
        if backup && path.exists() {
            backup_file(path)?;
        }
        fs::rename(&temporary, path).map_err(io)?;
        sync_parent(path)
    })();
    if operation.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    operation
}

fn backup_file(path: &Path) -> Result<()> {
    let previous = path.with_file_name(format!(
        "{}.previous",
        path.file_name()
            .ok_or_else(|| Error("Invalid filename.".into()))?
            .to_string_lossy()
    ));
    atomic_write(
        &previous,
        |backup| {
            std::io::copy(&mut File::open(path).map_err(io)?, backup).map_err(io)?;
            Ok(())
        },
        false,
    )
}

fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    )
    .map_err(io)?
    .sync_all()
    .map_err(io)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub mod website_export;
pub use website_export::{WrittenWebsiteExport, write_website_export};

#[cfg(test)]
mod safe_write_tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_new_destination_appearing_after_the_last_check_is_not_overwritten() {
        let directory =
            std::env::temp_dir().join(format!("desegnos-new-file-race-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("site.desegnos");
        let checks = Cell::new(0);
        let result = atomic_write_checked(
            &path,
            false,
            |temporary| temporary.write_all(b"my project").map_err(io),
            || {
                checks.set(checks.get() + 1);
                if checks.get() == 2 {
                    fs::write(&path, b"new external document").unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"new external document");
        assert!(!path.with_file_name("site.desegnos.previous").exists());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_change_during_compression_is_checked_before_the_backup_or_rename() {
        let directory =
            std::env::temp_dir().join(format!("desegnos-write-race-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("site.desegnos");
        fs::write(&path, b"original").unwrap();
        let result = atomic_write_checked(
            &path,
            true,
            |temporary| {
                temporary.write_all(b"my saved content").map_err(io)?;
                fs::write(&path, b"external change").map_err(io)?;
                Ok(())
            },
            || {
                if fs::read(&path).unwrap() != b"original" {
                    Err(SaveFailure::new(SaveFailureKind::Changed, "changed"))
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result.unwrap_err().kind, SaveFailureKind::Changed);
        assert_eq!(fs::read(&path).unwrap(), b"external change");
        assert!(!path.with_file_name("site.desegnos.previous").exists());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_change_during_backup_is_checked_again_before_the_rename() {
        let directory =
            std::env::temp_dir().join(format!("desegnos-write-race-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("site.desegnos");
        fs::write(&path, b"original").unwrap();
        let checks = Cell::new(0);
        let result = atomic_write_checked(
            &path,
            true,
            |temporary| temporary.write_all(b"my saved content").map_err(io),
            || {
                checks.set(checks.get() + 1);
                if checks.get() == 2 {
                    fs::write(&path, b"external change").unwrap();
                }
                if fs::read(&path).unwrap() != b"original" {
                    Err(SaveFailure::new(SaveFailureKind::Changed, "changed"))
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(checks.get(), 2);
        assert_eq!(result.unwrap_err().kind, SaveFailureKind::Changed);
        assert_eq!(fs::read(&path).unwrap(), b"external change");
        assert_eq!(
            fs::read(path.with_file_name("site.desegnos.previous")).unwrap(),
            b"original"
        );
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
        fs::remove_dir_all(directory).unwrap();
    }
}
