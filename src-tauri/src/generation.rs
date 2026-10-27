use crate::{
    chatgpt::ChatgptState,
    workspace::{Workspace, WorkspaceView},
};

pub(crate) use crate::chatgpt::ChatgptState as ExportChatgptState;
use desegnos_ai::{
    ApiKey, AuthenticationMethod, CancellationToken, ChatgptView, CredentialInfo,
    CredentialManager, CredentialStorage, ErrorKind, ModelInfo, OpenAiClient, ProviderClient,
    ProviderDescriptor, ProviderError, ProviderId, ProviderResponse, ProviderSettings,
    SubmissionOutcome, TokenUsage, model_catalogue, provider_catalogue,
};
use desegnos_core::{GenerationCandidate, GenerationMode, Project, RequirementsSnapshot, digest};
use desegnos_storage::write_markdown;
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
pub(crate) type AiState<'a> = State<'a, Mutex<GenerationService>>;
type WorkspaceState<'a> = State<'a, Mutex<Workspace>>;
// At most 20 attempts, two bounded 8 MiB replies each, plus reconstructed page source.
const MAX_JOB_BYTES: u64 = 768 * 1024 * 1024;
const MAX_CACHED_JOBS: usize = 16;
const MAX_ATTEMPTS: usize = 20;

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn collect_generation_targets(
    page_id: Uuid,
    elements: &[desegnos_core::Element],
    requested: &BTreeSet<Uuid>,
    rows: &mut Vec<GenerationTarget>,
) {
    for element in elements {
        if requested.contains(&element.id) {
            rows.push(GenerationTarget {
                page_id,
                element_id: element.id,
            });
        }
        collect_generation_targets(page_id, &element.children, requested, rows);
    }
}

/// A stored candidate is usable when it still belongs to its frozen request.
fn resolve_current(view: &GenerationJobView, project: &Project) -> NativeResult<()> {
    let candidate = view
        .candidate
        .as_ref()
        .ok_or("The candidate is unavailable.")?;
    candidate
        .validate(view.request_requirements(), project)
        .map_err(message)?;
    Ok(())
}

#[cfg(feature = "native-tests")]
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
struct GenerationTarget {
    page_id: Uuid,
    element_id: Uuid,
}

/// One sequential part of a request whose editable documents exceed the source budget.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GenerationBatch {
    part: usize,
    requirements: RequirementsSnapshot,
    estimate_usd: f64,
    candidate: Option<GenerationCandidate>,
    status: JobStatus,
    message: String,
    validation_events: Vec<desegnos_core::ValidationEvent>,
    repair_rounds: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationJobView {
    job_id: Uuid,
    project_id: Uuid,
    base_revision: u64,
    requirements: RequirementsSnapshot,
    /// Format 25: one frozen packet per sequential part.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    batches: Vec<GenerationBatch>,
    /// Format 25: every editable document without sources, used to validate and apply the
    /// merged result of all parts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    merged_requirements: Option<RequirementsSnapshot>,
    source_fingerprints: BTreeMap<Uuid, BTreeMap<String, String>>,
    applied_source_fingerprints: BTreeMap<Uuid, BTreeMap<String, String>>,
    context: Option<RequirementsSnapshot>,
    draft_feedback: Vec<desegnos_core::ValidationEvent>,
    earlier_contexts: Vec<RequirementsSnapshot>,
    raw_reply: Option<String>,
    validation_events: Vec<desegnos_core::ValidationEvent>,
    validation_counts_recorded: bool,
    repair_rounds: usize,
    first_exchange: Option<ValidationExchange>,
    repair_area_count: usize,
    created_at: Option<String>,
    areas: Vec<GenerationTarget>,
    generation_mode: GenerationMode,
    provider: ProviderId,
    settings: ProviderSettings,
    authentication: AuthenticationMethod,
    chatgpt_account_id: Option<Uuid>,
    failure_kind: Option<ErrorKind>,
    provider_error: Option<ProviderError>,
    status: JobStatus,
    estimate_usd: f64,
    received_bytes: usize,
    message: String,
    candidate: Option<GenerationCandidate>,
    usage: Option<TokenUsage>,
    attempts: Vec<GenerationAttempt>,
    accepted_decision_indices: Vec<usize>,
    recommended_decision_indices: Vec<usize>,
    dossier_error: Option<String>,
}

impl GenerationJobView {
    /// The snapshot that validates and applies the complete result. A batched job uses the
    /// merged document set; a single request uses its only snapshot.
    fn request_requirements(&self) -> &RequirementsSnapshot {
        self.merged_requirements
            .as_ref()
            .unwrap_or(&self.requirements)
    }

}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidationExchange {
    summary: String,
    requests: Vec<desegnos_core::RequestOutcome>,
    usage: Option<TokenUsage>,
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

/// The project's request-history index. The format tag names the requirements format that
/// produced the entries, so an index written by another format is ignored instead of being
/// mixed into a current request.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestHistoryIndex {
    format: u8,
    entries: Vec<desegnos_core::EarlierRequest>,
}

