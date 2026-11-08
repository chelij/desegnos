use crate::native_files::{
    LockExt, NativeResult, SMALL_RECORD_BYTES, message, open_lock_file, read_json,
};
use crate::project_history::ProjectHistoryView;
use desegnos_core::{
    Appearance, CanvasTemplate, Command, GenerationCandidate, Project, RequirementsSnapshot,
    Session,
    limits::{AUTOSAVE_INTERVAL_MS, MAX_NAME_CHARS},
};
use desegnos_storage::{
    AutosaveProject, FileStamp, LoadedProject, ProjectDatabase, SaveFailure, SaveFailureKind,
    remove_project_files, write_text,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

pub(crate) type WorkspaceState<'a> = State<'a, Mutex<Workspace>>;

const AUTOSAVE_INTERVAL: Duration = Duration::from_millis(AUTOSAVE_INTERVAL_MS);

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

fn lowest_unused_name(base: &str, names: impl IntoIterator<Item = String>) -> String {
    let used: BTreeSet<String> = names.into_iter().collect();
    let mut name = base.to_string();
    let mut number = 2;
    while used.contains(&name) {
        name = format!("{base} {number}");
        number += 1;
    }
    name
}

/// Remembers which project file a session had open, so its autosave can be offered after a
/// crash. The unsaved work itself lives inside the project database.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionPointer {
    project_path: PathBuf,
    #[serde(default)]
    untitled: bool,
}

#[derive(ts_rs::TS, Clone, Serialize)]
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

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    can_generate_from_brief: bool,
    project: Option<Project>,
    path: Option<String>,
    dirty: bool,
    /// The project has no user-chosen file yet; it can still autosave for recovery.
    untitled: bool,
    can_undo: bool,
    can_redo: bool,
    history: Option<HistoryView>,
    recovery: Option<RecoveryInfo>,
    recoveries: Vec<RecoveryInfo>,
    recovery_error: Option<String>,
    open_projects: Vec<OpenProjectSummary>,
    any_dirty: bool,
    /// Pointer clashes and pending wiring, derived from the project on every published view.
    pointer_states: Vec<desegnos_core::PointerStateView>,
}

#[derive(ts_rs::TS, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryView {
    session_id: Uuid,
    start: usize,
    position: usize,
    end: usize,
    version: u64,
    entries: Vec<desegnos_core::HistoryEntry>,
}

