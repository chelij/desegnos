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
// At most 20 attempts, two bounded 8 MiB replies each, plus reconstructed page source.
const MAX_JOB_BYTES: u64 = 768 * 1024 * 1024;
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

impl Default for JobStatus {
    fn default() -> Self {
        Self::Prepared
    }
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
    #[serde(default, skip_serializing_if = "desegni_core::RegionShape::is_rect")]
    shape: desegni_core::RegionShape,
    /// Set when this request edits a document shown inside one of the page's iframes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    document: Option<desegni_core::DocumentTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frame_rect: Option<desegni_core::Rect>,
}

/// One sequential part of a request whose editable documents exceed the source budget.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GenerationBatch {
    part: usize,
    requirements: RequirementsSnapshot,
    #[serde(default)]
    estimate_usd: f64,
    #[serde(default)]
    candidate: Option<GenerationCandidate>,
    #[serde(default)]
    status: JobStatus,
    #[serde(default)]
    message: String,
    #[serde(default)]
    validation_events: Vec<desegni_core::ValidationEvent>,
    #[serde(default)]
    documents: Vec<desegni_core::DocumentTarget>,
    #[serde(default)]
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
    #[serde(default)]
    source_fingerprints: BTreeMap<Uuid, BTreeMap<String, String>>,
    #[serde(default)]
    applied_source_fingerprints: BTreeMap<Uuid, BTreeMap<String, String>>,
    #[serde(default)]
    context: Option<RequirementsSnapshot>,
    #[serde(default)]
    draft_feedback: Vec<desegni_core::ValidationEvent>,
    #[serde(default)]
    earlier_contexts: Vec<RequirementsSnapshot>,
    #[serde(default)]
    raw_reply: Option<String>,
    #[serde(default)]
    validation_events: Vec<desegni_core::ValidationEvent>,
    #[serde(default)]
    drafts: Vec<desegni_core::GenerationDraft>,
    #[serde(default)]
    validation_counts_recorded: bool,
    #[serde(default)]
    repair_rounds: usize,
    #[serde(default)]
    first_exchange: Option<ValidationExchange>,
    #[serde(default)]
    repair_area_count: usize,
    #[serde(default)]
    created_at: Option<String>,
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

impl GenerationJobView {
    /// The snapshot that validates and applies the complete result. A batched job uses the
    /// merged document set; a single request uses its only snapshot.
    fn request_requirements(&self) -> &RequirementsSnapshot {
        self.merged_requirements
            .as_ref()
            .unwrap_or(&self.requirements)
    }