/// Reads only the frozen requirements heading, before the complete record is decoded.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobFormatProbe {
    view: Option<ViewFormatProbe>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewFormatProbe {
    requirements: Option<RequirementsFormatProbe>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequirementsFormatProbe {
    markdown: String,
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
    image_credential: Option<String>,
    image_key: CredentialInfo,
    main_image_access: Option<desegnos_ai::ImageAccess>,
    image_key_access: Option<desegnos_ai::ImageAccess>,
    image_access: Option<desegnos_ai::ImageAccess>,
    image_access_checking: bool,
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
    image_credentials: Arc<CredentialManager>,
    image_access: BTreeMap<String, desegnos_ai::ImageAccess>,
    image_access_checking: BTreeMap<String, Uuid>,
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

fn regular_directory(path: &Path) -> NativeResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(message)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("The generation folder is not a regular directory.".into());
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationJobSummary {
    job_id: Uuid,
    created_at: Option<String>,
    status: JobStatus,
    usage: Option<TokenUsage>,
    summary: String,
    first_request: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationHistory {
    jobs: Vec<GenerationJobSummary>,
    skipped: usize,
    truncated: bool,
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

fn read_limited(path: &Path, limit: u64) -> NativeResult<Vec<u8>> {
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
    Ok(bytes)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> NativeResult<T> {
    let bytes = read_limited(path, limit)?;
    serde_json::from_slice(&bytes).map_err(|_| "A generation record is damaged or uses an unsupported format. The original was preserved.".into())
}

/// A stored job is absent, not an error, when it was frozen by another requirements format or
/// cannot be decoded as one. The current format has no compatibility path, so such records are
/// never shown, resubmitted or upgraded; their files stay untouched on disk.
fn read_job_record(directory: &Path) -> NativeResult<Option<JobRecord>> {
    let path = directory.join("job.json");
    if !path.exists() {
        return Ok(None);
    }
    let bytes = read_limited(&path, MAX_JOB_BYTES)?;
    let Ok(probe) = serde_json::from_slice::<JobFormatProbe>(&bytes) else {
        return Ok(None);
    };
    let markdown = probe
        .view
        .and_then(|view| view.requirements)
        .map(|requirements| requirements.markdown);
    if !markdown.is_some_and(|markdown| {
        markdown.starts_with(&desegnos_core::current_requirements_heading())
    }) {
        return Ok(None);
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|_| {
        "A generation record is damaged. The original was preserved; it was not submitted or applied.".into()
    })
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
    fn image_cache_id(&self, account: Option<Uuid>) -> Option<String> {
        if self.provider != ProviderId::Openai {
            return None;
        }
        match self.authentication {
            AuthenticationMethod::Chatgpt => account.map(|id| format!("chatgpt:{id}")),
            AuthenticationMethod::ApiKey => self
                .credentials
                .key()
                .ok()
                .map(|key| key.image_access_cache_key()),
        }
    }
    fn image_key_cache_id(&self) -> Option<String> {
        self.image_credentials
            .key()
            .ok()
            .map(|key| key.image_access_cache_key())
    }
    fn selected_image_cache_id(&self, account: Option<Uuid>) -> Option<String> {
        let main = self.image_cache_id(account);
        if self.provider != ProviderId::Openai {
            return main;
        }
        let image = self.image_key_cache_id();
        if desegnos_ai::use_image_key(
            main.as_ref().and_then(|id| self.image_access.get(id)),
            image.as_ref().and_then(|id| self.image_access.get(id)),
        ) {
            image
        } else {
            main
        }
    }
    fn store_image_access(
        &mut self,
        id: String,
        access: desegnos_ai::ImageAccess,
    ) -> NativeResult<()> {
        self.image_access.insert(id, access);
        write_json(
            &self.directory.join("image-api-access.json"),
            &self.image_access,
        )
    }
    pub(super) fn invalidate_image_account(&mut self, account: Uuid) -> NativeResult<()> {
        let id = format!("chatgpt:{account}");
        self.image_access.remove(&id);
        // A reconnect invalidates any outstanding probe for the previous session.
        self.image_access_checking.remove(&id);
        write_json(
            &self.directory.join("image-api-access.json"),
            &self.image_access,
        )
    }
    pub fn initialize(data: &Path) -> NativeResult<Self> {
        Self::from_directory(data.join("generation"))
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
            if let Ok(endpoint) = std::env::var("DESEGNOS_NATIVE_TEST_OPENAI_ENDPOINT") {
                (
                    Arc::new(CredentialManager::isolated_for_tests()),
                    OpenAiClient::for_loopback(&endpoint).map_err(message),
                )
            } else if cfg!(debug_assertions)
                && std::env::var("DESEGNOS_GENERATION_BENCHMARK").as_deref() == Ok("1")
            {
                (Arc::new(CredentialManager::isolated_for_tests()), client)
            } else {
                (credentials, client)
            };
        let image_cache_path = directory.join("image-api-access.json");
        let image_access = if image_cache_path.exists() {
            read_json(&image_cache_path, 1024 * 1024)?
        } else {
            BTreeMap::new()
        };
        let mut service = Self {
            image_credentials: Arc::new(CredentialManager::for_images()),
            image_access,
            image_access_checking: BTreeMap::new(),
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

    /// The provider, authentication and frozen settings an export job records.
    pub(super) fn export_connection(
        &self,
    ) -> (AuthenticationMethod, ProviderId, ProviderSettings) {
        (
            self.authentication,
            self.provider,
            self.settings.clone(),
        )
    }

    pub(super) fn export_client(&self, provider: ProviderId) -> NativeResult<ProviderClient> {
        self.adapter(provider)
    }

    pub(super) fn export_key(&mut self, provider: ProviderId) -> NativeResult<ApiKey> {
        self.credentials_for(provider).key().map_err(message)
    }

    /// The same complete request check the paid send performs, without credentials or network.
    pub(super) fn export_estimate(
        &self,
        plan: &desegnos_core::ExportPlan,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
    ) -> NativeResult<f64> {
        let client = self.adapter(self.provider)?;
        client
            .estimate(
                self.authentication,
                snapshot,
                mode,
                &plan.schema,
                &self.settings,
            )
            .map(|estimate| estimate.estimate_usd)
            .map_err(message)
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
        // The on-disk REQUIREMENTS.md is the job's first packet; later parts are frozen in the
        // job record with the summaries they carried forward.
        let primary = record
            .view
            .batches
            .first()
            .map_or(&record.view.requirements, |batch| &batch.requirements);
        let markdown = directory.join("REQUIREMENTS.md");
        if markdown.exists() {
            regular_file(&markdown)?;
            let file = File::open(&markdown).map_err(message)?;
            if file.metadata().map_err(message)?.len()
                > desegnos_core::MAX_REQUIREMENTS_BYTES as u64
            {
                return Err(
                    "The frozen requirements file exceeds its limit. The original was preserved."
                        .into(),
                );
            }
            let mut existing = vec![];
            file.take(desegnos_core::MAX_REQUIREMENTS_BYTES as u64 + 1)
                .read_to_end(&mut existing)
                .map_err(message)?;
            if existing != primary.markdown.as_bytes() {
                return Err("The frozen requirements file changed on disk. The existing record was preserved.".into());
            }
        } else {
            write_markdown(&markdown, &primary.markdown).map_err(message)?;
        }
        if let Some(context) = &record.view.context {
            if context.markdown.len() > desegnos_core::MAX_REQUIREMENTS_BYTES
                || context.sha256 != digest(context.markdown.as_bytes())
                || context.job_id != record.view.job_id
                || !context
                    .markdown
                    .starts_with(&record.view.requirements.markdown)
            {
                return Err("The follow-up context is invalid.".into());
            }
            let path = directory.join("CONTEXT.md");
            if path.exists() {
                regular_file(&path)?;
                if fs::metadata(&path).map_err(message)?.len()
                    > desegnos_core::MAX_REQUIREMENTS_BYTES as u64
                    || fs::read(&path).map_err(message)? != context.markdown.as_bytes()
                {
                    return Err(
                        "The saved follow-up context changed. The original was preserved.".into(),
                    );
                }
            } else {
                write_markdown(&path, &context.markdown).map_err(message)?;
            }
        }
        if let Some(reply) = &record.view.raw_reply {
            if reply.len() > 64 * 1024 {
                return Err("The failed reply exceeds its diagnostic limit.".into());
            }
            write_markdown(&directory.join("REPLY.txt"), reply).map_err(message)?;
        }
        write_json(&directory.join("job.json"), &record)?;
        let history_path = self.history_path(record.view.project_id);
        if history_path.exists() {
            let mut entries = self.saved_history(record.view.project_id)?;
            entries.retain(|r| {
                r.project_id == record.view.project_id && r.job_id != record.view.job_id
            });
            entries.extend(self.history_entries(&record.view));
            desegnos_core::compact_request_history(&mut entries);
            write_json(
                &history_path,
                &RequestHistoryIndex {
                    format: desegnos_core::CURRENT_REQUIREMENTS_FORMAT,
                    entries,
                },
            )?;
        }
        let view = record.view.clone();
        self.unsaved_jobs.remove(&view.job_id);
        self.jobs.insert(view.job_id, record);
        self.trim_cache(view.job_id);
        Ok(view)
    }

    /// A stored job of another requirements format is absent rather than an error.
    fn saved_job(&self, project_id: Uuid, job_id: Uuid) -> NativeResult<Option<GenerationJobView>> {
        let record = if let Some(record) = self.jobs.get(&job_id) {
            Some(record.clone())
        } else {
            read_job_record(&self.job_directory(project_id, job_id))?
        };
        let Some(record) = record else {
            return Ok(None);
        };
        let view = record.view;
        if record.version != 1
            || view.requirements.format_version().is_err()
            || view.project_id != project_id
            || view.job_id != job_id
            || view.requirements.project_id != project_id
            || view.requirements.job_id != job_id
            || view.requirements.markdown.len() > desegnos_core::MAX_REQUIREMENTS_BYTES
            || view.requirements.sha256 != digest(view.requirements.markdown.as_bytes())
        {
            return Err("The saved generation record is invalid.".into());
        }
        Ok(Some(view))
    }

    // Compact text-only index of current-format requests.
    fn history_path(&self, project_id: Uuid) -> PathBuf {
        self.directory
            .join(project_id.to_string())
            .join("request-history.json")
    }

    /// The derived history index is absent when it was written by another requirements format or
    /// cannot be decoded; the next save replaces it with a current-format index.
    fn saved_history(
        &self,
        project_id: Uuid,
    ) -> NativeResult<Vec<desegnos_core::EarlierRequest>> {
        let path = self.history_path(project_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = read_limited(&path, MAX_JOB_BYTES)?;
        let Ok(index) = serde_json::from_slice::<RequestHistoryIndex>(&bytes) else {
            return Ok(Vec::new());
        };
        if index.format != desegnos_core::CURRENT_REQUIREMENTS_FORMAT {
            return Ok(Vec::new());
        }
        Ok(index.entries)
    }

    fn history_entries(&self, view: &GenerationJobView) -> Vec<desegnos_core::EarlierRequest> {
        let created = view
            .created_at
            .as_deref()
            .or_else(|| view.attempts.first().map(|a| a.started_at.as_str()))
            .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
            .map(|t| t.unix_timestamp())
            .or_else(|| {
                fs::metadata(
                    self.job_directory(view.project_id, view.job_id)
                        .join("job.json"),
                )
                .ok()?
                .modified()
                .ok()
                .map(|t| OffsetDateTime::from(t).unix_timestamp())
            })
            .unwrap_or(0);
        let outcome = match view.status {
            JobStatus::Applied => "applied",
            JobStatus::Discarded
                if view.failure_kind.is_some() || view.provider_error.is_some() =>
            {
                "failed"
            }
            JobStatus::Discarded => "rejected by user",
            JobStatus::Failed | JobStatus::Unknown | JobStatus::Cancelled => "failed",
            _ => "drafted",
        };
        let mut entries = desegnos_core::earlier_requests(
            view.request_requirements(),
            created,
            outcome,
            view.candidate
                .as_ref()
                .map(|c| c.summary.as_str())
                .or_else(|| view.first_exchange.as_ref().map(|e| e.summary.as_str()))
                .filter(|summary| !summary.trim().is_empty())
                .unwrap_or("No AI summary recorded."),
            view.candidate
                .as_ref()
                .map_or(&[], |c| c.requests.as_slice()),
        );
        for entry in &mut entries {
            // Discarding a blocked result must not turn a validator rejection into user intent.
            // Only unresolved conflicts count: repaired events and provider errors do not.
            if view.candidate.as_ref().map_or(
                matches!(view.status, JobStatus::Blocked | JobStatus::Discarded)
                    && view.failure_kind.is_none(),
                |c| {
                    c.requests.iter().any(|r| {
                        r.element_id == entry.element_id
                            && r.status == desegnos_core::RequestStatus::Conflict
                    })
                },
            ) && let Some(event) = view
                .validation_events
                .iter()
                .rev()
                .find(|e| e.disposition == "conflicted" && e.element_id == Some(entry.element_id))
                .or_else(|| {
                    view.validation_events.iter().rev().find(|e| {
                        e.disposition == "conflicted"
                            && e.element_id.is_none()
                            && !matches!(e.kind.as_str(), "repairProvider" | "repairStorage")
                    })
                })
            {
                desegnos_core::record_validation_rejection(entry, &event.message);
            }
        }
        entries
    }

    pub(crate) fn request_history(
        &self,
        project: &Project,
    ) -> NativeResult<Vec<desegnos_core::EarlierRequest>> {
        let path = self.history_path(project.id);
        // A crash can leave current job records durable before their history index.
        // Reconstruct a missing index only from validated current-format records. An index from
        // another requirements format is absent, never read as current earlier-request context.
        let mut entries: Vec<desegnos_core::EarlierRequest> = if path.exists() {
            self.saved_history(project.id)?
        } else {
            let directory = self.directory.join(project.id.to_string());
            let mut entries = Vec::new();
            let mut observations = Vec::new();
            if directory.exists() {
                regular_directory(&directory)?;
                for entry in fs::read_dir(directory).map_err(message)? {
                    let entry = entry.map_err(message)?;
                    let Some(id) = entry
                        .file_name()
                        .to_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                    else {
                        continue;
                    };
                    if let Ok(Some(view)) = self.saved_job(project.id, id) {
                        entries.extend(self.history_entries(&view));
                        if let Some(context) = &view.requirements.dossier_context {
                            observations.push((
                                view.requirements.base_revision,
                                context.job_id,
                                context.application_status,
                            ));
                        }
                    }
                }
            }
            // Later frozen dossiers can preserve an undo after the project's latest dossier moved on.
            observations.sort_by_key(|(revision, _, _)| *revision);
            for (_, job_id, status) in observations {
                for entry in entries.iter_mut().filter(|r| r.job_id == job_id) {
                    match status {
                        desegnos_core::GenerationApplicationStatus::Undone
                            if entry.validation_rejection.is_none() =>
                        {
                            entry.validation_rejection = None;
                            entry.outcome = "undone (do not repeat)".into()
                        }
                        desegnos_core::GenerationApplicationStatus::Applied
                            if entry.outcome.starts_with("undone") =>
                        {
                            entry.validation_rejection = None;
                            entry.outcome = "applied".into()
                        }
                        _ => {}
                    }
                }
            }
            entries
        };
        entries.retain(|entry| entry.project_id == project.id);
        // Undo/redo lives in the durable project dossier. Capture it before a newer job replaces it.
        if let Some(dossier) = &project.latest_generation {
            for entry in entries.iter_mut().filter(|r| r.job_id == dossier.job_id) {
                if entry.summary == "No AI summary recorded." {
                    entry.summary = dossier
                        .summary
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(200)
                        .collect();
                }
                match dossier.application_status {
                    desegnos_core::GenerationApplicationStatus::Undone
                        if entry.validation_rejection.is_none() =>
                    {
                        entry.validation_rejection = None;
                        entry.outcome = "undone (do not repeat)".into()
                    }
                    desegnos_core::GenerationApplicationStatus::Applied
                        if !entry.outcome.starts_with("needs details")
                            && entry.validation_rejection.is_none() =>
                    {
                        entry.validation_rejection = None;
                        entry.outcome = "applied".into()
                    }
                    _ => {}
                }
            }
        }
        private_directory(path.parent().ok_or("Missing history directory.")?)?;
        desegnos_core::compact_request_history(&mut entries);
        let index = RequestHistoryIndex {
            format: desegnos_core::CURRENT_REQUIREMENTS_FORMAT,
            entries,
        };
        write_json(&path, &index)?;
        Ok(index.entries)
    }

    fn past_jobs(&self, project_id: Uuid) -> NativeResult<GenerationHistory> {
        let directory = self.directory.join(project_id.to_string());
        if !directory.exists() {
            return Ok(GenerationHistory {
                jobs: vec![],
                skipped: 0,
                truncated: false,
            });
        }
        regular_directory(&directory)?;
        let mut jobs = Vec::new();
        let mut skipped = 0;
        let mut count = 0;
        for entry in fs::read_dir(&directory).map_err(message)? {
            let entry = entry.map_err(message)?;
            let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|name| Uuid::parse_str(name).ok())
            else {
                continue;
            };
            count += 1;
            if count > 4096 {
                break;
            }
            match self.saved_job(project_id, id) {
                Ok(Some(view)) => {
                    let first_request = frozen_first_request(&view);
                    let created_at = view
                        .created_at
                        .or_else(|| {
                            view.attempts
                                .first()
                                .map(|attempt| attempt.started_at.clone())
                        })
                        .or_else(|| {
                            fs::metadata(entry.path().join("job.json"))
                                .ok()?
                                .modified()
                                .ok()
                                .and_then(|time| OffsetDateTime::from(time).format(&Rfc3339).ok())
                        });
                    jobs.push(GenerationJobSummary {
                        job_id: id,
                        created_at,
                        status: view.status,
                        usage: view.usage,
                        first_request,
                        summary: view
                            .candidate
                            .map_or(view.message, |candidate| candidate.summary),
                    });
                }
                // Another requirements format is absent, not a record that could not be read.
                Ok(None) => {}
                Err(_) => skipped += 1,
            }
        }
        jobs.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.job_id.cmp(&a.job_id))
        });
        let truncated = count > 4096 || jobs.len() > 100;
        jobs.truncate(100);
        Ok(GenerationHistory {
            jobs,
            skipped,
            truncated,
        })
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
        let stored = if let Some(record) = self.jobs.get(&latest.job_id) {
            Some(record.clone())
        } else {
            read_job_record(&self.job_directory(project.id, latest.job_id))?
        };
        let Some(mut record) = stored else {
            return Ok(None);
        };
        if record.version != 1
            || record.view.requirements.format_version().is_err()
            || record.view.project_id != project.id
            || record.view.job_id != latest.job_id
            || record.view.requirements.project_id != project.id
            || record.view.requirements.job_id != latest.job_id
            || record.view.requirements.sha256
                != digest(record.view.requirements.markdown.as_bytes())
            || record.view.areas.len() > desegnos_core::MAX_GENERATION_REQUESTS
            || record.view.areas.len() != record.view.requirements.request_count
            || record.view.areas.iter().any(|area| {
                !record
                    .view
                    .requirements
                    .dependencies
                    .contains_key(&format!("element:{}:{}", area.page_id, area.element_id))
            })
            || record
                .view
                .areas
                .iter()
                .map(|area| area.element_id)
                .collect::<BTreeSet<_>>()
                .len()
                != record.view.areas.len()
            || record.view.attempts.len() > MAX_ATTEMPTS
            || !record.view.estimate_usd.is_finite()
            || record.view.estimate_usd < 0.0
        {
            return Err("The generation record is invalid. The original was preserved; it was not submitted or applied.".into());
        }
        record.view.provider_error = record.error.clone();
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
            record.view.message = "Generation already applied.".into();
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
            for batch in &mut record.view.batches {
                if batch.status == JobStatus::Generating {
                    batch.status = JobStatus::Unknown;
                    batch.message = record.view.message.clone();
                }
            }
            return self.persist(record).map(Some);
        }
        if record.view.status == JobStatus::Ready
            && let Err(error) = resolve_current(&record.view, project)
        {
            record.view.status = JobStatus::Blocked;
            record.view.message = format!(
                "This candidate is stale: {error} Undo the changed requirements to reuse this candidate, or discard it and generate from the current requirements."
            );
            return self.persist(record).map(Some);
        }
        // A partially failed batched job keeps its finished parts for Try again; it must not
        // become applyable before the failed parts are retried.
        let batches_complete = record.view.batches.is_empty()
            || record
                .view
                .batches
                .iter()
                .all(|batch| batch.status == JobStatus::Ready);
        if record.view.status == JobStatus::Blocked
            && batches_complete
            && record
                .view
                .candidate
                .as_ref()
                .is_some_and(|candidate| candidate.can_apply(record.view.request_requirements()))
            && resolve_current(&record.view, project).is_ok()
        {
            record.view.status = JobStatus::Ready;
            record.view.message = "Result ready to apply.".into();
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

    fn prepare_requests(
        &mut self,
        project: &Project,
        account_id: Option<Uuid>,
        previous: Option<&GenerationJobView>,
        element_ids: Option<&[Uuid]>,
        reference_ids: Option<&[Uuid]>,
    ) -> NativeResult<GenerationJobView> {
        let provider = previous.map_or(self.provider, |v| v.provider);
        let authentication = previous.map_or(self.authentication, |v| v.authentication);
        let settings = previous.map_or_else(|| self.settings.clone(), |v| v.settings.clone());
        if let Some(error) = &self.configuration_error {
            return Err(error.clone());
        }
        if let Some(error) = &self.authentication_error {
            return Err(error.clone());
        }
        if (authentication == AuthenticationMethod::Chatgpt) != account_id.is_some()
            || account_id.is_some_and(|id| id.is_nil())
        {
            return Err("Choose a connected ChatGPT account with plan usage enabled before preparing this request.".into());
        }
        self.client.as_ref().map_err(Clone::clone)?;
        if self.latest(project)?.is_some_and(|view| {
            view.status.active() && previous.is_none_or(|old| old.job_id != view.job_id)
        }) {
            return Err("Apply or discard the existing generation review, or cancel its running request, before starting another.".into());
        }
        let capabilities = provider.capabilities_for(
            authentication,
            &settings,
            self.selected_image_cache_id(account_id)
                .as_ref()
                .and_then(|id| self.image_access.get(id)),
        );
        let history = self.request_history(project)?;
        let retry_targets: Vec<_> = previous
            .map(|view| view.request_requirements().target_ids())
            .transpose()
            .map_err(message)?
            .unwrap_or_default()
            .into_iter()
            .map(|(_, id)| id)
            .collect();
        let reference_ids = reference_ids.unwrap_or(&[]);
        let requirements = if let Some(ids) = element_ids {
            project.prepare_targeted_requirements(capabilities, ids, reference_ids)
        } else if previous.is_some() {
            project.prepare_targeted_requirements(capabilities, &retry_targets, reference_ids)
        } else {
            project.prepare_requirements_with_capabilities(capabilities, reference_ids)
        }
        .and_then(|snapshot| {
            snapshot.with_earlier_requests(
                project,
                &history,
                OffsetDateTime::now_utc().unix_timestamp(),
            )
        })
        .map_err(message)?;
        let requirements_targets: BTreeSet<_> = requirements
            .target_ids()
            .map_err(message)?
            .into_iter()
            .map(|(_, element_id)| element_id)
            .collect();
        let estimate_usd = self
            .adapter(provider)?
            .estimate(
                authentication,
                &requirements,
                project.generation_mode,
                &desegnos_core::result_schema(&requirements),
                &settings,
            )
            .map_err(message)?
            .estimate_usd;
        let message = if authentication == AuthenticationMethod::Chatgpt {
            "Sending uses this ChatGPT account's plan allowance or credits.".into()
        } else {
            "Ready to send.".into()
        };
        let record = JobRecord {
            version: 1,
            error: None,
            view: GenerationJobView {
                job_id: requirements.job_id,
                project_id: project.id,
                base_revision: project.revision,
                source_fingerprints: BTreeMap::new(),
                applied_source_fingerprints: BTreeMap::new(),
                requirements,
                batches: vec![],
                merged_requirements: None,
                context: None,
                draft_feedback: vec![],
                earlier_contexts: vec![],
                validation_events: vec![],
                validation_counts_recorded: false,
                repair_rounds: 0,
                first_exchange: None,
                repair_area_count: 0,
                raw_reply: None,
                created_at: Some(now()),
                areas: {
                    let mut rows = Vec::new();
                    for page in &project.pages {
                        collect_generation_targets(
                            page.id,
                            &page.elements,
                            &requirements_targets,
                            &mut rows,
                        );
                    }
                    rows
                },
                generation_mode: project.generation_mode,
                provider,
                settings,
                authentication,
                chatgpt_account_id: account_id,
                failure_kind: None,
                provider_error: None,
                status: JobStatus::Prepared,
                estimate_usd,
                received_bytes: 0,
                message,
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
        // A user submission after an edit gets a new immutable packet. Keep the old
        // job and its attempts intact; only unchanged retries reuse the frozen job.
        if record.view.base_revision != project.revision
            || record.view.requirements.validate_current(project).is_err()
        {
            let fresh = self.prepare_requests(
                project,
                record.view.chatgpt_account_id,
                Some(&record.view),
                None,
                Some(&record.view.requirements.reference_ids),
            )?;
            return self.begin(project, fresh.job_id, false);
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
                &desegnos_core::result_schema(&record.view.requirements),
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
        let directory = self.job_directory(project.id, job_id);
        if let Some(context) = &record.view.context {
            write_markdown(
                &directory.join(format!("attempt-{}-CONTEXT.md", record.view.attempts.len())),
                &context.markdown,
            )
            .map_err(message)?;
            let path = directory.join("CONTEXT.md");
            if path.exists() {
                regular_file(&path)?;
                fs::remove_file(path).map_err(message)?;
            }
        }
        if let Some(reply) = &record.view.raw_reply {
            write_markdown(
                &directory.join(format!("attempt-{}-reply.txt", record.view.attempts.len())),
                reply,
            )
            .map_err(message)?;
        }
        record.view.context = None;
        record.view.earlier_contexts.clear();
        record.view.validation_events.clear();
        record.view.validation_counts_recorded = false;
        record.view.first_exchange = None;
        record.view.repair_area_count = 0;
        let reply_path = directory.join("REPLY.txt");
        if reply_path.exists() {
            regular_file(&reply_path)?;
            fs::remove_file(reply_path).map_err(message)?;
        }
        record.view.raw_reply = None;
        record.view.status = JobStatus::Generating;
        record.view.candidate = None;
        record.view.usage = None;
        record.view.received_bytes = 0;
        record.view.failure_kind = None;
        record.view.provider_error = None;
        record.view.message = "Generating…".into();
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

    fn progress(&mut self, job_id: Uuid, ordinal: usize, progress: desegnos_ai::ProviderProgress) {
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
        _current: Option<&Project>,
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
            let latest = record
                .view
                .attempts
                .last()
                .is_some_and(|attempt| attempt.ordinal == ordinal);
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
                    if latest {
                        record.view.usage = usage;
                    }
                }
                attempt.finished_at.get_or_insert_with(now);
                if latest {
                    self.record_validation_counts(&mut record);
                }
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
                self.record_validation_counts(&mut record);
                for batch in &mut record.view.batches {
                    if batch.status == JobStatus::Generating {
                        batch.status = record.view.status;
                        batch.message = record.view.message.clone();
                    }
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
                #[cfg(feature = "native-tests")]
                capture_benchmark_output(
                    &self.job_directory(original.id, job_id),
                    ordinal,
                    &response.text,
                    std::env::var("DESEGNOS_GENERATION_BENCHMARK").as_deref() == Ok("1"),
                )?;
                match GenerationCandidate::from_json(&response.text).and_then(|candidate| {
                    record.view.requirements.format_version()?;
                    candidate.validate(&record.view.requirements, original)?;
                    Ok(candidate)
                }) {
                    Ok(candidate) => {
                        let ready = candidate.can_apply(&record.view.requirements);
                        record.view.status = if ready {
                            JobStatus::Ready
                        } else {
                            JobStatus::Blocked
                        };
                        record.view.message = if ready {
                            "Result ready to apply.".into()
                        } else {
                            "Some requests need a choice.".into()
                        };
                        record.view.candidate = Some(candidate);
                    }
                    Err(error) => {
                        record.view.validation_events.push(desegnos_core::ValidationEvent {
                            kind: desegnos_core::validation_failure_kind(&error.to_string()).into(),
                            disposition: "conflicted".into(),
                            element_id: None,
                            message: error.to_string(),
                        });
                        record.view.raw_reply = Some(desegnos_ai::scrub_diagnostic_reply(&response.text, None));
                        record.view.status = JobStatus::Blocked;
                        record.view.message = format!(
                            "Desegnos rejected the candidate: {error} Nothing was applied."
                        );
                    }
                }
            }
            Err(mut error) => {
                record.view.raw_reply = error
                    .response_format
                    .as_mut()
                    .and_then(|format| format.diagnostic_reply.take())
                    .map(String::from);
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
        self.record_validation_counts(&mut record);
        if let Some(attempt) = record.view.attempts.last_mut() {
            attempt.status = record.view.status;
            attempt.finished_at = Some(now());
        }
        let _ = self.persist_retaining(record);
        self.active.remove(&job_id);
        Ok(())
    }

    fn record_validation_counts(&self, record: &mut JobRecord) {
        if record.view.validation_counts_recorded || record.view.validation_events.is_empty() {
            return;
        }
        let counter_path = self.directory.join("validation-counts.json");
        let mut counts: BTreeMap<String, u64> = if counter_path.exists() {
            match read_json(&counter_path, 64 * 1024) {
                Ok(counts) => counts,
                Err(error) => {
                    record
                        .view
                        .validation_events
                        .push(desegnos_core::ValidationEvent {
                            kind: "telemetryStorage".into(),
                            disposition: "conflicted".into(),
                            element_id: None,
                            message: format!(
                                "Kept unreadable local validation counts unchanged: {error}"
                            ),
                        });
                    return;
                }
            }
        } else {
            BTreeMap::new()
        };
        for event in &record.view.validation_events {
            let count = counts
                .entry(format!("{}:{}", event.kind, event.disposition))
                .or_default();
            *count = count.saturating_add(1);
        }
        if let Err(error) = write_json(&counter_path, &counts) {
            record
                .view
                .validation_events
                .push(desegnos_core::ValidationEvent {
                    kind: "telemetryStorage".into(),
                    disposition: "conflicted".into(),
                    element_id: None,
                    message: format!("Could not save local validation counts: {error}"),
                });
        }
        record.view.validation_counts_recorded = true;
    }

    fn handle_finish_error(&mut self, job_id: Uuid, ordinal: usize, error: &str) {
        if self
            .active
            .get(&job_id)
            .is_none_or(|(active, _)| *active != ordinal)
        {
            return;
        }
        if let Some(record) = self.jobs.get_mut(&job_id)
            && record.view.status != JobStatus::Cancelled
        {
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
            JobStatus::Prepared
                | JobStatus::Generating
                | JobStatus::Ready
                | JobStatus::Blocked
                | JobStatus::Failed
                | JobStatus::Unknown
                | JobStatus::Cancelled
        ) {
            return Err("This request is no longer running. Review or discard its result.".into());
        }
        record.view.status = JobStatus::Cancelled;
        record.view.candidate = None;
        record.view.message = if record.view.authentication == AuthenticationMethod::Chatgpt {
            "Stopped. A submitted request may still consume ChatGPT plan allowance or credits. Late output cannot be applied.".into()
        } else {
            "Stopped. A submitted request may still incur a charge. Late output cannot be applied."
                .into()
        };
        if let Some(attempt) = record.view.attempts.last_mut() {
            attempt.status = JobStatus::Cancelled;
            attempt.finished_at = Some(now());
        }
        for batch in &mut record.view.batches {
            if batch.status == JobStatus::Generating {
                batch.status = JobStatus::Cancelled;
                batch.message = record.view.message.clone();
            }
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
            for batch in &mut record.view.batches {
                if batch.status == JobStatus::Generating {
                    batch.status = JobStatus::Cancelled;
                    batch.message = record.view.message.clone();
                }
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

async fn credential_key(state: &AiState<'_>, provider: ProviderId) -> NativeResult<ApiKey> {
    let credentials = state.lock().map_err(message)?.credentials_for(provider);
    tauri::async_runtime::spawn_blocking(move || credentials.key())
        .await
        .map_err(message)?
        .map_err(message)
}

/// No lock is held across network I/O. The cache key binds results to the captured credential/account.
pub(super) async fn refresh_image_access(
    state: &Mutex<GenerationService>,
    chatgpt: &ChatgptState,
    force: bool,
) -> NativeResult<()> {
    refresh_main_image_access(state, chatgpt, force).await?;
    let probe = {
        let mut service = state.lock().map_err(message)?;
        if service.provider != ProviderId::Openai {
            return Ok(());
        }
        let Ok(key) = service.image_credentials.key() else {
            return Ok(());
        };
        let id = key.image_access_cache_key();
        if service.image_access_checking.contains_key(&id)
            || (!force
                && service
                    .image_access
                    .get(&id)
                    .is_some_and(|a| !a.expired(desegnos_ai::image_access_now())))
        {
            return Ok(());
        }
        let client = service.client.clone()?;
        let probe_id = Uuid::new_v4();
        service.image_access_checking.insert(id.clone(), probe_id);
        let prior_access = service.image_access.get(&id).cloned();
        (
            id,
            key,
            client,
            service.settings.clone(),
            probe_id,
            prior_access,
        )
    };
    let (id, key, client, settings, probe_id, prior_access) = probe;
    let access = client
        .probe_image_access(AuthenticationMethod::ApiKey, &settings, &key)
        .await;
    let mut service = state.lock().map_err(message)?;
    if service.image_access_checking.get(&id) == Some(&probe_id) {
        service.image_access_checking.remove(&id);
        if service.image_key_cache_id().as_ref() == Some(&id)
            && service.image_access.get(&id) == prior_access.as_ref()
        {
            service.store_image_access(id, access)?;
        }
    }
    Ok(())
}

async fn refresh_main_image_access(
    state: &Mutex<GenerationService>,
    chatgpt: &ChatgptState,
    force: bool,
) -> NativeResult<()> {
    let view = chatgpt.view().await;
    if view.sign_in.status == desegnos_ai::ChatgptSignInStatus::AwaitingBrowser {
        return Ok(());
    }
    let (id, auth, settings, client, credentials, prior_access) = {
        let service = state.lock().map_err(message)?;
        let Some(id) = service.image_cache_id(view.active_account_id) else {
            return Ok(());
        };
        if service.image_access_checking.contains_key(&id)
            || (!force
                && service
                    .image_access
                    .get(&id)
                    .is_some_and(|access| !access.expired(desegnos_ai::image_access_now())))
        {
            return Ok(());
        }
        if service.authentication == AuthenticationMethod::Chatgpt
            && !view.accounts.iter().any(|account| {
                Some(account.id) == view.active_account_id
                    && account.connected
                    && account.plan_enabled
                    && !account.storage_repair_required
            })
        {
            return Ok(());
        }
        (
            id.clone(),
            service.authentication,
            service.settings.clone(),
            service.client.clone()?,
            service.credentials.clone(),
            service.image_access.get(&id).cloned(),
        )
    };
    let key = if auth == AuthenticationMethod::Chatgpt {
        let Some(account) = view.active_account_id else {
            return Ok(());
        };
        match chatgpt
            .manager()?
            .bearer(account, &CancellationToken::new())
            .await
        {
            Ok(key) => key,
            Err(_) => return Ok(()),
        }
    } else {
        match tauri::async_runtime::spawn_blocking(move || credentials.key())
            .await
            .map_err(message)?
        {
            Ok(key) => key,
            Err(_) => return Ok(()),
        }
    };
    if auth == AuthenticationMethod::ApiKey && key.image_access_cache_key() != id {
        return Ok(());
    }
    let probe_id = Uuid::new_v4();
    {
        let mut service = state.lock().map_err(message)?;
        if service.image_access_checking.contains_key(&id) {
            return Ok(());
        }
        service.image_access_checking.insert(id.clone(), probe_id);
    }
    let access = client.probe_image_access(auth, &settings, &key).await;
    let mut service = state.lock().map_err(message)?;
    if service.image_access_checking.get(&id) != Some(&probe_id) {
        return Ok(());
    }
    service.image_access_checking.remove(&id);
    // A runtime rejection or reconnect during this probe takes precedence over its late answer.
    if service.image_access.get(&id) != prior_access.as_ref() {
        return Ok(());
    }
    service.store_image_access(id, access)
}

pub(super) async fn settings_view(
    state: &AiState<'_>,
    chatgpt_state: &ChatgptState,
) -> NativeResult<AiSettingsView> {
    refresh_image_access(state, chatgpt_state, false).await?;
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
    let (
        image_access,
        image_access_checking,
        image_credential,
        main_image_access,
        image_key_access,
        image_key,
    ) = {
        let service = state.lock().map_err(message)?;
        let main = service.image_cache_id(chatgpt.active_account_id);
        let image = service.image_key_cache_id();
        let id = service.selected_image_cache_id(chatgpt.active_account_id);
        let credential = id
            .as_ref()
            .filter(|id| {
                service.image_access.get(*id).is_some_and(|a| {
                    a.available == desegnos_ai::ImageAvailability::Yes
                        && !a.expired(desegnos_ai::image_access_now())
                })
            })
            .map(|id| {
                if main.as_ref() == Some(id) {
                    if authentication == AuthenticationMethod::Chatgpt {
                        "chatgpt"
                    } else {
                        "connection"
                    }
                } else {
                    "imageKey"
                }
                .to_owned()
            });
        (
            id.as_ref()
                .and_then(|id| service.image_access.get(id))
                .cloned(),
            id.as_ref()
                .is_some_and(|id| service.image_access_checking.contains_key(id))
                || image
                    .as_ref()
                    .is_some_and(|id| service.image_access_checking.contains_key(id)),
            credential,
            main.as_ref()
                .and_then(|id| service.image_access.get(id))
                .cloned(),
            image
                .as_ref()
                .and_then(|id| service.image_access.get(id))
                .cloned(),
            service.image_credentials.status(),
        )
    };
    Ok(AiSettingsView {
        image_credential,
        image_key,
        main_image_access,
        image_key_access,
        image_access,
        image_access_checking,
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
    image: Option<bool>,
    key: String,
    storage: CredentialStorage,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    let credentials = {
        let service = state.lock().map_err(message)?;
        if image.unwrap_or(false) {
            service.image_credentials.clone()
        } else {
            service.credentials.clone()
        }
    };
    tauri::async_runtime::spawn_blocking(move || credentials.set_key(key, storage))
        .await
        .map_err(message)?
        .map_err(message)?;
    refresh_image_access(&state, &chatgpt, true).await?;
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn remove_ai_key(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    image: Option<bool>,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    let credentials = {
        let service = state.lock().map_err(message)?;
        if image.unwrap_or(false) {
            service.image_credentials.clone()
        } else {
            service.credentials.clone()
        }
    };
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
    refresh_image_access(&state, &chatgpt, true).await?;
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
        return Ok(AccessResult {
            message: "Connected · ChatGPT plan and model access confirmed.".into(),
        });
    }
    let key = credential_key(&state, provider).await?;
    ProviderClient::new(provider, client)
        .map_err(message)?
        .test_model_access(&settings, &key, &CancellationToken::new())
        .await
        .map_err(message)?;
    Ok(AccessResult {
        message: "Connected · Model access confirmed.".into(),
    })
}

#[tauri::command]
pub async fn prepare_generation(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    element_ids: Option<Vec<Uuid>>,
    reference_ids: Option<Vec<Uuid>>,
) -> NativeResult<GenerationJobView> {
    trusted(&window)?;
    refresh_image_access(&state, &chatgpt, false).await?;
    let (authentication, provider) = {
        let service = state.lock().map_err(message)?;
        (service.authentication, service.provider)
    };
    let account_id = if authentication == AuthenticationMethod::Chatgpt {
        let view = chatgpt.view().await;
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
        Some(account_id)
    } else {
        None
    };
    let workspace = workspace.lock().map_err(message)?;
    let project = workspace.project()?;
    let mut service = state.lock().map_err(message)?;
    if service.authentication != authentication || service.provider != provider {
        return Err("The generation method changed while preparing this request. Review it and prepare again.".into());
    }
    let references = reference_ids.as_deref();
    if let Some(ids) = element_ids {
        service.prepare_requests(project, account_id, None, Some(&ids), references)
    } else {
        service.prepare_requests(project, account_id, None, None, references)
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

#[tauri::command]
pub fn list_generations(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    project_id: Uuid,
) -> NativeResult<GenerationHistory> {
    trusted(&window)?;
    if workspace.lock().map_err(message)?.project()?.id != project_id {
        return Err("The active project changed.".into());
    }
    state.lock().map_err(message)?.past_jobs(project_id)
}

#[tauri::command]
pub fn get_past_generation(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    project_id: Uuid,
    job_id: Uuid,
) -> NativeResult<GenerationJobView> {
    trusted(&window)?;
    if workspace.lock().map_err(message)?.project()?.id != project_id {
        return Err("The active project changed.".into());
    }
    state
        .lock()
        .map_err(message)?
        .saved_job(project_id, job_id)?
        .ok_or_else(|| "This saved generation record is no longer available.".into())
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
        .request_requirements()
        .validate_current(workspace.project()?)
        .is_err()
    {
        return Ok(view);
    }
    let recommended = workspace
        .project()?
        .compatible_decision_proposals(view.request_requirements(), candidate)
        .map_err(message)?;
    let error = workspace
        .record_generated(view.request_requirements(), candidate, view.attempts.len())
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

fn frozen_first_request(view: &GenerationJobView) -> String {
    let mut fence = None;
    let mut block = String::new();
    for line in view.requirements.markdown.lines() {
        if let Some(end) = &fence {
            if line == end {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&block)
                    && value.get("elementId").is_some()
                    && let Some(prompt) = value.get("request").and_then(|value| value.as_str())
                {
                    return prompt.into();
                }
                fence = None;
            } else {
                block.push_str(line);
                block.push('\n');
            }
        } else if let Some(start) = line.strip_suffix("json")
            && start.len() >= 3
            && start.bytes().all(|byte| byte == b'`')
        {
            fence = Some(start.to_owned());
            block.clear();
        }
    }
    String::new()
}

#[allow(clippy::too_many_arguments)]
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
    let job_id = view.job_id;
    let request = view.clone();
    tauri::async_runtime::spawn(async move {
            let ordinal = request.attempts.len();
            let result = async {
            // Keep preparation local. All credential refresh/model discovery belongs to
            // the registered, cancellable attempt, before any paid generation submission.
            let credentials = async {
                if authentication == AuthenticationMethod::Chatgpt {
                    let account_id = account_id.ok_or("This ChatGPT request has no bound account. Prepare a new request.")?;
                    let chatgpt = app.state::<ChatgptState>();
                    let manager = chatgpt.manager()?;
                    let mut available = chatgpt.view().await;
                    if available.models.is_empty() {
                        let client = app.state::<Mutex<GenerationService>>().lock().map_err(message)?.client.clone()?;
                        available = manager.discover_models(account_id, &client, &cancellation).await.map_err(message)?;
                    }
                    if available.active_account_id != Some(account_id)
                        || !available.models.iter().any(|model| model.id == request.settings.model)
                    {
                        return Err("Test ChatGPT access and choose an available verified model before generating.".into());
                    }
                    manager.bearer(account_id, &cancellation).await.map_err(message)
                } else {
                    credential_key(&app.state::<Mutex<GenerationService>>(), provider).await
                }
            };
            let key = cancellation.run_until_cancelled(credentials).await
                .ok_or_else(|| ProviderError::new(ErrorKind::Cancelled, SubmissionOutcome::NotSubmitted))?
                .map_err(|_| ProviderError::new(ErrorKind::MissingCredential, SubmissionOutcome::NotSubmitted))?;

            let (shared_app, request_ref, key_ref, cancel_ref, client_ref) =
                (&app, &request, &key, &cancellation, &client);
            let ask = |requirements: &RequirementsSnapshot| {
                let progress_app = shared_app.clone();
                let requirements = requirements.clone();
                async move {
                    let schema = desegnos_core::result_schema(&requirements);
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
            // A completed, unambiguous final answer with malformed JSON/schema is a
            // validation failure, not a transport retry. All other provider failures stop here.
            if let Err(error) = &result && error.outcome == SubmissionOutcome::Received && error.response_format.as_ref().is_some_and(|format|
                    matches!(format.rejection, Some(desegnos_ai::ResponseRejection::CandidateJson | desegnos_ai::ResponseRejection::CandidateShape))) && let Some(text) = error.response_format.as_ref().and_then(|format| format.diagnostic_reply.as_ref()) {
                result = Ok(ProviderResponse { text: text.to_string(), response_id: error.response_id.clone().unwrap_or_default(), usage: error.usage });
            }
            result
        }.await;
            let workspace_state = app.state::<Mutex<Workspace>>();
            let service_state = app.state::<Mutex<GenerationService>>();
            if let Ok(mut workspace) = workspace_state.lock()
                && let Ok(mut service) = service_state.lock()
            {
                let current = workspace.project().ok().cloned();
                if let Err(error) =
                    service.finish(&original, current.as_ref(), job_id, ordinal, result)
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
    record.view.message = "Result discarded.".into();
    service.persist(record)
}

#[tauri::command]
pub async fn apply_generation(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    job_id: Uuid,
    accepted_decision_indices: Vec<usize>,
) -> NativeResult<WorkspaceView> {
    trusted(&window)?;
    let prepared = {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?;
        if project.applied_job_ids.contains(&job_id) {
            return Ok(workspace.view());
        }
        let mut service = state.lock().map_err(message)?;
        let record = service.record(project, job_id)?;
        if record.view.status != JobStatus::Ready {
            return Err("Only a current candidate with a ready reply can be applied.".into());
        }
        let candidate = record
            .view
            .candidate
            .clone()
            .ok_or("The candidate is unavailable.")?;
        let snapshot = record.view.request_requirements().clone();
        snapshot.validate_current(project).map_err(message)?;
        let images_enabled = snapshot
            .capabilities
            .as_ref()
            .and_then(|capabilities| capabilities.image_generation.as_ref())
            .is_some_and(|images| !images.sizes.is_empty());
        let image_requests: Vec<(String, String)> = if images_enabled {
            candidate
                .images
                .iter()
                .map(|image| (image.key.clone(), image.prompt.clone()))
                .collect()
        } else {
            vec![]
        };
        let client = if image_requests.is_empty() {
            None
        } else {
            service.client.clone().ok()
        };
        let authentication = record.view.authentication;
        let provider = record.view.provider;
        let account_id = record.view.chatgpt_account_id;
        let image_key = if image_requests.is_empty() || provider != ProviderId::Openai {
            None
        } else {
            service.image_credentials.key().ok()
        };
        let main_key = if image_requests.is_empty() || provider != ProviderId::Openai {
            None
        } else if authentication == AuthenticationMethod::ApiKey {
            service.credentials.key().ok()
        } else {
            None
        };
        (
            snapshot,
            candidate,
            image_requests,
            client,
            authentication,
            account_id,
            image_key,
            main_key,
        )
    };
    let (
        snapshot,
        candidate,
        image_requests,
        client,
        authentication,
        account_id,
        image_key,
        main_key,
    ) = prepared;
    let generated = if image_requests.is_empty() {
        BTreeMap::new()
    } else {
        let key = if let Some(key) = image_key.or(main_key) {
            Some(key)
        } else if authentication == AuthenticationMethod::Chatgpt {
            match account_id {
                Some(account) => Some(
                    chatgpt
                        .manager()?
                        .bearer(account, &CancellationToken::new())
                        .await
                        .map_err(message)?,
                ),
                None => None,
            }
        } else {
            None
        };
        match (client, key) {
            (Some(client), Some(key)) => {
                client
                    .generate_requested_images(
                        &image_requests,
                        &key,
                        &CancellationToken::new(),
                    )
                    .await
            }
            _ => BTreeMap::new(),
        }
    };
    let mut workspace = workspace.lock().map_err(message)?;
    let mut service = state.lock().map_err(message)?;
    let record = service.record(workspace.project()?, job_id)?;
    if record.view.status != JobStatus::Ready {
        return Err("Only a current candidate with a ready reply can be applied.".into());
    }
    workspace
        .apply_generated(
            &snapshot,
            &candidate,
            &accepted_decision_indices,
            &generated,
        )
        .map_err(message)?;
    let mut record = service.record(workspace.project()?, job_id)?;
    record.view.status = JobStatus::Applied;
    record.view.message = "Layers updated.".into();
    record.view.accepted_decision_indices = accepted_decision_indices;
    service.persist(record)?;
    Ok(workspace.view())
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

impl GenerationService {
    pub(super) fn identity_settings(&self) -> (ProviderId, AuthenticationMethod, ProviderSettings) {
        (self.provider, self.authentication, self.settings.clone())
    }
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