#[derive(ts_rs::TS, Serialize)]
#[ts(rename = "SaveOutcome")]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SaveProjectOutcome {
    Saved { workspace: Box<WorkspaceView> },
    Cancelled,
    Conflict { conflict: SaveConflict },
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveConflict {
    token: Uuid,
    kind: crate::project_history::SaveFailureKind,
    path: String,
    file_name: String,
    message: String,
    default_choice: &'static str,
    can_replace: bool,
    details: SaveConflictDetails,
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveConflictDetails {
    expected: Option<crate::project_history::FileStamp>,
    actual: Option<crate::project_history::FileStamp>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveConflictChoice {
    Copy,
    Replace,
    Cancel,
}

#[derive(Clone)]
enum SaveDestination {
    Current,
    /// A project file that does not exist yet.
    New(PathBuf),
    /// A project file that already exists: saving it may conflict.
    Existing(PathBuf),
}

struct PendingSaveConflict {
    token: Uuid,
    project_id: Uuid,
    revision: u64,
    destination: SaveDestination,
    actual: Option<FileStamp>,
    can_replace: bool,
    save_as: bool,
    labels: Vec<String>,
    label: Option<String>,
}

pub struct Workspace {
    pub(crate) switch_guard: crate::ai_job::SwitchGuard,
    directory: PathBuf,
    system_project_location: PathBuf,
    active: Option<OpenProject>,
    recoveries: Vec<RecoveryCandidate>,
    recovery_error: Option<String>,
    /// The last autosave failed, so edits since are not being kept for recovery.
    autosave_error: Option<String>,
    recent_error: Option<String>,
    /// Other open projects, each with its own undo history, file and autosave.
    parked: Vec<OpenProject>,
    order: Vec<Uuid>,
}

/// All state owned by one open tab, active or parked.
struct OpenProject {
    session: Session,
    last_autosave: Option<Instant>,
    prepared: Option<RequirementsSnapshot>,
    pending_save: Option<PendingSaveConflict>,
    file: Option<ProjectDatabase>,
    stamp: Option<FileStamp>,
    ownership: Option<File>,
    saved_revision: Option<u64>,
    untitled: bool,
}

impl OpenProject {
    fn dirty(&self) -> bool {
        self.untitled || Some(self.session.project().revision) != self.saved_revision
    }

    /// A pending save conflict describes one destination and one project revision. Every
    /// transition that could change either of those — a save attempt, a resolution, or a
    /// wholesale swap of the tab's state — ends the choice here.
    fn invalidate_pending_save(&mut self) {
        self.pending_save = None;
    }
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectSummary {
    id: Uuid,
    name: String,
    dirty: bool,
    active: bool,
    pages: Vec<OpenPageSummary>,
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPageSummary {
    id: Uuid,
    name: String,
}

impl Workspace {
    fn session(&self) -> Option<&Session> {
        self.active.as_ref().map(|open| &open.session)
    }
    fn session_mut(&mut self) -> Option<&mut Session> {
        self.active.as_mut().map(|open| &mut open.session)
    }
    fn with_session<T>(
        &mut self,
        change: impl FnOnce(&mut Session) -> desegnos_core::Result<T>,
    ) -> NativeResult<T> {
        let session = self
            .session_mut()
            .ok_or("Create or open a project first.")?;
        let result = change(session).map_err(message)?;
        self.autosave_if_due();
        Ok(result)
    }
    fn file(&self) -> Option<&ProjectDatabase> {
        self.active.as_ref().and_then(|open| open.file.as_ref())
    }
    fn is_untitled(&self) -> bool {
        self.active.as_ref().is_some_and(|open| open.untitled)
    }

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
        self.session()
            .map(Session::project)
            .ok_or_else(|| "Create or open a project first.".into())
    }

    /// The newest saved version, recorded with an export.
    pub(super) fn head_version(&mut self) -> NativeResult<i64> {
        self.file()
            .ok_or("Save this project before exporting it.")?
            .head_version()
            .map_err(message)
    }

    /// Record a completed export so clashes compare against it. It is metadata about the
    /// design, not an edit, so it never adds an undo step.
    pub(super) fn record_export(
        &mut self,
        record: desegnos_core::ExportRecord,
    ) -> NativeResult<()> {
        self.with_session(|session| session.record_export(record))
    }

    pub(super) fn import_website(
        &mut self,
        imported: desegnos_core::ImportedWebsite,
        pictures: BTreeMap<Uuid, String>,
    ) -> NativeResult<WorkspaceView> {
        self.with_session(|session| {
            let revision = session.project().revision;
            session.execute(
                revision,
                Command::ImportWebsite {
                    pages: imported.pages,
                    reference: imported.reference,
                    pointers: imported.pointers,
                    pictures,
                },
            )
        })?;
        Ok(self.view())
    }

    /// Keep code: replace one pointer's comment with what its marked code now does.
    pub(super) fn rewrite_pointer_comment(
        &mut self,
        pointer_id: Uuid,
        comment: &str,
        fingerprint: &str,
    ) -> NativeResult<()> {
        let wiring = desegnos_core::PointerWiring {
            fingerprint: fingerprint.to_owned(),
            generated_comment: comment.to_owned(),
        };
        self.with_session(|session| {
            let revision = session.project().revision;
            session.execute(
                revision,
                Command::SetPointerWiring {
                    pointer_id,
                    wiring: Some(wiring),
                    comment: Some(comment.to_owned()),
                },
            )
        })
    }

    pub(super) fn record_generated(
        &mut self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        attempt_ordinal: usize,
    ) -> NativeResult<()> {
        self.with_session(|session| session.record_generation(snapshot, candidate, attempt_ordinal))
    }

    pub(super) fn apply_generated(
        &mut self,
        snapshot: &RequirementsSnapshot,
        candidate: &GenerationCandidate,
        accepted_decision_indices: &[usize],
        generated_pictures: &std::collections::BTreeMap<String, String>,
    ) -> NativeResult<()> {
        self.with_session(|session| {
            session.apply_generation(
                snapshot,
                candidate,
                accepted_decision_indices,
                generated_pictures,
            )
        })
    }

    pub fn initialize(
        app: &AppHandle,
        directory: PathBuf,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut workspace = Self::from_directory(directory)?;
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
            switch_guard: crate::ai_job::SwitchGuard::default(),
            active: None,
            recoveries: vec![],
            recovery_error: None,
            autosave_error: None,
            recent_error: None,
            parked: vec![],
            order: vec![],
        };
        state.refresh_recoveries();
        Ok(state)
    }

    fn pointer_path(&self, project_id: Uuid) -> PathBuf {
        self.directory.join(format!("{project_id}.recovery.json"))
    }

    fn acquire_ownership(&self, project_id: Uuid) -> NativeResult<Option<File>> {
        let path = self.directory.join(format!("{project_id}.session.lock"));
        let file = open_lock_file(&path).map_err(message)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(error) => Err(message(error)),
        }
    }

    fn read_pointer(&self, project_id: Uuid) -> NativeResult<SessionPointer> {
        read_json(
            &self.pointer_path(project_id),
            SMALL_RECORD_BYTES,
            "session record",
        )
    }

    fn write_pointer(&self, project_id: Uuid, path: &Path, untitled: bool) -> NativeResult<()> {
        let pointer = SessionPointer {
            project_path: path.to_owned(),
            untitled,
        };
        write_text(
            &self.pointer_path(project_id),
            &serde_json::to_string(&pointer).map_err(message)?,
        )
        .map_err(message)
    }

    fn untitled_path(&self, project_id: Uuid) -> PathBuf {
        self.directory
            .join("untitled")
            .join(format!("{project_id}.desegnos"))
    }

    fn remove_untitled_database(&self, project_id: Uuid) {
        if let Err(error) = remove_project_files(&self.untitled_path(project_id)) {
            eprintln!("Could not remove the untitled recovery database: {error}");
        }
    }

    fn remove_pointer(&self, project_id: Uuid) {
        let _ = fs::remove_file(self.pointer_path(project_id));
    }

    /// The unsaved work a crashed or closed session left in this project file.
    fn read_recovery(
        &self,
        project_id: Uuid,
    ) -> NativeResult<Option<(AutosaveProject, PathBuf, bool)>> {
        let pointer = self.read_pointer(project_id)?;
        if !pointer.project_path.exists() {
            // The project moved or was deleted; its unsaved work went with it.
            self.remove_pointer(project_id);
            return Ok(None);
        }
        let Some(work) = desegnos_storage::read_autosave(&pointer.project_path).map_err(message)?
        else {
            return Ok(None);
        };
        if work.project.id != project_id {
            return Err(
                "The session record and project identity disagree. The originals remain on disk."
                    .into(),
            );
        }
        Ok(Some((work, pointer.project_path, pointer.untitled)))
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
                Ok(Some((work, path, untitled))) => {
                    let modified = fs::metadata(&path)
                        .and_then(|metadata| metadata.modified())
                        .unwrap_or(SystemTime::UNIX_EPOCH);
                    candidates.push(RecoveryCandidate {
                        info: RecoveryInfo {
                            project_id,
                            name: work.project.name,
                            revision: work.project.revision,
                            saved_path: (!untitled).then(|| path.to_string_lossy().into_owned()),
                            edited_at_ms: work.created_at.max(0).try_into().unwrap_or(u64::MAX),
                        },
                        modified,
                    });
                }
                Ok(None) => {
                    // An untitled session that never edited has nothing to offer; its
                    // recovery database would otherwise accumulate on every launch.
                    if self
                        .read_pointer(project_id)
                        .is_ok_and(|pointer| pointer.untitled)
                    {
                        self.remove_pointer(project_id);
                        self.remove_untitled_database(project_id);
                    }
                }
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
        WorkspaceView {
            can_generate_from_brief: self.session().is_some_and(|session| {
                let project = session.project();
                !project.brief.purpose.trim().is_empty()
                    && project
                        .pages
                        .first()
                        .is_some_and(|page| page.elements.is_empty())
                    && !project
                        .requests
                        .iter()
                        .any(|request| !request.text.trim().is_empty())
            }),
            pointer_states: self
                .session()
                .map(|session| session.project().pointer_states())
                .unwrap_or_default(),
            project: self.session().map(|s| s.project().clone()),
            path: (!self.is_untitled())
                .then_some(self.file())
                .flatten()
                .map(|f| f.path().to_string_lossy().into_owned()),
            dirty: self.active_dirty(),
            untitled: self.is_untitled(),
            can_undo: self.session().is_some_and(Session::can_undo),
            can_redo: self.session().is_some_and(Session::can_redo),
            history: self.session().map(|session| {
                let history = session.history();
                HistoryView {
                    session_id: history.session_id,
                    start: history.start,
                    position: history.position,
                    end: history.end,
                    version: history.version,
                    entries: session.history_entries().to_vec(),
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
            recovery_error: self
                .autosave_error
                .clone()
                .or_else(|| self.recovery_error.clone()),
            open_projects: self.open_projects(),
            any_dirty: self.parked.iter().any(OpenProject::dirty) || self.active_dirty(),
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
        self.active.as_ref().is_some_and(OpenProject::dirty)
    }

    /// End the active project's pending save conflict; see
    /// [`OpenProject::invalidate_pending_save`].
    fn invalidate_pending_save(&mut self) {
        if let Some(open) = self.active.as_mut() {
            open.invalidate_pending_save();
        }
    }

    /// "Discard and continue": the unsaved work is gone, so it is no longer offered for
    /// recovery and no longer blocks opening the saved file.
    fn set_aside_recovery(&mut self) -> NativeResult<()> {
        let Some(id) = self.session().map(|session| session.project().id) else {
            return Ok(());
        };
        if let Some(file) = self.file() {
            file.discard_autosave().map_err(message)?;
        }
        self.remove_pointer(id);
        self.active.as_mut().expect("active project").last_autosave = None;
        Ok(())
    }

    /// Close the active project and activate the next open one in tab order. The caller opens
    /// a fresh untitled canvas when no project remains.
    fn close(&mut self) {
        let position = self.session().and_then(|session| {
            let id = session.project().id;
            self.order.iter().position(|other| *other == id)
        });
        let untitled = if self.is_untitled() {
            self.session().map(|session| session.project().id)
        } else {
            None
        };
        if let Some(session) = self.session() {
            let id = session.project().id;
            self.order.retain(|other| *other != id);
        }
        self.active = None;
        if let Some(id) = untitled {
            self.remove_untitled_database(id);
            self.remove_pointer(id);
        }
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

    /// Close one open project tab. Unsaved changes need an explicit discard; discarding drops
    /// the autosave so the edits are no longer offered for recovery.
    fn close_tab(&mut self, project_id: Option<Uuid>, discard: bool) -> NativeResult<()> {
        self.switch_guard.ensure_switch_allowed()?;
        let Some(target) = project_id.or_else(|| self.session().map(|s| s.project().id)) else {
            return Ok(());
        };
        if self
            .session()
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
            if let Some(file) = project.file.as_ref()
                && let Err(error) = file.discard_autosave()
            {
                self.parked.insert(index, project);
                return Err(message(error));
            }
            self.remove_pointer(target);
        }
        let untitled = project.untitled;
        drop(project);
        if untitled {
            self.remove_untitled_database(target);
        }
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
                if let Some(session) = self.session().filter(|s| s.project().id == *id) {
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

    fn park_active(&mut self) {
        if self.active.is_some() {
            self.autosave_if_due();
        }
        if let Some(mut project) = self.active.take() {
            project.prepared = None;
            project.invalidate_pending_save();
            project.last_autosave = None;
            self.parked.push(project);
        }
    }

    fn unpark(&mut self, project: OpenProject) {
        self.active = Some(project);
    }

    fn switch_to(&mut self, project_id: Uuid) -> NativeResult<()> {
        self.switch_guard.ensure_switch_allowed()?;
        if self.session().is_some_and(|s| s.project().id == project_id) {
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
    fn open_tab<T, E: From<String>>(
        &mut self,
        open: impl FnOnce(&mut Self) -> Result<T, E>,
    ) -> Result<T, E> {
        self.switch_guard.ensure_switch_allowed()?;
        let previous = self.session().map(|s| s.project().id);
        self.park_active();
        match open(self) {
            Ok(value) => Ok(value),
            Err(error) => {
                if self.session().is_none()
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
        file: Option<ProjectDatabase>,
        loaded: LoadedProject,
        discard_recovery: bool,
    ) -> OpenResult<()> {
        let revision = loaded.project.revision;
        self.start_with_recovery(
            loaded.project,
            file,
            Some(revision),
            discard_recovery,
            false,
        )?;
        self.remember_recent();
        Ok(())
    }

    /// Opens a project file in its own tab. Choosing the active project's own file again reloads
    /// it from disk with fresh history, unless it has unsaved edits, which are kept as they are.
    fn open_file_tab(&mut self, path: &std::path::Path, discard_recovery: bool) -> OpenResult<()> {
        self.switch_guard.ensure_switch_allowed()?;
        if path.is_dir()
            && let Some(file_path) = desegnos_storage::project_in_folder(path).map_err(message)?
        {
            return self.open_file_tab(&file_path, discard_recovery);
        }
        let active = self.file().is_some_and(|file| {
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
        let (file, loaded) = if let Some(current) = self.file().filter(|file| {
            file.path() == path || path.canonicalize().ok().as_deref() == Some(file.path())
        }) {
            // Reload through the handle this tab already holds.
            (None, current.load().map_err(message)?)
        } else {
            let file = ProjectDatabase::open(path).map_err(message)?;
            let loaded = file.load().map_err(message)?;
            (Some(file), loaded)
        };
        self.open_document(file, loaded, discard_recovery)
    }

    fn remember_recent(&mut self) {
        if let (Some(file), Some(session)) = (self.file(), self.session()) {
            self.recent_error =
                crate::recent::remember(&self.directory, file.path(), &session.project().name)
                    .err();
        }
    }

    /// Saving renames a recent entry but does not count as opening it.
    fn remember_recent_saved(&mut self) {
        if let (Some(file), Some(session)) = (self.file(), self.session()) {
            self.recent_error = crate::recent::remember_saved(
                &self.directory,
                file.path(),
                &session.project().name,
            )
            .err();
        }
    }

    fn start_with_recovery(
        &mut self,
        project: Project,
        file: Option<ProjectDatabase>,
        saved_revision: Option<u64>,
        discard_recovery: bool,
        untitled: bool,
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
            .session()
            .is_some_and(|session| session.project().id == project.id);
        let ownership = if same_project {
            None
        } else {
            let ownership = self
                .acquire_ownership(project.id)?
                .ok_or("This project is already active in another Desegnos window.")?;
            // A reload keeps the tab's own file handle, so look at that one too.
            let checked = file
                .as_ref()
                .or_else(|| self.file().filter(|_| same_project));
            if let Some(file) = checked {
                match file.autosave_work() {
                    Ok(Some(work)) if work.project != project => {
                        if !discard_recovery {
                            return Err(ProjectOpenError::Recovery {
                                code: "pendingRecovery",
                                project_id: project.id,
                                path: file.path().to_path_buf(),
                            });
                        }
                        // Ownership stays held across dismissal and activation.
                        file.discard_autosave().map_err(message)?;
                        self.remove_pointer(project.id);
                        self.refresh_recoveries();
                    }
                    Ok(_) => {}
                    Err(error) => {
                        return Err(format!(
                            "This project's unsaved work needs attention before it can be replaced: {error}"
                        )
                        .into());
                    }
                }
            }
            Some(ownership)
        };
        self.activate(project, file, saved_revision, ownership, untitled)
            .map_err(Into::into)
    }

    fn activate(
        &mut self,
        project: Project,
        file: Option<ProjectDatabase>,
        saved_revision: Option<u64>,
        ownership: Option<File>,
        untitled: bool,
    ) -> NativeResult<()> {
        self.switch_guard.ensure_switch_allowed()?;
        let session = Session::new(project).map_err(message)?;
        let id = session.project().id;
        let stamp = match file.as_ref() {
            Some(file) => Some(file.stamp().map_err(message)?),
            // A reload keeps the handle and stamp the tab already holds.
            None => None,
        };
        if let Some(file) = file.as_ref() {
            self.write_pointer(id, file.path(), untitled)?;
        }
        if !self.order.contains(&id) {
            self.order.push(id);
        }
        let previous = self.active.take();
        let (previous_file, previous_stamp, previous_ownership) = previous
            .map(|open| (open.file, open.stamp, open.ownership))
            .unwrap_or_default();
        let has_file = file.is_some();
        self.active = Some(OpenProject {
            session,
            file: file.or(previous_file),
            stamp: if has_file { stamp } else { previous_stamp },
            ownership: ownership.or(previous_ownership),
            saved_revision,
            untitled,
            last_autosave: None,
            prepared: None,
            pending_save: None,
        });
        self.refresh_recoveries();
        Ok(())
    }

    fn check_snapshot(&self, project: &Project) -> NativeResult<()> {
        let current = self.session().ok_or("The project was closed.")?.project();
        if current.id != project.id || current.revision != project.revision {
            return Err("The project changed while the file dialog was open. Save again from the current project.".into());
        }
        Ok(())
    }

    /// Keep unsaved in-memory edits on the hidden autosave branch every few seconds.
    fn autosave_if_due(&mut self) {
        if !self.active_dirty()
            || self
                .active
                .as_ref()
                .and_then(|open| open.last_autosave)
                .is_some_and(|at| at.elapsed() < AUTOSAVE_INTERVAL)
        {
            return;
        }
        let (Some(session), Some(file)) = (self.session(), self.file()) else {
            return;
        };
        match file.autosave(
            session.project(),
            desegnos_storage::project_db::now_millis(),
        ) {
            Ok(()) => {
                self.active.as_mut().expect("active project").last_autosave = Some(Instant::now());
                self.autosave_error = None;
            }
            Err(error) => {
                self.autosave_error = Some(format!("Could not keep unsaved work: {error}"));
            }
        }
    }

    /// Flush every tab without throttling or saving to the user's saved branch.
    pub(super) fn flush_recovery(&self) {
        for open in self.active.iter().chain(&self.parked) {
            if open.dirty()
                && let Some(file) = &open.file
                && let Err(error) = file.autosave(
                    open.session.project(),
                    desegnos_storage::project_db::now_millis(),
                )
            {
                eprintln!("Could not keep unsaved work: {error}");
            }
        }
    }

    fn take_version_labels(&mut self) -> Vec<String> {
        self.session_mut()
            .map(Session::take_version_labels)
            .unwrap_or_default()
    }

    fn save_destination(&self, path: Option<PathBuf>) -> NativeResult<SaveDestination> {
        let Some(path) = path else {
            if self.is_untitled() {
                return Err("Choose a name and location for this project first.".into());
            }
            return if self.file().is_some() {
                Ok(SaveDestination::Current)
            } else {
                let project = self.project()?;
                let path = desegnos_storage::create_project_folder(
                    &self.project_location()?,
                    &project.name,
                )
                .map_err(message)?;
                Ok(SaveDestination::New(path))
            };
        };
        let path = if path.extension().is_none() {
            path.with_extension("desegnos")
        } else {
            path
        };
        let current = self.file().is_some_and(|file| {
            file.path() == path || path.canonicalize().ok().as_deref() == Some(file.path())
        });
        if current {
            Ok(SaveDestination::Current)
        } else if path.exists() {
            Ok(SaveDestination::Existing(path))
        } else {
            Ok(SaveDestination::New(path))
        }
    }

    fn destination_path(&self, destination: &SaveDestination) -> NativeResult<PathBuf> {
        match destination {
            SaveDestination::Current => self
                .file()
                .map(|file| file.path().to_path_buf())
                .ok_or_else(|| "Choose a project file first.".into()),
            SaveDestination::New(path) | SaveDestination::Existing(path) => Ok(path.clone()),
        }
    }

    fn finish_save(&mut self, project: &Project) -> NativeResult<()> {
        self.active.as_mut().expect("active project").stamp = self
            .file()
            .map(|file| file.stamp().map_err(message))
            .transpose()?;
        if let Some(file) = self.file() {
            self.write_pointer(project.id, file.path(), false)?;
        }
        self.active.as_mut().expect("active project").saved_revision = Some(project.revision);
        self.active.as_mut().expect("active project").last_autosave = None;
        self.remember_recent_saved();
        Ok(())
    }

    /// Write the project to its destination. A new path becomes the file this tab saves to.
    fn write_destination(
        &mut self,
        destination: SaveDestination,
        project: &Project,
        labels: &[String],
        label: Option<&str>,
    ) -> NativeResult<()> {
        let now = desegnos_storage::project_db::now_millis();
        match destination {
            SaveDestination::Current => {
                let file = self.file().ok_or("Choose a project file first.")?;
                let stamp = self.active.as_ref().and_then(|open| open.stamp.clone());
                file.save_checked(project, stamp.as_ref(), labels, label, now)
                    .map_err(|failure| failure.message)?;
            }
            SaveDestination::New(path) => {
                let file = ProjectDatabase::create(&path, project, now).map_err(message)?;
                self.active.as_mut().expect("active project").file = Some(file);
            }
            SaveDestination::Existing(path) => {
                let file = ProjectDatabase::open(&path).map_err(message)?;
                file.save(project, labels, label, now).map_err(message)?;
                self.active.as_mut().expect("active project").file = Some(file);
            }
        }
        self.finish_save(project)
    }

    /// The checked write used by the editor's Save: conflicts keep their established choices.
    fn write_destination_checked(
        &mut self,
        destination: SaveDestination,
        project: &Project,
        labels: &[String],
        label: Option<&str>,
    ) -> std::result::Result<(), Box<SaveFailure>> {
        let now = desegnos_storage::project_db::now_millis();
        match destination {
            SaveDestination::Current => {
                let file = self.file().ok_or_else(|| {
                    SaveFailure::new(SaveFailureKind::Io, "Choose a project file first.")
                })?;
                let stamp = self.active.as_ref().and_then(|open| open.stamp.clone());
                file.save_checked(project, stamp.as_ref(), labels, label, now)?;
            }
            SaveDestination::New(path) => {
                let file = ProjectDatabase::create(&path, project, now)
                    .map_err(|error| SaveFailure::new(SaveFailureKind::Io, error.to_string()))?;
                self.active.as_mut().expect("active project").file = Some(file);
            }
            SaveDestination::Existing(path) => {
                let file = ProjectDatabase::open(&path).map_err(|error| {
                    SaveFailure::new(SaveFailureKind::Unreadable, error.to_string())
                })?;
                let saved = file.save_checked(project, None, labels, label, now);
                if saved.is_ok() {
                    self.active.as_mut().expect("active project").file = Some(file);
                }
                saved?;
            }
        }
        self.finish_save(project)
            .map_err(|error| SaveFailure::new(SaveFailureKind::Io, error))
    }

    fn next_untitled_name(&self, template: CanvasTemplate) -> String {
        let base = match template {
            CanvasTemplate::Blank => "Untitled canvas",
            CanvasTemplate::Website => "Untitled website",
            CanvasTemplate::AppScreen => "Untitled app",
            CanvasTemplate::SlideDeck => "Untitled deck",
            CanvasTemplate::SocialImage => "Untitled image",
        };
        let names = self
            .session()
            .map(|session| session.project().name.clone())
            .into_iter()
            .chain(
                self.parked
                    .iter()
                    .map(|project| project.session.project().name.clone()),
            );
        lowest_unused_name(base, names)
    }

    /// Open an untitled canvas whose recovery database lives in the app data folder until the
    /// user chooses a project file for it.
    fn create_untitled(
        &mut self,
        appearance: Appearance,
        template: CanvasTemplate,
    ) -> NativeResult<()> {
        let project = Project::untitled(self.next_untitled_name(template), appearance, template)
            .map_err(message)?;
        let path = self.untitled_path(project.id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(message)?;
        }
        let file =
            ProjectDatabase::create(&path, &project, desegnos_storage::project_db::now_millis())
                .map_err(message)?;
        let revision = project.revision;
        self.open_tab(|state| {
            state
                .start_with_recovery(project, Some(file), Some(revision), false, true)
                .map_err(String::from)
        })
    }

    /// The first save gives the untitled project its real file; the recovery copy is removed.
    fn finish_untitled_save(&mut self, project: &Project) {
        if self.is_untitled() {
            self.active.as_mut().expect("active project").untitled = false;
            let recovery_path = self.untitled_path(project.id);
            if self.file().is_none_or(|file| file.path() != recovery_path) {
                self.remove_untitled_database(project.id);
            }
        }
    }

    fn save_untitled_checked(
        &mut self,
        project: &Project,
        path: PathBuf,
        label: Option<String>,
    ) -> NativeResult<SaveProjectOutcome> {
        self.check_snapshot(project)?;
        self.invalidate_pending_save();
        let labels = self.take_version_labels();
        let destination = self.save_destination(Some(path))?;
        match self.write_destination_checked(
            destination.clone(),
            project,
            &labels,
            label.as_deref(),
        ) {
            Ok(()) => {
                self.finish_untitled_save(project);
                Ok(SaveProjectOutcome::Saved {
                    workspace: Box::new(self.view()),
                })
            }
            Err(failure) => self.save_conflict(project, destination, *failure, true, labels, label),
        }
    }

    fn save_snapshot(&mut self, project: &Project, path: Option<PathBuf>) -> NativeResult<()> {
        self.check_snapshot(project)?;
        self.invalidate_pending_save();
        let labels = self.take_version_labels();
        let destination = self.save_destination(path)?;
        self.write_destination(destination, project, &labels, None)?;
        self.finish_untitled_save(project);
        Ok(())
    }

    /// Write a new project file holding only the current version.
    fn save_copy(&mut self, project: &Project, path: PathBuf) -> NativeResult<()> {
        self.check_snapshot(project)?;
        let path = if path.extension().is_none() {
            path.with_extension("desegnos")
        } else {
            path
        };
        ProjectDatabase::create(&path, project, desegnos_storage::project_db::now_millis())
            .map_err(message)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn save_conflict(
        &mut self,
        project: &Project,
        destination: SaveDestination,
        failure: SaveFailure,
        save_as: bool,
        labels: Vec<String>,
        label: Option<String>,
    ) -> NativeResult<SaveProjectOutcome> {
        if !matches!(
            failure.kind,
            SaveFailureKind::Changed
                | SaveFailureKind::DifferentProject
                | SaveFailureKind::Unreadable
        ) {
            return Err(failure.message);
        }
        let path = self.destination_path(&destination)?;
        let path = path.to_string_lossy().into_owned();
        let file_name = Path::new(&path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project.desegnos".into());
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
            }
            .into(),
            path,
            file_name,
            message,
            default_choice: if different { "cancel" } else { "copy" },
            can_replace,
            details: SaveConflictDetails {
                expected: failure.expected.map(Into::into),
                actual: failure.actual.clone().map(Into::into),
            },
        };
        self.active.as_mut().expect("active project").pending_save = Some(PendingSaveConflict {
            token,
            project_id: project.id,
            revision: project.revision,
            destination,
            actual: failure.actual,
            can_replace,
            save_as,
            labels,
            label,
        });
        Ok(SaveProjectOutcome::Conflict { conflict })
    }

    fn save_snapshot_checked(
        &mut self,
        project: &Project,
        path: Option<PathBuf>,
        save_as: bool,
        label: Option<String>,
    ) -> NativeResult<SaveProjectOutcome> {
        self.check_snapshot(project)?;
        self.invalidate_pending_save();
        let labels = self.take_version_labels();
        let destination = self.save_destination(path)?;
        match self.write_destination_checked(
            destination.clone(),
            project,
            &labels,
            label.as_deref(),
        ) {
            Ok(()) => {
                self.finish_untitled_save(project);
                Ok(SaveProjectOutcome::Saved {
                    workspace: Box::new(self.view()),
                })
            }
            Err(failure) => {
                self.save_conflict(project, destination, *failure, save_as, labels, label)
            }
        }
    }

    fn take_save_conflict(&mut self, token: Uuid) -> NativeResult<PendingSaveConflict> {
        let pending = self
            .active
            .as_ref()
            .and_then(|open| open.pending_save.as_ref())
            .filter(|pending| pending.token == token)
            .ok_or("This save choice expired. Save again.")?;
        let current = self.project()?;
        if current.id != pending.project_id || current.revision != pending.revision {
            return Err("The project changed while this save choice was open. Save again.".into());
        }
        // Choosing replace or copy consumes the conflict, the same way cancelling ends it.
        self.active
            .as_mut()
            .expect("active project")
            .pending_save
            .take()
            .ok_or_else(|| "This save choice expired. Save again.".into())
    }

    fn cancel_save_conflict(&mut self, token: Uuid) -> NativeResult<()> {
        if self
            .active
            .as_ref()
            .and_then(|open| open.pending_save.as_ref())
            .is_none_or(|pending| pending.token != token)
        {
            return Err("This save choice expired. Save again.".into());
        }
        // Cancelling is a resolution: the choice ends like any other.
        self.invalidate_pending_save();
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
            .clone()
            .ok_or("This file cannot be replaced. Save a copy with a new name.")?;
        let now = desegnos_storage::project_db::now_millis();
        let result: NativeResult<()> = (|| {
            match &pending.destination {
                SaveDestination::Current => {
                    let file = self.file().ok_or("Choose a project file first.")?;
                    file.save_replacing(
                        &project,
                        &observed,
                        pending.save_as,
                        &pending.labels,
                        pending.label.as_deref(),
                        desegnos_storage::project_db::now_millis(),
                    )
                    .map(|_| ())
                    .map_err(|failure| failure.message)?;
                }
                SaveDestination::New(path) => {
                    let file = ProjectDatabase::create(path, &project, now).map_err(message)?;
                    self.active.as_mut().expect("active project").file = Some(file);
                }
                SaveDestination::Existing(path) => {
                    let file = ProjectDatabase::open(path).map_err(message)?;
                    file.save_replacing(
                        &project,
                        &observed,
                        true,
                        &pending.labels,
                        pending.label.as_deref(),
                        desegnos_storage::project_db::now_millis(),
                    )
                    .map(|_| ())
                    .map_err(|failure| failure.message)?;
                    self.active.as_mut().expect("active project").file = Some(file);
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.finish_save(&project)?;
                self.finish_untitled_save(&project);
                Ok(SaveProjectOutcome::Saved {
                    workspace: Box::new(self.view()),
                })
            }
            Err(error) => {
                let failure = SaveFailure {
                    kind: SaveFailureKind::Io,
                    message: error,
                    expected: pending.actual.clone(),
                    actual: None,
                };
                self.save_conflict(
                    &project,
                    pending.destination,
                    failure,
                    pending.save_as,
                    pending.labels,
                    pending.label,
                )
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
        let (_, path, untitled) = self
            .read_recovery(project_id)?
            .ok_or("There are no unsaved changes to recover for this project.")?;
        ProjectDatabase::open(&path)
            .map_err(message)?
            .discard_autosave()
            .map_err(message)?;
        self.remove_pointer(project_id);
        if untitled {
            self.remove_untitled_database(project_id);
        }
        self.refresh_recoveries();
        Ok(())
    }

    fn recover(&mut self, project_id: Option<Uuid>) -> NativeResult<()> {
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
        let (work, path, untitled) = self
            .read_recovery(project_id)?
            .ok_or("There are no unsaved changes to recover for this project.")?;
        let file = ProjectDatabase::open(&path).map_err(message)?;
        let saved_revision = Some(file.load().map_err(message)?.project.revision);
        self.activate(
            work.project,
            Some(file),
            saved_revision,
            Some(ownership),
            untitled,
        )?;
        Ok(())
    }
}

#[tauri::command(async)]
pub fn get_workspace(
    state: WorkspaceState<'_>,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.refresh_recoveries();
        Ok(state.view())
    })
}

#[tauri::command]
pub fn new_untitled_project(
    state: WorkspaceState<'_>,
    appearance: Option<Appearance>,
    template: Option<CanvasTemplate>,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.create_untitled(
            appearance.unwrap_or(Appearance::Light),
            template.unwrap_or_default(),
        )?;
        Ok(state.view())
    })
}

/// The first save of an untitled project: name it and write the chosen `.desegnos` file.
#[tauri::command]
pub fn save_untitled_project(
    state: WorkspaceState<'_>,
    name: String,
    path: Option<PathBuf>,
    label: Option<String>,
) -> Result<SaveProjectOutcome, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::SaveLocation.run(
        || -> NativeResult<SaveProjectOutcome> {
            let mut state = state.locked();
            if !state.is_untitled() {
                return Err("This project already has a file. Use Save instead.".into());
            }
            let name = name.trim();
            if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
                return Err(format!(
                    "Enter a project name of up to {MAX_NAME_CHARS} characters."
                ));
            }
            let current = state.project()?.clone();
            if current.name != name {
                let revision = current.revision;
                state.with_session(|session| {
                    session.execute(
                        revision,
                        Command::RenameProject {
                            name: name.to_owned(),
                        },
                    )
                })?;
            }
            if let Some(session) = state.session_mut() {
                session.end_history_step();
            }
            let path = match path {
                Some(path) => path,
                None => free_project_path(&state.project_location()?, name)?,
            };
            let project = state.project()?.clone();
            state.save_untitled_checked(&project, path, label)
        },
    )
}

/// Without a picked file, the first save goes to the default project folder under the
/// project's name, numbered so it never replaces another project's file.
fn free_project_path(directory: &Path, name: &str) -> NativeResult<PathBuf> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("The project folder could not be created: {error}"))?;
    let stem = desegnos_storage::sanitize_project_name(name);
    (1..1000)
        .map(|n| {
            directory.join(if n == 1 {
                format!("{stem}.desegnos")
            } else {
                format!("{stem} {n}.desegnos")
            })
        })
        .find(|path| !path.exists())
        .ok_or_else(|| "Choose a location for this project.".into())
}

/// Pick the project file for an untitled project's first save.
#[tauri::command]
pub async fn pick_project_save_path(
    app: AppHandle,
    state: WorkspaceState<'_>,
    name: String,
) -> Result<Option<PathBuf>, crate::native_error::NativeFailure> {
    let result: NativeResult<Option<PathBuf>> = async {
        let directory = {
            let state = state.locked();
            state.project_location().ok()
        };
        let suggested = format!(
            "{}.desegnos",
            desegnos_storage::sanitize_project_name(&name)
        );
        let picked = tauri::async_runtime::spawn_blocking(move || {
            let mut dialog = app
                .dialog()
                .file()
                .add_filter(
                    "Desegnos project",
                    &desegnos_storage::PROJECT_FILE_EXTENSIONS,
                )
                .set_file_name(suggested);
            if let Some(directory) = directory {
                dialog = dialog.set_directory(directory);
            }
            crate::file_dialogs::picked(dialog.blocking_save_file())
        })
        .await
        .map_err(message)??;
        picked
            .map(|picked| picked.into_path().map_err(message))
            .transpose()
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::SaveLocation.failure())
}

#[tauri::command]
pub fn switch_project(
    state: WorkspaceState<'_>,
    project_id: Uuid,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.switch_to(project_id)?;
        Ok(state.view())
    })
}

#[tauri::command]
pub fn close_project(
    state: WorkspaceState<'_>,
    project_id: Option<Uuid>,
    discard: bool,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.close_tab(project_id, discard)?;
        Ok(state.view())
    })
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
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut context = identity.locked().reviewed_context_selected(
            identity_analysis_id,
            &identity_finding_indices,
            &identity_asset_ids,
            identity_photo_ids.as_deref(),
            identity_item_indices.as_deref(),
            identity_page_paths.as_deref(),
        )?;
        context.attach_matching_pages = attach_matching_pages.unwrap_or(true);
        let mut state = state.locked();
        state.with_session(|session| session.import_context(revision, context))?;
        state.active.as_mut().expect("active project").prepared = None;
        Ok(state.view())
    })
}

impl Workspace {
    /// Save first, so switching alternatives never loses work on the current one.
    fn save_before_switch(&mut self) -> NativeResult<()> {
        if !self.active_dirty() {
            return Ok(());
        }
        let project = self
            .session()
            .ok_or("Create or open a project first.")?
            .project()
            .clone();
        self.save_snapshot(&project, None)
    }

    fn switch_alternative(&mut self, name: &str) -> NativeResult<()> {
        self.save_before_switch()?;
        let file = self.file().ok_or("Save this project first.")?;
        let loaded = file.switch_branch(name).map_err(message)?;
        let saved_revision = loaded.project.revision;
        let session = Session::new(loaded.project).map_err(message)?;
        let stamp = file.stamp().map_err(message)?;
        let active = self.active.as_mut().expect("active project");
        active.session = session;
        active.saved_revision = Some(saved_revision);
        active.stamp = Some(stamp);
        self.active.as_mut().expect("active project").last_autosave = None;
        self.active.as_mut().expect("active project").prepared = None;
        self.invalidate_pending_save();
        self.refresh_recoveries();
        Ok(())
    }

    fn create_alternative(&mut self, name: &str) -> NativeResult<()> {
        self.save_before_switch()?;
        let file = self.file().ok_or("Save this project first.")?;
        file.create_branch(name).map_err(message)?;
        self.active.as_mut().expect("active project").stamp = Some(file.stamp().map_err(message)?);
        self.active.as_mut().expect("active project").saved_revision =
            self.session().map(|session| session.project().revision);
        Ok(())
    }

    fn rename_alternative(&mut self, from: &str, to: &str) -> NativeResult<()> {
        let file = self.file().ok_or("Create or open a project first.")?;
        file.rename_branch(from, to).map_err(message)?;
        Ok(())
    }

    fn delete_alternative(&mut self, name: &str) -> NativeResult<()> {
        let file = self.file().ok_or("Create or open a project first.")?;
        let history = file.history().map_err(message)?;
        if history.branch == name {
            // Leave the alternative before deleting it; switching saves first.
            self.switch_alternative(&history.primary_branch)?;
        }
        let file = self.file().ok_or("Create or open a project first.")?;
        file.delete_branch(name).map_err(message)?;
        self.active.as_mut().expect("active project").stamp = Some(file.stamp().map_err(message)?);
        Ok(())
    }

    /// The saved version becomes the current state as one undo step.
    fn restore_version(&mut self, version: i64) -> NativeResult<()> {
        let file = self.file().ok_or("Create or open a project first.")?;
        let restored = file
            .restore(version, desegnos_storage::project_db::now_millis())
            .map_err(message)?;
        let stamp = file.stamp().map_err(message)?;
        let session = self
            .session_mut()
            .ok_or("Create or open a project first.")?;
        session.restore(restored).map_err(message)?;
        self.active.as_mut().expect("active project").saved_revision =
            Some(session.project().revision);
        self.active.as_mut().expect("active project").stamp = Some(stamp);
        self.active.as_mut().expect("active project").last_autosave = None;
        self.invalidate_pending_save();
        Ok(())
    }
}

#[tauri::command(async)]
pub fn edit_project(
    state: WorkspaceState<'_>,
    revision: u64,
    command: Command,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.with_session(|session| session.execute(revision, command))?;
        Ok(state.view())
    })
}

#[tauri::command(async)]
pub fn undo_project(
    state: WorkspaceState<'_>,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.with_session(|session| session.undo())?;
        Ok(state.view())
    })
}

