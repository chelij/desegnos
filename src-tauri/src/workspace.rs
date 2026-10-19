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
}

impl Workspace {
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
        Self::from_directory(app.path().app_local_data_dir()?)
    }

    fn from_directory(directory: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        fs::create_dir_all(&directory)?;
        let mut state = Self {
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
            && pointer.saved_sha256.as_ref() == Some(&document.source_sha256)
        {
            return Ok(None);
        }
        Ok(Some((document, pointer)))
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

    fn open_document(&mut self, file: ProjectFile, document: DecodedProject) -> NativeResult<()> {
        let migration_notice = document.migration_notice();
        let revision = document.project.revision;
        self.start(document.project, Some(file), Some(revision))?;
        if let Some(notice) = migration_notice {
            self.migration_notice = Some(notice.into());
            self.needs_migration_save = true;
        }
        self.remember_recent();
        Ok(())
    }

    fn open_path(&mut self, path: &std::path::Path) -> NativeResult<()> {
        let (file, document) = if let Some(current) = self.file.as_ref().filter(|file| {
            file.path() == path || path.canonicalize().ok().as_deref() == Some(file.path())
        }) {
            current.reopen().map_err(message)?
        } else {
            let file = ProjectFile::acquire(path).map_err(message)?;
            let document = file.load_document().map_err(message)?;
            (file, document)
        };
        self.open_document(file, document)
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
                        return Err("This project has unsaved recovery data. Recover it before opening its older saved file.".into());
                    }
                    Ok(_) => {}
                    Err(error) => {
                        return Err(format!(
                            "This project's recovery record needs attention before it can be replaced: {error}"
                        ));
                    }
                }
            }
            Some(ownership)
        };
        self.activate(project, file, saved_revision, ownership, None)
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
        )?;
        self.pending_save = None;
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
    ) -> NativeResult<()> {
        let pointer = RecoveryPointer {
            journal_file: journal
                .path()
                .file_name()
                .ok_or("Invalid recovery filename.")?
                .to_string_lossy()
                .into_owned(),
            saved_revision,
            saved_sha256: file
                .map(|file| -> NativeResult<String> {
                    let saved = file.saved_document().map_err(message)?;
                    if saved.project.id != project_id || Some(saved.project.revision) != saved_revision {
                        return Err("The saved file changed before its recovery state could be recorded. The project remains unsaved.".into());
                    }
                    Ok(saved.source_sha256)
                })
                .transpose()?,
            project_path: file.map(|f| f.path().to_owned()).or(last_saved_path),
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
            return Ok(SaveDestination::Current);
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

    fn recover(&mut self, project_id: Option<Uuid>, discard: bool) -> NativeResult<()> {
        self.check_replace(discard)?;
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
            self.migration_notice = Some(notice.into());
            self.needs_migration_save = true;
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
) -> NativeResult<WorkspaceView> {
    let _ = appearance; // Older callers may still supply it; all new projects start light.
    generation
        .lock()
        .map_err(message)?
        .ensure_switch_allowed()?;
    let mut state = state.lock().map_err(message)?;
    state.check_replace(discard)?;
    let project = if let Some(id) = identity_analysis_id {
        identity.lock().map_err(message)?.project_from_analysis(
            id,
            &identity_finding_indices.unwrap_or_default(),
            &identity_asset_ids.unwrap_or_default(),
            name,
            viewport,
        )?
    } else {
        if identity_finding_indices
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
        Project::new(name, viewport, Appearance::Light).map_err(message)?
    };
    state.start(project, None, None)?;
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
    open::that_detached(url).map_err(|_| "The source could not be opened in your browser.".into())
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

#[tauri::command]
pub fn inspect_element(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    target: desegni_core::ElementTarget,
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
    project.inspect_element(page_id, &target).map_err(message)
}

#[tauri::command]
pub fn edit_element(
    state: WorkspaceState<'_>,
    project_id: Uuid,
    page_id: Uuid,
    revision: u64,
    target: desegni_core::ElementTarget,
    change: desegni_core::ElementEdit,
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
    let element = session
        .edit_element(revision, page_id, &target, &change, |next| {
            journal.append(next)
        })
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
) -> NativeResult<()> {
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
            .copy_layer(page_id, &target, &visual_styles)
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
    )
    .await?;
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
                .edit_element(
                    revision,
                    page_id,
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
    let selection = session
        .paste_layer(revision, page_id, &packet, after.as_ref(), |next| {
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

#[tauri::command]
pub async fn open_project(
    app: AppHandle,
    state: WorkspaceState<'_>,
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
    discard: bool,
) -> NativeResult<Option<WorkspaceView>> {
    generation
        .lock()
        .map_err(message)?
        .ensure_switch_allowed()?;
    let before = {
        let state = state.lock().map_err(message)?;
        state.check_replace(discard)?;
        state.session.as_ref().map(|s| s.project().clone())
    };
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter(
                "Desegnos website",
                &desegni_storage::PROJECT_FILE_EXTENSIONS,
            )
            .blocking_pick_file()
    })
    .await
    .map_err(message)?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(message)?;
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
    state.check_replace(discard)?;
    state.open_path(&path)?;
    Ok(Some(state.view()))
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
            save_as || state.file.is_none(),
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

#[tauri::command]
pub async fn save_project_checked(
    app: AppHandle,
    state: WorkspaceState<'_>,
    save_as: bool,
) -> NativeResult<SaveProjectOutcome> {
    let (project, choose_path) = {
        let mut state = state.lock().map_err(message)?;
        state.pending_save = None;
        (state.project()?.clone(), save_as || state.file.is_none())
    };
    let picked = if choose_path {
        let Some(path) = choose_save_path(app, "website.desegnos".into()).await? else {
            return Ok(SaveProjectOutcome::Cancelled);
        };
        Some(path)
    } else {
        None
    };
    state
        .lock()
        .map_err(message)?
        .save_snapshot_checked(&project, picked, choose_path)
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
            state
                .lock()
                .map_err(message)?
                .save_snapshot_checked(&project, Some(path), true)
        }
    }
}

#[tauri::command]
pub fn prepare_requirements(state: WorkspaceState<'_>) -> NativeResult<RequirementsSnapshot> {
    let mut state = state.lock().map_err(message)?;
    let prepared = state
        .session
        .as_ref()
        .ok_or("Create or open a project first.")?
        .project()
        .prepare_requirements()
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
        workspace.open_path(&path).unwrap();
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
        assert!(workspace.open_path(&path).is_err());
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
        assert!(workspace.open_path(&path).is_err());
        assert_eq!(workspace.view().history, previous_history);
        assert_eq!(workspace.file.as_ref().unwrap().stamp().unwrap(), stamp);
        assert!(ProjectFile::acquire(&path).is_err());
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
            assert!(
                restarted.save_snapshot(&expected, None).is_err(),
                "{condition}"
            );
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
        assert!(workspace.recover(Some(original.id), false).is_err());
        assert_eq!(workspace.session.as_ref().unwrap().project().id, current.id);
        workspace.recover(Some(original.id), true).unwrap();
        assert_eq!(
            workspace.session.as_ref().unwrap().project().id,
            original.id
        );
        assert!(
            workspace
                .recoveries
                .iter()
                .any(|candidate| candidate.info.project_id == current.id)
        );
        let (retained, _) = workspace.read_recovery(current.id).unwrap().unwrap();
        assert!(retained.project.show_grid);
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
        workspace.open_document(file, document).unwrap();
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

#[tauri::command]
pub fn open_recent_project(
    window: tauri::WebviewWindow,
    state: WorkspaceState<'_>,
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
    path: PathBuf,
    discard: bool,
) -> NativeResult<WorkspaceView> {
    if window.label() != "editor" {
        return Err("Only the editor can open recent projects.".into());
    }
    generation
        .lock()
        .map_err(message)?
        .ensure_switch_allowed()?;
    let mut state = state.lock().map_err(message)?;
    state.check_replace(discard)?;
    if !crate::recent::read(&state.directory)?
        .iter()
        .any(|entry| entry.path == path)
    {
        return Err(
            "This project is not in your recent list. Use Open project to choose it.".into(),
        );
    }
    state.open_path(&path)?;
    Ok(state.view())
}

#[cfg(test)]
pub(super) fn open_archive_for_tests(directory: PathBuf, path: &std::path::Path) -> Workspace {
    let file = ProjectFile::acquire(path).unwrap();
    let document = file.load_document().unwrap();
    let mut workspace = Workspace::from_directory(directory).unwrap();
    workspace.open_document(file, document).unwrap();
    workspace
}
