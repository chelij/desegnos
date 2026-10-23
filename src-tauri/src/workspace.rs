use desegni_core::{
    Appearance, Command, DecodedProject, GenerationCandidate, Project, RequirementsSnapshot,
    Session, Viewport,
};
use desegni_storage::{
    FileStamp, Journal, ProjectFile, SaveFailure, SaveFailureKind, write_markdown,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::PathBuf,
    sync::Mutex,
    time::SystemTime,
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

type NativeResult<T> = Result<T, String>;
type WorkspaceState<'a> = State<'a, Mutex<Workspace>>;

/// Open failures keep existing text errors and expose recovery choices without parsing prose.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ProjectOpenError {
    Recovery {
        code: &'static str,
        #[serde(rename = "projectId")]
        project_id: Uuid,
        path: PathBuf,
    },
    Recent(RecentProjectOpenError),
    Message(String),
}

impl From<String> for ProjectOpenError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}
impl From<&str> for ProjectOpenError {
    fn from(message: &str) -> Self {
        Self::Message(message.into())
    }
}
impl From<ProjectOpenError> for String {
    fn from(error: ProjectOpenError) -> Self {
        match error {
            ProjectOpenError::Message(message) => message,
            ProjectOpenError::Recent(error) => error.message,
            ProjectOpenError::Recovery { .. } => {
                "This project has unsaved changes from an earlier session.".into()
            }
        }
    }
}

type OpenResult<T> = Result<T, ProjectOpenError>;

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryPointer {
    journal_file: String,
    saved_revision: Option<u64>,
    saved_sha256: Option<String>,
    project_path: Option<PathBuf>,
    /// The journal checksum of the saved file as silently upgraded on opening: the same project
    /// at the same revision in newer bytes, so it holds no unsaved work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upgraded_sha256: Option<String>,
    /// The saved file was upgraded with a notice; until saved, the upgrade is unsaved work.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    unsaved_migration: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryInfo {
    project_id: Uuid,
    name: String,
    revision: u64,
    saved_path: Option<String>,
    edited_at_ms: u64,
}

struct RecoveryCandidate {
    info: RecoveryInfo,
    modified: SystemTime,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    project: Option<Project>,
    path: Option<String>,
    dirty: bool,
    can_undo: bool,
    can_redo: bool,
    history: Option<HistoryView>,
    recovery: Option<RecoveryInfo>,
    recoveries: Vec<RecoveryInfo>,
    recovery_error: Option<String>,
    migration_notice: Option<String>,
    open_projects: Vec<OpenProjectSummary>,
    any_dirty: bool,
    site_warnings: Vec<String>,
    outside_edits: desegni_storage::project_folder::OutsideEdits,
    project_folder: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryView {
    session_id: Uuid,
    start: usize,
    position: usize,
    end: usize,
    version: u64,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SaveProjectOutcome {
    Saved { workspace: Box<WorkspaceView> },
    Cancelled,
    Conflict { conflict: SaveConflict },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveConflict {
    token: Uuid,
    kind: SaveFailureKind,
    path: String,
    file_name: String,
    message: String,
    default_choice: &'static str,
    can_replace: bool,
    details: SaveConflictDetails,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveConflictDetails {
    expected: Option<FileStamp>,
    actual: Option<FileStamp>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveConflictChoice {
    Copy,
    Replace,
    Cancel,
}

enum SaveDestination {
    Current,
    Chosen(ProjectFile),
}

struct PendingSaveConflict {
    token: Uuid,
    project_id: Uuid,
    revision: u64,
    destination: SaveDestination,
    actual: Option<FileStamp>,
    can_replace: bool,
    save_as: bool,
}

pub struct Workspace {
    directory: PathBuf,
    system_project_location: PathBuf,
    session: Option<Session>,
    file: Option<ProjectFile>,
    journal: Option<Journal>,
    ownership: Option<File>,
    saved_revision: Option<u64>,
    recoveries: Vec<RecoveryCandidate>,
    recovery_error: Option<String>,
    prepared: Option<RequirementsSnapshot>,
    migration_notice: Option<String>,
    needs_migration_save: bool,
    recent_error: Option<String>,
    pending_save: Option<PendingSaveConflict>,
    website_import: Option<WebsiteImportSnapshot>,
    /// Other open projects, each with its own undo history, file and recovery journal.
    parked: Vec<OpenProject>,
    /// Open projects in tab order.
    order: Vec<Uuid>,
    /// Reports the active project revision to the browser site server.
    site: Option<crate::local_site::RevisionWatch>,
}

/// A project kept open in the background while another one is active.
struct OpenProject {
    session: Session,
    file: Option<ProjectFile>,
    journal: Option<Journal>,
    ownership: Option<File>,
    saved_revision: Option<u64>,
    migration_notice: Option<String>,
    needs_migration_save: bool,
}

impl OpenProject {
    fn dirty(&self) -> bool {
        Some(self.session.project().revision) != self.saved_revision || self.needs_migration_save
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectSummary {
    id: Uuid,
    name: String,
    dirty: bool,
    active: bool,
    pages: Vec<OpenPageSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPageSummary {
    id: Uuid,
    name: String,
}

impl Workspace {
    pub(super) fn directory(&self) -> &std::path::Path {
        &self.directory
    }
    pub(super) fn system_project_location(&self) -> &std::path::Path {
        &self.system_project_location
    }
    fn project_location(&self) -> NativeResult<PathBuf> {
        crate::project_location::read(&self.directory, &self.system_project_location)
    }
    pub(super) fn project(&self) -> NativeResult<&Project> {
        self.session
            .as_ref()
            .map(Session::project)
            .ok_or_else(|| "Create or open a project first.".into())
    }

    pub(super) fn apply_generated(
        &mut self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        accepted_decision_indices: &[usize],
    ) -> NativeResult<()> {
        let journal = self
            .journal
            .as_ref()
            .ok_or("Recovery is unavailable. Nothing was applied.")?;
        self.session
            .as_mut()
            .ok_or("Create or open a project first.")?
            .apply_generation(snapshot, candidate, accepted_decision_indices, |project| {
                journal.append(project)
            })
            .map_err(message)
    }

    pub(super) fn apply_draft(
        &mut self,
        snapshot: &RequirementsSnapshot,
        draft: &desegni_core::GenerationDraft,
    ) -> NativeResult<()> {
        let journal = self
            .journal
            .as_ref()
            .ok_or("Recovery is unavailable. Nothing was applied.")?;
        self.session
            .as_mut()
            .ok_or("Create or open a project first.")?
            .apply_generation_draft(snapshot, draft, |project| journal.append(project))
            .map_err(message)
    }

    pub(super) fn record_generated(
        &mut self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        attempt_ordinal: usize,
    ) -> NativeResult<()> {
        let journal = self
            .journal
            .as_ref()
            .ok_or("Project recovery is unavailable. The generated result is preserved.")?;
        self.session
            .as_mut()
            .ok_or("Create or open a project first.")?
            .record_generation(snapshot, candidate, attempt_ordinal, |project| {
                journal.append(project)
            })
            .map_err(message)
    }

    pub fn initialize(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let mut workspace = Self::from_directory(app.path().app_local_data_dir()?)?;
        workspace.site = app
            .state::<Mutex<crate::local_site::LocalSite>>()
            .lock()
            .ok()
            .map(|site| site.watch());
        if let Ok(documents) = app.path().document_dir() {
            workspace.system_project_location =
                crate::project_location::default_from_documents(&documents);
        }
        // Otherwise keep from_directory's app-data Projects fallback. A localized
        // or absent Documents directory must not prevent the editor from starting.
        Ok(workspace)
    }

    fn from_directory(directory: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        fs::create_dir_all(&directory)?;
        let mut state = Self {
            system_project_location: directory.join("Projects"),
            directory,
            session: None,
            file: None,
            journal: None,
            ownership: None,
            saved_revision: None,
            recoveries: vec![],
            recovery_error: None,
            prepared: None,
            migration_notice: None,
            needs_migration_save: false,
            recent_error: None,
            pending_save: None,
            website_import: None,
            parked: vec![],
            order: vec![],
            site: None,
        };
        state.refresh_recoveries();
        Ok(state)
    }

    fn pointer_path(&self, project_id: Uuid) -> PathBuf {
        self.directory.join(format!("{project_id}.recovery.json"))
    }

    fn acquire_ownership(&self, project_id: Uuid) -> NativeResult<Option<File>> {
        let path = self.directory.join(format!("{project_id}.session.lock"));
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(message)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(error) => Err(message(error)),
        }
    }

    fn read_recovery(
        &self,
        project_id: Uuid,
    ) -> NativeResult<Option<(DecodedProject, RecoveryPointer)>> {
        let file = File::open(self.pointer_path(project_id)).map_err(message)?;
        if file.metadata().map_err(message)?.len() > 65_536 {
            return Err("The recovery pointer is too large. The original remains on disk.".into());
        }
        let mut bytes = Vec::new();
        file.take(65_537).read_to_end(&mut bytes).map_err(message)?;
        if bytes.len() > 65_536 {
            return Err("The recovery pointer is too large. The original remains on disk.".into());
        }
        let pointer: RecoveryPointer = serde_json::from_slice(&bytes).map_err(message)?;
        if !pointer.journal_file.ends_with(".journal")
            || pointer.journal_file.contains(['/', '\\', ':'])
            || pointer.journal_file.starts_with('.')
        {
            return Err("The recovery pointer contains an invalid journal path.".into());
        }
        let document = Journal::recover_document(&self.directory.join(&pointer.journal_file))
            .map_err(message)?;
        if let Some(document) = &document
            && (document.project.id != project_id
                || pointer
                    .saved_revision
                    .is_some_and(|revision| revision > document.project.revision))
        {
            return Err("The recovery pointer and project identity or revision disagree. The originals remain on disk.".into());
        }
        let Some(document) = document else {
            return Ok(None);
        };
        if Some(document.project.revision) == pointer.saved_revision
            && (pointer.saved_sha256.as_ref() == Some(&document.source_sha256)
                || pointer.upgraded_sha256.as_ref() == Some(&document.source_sha256)
                || self.verify_silent_upgrade(project_id, &document, &pointer))
        {
            return Ok(None);
        }
        Ok(Some((document, pointer)))
    }

    /// Records written before `upgradedSha256` existed: check the saved file itself. If the
    /// journal is that file silently upgraded, remember its checksum so this is checked once.
    fn verify_silent_upgrade(
        &self,
        project_id: Uuid,
        document: &DecodedProject,
        pointer: &RecoveryPointer,
    ) -> bool {
        if pointer.upgraded_sha256.is_some() || pointer.unsaved_migration {
            return false;
        }
        let Some(saved) = pointer
            .project_path
            .as_ref()
            .and_then(|path| desegni_storage::load_project_document(path).ok())
        else {
            return false;
        };
        let clean = saved.project.id == project_id
            && Some(saved.project.revision) == pointer.saved_revision
            && pointer.saved_sha256.as_ref() == Some(&saved.source_sha256)
            && saved.migrated()
            && saved.migration_notice().is_none();
        if clean {
            let pointer = RecoveryPointer {
                upgraded_sha256: Some(document.source_sha256.clone()),
                ..pointer.clone()
            };
            if let Ok(json) = serde_json::to_string(&pointer) {
                let _ = write_markdown(&self.pointer_path(project_id), &json);
            }
        }
        clean
    }

    fn refresh_recoveries(&mut self) {
        let mut candidates = vec![];
        let mut errors = vec![];
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) => {
                self.recovery_error = Some(message(error));
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(message(error));
                    continue;
                }
            };
            let name = entry.file_name();
            let Some(stem) = name
                .to_str()
                .and_then(|name| name.strip_suffix(".recovery.json"))
            else {
                continue;
            };
            let Ok(project_id) = Uuid::parse_str(stem) else {
                continue;
            };
            if stem != project_id.to_string() {
                continue;
            }
            let ownership = match self.acquire_ownership(project_id) {
                Ok(Some(ownership)) => ownership,
                Ok(None) => continue,
                Err(error) => {
                    errors.push(format!("{project_id}: {error}"));
                    continue;
                }
            };
            match self.read_recovery(project_id) {
                Ok(Some((document, pointer))) => {
                    let modified = fs::metadata(self.directory.join(pointer.journal_file))
                        .and_then(|metadata| metadata.modified())
                        .unwrap_or(SystemTime::UNIX_EPOCH);
                    candidates.push(RecoveryCandidate {
                        info: RecoveryInfo {
                            project_id,
                            name: document.project.name,
                            revision: document.project.revision,
                            saved_path: pointer
                                .project_path
                                .as_ref()
                                .map(|path| path.to_string_lossy().into_owned()),
                            edited_at_ms: modified
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis()
                                .try_into()
                                .unwrap_or(u64::MAX),
                        },
                        modified,
                    });
                }
                Ok(None) => {}
                Err(error) => errors.push(format!("{project_id}: {error}")),
            }
            drop(ownership);
        }
        candidates.sort_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| a.info.project_id.cmp(&b.info.project_id))
        });
        self.recoveries = candidates;
        self.recovery_error = if errors.is_empty() {
            None
        } else {
            Some(errors.join("\n"))
        };
    }

    pub(super) fn view(&self) -> WorkspaceView {
        // Every published view reports the active revision, which is how the browser
        // site server learns that the served project changed.
        if let Some(site) = &self.site {
            site.observe(
                self.session
                    .as_ref()
                    .map(|session| (session.project().id, session.project().revision)),
            );
        }
        WorkspaceView {
            project: self.session.as_ref().map(|s| s.project().clone()),
            path: self
                .file
                .as_ref()
                .map(|f| f.path().to_string_lossy().into_owned()),
            dirty: self.session.as_ref().is_some_and(|s| {
                Some(s.project().revision) != self.saved_revision || self.needs_migration_save
            }),
            can_undo: self.session.as_ref().is_some_and(Session::can_undo),
            can_redo: self.session.as_ref().is_some_and(Session::can_redo),
            history: self.session.as_ref().map(|session| {
                let (session_id, start, position, end, version) = session.history();
                HistoryView {
                    session_id,
                    start,
                    position,
                    end,
                    version,
                }
            }),
            recovery: self
                .recoveries
                .first()
                .map(|candidate| candidate.info.clone()),
            recoveries: self
                .recoveries
                .iter()
                .map(|candidate| candidate.info.clone())
                .collect(),
            recovery_error: self.recovery_error.clone(),
            migration_notice: self.migration_notice.clone(),
            open_projects: self.open_projects(),
            any_dirty: self.parked.iter().any(OpenProject::dirty) || self.active_dirty(),
            outside_edits: self
                .file
                .as_ref()
                .map(ProjectFile::outside_edits)
                .unwrap_or_default(),
            site_warnings: self
                .file
                .as_ref()
                .map(ProjectFile::site_warnings)
                .unwrap_or_default(),
            project_folder: self
                .file
                .as_ref()
                .is_some_and(ProjectFile::is_project_folder),
        }
    }

    fn check_replace(&self, discard: bool) -> NativeResult<()> {
        if self.view().dirty && !discard {
            return Err(
                "Save your current project or explicitly discard its unsaved changes first.".into(),
            );
        }
        Ok(())
    }

    fn active_dirty(&self) -> bool {
        self.session.as_ref().is_some_and(|session| {
            Some(session.project().revision) != self.saved_revision || self.needs_migration_save
        })
    }

    /// Back to the start screen. The project lock is released; its recovery journal stays,
    /// exactly as when another project replaces it.
    /// "Discard and continue": the unsaved work is no longer offered for recovery and no longer
    /// blocks opening the saved file. Its journal stays on disk; only the pointer moves aside.
    fn set_aside_recovery(&mut self) -> NativeResult<()> {
        let Some(id) = self.session.as_ref().map(|session| session.project().id) else {
            return Ok(());
        };
        self.discard_recovery_pointer(id)
    }

    fn discard_recovery_pointer(&self, id: Uuid) -> NativeResult<()> {
        let pointer = self.pointer_path(id);
        if pointer.exists() {
            fs::rename(
                &pointer,
                self.directory.join(format!("{id}.recovery.discarded.json")),
            )
            .map_err(message)?;
        }
        Ok(())
    }

    /// Close the active project and activate the next open one in tab order, or the start
    /// screen when no project remains.
    fn close(&mut self) {
        let position = self.session.as_ref().and_then(|session| {
            let id = session.project().id;
            self.order.iter().position(|other| *other == id)
        });
        if let Some(session) = &self.session {
            let id = session.project().id;
            self.order.retain(|other| *other != id);
        }
        self.session = None;
        self.file = None;
        self.journal = None;
        self.ownership = None;
        self.saved_revision = None;
        self.prepared = None;
        self.migration_notice = None;
        self.needs_migration_save = false;
        self.pending_save = None;
        // The tab that followed the closed one, or the one before it when it was last.
        let next = position
            .and_then(|index| {
                self.order
                    .get(index)
                    .or_else(|| index.checked_sub(1).and_then(|last| self.order.get(last)))
            })
            .copied();
        if let Some(index) = next.and_then(|id| {
            self.parked
                .iter()
                .position(|open| open.session.project().id == id)
        }) {
            let project = self.parked.remove(index);
            self.unpark(project);
        }
        self.refresh_recoveries();
    }

    /// Close one open project tab. Unsaved changes need an explicit discard; discarding keeps
    /// the recovery journal on disk and only stops offering it, as closing the active project does.
    fn close_tab(&mut self, project_id: Option<Uuid>, discard: bool) -> NativeResult<()> {
        let Some(target) = project_id.or_else(|| self.session.as_ref().map(|s| s.project().id))
        else {
            return Ok(());
        };
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.project().id == target)
        {
            if self.active_dirty() {
                self.check_replace(discard)?;
                self.set_aside_recovery()?;
            }
            self.close();
            return Ok(());
        }
        let index = self
            .parked
            .iter()
            .position(|open| open.session.project().id == target)
            .ok_or("That project is no longer open.")?;
        let project = self.parked.remove(index);
        if project.dirty() {
            if !discard {
                self.parked.insert(index, project);
                return Err(
                    "Save this project or explicitly discard its unsaved changes first.".into(),
                );
            }
            if let Err(error) = self.discard_recovery_pointer(target) {
                self.parked.insert(index, project);
                return Err(error);
            }
        }
        // Dropping the entry releases its lock; its recovery journal stays on disk.
        drop(project);
        self.order.retain(|other| *other != target);
        self.refresh_recoveries();
        Ok(())
    }

