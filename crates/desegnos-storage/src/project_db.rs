//! The project file: one SQLite database holding the current design and its whole history.
//!
//! Blobs are content-addressed by SHA-256, so an unchanged boundary, reference list or
//! picture is stored once across every version. A version is a manifest of part hashes with
//! its parent, time and label. Every save is one transaction with a rollback journal, so
//! a crash never leaves a half-written project and no journal files linger beside it. Save,
//! autosave into the hidden `__autosave` branch, alternatives and history all live here;
//! Tauri commands stay thin.
use crate::ensure;
use crate::project_parts::{self, PartData};
use desegnos_core::{Error, Project, Result, digest};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params, types::Value as SqlValue};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

/// The stored format version, kept in `meta` so a future change can migrate it.
pub const FORMAT_VERSION: u32 = 1;
pub const FORMAT: &str = "desegnos";
/// The branch a project starts on, and the one no alternative may delete.
pub const DEFAULT_BRANCH: &str = "Main";
/// Unsaved in-memory edits are kept here, out of the visible history.
pub const AUTOSAVE_BRANCH: &str = "__autosave";
/// Saves this close together amend the previous version instead of adding one.
pub const AMEND_WINDOW_MS: i64 = 5_000;
const MAX_LABEL_CHARS: usize = 120;
pub const MAX_BRANCH_CHARS: usize = 60;
const DAY_MS: i64 = 86_400_000;

const SCHEMA: &str = "
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS blobs(hash TEXT PRIMARY KEY, kind TEXT NOT NULL, bytes BLOB NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS versions(
    id INTEGER PRIMARY KEY,
    parent INTEGER REFERENCES versions(id),
    branch TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    label TEXT NOT NULL,
    manifest BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS branches(name TEXT PRIMARY KEY, head INTEGER NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS head(branch TEXT PRIMARY KEY, version INTEGER NOT NULL) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS versions_branch ON versions(branch);
CREATE INDEX IF NOT EXISTS versions_parent ON versions(parent);
";

use crate::io;



pub fn now_millis() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

fn project_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| crate::PROJECT_FILE_EXTENSIONS.contains(&extension))
}

/// What was read from the database at open or after the last save.
#[derive(ts_rs::TS, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    pub project_id: Uuid,
    pub version: i64,
}

#[derive(ts_rs::TS, Clone, Debug, PartialEq, Eq, Serialize)]
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
    pub fn new(kind: SaveFailureKind, message: impl Into<String>) -> Box<Self> {
        Box::new(Self {
            kind,
            message: message.into(),
            expected: None,
            actual: None,
        })
    }
}

/// A project as it currently opens: its design, branch and version.
#[derive(Clone, Debug)]
pub struct LoadedProject {
    pub project: Project,
    pub branch: String,
    pub version: i64,
    pub created_at: i64,
}

/// Unsaved work found on the hidden autosave branch.
#[derive(Clone, Debug)]
pub struct AutosaveProject {
    pub project: Project,
    pub created_at: i64,
}

#[derive(ts_rs::TS, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub id: i64,
    pub parent: Option<i64>,
    pub label: String,
    pub created_at: i64,
    /// This version is where the branch currently is.
    pub head: bool,
}

#[derive(ts_rs::TS, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummary {
    pub name: String,
    pub head: i64,
    pub current: bool,
    pub primary: bool,
}

#[derive(ts_rs::TS, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHistoryView {
    pub branch: String,
    pub primary_branch: String,
    pub branches: Vec<BranchSummary>,
    pub versions: Vec<VersionSummary>,
}

#[derive(ts_rs::TS, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactReport {
    pub removed_versions: usize,
    pub removed_blobs: usize,
}

#[derive(Clone, Debug)]
struct VersionRow {
    id: i64,
    parent: Option<i64>,
    branch: String,
    created_at: i64,
    label: String,
    manifest: Vec<u8>,
}

/// The one project database a window has open for writing.
pub struct ProjectDatabase {
    path: PathBuf,
    _lock: File,
    connection: Connection,
}

impl ProjectDatabase {
    /// Open an existing project. The sidecar lock stops two windows writing at once.
    pub fn open(path: &Path) -> Result<Self> {
        ensure(
            project_extension(path),
            "Project files must use .desegnos.".to_string(),
        )?;
        let path = resolve_project_path(path)?;
        let lock = acquire_lock(&path)?;
        let connection = open_connection(&path, false)?;
        let database = Self {
            path,
            _lock: lock,
            connection,
        };
        database.verify()?;
        Ok(database)
    }