#[tauri::command(async)]
pub fn redo_project(
    state: WorkspaceState<'_>,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.with_session(|session| session.redo())?;
        Ok(state.view())
    })
}

#[tauri::command(async)]
pub fn end_history_step(
    state: WorkspaceState<'_>,
) -> Result<(), crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<()> {
        let mut state = state.locked();
        if let Some(session) = state.session_mut() {
            session.end_history_step();
        }
        Ok(())
    })
}

#[tauri::command]
pub async fn open_project(
    app: AppHandle,
    state: WorkspaceState<'_>,
    folder: Option<bool>,
    path: Option<PathBuf>,
    discard_recovery: Option<bool>,
) -> Result<Option<WorkspaceView>, crate::native_error::NativeFailure> {
    let result: OpenResult<Option<WorkspaceView>> = async {
        let path = if let Some(path) = path {
            path
        } else {
            let picked = tauri::async_runtime::spawn_blocking(move || {
                let dialog = app.dialog().file();
                crate::file_dialogs::picked(if folder.unwrap_or(false) {
                    dialog
                        .set_title("Open project folder")
                        .blocking_pick_folder()
                } else {
                    dialog
                        .add_filter(
                            "Desegnos project",
                            &desegnos_storage::PROJECT_FILE_EXTENSIONS,
                        )
                        .blocking_pick_file()
                })
            })
            .await
            .map_err(message)??;
            let Some(picked) = picked else {
                return Ok(None);
            };
            picked.into_path().map_err(message)?
        };
        let mut state = state.locked();
        state.open_file_tab(&path, discard_recovery.unwrap_or(false))?;
        Ok(Some(state.view()))
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::Command.failure())
}

