//! Native export authority: frozen jobs, safe writing, previews and Open in browser.
//!
//! Visuals render directly from the element tree. Missing pointer behaviours use a frozen AI
//! request, sent only after an explicit paid-send decision. Every write goes
//! through the storage crate's staged, no-replace writer, and the project records the version,
//! folder and pointer fingerprints the export produced.
use crate::generation::{AiState, ExportChatgptState};
use crate::local_site::LocalSite;
use crate::workspace::Workspace;
use desegnos_ai::{
    AuthenticationMethod, CancellationToken, ProviderId, ProviderResponse, ProviderSettings,
    TokenUsage,
};
use desegnos_core::{
    ExportBundle, ExportPlan, ExportPlanKind, ExportRecord, GenerationMode, Project, WebsiteExport,
    export_manifest, pointer_fingerprints,
};
use desegnos_storage::{read_export_file, write_export_into, write_export_named};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

type NativeResult<T> = Result<T, String>;
type WorkspaceState<'a> = State<'a, Mutex<Workspace>>;
type ExportState<'a> = State<'a, Mutex<ExportService>>;
type SiteState<'a> = State<'a, Mutex<LocalSite>>;

/// The largest export file the clash check reads back.
const MAX_CHECK_BYTES: usize = 8 * 1024 * 1024;

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn trusted(window: &WebviewWindow) -> NativeResult<()> {
    if window.label() == "editor" {
        Ok(())
    } else {
        Err("Only the Desegnos editor can export.".into())
    }
}

fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

#[derive(ts_rs::TS, Debug, Clone, Copy, PartialEq, Eq)]
#[ts(rename = "ExportStatus", rename_all = "lowercase")]
enum JobStatus {
    Prepared,
    Sending,
    Ready,
    Failed,
    Cancelled,
    Stale,
}

impl JobStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Sending => "sending",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Stale => "stale",
        }
    }

    fn parse(label: &str) -> Self {
        match label {
            "ready" => Self::Ready,
            "sending" => Self::Sending,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "stale" => Self::Stale,
            _ => Self::Prepared,
        }
    }
}

#[derive(ts_rs::TS, Clone, Serialize)]
#[ts(rename = "ExportAttemptView")]
#[serde(rename_all = "camelCase")]
pub struct ExportAttempt {
    ordinal: usize,
    status: String,
    usage: Option<TokenUsage>,
}

#[derive(ts_rs::TS, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPageView {
    page_id: Uuid,
    name: String,
    path: String,
    output: desegnos_core::OutputType,
    file: String,
}

#[derive(ts_rs::TS, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResultView {
    label: String,
    path: String,
    file_count: usize,
    page_count: usize,
    preview_url: Option<String>,
}

#[derive(ts_rs::TS, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportJobView {
    job_id: Uuid,
    project_id: Uuid,
    base_revision: u64,
    #[ts(type = "'code' | 'comment'")]
    kind: &'static str,
    #[ts(as = "JobStatus")]
    status: &'static str,
    message: String,
    summary: String,
    estimate_usd: f64,
    usage: Option<TokenUsage>,
    attempts: Vec<ExportAttempt>,
    pages: Vec<ExportPageView>,
    size_count: usize,
    pointer_count: usize,
    reused_behaviour_count: usize,
    warnings: Vec<String>,
    result: Option<ExportResultView>,
    error: Option<String>,
    provider_error: Option<desegnos_ai::ProviderError>,
    provider: Option<ProviderId>,
}

struct ExportJob {
    view: ExportJobView,
    plan: ExportPlan,
    destination: Option<PathBuf>,
    preview: bool,
    preview_folder: Option<PathBuf>,
    settings: ProviderSettings,
    authentication: AuthenticationMethod,
    provider: ProviderId,
    account_id: Option<Uuid>,
    mode: GenerationMode,
    cancellation: CancellationToken,
}

/// Export jobs and the one preview folder a session keeps.
pub struct ExportService {
    directory: PathBuf,
    jobs: BTreeMap<Uuid, ExportJob>,
    accepted: BTreeMap<Uuid, ExportRecord>,
}