    /// Create a project file holding one version on the default branch.
    pub fn create(path: &Path, project: &Project, now: i64) -> Result<Self> {
        ensure(
            project_extension(path),
            "Project files must use .desegnos.".to_string(),
        )?;
        let requested_parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(requested_parent).map_err(io)?;
        let path = resolve_project_path(path)?;
        ensure(
            !path.exists(),
            "A file with that name already exists. Choose a new name.".to_string(),
        )?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let created = options.open(&path).map_err(io)?;
        created.sync_all().map_err(io)?;
        drop(created);
        let lock = acquire_lock(&path)?;
        let connection = open_connection(&path, false)?;
        let database = Self {
            path,
            _lock: lock,
            connection,
        };
        if let Err(error) = database.initialize(project, now) {
            let path = database.path.clone();
            drop(database);
            if let Err(cleanup) = fs::remove_file(path) {
                return Err(Error(format!(
                    "{error}; incomplete project cleanup failed: {cleanup}"
                )));
            }
            return Err(error);
        }
        Ok(database)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn initialize(&self, project: &Project, now: i64) -> Result<()> {
        project.validate()?;
        self.connection.execute_batch(SCHEMA).map_err(io)?;
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        initialize_tables(&transaction, project, now)?;
        transaction.commit().map_err(io)?;
        Ok(())
    }

    fn verify(&self) -> Result<()> {
        let format = meta(&self.connection, "format")
            .map_err(|_| Error("This file is not a Desegnos project.".into()))?;
        ensure(
            format.as_deref() == Some(FORMAT),
            "This file is not a Desegnos project.".to_string(),
        )?;
        let version = meta(&self.connection, "format_version")?
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| Error("This project has no format version.".into()))?;
        ensure(
            version == FORMAT_VERSION,
            "This project file was made by another version of Desegnos.".to_string(),
        )?;
        self.project_id()?;
        current_head(&self.connection)?;
        Ok(())
    }

    pub fn project_id(&self) -> Result<Uuid> {
        meta(&self.connection, "project_id")?
            .and_then(|value| Uuid::parse_str(&value).ok())
            .ok_or_else(|| Error("This project has no identity.".into()))
    }

    pub fn stamp(&self) -> Result<FileStamp> {
        let (_, version) = current_head(&self.connection)?;
        Ok(FileStamp {
            project_id: self.project_id()?,
            version,
        })
    }

    /// The newest version id on the current branch, recorded with an export.
    pub fn head_version(&self) -> Result<i64> {
        Ok(current_head(&self.connection)?.1)
    }

    pub fn branch(&self) -> Result<String> {
        Ok(current_head(&self.connection)?.0)
    }

    /// The saved design, as the file's current branch and version hold it.
    pub fn load(&self) -> Result<LoadedProject> {
        let (branch, version) = current_head(&self.connection)?;
        let row = version_row(&self.connection, version)?
            .ok_or_else(|| Error("This project's current version is missing.".into()))?;
        let project = project_from_manifest(&self.connection, &row.manifest)?;
        Ok(LoadedProject {
            project,
            branch,
            version,
            created_at: row.created_at,
        })
    }

    /// Save the design as a version on the current branch, amending a version saved moments
    /// ago on the same branch so quick saves stay one version.
    pub fn save(
        &self,
        project: &Project,
        labels: &[String],
        label: Option<&str>,
    ) -> Result<VersionSummary> {
        let now = now_millis();
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        let saved = save_in(&transaction, project, labels, label, now, true)?;
        transaction.commit().map_err(io)?;
        Ok(saved)
    }

    /// Save without replacing a file that changed underneath us.
    pub fn save_checked(
        &self,
        project: &Project,
        expected: Option<&FileStamp>,
        labels: &[String],
        label: Option<&str>,
    ) -> std::result::Result<VersionSummary, Box<SaveFailure>> {
        check_stamp(self, project, expected, false)?;
        self.save(project, labels, label)
            .map_err(|error| SaveFailure::new(SaveFailureKind::InvalidProject, error.to_string()))
    }

    /// Replace the project after an explicit user choice bound to this observed stamp.
    pub fn save_replacing(
        &self,
        project: &Project,
        observed: &FileStamp,
        allow_different_project: bool,
        labels: &[String],
        label: Option<&str>,
    ) -> std::result::Result<VersionSummary, Box<SaveFailure>> {
        check_stamp(self, project, Some(observed), allow_different_project)?;
        let actual = self
            .stamp()
            .map_err(|error| SaveFailure::new(SaveFailureKind::Io, error.to_string()))?;
        if actual.project_id != project.id {
            self.reset(project).map_err(|error| {
                SaveFailure::new(SaveFailureKind::InvalidProject, error.to_string())
            })?;
        }
        self.save(project, labels, label)
            .map_err(|error| SaveFailure::new(SaveFailureKind::InvalidProject, error.to_string()))
    }

    /// Throw away every version and start this file again from this project.
    fn reset(&self, project: &Project) -> Result<()> {
        project.validate()?;
        let now = now_millis();
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        transaction
            .execute_batch(
                "DELETE FROM head; DELETE FROM branches; DELETE FROM versions; DELETE FROM blobs; DELETE FROM meta;",
            )
            .map_err(io)?;
        initialize_tables(&transaction, project, now)?;
        transaction.commit().map_err(io)?;
        Ok(())
    }

