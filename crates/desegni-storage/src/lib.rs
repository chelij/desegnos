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
    sync::Mutex,
};
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub mod identity_source;
pub use identity_source::{
    IdentitySource, IdentitySourceAsset, IdentitySourceFile, collect_identity_source,
};

const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MODEL_BYTES: u64 = 128 * 1024 * 1024;

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

pub struct ProjectFile {
    path: PathBuf,
    _lock: File,
}

impl ProjectFile {
    pub fn acquire(path: &Path) -> Result<Self> {
        if path.extension().and_then(|x| x.to_str()) != Some("desegni") {
            return Err(Error(
                "Project files must use the .desegni extension.".into(),
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
                if !metadata.is_file()
                    || path.extension().and_then(|x| x.to_str()) != Some("desegni")
                {
                    return Err(Error("Choose a regular .desegni project file.".into()));
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
        Ok(Self { path, _lock: lock })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn save(&self, project: &Project) -> Result<()> {
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
        if self.path.exists() {
            load_project(&self.path).map_err(|error| Error(format!("The existing project cannot be safely backed up: {error} Save to a new filename to preserve it.")))?;
        }
        atomic_write(
            &self.path,
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
            true,
        )
    }

    pub fn load(&self) -> Result<Project> {
        load_project(&self.path)
    }

    pub fn load_document(&self) -> Result<DecodedProject> {
        load_project_document(&self.path)
    }
}

pub fn load_project(path: &Path) -> Result<Project> {
    Ok(load_project_document(path)?.project)
}

pub fn load_project_document(path: &Path) -> Result<DecodedProject> {
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
        || ![1, 2, 3, 4, 5, SCHEMA_VERSION].contains(&manifest.schema_version)
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
    Ok(document)
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