impl ExportService {
    pub fn initialize(data: &Path) -> Self {
        Self {
            directory: data.join("exports"),
            jobs: BTreeMap::new(),
            accepted: fs::read(data.join("exports/behaviours.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default(),
        }
    }

    fn with_accepted(&self, mut project: Project) -> Project {
        if let Some(accepted) = self.accepted.get(&project.id) {
            let fingerprints: BTreeMap<_, _> = project
                .pointers
                .iter()
                .map(|pointer| {
                    (
                        pointer.id,
                        desegnos_core::behaviour_fingerprint(&project, pointer),
                    )
                })
                .collect();
            let record = project.last_export.get_or_insert_with(|| accepted.clone());
            for (id, code) in &accepted.behaviours {
                let forgotten = !record.pointers.contains_key(id)
                    && record
                        .behaviours
                        .get(id)
                        .is_some_and(|old| old.accepted_revision >= code.accepted_revision);
                let matches = fingerprints.get(id) == Some(&code.fingerprint);
                if forgotten || !matches {
                    continue;
                }
                record.behaviours.insert(*id, code.clone());
                if let Some(pointer) = accepted.pointers.get(id) {
                    record.pointers.insert(*id, pointer.clone());
                }
            }
        }
        project
    }

    fn remember(&mut self, project: &Project, bundle: &ExportBundle) -> NativeResult<()> {
        let mut record = self
            .accepted
            .get(&project.id)
            .cloned()
            .or_else(|| project.last_export.clone())
            .unwrap_or_else(|| ExportRecord {
                id: Uuid::new_v4(),
                project_id: project.id,
                version: 0,
                destination: self.directory.to_string_lossy().into_owned(),
                exported_at: now(),
                pages: Vec::new(),
                pointers: BTreeMap::new(),
                behaviours: BTreeMap::new(),
            });
        let current: BTreeMap<_, _> = project
            .pointers
            .iter()
            .map(|pointer| {
                (
                    pointer.id,
                    desegnos_core::behaviour_fingerprint(project, pointer),
                )
            })
            .collect();
        record
            .behaviours
            .retain(|id, code| current.get(id) == Some(&code.fingerprint));
        record
            .pointers
            .retain(|id, _| record.behaviours.contains_key(id));
        record.behaviours.extend(bundle.behaviours.clone());
        record.pointers.extend(bundle.pointers.clone());
        self.accepted.insert(project.id, record);
        fs::create_dir_all(&self.directory).map_err(message)?;
        let bytes = serde_json::to_vec(&self.accepted).map_err(message)?;
        let pending = self.directory.join("behaviours.pending");
        fs::write(&pending, bytes).map_err(message)?;
        fs::rename(pending, self.directory.join("behaviours.json")).map_err(message)
    }

    fn previews(&self) -> PathBuf {
        self.directory.join("previews")
    }

    /// One preview at a time: every earlier temporary folder is removed.
    fn prune_previews(&self, keep: Option<&Path>) {
        let Ok(entries) = fs::read_dir(self.previews()) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if keep.is_some_and(|keep| keep == path) {
                continue;
            }
            let _ = fs::remove_dir_all(path);
        }
    }

    fn new_preview_folder(&self) -> NativeResult<PathBuf> {
        let folder = self.previews().join(Uuid::new_v4().to_string());
        fs::create_dir_all(&folder)
            .map_err(|error| format!("A temporary preview folder could not be created: {error}"))?;
        Ok(folder)
    }

    fn latest_for(&self, project_id: Uuid) -> Option<&ExportJob> {
        self.jobs
            .values()
            .filter(|job| job.view.project_id == project_id)
            .max_by_key(|job| job.view.base_revision)
    }
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickDestination {
    path: String,
}

/// Create only the default parent; the writer creates the project folder inside it.
fn create_default_destination(app: &AppHandle, path: &str) -> NativeResult<()> {
    let path = Path::new(path);
    let Ok(documents) = app.path().document_dir() else {
        return Ok(());
    };
    if path == documents.join("Desegnos") && !path.exists() {
        fs::create_dir_all(path)
            .map_err(|error| format!("The export folder could not be created: {error}"))?;
    }
    Ok(())
}

fn trusted_destination(path: String) -> NativeResult<PathBuf> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err("Choose an absolute destination folder.".into());
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "Choose an existing destination folder.".to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Choose a real folder rather than a symbolic link.".into());
    }
    path.canonicalize()
        .map_err(|error| format!("The destination folder could not be read: {error}"))
}

/// Choose where a full export writes. Renderer-supplied paths never reach the filesystem
/// without this native picker.
#[tauri::command]
pub async fn pick_export_destination(
    app: AppHandle,
    window: WebviewWindow,
) -> NativeResult<Option<PickDestination>> {
    trusted(&window)?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        crate::file_dialogs::picked(
            app.dialog()
                .file()
                .set_title("Choose where to export")
                .blocking_pick_folder(),
        )
    })
    .await
    .map_err(message)??;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(message)?;
    Ok(Some(PickDestination {
        path: path.to_string_lossy().into_owned(),
    }))
}

fn page_views(plan: &ExportPlan) -> Vec<ExportPageView> {
    plan.pages
        .iter()
        .map(|page| ExportPageView {
            page_id: page.page_id,
            name: page.name.clone(),
            path: page.path.clone(),
            output: page.output,
            file: page.file.clone(),
        })
        .collect()
}

