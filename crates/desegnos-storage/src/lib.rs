pub mod request_zip;
pub mod system_tools;
pub mod website_detection;
pub mod website_merge;
use desegnos_core::{
    DecodedProject, Error, Project, Result, SCHEMA_VERSION, decode_project,
    decode_project_with_notes, decode_project_with_source_hash, digest,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime},
};
use uuid::Uuid;
pub use website_detection::{WebsiteFolder, detect_website_folder, import_detected_website};
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
/// A journal written moments ago may belong to a session that has not recorded its pointer yet.
const JOURNAL_GRACE: Duration = Duration::from_secs(10 * 60);
/// Another instance writes site files just before the journal entry that references them.
const BLOB_GRACE: Duration = Duration::from_secs(60 * 60);
/// Writing site files is I/O bound, so more workers than this would only add contention.
const BLOB_WORKERS: usize = 8;
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
    outside_edits: Mutex<project_folder::OutsideEdits>,
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
            outside_edits: Mutex::new(Default::default()),
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
        let _ = self.sync_site_mode(project, false);
    }

    pub fn overwrite_site(&self, project: &Project) -> Result<()> {
        self.sync_site_mode(project, true)
    }

    fn sync_site_mode(&self, project: &Project, overwrite: bool) -> Result<()> {
        let result = project_folder::sync_project_site(
            &self.path,
            project,
            self.is_project_folder(),
            overwrite,
        );
        let warnings = match &result {
            Ok(outside) => {
                *self.outside_edits.lock().map_err(io)? = outside.clone();
                vec![]
            }
            Err(error) => vec![format!("Website files could not be synced: {error}")],
        };
        if project_folder::is_project_folder(&self.path) {
            self.managed.store(true, Ordering::Relaxed);
        }
        if let Ok(mut saved) = self.site_warnings.lock() {
            *saved = warnings;
        }
        result.map(|_| ())
    }

    pub fn outside_edits(&self) -> project_folder::OutsideEdits {
        self.outside_edits
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    pub fn mark_imported_site_owned(&self, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
        project_folder::mark_imported_site_owned(&self.path, files)?;
        *self.outside_edits.lock().map_err(io)? = Default::default();
        Ok(())
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
                format: "desegnos".into(),
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
            outside_edits: Mutex::new(Default::default()),
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
    if !matches!(manifest.format.as_str(), "desegnos" | "desegni")
        || ![1, 2, 3, 4, 5, 6, 7, 8, 9, SCHEMA_VERSION].contains(&manifest.schema_version)
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

/// A site file's digest cache key: the `Arc`'s thin pointer and its length.
type SiteFileKey = (usize, usize);
/// Digests of the current project's site files, with the `Arc` that keeps each key valid.
type SiteFileCache = HashMap<SiteFileKey, (Arc<str>, String)>;

pub struct Journal {
    path: PathBuf,
    project_id: Uuid,
    last_revision: Mutex<Option<u64>>,
    /// Imported site files are unchanged between keystroke edits, so remember each file's digest
    /// by `Arc` identity instead of re-hashing megabytes on every append. Keeping a clone of the
    /// `Arc` pins its address, so a freed allocation can never be mistaken for a cached file.
    blobs: Mutex<SiteFileCache>,
}

/// The serialized entry shape the journal writes by hand in `append`; tests keep it as the
/// reference for those bytes.
#[cfg(test)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalEntry {
    sha256: String,
    project: Project,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredJournalEntry<'a> {
    sha256: String,
    #[serde(borrow)]
    project: &'a serde_json::value::RawValue,
    /// Site files are stored once in `blobs/` and referenced here; absent in older journals,
    /// whose project bytes already contain every file.
    #[serde(default)]
    files: Option<BTreeMap<String, String>>,
}

/// What `clean_journals` removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct JournalCleanup {
    pub journals: usize,
    pub blobs: usize,
}