    /// What earlier parts already changed, so the next part stays consistent. Each finished
    /// part contributes its summary and the shared CSS it produced; a failed part is named so
    /// later parts do not silently repeat it.
    fn batch_earlier(&self, index: usize) -> String {
        let total = self.batches.len();
        let mut text = String::new();
        for batch in self.batches.iter().take(index) {
            match &batch.candidate {
                Some(candidate) => {
                    text.push_str(&format!(
                        "Part {} of {total} finished: {}\n",
                        batch.part,
                        candidate.summary.trim()
                    ));
                    let mut styles = Vec::new();
                    for page in &candidate.pages {
                        if !page.content.css.trim().is_empty() {
                            styles.push(page.content.css.as_str());
                        }
                    }
                    for document in &candidate.documents {
                        if !document.content.css.trim().is_empty() {
                            styles.push(document.content.css.as_str());
                        }
                    }
                    if !styles.is_empty() {
                        text.push_str(
                            "\nShared CSS after earlier parts. Keep later parts consistent with it:\n\n```css\n",
                        );
                        for css in styles {
                            text.push_str(&css.chars().take(8_000).collect::<String>());
                            text.push('\n');
                        }
                        text.push_str("```\n\n");
                    }
                }
                None => text.push_str(&format!(
                    "Part {} of {total} did not finish; do not repeat its work.\n",
                    batch.part
                )),
            }
            if text.chars().count() > 32_000 {
                text = text.chars().take(32_000).collect();
                break;
            }
        }
        text
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidationExchange {
    summary: String,
    regions: Vec<desegni_core::RegionOutcome>,
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
    main_image_access: Option<desegni_ai::ImageAccess>,
    image_key_access: Option<desegni_ai::ImageAccess>,
    image_access: Option<desegni_ai::ImageAccess>,
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
    image_access: BTreeMap<String, desegni_ai::ImageAccess>,
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
        if desegni_ai::use_image_key(
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
        access: desegni_ai::ImageAccess,
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
            if existing != primary.markdown.as_bytes() {
                return Err("The frozen requirements file changed on disk. The existing record was preserved.".into());
            }
        } else {
            write_markdown(&markdown, &primary.markdown).map_err(message)?;
        }
        if let Some(context) = &record.view.context {
            if context.markdown.len() > desegni_core::MAX_REQUIREMENTS_BYTES
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
                    > desegni_core::MAX_REQUIREMENTS_BYTES as u64
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
            let mut entries: Vec<desegni_core::EarlierRequest> =
                read_json(&history_path, MAX_JOB_BYTES)?;
            entries.retain(|r| {
                r.project_id == record.view.project_id && r.job_id != record.view.job_id
            });
            entries.extend(self.history_entries(&record.view));
            desegni_core::compact_request_history(&mut entries);
            write_json(&history_path, &entries)?;
        }
        let view = record.view.clone();
        self.unsaved_jobs.remove(&view.job_id);
        self.jobs.insert(view.job_id, record);
        self.trim_cache(view.job_id);
        Ok(view)
    }

    fn saved_job(&self, project_id: Uuid, job_id: Uuid) -> NativeResult<GenerationJobView> {
        let record = if let Some(record) = self.jobs.get(&job_id) {
            record.clone()
        } else {
            let directory = self.job_directory(project_id, job_id);
            regular_directory(&directory)?;
            read_json::<JobRecord>(&directory.join("job.json"), MAX_JOB_BYTES)?
        };
        let view = record.view;
        if record.version != 1
            || view.project_id != project_id
            || view.job_id != job_id
            || view.requirements.project_id != project_id
            || view.requirements.job_id != job_id
            || view.requirements.markdown.len() > desegni_core::MAX_REQUIREMENTS_BYTES
            || view.requirements.sha256 != digest(view.requirements.markdown.as_bytes())
        {
            return Err("The saved generation record is invalid.".into());
        }
        Ok(view)
    }

    // Compact text-only index. Full historical packets are read only for one-time backfill.
    fn history_path(&self, project_id: Uuid) -> PathBuf {
        self.directory
            .join(project_id.to_string())
            .join("request-history.json")
    }

    fn history_entries(&self, view: &GenerationJobView) -> Vec<desegni_core::EarlierRequest> {
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
        let mut entries = desegni_core::earlier_requests(
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
                .map_or(&[], |c| c.regions.as_slice()),
        );
        for entry in &mut entries {
            // Discarding a blocked result must not turn a validator rejection into user intent.
            // Only unresolved conflicts count: repaired events and provider errors do not.
            if view.candidate.as_ref().map_or(
                matches!(view.status, JobStatus::Blocked | JobStatus::Discarded)
                    && view.failure_kind.is_none(),
                |c| {
                    c.regions.iter().any(|r| {
                        r.region_id == entry.region_id
                            && r.status == desegni_core::RegionStatus::Conflict
                    })
                },
            ) && let Some(event) = view
                .validation_events
                .iter()
                .rev()
                .find(|e| e.disposition == "conflicted" && e.region_id == Some(entry.region_id))
                .or_else(|| {
                    view.validation_events.iter().rev().find(|e| {
                        e.disposition == "conflicted"
                            && e.region_id.is_none()
                            && !matches!(e.kind.as_str(), "repairProvider" | "repairStorage")
                    })
                })
            {
                desegni_core::record_validation_rejection(entry, &event.message);
            }
            if let Some(base) = view.source_fingerprints.get(&entry.page_id) {
                entry.base_source_fingerprints = Some(base.clone());
                entry.source_fingerprints = Some(base.clone());
            }
            if let Some(applied) = view.applied_source_fingerprints.get(&entry.page_id) {
                entry.source_fingerprints = Some(applied.clone());
            } else if view.status == JobStatus::Applied {
                // Older jobs predate stored post-apply fingerprints.
                entry.source_fingerprints = view
                    .candidate
                    .as_ref()
                    .and_then(|c| c.pages.iter().find(|p| p.page_id == entry.page_id))
                    .map(|p| desegni_core::source_fingerprints(Some(&p.content)));
            }
            // Every editable document this job covered, after any applied result.
            if view.status == JobStatus::Applied {
                let mut applied = entry.base_document_fingerprints.clone();
                if let Some(candidate) = &view.candidate {
                    for page in &candidate.pages {
                        if let Ok(hash) = page.content.sha256() {
                            applied.insert(
                                desegni_core::DocumentTarget::page(page.page_id).key(),
                                hash,
                            );
                        }
                    }
                    for document in &candidate.documents {
                        applied.insert(
                            document.document.key(),
                            digest(document.content.html.as_bytes()),
                        );
                    }
                }
                entry.document_fingerprints = applied;
            }
        }
        entries
    }

    pub(crate) fn request_history(
        &self,
        project: &Project,
    ) -> NativeResult<Vec<desegni_core::EarlierRequest>> {
        let path = self.history_path(project.id);
        let mut entries: Vec<desegni_core::EarlierRequest> = if path.exists() {
            read_json(&path, MAX_JOB_BYTES)?
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
                    if let Ok(view) = self.saved_job(project.id, id) {
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
                        desegni_core::GenerationApplicationStatus::Undone
                            if entry.validation_rejection.is_none() =>
                        {
                            entry.validation_rejection = None;
                            entry.outcome = "undone (do not repeat)".into()
                        }
                        desegni_core::GenerationApplicationStatus::Applied
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
        entries.retain(|r| r.project_id == project.id);
        let missing: BTreeSet<_> = entries
            .iter()
            .filter(|r| {
                r.source_fingerprints.is_none()
                    || (r.validation_rejection.is_none()
                        && (r.outcome == "rejected by user"
                            || r.outcome.starts_with("needs details")))
            })
            .map(|r| r.job_id)
            .collect();
        for id in missing {
            if let Ok(view) = self.saved_job(project.id, id) {
                let enriched = self.history_entries(&view);
                for entry in entries.iter_mut().filter(|r| r.job_id == id) {
                    if let Some(source) = enriched.iter().find(|r| r.region_id == entry.region_id) {
                        if source.validation_rejection.is_some()
                            || entry.outcome == "rejected by user"
                        {
                            entry.outcome = source.outcome.clone();
                            entry.validation_rejection = source.validation_rejection.clone();
                        }
                        entry.source_fingerprints = source.source_fingerprints.clone();
                        entry.base_source_fingerprints = source.base_source_fingerprints.clone();
                    }
                }
            }
        }
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
                    desegni_core::GenerationApplicationStatus::Undone
                        if entry.validation_rejection.is_none() =>
                    {
                        entry.validation_rejection = None;
                        entry.outcome = "undone (do not repeat)".into()
                    }
                    desegni_core::GenerationApplicationStatus::Applied
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
        desegni_core::compact_request_history(&mut entries);
        write_json(&path, &entries)?;
        Ok(entries)
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
                Ok(view) => {
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
                    || area.shape.validate().is_err()
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
                            shape: region.shape.clone(),
                            document: region
                                .frame_target
                                .as_ref()
                                .map(|frame| frame.document.clone()),
                            frame_rect: region.frame_target.as_ref().map(|frame| frame.rect),
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
            && let Err(error) = record.view.request_requirements().validate_current(project)
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
            && record
                .view
                .request_requirements()
                .validate_current(project)
                .is_ok()
            && record.view.candidate.as_ref().is_some_and(|candidate| {
                candidate
                    .validate(record.view.request_requirements(), project)
                    .is_ok()
            })
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

    fn prepare(&mut self, project: &Project) -> NativeResult<GenerationJobView> {
        self.prepare_bound(project, None)
    }

    fn prepare_bound(
        &mut self,
        project: &Project,
        account_id: Option<Uuid>,
    ) -> NativeResult<GenerationJobView> {
        self.prepare_targeted(project, account_id, None)
    }

    fn prepare_targeted(
        &mut self,
        project: &Project,
        account_id: Option<Uuid>,
        draft: Option<desegni_core::GenerationDraft>,
    ) -> NativeResult<GenerationJobView> {
        self.prepare_requests(project, account_id, draft, None, None)
    }

    fn prepare_requests(
        &mut self,
        project: &Project,
        account_id: Option<Uuid>,
        draft: Option<desegni_core::GenerationDraft>,
        previous: Option<&GenerationJobView>,
        region_ids: Option<&[Uuid]>,
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
        let mut requirements = if let Some(ids) = region_ids {
            project.prepare_targeted_requirements(capabilities, ids)
        } else if previous.is_some() {
            project.prepare_targeted_requirements(capabilities, &retry_targets)
        } else if let Some(draft) = &draft {
            project.prepare_targeted_requirements(capabilities, &draft.region_ids)
        } else {
            project.prepare_requirements_with_capabilities(capabilities)
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
            .map(|(_, region_id)| region_id)
            .collect();
        let draft_feedback: Vec<_> = draft
            .as_ref()
            .map(|draft| {
                draft
                    .region_ids
                    .iter()
                    .filter(|id| requirements_targets.contains(*id))
                    .map(|id| desegni_core::ValidationEvent {
                        kind: "draftRepair".into(),
                        disposition: "conflicted".into(),
                        region_id: Some(*id),
                        message: format!(
                            "Fix the previous draft for this request. Validation feedback: {}",
                            draft.reason
                        ),
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                previous.map_or_else(Vec::new, |view| {
                    view.draft_feedback
                        .iter()
                        .filter(|event| {
                            event
                                .region_id
                                .is_some_and(|id| requirements_targets.contains(&id))
                        })
                        .cloned()
                        .collect()
                })
            });
        let repair = requirements
            .validation_repair(&draft_feedback)
            .map_err(message)?;
        // Format 25: a selection beyond the source budget becomes sequential parts inside this
        // one job. Every part keeps the same provider settings, cancellation and review.
        let part_count = requirements.document_part_count(project).map_err(message)?;
        let mut batches = Vec::new();
        let mut merged_requirements = None;
        let estimate = if part_count > 1 {
            let merged = requirements.merged_documents(project).map_err(message)?;
            let mut total = 0.0;
            for part in 1..=part_count {
                let packet = requirements
                    .batched_part(project, part, "")
                    .map_err(message)?;
                let batch_estimate = self
                    .adapter(provider)?
                    .estimate(
                        authentication,
                        &packet,
                        project.generation_mode,
                        &desegni_core::result_schema(&packet),
                        &settings,
                    )
                    .map_err(message)?;
                total += batch_estimate.estimate_usd;
                batches.push(GenerationBatch {
                    part,
                    documents: packet
                        .documents
                        .iter()
                        .map(|item| item.document.clone())
                        .collect(),
                    requirements: packet,
                    estimate_usd: batch_estimate.estimate_usd,
                    candidate: None,
                    status: JobStatus::Prepared,
                    message: format!("Batch {part} of {part_count} is ready."),
                    validation_events: vec![],
                    repair_rounds: 0,
                });
            }
            if !spending_within_limit(authentication, total, settings.spending_limit_usd) {
                return Err(format!(
                    "This request needs {part_count} batches. Their combined estimate ${total:.2} exceeds the spending limit ${:.2}. Raise the limit or choose fewer pages.",
                    settings.spending_limit_usd
                ));
            }
            requirements = batches[0].requirements.clone();
            merged_requirements = Some(merged);
            Some(total)
        } else {
            Some(
                self.adapter(provider)?
                    .estimate(
                        authentication,
                        repair.as_ref().unwrap_or(&requirements),
                        project.generation_mode,
                        &desegni_core::result_schema(&requirements),
                        &settings,
                    )
                    .map_err(message)?
                    .estimate_usd,
            )
        };
        let estimate_usd = estimate.unwrap_or(0.0);
        let fingerprints = merged_requirements.as_ref().unwrap_or(&requirements);
        let message = if part_count > 1 {
            format!("Ready to send in {part_count} batches.")
        } else if authentication == AuthenticationMethod::Chatgpt {
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
                source_fingerprints: project
                    .pages
                    .iter()
                    .filter(|page| {
                        fingerprints
                            .dependencies
                            .contains_key(&format!("source:{}", page.id))
                    })
                    .map(|page| {
                        (
                            page.id,
                            desegni_core::source_fingerprints(page.content.as_ref()),
                        )
                    })
                    .collect(),
                applied_source_fingerprints: BTreeMap::new(),
                requirements,
                batches,
                merged_requirements,
                context: None,
                draft_feedback,
                earlier_contexts: vec![],
                validation_events: vec![],
                drafts: vec![],
                validation_counts_recorded: false,
                repair_rounds: 0,
                first_exchange: None,
                repair_area_count: 0,
                raw_reply: None,
                created_at: Some(now()),
                areas: project
                    .pages
                    .iter()
                    .flat_map(|page| {
                        page.regions
                            .iter()
                            .filter(|region| requirements_targets.contains(&region.id))
                            .map(move |region| GenerationArea {
                                page_id: page.id,
                                region_id: region.id,
                                rect: region.rect,
                                shape: region.shape.clone(),
                                document: region
                                    .frame_target
                                    .as_ref()
                                    .map(|frame| frame.document.clone()),
                                frame_rect: region.frame_target.as_ref().map(|frame| frame.rect),
                            })
                    })
                    .collect(),
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
                None,
                Some(&record.view),
                None,
            )?;
            return self.begin(project, fresh.job_id, false);
        }
        record
            .view
            .requirements
            .validate_current(project)
            .map_err(message)?;
        if !record.view.batches.is_empty() {
            if self.active.contains_key(&job_id) {
                return Err("A batch of this request is still running.".into());
            }
            if record.view.attempts.len() >= MAX_ATTEMPTS {
                return Err("This frozen job has reached its twenty-attempt limit. Start a separately reviewed job; earlier paid records remain preserved.".into());
            }
            if self.active.len() >= 4 {
                return Err("Four provider requests are already running. Wait for or cancel one before sending another.".into());
            }
            let index = if retry {
                record
                    .view
                    .batches
                    .iter()
                    .position(|batch| {
                        matches!(
                            batch.status,
                            JobStatus::Failed
                                | JobStatus::Unknown
                                | JobStatus::Cancelled
                                | JobStatus::Blocked
                                | JobStatus::Generating
                        )
                    })
                    .ok_or("Every batch of this request is already complete. Review or discard its result.")?
            } else {
                record
                    .view
                    .batches
                    .iter()
                    .position(|batch| batch.status == JobStatus::Prepared)
                    .ok_or("This batched request has no part left to send.")?
            };
            return self.start_batch(record, project, index);
        }
        let repair = record
            .view
            .requirements
            .validation_repair(&record.view.draft_feedback)
            .map_err(message)?;
        let estimate = self
            .adapter(record.view.provider)?
            .estimate(
                record.view.authentication,
                repair.as_ref().unwrap_or(&record.view.requirements),
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

    /// Start one part of a batched request: rebuild its frozen packet with the earlier parts'
    /// summaries and shared CSS, enforce the spending limit, and register the attempt.
    fn start_batch(
        &mut self,
        mut record: JobRecord,
        project: &Project,
        index: usize,
    ) -> NativeResult<(GenerationJobView, CancellationToken)> {
        let total = record.view.batches.len();
        let job_id = record.view.job_id;
        // Reserve every part up to this one before submitting the next paid request.
        let spent: f64 = record.view.batches[..=index]
            .iter()
            .map(|batch| batch.estimate_usd)
            .sum();
        if !spending_within_limit(
            record.view.authentication,
            spent,
            record.view.settings.spending_limit_usd,
        ) {
            for batch in &mut record.view.batches[index..] {
                batch.status = JobStatus::Failed;
                batch.message =
                    "Skipped because the batches would exceed the spending limit.".into();
                batch.validation_events.push(desegni_core::ValidationEvent {
                    kind: "spendingLimit".into(),
                    disposition: "conflicted".into(),
                    region_id: None,
                    message: format!(
                        "Batch {} of {total} was not sent: the first {} batches are estimated at ${spent:.2} and the spending limit is ${:.2}.",
                        index + 1,
                        index + 1,
                        record.view.settings.spending_limit_usd
                    ),
                });
            }
            self.settle_batches(&mut record, project)?;
            let message = format!(
                "Batch {} of {total} was not sent: the batches would exceed the ${:.2} spending limit. Raise the limit or choose fewer pages.",
                index + 1,
                record.view.settings.spending_limit_usd
            );
            record.view.message = message.clone();
            self.persist_retaining(record)?;
            return Err(message.into());
        }
        let earlier = record.view.batch_earlier(index);
        let packet = record.view.batches[index]
            .requirements
            .batched_part(project, index + 1, &earlier)
            .map_err(message)?;
        {
            let batch = &mut record.view.batches[index];
            batch.requirements = packet.clone();
            batch.status = JobStatus::Generating;
            batch.message = format!("Batch {} of {total}", index + 1);
            batch.validation_events.clear();
            batch.repair_rounds = 0;
        }
        record.view.requirements = packet;
        record.view.status = JobStatus::Generating;
        record.view.message = format!("Batch {} of {total}", index + 1);
        record.view.candidate = None;
        record.view.context = None;
        record.view.earlier_contexts.clear();
        record.view.first_exchange = None;
        record.view.validation_events.clear();
        record.view.validation_counts_recorded = false;
        record.view.repair_rounds = 0;
        record.view.repair_area_count = 0;
        record.view.raw_reply = None;
        record.view.failure_kind = None;
        record.view.provider_error = None;
        record.view.received_bytes = 0;
        record.error = None;
        let ordinal = record.view.attempts.len() + 1;
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

    /// Start the next unattempted part after a finished part, if any.
    fn advance_batch(
        &mut self,
        project: &Project,
        job_id: Uuid,
    ) -> NativeResult<Option<(GenerationJobView, CancellationToken)>> {
        let Some(record) = self.jobs.get(&job_id).cloned() else {
            return Ok(None);
        };
        if record.view.batches.is_empty() || record.view.status != JobStatus::Generating {
            return Ok(None);
        }
        let Some(index) = record
            .view
            .batches
            .iter()
            .position(|batch| batch.status == JobStatus::Prepared)
        else {
            return Ok(None);
        };
        match self.start_batch(record, project, index) {
            Ok(started) => Ok(Some(started)),
            Err(_) => Ok(None),
        }
    }

    /// Fold one part's result into the job and, once every part is terminal, merge the results
    /// into the single candidate the user reviews and applies.
    fn settle_batches(&self, record: &mut JobRecord, project: &Project) -> NativeResult<()> {
        let total = record.view.batches.len();
        if let Some(index) = record
            .view
            .batches
            .iter()
            .position(|batch| batch.status == JobStatus::Prepared)
        {
            record.view.status = JobStatus::Generating;
            record.view.candidate = None;
            record.view.message = format!("Batch {} of {total}", index + 1);
            return Ok(());
        }
        if record
            .view
            .batches
            .iter()
            .any(|batch| batch.status == JobStatus::Generating)
        {
            record.view.status = JobStatus::Generating;
            record.view.candidate = None;
            return Ok(());
        }
        let mut usage = None;
        for attempt in &record.view.attempts {
            usage = total_usage(usage, attempt.usage);
        }
        record.view.usage = usage;
        let successful: Vec<GenerationCandidate> = record
            .view
            .batches
            .iter()
            .filter_map(|batch| batch.candidate.clone())
            .collect();
        if successful.is_empty() {
            record.view.candidate = None;
            let status = if record
                .view
                .batches
                .iter()
                .any(|batch| batch.status == JobStatus::Cancelled)
            {
                JobStatus::Cancelled
            } else if record
                .view
                .batches
                .iter()
                .any(|batch| batch.status == JobStatus::Unknown)
            {
                JobStatus::Unknown
            } else if record
                .view
                .batches
                .iter()
                .any(|batch| batch.status == JobStatus::Failed)
            {
                JobStatus::Failed
            } else {
                JobStatus::Blocked
            };
            record.view.status = status;
            record.view.message = record
                .view
                .batches
                .iter()
                .rev()
                .find(|batch| !batch.message.trim().is_empty())
                .map(|batch| batch.message.clone())
                .unwrap_or_else(|| "The request did not finish.".into());
            return Ok(());
        }
        let merged_requirements = record
            .view
            .merged_requirements
            .clone()
            .ok_or("The batched request has no merged requirements.")?;
        let covered: BTreeSet<Uuid> = successful
            .iter()
            .flat_map(|candidate| candidate.regions.iter().map(|region| region.region_id))
            .collect();
        let mut unresolved = Vec::new();
        for (_, region_id) in merged_requirements.target_ids().map_err(message)? {
            if covered.contains(&region_id) {
                continue;
            }
            let explanation = record
                .view
                .batches
                .iter()
                .flat_map(|batch| batch.validation_events.iter())
                .find(|event| {
                    event.disposition == "conflicted" && event.region_id == Some(region_id)
                })
                .map(|event| event.message.clone())
                .unwrap_or_else(|| {
                    "This part of the request did not finish. Try again to complete it.".into()
                });
            unresolved.push((region_id, explanation));
        }
        let candidate = desegni_core::merge_document_batches(
            merged_requirements.job_id,
            &merged_requirements.sha256,
            &successful,
            &unresolved,
        )
        .map_err(message)?;
        candidate
            .validate(&merged_requirements, project)
            .map_err(message)?;
        let ready = candidate.is_ready();
        let completed = record
            .view
            .batches
            .iter()
            .all(|batch| matches!(batch.status, JobStatus::Ready | JobStatus::Blocked));
        record.view.candidate = Some(candidate);
        record.view.status = if ready && completed {
            JobStatus::Ready
        } else {
            JobStatus::Blocked
        };
        record.view.message = if ready && completed {
            "Result ready to apply.".into()
        } else {
            let waiting = record
                .view
                .batches
                .iter()
                .filter(|batch| !matches!(batch.status, JobStatus::Ready | JobStatus::Blocked))
                .count();
            format!(
                "{} of {total} batches finished; {waiting} need attention.",
                successful.len()
            )
        };
        Ok(())
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
                #[cfg(all(debug_assertions, feature = "native-tests"))]
                capture_benchmark_output(
                    &self.job_directory(original.id, job_id),
                    ordinal,
                    &response.text,
                    std::env::var("DESEGNI_GENERATION_BENCHMARK").as_deref() == Ok("1"),
                )?;
                match desegni_core::recover_generation_reply(&response.text, &record.view.requirements, original).and_then(|(mut candidate, events)| {
                    record.view.validation_events.extend(events);
                    let format = record.view.requirements.format_version()?;
                    if (8..12).contains(&format) { candidate.resolve_project_assets(original)?; }
                    candidate.validate(&record.view.requirements, original)?;
                    if format >= 7
                        && !candidate.image_download_urls()?.is_empty() {
                        return Err(desegni_core::Error("Images could not be prepared. Nothing was applied. Try generating again or remove the image request.".into()));
                    }
                    if format >= 17 && !candidate.image_generation_requests()?.is_empty() {
                        return Err(desegni_core::Error("Generated images could not be prepared.".into()));
                    }
                    Ok(candidate)
                }) {
                    Ok(candidate) => {
                        let stale = current
                            .filter(|project| project.id == original.id)
                            .and_then(|project| {
                                record.view.requirements.validate_current(project).err()
                            });
                        // A sequential part is complete when its own reply validated; the merged
                        // result decides whether anything can be applied.
                        let ready = if record.view.batches.is_empty() {
                            candidate.can_apply(&record.view.requirements)
                        } else {
                            candidate.is_ready()
                        };
                        record.view.status = if ready && stale.is_none() {
                            JobStatus::Ready
                        } else {
                            JobStatus::Blocked
                        };
                        record.view.message = if let Some(error) = stale {
                            format!(
                                "The generated result is stale: {error} Undo the changed requirements to reuse this candidate, or discard it and generate from the current requirements."
                            )
                        } else if ready {
                            "Result ready to apply.".into()
                        } else {
                            "Some requests need a choice.".into()
                        };
                        record.view.candidate = Some(candidate);
                    }
                    Err(error) => {
                        record.view.validation_events.push(desegni_core::ValidationEvent {
                            kind: desegni_core::validation_failure_kind(&error.to_string()).into(),
                            disposition: "conflicted".into(),
                            region_id: None,
                            message: error.to_string(),
                        });
                        record.view.raw_reply = Some(desegni_ai::scrub_diagnostic_reply(&response.text, None));
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
        if !record.view.batches.is_empty() {
            let index = record
                .view
                .batches
                .iter()
                .position(|batch| batch.status == JobStatus::Generating)
                .unwrap_or_else(|| record.view.batches.len().saturating_sub(1));
            let events = std::mem::take(&mut record.view.validation_events);
            {
                let batch = &mut record.view.batches[index];
                batch.candidate = record.view.candidate.take();
                batch.status = record.view.status;
                batch.message = record.view.message.clone();
                batch.validation_events = events.clone();
                batch.repair_rounds = record.view.repair_rounds;
                if batch.candidate.is_none()
                    && matches!(
                        batch.status,
                        JobStatus::Failed | JobStatus::Unknown | JobStatus::Cancelled
                    )
                    && batch.validation_events.is_empty()
                {
                    batch.validation_events.push(desegni_core::ValidationEvent {
                        kind: "batchFailure".into(),
                        disposition: "conflicted".into(),
                        region_id: None,
                        message: batch.message.clone(),
                    });
                }
            }
            record.view.validation_events = record.view.batches[index].validation_events.clone();
            if let Some(attempt) = record.view.attempts.last_mut() {
                attempt.status = record.view.batches[index].status;
                attempt.finished_at = Some(now());
            }
            if let Err(error) = self.settle_batches(&mut record, original) {
                record
                    .view
                    .validation_events
                    .push(desegni_core::ValidationEvent {
                        kind: "batchMerge".into(),
                        disposition: "conflicted".into(),
                        region_id: None,
                        message: error.to_string(),
                    });
                record.view.candidate = None;
                record.view.status = JobStatus::Blocked;
                record.view.message =
                    format!("The parts could not be merged into one change: {error}");
            }
            if record.view.candidate.is_some() {
                record.view.failure_kind = None;
                record.view.provider_error = None;
                record.error = None;
            }
            self.record_validation_counts(&mut record);
            let _ = self.persist_retaining(record);
            self.active.remove(&job_id);
            return Ok(());
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
                        .push(desegni_core::ValidationEvent {
                            kind: "telemetryStorage".into(),
                            disposition: "conflicted".into(),
                            region_id: None,
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
                .push(desegni_core::ValidationEvent {
                    kind: "telemetryStorage".into(),
                    disposition: "conflicted".into(),
                    region_id: None,
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
                    .is_some_and(|a| !a.expired(desegni_ai::image_access_now())))
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
    if view.sign_in.status == desegni_ai::ChatgptSignInStatus::AwaitingBrowser {
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
                    .is_some_and(|access| !access.expired(desegni_ai::image_access_now())))
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
                    a.available == desegni_ai::ImageAvailability::Yes
                        && !a.expired(desegni_ai::image_access_now())
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
    repair_job_id: Option<Uuid>,
    repair_draft_id: Option<Uuid>,
    region_ids: Option<Vec<Uuid>>,
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
    let mut workspace = workspace.lock().map_err(message)?;
    // A box drawn over an iframe before framed editing existed gains its document here, so its
    // source is supplied with the request instead of the AI reporting the source as unavailable.
    workspace.adopt_frame_targets()?;
    let project = workspace.project()?;
    let mut service = state.lock().map_err(message)?;
    if service.authentication != authentication || service.provider != provider {
        return Err("The generation method changed while preparing this request. Review it and prepare again.".into());
    }
    let draft = match (repair_job_id, repair_draft_id) {
        (Some(job_id), Some(draft_id)) => {
            let saved = service.saved_job(project.id, job_id)?;
            let mut draft = saved
                .drafts
                .into_iter()
                .find(|d| d.id == draft_id && !d.dismissed && !d.applied)
                .ok_or("The draft is no longer available.")?;
            if let Err(error) = draft.check(project) {
                draft.reason = error.to_string();
            }
            if draft.region_ids.is_empty() {
                draft.region_ids = saved
                    .areas
                    .iter()
                    .filter(|area| draft.page_id.is_none_or(|id| area.page_id == id))
                    .map(|area| area.region_id)
                    .collect();
            }
            Some(draft)
        }
        (None, None) => None,
        _ => return Err("Choose a saved draft to repair.".into()),
    };
    if let Some(ids) = region_ids {
        if draft.is_some() {
            return Err("Choose one request source.".into());
        }
        service.prepare_requests(project, account_id, None, None, Some(&ids))
    } else if let Some(draft) = draft {
        service.prepare_targeted(project, account_id, Some(draft))
    } else if let Some(account_id) = account_id {
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
    state.lock().map_err(message)?.saved_job(project_id, job_id)
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
                    && value.get("regionId").is_some()
                    && let Some(prompt) = value.get("prompt").and_then(|value| value.as_str())
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

fn total_usage(first: Option<TokenUsage>, second: Option<TokenUsage>) -> Option<TokenUsage> {
    match (first, second) {
        (Some(a), Some(b)) => Some(TokenUsage {
            input_tokens: a.input_tokens.saturating_add(b.input_tokens),
            output_tokens: a.output_tokens.saturating_add(b.output_tokens),
            total_tokens: a.total_tokens.saturating_add(b.total_tokens),
        }),
        (a, b) => b.or(a),
    }
}

/// A batched request reserves all of its parts against one spending limit. ChatGPT plan usage
/// is not priced in dollars, so only paid API keys are checked.
fn spending_within_limit(authentication: AuthenticationMethod, estimated: f64, limit: f64) -> bool {
    authentication == AuthenticationMethod::Chatgpt
        || (estimated.is_finite() && limit.is_finite() && estimated <= limit + 0.000_001)
}

#[allow(clippy::too_many_arguments)]
fn validation_repair_allowed(
    status: JobStatus,
    stopped: bool,
    rounds: usize,
    provider_failed: bool,
    authentication: AuthenticationMethod,
    spent_estimate: f64,
    repair_estimate: Option<f64>,
    limit: f64,
) -> bool {
    status == JobStatus::Generating
        && !stopped
        && !provider_failed
        && rounds < desegni_core::MAX_VALIDATION_REPAIR_ROUNDS
        && repair_estimate.is_some_and(|estimate| {
            estimate.is_finite()
                && estimate >= 0.0
                && (authentication == AuthenticationMethod::Chatgpt
                    || spent_estimate + estimate <= limit)
        })
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
    let begin = || {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?.clone();
        let mut service = state.lock().map_err(message)?;
        let client = service.adapter(provider)?;
        let (view, cancellation) = service.begin(&project, job_id, retry)?;
        Ok((project, client, view, cancellation))
    };
    let (original, client, view, mut cancellation) =
        if let (Some(account_id), Some(epoch)) = (account_id, epoch) {
            chatgpt.with_epoch(account_id, epoch, begin)?
        } else {
            begin()?
        };
    let job_id = view.job_id;
    let mut request = view.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let ordinal = request.attempts.len();
            let mut image_notice = None;
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
            let repair = request.requirements.validation_repair(&request.draft_feedback).ok().flatten();
            let mut result = ask(repair.as_ref().unwrap_or(&request.requirements)).await;
            if let (Some(repair), Ok(response)) = (&repair, &mut result) {
                // Reuse the validated follow-up digest adoption; the original job stays immutable.
                if let Ok(text) = request.requirements.adopt_follow_up_reply(repair, &response.text) {
                    response.text = text;
                }
            }
            // A completed, unambiguous final answer with malformed JSON/schema is a
            // validation failure, not a transport retry. All other provider failures stop here.
            if request.requirements.format_version().is_ok_and(|v| v >= 17)
                && let Err(error) = &result
                && error.outcome == SubmissionOutcome::Received
                && error.response_format.as_ref().is_some_and(|format|
                    matches!(format.rejection, Some(desegni_ai::ResponseRejection::CandidateJson | desegni_ai::ResponseRejection::CandidateShape)))
                && let Some(text) = error.response_format.as_ref().and_then(|format| format.diagnostic_reply.as_ref())
            {
                result = Ok(ProviderResponse { text: text.to_string(), response_id: error.response_id.clone().unwrap_or_default(), usage: error.usage });
            }
            // An element fix may ask once for other elements' HTML; reply automatically and keep
            // the final answer tied to the frozen requirements.
            if let Ok(first) = &result
                && let Ok(Some(follow_up)) = request
                    .requirements
                    .context_follow_up(&first.text, &original)
            {
                let usage = first.usage;
                let persisted = (|| -> NativeResult<()> {
                    let service_state = shared_app.state::<Mutex<GenerationService>>();
                    let mut service = service_state.lock().map_err(message)?;
                    let mut record = service
                        .jobs
                        .get(&job_id)
                        .cloned()
                        .ok_or("The active job is unavailable.")?;
                    if record.view.status != JobStatus::Generating || cancel_ref.is_cancelled() {
                        return Err("The request was cancelled.".into());
                    }
                    record.view.context = Some(follow_up.clone());
                    service.persist(record)?;
                    Ok(())
                })();
                result = if persisted.is_ok() {
                    ask(&follow_up).await
                } else {
                    Err(ProviderError::new(
                        ErrorKind::InvalidRequirements,
                        SubmissionOutcome::Received,
                    ))
                }
                .map_err(|mut error| {
                    error.usage = total_usage(usage, error.usage);
                    error
                })
                .map(|mut response| {
                    if let Ok(text) = request
                        .requirements
                        .adopt_follow_up_reply(&follow_up, &response.text)
                    {
                        response.text = text;
                    }
                    response.usage = total_usage(usage, response.usage);
                    response
                });
            }
            // One validation repair is distinct from a user retry. It uses the same frozen
            // provider/model, cancellation token, accounting and persisted context exchange.
            if request.requirements.format_version().is_ok_and(|v| v >= 17)
                && let Ok(first_response) = &mut result
            {
                    let recovered = desegni_core::recover_generation_reply(&first_response.text, &request.requirements, &original);
                    let (first_candidate, mut events) = match recovered {
                        Ok((candidate, events)) => (Some(candidate), events),
                        Err(error) => (None, vec![desegni_core::ValidationEvent {
                            kind: "replyStructure".into(), disposition: "conflicted".into(), region_id: None, message: error.to_string(),
                        }]),
                    };
                    let retained = desegni_core::retain_generation_drafts(&first_response.text, &request.requirements, &original, first_candidate.as_ref(), &events);
                    // Save rejected source before submitting repair: interruption must not lose it.
                    if let Ok(mut service) = shared_app.state::<Mutex<GenerationService>>().lock()
                        && let Some(mut record) = service.jobs.get(&job_id).cloned() {
                        record.view.drafts.extend(retained);
                        service.persist_retaining(record).map_err(|_| ProviderError::new(ErrorKind::InvalidRequirements, SubmissionOutcome::Received))?;
                    }
                    let initial_reply_diagnostic = first_candidate.is_none().then(|| desegni_ai::scrub_diagnostic_reply(&first_response.text, None));
                    let follow_up = request.requirements.validation_repair(&events).ok().flatten();
                    if let Some(follow_up) = follow_up.filter(|_| !cancel_ref.is_cancelled()) {
                        // Reserve the full first-round estimate plus the complete repair estimate.
                        // Unknown pricing/estimation never authorizes additional spending.
                        let repair_estimate = client_ref.estimate(request.authentication, &follow_up, request.generation_mode,
                            &desegni_core::result_schema(&follow_up), &request.settings).ok().map(|estimate| estimate.estimate_usd);
                        let previous_context = shared_app.state::<Mutex<GenerationService>>().lock().ok()
                            .and_then(|service| service.jobs.get(&job_id).and_then(|record| record.view.context.clone()));
                        let previous_estimate = previous_context.as_ref().map(|context| client_ref.estimate(request.authentication, context,
                            request.generation_mode, &desegni_core::result_schema(context), &request.settings).ok().map(|estimate| estimate.estimate_usd));
                        let spent_estimate = request.estimate_usd + previous_estimate.flatten().unwrap_or(0.0);
                        let known_cost = previous_estimate.is_none() || previous_estimate.flatten().is_some();
                        let allowed = validation_repair_allowed(request.status, cancel_ref.is_cancelled(), request.repair_rounds, false,
                            request.authentication, spent_estimate, repair_estimate.filter(|_| known_cost), request.settings.spending_limit_usd);
                        if allowed {
                            let persisted = (|| -> NativeResult<bool> {
                                let state = shared_app.state::<Mutex<GenerationService>>();
                                let mut service = state.lock().map_err(message)?;
                                let mut record = service.jobs.get(&job_id).cloned().ok_or("The active job is unavailable.")?;
                                if record.view.status != JobStatus::Generating || cancel_ref.is_cancelled()
                                    || record.view.repair_rounds >= desegni_core::MAX_VALIDATION_REPAIR_ROUNDS { return Ok(false); }
                                if let Some(previous) = record.view.context.take() {
                                    let path = service.job_directory(original.id, job_id).join("CONTEXT.md");
                                    regular_file(&path)?;
                                    let archived = service.job_directory(original.id, job_id).join(format!("attempt-{ordinal}-needs-CONTEXT.md"));
                                    if archived.exists() { return Err("The prior context archive already exists.".into()); }
                                    fs::rename(path, archived).map_err(message)?;
                                    record.view.earlier_contexts.push(previous);
                                }
                                record.view.first_exchange = Some(ValidationExchange {
                                    summary: first_candidate.as_ref().map_or_else(|| "The AI reply did not match the required format.".into(), |c| c.summary.clone()),
                                    regions: first_candidate.as_ref().map_or_else(Vec::new, |c| c.regions.clone()), usage: first_response.usage,
                                });
                                if let Some(reply) = &initial_reply_diagnostic { record.view.raw_reply = Some(reply.clone()); }
                                record.view.validation_events = events.clone();
                                record.view.context = Some(follow_up.clone());
                                record.view.repair_rounds += 1;
                                record.view.repair_area_count = if events.iter().any(|e| e.disposition == "conflicted" && e.region_id.is_none()) {
                                    record.view.areas.len()
                                } else { events.iter().filter(|e| e.disposition == "conflicted").filter_map(|e| e.region_id).collect::<BTreeSet<_>>().len() };
                                record.view.message = format!("Asked the AI to fix {} areas", record.view.repair_area_count);
                                service.persist(record)?;
                                Ok(true)
                            })().unwrap_or_else(|error| {
                                events.push(desegni_core::ValidationEvent { kind: "repairStorage".into(), disposition: "conflicted".into(), region_id: None, message: format!("Skipped the repair because its context could not be saved: {error}") });
                                false
                            });
                            if persisted {
                                let first_usage = first_response.usage;
                                match ask(&follow_up).await {
                                    Ok(second) => {
                                        first_response.usage = total_usage(first_usage, second.usage);
                                        let adopted = request.requirements.adopt_follow_up_reply(&follow_up, &second.text);
                                        let corrected = adopted.as_ref().map_err(Clone::clone)
                                            .and_then(|text| desegni_core::recover_generation_reply(text, &request.requirements, &original));
                                        let (kept, notes) = match &corrected {
                                            Ok((candidate, notes)) => (Some(candidate), notes.clone()),
                                            Err(error) => (None, vec![desegni_core::ValidationEvent { kind: "repairValidation".into(), disposition: "conflicted".into(), region_id: None, message: error.to_string() }]),
                                        };
                                        let retained = desegni_core::retain_generation_drafts(adopted.as_deref().unwrap_or(&second.text), &request.requirements, &original, kept, &notes);
                                        if let Ok(mut service) = shared_app.state::<Mutex<GenerationService>>().lock()
                                            && let Some(mut record) = service.jobs.get(&job_id).cloned() {
                                            record.view.drafts.extend(retained);
                                            service.persist_retaining(record).map_err(|_| ProviderError::new(ErrorKind::InvalidRequirements, SubmissionOutcome::Received))?;
                                        }
                                        match corrected {
                                            Ok((candidate, more)) => {
                                                events.extend(more);
                                                let merged = first_candidate.as_ref().map_or_else(|| Ok(candidate.clone()), |first|
                                                    desegni_core::merge_validation_repair(first, &candidate, &request.requirements, &original));
                                                if let Ok(merged) = merged {
                                                    for outcome in merged.regions.iter().filter(|r| r.status == desegni_core::RegionStatus::Conflict) {
                                                        if !events.iter().any(|e| e.region_id == Some(outcome.region_id) && e.message == outcome.explanation) {
                                                            events.push(desegni_core::ValidationEvent { kind: desegni_core::validation_failure_kind(&outcome.explanation).into(), disposition: "conflicted".into(), region_id: Some(outcome.region_id), message: outcome.explanation.clone() });
                                                        }
                                                    }
                                                    for e in &mut events {
                                                        if e.disposition == "conflicted" && (e.region_id.is_some_and(|id| merged.regions.iter().any(|r| r.region_id == id && r.status == desegni_core::RegionStatus::Implemented)) || e.region_id.is_none() && e.kind == "replyStructure" && merged.is_ready()) { e.disposition = "repaired".into(); }
                                                    }
                                                    first_response.text = serde_json::to_string(&merged).map_err(|_| ProviderError::new(ErrorKind::InvalidRequirements, SubmissionOutcome::Received))?;
                                                }
                                            }
                                            Err(error) => events.push(desegni_core::ValidationEvent { kind: "repairValidation".into(), disposition: "conflicted".into(), region_id: None, message: error.to_string() }),
                                        }
                                    }
                                    Err(error) => {
                                        // The paid first result remains usable if the repair transport fails.
                                        first_response.usage = total_usage(first_usage, error.usage);
                                        events.push(desegni_core::ValidationEvent { kind: "repairProvider".into(), disposition: "conflicted".into(), region_id: None, message: error.to_string() });
                                    }
                                }
                            }
                        } else {
                            let (kind, message) = if request.repair_rounds >= desegni_core::MAX_VALIDATION_REPAIR_ROUNDS {
                                ("repairRoundLimit", "This job has already used its one validation repair round.")
                            } else if repair_estimate.is_none() || !known_cost {
                                ("repairEstimate", "Skipped the repair because its combined cost could not be estimated.")
                            } else { ("repairLimit", "Skipped the repair because it would exceed the spending limit.") };
                            events.push(desegni_core::ValidationEvent { kind: kind.into(), disposition: "conflicted".into(), region_id: None, message: message.into() });
                        }
                    }
                    // Expand/normalise before the image adapters parse the candidate as well.
                    let final_candidate = desegni_core::recover_generation_reply(&first_response.text, &request.requirements, &original)
                        .map(|(candidate, _)| candidate)
                        .or_else(|error| desegni_core::conflicted_generation(&request.requirements, &original, &error.to_string()));
                    if let Ok(candidate) = final_candidate {
                        first_response.text = serde_json::to_string(&candidate).map_err(|_| ProviderError::new(ErrorKind::InvalidRequirements, SubmissionOutcome::Received))?;
                    }
                    if let Ok(mut service) = shared_app.state::<Mutex<GenerationService>>().lock()
                        && let Some(mut record) = service.jobs.get(&job_id).cloned()
                    {
                            record.view.validation_events = events;
                            if let Some(reply) = initial_reply_diagnostic { record.view.raw_reply = Some(reply); }
                            let _ = service.persist_retaining(record);
                    }
            }
            match result {
                Ok(response) => {
                    let response = client
                        .download_images(response, &request.requirements, &original, &cancellation)
                        .await;
                    let main_id = account_id.map(|id| format!("chatgpt:{id}"))
                        .unwrap_or_else(|| key.image_access_cache_key());
                    let (image_key, image_id) = {
                        let service = app.state::<Mutex<GenerationService>>();
                        let service = service.lock().map_err(message).map_err(|_| ProviderError::new(ErrorKind::InvalidRequirements, SubmissionOutcome::Received))?;
                        let fallback = service.image_credentials.key().ok();
                        let fallback_id = fallback.as_ref().map(|key| key.image_access_cache_key());
                        if desegni_ai::use_image_key(service.image_access.get(&main_id), fallback_id.as_ref().and_then(|id| service.image_access.get(id))) {
                            (fallback.unwrap(), fallback_id.unwrap())
                        } else { (key.clone(), main_id) }
                    };
                    let (response, unavailable) = client
                        .generate_images_with_status(
                            response, request.authentication, &request.settings,
                            &request.requirements, &original, &image_key, &cancellation,
                        ).await;
                    if unavailable {
                        let id = image_id;
                        let stored = app.state::<Mutex<GenerationService>>().lock().map_err(message)
                            .and_then(|mut service| service.store_image_access(id, desegni_ai::ImageAccess::new(
                                desegni_ai::ImageAvailability::No, "image_api_rejected")));
                        image_notice = Some(if stored.is_ok() { "Image generation is not available for this account; placeholders were kept.".to_owned() }
                            else { "Image generation is not available for this account; placeholders were kept. The access status could not be saved.".to_owned() });
                    }
                    Ok(response)
                }
                Err(error) => Err(error),
            }
        }.await;
            let workspace_state = app.state::<Mutex<Workspace>>();
            let service_state = app.state::<Mutex<GenerationService>>();
            let mut next = None;
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
                if let Some(notice) = image_notice
                    && let Some(mut record) = service.jobs.get(&job_id).cloned()
                {
                    record.view.message.push(' ');
                    record.view.message.push_str(&notice);
                    let _ = service.persist_retaining(record);
                }
                // Continue with the next sequential part inside this same job, using the same
                // provider settings and cancellation contract.
                next = service.advance_batch(&original, job_id).ok().flatten();
            }
            match next {
                Some((view, token)) => {
                    request = view;
                    cancellation = token;
                }
                None => break,
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
        return Err("Only a current candidate with ready regions can be applied.".into());
    }
    let candidate = record
        .view
        .candidate
        .as_ref()
        .ok_or("The candidate is unavailable.")?;
    workspace
        .project()?
        .apply_generation(
            record.view.request_requirements(),
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
        record.view.request_requirements(),
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
    record.view.applied_source_fingerprints = workspace
        .project()?
        .pages
        .iter()
        .filter(|page| record.view.source_fingerprints.contains_key(&page.id))
        .map(|page| {
            (
                page.id,
                desegni_core::source_fingerprints(page.content.as_ref()),
            )
        })
        .collect();
    record.view.status = JobStatus::Applied;
    let total = record
        .view
        .candidate
        .as_ref()
        .map_or(0, |c| c.regions.len());
    let applied = record.view.candidate.as_ref().map_or(0, |c| {
        c.regions
            .iter()
            .filter(|r| r.status == desegni_core::RegionStatus::Implemented)
            .count()
    });
    record.view.message = if applied < total {
        format!("Applied {applied} of {total} areas")
    } else {
        "Page updated.".into()
    };
    if let Err(error) = service.persist(record.clone()) {
        record.view.message = format!(
            "The project was applied and kept locally, but its job-history update failed: {error} The applied job ID prevents duplicate changes."
        );
        service.jobs.insert(job_id, record);
    }
    Ok(workspace.view())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn create_preview(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    previews: State<'_, Mutex<PreviewRegistry>>,
    page_id: Uuid,
    job_id: Option<Uuid>,
    draft_id: Option<Uuid>,
    run_scripts: bool,
) -> NativeResult<PreviewDescriptor> {
    trusted(&window)?;
    let workspace = workspace.lock().map_err(message)?;
    let project = workspace.project()?;
    let candidate = if let Some(job_id) = job_id {
        let view = state
            .lock()
            .map_err(message)?
            .saved_job(project.id, job_id)?;
        if let Some(draft_id) = draft_id {
            let draft = view
                .drafts
                .iter()
                .find(|d| d.id == draft_id && !d.dismissed && !d.applied)
                .ok_or("The draft is unavailable.")?;
            // Security failures are copy-only. Stale drafts can preview immutable retained bytes.
            draft.check_source(project).map_err(message)?;
            let content = draft
                .content
                .clone()
                .ok_or("The draft has no previewable source.")?;
            if draft.page_id != Some(page_id) {
                return Err("The draft belongs to another page.".into());
            }
            Some(GenerationCandidate {
                job_id,
                requirements_sha256: view.requirements.sha256,
                summary: "Draft preview".into(),
                regions: vec![],
                pages: vec![desegni_core::GeneratedPage { page_id, content }],
                component_requests: vec![],
                decision_proposals: vec![],
                criticism: vec![],

                documents: vec![],
            })
        } else {
            view.candidate
        }
    } else {
        None
    };
    if job_id.is_some() && candidate.is_none() {
        return Err("There is no generated source to preview for this job.".into());
    }
    previews.lock().map_err(message)?.register(
        project,
        candidate.as_ref(),
        page_id,
        run_scripts && draft_id.is_none(),
    )
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftReview {
    job: GenerationJobView,
    workspace: Option<WorkspaceView>,
    revision: u64,
    blocked: Option<String>,
    previewable: bool,
    confirmation: String,
}

#[tauri::command]
pub fn generation_draft(
    window: WebviewWindow,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    job_id: Uuid,
    draft_id: Uuid,
    action: String,
    confirmed_revision: Option<u64>,
    region_id: Option<Uuid>,
) -> NativeResult<DraftReview> {
    trusted(&window)?;
    let mut workspace = workspace.lock().map_err(message)?;
    let mut service = state.lock().map_err(message)?;
    let mut view = service.saved_job(workspace.project()?.id, job_id)?;
    let index = view
        .drafts
        .iter()
        .position(|d| d.id == draft_id)
        .ok_or("The draft is unavailable.")?;
    let mut draft = view.drafts[index].clone();
    let _ = confirmed_revision; // Apply uses the locked current page, including newer edits.
    if action == "apply" {
        if let Some(region_id) = region_id {
            draft = draft.for_region(region_id).map_err(message)?;
        }
    }
    let revision = workspace.project()?.revision;
    let blocked = draft.check(workspace.project()?).err().map(message);
    let previewable = draft.check_source(workspace.project()?).is_ok();
    let confirmation = draft.confirmation(workspace.project()?);
    let changed = match action.as_str() {
        "inspect" => false,
        "dismiss" => {
            for retained in &mut view.drafts {
                if retained.page_id == draft.page_id && retained.code == draft.code {
                    retained.dismissed = true;
                }
            }
            true
        }
        "apply" => {
            draft
                .rebased_content(workspace.project()?)
                .map_err(message)?;
            let checkpoint = service
                .job_directory(view.project_id, job_id)
                .join(format!("before-draft-{}-{revision}.desegni", draft.id));
            let file = ProjectFile::acquire(&checkpoint).map_err(message)?;
            if checkpoint.exists() {
                if file.load().map_err(message)? != *workspace.project()? {
                    return Err("An existing draft safety checkpoint differs from the current project. It was preserved.".into());
                }
            } else {
                file.save(workspace.project()?).map_err(message)?;
            }
            drop(file);
            workspace.apply_draft(&view.requirements, &draft)?;
            if let Some(page_id) = draft.page_id {
                let page = workspace.project()?.page(page_id).map_err(message)?;
                view.applied_source_fingerprints.insert(
                    page_id,
                    desegni_core::source_fingerprints(page.content.as_ref()),
                );
            }
            if let Some(candidate) = &mut view.candidate {
                for outcome in &mut candidate.regions {
                    if draft.region_ids.contains(&outcome.region_id) {
                        outcome.status = desegni_core::RegionStatus::Implemented;
                    }
                }
            }
            for retained in &mut view.drafts {
                if retained.page_id == draft.page_id && retained.code == draft.code {
                    if let Some(region_id) = region_id {
                        retained.region_ids.retain(|id| *id != region_id);
                        retained.applied = retained.region_ids.is_empty();
                    } else {
                        retained.applied = true;
                    }
                }
            }
            if view.areas.iter().all(|area| {
                workspace
                    .project()
                    .ok()
                    .and_then(|p| p.page(area.page_id).ok())
                    .is_some_and(|page| {
                        page.regions
                            .iter()
                            .any(|r| r.id == area.region_id && r.completed)
                    })
            }) {
                view.status = JobStatus::Applied;
            }
            true
        }
        _ => return Err("Unknown draft action.".into()),
    };
    if changed {
        // A past-job edit must never move the project's latest-job pointer.
        if let Err(error) = service.persist_retaining(JobRecord {
            version: 1,
            view: view.clone(),
            error: None,
        }) {
            if action != "apply" {
                return Err(error);
            }
            view.message =
                format!("Draft applied; its history update needs another local save: {error}");
        }
    }
    Ok(DraftReview {
        job: view,
        workspace: (action == "apply").then(|| workspace.view()),
        revision,
        blocked,
        previewable,
        confirmation,
    })
}