/// Start an export: render direct outputs, or freeze one AI job that waits for its paid-send
/// decision. A preview export serves its temporary folder locally.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn prepare_export(
    app: AppHandle,
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    ai: AiState<'_>,
    chatgpt: State<'_, ExportChatgptState>,
    service: ExportState<'_>,
    site: SiteState<'_>,
    project_id: Uuid,
    revision: u64,
    destination: Option<String>,
    page_ids: Option<Vec<Uuid>>,
    preview: bool,
    build_behaviours: Option<bool>,
) -> NativeResult<ExportJobView> {
    trusted(&window)?;
    let project = {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?.clone();
        if project.id != project_id || project.revision != revision {
            return Err("The design changed. Export the current version again.".into());
        }
        project
    };
    let page_ids = page_ids.unwrap_or_default();
    let preview_page_id = if preview {
        if page_ids.len() != 1 {
            return Err("Preview export prints one boundary at a time.".into());
        }
        let page = project
            .pages
            .iter()
            .find(|page| page.id == page_ids[0])
            .ok_or("This boundary no longer exists.")?;
        if !page.output.needs_ai() {
            return Err("Preview shows pages.".into());
        }
        Some(page.id)
    } else {
        if !page_ids.is_empty() {
            return Err("Export writes every included boundary.".into());
        }
        None
    };
    // A preview writes every included boundary, so links from the chosen page keep working.
    // AI pages render directly here: Preview never starts paid generation.
    let export_page_ids = match preview_page_id {
        Some(chosen) => desegnos_core::preview_page_ids(&project, chosen),
        None => page_ids.clone(),
    };
    if !preview {
        // A full export records the version it came from, so the project needs a file first.
        workspace.lock().map_err(message)?.head_version()?;
    }
    let job_id = Uuid::new_v4();
    let project = service.lock().map_err(message)?.with_accepted(project);
    let plan = desegnos_core::plan_export(&project, &export_page_ids, job_id).map_err(message)?;
    let preview_page = preview_page_id
        .and_then(|id| plan.pages.iter().find(|page| page.page_id == id))
        .map(|page| page.file.clone());
    let (destination_path, preview_folder) = if preview {
        let folder = {
            let service = service.lock().map_err(message)?;
            service.prune_previews(None);
            service.new_preview_folder()?
        };
        site.lock().map_err(message)?.stop();
        (Some(folder.clone()), Some(folder))
    } else {
        let parent = destination.ok_or("Choose a destination folder before exporting.")?;
        create_default_destination(&app, &parent)?;
        (Some(trusted_destination(parent)?), None)
    };
    let has_ai = plan.request_count > 0 && (!preview || build_behaviours.unwrap_or(false));
    let reused_behaviour_count = desegnos_core::reused_export_behaviour_count(&project, &plan);
    let mut view = ExportJobView {
        job_id,
        project_id,
        base_revision: project.revision,
        kind: "code",
        status: JobStatus::Prepared.label(),
        message: if has_ai {
            "Ready to generate behaviour. Sending uses your provider plan or credits.".into()
        } else {
            "Ready to write.".into()
        },
        summary: String::new(),
        estimate_usd: 0.0,
        usage: None,
        attempts: Vec::new(),
        pages: page_views(&plan),
        size_count: plan.pages.iter().map(|page| page.size_ids.len()).sum(),
        pointer_count: plan.pointers.len() + reused_behaviour_count,
        reused_behaviour_count,
        warnings: plan.warnings.clone(),
        result: None,
        error: None,
        provider_error: None,
        provider: None,
    };
    if !has_ai {
        let bundle =
            desegnos_core::direct_export_bundle(&project, &export_page_ids).map_err(message)?;
        let written = commit(
            &workspace,
            &site,
            &project,
            &bundle,
            destination_path
                .as_deref()
                .ok_or("Choose a destination folder.")?,
            preview_folder.as_deref(),
            preview,
            preview_page.as_deref(),
        )?;
        service
            .lock()
            .map_err(message)?
            .remember(&project, &bundle)?;
        view.status = JobStatus::Ready.label();
        view.pointer_count = bundle.behaviours.len();
        view.reused_behaviour_count = bundle.behaviours.len();
        view.message = written.label.clone();
        view.summary = if bundle.behaviours.is_empty() {
            "Rendered directly from the design.".into()
        } else {
            String::new()
        };
        // Each unbuilt behaviour is named; Connect AI says how to build them.
        view.warnings.extend(bundle.warnings.iter().cloned());
        if plan.request_count > 0 {
            view.warnings.push("Connect AI to build behaviours.".into());
        }
        view.result = Some(written);
        let mut service = service.lock().map_err(message)?;
        service.jobs.insert(
            job_id,
            ExportJob {
                view: view.clone(),
                plan,
                destination: destination_path,
                preview,
                preview_folder,
                settings: ProviderSettings::default(),
                authentication: AuthenticationMethod::ApiKey,
                provider: ProviderId::Openai,
                account_id: None,
                mode: project.generation_mode,
                cancellation: CancellationToken::new(),
            },
        );
        return Ok(view);
    }
    let (authentication, provider, settings) = {
        let ai = ai.lock().map_err(message)?;
        ai.export_connection()
    };
    let account_id = if authentication == AuthenticationMethod::Chatgpt {
        let view = chatgpt.view().await;
        let account = view
            .accounts
            .iter()
            .find(|account| Some(account.id) == view.active_account_id)
            .ok_or("Sign in to ChatGPT before exporting.")?;
        if !account.connected || !account.plan_enabled {
            return Err(
                "Sign in to this ChatGPT account and grant plan usage before exporting.".into(),
            );
        }
        Some(account.id)
    } else {
        None
    };
    let snapshot = desegnos_core::plan_snapshot(&project, &plan);
    view.estimate_usd = {
        let ai = ai.lock().map_err(message)?;
        ai.export_estimate(&plan, &snapshot, project.generation_mode)
            .map_err(message)?
    };
    let mut service = service.lock().map_err(message)?;
    service.jobs.insert(
        job_id,
        ExportJob {
            view: view.clone(),
            plan,
            destination: destination_path,
            preview,
            preview_folder,
            settings,
            authentication,
            provider,
            account_id,
            mode: project.generation_mode,
            cancellation: CancellationToken::new(),
        },
    );
    Ok(view)
}

fn export_website(bundle: &ExportBundle) -> WebsiteExport {
    WebsiteExport {
        files: bundle.files.clone(),
        page_count: bundle.page_count(),
        warnings: bundle.warnings.clone(),
    }
}