impl Journal {
    pub fn new(directory: &Path, project_id: Uuid) -> Result<Self> {
        fs::create_dir_all(directory).map_err(io)?;
        Ok(Self {
            path: directory.join(format!("{project_id}-{}.journal", Uuid::new_v4())),
            project_id,
            last_revision: Mutex::new(None),
            blobs: Mutex::new(Default::default()),
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
        // Imported site files live once in `<journal directory>/blobs`, so a keystroke edit only
        // writes the rest of the project. The entry is one line:
        // `{"sha256":"<hex>","project":<stripped project>,"files":{"<site path>":"<blob>"}}`.
        let mut stripped = project.clone();
        let site_files = stripped
            .website
            .as_mut()
            .map(|site| std::mem::take(&mut site.files))
            .unwrap_or_default();
        let blobs = blobs_directory(&self.path);
        let mut cache = self.blobs.lock().map_err(io)?;
        let mut files = BTreeMap::new();
        let mut next_cache = HashMap::with_capacity(site_files.len());
        let mut blob_bytes = 0_u64;
        // Cached files stay off the workers; a miss means the first append of this site, which
        // stores every file at once.
        let mut missing = Vec::new();
        for (path, content) in &site_files {
            let key = (content.as_ptr() as usize, content.len());
            blob_bytes = blob_bytes.saturating_add(content.len() as u64);
            match cache.get(&key) {
                Some((cached, hash)) if Arc::ptr_eq(cached, content) => {
                    next_cache.insert(key, (Arc::clone(content), hash.clone()));
                    files.insert(path.clone(), hash.clone());
                }
                _ => missing.push((path.clone(), Arc::clone(content))),
            }
        }
        let hashes = store_site_files(&blobs, &missing)?;
        for ((path, content), hash) in missing.into_iter().zip(hashes) {
            let key = (content.as_ptr() as usize, content.len());
            next_cache.insert(key, (Arc::clone(&content), hash.clone()));
            files.insert(path, hash);
        }
        // Only the current project's files stay cached; a replaced file is hashed again once.
        *cache = next_cache;
        // Serialize the stripped project once: the hash covers these bytes and the entry reuses
        // them, so a large project is not serialized twice.
        let project_bytes = serde_json::to_vec(&stripped).map_err(io)?;
        let hash = digest(&project_bytes);
        let files_bytes = serde_json::to_vec(&files).map_err(io)?;
        let mut bytes = Vec::with_capacity(project_bytes.len() + files_bytes.len() + 96);
        bytes.extend_from_slice(b"{\"sha256\":\"");
        bytes.extend_from_slice(hash.as_bytes());
        bytes.extend_from_slice(b"\",\"project\":");
        bytes.extend_from_slice(&project_bytes);
        bytes.extend_from_slice(b",\"files\":");
        bytes.extend_from_slice(&files_bytes);
        bytes.push(b'}');
        if (bytes.len() as u64).saturating_add(blob_bytes) > MAX_MODEL_BYTES {
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
        let bytes = read_journal(path)?;
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
            let document = match &entry.files {
                None => {
                    decode_project_with_source_hash(entry.project.get().as_bytes(), &entry.sha256)?
                }
                Some(files) => {
                    restore_site_files(path, entry.project.get().as_bytes(), &entry.sha256, files)?
                }
            };
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

fn read_journal(path: &Path) -> Result<Vec<u8>> {
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
    Ok(bytes)
}

fn blobs_directory(journal: &Path) -> PathBuf {
    journal
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("blobs")
}

/// Store the site files whose blob is not cached, returning their hashes in the same order.
/// Hashing and fsyncing ~100 files one after another made the first append of an imported site
/// take most of a second, so workers spread the work and the directory is synced once: every
/// blob is durable and visible before the journal entry that references it is written.
fn store_site_files(directory: &Path, files: &[(String, Arc<str>)]) -> Result<Vec<String>> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    fs::create_dir_all(directory).map_err(io)?;
    let workers = thread::available_parallelism()
        .map_or(1, |parallelism| parallelism.get())
        .min(BLOB_WORKERS)
        .min(files.len());
    let chunk = files.len().div_ceil(workers);
    let mut hashes = vec![String::new(); files.len()];
    let failure: Mutex<Option<Error>> = Mutex::new(None);
    thread::scope(|scope| {
        let failure = &failure;
        for (chunk, hashes) in files.chunks(chunk).zip(hashes.chunks_mut(chunk)) {
            scope.spawn(move || {
                for ((_, content), hash) in chunk.iter().zip(hashes) {
                    if failure.lock().is_ok_and(|failure| failure.is_some()) {
                        // Another worker already failed; the caller only reports the first.
                        return;
                    }
                    let value = digest(content.as_bytes());
                    if let Err(error) = write_blob(directory, &value, content) {
                        if let Ok(mut failure) = failure.lock()
                            && failure.is_none()
                        {
                            *failure = Some(error);
                        }
                        return;
                    }
                    *hash = value;
                }
            });
        }
    });
    if let Some(error) = failure.lock().ok().and_then(|mut failure| failure.take()) {
        return Err(error);
    }
    sync_directory(directory)?;
    Ok(hashes)
}

/// Site files are content addressed and shared by every journal in the directory, so a blob is
/// written once and only when it does not exist yet. `atomic_write_unsynced` keeps a partial blob
/// from ever being visible; `store_site_files` fsyncs the directory once all of them are durable.
fn write_blob(directory: &Path, hash: &str, content: &str) -> Result<()> {
    let path = directory.join(hash);
    if path.exists() {
        // Another instance's cleanup removes a blob no journal on disk references yet. A fresh
        // modification time keeps this one inside the grace period until the entry lands.
        if let Ok(file) = File::options().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
        }
        return Ok(());
    }
    atomic_write_unsynced(
        &path,
        |file| file.write_all(content.as_bytes()).map_err(io),
        false,
    )
}

/// Rebuild a project whose imported site files were stored out of line. The entry hashes the
/// stripped project, while `decode_project` hashes the fully restored one, exactly as a saved
/// file does, so recovery reports the same source checksum either way.
fn restore_site_files(
    journal: &Path,
    project: &[u8],
    expected_sha256: &str,
    files: &BTreeMap<String, String>,
) -> Result<DecodedProject> {
    if digest(project) != expected_sha256 {
        return Err(Error(
            "The project source failed its integrity check.".into(),
        ));
    }
    if files.is_empty() {
        return decode_project(project);
    }
    let mut total = project.len() as u64;
    let mut contents = BTreeMap::new();
    for (path, hash) in files {
        contents.insert(path.clone(), read_blob(journal, hash, &mut total)?);
    }
    let mut value: serde_json::Value = serde_json::from_slice(project)
        .map_err(|error| Error(format!("A recovery entry is damaged: {error}")))?;
    let website = value
        .get_mut("website")
        .filter(|website| !website.is_null())
        .ok_or_else(|| Error("The recovery entry's site files do not match its project.".into()))?;
    website["files"] = serde_json::to_value(contents).map_err(io)?;
    let restored = serde_json::to_vec(&value).map_err(io)?;
    decode_project(&restored)
}

fn read_blob(journal: &Path, hash: &str, total: &mut u64) -> Result<String> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(Error(
            "The recovery entry contains an invalid site file reference.".into(),
        ));
    }
    let damaged = || Error("A recovery file is missing or damaged.".into());
    let file = File::open(blobs_directory(journal).join(hash)).map_err(|_| damaged())?;
    let remaining = MAX_MODEL_BYTES.saturating_sub(*total);
    let mut bytes = Vec::new();
    file.take(remaining + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > remaining {
        return Err(Error(
            "The recovery entry exceeds the model size limit.".into(),
        ));
    }
    if digest(&bytes) != hash {
        return Err(damaged());
    }
    let content = String::from_utf8(bytes).map_err(|_| damaged())?;
    *total += content.len() as u64;
    Ok(content)
}

/// Remove journals of closed sessions, and site files that no remaining journal references.
/// `keep` holds the journal file names every recovery pointer still refers to; `is_closed`
/// reports whether no running instance owns a project. Nothing else in the directory is touched.
pub fn clean_journals(
    directory: &Path,
    keep: &BTreeSet<String>,
    is_closed: impl Fn(Uuid) -> bool,
) -> Result<JournalCleanup> {
    let mut report = JournalCleanup::default();
    let mut remaining = BTreeSet::new();
    for entry in fs::read_dir(directory).map_err(io)? {
        let entry = entry.map_err(io)?;
        let file_type = entry.file_type().map_err(io)?;
        if file_type.is_symlink() || !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.ends_with(".journal") {
            continue;
        }
        let closed = !keep.contains(name)
            && older_than(&entry.metadata().map_err(io)?, JOURNAL_GRACE)
            && journal_project_id(name).is_some_and(&is_closed);
        if closed {
            fs::remove_file(entry.path()).map_err(io)?;
            report.journals += 1;
        } else {
            remaining.insert(name.to_owned());
        }
    }
    let mut referenced = BTreeSet::new();
    for name in &remaining {
        match journal_blob_hashes(&directory.join(name)) {
            Ok(hashes) => referenced.extend(hashes),
            // An unparsed journal might reference any site file, so keep them all.
            Err(_) => return Ok(report),
        }
    }
    let blobs = directory.join("blobs");
    let entries = match fs::read_dir(&blobs) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(report),
        Err(error) => return Err(io(error)),
    };
    for entry in entries {
        let entry = entry.map_err(io)?;
        let file_type = entry.file_type().map_err(io)?;
        if file_type.is_symlink() || !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if referenced.contains(name) || !older_than(&entry.metadata().map_err(io)?, BLOB_GRACE) {
            continue;
        }
        fs::remove_file(entry.path()).map_err(io)?;
        report.blobs += 1;
    }
    Ok(report)
}

fn older_than(metadata: &fs::Metadata, grace: Duration) -> bool {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age > grace)
}

