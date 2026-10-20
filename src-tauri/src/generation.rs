use crate::{
    chatgpt::ChatgptState,
    preview::{PreviewDescriptor, PreviewRegistry},
    workspace::{Workspace, WorkspaceView},
};
use desegni_ai::{
    ApiKey, AuthenticationMethod, CancellationToken, ChatgptView, CredentialInfo,
    CredentialManager, CredentialStorage, ErrorKind, ModelInfo, OpenAiClient, ProviderClient,
    ProviderDescriptor, ProviderError, ProviderId, ProviderResponse, ProviderSettings,
    SubmissionOutcome, TokenUsage, model_catalogue, provider_catalogue,
};
use desegni_core::{GenerationCandidate, GenerationMode, Project, RequirementsSnapshot, digest};
use desegni_storage::{ProjectFile, write_markdown};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

type NativeResult<T> = Result<T, String>;
type AiState<'a> = State<'a, Mutex<GenerationService>>;
type WorkspaceState<'a> = State<'a, Mutex<Workspace>>;
const MAX_JOB_BYTES: u64 = 24 * 1024 * 1024;
const MAX_CACHED_JOBS: usize = 16;
const MAX_ATTEMPTS: usize = 20;

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(all(debug_assertions, feature = "native-tests"))]
fn capture_benchmark_output(
    directory: &Path,
    ordinal: usize,
    text: &str,
    enabled: bool,
) -> NativeResult<()> {
    if enabled {
        private_directory(directory)?;
        write_markdown(
            &directory.join(format!("provider-output-{ordinal}.txt")),
            text,
        )
        .map_err(message)?;
    }
    Ok(())
}

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("UTC timestamps fit RFC3339")
}
fn trusted(window: &WebviewWindow) -> NativeResult<()> {
    if window.label() == "editor" {
        Ok(())
    } else {
        Err("Only the Desegnos editor can perform this operation.".into())
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Prepared,
    Generating,
    Ready,
    Blocked,
    Failed,
    Unknown,
    Cancelled,
    Discarded,
    Applying,
    Applied,
}

impl JobStatus {
    fn active(self) -> bool {
        matches!(
            self,
            Self::Prepared | Self::Generating | Self::Ready | Self::Blocked | Self::Applying
        )
    }
    fn retryable(self) -> bool {
        matches!(
            self,
            Self::Failed | Self::Unknown | Self::Cancelled | Self::Blocked
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationAttempt {
    ordinal: usize,
    status: JobStatus,
    started_at: String,
    finished_at: Option<String>,
    response_id: Option<String>,
    usage: Option<TokenUsage>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GenerationArea {
    page_id: Uuid,
    region_id: Uuid,
    rect: desegni_core::Rect,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationJobView {
    job_id: Uuid,
    project_id: Uuid,
    base_revision: u64,
    requirements: RequirementsSnapshot,
    #[serde(default)]
    areas: Vec<GenerationArea>,
    generation_mode: GenerationMode,
    #[serde(default)]
    provider: ProviderId,
    settings: ProviderSettings,
    #[serde(default)]
    authentication: AuthenticationMethod,
    #[serde(default)]
    chatgpt_account_id: Option<Uuid>,
    #[serde(default)]
    failure_kind: Option<ErrorKind>,
    #[serde(default)]
    provider_error: Option<ProviderError>,
    status: JobStatus,
    estimate_usd: f64,
    received_bytes: usize,
    message: String,
    candidate: Option<GenerationCandidate>,
    usage: Option<TokenUsage>,
    attempts: Vec<GenerationAttempt>,
    accepted_decision_indices: Vec<usize>,
    #[serde(default)]
    recommended_decision_indices: Vec<usize>,
    #[serde(default)]
    dossier_error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JobRecord {
    version: u32,
    view: GenerationJobView,
    error: Option<ProviderError>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LatestJob {
    job_id: Uuid,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderPreference {
    version: u32,
    provider: ProviderId,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthenticationPreference {
    version: u32,
    authentication: AuthenticationMethod,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsView {
    provider: ProviderId,
    providers: Vec<ProviderDescriptor>,
    settings: ProviderSettings,
    credential: CredentialInfo,
    catalogue: Vec<ModelInfo>,
    configuration_error: Option<String>,
    authentication: AuthenticationMethod,
    chatgpt: ChatgptView,
}

impl AiSettingsView {
    pub(super) fn with_warning(mut self, warning: Option<String>) -> Self {
        if let Some(warning) = warning {
            self.configuration_error = Some(match self.configuration_error.take() {
                Some(existing) => format!("{existing} {warning}"),
                None => warning,
            });
        }
        self
    }
}

pub struct GenerationService {
    provider: ProviderId,
    provider_credentials: BTreeMap<ProviderId, Arc<CredentialManager>>,
    directory: PathBuf,
    settings: ProviderSettings,
    configuration_error: Option<String>,
    authentication: AuthenticationMethod,
    authentication_error: Option<String>,
    credentials: Arc<CredentialManager>,
    client: Result<OpenAiClient, String>,
    jobs: BTreeMap<Uuid, JobRecord>,
    unsaved_jobs: BTreeSet<Uuid>,
    active: BTreeMap<Uuid, (usize, CancellationToken)>,
}

fn private_directory(path: &Path) -> NativeResult<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Desegnos's generation directory is not a regular directory. The original data was preserved.".into());
        }
        return Ok(());
    }
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(message)
}

fn regular_file(path: &Path) -> NativeResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(message)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(
            "A generation record is not a regular file. The original was preserved.".into(),
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err("A generation record has unexpected filesystem aliases. The original was preserved.".into());
        }
    }
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> NativeResult<T> {
    regular_file(path)?;
    let file = File::open(path).map_err(message)?;
    if file.metadata().map_err(message)?.len() > limit {
        return Err(
            "A generation record exceeds its size limit. The original was preserved.".into(),
        );
    }
    let mut bytes = vec![];
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(message)?;
    if bytes.len() as u64 > limit {
        return Err(
            "A generation record exceeds its size limit. The original was preserved.".into(),
        );
    }
    serde_json::from_slice(&bytes).map_err(|_| "A generation record is damaged or uses an unsupported format. The original was preserved.".into())
}

fn write_json(path: &Path, value: &impl Serialize) -> NativeResult<()> {
    if path.exists() {
        regular_file(path)?;
    }
    let bytes = serde_json::to_string(value).map_err(message)?;
    if bytes.len() as u64 > MAX_JOB_BYTES {
        return Err("The complete generation record exceeds its local storage limit.".into());
    }
    write_markdown(path, &bytes).map_err(message)
}

impl GenerationService {
    pub fn initialize(app: &AppHandle) -> NativeResult<Self> {
        Self::from_directory(
            app.path()
                .app_local_data_dir()
                .map_err(message)?
                .join("generation"),
        )
    }

    fn from_directory(directory: PathBuf) -> NativeResult<Self> {
        let directory_error = private_directory(&directory).err();
        let path = directory.join("settings.json");
        let loaded = if let Some(error) = directory_error {
            Err(error)
        } else if path.exists() {
            read_json::<ProviderSettings>(&path, 65_536).and_then(|settings| {
                settings
                    .validate_for_auth(AuthenticationMethod::Chatgpt)
                    .map_err(message)?;
                Ok(settings)
            })
        } else {
            Ok(ProviderSettings::default())
        };
        let (mut settings, mut configuration_error) = match loaded {
            Ok(settings) => (settings, None),
            Err(error) => (
                ProviderSettings::default(),
                Some(format!(
                    "Saved AI settings could not be loaded: {error} Review and explicitly save settings before generating. The original file remains on disk."
                )),
            ),
        };
        let preference_path = directory.join("authentication.json");
        let preference = if preference_path.exists() {
            read_json::<AuthenticationPreference>(&preference_path, 4096).and_then(|value| {
                if value.version == 1 {
                    Ok(value.authentication)
                } else {
                    Err("The authentication preference has an unsupported format.".into())
                }
            })
        } else {
            Ok(AuthenticationMethod::ApiKey)
        };
        let (authentication, authentication_error) = match preference {
            Ok(authentication) => (authentication, None),
            Err(error) => (
                AuthenticationMethod::ApiKey,
                Some(format!(
                    "Saved authentication settings could not be loaded: {error} Explicitly choose a generation method before sending. The original file remains on disk."
                )),
            ),
        };
        if configuration_error.is_none()
            && let Err(error) = settings.validate_for_auth(authentication)
        {
            configuration_error = Some(format!(
                "Saved AI settings could not be loaded: {error} Review and explicitly save settings before generating. The original file remains on disk."
            ));
            settings = ProviderSettings::default();
        }
        let credentials = Arc::new(CredentialManager::new());
        let client = OpenAiClient::new().map_err(message);
        #[cfg(feature = "native-tests")]
        let (credentials, client) =
            if let Ok(endpoint) = std::env::var("DESEGNI_NATIVE_TEST_OPENAI_ENDPOINT") {
                (
                    Arc::new(CredentialManager::isolated_for_tests()),
                    OpenAiClient::for_loopback(&endpoint).map_err(message),
                )
            } else if cfg!(debug_assertions)
                && std::env::var("DESEGNI_GENERATION_BENCHMARK").as_deref() == Ok("1")
            {
                (Arc::new(CredentialManager::isolated_for_tests()), client)
            } else {
                (credentials, client)
            };
        let mut service = Self {
            provider: ProviderId::Openai,
            provider_credentials: BTreeMap::new(),
            directory,
            settings,
            configuration_error,
            authentication,
            authentication_error,
            credentials,
            client,
            jobs: BTreeMap::new(),
            unsaved_jobs: BTreeSet::new(),
            active: BTreeMap::new(),
        };
        let preference = service.directory.join("provider.json");
        if preference.exists() {
            match read_json::<ProviderPreference>(&preference, 4096).and_then(|p| {
                if p.version != 1 {
                    return Err("The saved AI choice has an unsupported format.".into());
                }
                service.select_provider(p.provider, false)
            }) {
                Ok(()) => (),
                Err(error) => service.configuration_error = Some(error),
            }
        }
        Ok(service)
    }

    fn settings_path(&self, provider: ProviderId) -> PathBuf {
        if provider == ProviderId::Openai {
            self.directory.join("settings.json")
        } else {
            self.directory.join(format!(
                "settings-{}.json",
                serde_json::to_value(provider)
                    .expect("provider is serializable")
                    .as_str()
                    .expect("provider is a string")
            ))
        }
    }
    fn adapter(&self, provider: ProviderId) -> NativeResult<ProviderClient> {
        ProviderClient::new(provider, self.client.clone()?).map_err(message)
    }
    fn credentials_for(&mut self, provider: ProviderId) -> Arc<CredentialManager> {
        if provider == self.provider {
            return self.credentials.clone();
        }
        self.provider_credentials
            .entry(provider)
            .or_insert_with(|| Arc::new(CredentialManager::for_provider(provider)))
            .clone()
    }
    fn select_provider(&mut self, provider: ProviderId, persist: bool) -> NativeResult<()> {
        self.ensure_switch_allowed()?;
        if !provider.available() {
            return Err("This AI connection is not available yet.".into());
        }
        let authentication = if provider == ProviderId::Openai {
            let path = self.directory.join("authentication.json");
            if path.exists() {
                let p: AuthenticationPreference = read_json(&path, 4096)?;
                if p.version != 1 {
                    return Err("The saved sign-in preference is unsupported.".into());
                }
                p.authentication
            } else {
                AuthenticationMethod::ApiKey
            }
        } else {
            AuthenticationMethod::ApiKey
        };
        let path = self.settings_path(provider);
        let settings = if path.exists() {
            read_json::<ProviderSettings>(&path, 65_536)?
        } else {
            provider.defaults()
        };
        provider
            .validate(&settings, authentication)
            .map_err(message)?;
        self.adapter(provider)?;
        if persist {
            write_json(
                &self.directory.join("provider.json"),
                &ProviderPreference {
                    version: 1,
                    provider,
                },
            )?;
        }
        self.provider_credentials
            .insert(self.provider, self.credentials.clone());
        self.credentials = self.credentials_for(provider);
        self.provider = provider;
        self.authentication = authentication;
        self.settings = settings;
        self.configuration_error = None;
        self.authentication_error = None;
        Ok(())
    }

    pub(super) fn chatgpt_directory(&self) -> PathBuf {
        self.directory.join("chatgpt")
    }

    pub(super) fn client(&self) -> NativeResult<OpenAiClient> {
        self.client.clone()
    }

    pub(super) fn set_authentication(
        &mut self,
        authentication: AuthenticationMethod,
    ) -> NativeResult<()> {
        if self.provider != ProviderId::Openai {
            return Err("Choose OpenAI before changing its sign-in method.".into());
        }
        write_json(
            &self.directory.join("authentication.json"),
            &AuthenticationPreference {
                version: 1,
                authentication,
            },
        )?;
        self.authentication = authentication;
        self.authentication_error = None;
        Ok(())
    }

    fn job_directory(&self, project_id: Uuid, job_id: Uuid) -> PathBuf {
        self.directory
            .join(project_id.to_string())
            .join(job_id.to_string())
    }

    fn persist(&mut self, record: JobRecord) -> NativeResult<GenerationJobView> {
        let directory = self.job_directory(record.view.project_id, record.view.job_id);
        private_directory(&directory)?;
        let markdown = directory.join("REQUIREMENTS.md");
        if markdown.exists() {
            regular_file(&markdown)?;
            let file = File::open(&markdown).map_err(message)?;
            if file.metadata().map_err(message)?.len() > desegni_core::MAX_REQUIREMENTS_BYTES as u64
            {
                return Err(
                    "The frozen requirements file exceeds its limit. The original was preserved."
                        .into(),
                );
            }
            let mut existing = vec![];
            file.take(desegni_core::MAX_REQUIREMENTS_BYTES as u64 + 1)
                .read_to_end(&mut existing)
                .map_err(message)?;
            if existing != record.view.requirements.markdown.as_bytes() {
                return Err("The frozen requirements file changed on disk. The existing record was preserved.".into());
            }
        } else {
            write_markdown(&markdown, &record.view.requirements.markdown).map_err(message)?;
        }
        write_json(&directory.join("job.json"), &record)?;
        let view = record.view.clone();
        self.unsaved_jobs.remove(&view.job_id);
        self.jobs.insert(view.job_id, record);
        self.trim_cache(view.job_id);
        Ok(view)
    }

    fn persist_retaining(&mut self, record: JobRecord) -> NativeResult<GenerationJobView> {
        match self.persist(record.clone()) {
            Ok(view) => Ok(view),
            Err(error) => {
                self.unsaved_jobs.insert(record.view.job_id);
                self.jobs.insert(record.view.job_id, record);
                Err(error)
            }
        }
    }

    fn trim_cache(&mut self, protected_job_id: Uuid) {
        while self.jobs.len() > MAX_CACHED_JOBS {
            let expired = self
                .jobs
                .keys()
                .find(|id| {
                    **id != protected_job_id
                        && !self.active.contains_key(id)
                        && !self.unsaved_jobs.contains(id)
                })
                .copied();
            let Some(expired) = expired else {
                break;
            };
            self.jobs.remove(&expired);
        }
    }

    fn latest(&mut self, project: &Project) -> NativeResult<Option<GenerationJobView>> {
        let latest = self
            .directory
            .join(project.id.to_string())
            .join("latest.json");
        if !latest.exists() {
            return Ok(None);
        }
        let latest: LatestJob = read_json(&latest, 1024)?;
        let mut record = if let Some(record) = self.jobs.get(&latest.job_id) {
            record.clone()
        } else {
            read_json::<JobRecord>(
                &self
                    .job_directory(project.id, latest.job_id)
                    .join("job.json"),
                MAX_JOB_BYTES,
            )?
        };
        if record.version != 1
            || record.view.project_id != project.id
            || record.view.job_id != latest.job_id
            || record.view.requirements.project_id != project.id
            || record.view.requirements.job_id != latest.job_id
            || record.view.requirements.sha256
                != digest(record.view.requirements.markdown.as_bytes())
            || record.view.areas.len() > desegni_core::MAX_GENERATION_REGIONS
            || (!record.view.areas.is_empty()
                && record.view.areas.len() != record.view.requirements.region_count)
            || record.view.areas.iter().any(|area| {
                area.rect.validate(project.viewport).is_err()
                    || !record
                        .view
                        .requirements
                        .dependencies
                        .contains_key(&format!("region:{}:{}", area.page_id, area.region_id))
            })
            || record
                .view
                .areas
                .iter()
                .map(|area| area.region_id)
                .collect::<BTreeSet<_>>()
                .len()
                != record.view.areas.len()
            || record.view.attempts.len() > MAX_ATTEMPTS
            || !record.view.estimate_usd.is_finite()
            || record.view.estimate_usd < 0.0
        {
            return Err("The generation record is invalid. The original was preserved; it was not submitted or applied.".into());
        }
        // Older saved jobs have no drawing metadata. Rebuild it only when their
        // frozen requirements still exactly match the current project.
        if record.view.areas.is_empty()
            && record.view.requirements.validate_current(project).is_ok()
        {
            record.view.areas = project
                .pages
                .iter()
                .flat_map(|page| {
                    page.regions
                        .iter()
                        .filter(|region| {
                            record
                                .view
                                .requirements
                                .dependencies
                                .contains_key(&format!("region:{}:{}", page.id, region.id))
                        })
                        .map(move |region| GenerationArea {
                            page_id: page.id,
                            region_id: region.id,
                            rect: region.rect,
                        })
                })
                .collect();
        }
        record.view.provider_error = record.error.clone();
        if record.view.status == JobStatus::Failed
            && record.error.as_ref().is_some_and(|error| {
                error.kind == ErrorKind::Malformed
                    && error.outcome == SubmissionOutcome::Received
                    && error.usage.is_some()
                    && error.response_format.as_ref().is_some_and(|format| {
                        format.body_kind == desegni_ai::ResponseBodyKind::EventStream
                            && format.rejection.is_none()
                    })
            })
        {
            record.view.message = "The completed reply was rejected by an older reader. The exact failed check was not recorded. Nothing was applied or retried.".into();
        }
        if record.view.status == JobStatus::Unknown
            && record.error.as_ref().is_some_and(|error| {
                error.kind == ErrorKind::OutputLimit
                    && error.outcome == SubmissionOutcome::Unknown
                    && error.http_status == Some(200)
                    && error.response_id.is_none()
                    && error.response_format.as_ref().is_some_and(|format| {
                        format.body_kind == desegni_ai::ResponseBodyKind::Unavailable
                            && format.body_bytes == 0
                            && format.limit_bytes.is_none()
                    })
            })
        {
            record.view.message = "This attempt reached an older error-reply reader's 64 KB limit before Desegnos could identify the reply. Its site size and usage are unknown. Nothing was applied or retried.".into();
        }
        if record.view.status == JobStatus::Unknown
            && record.view.received_bytes == 0
            && record.error.as_ref().is_some_and(|error| {
                error.kind == ErrorKind::Malformed
                    && error.outcome == SubmissionOutcome::Unknown
                    && error.http_status.is_none()
                    && error.response_id.is_none()
                    && error.usage.is_none()
            })
        {
            record.view.message = "OpenAI returned a non-stream response. This older attempt did not save response details, so its exact cause and usage are unknown. Nothing was applied or retried.".into();
        }
        record
            .view
            .provider
            .validate(&record.view.settings, record.view.authentication)
            .map_err(message)?;
        if (record.view.authentication == AuthenticationMethod::Chatgpt)
            != record.view.chatgpt_account_id.is_some()
            || record.view.chatgpt_account_id.is_some_and(|id| id.is_nil())
        {
            return Err("The generation record's authentication binding is invalid. It was not submitted or applied.".into());
        }
        if project.applied_job_ids.contains(&record.view.job_id)
            && record.view.status != JobStatus::Applied
        {
            record.view.status = JobStatus::Applied;
            record.view.message = "This generation was already applied. Paid requests are not replayed by recovery or undo.".into();
            return self.persist(record).map(Some);
        }
        if matches!(
            record.view.status,
            JobStatus::Generating | JobStatus::Applying
        ) && !self.active.contains_key(&record.view.job_id)
        {
            record.view.status = JobStatus::Unknown;
            record.view.message = "Desegnos stopped before this operation finished. The outcome may be uncertain; no request was automatically repeated. Review the project before an explicit retry.".into();
            if let Some(attempt) = record.view.attempts.last_mut() {
                attempt.status = JobStatus::Unknown;
                attempt.finished_at = Some(now());
            }
            return self.persist(record).map(Some);
        }
        if record.view.status == JobStatus::Ready
            && let Err(error) = record.view.requirements.validate_current(project)
        {
            record.view.status = JobStatus::Blocked;
            record.view.message = format!(
                "This candidate is stale: {error} Undo the changed requirements to reuse this candidate, or discard it and generate from the current requirements."
            );
            return self.persist(record).map(Some);
        }
        if record.view.status == JobStatus::Blocked
            && record
                .view
                .candidate
                .as_ref()
                .is_some_and(|candidate| candidate.is_ready())
            && record.view.requirements.validate_current(project).is_ok()
            && record.view.candidate.as_ref().is_some_and(|candidate| {
                candidate
                    .validate(&record.view.requirements, project)
                    .is_ok()
            })
        {
            record.view.status = JobStatus::Ready;
            record.view.message = "The current requirements match this complete candidate again. Review it before applying; no provider request was repeated.".into();
            return self.persist(record).map(Some);
        }
        if self.unsaved_jobs.contains(&record.view.job_id) {
            match self.persist_retaining(record.clone()) {
                Ok(view) => return Ok(Some(view)),
                Err(error) => {
                    let mut view = record.view;
                    view.message = format!(
                        "{} The result is kept in memory, but its local record could not be saved: {error} Keep Desegnos open and restore writable storage before applying. No request will be automatically repeated.",
                        view.message
                    );
                    return Ok(Some(view));
                }
            }
        }
        self.jobs.insert(record.view.job_id, record.clone());
        self.trim_cache(record.view.job_id);
        Ok(Some(record.view))
    }

    fn record(&mut self, project: &Project, job_id: Uuid) -> NativeResult<JobRecord> {
        let view = self
            .latest(project)?
            .filter(|view| view.job_id == job_id)
            .ok_or("This is no longer the active generation request for the project.")?;
        self.jobs
            .get(&view.job_id)
            .cloned()
            .ok_or_else(|| "The generation record is unavailable.".into())
    }

    fn prepare(&mut self, project: &Project) -> NativeResult<GenerationJobView> {
        self.prepare_bound(project, None)
    }

    fn prepare_bound(
        &mut self,
        project: &Project,
        account_id: Option<Uuid>,
    ) -> NativeResult<GenerationJobView> {
        if let Some(error) = &self.configuration_error {
            return Err(error.clone());
        }
        if let Some(error) = &self.authentication_error {
            return Err(error.clone());
        }
        if (self.authentication == AuthenticationMethod::Chatgpt) != account_id.is_some()
            || account_id.is_some_and(|id| id.is_nil())
        {
            return Err("Choose a connected ChatGPT account with plan usage enabled before preparing this request.".into());
        }
        self.client.as_ref().map_err(Clone::clone)?;
        if self
            .latest(project)?
            .is_some_and(|view| view.status.active())
        {
            return Err("Apply or discard the existing generation review, or cancel its running request, before starting another.".into());
        }
        let requirements = project
            .prepare_requirements_with_capabilities(self.provider.capabilities())
            .map_err(message)?;
        let estimate = self
            .adapter(self.provider)?
            .estimate(
                self.authentication,
                &requirements,
                project.generation_mode,
                &desegni_core::result_schema(&requirements),
                &self.settings,
            )
            .map_err(message)?;
        let record = JobRecord {
            version: 1,
            error: None,
            view: GenerationJobView {
                job_id: requirements.job_id,
                project_id: project.id,
                base_revision: project.revision,
                requirements,
                areas: project
                    .pages
                    .iter()
                    .flat_map(|page| {
                        page.regions
                            .iter()
                            .filter(|region| !region.completed && !region.prompt.trim().is_empty())
                            .map(move |region| GenerationArea {
                                page_id: page.id,
                                region_id: region.id,
                                rect: region.rect,
                            })
                    })
                    .collect(),
                generation_mode: project.generation_mode,
                provider: self.provider,
                settings: self.settings.clone(),
                authentication: self.authentication,
                chatgpt_account_id: account_id,
                failure_kind: None,
                provider_error: None,
                status: JobStatus::Prepared,
                estimate_usd: estimate.estimate_usd,
                received_bytes: 0,
                message: if self.authentication == AuthenticationMethod::Chatgpt {
                    "Review the exact requirements and selected ChatGPT account. Sending uses that account's plan allowance or credits; no generation request has been sent.".into()
                } else {
                    "Review the exact requirements, provider and estimated cost before sending. No request has been sent.".into()
                },
                candidate: None,
                usage: None,
                attempts: vec![],
                accepted_decision_indices: vec![],
                recommended_decision_indices: vec![],
                dossier_error: None,
            },
        };
        let view = self.persist(record)?;
        write_json(
            &self
                .directory
                .join(project.id.to_string())
                .join("latest.json"),
            &LatestJob {
                job_id: view.job_id,
            },
        )?;
        Ok(view)
    }

    fn begin(
        &mut self,
        project: &Project,
        job_id: Uuid,
        retry: bool,
    ) -> NativeResult<(GenerationJobView, CancellationToken)> {
        let mut record = self.record(project, job_id)?;
        if project.applied_job_ids.contains(&job_id) {
            return Err(
                "This generation was already applied. Start a new job for further changes.".into(),
            );
        }
        if (!retry && record.view.status != JobStatus::Prepared)
            || (retry && !record.view.status.retryable())
        {
            return Err("This generation cannot be submitted from its current state.".into());
        }
        record
            .view
            .requirements
            .validate_current(project)
            .map_err(message)?;
        let estimate = self
            .adapter(record.view.provider)?
            .estimate(
                record.view.authentication,
                &record.view.requirements,
                record.view.generation_mode,
                &desegni_core::result_schema(&record.view.requirements),
                &record.view.settings,
            )
            .map_err(message)?;
        if estimate.estimate_usd > record.view.estimate_usd + 0.000_001 {
            return Err(
                "The cost estimate changed. Prepare and review a new request before sending."
                    .into(),
            );
        }
        if record.view.attempts.len() >= MAX_ATTEMPTS {
            return Err("This frozen job has reached its twenty-attempt limit. Start a separately reviewed job; earlier paid records remain preserved.".into());
        }
        if self.active.len() >= 4 {
            return Err("Four provider requests are already running. Wait for or cancel one before sending another.".into());
        }
        if let Some(candidate) = &record.view.candidate {
            write_json(
                &self.job_directory(project.id, job_id).join(format!(
                    "attempt-{}-candidate.json",
                    record.view.attempts.len()
                )),
                candidate,
            )?;
        }
        let ordinal = record.view.attempts.len() + 1;
        if let Some(error) = &record.error {
            write_json(
                &self
                    .job_directory(project.id, job_id)
                    .join(format!("attempt-{}-error.json", record.view.attempts.len())),
                error,
            )?;
        }
        record.view.status = JobStatus::Generating;
        record.view.candidate = None;
        record.view.usage = None;
        record.view.received_bytes = 0;
        record.view.failure_kind = None;
        record.view.provider_error = None;
        record.view.message = "Sending the frozen REQUIREMENTS.md and global generation mode to OpenAI. Partial output is not applied.".into();
        record.error = None;
        record.view.attempts.push(GenerationAttempt {
            ordinal,
            status: JobStatus::Generating,
            started_at: now(),
            finished_at: None,
            response_id: None,
            usage: None,
        });
        let view = self.persist(record)?;
        let cancellation = CancellationToken::new();
        self.active.insert(job_id, (ordinal, cancellation.clone()));
        Ok((view, cancellation))
    }

    fn progress(&mut self, job_id: Uuid, ordinal: usize, progress: desegni_ai::ProviderProgress) {
        if self
            .active
            .get(&job_id)
            .is_none_or(|(active, token)| *active != ordinal || token.is_cancelled())
        {
            return;
        }
        if let Some(record) = self.jobs.get_mut(&job_id) {
            record.view.received_bytes = progress.received_bytes;
            if let Some(attempt) = record.view.attempts.last_mut() {
                attempt.response_id = progress.response_id;
            }
        }
    }

    fn finish(
        &mut self,
        original: &Project,
        current: Option<&Project>,
        job_id: Uuid,
        ordinal: usize,
        result: Result<ProviderResponse, ProviderError>,
    ) -> NativeResult<()> {
        if self
            .active
            .get(&job_id)
            .is_none_or(|(active, _)| *active != ordinal)
        {
            let mut record = if let Some(record) = self.jobs.get(&job_id) {
                record.clone()
            } else {
                read_json::<JobRecord>(
                    &self.job_directory(original.id, job_id).join("job.json"),
                    MAX_JOB_BYTES,
                )?
            };
            if let Some(attempt) = record
                .view
                .attempts
                .iter_mut()
                .find(|attempt| attempt.ordinal == ordinal)
            {
                let (response_id, usage) = match result {
                    Ok(response) => (Some(response.response_id), response.usage),
                    Err(error) => (error.response_id, error.usage),
                };
                if response_id.is_some() {
                    attempt.response_id = response_id;
                }
                if usage.is_some() {
                    attempt.usage = usage;
                }
                attempt.finished_at.get_or_insert_with(now);
                self.persist_retaining(record)?;
            }
            return Ok(());
        }
        let cancelled = self
            .active
            .get(&job_id)
            .is_some_and(|(_, token)| token.is_cancelled());
        let Some(mut record) = self.jobs.get(&job_id).cloned() else {
            return Err("The active generation record is unavailable.".into());
        };
        if record.view.status != JobStatus::Generating || cancelled {
            if let Some(attempt) = record.view.attempts.last_mut() {
                let (response_id, usage) = match result {
                    Ok(response) => (Some(response.response_id), response.usage),
                    Err(error) => (error.response_id, error.usage),
                };
                if response_id.is_some() {
                    attempt.response_id = response_id;
                }
                if usage.is_some() {
                    attempt.usage = usage;
                    record.view.usage = usage;
                }
                self.persist_retaining(record)?;
            }
            self.active.remove(&job_id);
            return Ok(());
        }
        match result {
            Ok(response) => {
                record.view.usage = response.usage;
                if let Some(attempt) = record.view.attempts.last_mut() {
                    attempt.response_id = Some(response.response_id);
                    attempt.usage = response.usage;
                }
                // Only the explicit owner-run debug benchmark captures model output.
                // Successful transport has already bounded it and rejected credential echoes.
                #[cfg(all(debug_assertions, feature = "native-tests"))]
                capture_benchmark_output(
                    &self.job_directory(original.id, job_id),
                    ordinal,
                    &response.text,
                    std::env::var("DESEGNI_GENERATION_BENCHMARK").as_deref() == Ok("1"),
                )?;
                match GenerationCandidate::from_reply(&response.text, &record.view.requirements, original).and_then(|mut candidate| {
                    let format = record.view.requirements.format_version()?;
                    if format >= 8 { candidate.resolve_project_assets(original)?; }
                    candidate.validate(&record.view.requirements, original)?;
                    if format >= 7
                        && !candidate.image_download_urls()?.is_empty() {
                        return Err(desegni_core::Error("Images could not be prepared. Nothing was applied. Try generating again or remove the image request.".into()));
                    }
                    Ok(candidate)
                }) {
                    Ok(candidate) => {
                        let stale = current
                            .filter(|project| project.id == original.id)
                            .and_then(|project| {
                                record.view.requirements.validate_current(project).err()
                            });
                        record.view.status = if candidate.is_ready() && stale.is_none() {
                            JobStatus::Ready
                        } else {
                            JobStatus::Blocked
                        };
                        record.view.message = if let Some(error) = stale {
                            format!(
                                "The generated result is stale: {error} Undo the changed requirements to reuse this candidate, or discard it and generate from the current requirements."
                            )
                        } else if candidate.is_ready() {
                            "Review the generated website, component choices, identity proposals and design feedback before applying.".into()
                        } else {
                            "Some requests need a choice. You can edit them or generate a draft with labelled placeholders.".into()
                        };
                        record.view.candidate = Some(candidate);
                    }
                    Err(error) => {
                        record.view.status = JobStatus::Blocked;
                        record.view.message = format!(
                            "Desegnos rejected the candidate: {error} Nothing was applied."
                        );
                    }
                }
            }
            Err(error) => {
                record.view.failure_kind = Some(error.kind);
                record.view.provider_error = Some(error.clone());
                if let Some(format) = &error.response_format {
                    record.view.received_bytes = record.view.received_bytes.max(format.body_bytes);
                }
                record.view.status = if error.kind == ErrorKind::Cancelled {
                    JobStatus::Cancelled
                } else if error.outcome == SubmissionOutcome::Unknown {
                    JobStatus::Unknown
                } else {
                    JobStatus::Failed
                };
                record.view.message = error.to_string();
                record.view.usage = error.usage;
                if let Some(attempt) = record.view.attempts.last_mut() {
                    attempt.response_id = error.response_id.clone();
                    attempt.usage = error.usage;
                }
                record.error = Some(error);
            }
        }
        if let Some(attempt) = record.view.attempts.last_mut() {
            attempt.status = record.view.status;
            attempt.finished_at = Some(now());
        }
        let _ = self.persist_retaining(record);
        self.active.remove(&job_id);
        Ok(())
    }

    fn handle_finish_error(&mut self, job_id: Uuid, ordinal: usize, error: &str) {
        if self
            .active
            .get(&job_id)
            .is_none_or(|(active, _)| *active != ordinal)
        {
            return;
        }
        if let Some(record) = self.jobs.get_mut(&job_id) {
            record.view.status = JobStatus::Unknown;
            record.view.message = format!(
                "Generation stopped while recording its result: {error} No request will be automatically repeated."
            );
        }
        self.active.remove(&job_id);
    }

    fn cancel(&mut self, project: &Project, job_id: Uuid) -> NativeResult<GenerationJobView> {
        if let Some((_, token)) = self.active.get(&job_id) {
            token.cancel();
        }
        let mut record = if let Some(record) = self
            .jobs
            .get(&job_id)
            .filter(|record| record.view.project_id == project.id)
        {
            record.clone()
        } else {
            self.record(project, job_id)?
        };
        if !matches!(
            record.view.status,
            JobStatus::Prepared | JobStatus::Generating
        ) {
            return Err("This request is no longer running. Review or discard its result.".into());
        }
        record.view.status = JobStatus::Cancelled;
        record.view.message = if record.view.authentication == AuthenticationMethod::Chatgpt {
            "Generation cancelled. A submitted request may still consume ChatGPT plan allowance or credits. Late output cannot be applied.".into()
        } else {
            "Generation cancelled. A submitted request may still incur a charge. Late output cannot be applied.".into()
        };
        if let Some(attempt) = record.view.attempts.last_mut() {
            attempt.status = JobStatus::Cancelled;
            attempt.finished_at = Some(now());
        }
        self.jobs.insert(job_id, record.clone());
        match self.persist(record.clone()) {
            Ok(view) => Ok(view),
            Err(error) => {
                record.view.message = format!(
                    "Generation cancelled and late output blocked, but its local cancellation record could not be saved: {error} Restore writable storage. A submitted request may still incur a charge; restart will mark it uncertain and never resubmit automatically."
                );
                let view = record.view.clone();
                self.jobs.insert(job_id, record);
                Ok(view)
            }
        }
    }

    pub(super) fn ensure_switch_allowed(&self) -> NativeResult<()> {
        if self.active.values().any(|(_, token)| !token.is_cancelled()) {
            return Err("Generation is still running. Wait for its result or cancel it before switching projects; local editing and saving remain available.".into());
        }
        Ok(())
    }

    pub(super) fn cancel_account(&mut self, account_id: Uuid) -> NativeResult<()> {
        let jobs: Vec<_> = self
            .jobs
            .values()
            .filter(|record| {
                record.view.chatgpt_account_id == Some(account_id)
                    && matches!(
                        record.view.status,
                        JobStatus::Prepared | JobStatus::Generating
                    )
            })
            .map(|record| record.view.job_id)
            .collect();
        for job_id in jobs {
            if let Some((_, token)) = self.active.get(&job_id) {
                token.cancel();
            }
            let mut record = self
                .jobs
                .get(&job_id)
                .cloned()
                .ok_or("The account's generation record is unavailable.")?;
            record.view.status = JobStatus::Cancelled;
            record.view.message = "Account signed out. Generation was cancelled and late output cannot be applied. A submitted request may still consume ChatGPT plan allowance or credits.".into();
            if let Some(attempt) = record.view.attempts.last_mut() {
                attempt.status = JobStatus::Cancelled;
                attempt.finished_at = Some(now());
            }
            self.jobs.insert(job_id, record.clone());
            if let Err(error) = self.persist(record.clone()) {
                record.view.message = format!(
                    "Account generation was cancelled and late output blocked, but the local cancellation record could not be saved: {error} Restore writable storage. Restart will never repeat it automatically."
                );
                self.jobs.insert(job_id, record);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;

async fn credential_key(state: &AiState<'_>, provider: ProviderId) -> NativeResult<ApiKey> {
    let credentials = state.lock().map_err(message)?.credentials_for(provider);
    tauri::async_runtime::spawn_blocking(move || credentials.key())
        .await
        .map_err(message)?
        .map_err(message)
}

pub(super) async fn settings_view(
    state: &AiState<'_>,
    chatgpt_state: &ChatgptState,
) -> NativeResult<AiSettingsView> {
    let (settings, credentials, configuration_error, authentication, provider) = {
        let service = state.lock().map_err(message)?;
        (
            service.settings.clone(),
            service.credentials.clone(),
            [
                service.configuration_error.clone(),
                service.authentication_error.clone(),
                service.client.as_ref().err().cloned(),
            ]
            .into_iter()
            .flatten()
            .reduce(|first, next| format!("{first} {next}")),
            service.authentication,
            service.provider,
        )
    };
    let credential = tauri::async_runtime::spawn_blocking(move || credentials.status())
        .await
        .map_err(message)?;
    let chatgpt = chatgpt_state.view().await;
    let catalogue = if authentication == AuthenticationMethod::Chatgpt {
        if chatgpt.accounts.iter().any(|account| {
            Some(account.id) == chatgpt.active_account_id
                && account.connected
                && account.plan_enabled
        }) {
            chatgpt
                .models
                .iter()
                .filter_map(|available| {
                    model_catalogue()
                        .into_iter()
                        .find(|verified| verified.id == available.id)
                        .map(|mut verified| {
                            verified.name = available.name.clone();
                            verified
                        })
                })
                .collect()
        } else {
            vec![]
        }
    } else {
        provider.catalogue()
    };
    Ok(AiSettingsView {
        provider,
        providers: provider_catalogue(),
        settings,
        credential,
        catalogue,
        configuration_error,
        authentication,
        chatgpt,
    })
}

#[tauri::command]
pub async fn get_ai_settings(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn set_ai_provider(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    provider: ProviderId,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    state
        .lock()
        .map_err(message)?
        .select_provider(provider, true)?;
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn save_ai_settings(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    settings: ProviderSettings,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    {
        let mut service = state.lock().map_err(message)?;
        service
            .provider
            .validate(&settings, service.authentication)
            .map_err(message)?;
        write_json(&service.settings_path(service.provider), &settings)?;
        service.settings = settings;
        service.configuration_error = None;
        if service.client.is_err() {
            service.client = OpenAiClient::new().map_err(message);
        }
    }
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn set_ai_key(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    key: String,
    storage: CredentialStorage,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    let credentials = state.lock().map_err(message)?.credentials.clone();
    tauri::async_runtime::spawn_blocking(move || credentials.set_key(key, storage))
        .await
        .map_err(message)?
        .map_err(message)?;
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn remove_ai_key(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    let credentials = state.lock().map_err(message)?.credentials.clone();
    tauri::async_runtime::spawn_blocking(move || credentials.remove_key())
        .await
        .map_err(message)?
        .map_err(message)?;
    settings_view(&state, &chatgpt).await
}

#[derive(Serialize)]
pub struct AccessResult {
    message: String,
}

#[tauri::command]
pub async fn test_ai_access(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
) -> NativeResult<AccessResult> {
    trusted(&window)?;
    let (client, settings, authentication, provider) = {
        let service = state.lock().map_err(message)?;
        (
            service.client.clone()?,
            service.settings.clone(),
            service.authentication,
            service.provider,
        )
    };
    if authentication == AuthenticationMethod::Chatgpt {
        let manager = chatgpt.manager()?;
        let account_id = manager
            .view()
            .await
            .active_account_id
            .ok_or("Sign in to ChatGPT before checking access.")?;
        let view = manager
            .discover_models(account_id, &client, &CancellationToken::new())
            .await
            .map_err(message)?;
        if view.active_account_id != Some(account_id) {
            return Err("The selected ChatGPT account changed while checking access. Test the currently selected account again.".into());
        }
        if !view
            .models
            .iter()
            .any(|available| available.id == settings.model)
        {
            return Err(view.models_error.unwrap_or_else(|| "The selected model is not available for this ChatGPT account. Choose one of Desegnos's verified models returned by your account.".into()));
        }
        return Ok(AccessResult { message: "OpenAI confirmed ChatGPT plan permission and access to the selected model. No generation request was made.".into() });
    }
    let key = credential_key(&state, provider).await?;
    ProviderClient::new(provider, client)
        .map_err(message)?
        .test_model_access(&settings, &key, &CancellationToken::new())
        .await
        .map_err(message)?;
    Ok(AccessResult { message: "The AI service accepted the key and confirmed access to the selected model. No text generation request was made.".into() })
}

#[tauri::command]
pub async fn prepare_generation(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
) -> NativeResult<GenerationJobView> {
    trusted(&window)?;
    let (authentication, provider) = {
        let service = state.lock().map_err(message)?;
        (service.authentication, service.provider)
    };
    let account_id = if authentication == AuthenticationMethod::Chatgpt {
        let mut view = chatgpt.view().await;
        let account = view
            .accounts
            .iter()
            .find(|account| Some(account.id) == view.active_account_id)
            .ok_or("Sign in to ChatGPT before preparing generation.")?;
        if !account.connected || !account.plan_enabled {
            return Err(
                "Sign in to this ChatGPT account and grant plan usage before generating.".into(),
            );
        }
        if account.storage_repair_required {
            return Err("Keep Desegnos open and repair this account's secure storage in AI settings before generating.".into());
        }
        let account_id = account.id;
        let model = state.lock().map_err(message)?.settings.model.clone();
        if view.models.is_empty() {
            let client = state.lock().map_err(message)?.client.clone()?;
            view = chatgpt
                .manager()?
                .discover_models(account_id, &client, &CancellationToken::new())
                .await
                .map_err(message)?;
            if view.active_account_id != Some(account_id) {
                return Err("The selected ChatGPT account changed. Generate again using the current account.".into());
            }
        }
        if !view.models.iter().any(|available| available.id == model) {
            return Err(view.models_error.unwrap_or_else(|| {
                "Test ChatGPT access and choose an available verified model before generating."
                    .into()
            }));
        }
        Some(account_id)
    } else {
        let _key = credential_key(&state, provider).await?;
        None
    };
    let workspace = workspace.lock().map_err(message)?;
    let project = workspace.project()?;
    let mut service = state.lock().map_err(message)?;
    if service.authentication != authentication || service.provider != provider {
        return Err("The generation method changed while preparing this request. Review it and prepare again.".into());
    }
    if let Some(account_id) = account_id {
        service.prepare_bound(project, Some(account_id))
    } else {
        service.prepare(project)
    }
}

#[tauri::command]
pub fn get_generation(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    project_id: Uuid,
) -> NativeResult<Option<GenerationJobView>> {
    trusted(&window)?;
    let mut workspace = workspace.lock().map_err(message)?;
    let project = workspace.project()?;
    if project.id != project_id {
        return Err("The active project changed. Reopen its generation review.".into());
    }
    let mut service = state.lock().map_err(message)?;
    let view = service.latest(project)?;
    view.map(|view| maintain_dossier(&mut workspace, &mut service, view))
        .transpose()
}

fn maintain_dossier(
    workspace: &mut Workspace,
    service: &mut GenerationService,
    mut view: GenerationJobView,
) -> NativeResult<GenerationJobView> {
    if !matches!(view.status, JobStatus::Ready | JobStatus::Blocked) {
        return Ok(view);
    }
    let Some(candidate) = &view.candidate else {
        return Ok(view);
    };
    if view
        .requirements
        .validate_current(workspace.project()?)
        .is_err()
    {
        return Ok(view);
    }
    let recommended = workspace
        .project()?
        .compatible_decision_proposals(&view.requirements, candidate)
        .map_err(message)?;
    let error = workspace
        .record_generated(&view.requirements, candidate, view.attempts.len())
        .err();
    if recommended != view.recommended_decision_indices || error != view.dossier_error {
        view.recommended_decision_indices = recommended;
        view.dossier_error = error;
        let mut record = service.record(workspace.project()?, view.job_id)?;
        view.message.clone_from(&record.view.message);
        record.view = view.clone();
        service.persist(record)?;
    }
    Ok(view)
}

async fn submit(
    app: AppHandle,
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    job_id: Uuid,
    retry: bool,
) -> NativeResult<GenerationJobView> {
    trusted(&window)?;
    let (authentication, account_id, provider) = {
        let workspace = workspace.lock().map_err(message)?;
        let mut service = state.lock().map_err(message)?;
        let record = service.record(workspace.project()?, job_id)?;
        (
            record.view.authentication,
            record.view.chatgpt_account_id,
            record.view.provider,
        )
    };
    let epoch = account_id.map(|id| chatgpt.epoch(id)).transpose()?;
    let key = if authentication == AuthenticationMethod::Chatgpt {
        chatgpt
            .manager()?
            .bearer(
                account_id
                    .ok_or("This ChatGPT request has no bound account. Prepare a new request.")?,
                &CancellationToken::new(),
            )
            .await
            .map_err(message)?
    } else {
        credential_key(&state, provider).await?
    };
    let begin = || {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?.clone();
        let mut service = state.lock().map_err(message)?;
        let client = service.adapter(provider)?;
        let (view, cancellation) = service.begin(&project, job_id, retry)?;
        Ok((project, client, view, cancellation))
    };
    let (original, client, view, cancellation) =
        if let (Some(account_id), Some(epoch)) = (account_id, epoch) {
            chatgpt.with_epoch(account_id, epoch, begin)?
        } else {
            begin()?
        };
    let request = view.clone();
    tauri::async_runtime::spawn(async move {
        let ordinal = request.attempts.len();
        let (shared_app, request_ref, key_ref, cancel_ref, client_ref) =
            (&app, &request, &key, &cancellation, &client);
        let ask = |requirements: &RequirementsSnapshot| {
            let progress_app = shared_app.clone();
            let requirements = requirements.clone();
            async move {
                let schema = desegni_core::result_schema(&requirements);
                client_ref
                    .generate_for_auth(
                        request_ref.authentication,
                        &requirements,
                        request_ref.generation_mode,
                        &schema,
                        &request_ref.settings,
                        key_ref,
                        cancel_ref,
                        move |progress| {
                            if let Ok(mut service) =
                                progress_app.state::<Mutex<GenerationService>>().lock()
                            {
                                service.progress(job_id, ordinal, progress);
                            }
                        },
                    )
                    .await
            }
        };
        let mut result = ask(&request.requirements).await;
        // An element fix may ask once for other elements' HTML; reply automatically and keep
        // the final answer tied to the frozen requirements.
        if let Ok(first) = &result
            && let Ok(Some(follow_up)) = request
                .requirements
                .context_follow_up(&first.text, &original)
        {
            let usage = first.usage;
            result = ask(&follow_up).await.map(|mut response| {
                if let Ok(text) = request
                    .requirements
                    .adopt_follow_up_reply(&follow_up, &response.text)
                {
                    response.text = text;
                }
                response.usage = match (usage, response.usage) {
                    (Some(a), Some(b)) => Some(desegni_ai::TokenUsage {
                        input_tokens: a.input_tokens + b.input_tokens,
                        output_tokens: a.output_tokens + b.output_tokens,
                        total_tokens: a.total_tokens + b.total_tokens,
                    }),
                    (a, b) => b.or(a),
                };
                response
            });
        }
        let result = match result {
            Ok(response) => Ok(client
                .download_images(response, &request.requirements, &original, &cancellation)
                .await),
            Err(error) => Err(error),
        };
        let workspace_state = app.state::<Mutex<Workspace>>();
        let service_state = app.state::<Mutex<GenerationService>>();
        if let Ok(mut workspace) = workspace_state.lock()
            && let Ok(mut service) = service_state.lock()
        {
            let current = workspace.project().ok().cloned();
            if let Err(error) = service.finish(&original, current.as_ref(), job_id, ordinal, result)
            {
                service.handle_finish_error(job_id, ordinal, &error);
            } else if current
                .as_ref()
                .is_some_and(|project| project.id == original.id)
                && let Some(record) = service.jobs.get(&job_id)
            {
                let view = record.view.clone();
                if let Err(error) = maintain_dossier(&mut workspace, &mut service, view)
                    && let Some(record) = service.jobs.get_mut(&job_id)
                {
                    record.view.dossier_error = Some(format!(
                        "The result is preserved, but its project dossier could not be saved: {error}"
                    ));
                }
            }
        }
    });
    Ok(view)
}

#[tauri::command]
pub async fn start_generation(
    app: AppHandle,
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    job_id: Uuid,
) -> NativeResult<GenerationJobView> {
    submit(app, window, workspace, state, chatgpt, job_id, false).await
}

#[tauri::command]
pub async fn retry_generation(
    app: AppHandle,
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    job_id: Uuid,
) -> NativeResult<GenerationJobView> {
    submit(app, window, workspace, state, chatgpt, job_id, true).await
}

#[tauri::command]
pub fn cancel_generation(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    job_id: Uuid,
) -> NativeResult<GenerationJobView> {
    trusted(&window)?;
    let workspace = workspace.lock().map_err(message)?;
    let mut service = state.lock().map_err(message)?;
    service.cancel(workspace.project()?, job_id)
}

#[tauri::command]
pub fn discard_generation(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    job_id: Uuid,
) -> NativeResult<GenerationJobView> {
    trusted(&window)?;
    let workspace = workspace.lock().map_err(message)?;
    let mut service = state.lock().map_err(message)?;
    let mut record = service.record(workspace.project()?, job_id)?;
    if matches!(
        record.view.status,
        JobStatus::Generating | JobStatus::Applying | JobStatus::Applied
    ) {
        return Err(
            "Cancel running generation or finish its application before discarding.".into(),
        );
    }
    record.view.status = JobStatus::Discarded;
    record.view.message = "Candidate discarded. The project was not changed; the job's request and paid-attempt records remain available locally.".into();
    service.persist(record)
}

#[tauri::command]
pub fn apply_generation(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    job_id: Uuid,
    accepted_decision_indices: Vec<usize>,
) -> NativeResult<WorkspaceView> {
    trusted(&window)?;
    let mut workspace = workspace.lock().map_err(message)?;
    let mut service = state.lock().map_err(message)?;
    let mut record = service.record(workspace.project()?, job_id)?;
    if workspace.project()?.applied_job_ids.contains(&job_id) {
        return Ok(workspace.view());
    }
    if record.view.status != JobStatus::Ready {
        return Err("Only a complete, current candidate can be applied.".into());
    }
    let candidate = record
        .view
        .candidate
        .as_ref()
        .ok_or("The candidate is unavailable.")?;
    workspace
        .project()?
        .apply_generation(
            &record.view.requirements,
            candidate,
            &accepted_decision_indices,
        )
        .map_err(message)?;
    let checkpoint = service
        .job_directory(record.view.project_id, job_id)
        .join(format!(
            "before-apply-{}.desegni",
            workspace.project()?.revision
        ));
    let file = ProjectFile::acquire(&checkpoint).map_err(message)?;
    if checkpoint.exists() {
        if file.load().map_err(message)? != *workspace.project()? {
            return Err(
                "An existing safety checkpoint differs from the current project. It was preserved."
                    .into(),
            );
        }
    } else {
        file.save(workspace.project()?).map_err(message)?;
    }
    drop(file);
    record.view.status = JobStatus::Applying;
    record.view.accepted_decision_indices = accepted_decision_indices.clone();
    service.persist(record.clone())?;
    if let Err(error) = workspace.apply_generated(
        &record.view.requirements,
        record
            .view
            .candidate
            .as_ref()
            .ok_or("The candidate is unavailable.")?,
        &accepted_decision_indices,
    ) {
        record.view.status = JobStatus::Ready;
        record.view.message =
            format!("Apply did not complete: {error} The prior project remains intact.");
        let _ = service.persist(record);
        return Err(error);
    }
    record.view.status = JobStatus::Applied;
    record.view.message = "Generated site and selected identity decisions applied together. A safety checkpoint was retained; undo does not replay the paid request.".into();
    if let Err(error) = service.persist(record.clone()) {
        record.view.message = format!(
            "The project was applied and kept locally, but its job-history update failed: {error} The applied job ID prevents duplicate changes."
        );
        service.jobs.insert(job_id, record);
    }
    Ok(workspace.view())
}

#[tauri::command]
pub fn create_preview(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    previews: State<'_, Mutex<PreviewRegistry>>,
    page_id: Uuid,
    job_id: Option<Uuid>,
    run_scripts: bool,
) -> NativeResult<PreviewDescriptor> {
    trusted(&window)?;
    let workspace = workspace.lock().map_err(message)?;
    let project = workspace.project()?;
    let candidate = if let Some(job_id) = job_id {
        state
            .lock()
            .map_err(message)?
            .record(project, job_id)?
            .view
            .candidate
    } else {
        None
    };
    if job_id.is_some() && candidate.is_none() {
        return Err("There is no generated source to preview for this job.".into());
    }
    previews
        .lock()
        .map_err(message)?
        .register(project, candidate.as_ref(), page_id, run_scripts)
}

#[tauri::command]
pub fn release_preview(
    window: WebviewWindow,
    previews: State<'_, Mutex<PreviewRegistry>>,
    session_id: Uuid,
) -> NativeResult<()> {
    trusted(&window)?;
    previews.lock().map_err(message)?.release(session_id);
    Ok(())
}

/// Native-only access binding for identity analysis. Credentials never enter an
/// IPC view, project, source snapshot, or persisted analysis record.
pub(super) struct IdentityAccess {
    pub authentication: AuthenticationMethod,
    pub account_id: Option<Uuid>,
    pub account_epoch: Option<u64>,
    pub settings: ProviderSettings,
    pub client: ProviderClient,
    pub key: ApiKey,
}

pub(super) async fn identity_access(
    state: &Mutex<GenerationService>,
    chatgpt: &ChatgptState,
    cancellation: &CancellationToken,
) -> NativeResult<IdentityAccess> {
    let (authentication, settings, client, credentials, provider) = {
        let service = state.lock().map_err(message)?;
        if let Some(error) = service
            .configuration_error
            .as_ref()
            .or(service.authentication_error.as_ref())
        {
            return Err(error.clone());
        }
        service
            .provider
            .validate(&service.settings, service.authentication)
            .map_err(message)?;
        (
            service.authentication,
            service.settings.clone(),
            service.client.clone()?,
            service.credentials.clone(),
            service.provider,
        )
    };
    let (account_id, account_epoch, key) = if authentication == AuthenticationMethod::Chatgpt {
        let mut view = chatgpt.view().await;
        let account = view
            .accounts
            .iter()
            .find(|account| Some(account.id) == view.active_account_id)
            .ok_or("Sign in to ChatGPT in AI settings before analyzing identity.")?;
        if !account.connected || !account.plan_enabled {
            return Err(
                "Reconnect this ChatGPT account and grant plan use before analyzing identity."
                    .into(),
            );
        }
        if account.storage_repair_required {
            return Err(
                "Repair this account's secure storage in AI settings before analyzing identity."
                    .into(),
            );
        }
        let account_id = account.id;
        let epoch = chatgpt.epoch(account_id)?;
        if view.models.is_empty() {
            view = chatgpt
                .manager()?
                .discover_models(account_id, &client, cancellation)
                .await
                .map_err(message)?;
        }
        if view.active_account_id != Some(account_id)
            || !view.models.iter().any(|model| model.id == settings.model)
        {
            return Err(view.models_error.unwrap_or_else(|| "Choose an available verified ChatGPT model in AI settings before analyzing identity.".into()));
        }
        let key = chatgpt
            .manager()?
            .bearer(account_id, cancellation)
            .await
            .map_err(message)?;
        validate_identity_account(&chatgpt.view().await, account_id, &settings.model)?;
        chatgpt.with_epoch(account_id, epoch, || Ok(()))?;
        (Some(account_id), Some(epoch), key)
    } else {
        let key = tauri::async_runtime::spawn_blocking(move || credentials.key())
            .await
            .map_err(message)?
            .map_err(message)?;
        ProviderClient::new(provider, client.clone())
            .map_err(message)?
            .test_model_access(&settings, &key, cancellation)
            .await
            .map_err(message)?;
        (None, None, key)
    };
    if cancellation.is_cancelled() {
        return Err("Identity analysis was cancelled before submission.".into());
    }
    let service = state.lock().map_err(message)?;
    if service.authentication != authentication
        || service.provider != provider
        || service.settings != settings
    {
        return Err(
            "AI settings changed while checking access. Review them and explicitly analyze again."
                .into(),
        );
    }
    Ok(IdentityAccess {
        authentication,
        account_id,
        account_epoch,
        settings,
        client: ProviderClient::new(provider, client).map_err(message)?,
        key,
    })
}

fn validate_identity_account(
    view: &ChatgptView,
    account_id: Uuid,
    model: &str,
) -> NativeResult<()> {
    let account = view
        .accounts
        .iter()
        .find(|account| account.id == account_id);
    if view.active_account_id != Some(account_id)
        || !account.is_some_and(|account| {
            account.connected && account.plan_enabled && !account.storage_repair_required
        })
        || !view.models.iter().any(|available| available.id == model)
    {
        return Err("The selected ChatGPT account, its plan permission, secure storage, or model changed while checking access. Review AI settings and explicitly analyze again.".into());
    }
    Ok(())
}