async fn choose_save_path(app: AppHandle, suggested: String) -> NativeResult<Option<PathBuf>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        crate::file_dialogs::picked(
            app.dialog()
                .file()
                .add_filter(
                    "Desegnos project",
                    &desegnos_storage::PROJECT_FILE_EXTENSIONS,
                )
                .set_file_name(suggested)
                .blocking_save_file(),
        )
    })
    .await
    .map_err(message)??;
    picked
        .map(|picked| picked.into_path().map_err(message))
        .transpose()
}

/// New Save As / conflict copies get their own folder. Choosing an existing archive still
/// runs the established conflict/Replace flow, including standalone destinations.
fn copy_destination(path: Option<PathBuf>) -> NativeResult<Option<PathBuf>> {
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
            .is_none_or(|ext| !desegnos_storage::PROJECT_FILE_EXTENSIONS.contains(&ext))
        {
            return Err("Project copies must use .desegnos.".into());
        }
        let parent = path.parent().ok_or("Choose a project location.")?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Choose a project name.")?;
        desegnos_storage::create_project_folder(parent, name).map_err(message)
    })
    .transpose()
}

#[tauri::command]
pub async fn save_project_checked(
    app: AppHandle,
    state: WorkspaceState<'_>,
    save_as: bool,
    label: Option<String>,
) -> Result<SaveProjectOutcome, crate::native_error::NativeFailure> {
    let result: NativeResult<SaveProjectOutcome> = async {
        let (project, choose_path) = {
            let mut state = state.locked();
            let project = state.project()?.clone();
            state.invalidate_pending_save();
            (project, save_as)
        };
        let picked = if choose_path {
            let suggested = format!(
                "{}.desegnos",
                desegnos_storage::sanitize_project_name(&project.name)
            );
            let Some(path) = choose_save_path(app, suggested).await? else {
                return Ok(SaveProjectOutcome::Cancelled);
            };
            Some(path)
        } else {
            None
        };
        let mut state = state.locked();
        if let Some(session) = state.session_mut() {
            session.end_history_step();
        }
        state.check_snapshot(&project)?;
        let picked = copy_destination(picked)?;
        state.save_snapshot_checked(&project, picked, choose_path, label)
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::SaveLocation.failure())
}