    fn open_projects(&self) -> Vec<OpenProjectSummary> {
        let summary = |project: &Project, dirty: bool, active: bool| OpenProjectSummary {
            id: project.id,
            name: project.name.clone(),
            dirty,
            active,
            pages: project
                .pages
                .iter()
                .map(|page| OpenPageSummary {
                    id: page.id,
                    name: page.name.clone(),
                })
                .collect(),
        };
        self.order
            .iter()
            .filter_map(|id| {
                if let Some(session) = self.session.as_ref().filter(|s| s.project().id == *id) {
                    let dirty = self.active_dirty();
                    return Some(summary(session.project(), dirty, true));
                }
                self.parked
                    .iter()
                    .find(|open| open.session.project().id == *id)
                    .map(|open| summary(open.session.project(), open.dirty(), false))
            })
            .collect()
    }

    /// Keep the active project open in the background so another one can become active.
    fn park_active(&mut self) {
        if let Some(session) = self.session.take() {
            self.parked.push(OpenProject {
                session,
                file: self.file.take(),
                journal: self.journal.take(),
                ownership: self.ownership.take(),
                saved_revision: self.saved_revision.take(),
                migration_notice: self.migration_notice.take(),
                needs_migration_save: std::mem::take(&mut self.needs_migration_save),
            });
        }
        self.prepared = None;
        self.pending_save = None;
    }

    fn unpark(&mut self, project: OpenProject) {
        self.session = Some(project.session);
        self.file = project.file;
        self.journal = project.journal;
        self.ownership = project.ownership;
        self.saved_revision = project.saved_revision;
        self.migration_notice = project.migration_notice;
        self.needs_migration_save = project.needs_migration_save;
        self.prepared = None;
        self.pending_save = None;
    }

    fn switch_to(&mut self, project_id: Uuid) -> NativeResult<()> {
        if self
            .session
            .as_ref()
            .is_some_and(|s| s.project().id == project_id)
        {
            return Ok(());
        }
        let index = self
            .parked
            .iter()
            .position(|open| open.session.project().id == project_id)
            .ok_or("That project is no longer open.")?;
        let project = self.parked.remove(index);
        self.park_active();
        self.unpark(project);
        Ok(())
    }

    /// Open or create another project in a new tab group. If that fails, the previous
    /// project becomes active again.
    fn open_tab<T, E>(&mut self, open: impl FnOnce(&mut Self) -> Result<T, E>) -> Result<T, E> {
        let previous = self.session.as_ref().map(|s| s.project().id);
        self.park_active();
        match open(self) {
            Ok(value) => Ok(value),
            Err(error) => {
                if self.session.is_none()
                    && let Some(id) = previous
                    && let Some(index) = self
                        .parked
                        .iter()
                        .position(|open| open.session.project().id == id)
                {
                    let project = self.parked.remove(index);
                    self.unpark(project);
                }
                Err(error)
            }
        }
    }

    fn open_document(
        &mut self,
        file: ProjectFile,
        document: DecodedProject,
        discard_recovery: bool,
    ) -> OpenResult<()> {
        let migration_notice = document.migration_notice();
        let revision = document.project.revision;
        self.start_with_recovery(
            document.project,
            Some(file),
            Some(revision),
            discard_recovery,
        )?;
        if let Some(notice) = migration_notice {
            self.note_migration(notice)?;
        }
        self.remember_recent();
        Ok(())
    }

    /// Opens a project file in its own tab. Choosing the active project's own file again reloads
    /// it from disk with fresh history, unless it has unsaved edits, which are kept as they are.
    fn open_file_tab(&mut self, path: &std::path::Path, discard_recovery: bool) -> OpenResult<()> {
        if path.is_dir() {
            if let Some(file_path) = desegni_storage::project_in_folder(path).map_err(message)? {
                let document =
                    desegni_storage::load_project_document(&file_path).map_err(message)?;
                if !path.join(".desegnos-site.json").exists() {
                    desegni_storage::manage_project_folder(
                        &file_path,
                        document.project.id,
                        Default::default(),
                    )
                    .map_err(message)?;
                }
                return self.open_file_tab(&file_path, discard_recovery);
            }
            let file = desegni_storage::import_detected_website(path, &self.project_location()?)
                .map_err(message)?;
            let document = file.load_document().map_err(message)?;
            return self.open_tab(|state| state.open_document(file, document, discard_recovery));
        }
        let active = self.file.as_ref().is_some_and(|file| {
            file.path() == path || path.canonicalize().ok().as_deref() == Some(file.path())
        });
        match (active, self.view().dirty) {
            (true, false) => self.open_path(path, discard_recovery),
            (true, true) => Ok(()),
            (false, _) => self.open_tab(|state| state.open_path(path, discard_recovery)),
        }
    }

    fn open_path(&mut self, path: &std::path::Path, discard_recovery: bool) -> OpenResult<()> {
        let canonical = path.canonicalize().ok();
        if let Some(index) = self.parked.iter().position(|open| {
            open.file.as_ref().is_some_and(|file| {
                file.path() == path || canonical.as_deref() == Some(file.path())
            })
        }) {
            let project = self.parked.remove(index);
            self.park_active();
            self.unpark(project);
            return Ok(());
        }
        let (file, document) = if let Some(current) = self.file.as_ref().filter(|file| {
            file.path() == path || path.canonicalize().ok().as_deref() == Some(file.path())
        }) {
            current.reopen().map_err(message)?
        } else {
            let file = ProjectFile::acquire(path).map_err(message)?;
            let document = file.load_document().map_err(message)?;
            (file, document)
        };
        self.open_document(file, document, discard_recovery)
    }

    /// The active project was upgraded from its saved file with a notice: until it is saved,
    /// the upgrade itself is unsaved work and stays offered for recovery.
    fn note_migration(&mut self, notice: &str) -> NativeResult<()> {
        self.migration_notice = Some(notice.into());
        self.needs_migration_save = true;
        if let (Some(session), Some(journal), Some(file)) =
            (&self.session, &self.journal, &self.file)
        {
            self.write_pointer(
                session.project().id,
                journal,
                Some(file),
                self.saved_revision,
                None,
                true,
            )?;
        }
        Ok(())
    }

    fn remember_recent(&mut self) {
        if let (Some(file), Some(session)) = (&self.file, &self.session) {
            self.recent_error =
                crate::recent::remember(&self.directory, file.path(), &session.project().name)
                    .err();
        }
    }

    fn start(
        &mut self,
        project: Project,
        file: Option<ProjectFile>,
        saved_revision: Option<u64>,
    ) -> NativeResult<()> {
        self.start_with_recovery(project, file, saved_revision, false)
            .map_err(String::from)
    }

    fn start_with_recovery(
        &mut self,
        project: Project,
        file: Option<ProjectFile>,
        saved_revision: Option<u64>,
        discard_recovery: bool,
    ) -> OpenResult<()> {
        if let Some(index) = self
            .parked
            .iter()
            .position(|open| open.session.project().id == project.id)
        {
            let open = self.parked.remove(index);
            self.park_active();
            self.unpark(open);
            return Ok(());
        }
        let same_project = self
            .session
            .as_ref()
            .is_some_and(|session| session.project().id == project.id);
        let ownership = if same_project {
            None
        } else {
            let ownership = self
                .acquire_ownership(project.id)?
                .ok_or("This project is already active in another Desegnos window.")?;
            if self.pointer_path(project.id).exists() {
                match self.read_recovery(project.id) {
                    Ok(Some((recovered, _))) if recovered.project != project => {
                        if !discard_recovery {
                            return Err(ProjectOpenError::Recovery {
                                code: "pendingRecovery",
                                project_id: project.id,
                                path: file
                                    .as_ref()
                                    .map(|file| file.path().to_path_buf())
                                    .ok_or("This project has no saved file to open.")?,
                            });
                        }
                        // Ownership stays held across dismissal and activation. Use the same
                        // non-destructive pointer archival as dismiss_recovery.
                        self.discard_recovery_pointer(project.id)?;
                        self.refresh_recoveries();
                    }
                    Ok(_) => {}
                    Err(error) => {
                        return Err(format!(
                            "This project's recovery record needs attention before it can be replaced: {error}"
                        ).into());
                    }
                }
            }
            Some(ownership)
        };
        self.activate(project, file, saved_revision, ownership, None)
            .map_err(Into::into)
    }

    fn activate(
        &mut self,
        project: Project,
        file: Option<ProjectFile>,
        saved_revision: Option<u64>,
        ownership: Option<File>,
        last_saved_path: Option<PathBuf>,
    ) -> NativeResult<()> {
        let session = Session::new(project).map_err(message)?;
        let journal = Journal::new(&self.directory, session.project().id).map_err(message)?;
        journal.append(session.project()).map_err(message)?;
        self.write_pointer(
            session.project().id,
            &journal,
            file.as_ref(),
            saved_revision,
            last_saved_path,
            false,
        )?;
        self.pending_save = None;
        if !self.order.contains(&session.project().id) {
            self.order.push(session.project().id);
        }
        self.session = Some(session);
        self.file = file;
        self.journal = Some(journal);
        if let Some(ownership) = ownership {
            self.ownership = Some(ownership);
        }
        self.saved_revision = saved_revision;
        self.prepared = None;
        self.migration_notice = None;
        self.needs_migration_save = false;
        self.refresh_recoveries();
        Ok(())
    }

    fn write_pointer(
        &self,
        project_id: Uuid,
        journal: &Journal,
        file: Option<&ProjectFile>,
        saved_revision: Option<u64>,
        last_saved_path: Option<PathBuf>,
        unsaved_migration: bool,
    ) -> NativeResult<()> {
        let saved = file
            .map(|file| -> NativeResult<DecodedProject> {
                let saved = file.saved_document().map_err(message)?;
                if saved.project.id != project_id || Some(saved.project.revision) != saved_revision {
                    return Err("The saved file changed before its recovery state could be recorded. The project remains unsaved.".into());
                }
                Ok(saved)
            })
            .transpose()?;
        // A silently upgraded file is journaled in newer bytes; remember them as saved too.
        let upgraded_sha256 = match &saved {
            Some(saved) if saved.migrated() && !unsaved_migration => {
                Journal::recover_document(journal.path())
                    .map_err(message)?
                    .filter(|entry| Some(entry.project.revision) == saved_revision)
                    .map(|entry| entry.source_sha256)
            }
            _ => None,
        };
        let pointer = RecoveryPointer {
            journal_file: journal
                .path()
                .file_name()
                .ok_or("Invalid recovery filename.")?
                .to_string_lossy()
                .into_owned(),
            saved_revision,
            saved_sha256: saved.map(|saved| saved.source_sha256),
            project_path: file.map(|f| f.path().to_owned()).or(last_saved_path),
            upgraded_sha256,
            unsaved_migration,
        };
        write_markdown(
            &self.pointer_path(project_id),
            &serde_json::to_string(&pointer).map_err(message)?,
        )
        .map_err(message)
    }

    fn check_snapshot(&self, project: &Project) -> NativeResult<()> {
        let current = self
            .session
            .as_ref()
            .ok_or("The project was closed.")?
            .project();
        if current.id != project.id || current.revision != project.revision {
            return Err("The project changed while the file dialog was open. Save again from the current project.".into());
        }
        Ok(())
    }

    fn save_destination(&self, path: Option<PathBuf>) -> NativeResult<SaveDestination> {
        let Some(path) = path else {
            return if self.file.is_some() {
                Ok(SaveDestination::Current)
            } else {
                let project = self.project()?;
                let path = desegni_storage::create_project_folder(
                    &self.project_location()?,
                    &project.name,
                    project.id,
                )
                .map_err(message)?;
                Ok(SaveDestination::Chosen(
                    ProjectFile::acquire(&path).map_err(message)?,
                ))
            };
        };
        let path = if path.extension().is_none() {
            path.with_extension("desegnos")
        } else {
            path
        };
        let current = self.file.as_ref().is_some_and(|file| {
            file.path() == path || path.canonicalize().ok().as_deref() == Some(file.path())
        });
        if current {
            Ok(SaveDestination::Current)
        } else {
            Ok(SaveDestination::Chosen(
                ProjectFile::acquire(&path).map_err(message)?,
            ))
        }
    }