    /// Keep unsaved edits as a version on the hidden autosave branch.
    pub fn autosave(&self, project: &Project, now: i64) -> Result<()> {
        project.validate()?;
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        let (_, head) = current_head(&transaction)?;
        let head_manifest = version_row(&transaction, head)?
            .map(|row| row.manifest)
            .ok_or_else(|| Error("This project's current version is missing.".into()))?;
        let manifest = write_parts(&transaction, project)?;
        if manifest == head_manifest {
            clear_autosave(&transaction)?;
        } else {
            clear_autosave(&transaction)?;
            let id = insert_version(
                &transaction,
                Some(head),
                AUTOSAVE_BRANCH,
                now,
                "Unsaved changes",
                &manifest,
            )?;
            transaction
                .execute(
                    "INSERT INTO branches(name, head) VALUES (?1, ?2)",
                    params![AUTOSAVE_BRANCH, id],
                )
                .map_err(io)?;
        }
        transaction.commit().map_err(io)?;
        Ok(())
    }

    /// Unsaved work that is newer than the saved version, if any.
    pub fn autosave_work(&self) -> Result<Option<AutosaveProject>> {
        read_autosave_work(&self.connection)
    }

    /// Stop offering the autosave: the unsaved edits are gone.
    pub fn discard_autosave(&self) -> Result<()> {
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        clear_autosave(&transaction)?;
        transaction.commit().map_err(io)?;
        Ok(())
    }