#[tauri::command]
pub async fn resolve_save_conflict(
    app: AppHandle,
    state: WorkspaceState<'_>,
    token: Uuid,
    choice: SaveConflictChoice,
) -> Result<SaveProjectOutcome, crate::native_error::NativeFailure> {
    let result: NativeResult<SaveProjectOutcome> = async {
        match choice {
            SaveConflictChoice::Replace => state.locked().replace_save_conflict(token),
            SaveConflictChoice::Cancel => {
                state.locked().cancel_save_conflict(token)?;
                Ok(SaveProjectOutcome::Cancelled)
            }
            SaveConflictChoice::Copy => {
                let (project, suggested, label) = {
                    let mut state = state.locked();
                    let pending = state.take_save_conflict(token)?;
                    let stem = state
                        .destination_path(&pending.destination)?
                        .file_stem()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "project".into());
                    (
                        state.project()?.clone(),
                        format!("{stem}-copy.desegnos"),
                        pending.label,
                    )
                };
                let Some(path) = choose_save_path(app, suggested).await? else {
                    return Ok(SaveProjectOutcome::Cancelled);
                };
                let mut state = state.locked();
                state.check_snapshot(&project)?;
                let path = copy_destination(Some(path))?;
                state.save_snapshot_checked(&project, path, true, label)
            }
        }
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::SaveLocation.failure())
}