    fn destination_file<'a>(
        &'a self,
        destination: &'a SaveDestination,
    ) -> NativeResult<&'a ProjectFile> {
        match destination {
            SaveDestination::Current => self
                .file
                .as_ref()
                .ok_or_else(|| "Choose a project file first.".into()),
            SaveDestination::Chosen(file) => Ok(file),
        }
    }

    fn finish_save(&mut self, project: &Project, destination: SaveDestination) -> NativeResult<()> {
        self.write_pointer(
            project.id,
            self.journal.as_ref().ok_or("Recovery is unavailable.")?,
            Some(self.destination_file(&destination)?),
            Some(project.revision),
            None,
            false,
        )?;
        if let SaveDestination::Chosen(file) = destination {
            self.file = Some(file);
        }
        self.saved_revision = Some(project.revision);
        self.migration_notice = None;
        self.needs_migration_save = false;
        self.remember_recent();
        Ok(())
    }

    fn create_in_folder(&mut self, project: Project, parent: Option<PathBuf>) -> NativeResult<()> {
        let parent = parent.map(Ok).unwrap_or_else(|| self.project_location())?;
        let path = desegni_storage::create_project_folder(&parent, &project.name, project.id)
            .map_err(message)?;
        let file = ProjectFile::acquire(&path).map_err(message)?;
        file.save(&project).map_err(message)?;
        let revision = project.revision;
        self.open_tab(|state| state.start(project, Some(file), Some(revision)))?;
        self.remember_recent();
        Ok(())
    }

    fn save_snapshot(&mut self, project: &Project, path: Option<PathBuf>) -> NativeResult<()> {
        self.check_snapshot(project)?;
        self.pending_save = None;
        let destination = self.save_destination(path)?;
        self.destination_file(&destination)?
            .save(project)
            .map_err(message)?;
        self.finish_save(project, destination)
    }

    fn save_conflict(
        &mut self,
        project: &Project,
        destination: SaveDestination,
        failure: SaveFailure,
        save_as: bool,
    ) -> NativeResult<SaveProjectOutcome> {
        if !matches!(
            failure.kind,
            SaveFailureKind::Changed
                | SaveFailureKind::DifferentProject
                | SaveFailureKind::Unreadable
        ) {
            return Err(failure.message);
        }
        let file = self.destination_file(&destination)?;
        let path = file.path().to_string_lossy().into_owned();
        let file_name = file
            .path()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "website.desegnos".into());
        let different = failure
            .actual
            .as_ref()
            .is_some_and(|actual| actual.project_id != project.id);
        let can_replace = failure.actual.is_some() && (!different || save_as);
        let message = if different && save_as {
            format!("{file_name} is a different project. Replace it?")
        } else {
            failure.message
        };
        let token = Uuid::new_v4();
        let conflict = SaveConflict {
            token,
            kind: if different {
                SaveFailureKind::DifferentProject
            } else {
                failure.kind
            },
            path,
            file_name,
            message,
            default_choice: if different { "cancel" } else { "copy" },
            can_replace,
            details: SaveConflictDetails {
                expected: failure.expected,
                actual: failure.actual.clone(),
            },
        };
        self.pending_save = Some(PendingSaveConflict {
            token,
            project_id: project.id,
            revision: project.revision,
            destination,
            actual: failure.actual,
            can_replace,
            save_as,
        });
        Ok(SaveProjectOutcome::Conflict { conflict })
    }

    fn save_snapshot_checked(
        &mut self,
        project: &Project,
        path: Option<PathBuf>,
        save_as: bool,
    ) -> NativeResult<SaveProjectOutcome> {
        self.check_snapshot(project)?;
        self.pending_save = None;
        let destination = self.save_destination(path)?;
        match self.destination_file(&destination)?.save_checked(project) {
            Ok(()) => {
                self.finish_save(project, destination)?;
                Ok(SaveProjectOutcome::Saved {
                    workspace: Box::new(self.view()),
                })
            }
            Err(failure) => self.save_conflict(project, destination, *failure, save_as),
        }
    }

    fn take_save_conflict(&mut self, token: Uuid) -> NativeResult<PendingSaveConflict> {
        let pending = self
            .pending_save
            .as_ref()
            .filter(|pending| pending.token == token)
            .ok_or("This save choice expired. Save again.")?;
        let current = self.project()?;
        if current.id != pending.project_id || current.revision != pending.revision {
            return Err("The project changed while this save choice was open. Save again.".into());
        }
        self.pending_save
            .take()
            .ok_or_else(|| "This save choice expired. Save again.".into())
    }

    fn cancel_save_conflict(&mut self, token: Uuid) -> NativeResult<()> {
        if self
            .pending_save
            .as_ref()
            .is_none_or(|pending| pending.token != token)
        {
            return Err("This save choice expired. Save again.".into());
        }
        self.pending_save = None;
        Ok(())
    }

    fn replace_save_conflict(&mut self, token: Uuid) -> NativeResult<SaveProjectOutcome> {
        let pending = self.take_save_conflict(token)?;
        if !pending.can_replace {
            return Err("This file cannot be replaced. Save a copy with a new name.".into());
        }
        let project = self.project()?.clone();
        let observed = pending
            .actual
            .as_ref()
            .ok_or("This file cannot be replaced. Save a copy with a new name.")?;
        match self.destination_file(&pending.destination)?.save_replacing(
            &project,
            observed,
            pending.save_as,
        ) {
            Ok(()) => {
                self.finish_save(&project, pending.destination)?;
                Ok(SaveProjectOutcome::Saved {
                    workspace: Box::new(self.view()),
                })
            }
            Err(failure) => {
                self.save_conflict(&project, pending.destination, *failure, pending.save_as)
            }
        }
    }

    fn dismiss_recovery(&mut self, project_id: Uuid) -> NativeResult<()> {
        if self
            .open_projects()
            .iter()
            .any(|open| open.id == project_id)
        {
            return Err("Close this project before dismissing its recovery.".into());
        }
        let _ownership = self
            .acquire_ownership(project_id)?
            .ok_or("This project is already active in another Desegnos window.")?;
        self.read_recovery(project_id)?
            .ok_or("There are no unsaved changes to recover for this project.")?;
        self.discard_recovery_pointer(project_id)?;
        self.refresh_recoveries();
        Ok(())
    }

    fn recover(&mut self, project_id: Option<Uuid>, discard: bool) -> NativeResult<()> {
        let _ = discard; // Recovery opens in its own tab group; the active project stays open.
        self.open_tab(|state| state.recover_inner(project_id))
    }

    fn recover_inner(&mut self, project_id: Option<Uuid>) -> NativeResult<()> {
        self.refresh_recoveries();
        let project_id = project_id
            .or_else(|| {
                self.recoveries
                    .first()
                    .map(|candidate| candidate.info.project_id)
            })
            .ok_or("There are no unsaved changes to recover.")?;
        if !self
            .recoveries
            .iter()
            .any(|candidate| candidate.info.project_id == project_id)
        {
            return Err(
                "This recovery is no longer available. It may be active in another Desegnos window."
                    .into(),
            );
        }
        let ownership = self
            .acquire_ownership(project_id)?
            .ok_or("This project is already active in another Desegnos window.")?;
        let (document, pointer) = self
            .read_recovery(project_id)?
            .ok_or("There are no unsaved changes to recover for this project.")?;
        let mut migration_notice = document.migration_notice();
        let project = document.project;
        let file = pointer.project_path.as_ref().and_then(|path| {
            let file = ProjectFile::acquire(path).ok()?;
            let saved = file.load_document().ok()?;
            let matches = saved.project.id == project.id
                && Some(saved.project.revision) == pointer.saved_revision
                && Some(&saved.source_sha256) == pointer.saved_sha256.as_ref();
            if matches {
                migration_notice = migration_notice.or(saved.migration_notice());
            }
            matches.then_some(file)
        });
        let saved_revision = file.as_ref().and(pointer.saved_revision);
        self.activate(
            project,
            file,
            saved_revision,
            Some(ownership),
            pointer.project_path,
        )?;
        if let Some(notice) = migration_notice {
            self.note_migration(notice)?;
        }
        Ok(())
    }
}

#[tauri::command]
pub fn get_workspace(state: WorkspaceState<'_>) -> NativeResult<WorkspaceView> {
    let mut state = state.lock().map_err(message)?;
    state.refresh_recoveries();
    Ok(state.view())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn create_project(
    state: WorkspaceState<'_>,
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
    name: String,
    viewport: Viewport,
    appearance: Option<Appearance>,
    discard: bool,
    identity: State<'_, Mutex<crate::identity::IdentityService>>,
    identity_analysis_id: Option<Uuid>,
    identity_finding_indices: Option<Vec<usize>>,
    identity_asset_ids: Option<Vec<Uuid>>,
    identity_photo_ids: Option<Vec<Uuid>>,
    identity_item_indices: Option<Vec<usize>>,
    identity_page_paths: Option<Vec<String>>,
    parent_directory: Option<PathBuf>,
) -> NativeResult<WorkspaceView> {
    // New projects follow the editor's light/dark mode; older callers omit it and start light.
    let appearance = appearance.unwrap_or(Appearance::Light);
    generation
        .lock()
        .map_err(message)?
        .ensure_switch_allowed()?;
    let _ = discard; // New projects open in their own tab group; nothing is replaced.
    let mut state = state.lock().map_err(message)?;
    let mut project = if let Some(id) = identity_analysis_id {
        let service = identity.lock().map_err(message)?;
        let context = service.reviewed_context_selected(
            id,
            &identity_finding_indices.unwrap_or_default(),
            &identity_asset_ids.unwrap_or_default(),
            identity_photo_ids.as_deref(),
            identity_item_indices.as_deref(),
            identity_page_paths.as_deref(),
        )?;
        service.project_with_context(context, name, viewport)?
    } else {
        if identity_page_paths
            .as_ref()
            .is_some_and(|paths| !paths.is_empty())
            || identity_photo_ids
                .as_ref()
                .is_some_and(|values| !values.is_empty())
            || identity_item_indices
                .as_ref()
                .is_some_and(|values| !values.is_empty())
            || identity_finding_indices
                .as_ref()
                .is_some_and(|values| !values.is_empty())
            || identity_asset_ids
                .as_ref()
                .is_some_and(|values| !values.is_empty())
        {
            return Err(
                "Choose a completed identity analysis before importing its findings or images."
                    .into(),
            );
        }
        Project::new(name, viewport, appearance).map_err(message)?
    };
    project.appearance = appearance;
    for page in &mut project.pages {
        page.appearance = appearance;
    }
    state.create_in_folder(project, parent_directory)?;
    Ok(state.view())
}

#[tauri::command]
pub fn switch_project(
    state: WorkspaceState<'_>,
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
    project_id: Uuid,
) -> NativeResult<WorkspaceView> {
    generation
        .lock()
        .map_err(message)?
        .ensure_switch_allowed()?;
    let mut state = state.lock().map_err(message)?;
    state.switch_to(project_id)?;
    Ok(state.view())
}

#[tauri::command]
pub fn close_project(
    state: WorkspaceState<'_>,
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
    project_id: Option<Uuid>,
    discard: bool,
) -> NativeResult<WorkspaceView> {
    generation
        .lock()
        .map_err(message)?
        .ensure_switch_allowed()?;
    let mut state = state.lock().map_err(message)?;
    state.close_tab(project_id, discard)?;
    Ok(state.view())
}

/// Import context: merge the reviewed findings and logos of a completed analysis into the
/// open project as one undoable change. Only a native analysis record is accepted.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn import_context(
    state: WorkspaceState<'_>,
    identity: State<'_, Mutex<crate::identity::IdentityService>>,
    revision: u64,
    identity_analysis_id: Uuid,
    identity_finding_indices: Vec<usize>,
    identity_asset_ids: Vec<Uuid>,
    identity_photo_ids: Option<Vec<Uuid>>,
    identity_item_indices: Option<Vec<usize>>,
    identity_page_paths: Option<Vec<String>>,
    attach_matching_pages: Option<bool>,
) -> NativeResult<WorkspaceView> {
    let mut context = identity
        .lock()
        .map_err(message)?
        .reviewed_context_selected(
            identity_analysis_id,
            &identity_finding_indices,
            &identity_asset_ids,
            identity_photo_ids.as_deref(),
            identity_item_indices.as_deref(),
            identity_page_paths.as_deref(),
        )?;
    context.attach_matching_pages = attach_matching_pages.unwrap_or(true);
    let mut state = state.lock().map_err(message)?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let journal = journal.as_ref().ok_or("Create or open a project first.")?;
    session
        .as_mut()
        .ok_or("Create or open a project first.")?
        .import_context(revision, context, |project| journal.append(project))
        .map_err(message)?;
    state.prepared = None;
    Ok(state.view())
}

#[tauri::command]
pub fn edit_project(
    state: WorkspaceState<'_>,
    revision: u64,
    command: Command,
) -> NativeResult<WorkspaceView> {
    let mut state = state.lock().map_err(message)?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let journal = journal.as_ref().ok_or("Create or open a project first.")?;
    session
        .as_mut()
        .ok_or("Create or open a project first.")?
        .execute(revision, command, |p| journal.append(p))
        .map_err(message)?;
    Ok(state.view())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementEditView {
    workspace: WorkspaceView,
    element: Option<desegni_core::ElementTarget>,
}

#[tauri::command]
pub fn edit_page_source(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    revision: u64,
    html: Option<String>,
    css: Option<String>,
    javascript: Option<String>,
) -> desegni_core::SourceEditResult<WorkspaceView> {
    let mut state = state.lock().map_err(|_| {
        desegni_core::SourceEditError::general(
            "The workspace is busy. Keep your text and try again.",
        )
    })?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let session = session
        .as_mut()
        .ok_or_else(|| desegni_core::SourceEditError::general("Open a project first."))?;
    if session.project().id != project_id {
        return Err(desegni_core::SourceEditError::general(
            "Return to the project that owns this page.",
        ));
    }
    let journal = journal.as_ref().ok_or_else(|| {
        desegni_core::SourceEditError::general(
            "Recovery is unavailable. Keep your text; nothing was changed.",
        )
    })?;
    session.edit_page_source(
        revision,
        page_id,
        &desegni_core::PageSourceEdit {
            html,
            css,
            javascript,
        },
        |project| journal.append(project),
    )?;
    state.prepared = None;
    Ok(state.view())
}

#[tauri::command]
pub fn open_found_source(
    window: tauri::WebviewWindow,
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    target: desegni_core::ElementTarget,
) -> NativeResult<()> {
    if window.label() != "editor" {
        return Err("Only the editor can open found-content sources.".into());
    }
    let url = {
        let state = state.lock().map_err(message)?;
        let project = state.project()?;
        if project.id != project_id {
            return Err("Return to the project that owns this item.".into());
        }
        project
            .found_source(page_id, &target)
            .map_err(message)?
            .ok_or("This item has no online source.")?
    };
    crate::system_open::open(url).map_err(|_| {
        "Couldn't open a browser. Copy the source address and open it manually.".into()
    })
}

/// A web link the user clicked in a preview, opened in their browser after they confirm it.
#[tauri::command]
pub fn open_preview_link(window: tauri::WebviewWindow, url: String) -> NativeResult<()> {
    if window.label() != "editor" {
        return Err("Only the editor can open links.".into());
    }
    let parsed = preview_link(&url)?;
    crate::system_open::open(parsed.as_str())
        .map_err(|_| "Couldn't open a browser. Copy the link and open it manually.".into())
}

fn preview_link(url: &str) -> NativeResult<desegni_ai::WebUrl> {
    let parsed =
        desegni_ai::WebUrl::parse(url).map_err(|_| "This link is not a valid web address.")?;
    if url.len() > 2048
        || !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("Only ordinary web links can be opened from the preview.".into());
    }
    Ok(parsed)
}

#[tauri::command]
pub fn get_page_source_map(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
) -> desegni_core::SourceEditResult<desegni_core::PageSourceMap> {
    let state = state
        .lock()
        .map_err(|_| desegni_core::SourceEditError::general("The workspace is unavailable."))?;
    let project = state
        .project()
        .map_err(desegni_core::SourceEditError::general)?;
    if project.id != project_id {
        return Err(desegni_core::SourceEditError::general(
            "Return to the project that owns this page.",
        ));
    }
    project.page_source_map(page_id)
}