    /// Every version on the branch's chain, newest first.
    pub fn history(&self) -> Result<ProjectHistoryView> {
        let (branch, head) = current_head(&self.connection)?;
        let mut versions = Vec::new();
        let mut cursor = Some(head);
        while let Some(id) = cursor {
            let Some(row) = version_row(&self.connection, id)? else {
                break;
            };
            versions.push(VersionSummary {
                id: row.id,
                parent: row.parent,
                label: row.label,
                created_at: row.created_at,
                head: row.id == head,
            });
            if versions.len() >= 10_000 {
                break;
            }
            cursor = row.parent;
        }
        let primary = primary_branch(&self.connection)?;
        let mut branches: Vec<BranchSummary> = self
            .branch_list()?
            .into_iter()
            .map(|(name, head)| BranchSummary {
                current: name == branch,
                primary: name == primary,
                name,
                head,
            })
            .collect();
        branches.sort_by(|left, right| {
            right
                .primary
                .cmp(&left.primary)
                .then_with(|| right.current.cmp(&left.current))
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(ProjectHistoryView {
            branches,
            branch,
            primary_branch: primary,
            versions,
        })
    }

    fn branch_list(&self) -> Result<Vec<(String, i64)>> {
        let mut statement = self
            .connection
            .prepare("SELECT name, head FROM branches WHERE name <> ?1 ORDER BY name")
            .map_err(io)?;
        let rows = statement
            .query_map(params![AUTOSAVE_BRANCH], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(io)?;
        let mut list = Vec::new();
        for row in rows {
            list.push(row.map_err(io)?);
        }
        Ok(list)
    }

    /// The design stored in one version.
    pub fn version_project(&self, version: i64) -> Result<Project> {
        let row = version_row(&self.connection, version)?
            .ok_or_else(|| Error("That version is no longer in this project.".into()))?;
        project_from_manifest(&self.connection, &row.manifest)
    }

    /// Restore a version as a new version on the current branch. History is never rewritten.
    pub fn restore(&self, version: i64) -> Result<Project> {
        let project = self.version_project(version)?;
        let now = now_millis();
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        save_in(
            &transaction,
            &project,
            &["Restored a version".to_string()],
            None,
            now,
            false,
        )?;
        transaction.commit().map_err(io)?;
        Ok(project)
    }

    /// Open another branch's current version and make it the file's branch.
    pub fn switch_branch(&self, name: &str) -> Result<LoadedProject> {
        ensure(
            name != AUTOSAVE_BRANCH,
            "That branch is not an alternative.".to_string(),
        )?;
        let head = branch_head(&self.connection, name)?
            .ok_or_else(|| Error("That alternative is no longer in this project.".into()))?;
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        set_head(&transaction, name, head)?;
        transaction.commit().map_err(io)?;
        let row = version_row(&self.connection, head)?
            .ok_or_else(|| Error("That alternative's version is missing.".into()))?;
        let project = project_from_manifest(&self.connection, &row.manifest)?;
        Ok(LoadedProject {
            project,
            branch: name.to_owned(),
            version: head,
            created_at: row.created_at,
        })
    }

    /// Start an alternative at the current version and make it the file's branch.
    pub fn create_branch(&self, name: &str) -> Result<String> {
        let name = valid_branch_name(name)?;
        let (_, head) = current_head(&self.connection)?;
        ensure(
            branch_head(&self.connection, &name)?.is_none(),
            "An alternative with that name already exists.".to_string(),
        )?;
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        transaction
            .execute(
                "INSERT INTO branches(name, head) VALUES (?1, ?2)",
                params![name, head],
            )
            .map_err(io)?;
        set_head(&transaction, &name, head)?;
        transaction.commit().map_err(io)?;
        Ok(name)
    }

    pub fn rename_branch(&self, from: &str, to: &str) -> Result<String> {
        let to = valid_branch_name(to)?;
        ensure(
            from != AUTOSAVE_BRANCH,
            "That branch is not an alternative.".to_string(),
        )?;
        ensure(
            branch_head(&self.connection, from)?.is_some(),
            "That alternative is no longer in this project.".to_string(),
        )?;
        ensure(
            branch_head(&self.connection, &to)?.is_none(),
            "An alternative with that name already exists.".to_string(),
        )?;
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        transaction
            .execute(
                "UPDATE branches SET name = ?1 WHERE name = ?2",
                params![to, from],
            )
            .map_err(io)?;
        transaction
            .execute(
                "UPDATE versions SET branch = ?1 WHERE branch = ?2",
                params![to, from],
            )
            .map_err(io)?;
        transaction
            .execute(
                "UPDATE head SET branch = ?1 WHERE branch = ?2",
                params![to, from],
            )
            .map_err(io)?;
        if meta(&transaction, "primary_branch")?.as_deref() == Some(from) {
            set_meta(&transaction, "primary_branch", &to)?;
        }
        transaction.commit().map_err(io)?;
        Ok(to)
    }

    /// Delete an alternative. The primary branch, the current branch and the last branch stay.
    pub fn delete_branch(&self, name: &str) -> Result<()> {
        ensure(
            name != AUTOSAVE_BRANCH,
            "That branch is not an alternative.".to_string(),
        )?;
        let (current, _) = current_head(&self.connection)?;
        let primary = primary_branch(&self.connection)?;
        ensure(
            name != primary,
            "The main branch cannot be deleted.".to_string(),
        )?;
        ensure(
            name != current,
            "Switch to another alternative before deleting this one.".to_string(),
        )?;
        let branches = self.branch_list()?;
        ensure(
            branches.iter().any(|(branch, _)| branch == name),
            "That alternative is no longer in this project.".to_string(),
        )?;
        ensure(
            branches.len() > 1,
            "A project keeps at least one branch.".to_string(),
        )?;
        let used = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM versions child JOIN versions parent ON parent.id = child.parent WHERE child.branch <> ?1 AND parent.branch = ?1)",
            params![name],
            |row| row.get::<_, bool>(0),
        ).map_err(io)?;
        ensure(
            !used,
            "Another alternative started from this one. Delete that one first.".to_string(),
        )?;
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        let mut ids = Vec::new();
        {
            let mut statement = transaction
                .prepare("SELECT id FROM versions WHERE branch = ?1 ORDER BY id DESC")
                .map_err(io)?;
            let rows = statement.query_map(params![name], |row| row.get::<_, i64>(0)).map_err(io)?;
            for row in rows {
                ids.push(row.map_err(io)?);
            }
        }
        for id in ids {
            // Any child outside this branch (an autosave, most often) moves to the parent.
            let parent = version_row(&transaction, id)?.and_then(|row| row.parent);
            transaction
                .execute(
                    "UPDATE versions SET parent = ?1 WHERE parent = ?2",
                    params![parent, id],
                )
                .map_err(io)?;
            transaction
                .execute("DELETE FROM versions WHERE id = ?1", params![id])
                .map_err(io)?;
        }
        transaction
            .execute("DELETE FROM branches WHERE name = ?1", params![name])
            .map_err(io)?;
        transaction.commit().map_err(io)?;
        Ok(())
    }

    /// Keep every version from the last 7 days, then one per day for 30 days, then one per
    /// week; drop blobs nothing references; and shrink the file.
    pub fn compact(&self, now: i64) -> Result<CompactReport> {
        let mut removed_versions = 0;
        let transaction = self.connection.unchecked_transaction().map_err(io)?;
        for (branch, _) in self.branch_list()? {
            let chain = branch_chain(&transaction, &branch)?;
            if chain.is_empty() {
                continue;
            }
            let keep = kept_versions(&chain, now);
            for version in chain {
                if keep.contains(&version.id) {
                    continue;
                }
                transaction
                    .execute(
                        "UPDATE versions SET parent = ?1 WHERE parent = ?2",
                        params![version.parent, version.id],
                    )
                    .map_err(io)?;
                transaction
                    .execute("DELETE FROM versions WHERE id = ?1", params![version.id])
                    .map_err(io)?;
                removed_versions += 1;
            }
        }
        let referenced = referenced_blobs(&transaction)?;
        let stored = blob_hashes(&transaction)?;
        let mut removed_blobs = 0;
        for hash in stored {
            if referenced.contains(&hash) {
                continue;
            }
            transaction
                .execute("DELETE FROM blobs WHERE hash = ?1", params![hash])
                .map_err(io)?;
            removed_blobs += 1;
        }
        transaction.commit().map_err(io)?;
        self.connection.execute_batch("VACUUM").map_err(io)?;
        Ok(CompactReport {
            removed_versions,
            removed_blobs,
        })
    }
}

/// Read a project without taking the writing lock, e.g. while it is open in another window.
pub fn load_project(path: &Path) -> Result<Project> {
    let connection = open_connection(path, true)?;
    let (_, version) = current_head(&connection)?;
    let row = version_row(&connection, version)?
        .ok_or_else(|| Error("This project's current version is missing.".into()))?;
    project_from_manifest(&connection, &row.manifest)
}

/// Read a project's unsaved work without taking the writing lock, for the recovery list.
pub fn read_autosave(path: &Path) -> Result<Option<AutosaveProject>> {
    if !project_extension(path) {
        return Ok(None);
    }
    let connection = open_connection(path, true)?;
    if meta(&connection, "format")?.as_deref() != Some(FORMAT) {
        return Err(Error("This file is not a Desegnos project.".into()));
    }
    read_autosave_work(&connection)
}

fn resolve_project_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| Error("Choose a project filename.".into()))?;
    if path.exists() {
        let resolved = path.canonicalize().map_err(io)?;
        let metadata = resolved.metadata().map_err(io)?;
        ensure(
            metadata.is_file(),
            "Choose a regular .desegnos project file.".to_string(),
        )?;
        return Ok(resolved);
    }
    let parent = parent.canonicalize().map_err(io)?;
    Ok(parent.join(name))
}