/// Write a bundle, serve a preview, and record a full export in the project.
#[allow(clippy::too_many_arguments)]
fn commit(
    workspace: &WorkspaceState<'_>,
    site: &SiteState<'_>,
    project: &Project,
    bundle: &ExportBundle,
    destination: &Path,
    preview_folder: Option<&Path>,
    preview: bool,
    preview_page: Option<&str>,
) -> NativeResult<ExportResultView> {
    // A preview opens the chosen page's own file: a page away from the root is not index.html.
    let mut preview_file = String::new();
    let (path, file_count) = if preview {
        let folder = preview_folder.ok_or("A preview needs its temporary folder.")?;
        let mut export = export_website(bundle);
        export.files.remove(desegnos_core::EXPORT_MANIFEST_FILE);
        preview_file = preview_page
            .filter(|file| !file.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                export
                    .files
                    .keys()
                    .find(|file| file.ends_with(".html") && !file.contains('/'))
                    .cloned()
            })
            .unwrap_or_default();
        write_export_into(folder, &export).map_err(message)?;
        (folder.to_path_buf(), export.files.len())
    } else {
        let version = {
            let mut workspace = workspace.lock().map_err(message)?;
            workspace.head_version()?
        };
        let mut record = ExportRecord {
            id: Uuid::new_v4(),
            project_id: project.id,
            version,
            destination: String::new(),
            exported_at: now(),
            pages: bundle.pages.clone(),
            pointers: bundle.pointers.clone(),
            behaviours: bundle.behaviours.clone(),
        };
        let written = write_export_named(destination, &project.name, |path| {
            record.destination = path.to_string_lossy().into_owned();
            let mut export = export_website(bundle);
            export.files.insert(
                desegnos_core::EXPORT_MANIFEST_FILE.into(),
                export_manifest(&record),
            );
            Ok(export)
        })
        .map_err(message)?;
        {
            let mut workspace = workspace.lock().map_err(message)?;
            workspace.record_export(record)?;
        }
        (written.path, written.file_count)
    };
    let preview_url = if preview {
        Some(
            site.lock()
                .map_err(message)?
                .serve(&path, 64 * 1024 * 1024)
                .map_err(message)?
                + &preview_file,
        )
    } else {
        None
    };
    let label = if preview {
        "Preview is ready.".to_string()
    } else {
        format!(
            "Exported {} {}.",
            bundle.page_count(),
            if bundle.page_count() == 1 {
                "boundary"
            } else {
                "boundaries"
            }
        )
    };
    Ok(ExportResultView {
        label,
        path: path.to_string_lossy().into_owned(),
        file_count,
        page_count: bundle.page_count(),
        preview_url,
    })
}

/// Send a prepared export. This is the explicit paid-send decision.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_export(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    ai: AiState<'_>,
    chatgpt: State<'_, ExportChatgptState>,
    service: ExportState<'_>,
    site: SiteState<'_>,
    job_id: Uuid,
) -> NativeResult<ExportJobView> {
    trusted(&window)?;
    let (plan, authentication, provider, account_id, settings, mode, cancellation) = {
        let mut service = service.lock().map_err(message)?;
        let job = service
            .jobs
            .get_mut(&job_id)
            .ok_or("This export is no longer available. Prepare it again.")?;
        if !matches!(
            JobStatus::parse(job.view.status),
            JobStatus::Prepared | JobStatus::Failed | JobStatus::Cancelled
        ) {
            return Err("Only a prepared or failed export can be sent.".into());
        }
        job.view.status = JobStatus::Sending.label();
        job.view.error = None;
        job.view.provider_error = None;
        job.view.provider = Some(job.provider);
        job.view.message = "Exporting…".into();
        job.cancellation = CancellationToken::new();
        (
            job.plan.clone(),
            job.authentication,
            job.provider,
            job.account_id,
            job.settings.clone(),
            job.mode,
            job.cancellation.clone(),
        )
    };
    let mut provider_error = None;
    let result = async {
        let client = {
            let ai = ai.lock().map_err(message)?;
            ai.export_client(provider, &settings).map_err(message)?
        };
        let mut account_model = None;
        let key = if authentication == AuthenticationMethod::Chatgpt {
            let Some(account_id) = account_id else {
                return Err("This ChatGPT export has no bound account. Prepare it again.".into());
            };
            let mut view = chatgpt.view().await;
            let account = view
                .accounts
                .iter()
                .find(|account| account.id == account_id)
                .ok_or("This ChatGPT account is no longer connected.")?;
            if !account.connected || !account.plan_enabled {
                return Err(
                    "Sign in to this ChatGPT account and grant plan usage before exporting.".into(),
                );
            }
            let manager = chatgpt.manager()?;
            if view.models.is_empty() {
                let client = ai.lock().map_err(message)?.client()?;
                view = manager
                    .discover_models(account_id, &client, &cancellation)
                    .await
                    .map_err(message)?;
            }
            if view.active_account_id != Some(account_id) {
                return Err(
                    "The selected ChatGPT account changed. Prepare the export again.".into(),
                );
            }
            if let Some(error) = view.models_provider_error {
                provider_error = Some(error.clone());
                return Err(message(error));
            }
            account_model = Some(view.selected_model(&settings).map_err(message)?.clone());
            chatgpt
                .manager()?
                .bearer(account_id, &cancellation)
                .await
                .map_err(message)?
        } else {
            let mut ai = ai.lock().map_err(message)?;
            ai.export_key(provider, &settings).map_err(message)?
        };
        let snapshot = {
            let workspace = workspace.lock().map_err(message)?;
            let project = workspace.project()?;
            if project.id != plan.project_id || project.revision != plan.base_revision {
                return Err("The design changed. Prepare the export again.".into());
            }
            desegnos_core::plan_snapshot(project, &plan)
        };
        if authentication == AuthenticationMethod::ApiKey {
            let model =
                crate::generation::api_model_for_key(&ai, provider, &settings, &key, &cancellation)
                    .await
                    .map_err(|error| {
                        provider_error = Some(error.clone());
                        message(error)
                    })?;
            let estimate = client
                .estimate(authentication, &snapshot, mode, &plan.schema, &settings)
                .map_err(message)?;
            model
                .validate_api_input(&settings, estimate.input_token_upper_bound)
                .map_err(message)?;
        }
        if let Some(model) = &account_model {
            let estimate = client
                .estimate(authentication, &snapshot, mode, &plan.schema, &settings)
                .map_err(message)?;
            model
                .validate_input(estimate.input_token_upper_bound)
                .map_err(message)?;
        }
        client
            .generate(
                desegnos_ai::GenerationRequest {
                    authentication,
                    snapshot: &snapshot,
                    mode,
                    schema: &plan.schema,
                    settings: &settings,
                    key: &key,
                    cancellation: &cancellation,
                },
                |_progress| {},
            )
            .await
            .map_err(|error| {
                provider_error = Some(error.clone());
                message(error)
            })
    }
    .await;
    if let Some(error) = provider_error
        && let Some(job) = service.lock().map_err(message)?.jobs.get_mut(&job_id)
    {
        job.view.provider_error = Some(error);
    }
    finish(&workspace, &service, &site, job_id, plan, result)
}