/// Every iframe on a page, with its editor ID and resolved target document.
#[tauri::command]
pub fn get_frame_targets(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
) -> NativeResult<Vec<desegni_core::PageFrameTarget>> {
    let state = state.lock().map_err(message)?;
    let project = state.project()?;
    if project.id != project_id {
        return Err("Return to the project that owns this page.".into());
    }
    project.frame_targets(page_id).map_err(message)
}

#[tauri::command]
pub fn inspect_element(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    target: desegni_core::ElementTarget,
    document: Option<desegni_core::DocumentTarget>,
) -> NativeResult<desegni_core::ElementDetails> {
    let state = state.lock().map_err(message)?;
    let project = state
        .session
        .as_ref()
        .ok_or("Open a project first.")?
        .project();
    if project.id != project_id {
        return Err("Return to the project that owns this element.".into());
    }
    let document = document.unwrap_or(desegni_core::DocumentTarget::Page { page_id });
    project
        .inspect_element_in(&document, &target)
        .map_err(message)
}

#[tauri::command]
pub fn edit_element(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    revision: u64,
    target: desegni_core::ElementTarget,
    change: desegni_core::ElementEdit,
    document: Option<desegni_core::DocumentTarget>,
) -> NativeResult<ElementEditView> {
    let mut state = state.lock().map_err(message)?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let session = session.as_mut().ok_or("Open a project first.")?;
    if session.project().id != project_id {
        return Err("Return to the project that owns this element.".into());
    }
    let journal = journal.as_ref().ok_or("Open a project first.")?;
    let document = document.unwrap_or(desegni_core::DocumentTarget::Page { page_id });
    let element = session
        .edit_element_document(revision, &document, &target, &change, |next| {
            journal.append(next)
        })
        .map_err(message)?;
    Ok(ElementEditView {
        workspace: state.view(),
        element,
    })
}

/// Steps the selected element back to its previous version; other changes stay.
#[tauri::command]
pub fn undo_element(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    revision: u64,
    target: desegni_core::ElementTarget,
    document: Option<desegni_core::DocumentTarget>,
) -> NativeResult<ElementEditView> {
    let mut state = state.lock().map_err(message)?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let session = session.as_mut().ok_or("Open a project first.")?;
    if session.project().id != project_id {
        return Err("Return to the project that owns this element.".into());
    }
    let journal = journal.as_ref().ok_or("Open a project first.")?;
    let document = document.unwrap_or(desegni_core::DocumentTarget::Page { page_id });
    let element = session
        .undo_element_document(revision, &document, &target, |next| journal.append(next))
        .map_err(message)?;
    Ok(ElementEditView {
        workspace: state.view(),
        element,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerPasteView {
    workspace: WorkspaceView,
    selection: desegni_core::LayerPasteSelection,
}

#[tauri::command]
pub async fn copy_layer(
    app: AppHandle,
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    revision: u64,
    target: desegni_core::LayerTarget,
    visual_styles: Vec<desegni_core::ElementStyleEdit>,
    document: Option<desegni_core::DocumentTarget>,
) -> NativeResult<()> {
    let document = document.unwrap_or(desegni_core::DocumentTarget::Page { page_id });
    let packet = {
        let state = state.lock().map_err(message)?;
        let project = state
            .session
            .as_ref()
            .ok_or("Open a project first.")?
            .project();
        if project.id != project_id || project.revision != revision {
            return Err("The project changed before the layer was copied. Select it again.".into());
        }
        project
            .copy_layer_in(&document, &target, &visual_styles)
            .map_err(message)?
            .to_json()
            .map_err(message)?
    };
    // OS clipboard owners can need their GUI thread to answer a transfer.
    // Never hold the workspace lock or use the GUI thread while waiting.
    tauri::async_runtime::spawn_blocking(move || crate::clipboard::write(&app, packet))
        .await
        .map_err(message)?
}

#[tauri::command]
pub async fn cut_layer(
    app: AppHandle,
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    revision: u64,
    target: desegni_core::LayerTarget,
    visual_styles: Vec<desegni_core::ElementStyleEdit>,
    document: Option<desegni_core::DocumentTarget>,
) -> NativeResult<WorkspaceView> {
    // Keep the original unless the OS has accepted the complete clipboard
    // packet. A concurrent edit while the transfer runs must never be cut.
    copy_layer(
        app,
        state.clone(),
        project_id,
        page_id,
        revision,
        target.clone(),
        visual_styles,
        document.clone(),
    )
    .await?;
    let document = document.unwrap_or(desegni_core::DocumentTarget::Page { page_id });
    let mut state = state.lock().map_err(message)?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let session = session.as_mut().ok_or("Open a project first.")?;
    if session.project().id != project_id || session.project().revision != revision {
        return Err("The project changed while copying. The layer was copied, but nothing was removed. Select it again to cut.".into());
    }
    let journal = journal.as_ref().ok_or("Open a project first.")?;
    match target {
        desegni_core::LayerTarget::Request { region_id } => session
            .execute(
                revision,
                desegni_core::Command::RemoveRegion { page_id, region_id },
                |next| journal.append(next),
            )
            .map_err(message)?,
        desegni_core::LayerTarget::Element { target } => {
            session
                .edit_element_document(
                    revision,
                    &document,
                    &target,
                    &desegni_core::ElementEdit::Remove,
                    |next| journal.append(next),
                )
                .map_err(message)?;
        }
    }
    Ok(state.view())
}

#[tauri::command]
pub async fn paste_layer(
    app: AppHandle,
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    revision: u64,
    after: Option<desegni_core::ElementTarget>,
    document: Option<desegni_core::DocumentTarget>,
) -> NativeResult<LayerPasteView> {
    let text = tauri::async_runtime::spawn_blocking(move || crate::clipboard::read(&app))
        .await
        .map_err(message)??;
    let packet = desegni_core::LayerClipboard::from_json(&text).map_err(message)?;
    let mut state = state.lock().map_err(message)?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let session = session.as_mut().ok_or("Open a project first.")?;
    if session.project().id != project_id {
        return Err("Return to the project where you want to paste this layer.".into());
    }
    let journal = journal.as_ref().ok_or("Open a project first.")?;
    let document = document.unwrap_or(desegni_core::DocumentTarget::Page { page_id });
    let selection = session
        .paste_layer_document(revision, &document, &packet, after.as_ref(), |next| {
            journal.append(next)
        })
        .map_err(message)?;
    Ok(LayerPasteView {
        workspace: state.view(),
        selection,
    })
}

#[tauri::command]
pub fn refresh_element_bounds(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    viewport: Viewport,
    bounds: Vec<desegni_core::ElementBounds>,
) -> NativeResult<WorkspaceView> {
    let mut state = state.lock().map_err(message)?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let session = session.as_mut().ok_or("Open a project first.")?;
    if session.project().id != project_id {
        return Err("Return to the project that owns these requests.".into());
    }
    let journal = journal.as_ref().ok_or("Open a project first.")?;
    session
        .refresh_element_bounds(page_id, viewport, &bounds, |next| journal.append(next))
        .map_err(message)?;
    Ok(state.view())
}

#[tauri::command]
pub fn refresh_element_target_bounds(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    viewport: Viewport,
    bounds: Vec<desegni_core::ElementTargetBounds>,
) -> NativeResult<WorkspaceView> {
    let mut state = state.lock().map_err(message)?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let session = session.as_mut().ok_or("Open a project first.")?;
    if session.project().id != project_id {
        return Err("Return to the project that owns these requests.".into());
    }
    let journal = journal.as_ref().ok_or("Open a project first.")?;
    session
        .refresh_element_target_bounds(page_id, viewport, &bounds, |next| journal.append(next))
        .map_err(message)?;
    Ok(state.view())
}

#[tauri::command]
pub fn undo_project(state: WorkspaceState<'_>) -> NativeResult<WorkspaceView> {
    let mut state = state.lock().map_err(message)?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let journal = journal.as_ref().ok_or("Create or open a project first.")?;
    session
        .as_mut()
        .ok_or("Create or open a project first.")?
        .undo(|p| journal.append(p))
        .map_err(message)?;
    Ok(state.view())
}

#[tauri::command]
pub fn redo_project(state: WorkspaceState<'_>) -> NativeResult<WorkspaceView> {
    let mut state = state.lock().map_err(message)?;
    let Workspace {
        session, journal, ..
    } = &mut *state;
    let journal = journal.as_ref().ok_or("Create or open a project first.")?;
    session
        .as_mut()
        .ok_or("Create or open a project first.")?
        .redo(|p| journal.append(p))
        .map_err(message)?;
    Ok(state.view())
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum OpenProjectResult {
    Workspace(Box<WorkspaceView>),
    Website {
        website: Box<crate::website_import::FolderSelection>,
    },
}
#[tauri::command]
pub async fn edit_website_folder(
    app: AppHandle,
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
    imports: State<'_, Mutex<crate::website_import::WebsiteImports>>,
    folder_id: Uuid,
    discard_recovery: Option<bool>,
) -> OpenResult<WorkspaceView> {
    generation
        .lock()
        .map_err(message)?
        .ensure_switch_allowed()?;
    let path = imports.lock().map_err(message)?.path(folder_id)?;
    // Snapshotting pictures and writing the initial archive can take seconds. Keep the UI
    // event loop responsive while the worker owns the workspace transaction.
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Mutex<Workspace>>();
        let mut state = state.lock().map_err(message)?;
        app.state::<Mutex<crate::generation::GenerationService>>()
            .lock()
            .map_err(message)?
            .ensure_switch_allowed()?;
        state.open_file_tab(&path, discard_recovery.unwrap_or(false))?;
        Ok(state.view())
    })
    .await
    .map_err(message)?
}
#[tauri::command]
pub async fn open_project(
    app: AppHandle,
    state: WorkspaceState<'_>,
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
    discard: bool,
    folder: Option<bool>,
    path: Option<PathBuf>,
    discard_recovery: Option<bool>,
    imports: State<'_, Mutex<crate::website_import::WebsiteImports>>,
) -> OpenResult<Option<OpenProjectResult>> {
    generation
        .lock()
        .map_err(message)?
        .ensure_switch_allowed()?;
    let _ = discard; // Opening adds a tab group; the active project stays open.
    let before = {
        let state = state.lock().map_err(message)?;
        state.session.as_ref().map(|s| s.project().clone())
    };
    let path = if let Some(path) = path {
        path
    } else {
        let picked = tauri::async_runtime::spawn_blocking(move || {
            let dialog = app.dialog().file();
            if folder.unwrap_or(false) {
                dialog
                    .set_title("Open project folder")
                    .blocking_pick_folder()
            } else {
                dialog
                    .add_filter(
                        "Desegnos website",
                        &desegni_storage::PROJECT_FILE_EXTENSIONS,
                    )
                    .blocking_pick_file()
            }
        })
        .await
        .map_err(message)?;
        let Some(picked) = picked else {
            return Ok(None);
        };
        picked.into_path().map_err(message)?
    };
    let mut state = state.lock().map_err(message)?;
    generation
        .lock()
        .map_err(message)?
        .ensure_switch_allowed()?;
    if let Some(before) = before {
        state.check_snapshot(&before)?;
    } else if state.session.is_some() {
        return Err(
            "A project was opened while the dialog was active. Open the file again.".into(),
        );
    }
    if path.is_dir()
        && desegni_storage::project_in_folder(&path)
            .map_err(message)?
            .is_none()
    {
        let website = imports.lock().map_err(message)?.select(path)?;
        return Ok(Some(OpenProjectResult::Website {
            website: Box::new(website),
        }));
    }
    state.open_file_tab(&path, discard_recovery.unwrap_or(false))?;
    Ok(Some(OpenProjectResult::Workspace(Box::new(state.view()))))
}

#[tauri::command]
pub async fn save_project(
    app: AppHandle,
    state: WorkspaceState<'_>,
    save_as: bool,
) -> NativeResult<Option<WorkspaceView>> {
    let (project, choose_path) = {
        let state = state.lock().map_err(message)?;
        (
            state
                .session
                .as_ref()
                .ok_or("Create or open a project first.")?
                .project()
                .clone(),
            save_as,
        )
    };
    let picked = if choose_path {
        let picked = tauri::async_runtime::spawn_blocking(move || {
            app.dialog()
                .file()
                .add_filter(
                    "Desegnos website",
                    &desegni_storage::PROJECT_FILE_EXTENSIONS,
                )
                .set_file_name("website.desegnos")
                .blocking_save_file()
        })
        .await
        .map_err(message)?;
        let Some(picked) = picked else {
            return Ok(None);
        };
        Some(picked.into_path().map_err(message)?)
    } else {
        None
    };
    let mut state = state.lock().map_err(message)?;
    state.check_snapshot(&project)?;
    let picked = copy_destination(picked, &project)?;
    state.save_snapshot(&project, picked)?;
    Ok(Some(state.view()))
}

async fn choose_save_path(app: AppHandle, suggested: String) -> NativeResult<Option<PathBuf>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter(
                "Desegnos website",
                &desegni_storage::PROJECT_FILE_EXTENSIONS,
            )
            .set_file_name(suggested)
            .blocking_save_file()
    })
    .await
    .map_err(message)?;
    picked
        .map(|picked| picked.into_path().map_err(message))
        .transpose()
}

/// New Save As / conflict copies get their own folder. Choosing an existing archive still
/// runs the established conflict/Replace flow, including legacy standalone destinations.
fn copy_destination(path: Option<PathBuf>, project: &Project) -> NativeResult<Option<PathBuf>> {
    path.map(|path| {
        let path = if path.extension().is_none() {
            path.with_extension("desegnos")
        } else {
            path
        };
        if path.exists() {
            return Ok(path);
        }
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_none_or(|ext| !desegni_storage::PROJECT_FILE_EXTENSIONS.contains(&ext))
        {
            return Err("Project copies must use .desegnos or .desegni.".into());
        }
        let parent = path.parent().ok_or("Choose a project location.")?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Choose a project name.")?;
        desegni_storage::create_project_folder(parent, name, project.id).map_err(message)
    })
    .transpose()
}

#[tauri::command]
pub fn move_into_project_folder(state: WorkspaceState<'_>) -> NativeResult<WorkspaceView> {
    let mut state = state.lock().map_err(message)?;
    let project = state.project()?.clone();
    let former = state.file.as_ref().map(|file| file.path().to_owned());
    if state
        .file
        .as_ref()
        .is_some_and(ProjectFile::is_project_folder)
    {
        return Ok(state.view());
    }
    let path = desegni_storage::create_project_folder(
        &state.project_location()?,
        &project.name,
        project.id,
    )
    .map_err(message)?;
    // Keep the old archive as a fallback. Only the active save/recovery/recent destination moves.
    state.save_snapshot(&project, Some(path))?;
    if let (Some(former), Some(file)) = (former, state.file.as_ref()) {
        state.recent_error =
            crate::recent::moved(&state.directory, file.path(), &project.name, &former).err();
    }
    Ok(state.view())
}