/// Journal files are `<project uuid>-<session uuid>.journal`; other names are left alone.
fn journal_project_id(name: &str) -> Option<Uuid> {
    let stem = name.strip_suffix(".journal")?;
    let (project, session) = stem.split_at(stem.len().checked_sub(37)?);
    let session = session.strip_prefix('-')?;
    let project_id = Uuid::parse_str(project).ok()?;
    let session_id = Uuid::parse_str(session).ok()?;
    (project_id.to_string() == project && session_id.to_string() == session).then_some(project_id)
}

fn journal_blob_hashes(path: &Path) -> Result<BTreeSet<String>> {
    let bytes = read_journal(path)?;
    let mut hashes = BTreeSet::new();
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        if !line.ends_with(b"\n") {
            // A torn tail may hide references, so treat the whole journal as unreadable.
            return Err(Error("A recovery entry is damaged.".into()));
        }
        let entry: StoredJournalEntry<'_> = serde_json::from_slice(line)
            .map_err(|e| Error(format!("A recovery entry is damaged: {e}")))?;
        if let Some(files) = entry.files {
            hashes.extend(files.into_values());
        }
    }
    Ok(hashes)
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
    let temporary = parent.join(format!(".desegnos-{}.tmp", Uuid::new_v4()));
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
    atomic_write_unsynced(path, write, backup)?;
    sync_parent(path)
}