fn finish(
    workspace: &WorkspaceState<'_>,
    service: &ExportState<'_>,
    site: &SiteState<'_>,
    job_id: Uuid,
    plan: ExportPlan,
    result: Result<ProviderResponse, String>,
) -> NativeResult<ExportJobView> {
    let (destination, preview, preview_folder, cancelled) = {
        let guard = service.lock().map_err(message)?;
        let job = guard
            .jobs
            .get(&job_id)
            .ok_or("This export is no longer available.")?;
        (
            job.destination.clone(),
            job.preview,
            job.preview_folder.clone(),
            job.cancellation.is_cancelled(),
        )
    };
    if cancelled {
        // A cancel arrived before the reply did: never write files after the user stopped.
        let mut guard = service.lock().map_err(message)?;
        let Some(job) = guard.jobs.get_mut(&job_id) else {
            return Err("This export is no longer available.".into());
        };
        job.view.status = JobStatus::Cancelled.label();
        job.view.message = "Export cancelled. Work already sent may still be charged.".into();
        return Ok(job.view.clone());
    }
    // Take the project without holding the export lock, so cancel and status reads stay live.
    let project = {
        let workspace = workspace.lock().map_err(message)?;
        workspace.project()?.clone()
    };
    let project = service.lock().map_err(message)?.with_accepted(project);
    let _outcome: Result<(), String> = match result {
        Ok(response) => {
            let stale = project.id != plan.project_id || project.revision != plan.base_revision;
            let failure = if stale {
                Some(if project.id != plan.project_id {
                    "Return to the project that owns this export.".into()
                } else {
                    "The design changed while exporting. Export the current version again.".into()
                })
            } else {
                match &plan.kind {
                    ExportPlanKind::Comment { pointer_id } => {
                        apply_comment(workspace, &project, &plan, &response.text, *pointer_id).err()
                    }
                    ExportPlanKind::Code => {
                        let parsed = desegnos_core::parse_export_reply(&plan, &response.text)
                            .and_then(|reply| {
                                desegnos_core::ai_export_bundle(&project, &plan, &reply)
                                    .map(|bundle| (reply, bundle))
                            });
                        match parsed {
                            Ok((reply, bundle)) => {
                                let Some(destination) = destination.as_deref() else {
                                    return record_failure(
                                        service,
                                        job_id,
                                        response,
                                        "This export has no destination folder.".into(),
                                    );
                                };
                                match commit(
                                    workspace,
                                    site,
                                    &project,
                                    &bundle,
                                    destination,
                                    preview_folder.as_deref(),
                                    preview,
                                    None,
                                ) {
                                    Ok(written) => {
                                        service
                                            .lock()
                                            .map_err(message)?
                                            .remember(&project, &bundle)?;
                                        let mut guard = service.lock().map_err(message)?;
                                        let Some(job) = guard.jobs.get_mut(&job_id) else {
                                            return Err(
                                                "This export is no longer available.".into()
                                            );
                                        };
                                        job.view.usage = response.usage;
                                        job.view.attempts.push(ExportAttempt {
                                            ordinal: job.view.attempts.len() + 1,
                                            status: "received".into(),
                                            usage: response.usage,
                                        });
                                        job.view.pointer_count = bundle.behaviours.len();
                                        job.view.summary = reply.summary.clone();
                                        job.view.warnings = bundle.warnings.clone();
                                        job.view.message = written.label.clone();
                                        job.view.result = Some(written);
                                        job.view.status = JobStatus::Ready.label();
                                        return Ok(job.view.clone());
                                    }
                                    Err(error) => Some(error),
                                }
                            }
                            Err(error) => Some(error.to_string()),
                        }
                    }
                }
            };
            let mut guard = service.lock().map_err(message)?;
            let Some(job) = guard.jobs.get_mut(&job_id) else {
                return Err("This export is no longer available.".into());
            };
            job.view.usage = response.usage;
            job.view.attempts.push(ExportAttempt {
                ordinal: job.view.attempts.len() + 1,
                status: "received".into(),
                usage: response.usage,
            });
            match failure {
                Some(error) => {
                    if stale {
                        job.view.status = JobStatus::Stale.label();
                    } else {
                        job.view.status = JobStatus::Failed.label();
                        job.view.error = Some(error.clone());
                    }
                    job.view.message = error;
                }
                None => {
                    job.view.status = JobStatus::Ready.label();
                }
            }
            Ok(())
        }
        Err(error) => {
            let mut guard = service.lock().map_err(message)?;
            let Some(job) = guard.jobs.get_mut(&job_id) else {
                return Err("This export is no longer available.".into());
            };
            let cancelled = job.cancellation.is_cancelled();
            job.view.attempts.push(ExportAttempt {
                ordinal: job.view.attempts.len() + 1,
                status: if cancelled {
                    "cancelled".into()
                } else {
                    "failed".into()
                },
                usage: None,
            });
            if cancelled {
                job.view.status = JobStatus::Cancelled.label();
                job.view.message =
                    "Export cancelled. Work already sent may still be charged.".into();
            } else {
                job.view.status = JobStatus::Failed.label();
                job.view.message = error.clone();
                job.view.error = Some(error);
            }
            Ok(())
        }
    };
    let guard = service.lock().map_err(message)?;
    guard
        .jobs
        .get(&job_id)
        .map(|job| job.view.clone())
        .ok_or_else(|| "This export is no longer available.".to_string())
}