#[tauri::command]
pub async fn save_project_checked(
    app: AppHandle,
    state: WorkspaceState<'_>,
    save_as: bool,
) -> NativeResult<SaveProjectOutcome> {
    let (project, choose_path) = {
        let mut state = state.lock().map_err(message)?;
        state.pending_save = None;
        (state.project()?.clone(), save_as)
    };
    let picked = if choose_path {
        let suggested = format!(
            "{}.desegnos",
            desegni_storage::sanitize_project_name(&project.name)
        );
        let Some(path) = choose_save_path(app, suggested).await? else {
            return Ok(SaveProjectOutcome::Cancelled);
        };
        Some(path)
    } else {
        None
    };
    let mut state = state.lock().map_err(message)?;
    state.check_snapshot(&project)?;
    let picked = copy_destination(picked, &project)?;
    state.save_snapshot_checked(&project, picked, choose_path)
}

#[tauri::command]
pub async fn resolve_save_conflict(
    app: AppHandle,
    state: WorkspaceState<'_>,
    token: Uuid,
    choice: SaveConflictChoice,
) -> NativeResult<SaveProjectOutcome> {
    match choice {
        SaveConflictChoice::Replace => state.lock().map_err(message)?.replace_save_conflict(token),
        SaveConflictChoice::Cancel => {
            state.lock().map_err(message)?.cancel_save_conflict(token)?;
            Ok(SaveProjectOutcome::Cancelled)
        }
        SaveConflictChoice::Copy => {
            let (project, suggested) = {
                let mut state = state.lock().map_err(message)?;
                let pending = state.take_save_conflict(token)?;
                let file = state.destination_file(&pending.destination)?;
                let stem = file
                    .path()
                    .file_stem()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "website".into());
                (state.project()?.clone(), format!("{stem}-copy.desegnos"))
            };
            let Some(path) = choose_save_path(app, suggested).await? else {
                return Ok(SaveProjectOutcome::Cancelled);
            };
            let mut state = state.lock().map_err(message)?;
            state.check_snapshot(&project)?;
            let path = copy_destination(Some(path), &project)?;
            state.save_snapshot_checked(&project, path, true)
        }
    }
}

#[tauri::command]
pub fn prepare_requirements(
    state: WorkspaceState<'_>,
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
) -> NativeResult<RequirementsSnapshot> {
    let mut state = state.lock().map_err(message)?;
    let prepared = state
        .session
        .as_ref()
        .ok_or("Create or open a project first.")?
        .project()
        .prepare_requirements()
        .map_err(message)?;
    let history = generation
        .lock()
        .map_err(message)?
        .request_history(state.project()?)?;
    let prepared = prepared
        .with_earlier_requests(
            state.project()?,
            &history,
            time::OffsetDateTime::now_utc().unix_timestamp(),
        )
        .map_err(message)?;
    state.prepared = Some(prepared.clone());
    Ok(prepared)
}

#[tauri::command]
pub async fn save_requirements(
    app: AppHandle,
    state: WorkspaceState<'_>,
    job_id: String,
) -> NativeResult<bool> {
    let prepared = state
        .lock()
        .map_err(message)?
        .prepared
        .clone()
        .filter(|p| p.job_id.to_string() == job_id)
        .ok_or("Prepare the requirements again before saving them.")?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Markdown document", &["md"])
            .set_file_name("REQUIREMENTS.md")
            .blocking_save_file()
    })
    .await
    .map_err(message)?;
    let Some(picked) = picked else {
        return Ok(false);
    };
    write_markdown(&picked.into_path().map_err(message)?, &prepared.markdown).map_err(message)?;
    Ok(true)
}

#[tauri::command]
pub fn dismiss_recovery(
    state: WorkspaceState<'_>,
    project_id: Uuid,
) -> NativeResult<WorkspaceView> {
    let mut state = state.lock().map_err(message)?;
    state.dismiss_recovery(project_id)?;
    Ok(state.view())
}

#[tauri::command]
pub fn recover_project(
    state: WorkspaceState<'_>,
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
    project_id: Option<Uuid>,
    discard: Option<bool>,
) -> NativeResult<WorkspaceView> {
    generation
        .lock()
        .map_err(message)?
        .ensure_switch_allowed()?;
    let mut state = state.lock().map_err(message)?;
    state.recover(project_id, discard.unwrap_or(false))?;
    Ok(state.view())
}