fn acquire_lock(path: &Path) -> Result<File> {
    let name = path
        .file_name()
        .ok_or_else(|| Error("Choose a project filename.".into()))?;
    let lock_path = path.with_file_name(format!(".{}.lock", name.to_string_lossy()));
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(&lock_path).map_err(io)?;
    lock.try_lock()
        .map_err(|_| Error("This project is already open for writing in another window.".into()))?;
    Ok(lock)
}

fn open_connection(path: &Path, read_only: bool) -> Result<Connection> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };
    let connection = Connection::open_with_flags(path, flags)
        .map_err(|error| Error(format!("This project cannot be read: {error}")))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(io)?;
    let unreadable = |error: rusqlite::Error| {
        Error(format!(
            "This file is not a readable Desegnos project: {error}"
        ))
    };
    if read_only {
        connection
            .execute_batch("PRAGMA foreign_keys = ON")
            .map_err(unreadable)?;
    } else {
        connection
            .execute_batch(
                "PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON",
            )
            .map_err(unreadable)?;
    }
    Ok(connection)
}

fn initialize_tables(connection: &Connection, project: &Project, now: i64) -> Result<()> {
    let manifest = write_parts(connection, project)?;
    let version = insert_version(
        connection,
        None,
        DEFAULT_BRANCH,
        now,
        "Created the project",
        &manifest,
    )?;
    connection
        .execute(
            "INSERT INTO branches(name, head) VALUES (?1, ?2)",
            params![DEFAULT_BRANCH, version],
        )
        .map_err(io)?;
    set_head(connection, DEFAULT_BRANCH, version)?;
    for (key, value) in [
        ("format", FORMAT.to_owned()),
        ("format_version", FORMAT_VERSION.to_string()),
        ("project_id", project.id.to_string()),
        ("name", project.name.clone()),
        ("primary_branch", DEFAULT_BRANCH.to_owned()),
        ("created_at", now.to_string()),
        ("updated_at", now.to_string()),
    ] {
        set_meta(connection, key, &value)?;
    }
    Ok(())
}

fn meta(connection: &Connection, key: &str) -> Result<Option<String>> {
    connection
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(io)
}

fn set_meta(connection: &Connection, key: &str, value: &str) -> Result<()> {
    connection
        .execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map_err(io)?;
    Ok(())
}