/// `atomic_write` without the directory fsync, so a batch of files in one directory pays for a
/// single one. The caller must sync the directory before the files are referenced.
fn atomic_write_unsynced(
    path: &Path,
    write: impl FnOnce(&mut File) -> Result<()>,
    backup: bool,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = parent.join(format!(".desegnos-{}.tmp", Uuid::new_v4()));
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
        fs::rename(&temporary, path).map_err(io)
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
    sync_directory(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    )
}

fn sync_directory(directory: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(directory).map_err(io)?.sync_all().map_err(io)?;
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

pub mod website_export;
pub use website_export::{WrittenWebsiteExport, write_website_export};

#[cfg(test)]
mod journal_tests {
    use super::*;
    use desegnos_core::{Appearance, ImportedWebsite, Viewport};

    fn directory() -> PathBuf {
        let directory = std::env::temp_dir().join(format!("desegnos-journal-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        directory
    }

    fn project(name: &str) -> Project {
        Project::new(
            name.into(),
            Viewport {
                width: 390,
                height: 844,
            },
            Appearance::Dark,
        )
        .unwrap()
    }

    fn site_project(name: &str, files: &[(&str, &str)]) -> Project {
        let mut project = project(name);
        let page_id = project.pages[0].id;
        project.website = Some(ImportedWebsite {
            page_files: BTreeMap::from([(page_id, "index.html".into())]),
            files: files
                .iter()
                .map(|(path, content)| ((*path).into(), Arc::from(*content)))
                .collect(),
            framework: None,
            skipped: vec![],
        });
        project
    }

    fn journal_line(journal: &Journal) -> String {
        let bytes = fs::read(journal.path()).unwrap();
        String::from_utf8(bytes.strip_suffix(b"\n").unwrap().to_vec()).unwrap()
    }

    fn write_line(path: &Path, line: &str) {
        fs::write(path, format!("{line}\n")).unwrap();
    }

    fn age(path: &Path, duration: Duration) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - duration)
            .unwrap();
    }

    fn blob_modified(directory: &Path) -> BTreeMap<String, SystemTime> {
        fs::read_dir(directory)
            .unwrap()
            .flatten()
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.metadata().unwrap().modified().unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn an_appended_entry_is_the_serialized_journal_entry_and_recovers() {
        let directory = directory();
        let project = project("Journaled");
        let journal = Journal::new(&directory, project.id).unwrap();
        journal.append(&project).unwrap();
        let mut expected = serde_json::to_vec(&JournalEntry {
            sha256: digest(&serde_json::to_vec(&project).unwrap()),
            project: project.clone(),
            files: BTreeMap::new(),
        })
        .unwrap();
        expected.push(b'\n');
        assert_eq!(fs::read(journal.path()).unwrap(), expected);
        assert_eq!(Journal::recover(journal.path()).unwrap(), Some(project));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn site_files_are_written_once_and_recover_with_the_saved_source_hash() {
        let directory = directory();
        let html = format!("<html><body>{}</body></html>", "Home ".repeat(1000));
        let css = "body { margin: 0 }";
        let project = site_project("Site", &[("index.html", &html), ("assets/site.css", css)]);
        let journal = Journal::new(&directory, project.id).unwrap();
        journal.append(&project).unwrap();

        let blobs = directory.join("blobs");
        let html_hash = digest(html.as_bytes());
        assert_eq!(fs::read(blobs.join(&html_hash)).unwrap(), html.as_bytes());
        assert_eq!(
            fs::read(blobs.join(digest(css.as_bytes()))).unwrap(),
            css.as_bytes()
        );
        assert_eq!(blob_modified(&blobs).len(), 2);
        let entry: serde_json::Value = serde_json::from_str(&journal_line(&journal)).unwrap();
        assert_eq!(
            entry["project"]["website"]["files"],
            serde_json::json!({}),
            "the journal must not repeat the site files in the project"
        );
        assert_eq!(entry["files"]["index.html"], html_hash);
        assert_eq!(entry["files"]["assets/site.css"], digest(css.as_bytes()));
        assert_eq!(entry.as_object().unwrap().len(), 3);

        let before = blob_modified(&blobs);
        let mut edited = project.clone();
        edited.revision += 1;
        edited.name = "Site edited".into();
        journal.append(&edited).unwrap();
        assert_eq!(
            blob_modified(&blobs),
            before,
            "unchanged site files must not be hashed or rewritten"
        );
        let document = Journal::recover_document(journal.path()).unwrap().unwrap();
        assert_eq!(document.project, edited);
        // Recovery reports the checksum of the full project, as a saved file does.
        let saved = decode_project(&serde_json::to_vec(&edited).unwrap()).unwrap();
        assert_eq!(document.source_sha256, saved.source_sha256);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_existing_site_file_stays_recent_when_another_journal_reuses_it() {
        let directory = directory();
        let project = site_project("Shared", &[("index.html", "<h1>Home</h1>")]);
        Journal::new(&directory, project.id)
            .unwrap()
            .append(&project)
            .unwrap();
        let blob = directory.join("blobs").join(digest(b"<h1>Home</h1>"));
        age(&blob, Duration::from_secs(2 * 60 * 60));
        let before = fs::metadata(&blob).unwrap().modified().unwrap();
        // A second session appends the same site, so the blob is reused instead of rewritten.
        // Its modification time must move forward, or another instance's cleanup would be free
        // to remove it as an orphan before the entry that references it is written.
        Journal::new(&directory, project.id)
            .unwrap()
            .append(&project)
            .unwrap();
        let metadata = fs::metadata(&blob).unwrap();
        assert_eq!(fs::read(&blob).unwrap(), b"<h1>Home</h1>");
        assert!(
            metadata.modified().unwrap() > before,
            "reusing a site file must refresh its modification time"
        );
        assert!(!older_than(&metadata, BLOB_GRACE));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_old_single_file_journal_still_recovers() {
        let directory = directory();
        let project = site_project("Old journal", &[("index.html", "<h1>Old</h1>")]);
        let path = directory.join(format!("{}-{}.journal", project.id, Uuid::new_v4()));
        let project_bytes = serde_json::to_vec(&project).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"{\"sha256\":\"");
        bytes.extend_from_slice(digest(&project_bytes).as_bytes());
        bytes.extend_from_slice(b"\",\"project\":");
        bytes.extend_from_slice(&project_bytes);
        bytes.extend_from_slice(b"}\n");
        fs::write(&path, bytes).unwrap();
        assert_eq!(Journal::recover(&path).unwrap(), Some(project));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_tampered_stripped_project_is_rejected() {
        let directory = directory();
        let project = site_project("Tampered", &[("index.html", "<h1>Home</h1>")]);
        let journal = Journal::new(&directory, project.id).unwrap();
        journal.append(&project).unwrap();
        let line =
            journal_line(&journal).replace("\"name\":\"Tampered\"", "\"name\":\"Tampered!\"");
        write_line(journal.path(), &line);
        let error = Journal::recover(journal.path()).unwrap_err();
        assert!(error.to_string().contains("integrity check"), "{error}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_tampered_site_file_is_rejected() {
        let directory = directory();
        let project = site_project("Damaged", &[("index.html", "<h1>Home</h1>")]);
        let journal = Journal::new(&directory, project.id).unwrap();
        journal.append(&project).unwrap();
        fs::write(
            directory.join("blobs").join(digest(b"<h1>Home</h1>")),
            b"<h1>Fake</h1>",
        )
        .unwrap();
        let error = Journal::recover(journal.path()).unwrap_err();
        assert_eq!(error.to_string(), "A recovery file is missing or damaged.");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_missing_site_file_is_rejected() {
        let directory = directory();
        let project = site_project("Missing", &[("index.html", "<h1>Home</h1>")]);
        let journal = Journal::new(&directory, project.id).unwrap();
        journal.append(&project).unwrap();
        fs::remove_file(directory.join("blobs").join(digest(b"<h1>Home</h1>"))).unwrap();
        let error = Journal::recover(journal.path()).unwrap_err();
        assert_eq!(error.to_string(), "A recovery file is missing or damaged.");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_invalid_site_file_reference_is_rejected() {
        let directory = directory();
        let project = site_project("Traversal", &[("index.html", "<h1>Home</h1>")]);
        let journal = Journal::new(&directory, project.id).unwrap();
        journal.append(&project).unwrap();
        let line = journal_line(&journal);
        let reference = format!("\"index.html\":\"{}\"", digest(b"<h1>Home</h1>"));
        assert!(line.contains(&reference));
        write_line(
            journal.path(),
            &line.replace(&reference, "\"index.html\":\"../x\""),
        );
        let error = Journal::recover(journal.path()).unwrap_err();
        assert!(
            error.to_string().contains("invalid site file reference"),
            "{error}"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cleaning_keeps_referenced_journals_and_removes_closed_and_orphaned_files() {
        let directory = directory();
        let referenced = site_project("Referenced", &[("index.html", "<h1>Referenced</h1>")]);
        let closed = project("Closed");
        let open = project("Open");
        let recent = project("Recent");
        let referenced_journal = Journal::new(&directory, referenced.id).unwrap();
        referenced_journal.append(&referenced).unwrap();
        let closed_journal = Journal::new(&directory, closed.id).unwrap();
        closed_journal.append(&closed).unwrap();
        let open_journal = Journal::new(&directory, open.id).unwrap();
        open_journal.append(&open).unwrap();
        let recent_journal = Journal::new(&directory, recent.id).unwrap();
        recent_journal.append(&recent).unwrap();
        let keep = BTreeSet::from([referenced_journal
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()]);
        let two_hours = Duration::from_secs(2 * 60 * 60);
        for journal in [&referenced_journal, &closed_journal, &open_journal] {
            age(journal.path(), two_hours);
        }
        let blobs = directory.join("blobs");
        let referenced_blob = blobs.join(digest(b"<h1>Referenced</h1>"));
        age(&referenced_blob, two_hours);
        let orphan = blobs.join(digest(b"orphan"));
        fs::write(&orphan, b"orphan").unwrap();
        age(&orphan, two_hours);
        let fresh = blobs.join(digest(b"fresh"));
        fs::write(&fresh, b"fresh").unwrap();

        let report = clean_journals(&directory, &keep, |id| id != open.id).unwrap();
        assert_eq!(
            report,
            JournalCleanup {
                journals: 1,
                blobs: 1
            }
        );
        assert!(referenced_journal.path().exists());
        assert!(!closed_journal.path().exists());
        assert!(
            open_journal.path().exists(),
            "an open project keeps its journal"
        );
        assert!(
            recent_journal.path().exists(),
            "a fresh journal is never removed"
        );
        assert!(referenced_blob.exists(), "a referenced site file stays");
        assert!(!orphan.exists(), "an old unreferenced site file is removed");
        assert!(
            fresh.exists(),
            "a fresh site file may belong to a journal being written"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cleaning_keeps_all_site_files_when_a_remaining_journal_cannot_be_read() {
        let directory = directory();
        let name = format!("{}-{}.journal", Uuid::new_v4(), Uuid::new_v4());
        let broken = directory.join(&name);
        fs::write(&broken, b"{not a journal").unwrap();
        age(&broken, Duration::from_secs(2 * 60 * 60));
        let blobs = directory.join("blobs");
        fs::create_dir(&blobs).unwrap();
        let orphan = blobs.join(digest(b"orphan"));
        fs::write(&orphan, b"orphan").unwrap();
        age(&orphan, Duration::from_secs(2 * 60 * 60));

        let report = clean_journals(&directory, &BTreeSet::from([name]), |_| true).unwrap();
        assert_eq!(report, JournalCleanup::default());
        assert!(
            broken.exists(),
            "a journal that cannot be parsed is left alone"
        );
        assert!(orphan.exists(), "it might reference any site file");
        fs::remove_dir_all(directory).unwrap();
    }
}

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