#[tauri::command]
pub fn get_project_notes(state: WorkspaceState<'_>) -> NativeResult<String> {
    state
        .lock()
        .map_err(message)?
        .session
        .as_ref()
        .ok_or("Create or open a project first.")?
        .project()
        .project_markdown()
        .map_err(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use desegni_core::GENERATION_MIGRATION_NOTICE;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("desegni-workspace-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
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

    #[test]
    fn creating_a_project_immediately_saves_a_numbered_folder_and_live_site() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        let first = project("New: website");
        workspace.create_in_folder(first.clone(), None).unwrap();
        let path = temp.0.join("Projects/New_ website/New_ website.desegnos");
        assert_eq!(workspace.file.as_ref().unwrap().path(), path);
        assert_eq!(desegni_storage::load_project(&path).unwrap(), first);
        assert!(path.parent().unwrap().join("index.html").is_file());
        assert!(!workspace.view().dirty);
        assert!(workspace.view().project_folder);
        assert_eq!(crate::recent::read(&temp.0).unwrap()[0].path, path);
        workspace
            .create_in_folder(project("New: website"), None)
            .unwrap();
        assert_eq!(
            workspace.file.as_ref().unwrap().path(),
            temp.0
                .join("Projects/New_ website 2/New_ website 2.desegnos")
        );
        let chosen = temp.0.join("Chosen parent");
        workspace
            .create_in_folder(project("Different parent"), Some(chosen.clone()))
            .unwrap();
        assert!(workspace.file.as_ref().unwrap().path().starts_with(chosen));
    }

    #[test]
    fn opening_a_project_folder_reloads_the_full_document_with_fresh_history() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.join("app-data")).unwrap();
        let mut original = project("Folder with context");
        original.brief.purpose = "Retain this brief, page content and decisions".into();
        original.pages[0].content = Some(desegni_core::SiteContent {
            html: "<h1>Existing page</h1>".into(),
            css: String::new(),
            javascript: String::new(),
        });
        workspace
            .create_in_folder(original.clone(), Some(temp.0.clone()))
            .unwrap();
        let folder = workspace
            .file
            .as_ref()
            .unwrap()
            .path()
            .parent()
            .unwrap()
            .to_owned();
        change_grid(&mut workspace);
        let edited = workspace.project().unwrap().clone();
        workspace.save_snapshot(&edited, None).unwrap();
        assert!(workspace.view().can_undo);
        workspace.open_file_tab(&folder, false).unwrap();
        assert_eq!(workspace.project().unwrap(), &edited);
        assert!(!workspace.view().can_undo);
        assert!(!workspace.view().dirty);
        assert_eq!(
            workspace.project().unwrap().brief.purpose,
            original.brief.purpose
        );
    }

    #[test]
    fn opening_a_plain_folder_imports_once_and_keeps_its_destination_through_recovery() {
        let temp = Temp::new();
        let folder = temp.0.join("Existing site");
        fs::create_dir(&folder).unwrap();
        let html = "<!doctype html><html><head><title>Original</title></head><body><h1>Website</h1></body></html>";
        fs::write(folder.join("index.html"), html).unwrap();
        let directory = temp.0.join("app-data");
        let mut workspace = Workspace::from_directory(directory.clone()).unwrap();
        workspace.open_file_tab(&folder, false).unwrap();
        let path = folder.join("Existing site.desegnos");
        assert_eq!(workspace.file.as_ref().unwrap().path(), path);
        assert_eq!(fs::read_to_string(folder.join("index.html")).unwrap(), html);
        assert_eq!(
            fs::read_to_string(folder.join(".desegnos-original/index.html")).unwrap(),
            html
        );
        assert!(
            workspace.project().unwrap().pages[0]
                .content
                .as_ref()
                .unwrap()
                .html
                .contains("data-desegnos-id")
        );
        change_grid(&mut workspace);
        let edited = workspace.project().unwrap().clone();
        drop(workspace);
        let mut restarted = Workspace::from_directory(directory).unwrap();
        restarted.recover(Some(edited.id), false).unwrap();
        assert_eq!(restarted.project().unwrap(), &edited);
        assert_eq!(restarted.file.as_ref().unwrap().path(), path);
        restarted.save_snapshot(&edited, None).unwrap();
        assert!(!restarted.view().dirty);
        assert_eq!(
            fs::read_to_string(folder.join(".desegnos-original/index.html")).unwrap(),
            html
        );
    }

    #[test]
    fn fresh_initialization_offers_committed_unsaved_changes() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace
            .start(project("Recover this project"), None, None)
            .unwrap();
        let Workspace {
            session, journal, ..
        } = &mut workspace;
        session
            .as_mut()
            .unwrap()
            .execute(
                0,
                Command::SetDecision {
                    topic: "Brand color".into(),
                    value: "#185F48".into(),
                    scope: desegni_core::Scope::Project,
                    protected: true,
                },
                |p| journal.as_ref().unwrap().append(p),
            )
            .unwrap();
        drop(workspace);
        let restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        let candidate = restarted.recoveries.first().unwrap();
        let (recovered, _) = restarted
            .read_recovery(candidate.info.project_id)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.project.revision, 1);
        assert_eq!(recovered.project.decisions[0].value, "#185F48");
        assert!(restarted.session.is_none());
        assert!(restarted.recovery_error.is_none());
    }

    fn region() -> &'static str {
        "a41bd1d5-0a1e-4f0e-9e2f-8a3c1d6b7e90"
    }
    fn imported_site_project() -> Project {
        let files = std::collections::BTreeMap::from([
            (
                "index.html".into(),
                format!(
                    "<!doctype html><html><head><title>Imported site</title></head><body><main><h1>Home</h1></main><script type=\"application/x-desegnos-region\" data-desegnos-script=\"{}\">document.title = 'Region';</script></body></html>",
                    region()
                )
                .into_bytes(),
            ),
            ("styles.css".into(), b"h1 { color: blue; }".to_vec()),
        ]);
        desegni_core::import_website_files("Imported site", &files).unwrap()
    }
    fn saved_site(temp: &Temp) -> (Workspace, std::path::PathBuf) {
        let mut workspace = Workspace::from_directory(temp.0.join("profile")).unwrap();
        let project = imported_site_project();
        let path =
            desegni_storage::create_project_folder(&temp.0, &project.name, project.id).unwrap();
        let file = ProjectFile::acquire(&path).unwrap();
        file.save(&project).unwrap();
        workspace.start(project, Some(file), Some(0)).unwrap();
        let root = path.parent().unwrap().to_path_buf();
        (workspace, root)
    }

    #[test]
    fn own_site_export_stays_readable_so_import_changes_can_run() {
        let temp = Temp::new();
        let (mut workspace, root) = saved_site(&temp);
        // The region runtime script must live inside the document, not after `</html>`.
        let html = fs::read_to_string(root.join("index.html")).unwrap();
        assert!(html.contains("<script defer src=\"desegnos-regions-"));
        assert!(html.trim_end().ends_with("</body></html>"));
        let files = desegni_storage::project_folder::read_website_files(&root)
            .unwrap()
            .files;
        let selection = workspace.prepare_site_import(files).unwrap().unwrap();
        assert_eq!(selection.review.updated, ["/"]);
    }

    #[test]
    fn importing_the_changed_own_site_is_not_an_outside_edit_but_plain_save_still_is() {
        let temp = Temp::new();
        let (mut workspace, root) = saved_site(&temp);
        let edited = format!(
            "<!doctype html><html><head><title>Imported site</title></head><body><main><h1>Outside home</h1></main><script type=\"application/x-desegnos-region\" data-desegnos-script=\"{}\">document.title = 'Outside';</script></body></html>",
            region()
        );
        fs::write(root.join("index.html"), &edited).unwrap();
        let current = workspace.project().unwrap().clone();
        workspace
            .save_snapshot_checked(&current, None, false)
            .unwrap();
        assert_eq!(workspace.view().outside_edits.changed_paths, ["index.html"]);
        assert_eq!(fs::read_to_string(root.join("index.html")).unwrap(), edited);
        // "Import changes" reads the project's own site and adopts exactly those bytes.
        let files = desegni_storage::project_folder::read_website_files(&root)
            .unwrap()
            .files;
        let selection = workspace.prepare_site_import(files).unwrap().unwrap();
        workspace.apply_site_import(selection.token, &[]).unwrap();
        assert!(workspace.project().unwrap().pages.iter().any(|page| {
            page.content
                .as_ref()
                .is_some_and(|content| content.html.contains("Outside home"))
        }));
        let current = workspace.project().unwrap().clone();
        workspace
            .save_snapshot_checked(&current, None, false)
            .unwrap();
        let outside = workspace.view().outside_edits;
        assert_eq!(outside.changed_paths, Vec::<String>::new(), "{outside:?}");
        assert_eq!(outside.kept_stale_paths, Vec::<String>::new());
        assert!(
            fs::read_to_string(root.join("index.html"))
                .unwrap()
                .contains("desegnos-regions-")
        );
    }

    #[test]
    fn successful_save_clears_recovery_offer_and_preserves_actual_archive() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        let project = project("Saved native project");
        workspace.start(project.clone(), None, None).unwrap();
        workspace
            .save_snapshot(&project, Some(temp.0.join("site.desegni")))
            .unwrap();
        assert!(!workspace.view().dirty);
        drop(workspace);
        let restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert!(restarted.recoveries.is_empty());
        assert_eq!(
            desegni_storage::load_project(&temp.0.join("site.desegni")).unwrap(),
            project
        );
    }

    #[test]
    fn another_project_with_the_same_revision_cannot_be_saved_from_an_old_dialog() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        let original = project("Original");
        workspace.start(original.clone(), None, None).unwrap();
        workspace.start(project("New project"), None, None).unwrap();
        let path = temp.0.join("must-not-be-created.desegni");
        assert!(
            workspace
                .save_snapshot(&original, Some(path.clone()))
                .is_err()
        );
        assert!(!path.exists());
        assert_eq!(
            workspace.session.as_ref().unwrap().project().name,
            "New project"
        );
    }

    #[test]
    fn unsafe_recovery_pointer_is_reported_without_blocking_new_work() {
        let temp = Temp::new();
        let damaged_id = Uuid::new_v4();
        write_markdown(
            &temp.0.join(format!("{damaged_id}.recovery.json")),
            "{\"journalFile\":\"../outside.journal\",\"savedRevision\":null,\"projectPath\":null}",
        )
        .unwrap();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        assert!(
            workspace
                .recovery_error
                .as_ref()
                .unwrap()
                .contains("invalid journal path")
        );
        workspace
            .start(project("New safe project"), None, None)
            .unwrap();
        assert!(workspace.recovery_error.is_some());
        assert_eq!(
            workspace.session.as_ref().unwrap().project().name,
            "New safe project"
        );
    }

    #[test]
    fn switching_away_from_dirty_work_requires_a_deliberate_choice() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(project("Unsaved"), None, None).unwrap();
        assert!(workspace.check_replace(false).is_err());
        workspace.check_replace(true).unwrap();
    }

    #[test]
    fn only_ordinary_web_links_open_from_a_preview() {
        assert!(preview_link("https://chelib.com/projects/desegnos").is_ok());
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user:secret@example.com/",
            "mailto:someone@example.com",
            "not a link",
        ] {
            assert!(preview_link(url).is_err(), "{url}");
        }
    }

    #[test]
    fn discarding_on_close_stops_offering_the_work_but_keeps_its_journal() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(project("Discard me"), None, None).unwrap();
        change_grid(&mut workspace);
        let id = workspace.session.as_ref().unwrap().project().id;
        assert!(workspace.view().dirty);
        workspace.set_aside_recovery().unwrap();
        workspace.close();
        assert!(workspace.recoveries.is_empty());
        let restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert!(restarted.recoveries.is_empty());
        assert!(
            temp.0
                .join(format!("{id}.recovery.discarded.json"))
                .exists()
        );
        assert!(
            fs::read_dir(&temp.0)
                .unwrap()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().ends_with(".journal"))
        );
    }

    #[test]
    fn closing_a_clean_parked_project_releases_its_lock_and_keeps_the_rest_open() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        let first = project("Active project");
        let second = project("Clean parked project");
        workspace
            .start(first.clone(), None, Some(first.revision))
            .unwrap();
        workspace
            .open_tab(|state| state.start(second.clone(), None, Some(second.revision)))
            .unwrap();
        workspace.close_tab(Some(first.id), false).unwrap();
        assert_eq!(
            workspace.session.as_ref().unwrap().project().id,
            second.id
        );
        assert_eq!(
            workspace.view().open_projects.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            [second.id]
        );
        // The closed tab released its lock, so another window could open the project.
        assert!(workspace.acquire_ownership(first.id).unwrap().is_some());
    }

    #[test]
    fn closing_the_active_project_activates_the_next_open_one_then_the_start_screen() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        let first = project("First project");
        let second = project("Second project");
        let third = project("Third project");
        workspace
            .start(first.clone(), None, Some(first.revision))
            .unwrap();
        workspace
            .open_tab(|state| state.start(second.clone(), None, Some(second.revision)))
            .unwrap();
        workspace
            .open_tab(|state| state.start(third.clone(), None, Some(third.revision)))
            .unwrap();
        assert_eq!(
            workspace.session.as_ref().unwrap().project().id,
            third.id
        );
        // The third tab was last, so closing it falls back to the tab before it.
        workspace.close_tab(None, false).unwrap();
        assert_eq!(
            workspace.session.as_ref().unwrap().project().id,
            second.id
        );
        // With the second project parked behind the first, closing the first activates the
        // next tab in order rather than the most recently parked one.
        workspace.switch_to(first.id).unwrap();
        assert_eq!(
            workspace.session.as_ref().unwrap().project().id,
            first.id
        );
        workspace.close_tab(None, false).unwrap();
        assert_eq!(
            workspace.session.as_ref().unwrap().project().id,
            second.id
        );
        workspace.close_tab(None, false).unwrap();
        assert!(workspace.view().project.is_none());
        assert!(workspace.view().open_projects.is_empty());
    }

    #[test]
    fn closing_a_dirty_parked_project_needs_a_choice_and_discarding_keeps_its_journal() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        let first = project("Stays open");
        let second = project("Dirty parked project");
        workspace
            .start(first.clone(), None, Some(first.revision))
            .unwrap();
        workspace
            .open_tab(|state| state.start(second.clone(), None, Some(second.revision)))
            .unwrap();
        change_grid(&mut workspace);
        assert!(workspace.view().dirty);
        workspace.switch_to(first.id).unwrap();
        assert!(!workspace.view().dirty);
        let pointer: RecoveryPointer = serde_json::from_slice(
            &fs::read(workspace.pointer_path(second.id)).unwrap(),
        )
        .unwrap();
        let journal = temp.0.join(pointer.journal_file);
        assert!(journal.exists());
        assert!(workspace.close_tab(Some(second.id), false).is_err());
        assert!(
            workspace.view().open_projects.iter().any(|tab| tab.id == second.id),
            "a project with unsaved changes must stay open until the choice is explicit"
        );
        workspace.close_tab(Some(second.id), true).unwrap();
        assert_eq!(
            workspace.view().open_projects.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            [first.id]
        );
        assert!(
            workspace
                .recoveries
                .iter()
                .all(|item| item.info.project_id != second.id),
            "discarded work is no longer offered for recovery"
        );
        assert!(journal.exists(), "the discarded journal stays on disk");
        assert!(
            temp.0
                .join(format!("{}.recovery.discarded.json", second.id))
                .exists()
        );
        drop(workspace);
        let restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert!(
            restarted
                .view()
                .recoveries
                .iter()
                .all(|item| item.project_id != second.id),
            "discarded work is no longer offered, but its journal stays on disk"
        );
    }

    #[test]
    fn saving_a_dirty_parked_project_first_lets_it_close_cleanly() {
        let temp = Temp::new();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        let first = project("Stays open too");
        let second = project("Saved before closing");
        let path = temp.0.join("saved-before-closing.desegni");
        workspace
            .start(first.clone(), None, Some(first.revision))
            .unwrap();
        workspace
            .open_tab(|state| state.start(second.clone(), None, Some(second.revision)))
            .unwrap();
        change_grid(&mut workspace);
        let saved = workspace.session.as_ref().unwrap().project().clone();
        workspace.save_snapshot(&saved, Some(path.clone())).unwrap();
        workspace.switch_to(first.id).unwrap();
        workspace.close_tab(Some(second.id), false).unwrap();
        assert_eq!(
            workspace.view().open_projects.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            [first.id]
        );
        assert!(desegni_storage::load_project(&path).unwrap().show_grid);
    }

    #[test]
    fn dismiss_recovery_preserves_journal_and_saved_file_and_refuses_open_projects() {
        for saved in [false, true] {
            let temp = Temp::new();
            let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
            let original = project("Dismiss recovery");
            let id = original.id;
            let path = temp.0.join("saved.desegni");
            workspace.start(original.clone(), None, None).unwrap();
            if saved {
                workspace
                    .save_snapshot(&original, Some(path.clone()))
                    .unwrap();
            }
            change_grid(&mut workspace);
            let saved_bytes = fs::read(&path).ok();
            let pointer_bytes = fs::read(workspace.pointer_path(id)).unwrap();
            let pointer: RecoveryPointer = serde_json::from_slice(&pointer_bytes).unwrap();
            let journal = temp.0.join(pointer.journal_file);
            let journal_bytes = fs::read(&journal).unwrap();

            assert!(workspace.dismiss_recovery(id).is_err());
            workspace
                .open_tab(|state| state.start(project("Other open project"), None, None))
                .unwrap();
            assert!(workspace.dismiss_recovery(id).is_err());
            let mut other_window = Workspace::from_directory(temp.0.clone()).unwrap();
            assert!(other_window.dismiss_recovery(id).is_err());
            assert_eq!(fs::read(workspace.pointer_path(id)).unwrap(), pointer_bytes);
            drop(other_window);
            drop(workspace);

            let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
            assert!(
                restarted
                    .view()
                    .recoveries
                    .iter()
                    .any(|item| item.project_id == id)
            );
            restarted.dismiss_recovery(id).unwrap();
            assert!(
                !restarted
                    .view()
                    .recoveries
                    .iter()
                    .any(|item| item.project_id == id)
            );
            assert!(!restarted.pointer_path(id).exists());
            assert_eq!(
                fs::read(temp.0.join(format!("{id}.recovery.discarded.json"))).unwrap(),
                pointer_bytes
            );
            assert_eq!(fs::read(&journal).unwrap(), journal_bytes);
            assert_eq!(fs::read(&path).ok(), saved_bytes);
            drop(restarted);
            let restarted = Workspace::from_directory(temp.0.clone()).unwrap();
            assert!(
                !restarted
                    .view()
                    .recoveries
                    .iter()
                    .any(|item| item.project_id == id)
            );
        }
    }

    fn change_grid(workspace: &mut Workspace) {
        let Workspace {
            session, journal, ..
        } = workspace;
        let session = session.as_mut().unwrap();
        session
            .execute(
                session.project().revision,
                Command::SetGrid { enabled: true },
                |project| journal.as_ref().unwrap().append(project),
            )
            .unwrap();
    }

    #[test]
    fn independent_live_projects_keep_separate_recovery_records() {
        let temp = Temp::new();
        let first_project = project("First independent project");
        let second_project = project("Second independent project");
        let mut first = Workspace::from_directory(temp.0.clone()).unwrap();
        first.start(first_project.clone(), None, None).unwrap();
        change_grid(&mut first);
        let mut second = Workspace::from_directory(temp.0.clone()).unwrap();
        assert!(second.recoveries.is_empty());
        second.start(second_project.clone(), None, None).unwrap();
        change_grid(&mut second);
        assert!(
            temp.0
                .join(format!("{}.recovery.json", first_project.id))
                .exists()
        );
        assert!(
            temp.0
                .join(format!("{}.recovery.json", second_project.id))
                .exists()
        );
        drop(second);
        let while_first_active = Workspace::from_directory(temp.0.clone()).unwrap();
        assert_eq!(while_first_active.recoveries.len(), 1);
        assert_eq!(
            while_first_active.recoveries[0].info.project_id,
            second_project.id
        );
        drop(while_first_active);
        drop(first);
        let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert_eq!(restarted.view().recoveries.len(), 2);
        restarted.recover(Some(first_project.id), false).unwrap();
        assert_eq!(
            restarted.session.as_ref().unwrap().project().id,
            first_project.id
        );
        assert!(restarted.session.as_ref().unwrap().project().show_grid);
        assert_eq!(restarted.recoveries.len(), 1);
        assert_eq!(restarted.recoveries[0].info.project_id, second_project.id);
    }

    #[test]
    fn new_and_open_projects_do_not_supersede_inactive_recovery() {
        let temp = Temp::new();
        let original = project("Keep this unsaved work");
        let mut previous = Workspace::from_directory(temp.0.clone()).unwrap();
        previous.start(original.clone(), None, None).unwrap();
        change_grid(&mut previous);
        drop(previous);
        let record = temp.0.join(format!("{}.recovery.json", original.id));
        let before = fs::read(&record).unwrap();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(project("New project"), None, None).unwrap();
        assert!(
            workspace
                .recoveries
                .iter()
                .any(|candidate| candidate.info.project_id == original.id)
        );
        let opened = project("Another saved project");
        let file = ProjectFile::acquire(&temp.0.join("another.desegni")).unwrap();
        file.save(&opened).unwrap();
        workspace
            .start(opened.clone(), Some(file), Some(opened.revision))
            .unwrap();
        assert_eq!(fs::read(&record).unwrap(), before);
        assert!(
            workspace
                .recoveries
                .iter()
                .any(|candidate| candidate.info.project_id == original.id)
        );
        drop(workspace);
        let restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert_eq!(restarted.recoveries.len(), 2);
        assert!(
            restarted
                .recoveries
                .iter()
                .any(|candidate| candidate.info.project_id == original.id)
        );
    }

    #[test]
    fn project_ownership_excludes_another_window_and_stale_recovery_choice() {
        let temp = Temp::new();
        let original = project("One active owner");
        let mut first = Workspace::from_directory(temp.0.clone()).unwrap();
        first.start(original.clone(), None, None).unwrap();
        let mut second = Workspace::from_directory(temp.0.clone()).unwrap();
        assert!(second.start(original.clone(), None, None).is_err());
        assert!(second.recover(Some(original.id), false).is_err());
        drop(first);
        second.refresh_recoveries();
        assert_eq!(second.recoveries.len(), 1);
        let mut third = Workspace::from_directory(temp.0.clone()).unwrap();
        third.recover(Some(original.id), false).unwrap();
        assert!(second.recover(Some(original.id), false).is_err());
        assert!(second.session.is_none());
        assert_eq!(third.session.as_ref().unwrap().project().id, original.id);
    }

    #[test]
    fn a_failed_save_as_pointer_write_preserves_the_previous_file_and_lock() {
        let temp = Temp::new();
        let original = project("Preserve file ownership");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        let old_path = temp.0.join("original.desegni");
        workspace
            .save_snapshot(&original, Some(old_path.clone()))
            .unwrap();
        change_grid(&mut workspace);
        let candidate = workspace.session.as_ref().unwrap().project().clone();
        let pointer = workspace.pointer_path(original.id);
        let before = fs::read(&pointer).unwrap();
        let backup = pointer.with_file_name(format!(
            "{}.previous",
            pointer.file_name().unwrap().to_string_lossy()
        ));
        fs::remove_file(&backup).unwrap();
        fs::create_dir(&backup).unwrap();
        let new_path = temp.0.join("new.desegni");
        assert!(
            workspace
                .save_snapshot(&candidate, Some(new_path.clone()))
                .is_err()
        );
        assert_eq!(
            workspace.file.as_ref().unwrap().path(),
            old_path.canonicalize().unwrap()
        );
        assert_eq!(workspace.saved_revision, Some(original.revision));
        assert!(workspace.view().dirty);
        assert!(ProjectFile::acquire(&old_path).is_err());
        ProjectFile::acquire(&new_path).unwrap();
        assert_eq!(fs::read(pointer).unwrap(), before);
        assert_eq!(desegni_storage::load_project(&old_path).unwrap(), original);
        assert_eq!(desegni_storage::load_project(&new_path).unwrap(), candidate);
    }

    #[test]
    fn reopening_the_current_file_resets_history_without_releasing_its_lock() {
        let temp = Temp::new();
        let original = project("Reopen my saved site");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        let path = temp.0.join("site.desegnos");
        workspace
            .save_snapshot(&original, Some(path.clone()))
            .unwrap();
        change_grid(&mut workspace);
        let edited = workspace.session.as_ref().unwrap().project().clone();
        workspace.save_snapshot(&edited, None).unwrap();
        let previous_session = workspace.view().history.unwrap().session_id;
        workspace.open_path(&path, false).unwrap();
        assert_eq!(workspace.session.as_ref().unwrap().project(), &edited);
        let history = workspace.view().history.unwrap();
        assert_ne!(history.session_id, previous_session);
        assert_eq!((history.position, history.end), (0, 0));
        assert!(!workspace.view().dirty);
        assert!(ProjectFile::acquire(&path).is_err());
        drop(workspace);
        ProjectFile::acquire(&path).unwrap();
    }

    #[test]
    fn a_failed_reopen_keeps_the_active_session_lock_and_original_save_baseline() {
        let temp = Temp::new();
        let original = project("Preserve a failed reopen");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        let path = temp.0.join("site.desegnos");
        workspace
            .save_snapshot(&original, Some(path.clone()))
            .unwrap();
        let previous_history = workspace.view().history;
        let stamp = workspace.file.as_ref().unwrap().stamp().unwrap();
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, b"Unreadable external changes").unwrap();
        assert!(workspace.open_path(&path, false).is_err());
        assert_eq!(workspace.session.as_ref().unwrap().project(), &original);
        assert_eq!(workspace.view().history, previous_history);
        assert_eq!(workspace.file.as_ref().unwrap().stamp().unwrap(), stamp);
        assert!(ProjectFile::acquire(&path).is_err());
        fs::write(&path, bytes).unwrap();
        let pointer = workspace.pointer_path(original.id);
        let backup = pointer.with_file_name(format!(
            "{}.previous",
            pointer.file_name().unwrap().to_string_lossy()
        ));
        if backup.exists() {
            fs::remove_file(&backup).unwrap();
        }
        fs::create_dir(&backup).unwrap();
        assert!(workspace.open_path(&path, false).is_err());
        assert_eq!(workspace.view().history, previous_history);
        assert_eq!(workspace.file.as_ref().unwrap().stamp().unwrap(), stamp);
        assert!(ProjectFile::acquire(&path).is_err());
    }

    #[test]
    fn pending_recovery_is_structured_and_saved_version_retry_preserves_original_data() {
        for folder in [false, true] {
            let temp = Temp::new();
            let original = project("Saved version");
            let path = temp.0.join("site.desegnos");
            let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
            workspace.start(original.clone(), None, None).unwrap();
            workspace
                .save_snapshot(&original, Some(path.clone()))
                .unwrap();
            change_grid(&mut workspace);
            let pointer_bytes = fs::read(workspace.pointer_path(original.id)).unwrap();
            let pointer: RecoveryPointer = serde_json::from_slice(&pointer_bytes).unwrap();
            let journal = temp.0.join(pointer.journal_file);
            let journal_bytes = fs::read(&journal).unwrap();
            let saved_bytes = fs::read(&path).unwrap();
            drop(workspace);

            let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
            let other = project("Keep this tab");
            restarted.start(other.clone(), None, None).unwrap();
            let source = if folder {
                temp.0.as_path()
            } else {
                path.as_path()
            };
            let error = restarted.open_file_tab(source, false).unwrap_err();
            assert_eq!(
                serde_json::to_value(&error).unwrap(),
                serde_json::json!({
                    "code": "pendingRecovery", "projectId": original.id, "path": path.canonicalize().unwrap(),
                })
            );
            assert_eq!(restarted.session.as_ref().unwrap().project(), &other);
            assert_eq!(
                fs::read(restarted.pointer_path(original.id)).unwrap(),
                pointer_bytes
            );
            assert_eq!(fs::read(&journal).unwrap(), journal_bytes);

            restarted.open_file_tab(source, true).unwrap();
            assert_eq!(restarted.session.as_ref().unwrap().project(), &original);
            assert!(!restarted.view().dirty);
            assert!(
                restarted
                    .parked
                    .iter()
                    .any(|open| open.session.project() == &other)
            );
            assert!(
                !restarted
                    .view()
                    .recoveries
                    .iter()
                    .any(|item| item.project_id == original.id)
            );
            assert_eq!(
                fs::read(
                    temp.0
                        .join(format!("{}.recovery.discarded.json", original.id))
                )
                .unwrap(),
                pointer_bytes
            );
            assert_eq!(fs::read(&journal).unwrap(), journal_bytes);
            assert_eq!(fs::read(&path).unwrap(), saved_bytes);
            drop(restarted);
            let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
            restarted.open_file_tab(&path, false).unwrap();
            assert_eq!(restarted.session.as_ref().unwrap().project(), &original);
        }
    }

    #[test]
    fn discard_and_open_still_rejects_live_ownership_and_damaged_recovery() {
        let temp = Temp::new();
        let original = project("Protected recovery");
        let path = temp.0.join("site.desegnos");
        let mut first = Workspace::from_directory(temp.0.clone()).unwrap();
        first.start(original.clone(), None, None).unwrap();
        first.save_snapshot(&original, Some(path.clone())).unwrap();
        change_grid(&mut first);
        let mut second = Workspace::from_directory(temp.0.clone()).unwrap();
        // A second file with the same project ID reaches the ownership guard.
        let copy = temp.0.join("copy.desegnos");
        fs::copy(&path, &copy).unwrap();
        let error = second.open_file_tab(&copy, true).unwrap_err();
        assert_eq!(
            String::from(error),
            "This project is already active in another Desegnos window."
        );
        drop(first);
        let pointer = second.pointer_path(original.id);
        fs::write(&pointer, b"damaged recovery").unwrap();
        let error = second.open_file_tab(&path, true).unwrap_err();
        assert!(String::from(error).starts_with(
            "This project's recovery record needs attention before it can be replaced:"
        ));
        assert_eq!(fs::read(pointer).unwrap(), b"damaged recovery");
        assert!(second.session.is_none());
        assert!(
            !temp
                .0
                .join(format!("{}.recovery.discarded.json", original.id))
                .exists()
        );
    }

    #[test]
    fn an_older_saved_file_cannot_replace_a_pending_recovery_record() {
        let temp = Temp::new();
        let original = project("Recover before opening an older copy");
        let path = temp.0.join("site.desegni");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        workspace
            .save_snapshot(&original, Some(path.clone()))
            .unwrap();
        change_grid(&mut workspace);
        drop(workspace);
        let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        let file = ProjectFile::acquire(&path).unwrap();
        assert!(
            restarted
                .start(original.clone(), Some(file), Some(original.revision))
                .is_err()
        );
        assert!(restarted.session.is_none());
        restarted.recover(Some(original.id), false).unwrap();
        assert!(restarted.session.as_ref().unwrap().project().show_grid);
        assert!(restarted.file.is_some());
    }

    #[test]
    fn recovery_detaches_from_missing_corrupt_different_and_changed_archives() {
        for condition in ["missing", "corrupt", "different", "changed", "diverged"] {
            let temp = Temp::new();
            let original = project("Recover without overwriting external changes");
            let path = temp.0.join("site.desegni");
            let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
            workspace.start(original.clone(), None, None).unwrap();
            workspace
                .save_snapshot(&original, Some(path.clone()))
                .unwrap();
            change_grid(&mut workspace);
            let expected = workspace.session.as_ref().unwrap().project().clone();
            drop(workspace);
            match condition {
                "missing" => fs::remove_file(&path).unwrap(),
                "corrupt" => fs::write(&path, b"Keep the damaged original").unwrap(),
                "different" => {
                    let file = ProjectFile::acquire(&path).unwrap();
                    let observed = {
                        file.load_document().unwrap();
                        file.stamp().unwrap().unwrap()
                    };
                    file.save_replacing(&project("An unrelated actual project"), &observed, true)
                        .unwrap();
                }
                "changed" => {
                    let file = ProjectFile::acquire(&path).unwrap();
                    let external = original
                        .apply(
                            0,
                            Command::RenameProject {
                                name: "Externally changed".into(),
                            },
                        )
                        .unwrap();
                    file.load_document().unwrap();
                    file.save(&external).unwrap();
                }
                "diverged" => {
                    let file = ProjectFile::acquire(&path).unwrap();
                    let mut external = original.clone();
                    external.name = "A divergent copy at the same revision".into();
                    file.load_document().unwrap();
                    file.save(&external).unwrap();
                }
                _ => unreachable!(),
            }
            let before = fs::read(&path).ok();
            let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
            restarted.recover(Some(original.id), false).unwrap();
            assert_eq!(
                restarted.session.as_ref().unwrap().project(),
                &expected,
                "{condition}"
            );
            assert!(restarted.file.is_none(), "{condition}");
            assert!(restarted.view().dirty, "{condition}");
            restarted.save_snapshot(&expected, None).unwrap();
            let recovered_path = restarted.file.as_ref().unwrap().path();
            assert_ne!(recovered_path, path, "{condition}");
            assert!(recovered_path.starts_with(temp.0.join("Projects")));
            assert!(!restarted.view().dirty);
            assert_eq!(fs::read(&path).ok(), before, "{condition}");
        }
    }

    #[test]
    fn recovering_another_project_requires_a_choice_and_preserves_current_recovery() {
        let temp = Temp::new();
        let original = project("Previously interrupted project");
        let mut previous = Workspace::from_directory(temp.0.clone()).unwrap();
        previous.start(original.clone(), None, None).unwrap();
        drop(previous);
        let current = project("Current unsaved project");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(current.clone(), None, None).unwrap();
        change_grid(&mut workspace);
        // Recovery opens in its own tab group; the unsaved current project stays open behind it.
        workspace.recover(Some(original.id), false).unwrap();
        assert_eq!(
            workspace.session.as_ref().unwrap().project().id,
            original.id
        );
        let parked = workspace
            .parked
            .iter()
            .find(|open| open.session.project().id == current.id)
            .unwrap();
        assert!(parked.dirty() && parked.session.project().show_grid);
        let tabs = workspace.view().open_projects;
        assert_eq!(
            tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            [current.id, original.id]
        );
        assert!(workspace.view().any_dirty);
        let (retained, _) = workspace.read_recovery(current.id).unwrap().unwrap();
        assert!(retained.project.show_grid);
        // Switching back keeps each project's own state.
        workspace.switch_to(current.id).unwrap();
        assert!(workspace.session.as_ref().unwrap().project().show_grid);
        workspace.close();
        assert_eq!(
            workspace.session.as_ref().unwrap().project().id,
            original.id
        );
        drop(workspace);
        let restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert_eq!(restarted.recoveries.len(), 2);
    }

    #[test]
    fn one_damaged_record_does_not_hide_valid_recoveries_or_change_its_original() {
        let temp = Temp::new();
        let original = project("A valid independently recoverable project");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        drop(workspace);
        let damaged_id = Uuid::new_v4();
        let damaged_path = temp.0.join(format!("{damaged_id}.recovery.json"));
        fs::write(&damaged_path, b"An incomplete recovery pointer").unwrap();
        let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert_eq!(restarted.recoveries.len(), 1);
        assert_eq!(restarted.recoveries[0].info.project_id, original.id);
        assert!(restarted.recovery_error.is_some());
        restarted.recover(Some(original.id), false).unwrap();
        assert_eq!(
            fs::read(&damaged_path).unwrap(),
            b"An incomplete recovery pointer"
        );
        assert!(restarted.recovery_error.is_some());
    }

    #[test]
    fn a_pointer_cannot_hide_unsaved_data_by_claiming_a_clean_revision() {
        let temp = Temp::new();
        let original = project("Verify the actual clean snapshot");
        let path = temp.0.join("site.desegni");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        workspace
            .save_snapshot(&original, Some(path.clone()))
            .unwrap();
        change_grid(&mut workspace);
        let revision = workspace.session.as_ref().unwrap().project().revision;
        let pointer = workspace.pointer_path(original.id);
        drop(workspace);
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&pointer).unwrap()).unwrap();
        record["savedRevision"] = revision.into();
        fs::write(&pointer, serde_json::to_vec(&record).unwrap()).unwrap();
        let before = fs::read(&path).unwrap();
        let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert_eq!(restarted.recoveries.len(), 1);
        restarted.recover(Some(original.id), false).unwrap();
        assert!(restarted.session.as_ref().unwrap().project().show_grid);
        assert!(restarted.file.is_none());
        assert_eq!(fs::read(path).unwrap(), before);
    }

    fn external_project_file(
        directory: &std::path::Path,
        path: &std::path::Path,
        project: &Project,
    ) {
        let source = directory.join(format!("external-{}.desegnos", Uuid::new_v4()));
        let file = ProjectFile::acquire(&source).unwrap();
        file.save(project).unwrap();
        fs::copy(&source, path).unwrap();
    }

    fn conflict(outcome: SaveProjectOutcome) -> SaveConflict {
        match outcome {
            SaveProjectOutcome::Conflict { conflict } => conflict,
            _ => panic!("expected a save conflict"),
        }
    }

    #[test]
    fn checked_save_requires_a_bound_choice_and_replaces_with_an_exact_backup() {
        let temp = Temp::new();
        let path = temp.0.join("site.desegnos");
        let original = project("Original");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        workspace
            .save_snapshot(&original, Some(path.clone()))
            .unwrap();
        change_grid(&mut workspace);
        let local = workspace.project().unwrap().clone();
        let mut external = original.clone();
        external.name = "External edit at the same revision".into();
        external_project_file(&temp.0, &path, &external);
        let bytes = fs::read(&path).unwrap();
        let journal_bytes = fs::read(workspace.journal.as_ref().unwrap().path()).unwrap();
        let choice = conflict(
            workspace
                .save_snapshot_checked(&local, None, false)
                .unwrap(),
        );
        assert_eq!(choice.kind, SaveFailureKind::Changed);
        assert_eq!(choice.default_choice, "copy");
        assert!(choice.can_replace);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            fs::read(workspace.journal.as_ref().unwrap().path()).unwrap(),
            journal_bytes
        );
        let shape = serde_json::to_value(&choice).unwrap();
        assert_eq!(shape["fileName"], "site.desegnos");
        assert!(shape["details"]["actual"]["manifestSha256"].is_string());
        assert!(matches!(
            workspace.replace_save_conflict(choice.token).unwrap(),
            SaveProjectOutcome::Saved { .. }
        ));
        assert_eq!(desegni_storage::load_project(&path).unwrap(), local);
        assert_eq!(
            fs::read(path.with_file_name("site.desegnos.previous")).unwrap(),
            bytes
        );
        assert!(!workspace.view().dirty);
        assert!(workspace.replace_save_conflict(choice.token).is_err());
    }

    #[test]
    fn changing_the_file_again_invalidates_the_replace_token_and_offers_a_new_choice() {
        let temp = Temp::new();
        let path = temp.0.join("site.desegnos");
        let original = project("Original");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        workspace
            .save_snapshot(&original, Some(path.clone()))
            .unwrap();
        let mut external = original.clone();
        external.name = "First external change".into();
        external_project_file(&temp.0, &path, &external);
        let first = conflict(
            workspace
                .save_snapshot_checked(&original, None, false)
                .unwrap(),
        );
        external.name = "Second external change".into();
        external_project_file(&temp.0, &path, &external);
        let bytes = fs::read(&path).unwrap();
        let second = conflict(workspace.replace_save_conflict(first.token).unwrap());
        assert_ne!(first.token, second.token);
        assert_ne!(first.details.actual, second.details.actual);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(workspace.replace_save_conflict(first.token).is_err());
        workspace.cancel_save_conflict(second.token).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn save_as_into_a_different_project_requires_an_in_app_confirmation_but_save_never_allows_it() {
        let temp = Temp::new();
        let target = temp.0.join("someone-elses.desegnos");
        let other = project("Different project");
        external_project_file(&temp.0, &target, &other);
        let bytes = fs::read(&target).unwrap();
        let original = project("My project");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        assert!(
            workspace
                .save_snapshot(&original, Some(target.clone()))
                .is_err()
        );
        let choice = conflict(
            workspace
                .save_snapshot_checked(&original, Some(target.clone()), true)
                .unwrap(),
        );
        assert_eq!(choice.kind, SaveFailureKind::DifferentProject);
        assert_eq!(choice.default_choice, "cancel");
        assert!(choice.can_replace);
        assert_eq!(
            choice.message,
            "someone-elses.desegnos is a different project. Replace it?"
        );
        assert_eq!(fs::read(&target).unwrap(), bytes);
        workspace.replace_save_conflict(choice.token).unwrap();
        assert_eq!(workspace.file.as_ref().unwrap().path(), target);
        assert_eq!(
            fs::read(target.with_file_name("someone-elses.desegnos.previous")).unwrap(),
            bytes
        );
        external_project_file(&temp.0, &target, &other);
        let blocked = conflict(
            workspace
                .save_snapshot_checked(&original, None, false)
                .unwrap(),
        );
        assert!(!blocked.can_replace);
        assert!(workspace.replace_save_conflict(blocked.token).is_err());
        assert_eq!(desegni_storage::load_project(&target).unwrap(), other);
    }

    #[test]
    fn a_local_edit_expires_replace_but_cancel_and_copy_cancellation_preserve_recovery() {
        let temp = Temp::new();
        let path = temp.0.join("site.desegnos");
        let original = project("Original");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        workspace
            .save_snapshot(&original, Some(path.clone()))
            .unwrap();
        change_grid(&mut workspace);
        let local = workspace.project().unwrap().clone();
        let mut external = original.clone();
        external.name = "External change".into();
        external_project_file(&temp.0, &path, &external);
        let choice = conflict(
            workspace
                .save_snapshot_checked(&local, None, false)
                .unwrap(),
        );
        change_grid(&mut workspace);
        assert!(workspace.replace_save_conflict(choice.token).is_err());
        workspace.cancel_save_conflict(choice.token).unwrap();
        let current = workspace.project().unwrap().clone();
        let choice = conflict(
            workspace
                .save_snapshot_checked(&current, None, false)
                .unwrap(),
        );
        let journal_path = workspace.journal.as_ref().unwrap().path().to_owned();
        let journal_bytes = fs::read(&journal_path).unwrap();
        let pointer_bytes = fs::read(workspace.pointer_path(original.id)).unwrap();
        // Copy consumes the pending choice, then a dismissed file dialog does nothing further.
        workspace.take_save_conflict(choice.token).unwrap();
        assert_eq!(fs::read(&journal_path).unwrap(), journal_bytes);
        assert_eq!(
            fs::read(workspace.pointer_path(original.id)).unwrap(),
            pointer_bytes
        );
        assert!(workspace.view().dirty);
        drop(workspace);
        let restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert_eq!(restarted.recoveries.len(), 1);
        assert_eq!(
            restarted.recoveries[0].info.saved_path.as_deref(),
            path.to_str()
        );
        assert!(restarted.recoveries[0].info.edited_at_ms > 0);
        assert_eq!(
            restarted
                .read_recovery(original.id)
                .unwrap()
                .unwrap()
                .0
                .project,
            current
        );
        let mut recovered = restarted;
        recovered.recover(Some(original.id), false).unwrap();
        assert!(recovered.file.is_none());
        drop(recovered);
        let restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert_eq!(
            restarted.recoveries[0].info.saved_path.as_deref(),
            path.to_str()
        );
    }

    #[test]
    fn unreadable_destinations_are_not_replaceable_and_successful_copy_keeps_the_external_file() {
        let temp = Temp::new();
        let path = temp.0.join("not-a-project.desegnos");
        fs::write(&path, b"Keep this original document").unwrap();
        let original = project("My unsaved project");
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.start(original.clone(), None, None).unwrap();
        let choice = conflict(
            workspace
                .save_snapshot_checked(&original, Some(path.clone()), true)
                .unwrap(),
        );
        assert_eq!(choice.kind, SaveFailureKind::Unreadable);
        assert!(!choice.can_replace);
        assert_eq!(choice.default_choice, "copy");
        workspace.take_save_conflict(choice.token).unwrap();
        let copy = temp.0.join("my-copy.desegnos");
        assert!(matches!(
            workspace
                .save_snapshot_checked(&original, Some(copy.clone()), true)
                .unwrap(),
            SaveProjectOutcome::Saved { .. }
        ));
        assert_eq!(fs::read(path).unwrap(), b"Keep this original document");
        assert_eq!(desegni_storage::load_project(&copy).unwrap(), original);
        assert!(!workspace.view().dirty);
        drop(workspace);
        assert!(
            Workspace::from_directory(temp.0.clone())
                .unwrap()
                .recoveries
                .is_empty()
        );
    }

    const SCHEMA_ONE: &[u8] = include_bytes!(
        "../../crates/desegni-storage/tests/fixtures/schema1-mixed-generation.desegni"
    );

    #[test]
    fn opening_schema_one_discloses_migration_and_keeps_the_original_until_save() {
        let temp = Temp::new();
        let path = temp.0.join("legacy.desegni");
        fs::write(&path, SCHEMA_ONE).unwrap();
        let file = ProjectFile::acquire(&path).unwrap();
        let document = file.load_document().unwrap();
        let expected = document.project.clone();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.open_document(file, document, false).unwrap();
        assert_eq!(workspace.session.as_ref().unwrap().project(), &expected);
        assert_eq!(
            workspace.view().migration_notice.as_deref(),
            Some(GENERATION_MIGRATION_NOTICE)
        );
        assert!(workspace.view().dirty);
        assert!(workspace.check_replace(false).is_err());
        assert_eq!(fs::read(&path).unwrap(), SCHEMA_ONE);
        drop(workspace);
        let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert_eq!(restarted.recoveries.len(), 1);
        restarted.recover(Some(expected.id), false).unwrap();
        assert_eq!(
            restarted.file.as_ref().unwrap().path(),
            path.canonicalize().unwrap()
        );
        assert_eq!(
            restarted.view().migration_notice.as_deref(),
            Some(GENERATION_MIGRATION_NOTICE)
        );
        assert!(restarted.view().dirty);
        restarted.save_snapshot(&expected, None).unwrap();
        assert!(!restarted.view().dirty);
        assert!(restarted.view().migration_notice.is_none());
        assert_eq!(
            fs::read(temp.0.join("legacy.desegni.previous")).unwrap(),
            SCHEMA_ONE
        );
        assert_eq!(restarted.file.as_ref().unwrap().load().unwrap(), expected);
        drop(restarted);
        assert!(
            Workspace::from_directory(temp.0.clone())
                .unwrap()
                .recoveries
                .is_empty()
        );
    }

    const SCHEMA_SEVEN: &[u8] =
        include_bytes!("../../crates/desegni-storage/tests/fixtures/schema7-untouched.desegni");

    #[test]
    fn an_untouched_silently_upgraded_file_is_not_offered_as_unsaved_work() {
        // Opening a schema 7 file upgrades it without a notice, so its journal holds the saved
        // file in newer bytes at the same revision. That used to be offered as unsaved work after
        // a restart; recovered it was not dirty, so closing kept the record and the file could
        // no longer be opened.
        let temp = Temp::new();
        let path = temp.0.join("site.desegni");
        fs::write(&path, SCHEMA_SEVEN).unwrap();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.open_file_tab(&path, false).unwrap();
        let opened = workspace.session.as_ref().unwrap().project().clone();
        assert!(!workspace.view().dirty);
        assert!(workspace.view().migration_notice.is_none());
        let pointer_path = workspace.pointer_path(opened.id);
        drop(workspace);

        let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert!(restarted.recoveries.is_empty());
        restarted.open_file_tab(&path, false).unwrap();
        assert_eq!(restarted.session.as_ref().unwrap().project(), &opened);
        assert!(!restarted.view().dirty);
        restarted.close();
        assert!(restarted.recoveries.is_empty());
        drop(restarted);

        // Records written before the upgrade checksum existed are checked against the file once.
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&pointer_path).unwrap()).unwrap();
        let upgraded = record
            .as_object_mut()
            .unwrap()
            .remove("upgradedSha256")
            .unwrap();
        fs::write(&pointer_path, serde_json::to_vec(&record).unwrap()).unwrap();
        let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert!(restarted.recoveries.is_empty());
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(&pointer_path).unwrap()).unwrap();
        assert_eq!(record["upgradedSha256"], upgraded);
        assert_eq!(fs::read(&path).unwrap(), SCHEMA_SEVEN);

        // Real edits are still offered.
        restarted.open_file_tab(&path, false).unwrap();
        change_grid(&mut restarted);
        drop(restarted);
        let mut restarted = Workspace::from_directory(temp.0.clone()).unwrap();
        assert_eq!(restarted.recoveries.len(), 1);
        restarted.recover(Some(opened.id), false).unwrap();
        assert!(restarted.view().dirty);
    }

    #[test]
    fn a_schema_one_recovery_journal_migrates_without_rewriting_its_original() {
        // The actual old archive provides the original typed JSON and checksum, rather than a reconstructed model.
        let temp = Temp::new();
        let fixture_path = temp.0.join("source.desegni");
        fs::write(&fixture_path, SCHEMA_ONE).unwrap();
        let source = desegni_storage::load_project_document(&fixture_path).unwrap();
        let original_json = include_str!(
            "../../crates/desegni-storage/tests/fixtures/schema1-mixed-generation.project.json"
        );
        assert_eq!(
            desegni_core::digest(original_json.as_bytes()),
            source.source_sha256
        );
        let journal_name = format!("{}-legacy.journal", source.project.id);
        let journal_path = temp.0.join(&journal_name);
        let journal_bytes = format!(
            "{{\"sha256\":\"{}\",\"project\":{original_json}}}\n",
            source.source_sha256
        )
        .into_bytes();
        fs::write(&journal_path, &journal_bytes).unwrap();
        let pointer = RecoveryPointer {
            journal_file: journal_name,
            saved_revision: None,
            saved_sha256: None,
            project_path: None,
            upgraded_sha256: None,
            unsaved_migration: false,
        };
        write_markdown(
            &temp.0.join(format!("{}.recovery.json", source.project.id)),
            &serde_json::to_string(&pointer).unwrap(),
        )
        .unwrap();
        let mut workspace = Workspace::from_directory(temp.0.clone()).unwrap();
        workspace.recover(Some(source.project.id), false).unwrap();
        assert_eq!(
            workspace.session.as_ref().unwrap().project(),
            &source.project
        );
        assert_eq!(
            workspace.view().migration_notice.as_deref(),
            Some(GENERATION_MIGRATION_NOTICE)
        );
        assert!(workspace.view().dirty);
        assert!(workspace.file.is_none());
        assert_eq!(fs::read(journal_path).unwrap(), journal_bytes);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProjectsView {
    projects: Vec<crate::recent::RecentProject>,
    error: Option<String>,
}

#[tauri::command]
pub fn get_recent_projects(
    window: tauri::WebviewWindow,
    state: WorkspaceState<'_>,
) -> NativeResult<RecentProjectsView> {
    if window.label() != "editor" {
        return Err("Only the editor can read recent projects.".into());
    }
    let state = state.lock().map_err(message)?;
    match crate::recent::read(&state.directory) {
        Ok(projects) => Ok(RecentProjectsView {
            projects,
            error: state.recent_error.clone(),
        }),
        Err(error) => Ok(RecentProjectsView {
            projects: vec![],
            error: Some(error),
        }),
    }
}

#[derive(Debug, Serialize)]
pub struct RecentProjectOpenError {
    message: String,
    missing: bool,
}

impl From<RecentProjectOpenError> for ProjectOpenError {
    fn from(error: RecentProjectOpenError) -> Self {
        Self::Recent(error)
    }
}

impl From<String> for RecentProjectOpenError {
    fn from(message: String) -> Self {
        Self {
            message,
            missing: false,
        }
    }
}

#[tauri::command]
pub fn open_recent_project(
    window: tauri::WebviewWindow,
    state: WorkspaceState<'_>,
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
    path: PathBuf,
    discard: bool,
    discard_recovery: Option<bool>,
) -> OpenResult<WorkspaceView> {
    (|| -> OpenResult<WorkspaceView> {
        if window.label() != "editor" {
            return Err("Only the editor can open recent projects."
                .to_owned()
                .into());
        }
        generation
            .lock()
            .map_err(message)?
            .ensure_switch_allowed()?;
        let _ = discard; // Opening adds a tab group; the active project stays open.
        let mut state = state.lock().map_err(message)?;
        if !crate::recent::read(&state.directory)?
            .iter()
            .any(|entry| entry.path == path)
        {
            return Err(
                "This project is not in your recent list. Use Open project to choose it."
                    .to_owned()
                    .into(),
            );
        }
        if let Err(error) = fs::metadata(&path) {
            return Err(RecentProjectOpenError {
                missing: matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ),
                message: format!(
                    "Could not open this recent project ({}): {error}",
                    path.display()
                ),
            }
            .into());
        }
        state.open_file_tab(&path, discard_recovery.unwrap_or(false))?;
        Ok(state.view())
    })()
    .map_err(|error| match error {
        ProjectOpenError::Message(message) => RecentProjectOpenError::from(message).into(),
        error => error,
    })
}