#[tauri::command]
pub fn dismiss_recovery(
    state: WorkspaceState<'_>,
    project_id: Uuid,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.dismiss_recovery(project_id)?;
        Ok(state.view())
    })
}

#[tauri::command]
pub fn recover_project(
    state: WorkspaceState<'_>,
    project_id: Option<Uuid>,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.recover(project_id)?;
        Ok(state.view())
    })
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProjectsView {
    projects: Vec<crate::recent::RecentProject>,
    error: Option<String>,
}

#[tauri::command]
pub fn get_recent_projects(
    state: WorkspaceState<'_>,
) -> Result<RecentProjectsView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<RecentProjectsView> {
        let state = state.locked();
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
    })
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

#[tauri::command(async)]
pub fn open_recent_project(
    state: WorkspaceState<'_>,
    path: PathBuf,
    discard_recovery: Option<bool>,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> OpenResult<WorkspaceView> {
        open_recent(&state, &path, discard_recovery.unwrap_or(false)).map_err(|error| match error {
            ProjectOpenError::Message(message) => RecentProjectOpenError::from(message).into(),
            error => error,
        })
    })
}

fn open_recent(
    state: &WorkspaceState<'_>,
    path: &Path,
    discard_recovery: bool,
) -> OpenResult<WorkspaceView> {
    let mut state = state.locked();
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
    if let Err(error) = fs::metadata(path) {
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
    state.open_file_tab(path, discard_recovery)?;
    Ok(state.view())
}

/// Write unsaved in-memory edits to the hidden autosave branch, at most every few seconds.
#[tauri::command(async)]
pub fn autosave_project(
    state: WorkspaceState<'_>,
) -> Result<(), crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::SaveLocation.run(|| -> NativeResult<()> {
        state.locked().autosave_if_due();
        Ok(())
    })
}