fn current_head(connection: &Connection) -> Result<(String, i64)> {
    connection
        .query_row("SELECT branch, version FROM head", [], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .optional()
        .map_err(io)?
        .ok_or_else(|| Error("This project has no current branch.".into()))
}

fn branch_head(connection: &Connection, name: &str) -> Result<Option<i64>> {
    connection
        .query_row(
            "SELECT head FROM branches WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )
        .optional()
        .map_err(io)
}

fn set_head(connection: &Connection, branch: &str, version: i64) -> Result<()> {
    connection.execute("DELETE FROM head", []).map_err(io)?;
    connection
        .execute(
            "INSERT INTO head(branch, version) VALUES (?1, ?2)",
            params![branch, version],
        )
        .map_err(io)?;
    connection
        .execute(
            "INSERT INTO branches(name, head) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET head = excluded.head",
            params![branch, version],
        )
        .map_err(io)?;
    Ok(())
}

fn primary_branch(connection: &Connection) -> Result<String> {
    Ok(meta(connection, "primary_branch")?.unwrap_or_else(|| DEFAULT_BRANCH.to_owned()))
}

fn version_row(connection: &Connection, id: i64) -> Result<Option<VersionRow>> {
    connection
        .query_row(
            "SELECT id, parent, branch, created_at, label, manifest FROM versions WHERE id = ?1",
            params![id],
            |row| {
                Ok(VersionRow {
                    id: row.get(0)?,
                    parent: row.get(1)?,
                    branch: row.get(2)?,
                    created_at: row.get(3)?,
                    label: row.get(4)?,
                    manifest: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(io)
}

fn insert_version(
    connection: &Connection,
    parent: Option<i64>,
    branch: &str,
    now: i64,
    label: &str,
    manifest: &[u8],
) -> Result<i64> {
    connection
        .execute(
            "INSERT INTO versions(parent, branch, created_at, label, manifest)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![parent, branch, now, label, manifest],
        )
        .map_err(io)?;
    Ok(connection.last_insert_rowid())
}

fn has_children(connection: &Connection, id: i64) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM versions WHERE parent = ?1)",
            params![id],
            |row| row.get(0),
        )
        .map_err(io)
}

/// Write every part as a blob and return the version's manifest bytes.
fn write_parts(connection: &Connection, project: &Project) -> Result<Vec<u8>> {
    let parts = project_parts::split_project(project)?;
    let mut hashes = serde_json::Map::new();
    for (name, part) in parts {
        let bytes = part.bytes();
        let hash = digest(bytes);
        connection
            .execute(
                "INSERT OR IGNORE INTO blobs(hash, kind, bytes) VALUES (?1, ?2, ?3)",
                params![hash, part.kind(), bytes],
            )
            .map_err(io)?;
        hashes.insert(name, serde_json::Value::String(hash));
    }
    Ok(project_parts::canonical_json(&serde_json::Value::Object(
        hashes,
    )))
}

fn project_from_manifest(connection: &Connection, manifest: &[u8]) -> Result<Project> {
    let hashes: BTreeMap<String, String> =
        serde_json::from_slice(manifest).map_err(|error| Error(format!("A version's manifest is damaged: {error}")))?;
    let mut parts = BTreeMap::new();
    for (name, hash) in hashes {
        let row = connection
            .query_row(
                "SELECT kind, bytes FROM blobs WHERE hash = ?1",
                params![hash],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, SqlValue>(1)?)),
            )
            .optional()
            .map_err(io)?
            .ok_or_else(|| Error("A version refers to a stored part that is missing.".into()))?;
        let bytes = match row.1 {
            SqlValue::Blob(bytes) => bytes,
            _ => return Err(Error("A stored part is not binary data.".into())),
        };
        let part = if row.0 == "json" {
            PartData::Json(bytes)
        } else {
            PartData::Picture { mime: row.0, bytes }
        };
        parts.insert(name, part);
    }
    project_parts::join_project(&parts)
}

fn save_in(
    connection: &Connection,
    project: &Project,
    labels: &[String],
    label: Option<&str>,
    now: i64,
    allow_amend: bool,
) -> Result<VersionSummary> {
    project.validate()?;
    let stored_id = meta(connection, "project_id")?
        .and_then(|value| Uuid::parse_str(&value).ok())
        .ok_or_else(|| Error("This project has no identity.".into()))?;
    ensure(
        stored_id == project.id,
        "This file is a different project. Save a copy with a new name.".to_string(),
    )?;
    let (branch, head) = current_head(connection)?;
    let manifest = write_parts(connection, project)?;
    let label = match label.map(str::trim) {
        Some(label) if !label.is_empty() => clean_label(label),
        _ => describe_labels(labels),
    };
    let previous = version_row(connection, head)?;
    let amend = allow_amend
        && previous.as_ref().is_some_and(|previous| {
            previous.branch == branch
                && now.saturating_sub(previous.created_at) <= AMEND_WINDOW_MS
        })
        && !has_children(connection, head)?;
    let id = if amend {
        let previous = previous.unwrap_or_else(|| unreachable!("checked above"));
        let merged = merge_labels(&previous.label, &label);
        connection
            .execute(
                "UPDATE versions SET manifest = ?1, created_at = ?2, label = ?3 WHERE id = ?4",
                params![manifest, now, merged, head],
            )
            .map_err(io)?;
        head
    } else {
        let id = insert_version(connection, Some(head), &branch, now, &label, &manifest)?;
        set_head(connection, &branch, id)?;
        id
    };
    clear_autosave(connection)?;
    set_meta(connection, "name", &project.name)?;
    set_meta(connection, "updated_at", &now.to_string())?;
    let row = version_row(connection, id)?
        .ok_or_else(|| Error("The saved version could not be read back.".into()))?;
    Ok(VersionSummary {
        id: row.id,
        parent: row.parent,
        label: row.label,
        created_at: row.created_at,
        head: true,
    })
}

fn clear_autosave(connection: &Connection) -> Result<()> {
    connection
        .execute(
            "DELETE FROM versions WHERE branch = ?1",
            params![AUTOSAVE_BRANCH],
        )
        .map_err(io)?;
    connection
        .execute(
            "DELETE FROM branches WHERE name = ?1",
            params![AUTOSAVE_BRANCH],
        )
        .map_err(io)?;
    Ok(())
}

fn read_autosave_work(connection: &Connection) -> Result<Option<AutosaveProject>> {
    let row = connection
        .query_row(
            "SELECT created_at, manifest FROM versions WHERE branch = ?1 ORDER BY id DESC LIMIT 1",
            params![AUTOSAVE_BRANCH],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(io)?;
    let Some((created_at, manifest)) = row else {
        return Ok(None);
    };
    // A save clears the autosave in its own transaction, so an autosave that differs from the
    // head is unsaved work from after it.
    let (_, head) = current_head(connection)?;
    let Some(head_row) = version_row(connection, head)? else {
        return Ok(None);
    };
    if manifest == head_row.manifest {
        return Ok(None);
    }
    let project = project_from_manifest(connection, &manifest).map_err(|error| {
        Error(format!(
            "The unsaved work in this project is damaged: {error}"
        ))
    })?;
    Ok(Some(AutosaveProject {
        project,
        created_at,
    }))
}

fn check_stamp(
    database: &ProjectDatabase,
    project: &Project,
    expected: Option<&FileStamp>,
    allow_different_project: bool,
) -> std::result::Result<(), Box<SaveFailure>> {
    let actual = database
        .stamp()
        .map_err(|error| SaveFailure::new(SaveFailureKind::Unreadable, error.to_string()))?;
    if actual.project_id != project.id && !allow_different_project {
        return Err(Box::new(SaveFailure {
            kind: SaveFailureKind::DifferentProject,
            message: "This file is a different project. Save a copy with a new name.".into(),
            expected: expected.cloned(),
            actual: Some(actual),
        }));
    }
    if expected.is_some_and(|expected| *expected != actual) {
        return Err(Box::new(SaveFailure {
            kind: SaveFailureKind::Changed,
            message: "This file changed since you opened it.".into(),
            expected: expected.cloned(),
            actual: Some(actual),
        }));
    }
    Ok(())
}

fn valid_branch_name(name: &str) -> Result<String> {
    let name = name.trim();
    ensure(
        !name.is_empty() && name.chars().count() <= MAX_BRANCH_CHARS,
        "Name the alternative in 60 characters or fewer.".to_string(),
    )?;
    ensure(
        !name.chars().any(char::is_control),
        "That alternative name contains invalid characters.".to_string(),
    )?;
    ensure(
        name != AUTOSAVE_BRANCH,
        "That name is reserved. Choose another one.".to_string(),
    )?;
    Ok(name.to_owned())
}

/// The stored label for a version: the undo labels that led to it, shortest useful form.
fn describe_labels(labels: &[String]) -> String {
    let mut unique: Vec<&str> = Vec::new();
    for label in labels {
        let label = label.trim();
        if !label.is_empty() && !unique.contains(&label) {
            unique.push(label);
        }
    }
    if unique.is_empty() {
        return "Saved".into();
    }
    clean_label(&unique.join(", "))
}

fn clean_label(label: &str) -> String {
    let flattened: String = label
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let trimmed = flattened.trim();
    if trimmed.chars().count() <= MAX_LABEL_CHARS {
        return trimmed.to_owned();
    }
    let mut cut: String = trimmed.chars().take(MAX_LABEL_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// Fold labels into one, without repeating what the version already says.
fn merge_labels(existing: &str, incoming: &str) -> String {
    let mut merged: Vec<&str> = existing
        .split(", ")
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .collect();
    for label in incoming.split(", ").map(str::trim) {
        if !label.is_empty() && label != "Saved" && !merged.contains(&label) {
            merged.push(label);
        }
    }
    clean_label(&merged.join(", "))
}

fn branch_chain(connection: &Connection, branch: &str) -> Result<Vec<VersionRow>> {
    let Some(head) = branch_head(connection, branch)? else {
        return Ok(vec![]);
    };
    let mut chain = Vec::new();
    let mut cursor = Some(head);
    while let Some(id) = cursor {
        let Some(row) = version_row(connection, id)? else {
            break;
        };
        cursor = row.parent;
        chain.push(row);
        if chain.len() >= 10_000 {
            break;
        }
    }
    Ok(chain)
}

/// Pick the versions history compaction keeps, newest first.
fn kept_versions(chain: &[VersionRow], now: i64) -> BTreeSet<i64> {
    let mut keep = BTreeSet::new();
    let mut days = BTreeSet::new();
    let mut weeks = BTreeSet::new();
    for (index, version) in chain.iter().enumerate() {
        let age = now.saturating_sub(version.created_at);
        if index == 0 || age <= 7 * DAY_MS {
            keep.insert(version.id);
        } else if age <= 37 * DAY_MS {
            let day = version.created_at.div_euclid(DAY_MS);
            if days.insert(day) {
                keep.insert(version.id);
            }
        } else {
            let week = (version.created_at.div_euclid(DAY_MS) + 3).div_euclid(7);
            if weeks.insert(week) {
                keep.insert(version.id);
            }
        }
    }
    keep
}

fn referenced_blobs(connection: &Connection) -> Result<BTreeSet<String>> {
    let mut statement = connection
        .prepare("SELECT manifest FROM versions")
        .map_err(io)?;
    let rows = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(io)?;
    let mut referenced = BTreeSet::new();
    for row in rows {
        let manifest: BTreeMap<String, String> = serde_json::from_slice(&row.map_err(io)?)
            .map_err(|error| Error(format!("A version's manifest is damaged: {error}")))?;
        referenced.extend(manifest.into_values());
    }
    Ok(referenced)
}

fn blob_hashes(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare("SELECT hash FROM blobs")
        .map_err(io)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(io)?;
    let mut hashes = Vec::new();
    for row in rows {
        hashes.push(row.map_err(io)?);
    }
    Ok(hashes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use desegnos_core::{Appearance, ElementKind, Rect, Viewport, empty_element};
    use std::path::PathBuf;

    const DAY: i64 = 86_400_000;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "desegnos-db-{label}-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn project_path(&self) -> PathBuf {
            self.0.join("project.desegnos")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn project() -> Project {
        let project = Project::new(
            "History".into(),
            Viewport {
                width: 800,
                height: 600,
            },
            Appearance::Light,
        )
        .unwrap();
        let page = project.home_page_id;
        let mut element = empty_element(
            ElementKind::Text,
            Rect {
                x: 10.0,
                y: 10.0,
                width: 200.0,
                height: 40.0,
            },
        );
        element.text = "Start".into();
        project
            .apply(
                project.revision,
                desegnos_core::Command::InsertElements {
                    page_id: page,
                    parent_id: None,
                    index: 0,
                    elements: vec![element],
                    picture_data_uri: None,
                    size_id: None,
                },
            )
            .unwrap()
    }

    fn version_at(database: &ProjectDatabase, project: &Project, now: i64, label: &str) -> i64 {
        let transaction = database.connection.unchecked_transaction().unwrap();
        let version = save_in(&transaction, project, &[], Some(label), now, true).unwrap();
        transaction.commit().unwrap();
        version.id
    }

    #[test]
    fn saves_amend_only_inside_the_window_and_without_children() {
        let directory = TempDir::new("amend");
        let now = 100 * DAY;
        let project = project();
        let database =
            ProjectDatabase::create(&directory.project_path(), &project, now - DAY).unwrap();

        let mut first = project.clone();
        first.name = "First".into();
        let id = version_at(&database, &first, now, "First");
        let mut second = project.clone();
        second.name = "Second".into();
        let amended = version_at(&database, &second, now + AMEND_WINDOW_MS, "Second");
        assert_eq!(amended, id, "a save inside the window amends");
        assert_eq!(database.load().unwrap().project.name, "Second");

        let mut third = project.clone();
        third.name = "Third".into();
        let next = version_at(&database, &third, now + 2 * AMEND_WINDOW_MS + 1, "Third");
        assert_ne!(next, id, "a save after the window is a new version");
        assert_eq!(database.history().unwrap().versions.len(), 3);

        // A version with a child is never amended, even inside the window.
        let transaction = database.connection.unchecked_transaction().unwrap();
        let child = insert_version(&transaction, Some(next), "Child", now + 10, "Child", b"{}")
            .unwrap();
        set_head(&transaction, DEFAULT_BRANCH, child).unwrap();
        transaction.commit().unwrap();
        let mut fourth = project.clone();
        fourth.name = "Fourth".into();
        let after_child = version_at(&database, &fourth, now + 20, "Fourth");
        assert_ne!(after_child, child, "a version with children keeps its history");
    }

    #[test]
    fn compact_keeps_the_documented_versions_and_drops_unreferenced_blobs() {
        let directory = TempDir::new("compact");
        let now = 200 * DAY;
        let project = project();
        let database =
            ProjectDatabase::create(&directory.project_path(), &project, now - 100 * DAY).unwrap();

        let times = [
            now - 40 * DAY,
            now - 20 * DAY,
            now - 20 * DAY + 3_600_000,
            now - 20 * DAY + 7_200_000,
            now - 2 * DAY,
            now,
        ];
        let mut versions = Vec::new();
        for (index, time) in times.iter().enumerate() {
            let mut changed = project.clone();
            changed.revision = index as u64 + 1;
            changed.pages[0].elements[0].text = format!("Version {index}");
            versions.push(version_at(&database, &changed, *time, &format!("V{index}")));
        }
        let before_blobs: i64 = database
            .connection
            .query_row("SELECT COUNT(*) FROM blobs", [], |row| row.get(0))
            .unwrap();
        let report = database.compact(now).unwrap();
        assert_eq!(report.removed_versions, 2, "same-day older versions are thinned");
        assert!(
            report.removed_blobs >= 2,
            "blobs only the removed versions used are gone"
        );
        let after_blobs: i64 = database
            .connection
            .query_row("SELECT COUNT(*) FROM blobs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(before_blobs - after_blobs, report.removed_blobs as i64);

        let mut labels = Vec::new();
        let mut statement = database
            .connection
            .prepare("SELECT label FROM versions ORDER BY id")
            .unwrap();
        let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap();
        for row in rows {
            labels.push(row.unwrap());
        }
        assert!(labels.contains(&"Created the project".to_string()));
        assert!(labels.contains(&"V0".to_string()), "an old week is kept");
        assert!(labels.contains(&"V3".to_string()), "the newest of the day is kept");
        assert!(labels.contains(&"V4".to_string()), "the last 7 days stay");
        assert!(labels.contains(&"V5".to_string()), "the head stays");
        assert!(!labels.contains(&"V1".to_string()), "older same-day versions go");
        assert!(!labels.contains(&"V2".to_string()), "older same-day versions go");
        assert!(versions.contains(&versions[5]), "the head id is stable");
    }
}