fn record_failure(
    service: &ExportState<'_>,
    job_id: Uuid,
    response: ProviderResponse,
    error: String,
) -> NativeResult<ExportJobView> {
    let mut guard = service.lock().map_err(message)?;
    let Some(job) = guard.jobs.get_mut(&job_id) else {
        return Err("This export is no longer available.".into());
    };
    job.view.usage = response.usage;
    job.view.attempts.push(ExportAttempt {
        ordinal: job.view.attempts.len() + 1,
        status: "received".into(),
        usage: response.usage,
    });
    job.view.status = JobStatus::Failed.label();
    job.view.message = error.clone();
    job.view.error = Some(error);
    Ok(job.view.clone())
}

fn apply_comment(
    workspace: &WorkspaceState<'_>,
    project: &Project,
    plan: &ExportPlan,
    text: &str,
    pointer_id: Uuid,
) -> Result<String, String> {
    let comment = desegnos_core::parse_comment_reply(plan, text).map_err(message)?;
    let record = project
        .last_export
        .as_ref()
        .ok_or("Export once before rewriting a pointer comment.")?;
    let path = PathBuf::from(&record.destination);
    let files = read_export_files(&path).map_err(message)?;
    let fingerprint = pointer_fingerprints(&files)
        .get(&pointer_id)
        .cloned()
        .or_else(|| {
            record
                .pointers
                .get(&pointer_id)
                .map(|pointer| pointer.fingerprint.clone())
        })
        .ok_or("This pointer's code is no longer in the export folder.")?;
    let mut workspace = workspace.lock().map_err(message)?;
    workspace
        .rewrite_pointer_comment(pointer_id, &comment, &fingerprint)
        .map_err(message)?;
    Ok(comment)
}

/// Cancel a running export. A send already submitted may still be charged.
#[tauri::command]
pub fn cancel_export(
    window: WebviewWindow,
    service: ExportState<'_>,
    job_id: Uuid,
) -> NativeResult<ExportJobView> {
    trusted(&window)?;
    let mut service = service.lock().map_err(message)?;
    let job = service
        .jobs
        .get_mut(&job_id)
        .ok_or("This export is no longer available.")?;
    job.cancellation.cancel();
    if JobStatus::parse(job.view.status) == JobStatus::Sending {
        job.view.status = JobStatus::Cancelled.label();
        job.view.message = "Export cancelled. Work already sent may still be charged.".into();
    }
    Ok(job.view.clone())
}

/// The newest export job for a project, so a reload returns to its review.
#[tauri::command]
pub fn get_export_job(
    window: WebviewWindow,
    service: ExportState<'_>,
    project_id: Uuid,
) -> NativeResult<Option<ExportJobView>> {
    trusted(&window)?;
    let service = service.lock().map_err(message)?;
    Ok(service.latest_for(project_id).map(|job| job.view.clone()))
}

/// Recompute pointer fingerprints from the last export's folder, so code changed outside a
/// pointer shows as a clash. A missing or replaced folder simply reports no new fingerprints.
#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCheck {
    states: Vec<desegnos_core::PointerStateView>,
    last_export: Option<ExportRecord>,
    readable: bool,
}