#[tauri::command(async)]
pub fn get_project_history(
    state: WorkspaceState<'_>,
) -> Result<Option<ProjectHistoryView>, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(
        || -> NativeResult<Option<ProjectHistoryView>> {
            let state = state.locked();
            match state.file() {
                Some(file) => file
                    .history()
                    .map(|history| Some(history.into()))
                    .map_err(message),
                None => Ok(None),
            }
        },
    )
}

/// Restore a saved version. History is never rewritten: the restore is a new version, and it
/// becomes the current state as one undo step.
#[tauri::command(async)]
pub fn restore_version(
    state: WorkspaceState<'_>,
    version: i64,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.restore_version(version)?;
        state.autosave_if_due();
        Ok(state.view())
    })
}

#[tauri::command(async)]
pub fn switch_alternative(
    state: WorkspaceState<'_>,
    branch: String,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.switch_alternative(&branch)?;
        Ok(state.view())
    })
}

#[tauri::command(async)]
pub fn create_alternative(
    state: WorkspaceState<'_>,
    name: String,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.create_alternative(&name)?;
        Ok(state.view())
    })
}

#[tauri::command(async)]
pub fn rename_alternative(
    state: WorkspaceState<'_>,
    from: String,
    to: String,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.rename_alternative(&from, &to)?;
        Ok(state.view())
    })
}