#[cfg(test)]
pub(super) fn open_archive_for_tests(directory: PathBuf, path: &std::path::Path) -> Workspace {
    let file = ProjectFile::acquire(path).unwrap();
    let document = file.load_document().unwrap();
    let mut workspace = Workspace::from_directory(directory).unwrap();
    workspace.open_document(file, document, false).unwrap();
    workspace
}

struct WebsiteImportSnapshot {
    token: Uuid,
    project_id: Uuid,
    revision: u64,
    files: std::collections::BTreeMap<String, Vec<u8>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsiteImportSelection {
    token: Uuid,
    review: desegni_core::WebsiteImportReview,
}

#[tauri::command]
pub fn project_site_import_source(
    state: WorkspaceState<'_>,
    identity: State<'_, Mutex<crate::identity::IdentityService>>,
) -> NativeResult<crate::identity::IdentitySourceManifest> {
    let state = state.lock().map_err(message)?;
    let file = state
        .file
        .as_ref()
        .filter(|f| f.is_project_folder())
        .ok_or("Save in a project folder first.")?;
    let root = file
        .path()
        .parent()
        .ok_or("Missing project folder.")?
        .canonicalize()
        .map_err(message)?;
    let files = desegni_storage::project_folder::read_website_files(&root)
        .map_err(message)?
        .files;
    identity
        .lock()
        .map_err(message)?
        .retain_project_site(&state.project()?.name, files)
}

#[tauri::command]
pub fn overwrite_project_site(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    revision: u64,
) -> NativeResult<WorkspaceView> {
    let mut state = state.lock().map_err(message)?;
    let project = state.project()?.clone();
    if project.id != project_id || project.revision != revision {
        return Err("The project changed. Save again.".into());
    }
    let file = state
        .file
        .as_ref()
        .filter(|f| f.is_project_folder())
        .ok_or("Save in a project folder first.")?;
    file.save_checked(&project).map_err(message)?;
    file.overwrite_site(&project).map_err(message)?;
    state.finish_save(&project, SaveDestination::Current)?;
    Ok(state.view())
}

#[tauri::command]
pub async fn prepare_website_import(
    state: WorkspaceState<'_>,
    identity: State<'_, Mutex<crate::identity::IdentityService>>,
    source_id: Uuid,
) -> NativeResult<Option<WebsiteImportSelection>> {
    let files = {
        let identity = identity.lock().map_err(message)?;
        identity.website_files(source_id)?
    };
    state.lock().map_err(message)?.prepare_site_import(files)
}

impl Workspace {
    fn prepare_site_import(
        &mut self,
        files: std::collections::BTreeMap<String, Vec<u8>>,
    ) -> NativeResult<Option<WebsiteImportSelection>> {
        if !files.keys().any(|name| {
            name.to_ascii_lowercase().ends_with(".html")
                || name.to_ascii_lowercase().ends_with(".htm")
        }) {
            return Ok(None);
        }
        let project = self
            .session
            .as_ref()
            .ok_or("Open a project first.")?
            .project();
        let (project_id, revision) = (project.id, project.revision);
        let review = project.review_website_import(&files).map_err(message)?;
        let token = Uuid::new_v4();
        self.website_import = Some(WebsiteImportSnapshot {
            token,
            project_id,
            revision,
            files,
        });
        Ok(Some(WebsiteImportSelection { token, review }))
    }
}

#[tauri::command]
pub fn apply_website_import(
    state: WorkspaceState<'_>,
    token: Uuid,
    remove: Vec<Uuid>,
) -> NativeResult<WorkspaceView> {
    let mut state = state.lock().map_err(message)?;
    state.apply_site_import(token, &remove)
}

impl Workspace {
    fn apply_site_import(&mut self, token: Uuid, remove: &[Uuid]) -> NativeResult<WorkspaceView> {
        let Workspace {
            session,
            journal,
            website_import,
            file,
            ..
        } = self;
        let snapshot = website_import
            .as_ref()
            .filter(|s| s.token == token)
            .ok_or("Choose the website again.")?;
        let session = session.as_mut().ok_or("Open a project first.")?;
        if session.project().id != snapshot.project_id {
            return Err("Choose the website again for this project.".into());
        }
        let journal = journal.as_ref().ok_or("Recovery is unavailable.")?;
        session
            .import_website(snapshot.revision, &snapshot.files, remove, |p| {
                journal.append(p)
            })
            .map_err(message)?;
        // Adopt only the bytes this import captured. The manifest check hashes the folder first,
        // so a source that merely resembles the project stays foreign.
        if let Some(file) = file.as_ref().filter(|f| f.is_project_folder()) {
            file.mark_imported_site_owned(&snapshot.files)
                .map_err(message)?;
        }
        *website_import = None;
        self.prepared = None;
        Ok(self.view())
    }
}