#[tauri::command]
pub fn recheck_export(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    project_id: Uuid,
) -> NativeResult<ExportCheck> {
    trusted(&window)?;
    let project = {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?.clone();
        if project.id != project_id {
            return Err("The active project changed.".into());
        }
        project
    };
    let Some(record) = project.last_export.clone() else {
        return Ok(ExportCheck {
            states: project.pointer_states(),
            last_export: None,
            readable: false,
        });
    };
    let path = PathBuf::from(&record.destination);
    match read_export_files(&path) {
        Ok(files) => Ok(ExportCheck {
            states: project.pointer_states_with_fingerprints(&pointer_fingerprints(&files)),
            last_export: Some(record),
            readable: true,
        }),
        Err(_) => Ok(ExportCheck {
            states: project.pointer_states(),
            last_export: Some(record),
            readable: false,
        }),
    }
}

/// Every text file of an export folder, bounded, for fingerprint recomputation.
fn read_export_files(root: &Path) -> desegnos_core::Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut candidates = Vec::new();
    collect_text_files(root, "", &mut candidates, 0)?;
    for candidate in candidates {
        if let Ok(bytes) = read_export_file(root, &candidate)
            && bytes.len() <= MAX_CHECK_BYTES
        {
            files.insert(candidate, bytes);
        }
    }
    Ok(files)
}

fn collect_text_files(
    root: &Path,
    prefix: &str,
    out: &mut Vec<String>,
    depth: usize,
) -> desegnos_core::Result<()> {
    if depth > 2 {
        return Ok(());
    }
    let directory = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix)
    };
    let entries = fs::read_dir(&directory).map_err(|error| {
        desegnos_core::Error(format!("The export folder could not be read: {error}"))
    })?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.contains(['/', '\\', '\0']) {
            continue;
        }
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
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

/// Rewrite one pointer's comment from the marked code an outside edit left behind. One undo step
/// when the reply arrives; sending is the usual paid-send decision.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn keep_pointer_code(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    ai: AiState<'_>,
    service: ExportState<'_>,
    project_id: Uuid,
    pointer_id: Uuid,
) -> NativeResult<ExportJobView> {
    trusted(&window)?;
    let project = {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?.clone();
        if project.id != project_id {
            return Err("The active project changed.".into());
        }
        project
    };
    let record = project
        .last_export
        .clone()
        .ok_or("Export once before rewriting a pointer comment.")?;
    let files = read_export_files(&PathBuf::from(&record.destination)).map_err(message)?;
    let marked = marked_code_for(&files, pointer_id);
    if marked.trim().is_empty() {
        return Err(
            "That pointer's code is no longer in the export folder. Keep the comment instead."
                .into(),
        );
    }
    let job_id = Uuid::new_v4();
    let plan =
        desegnos_core::plan_comment(&project, pointer_id, &marked, job_id).map_err(message)?;
    let (authentication, provider, settings) = {
        let ai = ai.lock().map_err(message)?;
        ai.export_connection()
    };
    let snapshot = desegnos_core::plan_snapshot(&project, &plan);
    let estimate_usd = {
        let ai = ai.lock().map_err(message)?;
        ai.export_estimate(&plan, &snapshot, project.generation_mode)
            .map_err(message)?
    };
    let view = ExportJobView {
        job_id,
        project_id,
        base_revision: project.revision,
        kind: "comment",
        status: JobStatus::Prepared.label(),
        message: "Ready to rewrite the comment. Sending uses your provider plan or credits.".into(),
        summary: String::new(),
        estimate_usd,
        usage: None,
        attempts: Vec::new(),
        pages: Vec::new(),
        size_count: 0,
        pointer_count: 1,
        reused_behaviour_count: 0,
        warnings: Vec::new(),
        result: None,
        error: None,
        provider_error: None,
        provider: None,
    };
    let mut service = service.lock().map_err(message)?;
    service.jobs.insert(
        job_id,
        ExportJob {
            view: view.clone(),
            plan,
            destination: None,
            preview: false,
            preview_folder: None,
            settings,
            authentication,
            provider,
            account_id: None,
            mode: project.generation_mode,
            cancellation: CancellationToken::new(),
        },
    );
    Ok(view)
}

fn marked_code_for(files: &BTreeMap<String, Vec<u8>>, pointer_id: Uuid) -> String {
    let needle = pointer_id.to_string();
    let mut code = String::new();
    for (path, bytes) in files {
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        let mut search = 0;
        while let Some(found) = text[search..].find(&needle) {
            let at = search + found;
            // A marked block wins; an HTML element carrying the attribute starts at its tag.
            let start = text[..at]
                .rfind("/* desegnos-pointer:")
                .or_else(|| text[..at].rfind('<'))
                .unwrap_or(at);
            let close = format!("/* /desegnos-pointer:{needle} */");
            let end = match text[at..].find(&close) {
                Some(offset) => at + offset + close.len(),
                None => text[at..]
                    .find('>')
                    .map(|offset| at + offset + 1)
                    .unwrap_or(text.len().min(at + 4000)),
            };
            code.push_str(&format!("/* {path} */\n{}\n\n", &text[start..end]));
            search = end.max(at + needle.len());
        }
    }
    code
}

/// Close a preview: stop its loopback site and remove the temporary folder.
#[tauri::command]
pub fn close_preview(
    window: WebviewWindow,
    service: ExportState<'_>,
    site: SiteState<'_>,
) -> NativeResult<()> {
    trusted(&window)?;
    site.lock().map_err(message)?.stop();
    service.lock().map_err(message)?.prune_previews(None);
    Ok(())
}