#[tauri::command(async)]
pub fn delete_alternative(
    state: WorkspaceState<'_>,
    name: String,
) -> Result<WorkspaceView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> NativeResult<WorkspaceView> {
        let mut state = state.locked();
        state.delete_alternative(&name)?;
        Ok(state.view())
    })
}

#[tauri::command(async)]
pub async fn save_project_copy(
    app: AppHandle,
    state: WorkspaceState<'_>,
) -> Result<Option<WorkspaceView>, crate::native_error::NativeFailure> {
    let result: NativeResult<Option<WorkspaceView>> = async {
        let (project, suggested) = {
            let state = state.locked();
            let project = state.project()?.clone();
            let suggested = format!(
                "{}-copy.desegnos",
                desegnos_storage::sanitize_project_name(&project.name)
            );
            (project, suggested)
        };
        let Some(path) = choose_save_path(app, suggested).await? else {
            return Ok(None);
        };
        let path = copy_destination(Some(path))?;
        let mut state = state.locked();
        state.check_snapshot(&project)?;
        if let Some(path) = path {
            state.save_copy(&project, path)?;
        }
        Ok(Some(state.view()))
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::SaveLocation.failure())
}

/// Keep recent versions in full, thin out older ones, drop unreferenced blobs and shrink.
#[tauri::command(async)]
pub fn compact_history(
    state: WorkspaceState<'_>,
) -> Result<crate::project_history::CompactReport, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(
        || -> NativeResult<crate::project_history::CompactReport> {
            let state = state.locked();
            let file = state.file().ok_or("Create or open a project first.")?;
            file.compact(desegnos_storage::project_db::now_millis())
                .map(Into::into)
                .map_err(message)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::lowest_unused_name;

    #[test]
    fn untitled_names_use_template_and_lowest_free_suffix() {
        assert_eq!(
            lowest_unused_name("Untitled deck", Vec::new()),
            "Untitled deck"
        );
        assert_eq!(
            lowest_unused_name(
                "Untitled deck",
                ["Untitled deck".into(), "Untitled deck 3".into()]
            ),
            "Untitled deck 2"
        );
        assert_eq!(
            lowest_unused_name("Untitled image", ["Untitled deck".into()]),
            "Untitled image"
        );
    }
}