/// Open the last export's folder after checking the renderer's path against the record.
#[tauri::command]
pub fn open_export_folder(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    project_id: Uuid,
    path: String,
) -> NativeResult<()> {
    trusted(&window)?;
    let record = {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?;
        if project.id != project_id {
            return Err("The active project changed.".into());
        }
        project
            .last_export
            .clone()
            .ok_or("Export before opening the result folder.")?
    };
    if record.destination != path {
        return Err("That folder is not this project's last export.".into());
    }
    let folder = PathBuf::from(&path);
    if !folder.is_dir() {
        return Err("The last export folder is no longer there. Export again.".into());
    }
    crate::system_open::open(&folder).map_err(|_| {
        "Your export is safe, but its folder could not open. Open it in your file manager.".into()
    })
}

/// Serve the last export in the system browser.
#[tauri::command]
pub fn open_in_browser(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    site: SiteState<'_>,
    project_id: Uuid,
) -> NativeResult<String> {
    trusted(&window)?;
    let project = {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?;
        if project.id != project_id {
            return Err("The active project changed.".into());
        }
        project.clone()
    };
    let record = project
        .last_export
        .clone()
        .ok_or("Export once before opening the result in a browser.")?;
    let path = PathBuf::from(&record.destination);
    if !path.is_dir() {
        return Err("The last export folder is no longer there. Export again.".into());
    }
    let url = site
        .lock()
        .map_err(message)?
        .serve(&path, 64 * 1024 * 1024)
        .map_err(message)?;
    let target = entry_url(&path, &url);
    crate::system_open::open(&target)
        .map_err(|_| format!("The site is running at {url}, but no browser could open it."))?;
    Ok(target)
}

/// The page a browser should open: the home page, else a single-page export's own file.
fn entry_url(folder: &Path, base: &str) -> String {
    if folder.join("index.html").is_file() {
        return base.to_owned();
    }
    if folder.join("slides.pdf").is_file() {
        return format!("{base}slides.pdf");
    }
    if let Ok(entries) = fs::read_dir(folder) {
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                (name.ends_with(".html") || name.ends_with(".svg") || name.ends_with(".png"))
                    .then_some(name)
            })
            .collect();
        names.sort();
        if let Some(name) = names.into_iter().next() {
            return format!("{base}{name}");
        }
    }
    base.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use desegnos_core::{
        Appearance, ExportBehaviour, ExportedPointer, Pointer, PointerKind, PointerSource,
        PointerTarget, Viewport, behaviour_fingerprint, plan_export,
    };

    #[test]
    fn preview_acceptance_reuses_code_but_respects_keep_comment() {
        let mut project = Project::new(
            "Cache".into(),
            Viewport {
                width: 800,
                height: 600,
            },
            Appearance::Light,
        )
        .unwrap();
        let page_id = project.home_page_id;
        let pointer = Pointer {
            id: Uuid::new_v4(),
            kind: PointerKind::Change,
            from: PointerSource::Boundary {
                page_id,
                size_id: None,
            },
            to: PointerTarget::Boundary {
                page_id,
                size_id: None,
            },
            comment: "Toggle this page".into(),
            wiring: None,
            play: None,
            play_comment: String::new(),
        };
        let pointer_id = pointer.id;
        project.pointers.push(pointer.clone());
        let record = ExportRecord {
            id: Uuid::new_v4(),
            project_id: project.id,
            version: 1,
            destination: "/tmp/export".into(),
            exported_at: "now".into(),
            pages: vec![],
            pointers: BTreeMap::from([(
                pointer_id,
                ExportedPointer {
                    fingerprint: "a".repeat(64),
                    comment: pointer.comment.clone(),
                },
            )]),
            behaviours: BTreeMap::from([(
                pointer_id,
                ExportBehaviour {
                    accepted_revision: project.revision,
                    fingerprint: behaviour_fingerprint(&project, &pointer),
                    js: "console.log('accepted');".into(),
                },
            )]),
        };
        let mut service = ExportService {
            directory: PathBuf::new(),
            jobs: BTreeMap::new(),
            accepted: BTreeMap::from([(project.id, record.clone())]),
        };
        let prepared = service.with_accepted(project.clone());
        assert_eq!(
            plan_export(&prepared, &[], Uuid::new_v4())
                .unwrap()
                .request_count,
            0
        );
        project.last_export = Some(record);
        project
            .last_export
            .as_mut()
            .unwrap()
            .pointers
            .remove(&pointer_id);
        project.revision += 1;
        let forgotten = service.with_accepted(project.clone());
        assert_eq!(
            plan_export(&forgotten, &[], Uuid::new_v4())
                .unwrap()
                .request_count,
            1
        );
        // Accepting the replacement at the new revision supersedes the forgotten code.
        service
            .accepted
            .get_mut(&project.id)
            .unwrap()
            .behaviours
            .get_mut(&pointer_id)
            .unwrap()
            .accepted_revision = project.revision;
        let accepted = service.with_accepted(project.clone());
        assert_eq!(
            plan_export(&accepted, &[], Uuid::new_v4())
                .unwrap()
                .request_count,
            0
        );
        project.revision += 1;
        let later_edit = service.with_accepted(project);
        assert_eq!(
            plan_export(&later_edit, &[], Uuid::new_v4())
                .unwrap()
                .request_count,
            0
        );
    }
}
