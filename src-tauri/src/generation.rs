use crate::{
    chatgpt::ChatgptState,
    workspace::{Workspace, WorkspaceView},
};

pub(crate) use crate::chatgpt::ChatgptState as ExportChatgptState;
use desegnos_ai::{
    ApiKey, AuthenticationMethod, CancellationToken, ChatgptView, CredentialInfo,
    CredentialManager, CredentialStorage, ErrorKind, ModelInfo, OpenAiClient, ProviderClient,
    ProviderDescriptor, ProviderError, ProviderId, ProviderResponse, ProviderSettings,
    SubmissionOutcome, TokenUsage, chatgpt_model_catalogue, provider_catalogue,
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

const UNREADABLE_REPLY_RETRY_MESSAGE: &str = "Retrying after an unreadable reply…";
/// The streamed text kept for progress and cut-off recovery; replies are bounded far below.
const MAX_PARTIAL_REPLY_BYTES: usize = 16 * 1024 * 1024;

/// Pages receiving content in streamed reply text: the `pageId` of each `insertInBoundary`
/// operation, in order of appearance. Page renames name every page up front and don't count.
fn page_ids_in(text: &str) -> Vec<Uuid> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("\"pageId\"") {
        let before = &rest[..at];
        let operation = before
            .rfind("\"kind\"")
            .map(|kind| &before[kind..])
            .is_some_and(|kind| kind.contains("insertInBoundary") && kind.len() < 400);
        rest = &rest[at + 8..];
        if !operation {
            continue;
        }
        let value = rest.trim_start().strip_prefix(':').map(str::trim_start);
        if let Some(id) = value
            .and_then(|value| value.strip_prefix('"'))
            .and_then(|value| value.get(..36))
            .and_then(|id| Uuid::parse_str(id).ok())
            && !found.contains(&id)
        {
            found.push(id);
        }
    }
    found
}

/// Validate a model reply into the job's candidate and say what the job becomes. Used for a
/// complete reply and for the complete part of a reply that was cut off mid-stream.
fn candidate_status(text: &str, record: &mut JobRecord, original: &Project) -> JobStatus {
    let checked = GenerationCandidate::from_json_for_project(
        text,
        &record.view.requirements,
        original,
        &mut record.view.validation_events,
    )
    .map(|mut candidate| {
        let valid = candidate.retain_valid_operations(
            &record.view.requirements,
            original,
            &mut record.view.validation_events,
        );
        (candidate, valid)
    });
    match checked {
        Ok((candidate, Ok(()))) => {
            let ready = candidate.can_apply();
            record.view.message = if ready {
                "Result ready to apply.".into()
            } else {
                "Some requests need a choice.".into()
            };
            record.view.candidate = Some(candidate);
            if ready {
                JobStatus::Ready
            } else {
                JobStatus::Blocked
            }
        }
        Ok((_, Err(error))) => {
            let element_id = error.element_id;
            record
                .view
                .validation_events
                .push(desegnos_core::ValidationEvent {
                    kind: desegnos_core::validation_failure_kind(&error).into(),
                    disposition: "conflicted".into(),
                    element_id,
                    message: error.to_string(),
                });
            record.view.raw_reply = Some(desegnos_ai::scrub_diagnostic_reply(text, None));
            record.view.message =
                format!("Desegnos rejected the candidate: {error} Nothing was applied.");
            JobStatus::Blocked
        }
        Err(error) => {
            record
                .view
                .validation_events
                .push(desegnos_core::ValidationEvent {
                    kind: desegnos_core::validation_failure_kind(&error).into(),
                    disposition: "conflicted".into(),
                    element_id: None,
                    message: error.to_string(),
                });
            record.view.raw_reply = Some(desegnos_ai::scrub_diagnostic_reply(text, None));
            record.view.message =
                format!("Desegnos rejected the candidate: {error} Nothing was applied.");
            JobStatus::Blocked
        }
    }
}

/// A complete reply whose text is not a JSON document even after the same control-character
/// repair `GenerationCandidate::from_json` applies. A schema, data or validation failure parses
/// as JSON, so resending the identical frozen request would not help and must not be retried.
fn is_unreadable_json(text: &str) -> bool {
    match desegnos_core::parse_provider_json::<serde_json::Value>(text) {
        Ok(_) => false,
        Err(error) => matches!(
            error.classify(),
            serde_json::error::Category::Syntax | serde_json::error::Category::Eof
        ),
    }
}

fn usage_cost_usd(settings: &ProviderSettings, usage: TokenUsage) -> Option<f64> {
    let pricing = settings.pricing.as_ref()?;
    pricing.validate(&settings.model).ok()?;
    Some(
        (usage.input_tokens as f64 * pricing.input_usd_per_million
            + usage.output_tokens as f64 * pricing.output_usd_per_million)
            / 1_000_000.0
            + pricing.request_usd,
    )
}

fn retry_covers_budget(
    settings: &ProviderSettings,
    estimate_usd: f64,
    usage: Option<TokenUsage>,
) -> bool {
    let first = usage
        .and_then(|usage| usage_cost_usd(settings, usage))
        .unwrap_or(estimate_usd);
    first.is_finite()
        && estimate_usd.is_finite()
        && first + estimate_usd <= settings.spending_limit_usd + f64::EPSILON
}

/// Only a complete, received reply with an unreadable JSON body is worth one automatic resend.
fn should_retry_unreadable_reply(
    result: &Result<ProviderResponse, ProviderError>,
    cancelled: bool,
    settings: &ProviderSettings,
    estimate_usd: f64,
) -> bool {
    if cancelled {
        return false;
    }
    let Ok(response) = result else {
        return false;
    };
    is_unreadable_json(&response.text)
        && retry_covers_budget(settings, estimate_usd, response.usage)
}

fn add_usage(first: Option<TokenUsage>, second: Option<TokenUsage>) -> Option<TokenUsage> {
    match (first, second) {
        (Some(first), Some(second)) => Some(TokenUsage {
            input_tokens: first.input_tokens.saturating_add(second.input_tokens),
            output_tokens: first.output_tokens.saturating_add(second.output_tokens),
            total_tokens: first.total_tokens.saturating_add(second.total_tokens),
        }),
        (first, second) => first.or(second),
    }
}

fn with_first_usage(
    result: Result<ProviderResponse, ProviderError>,
    first: Option<TokenUsage>,
) -> Result<ProviderResponse, ProviderError> {
    let Some(first) = first else {
        return result;
    };
    match result {
        Ok(mut response) => {
            response.usage = add_usage(Some(first), response.usage);
            Ok(response)
        }
        Err(mut error) => {
            error.usage = add_usage(Some(first), error.usage);
            Err(error)
        }
    }
}

async fn ask_with_one_unreadable_reply_retry<F, Fut>(
    mut ask: F,
    cancel: &CancellationToken,
    settings: &ProviderSettings,
    estimate_usd: f64,
    before_retry: impl FnOnce(Option<TokenUsage>),
) -> (Result<ProviderResponse, ProviderError>, bool)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<ProviderResponse, ProviderError>>,
{
    let first = ask().await;
    if !should_retry_unreadable_reply(&first, cancel.is_cancelled(), settings, estimate_usd) {
        return (first, false);
    }
    if cancel.is_cancelled() {
        return (first, false);
    }
    let first_usage = first.as_ref().ok().and_then(|response| response.usage);
    before_retry(first_usage);
    let second = ask().await;
    (with_first_usage(second, first_usage), true)
}

/// A completed, unambiguous final answer with malformed JSON/schema is a validation failure, not
/// a transport failure. Its text is handed to the candidate parser, which can repair control
/// characters in strings before deciding whether the reply is readable.
fn accept_completed_candidate(
    result: Result<ProviderResponse, ProviderError>,
) -> Result<ProviderResponse, ProviderError> {
    if let Err(error) = &result
        && error.outcome == SubmissionOutcome::Received
        && error.response_format.as_ref().is_some_and(|format| {
            matches!(
                format.rejection,
                Some(
                    desegnos_ai::ResponseRejection::CandidateJson
                        | desegnos_ai::ResponseRejection::CandidateShape
                )
            )
        })
        && let Some(text) = error
            .response_format
            .as_ref()
            .and_then(|format| format.diagnostic_reply.as_ref())
    {
        return Ok(ProviderResponse {
            text: text.to_string(),
            response_id: error.response_id.clone().unwrap_or_default(),
            usage: error.usage,
        });
    }
    result
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
    #[serde(default)]
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
    #[serde(default)]
    provider_retry: Option<desegnos_ai::ProviderRetry>,
    status: JobStatus,
    estimate_usd: f64,
    received_bytes: usize,
    /// Boundaries the streaming reply has started building, in order, for live progress.
    #[serde(default)]
    pages_started: Vec<Uuid>,
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
    /// The reply text streamed so far in this attempt; kept in memory only, so a reply cut
    /// off mid-stream can still apply the operations that arrived complete.
    #[serde(skip)]
    partial_text: String,
}

impl JobRecord {
    /// Keep the persisted status mirrors together. Ready and Blocked finish a provider
    /// attempt even though the job can still be reviewed, retried, or applied.
    /// Call after setting the message so an active batch receives the same explanation.
    fn transition(&mut self, status: JobStatus) {
        use JobStatus::*;

        debug_assert!(
            self.view.status == status
                || matches!(
                    (self.view.status, status),
                    (Prepared | Failed | Unknown | Cancelled | Blocked, Generating)
                        | (Generating, Ready | Blocked | Failed | Unknown | Cancelled)
                        | (Ready, Blocked)
                        | (Blocked, Ready)
                        | (Prepared | Ready | Blocked | Failed | Unknown, Cancelled)
                        | (Prepared | Ready | Blocked | Failed | Unknown | Cancelled, Discarded)
                        // Recording failures can make even a late result uncertain.
                        | (_, Unknown)
                        // The project's applied-job ledger is authoritative during recovery.
                        | (_, Applied)
                )
        );
        self.view.status = status;
        if let Some(attempt) = self.view.attempts.last_mut() {
            attempt.status = status;
            if !matches!(status, Prepared | Generating | Applying) {
                attempt.finished_at.get_or_insert_with(now);
            }
        }
        for batch in &mut self.view.batches {
            if batch.status == Generating {
                batch.status = status;
                batch.message.clone_from(&self.view.message);
            }
        }
    }
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
    models_error: Option<String>,
    models_provider_error: Option<ProviderError>,
    models_auth_required: bool,
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
    api_models: BTreeMap<(ProviderId, String), desegnos_ai::Result<Vec<ModelInfo>>>,
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

async fn api_catalogue_for_key(
    state: &Mutex<GenerationService>,
    provider: ProviderId,
    settings: &ProviderSettings,
    key: &ApiKey,
    cancellation: &CancellationToken,
    force: bool,
) -> desegnos_ai::Result<Vec<ModelInfo>> {
    let credential_id = catalogue_identity(provider, settings, key)?;
    let client = {
        let service = state.lock().map_err(|_| {
            ProviderError::new(ErrorKind::InvalidSettings, SubmissionOutcome::NotSubmitted)
        })?;
        if !force
            && let Some(Ok(cached)) = service.api_models.get(&(provider, credential_id.clone()))
        {
            return Ok(cached.clone());
        }
        ProviderClient::with_settings(
            provider,
            service.client.clone().map_err(|_| {
                ProviderError::new(ErrorKind::InvalidSettings, SubmissionOutcome::NotSubmitted)
            })?,
            settings,
        )
        .map_err(|_| {
            ProviderError::new(ErrorKind::InvalidSettings, SubmissionOutcome::NotSubmitted)
        })?
    };
    let result = client.list_api_models(key, cancellation).await;
    if !cancellation.is_cancelled()
        && let Ok(mut service) = state.lock()
    {
        service
            .api_models
            .insert((provider, credential_id), result.clone());
    }
    result
}

fn catalogue_identity(
    provider: ProviderId,
    settings: &ProviderSettings,
    key: &ApiKey,
) -> desegnos_ai::Result<String> {
    let mut id = key.image_access_cache_key();
    if provider == ProviderId::Local {
        id.push_str(&digest(desegnos_ai::local_endpoint(settings)?.as_bytes()));
    }
    Ok(id)
}

fn provider_key(
    provider: ProviderId,
    local_authentication: bool,
    credentials: &CredentialManager,
) -> desegnos_ai::Result<ApiKey> {
    if provider == ProviderId::Local && !local_authentication {
        return Ok(ApiKey::local_anonymous());
    }
    credentials.key()
}

pub(super) async fn api_model_for_key(
    state: &Mutex<GenerationService>,
    provider: ProviderId,
    settings: &ProviderSettings,
    key: &ApiKey,
    cancellation: &CancellationToken,
) -> desegnos_ai::Result<ModelInfo> {
    let models = api_catalogue_for_key(state, provider, settings, key, cancellation, false).await?;
    let model = models
        .into_iter()
        .find(|model| model.id == settings.model)
        .ok_or_else(|| {
            ProviderError::new(ErrorKind::ModelUnavailable, SubmissionOutcome::NotSubmitted)
        })?;
    model.validate_api_settings(settings)?;
    Ok(model)
}

pub(super) async fn refresh_api_models(state: &Mutex<GenerationService>) -> NativeResult<()> {
    let (provider, credentials, settings) = {
        let service = state.lock().map_err(message)?;
        if service.authentication != AuthenticationMethod::ApiKey {
            return Ok(());
        }
        (
            service.provider,
            service.credentials.clone(),
            service.settings.clone(),
        )
    };
    let local_authentication = settings.local_authentication;
    let key = tauri::async_runtime::spawn_blocking(move || {
        provider_key(provider, local_authentication, &credentials)
    })
    .await
    .map_err(message)?;
    if let Ok(key) = key {
        // Safe provider failures belong in the returned model status, not an API-key fallback.
        let _ = api_catalogue_for_key(
            state,
            provider,
            &settings,
            &key,
            &CancellationToken::new(),
            true,
        )
        .await;
    } else {
        state
            .lock()
            .map_err(message)?
            .api_models
            .retain(|(id, _), _| *id != provider);
    }
    Ok(())
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
            api_models: BTreeMap::new(),
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
        ProviderClient::with_settings(provider, self.client.clone()?, &self.settings)
            .map_err(message)
    }
    fn set_local_authentication(&mut self, enabled: bool) -> NativeResult<()> {
        let path = self.settings_path(ProviderId::Local);
        let mut settings = if self.provider == ProviderId::Local {
            self.settings.clone()
        } else if path.exists() {
            read_json(&path, 65_536)?
        } else {
            ProviderId::Local.defaults()
        };
        settings.local_authentication = enabled;
        write_json(&path, &settings)?;
        if self.provider == ProviderId::Local {
            self.settings = settings;
        }
        Ok(())
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
    pub(super) fn export_connection(&self) -> (AuthenticationMethod, ProviderId, ProviderSettings) {
        (self.authentication, self.provider, self.settings.clone())
    }

    pub(super) fn export_client(
        &self,
        provider: ProviderId,
        settings: &ProviderSettings,
    ) -> NativeResult<ProviderClient> {
        ProviderClient::with_settings(provider, self.client.clone()?, settings).map_err(message)
    }

    pub(super) fn export_key(
        &mut self,
        provider: ProviderId,
        settings: &ProviderSettings,
    ) -> NativeResult<ApiKey> {
        provider_key(
            provider,
            settings.local_authentication,
            &self.credentials_for(provider),
        )
        .map_err(message)
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
            || view.requirements.validate_format().is_err()
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
    fn saved_history(&self, project_id: Uuid) -> NativeResult<Vec<desegnos_core::EarlierRequest>> {
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
            JobStatus::Applied => desegnos_core::RequestHistoryOutcome::Applied,
            JobStatus::Discarded
                if view.failure_kind.is_some() || view.provider_error.is_some() =>
            {
                desegnos_core::RequestHistoryOutcome::Failed
            }
            JobStatus::Discarded => desegnos_core::RequestHistoryOutcome::RejectedByUser,
            JobStatus::Failed | JobStatus::Unknown | JobStatus::Cancelled => {
                desegnos_core::RequestHistoryOutcome::Failed
            }
            _ => desegnos_core::RequestHistoryOutcome::Drafted,
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
                        r.element_id == Some(entry.element_id)
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
                desegnos_core::record_validation_rejection(
                    entry,
                    match event.kind.as_str() {
                        "outsideRequest" => desegnos_core::RejectionKind::OutsideRequest,
                        "outsideFrame" | "element" => desegnos_core::RejectionKind::OutsideFrame,
                        "pointer" => desegnos_core::RejectionKind::Pointer,
                        "image" => desegnos_core::RejectionKind::Image,
                        _ => desegnos_core::RejectionKind::Other,
                    },
                    &event.message,
                );
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
            || record.view.requirements.validate_format().is_err()
            || record.view.project_id != project.id
            || record.view.job_id != latest.job_id
            || record.view.requirements.project_id != project.id
            || record.view.requirements.job_id != latest.job_id
            || record.view.requirements.sha256
                != digest(record.view.requirements.markdown.as_bytes())
            || record.view.areas.len() > desegnos_core::MAX_GENERATION_REQUESTS
            // Areas are the distinct layers requested; several requests may share one layer.
            || record.view.requirements.target_ids().map_or(true, |targets| targets.len() != record.view.areas.len())
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
            record.view.message = "Generation already applied.".into();
            record.transition(JobStatus::Applied);
            return self.persist(record).map(Some);
        }
        if matches!(
            record.view.status,
            JobStatus::Generating | JobStatus::Applying
        ) && !self.active.contains_key(&record.view.job_id)
        {
            record.view.message = "Desegnos stopped before this operation finished. The outcome may be uncertain; no request was automatically repeated. Review the project before an explicit retry.".into();
            record.transition(JobStatus::Unknown);
            return self.persist(record).map(Some);
        }
        if record.view.status == JobStatus::Ready
            && let Err(error) = resolve_current(&record.view, project)
        {
            record.view.message = format!(
                "This candidate is stale: {error} Undo the changed requirements to reuse this candidate, or discard it and generate from the current requirements."
            );
            record.transition(JobStatus::Blocked);
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
                .is_some_and(|candidate| candidate.can_apply())
            && resolve_current(&record.view, project).is_ok()
        {
            record.view.message = "Result ready to apply.".into();
            record.transition(JobStatus::Ready);
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
                &desegnos_core::result_schema(),
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
            partial_text: String::new(),
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
                provider_retry: None,
                status: JobStatus::Prepared,
                estimate_usd,
                received_bytes: 0,
                pages_started: Vec::new(),
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
                &desegnos_core::result_schema(),
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
        record.view.candidate = None;
        record.view.usage = None;
        record.view.received_bytes = 0;
        record.view.pages_started.clear();
        record.partial_text.clear();
        record.view.failure_kind = None;
        record.view.provider_error = None;
        record.view.provider_retry = None;
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
        record.transition(JobStatus::Generating);
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
            if !progress.text_delta.is_empty()
                && record.partial_text.len() + progress.text_delta.len() <= MAX_PARTIAL_REPLY_BYTES
            {
                // Rescan a short overlap so a page id split across two deltas is still found.
                let from = record.partial_text.len().saturating_sub(64);
                record.partial_text.push_str(&progress.text_delta);
                let from = (0..=from)
                    .rev()
                    .find(|at| record.partial_text.is_char_boundary(*at))
                    .unwrap_or(0);
                for page in page_ids_in(&record.partial_text[from..]) {
                    if !record.view.pages_started.contains(&page) {
                        record.view.pages_started.push(page);
                    }
                }
            }
            if let Some(retry) = &progress.retry {
                record.view.message = retry.message(record.view.provider);
            }
            record.view.provider_retry = progress.retry;
            if let Some(attempt) = record.view.attempts.last_mut() {
                attempt.response_id = progress.response_id;
            }
        }
    }

    fn retrying_after_unreadable_reply(
        &mut self,
        job_id: Uuid,
        ordinal: usize,
        usage: Option<TokenUsage>,
    ) -> NativeResult<()> {
        if self
            .active
            .get(&job_id)
            .is_none_or(|(active, token)| *active != ordinal || token.is_cancelled())
        {
            return Ok(());
        }
        let Some(record) = self.jobs.get_mut(&job_id) else {
            return Ok(());
        };
        record.view.message = UNREADABLE_REPLY_RETRY_MESSAGE.into();
        record.view.received_bytes = 0;
        record.view.pages_started.clear();
        record.partial_text.clear();
        record.view.usage = usage;
        record.view.validation_events.push(desegnos_core::ValidationEvent {
            kind: "unreadableReply".into(),
            disposition: "retried".into(),
            element_id: None,
            message: "The AI reply was not readable JSON, so Desegnos sent the same frozen request once more.".into(),
        });
        let record = record.clone();
        self.persist_retaining(record)?;
        Ok(())
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
                record.transition(record.view.status);
                self.persist_retaining(record)?;
            }
            self.active.remove(&job_id);
            return Ok(());
        }
        let status = match result {
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
                candidate_status(&response.text, &mut record, original)
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
                let status = if error.kind == ErrorKind::Cancelled {
                    JobStatus::Cancelled
                } else if error.outcome == SubmissionOutcome::Unknown
                    && !error.http_status.is_some_and(|status| status >= 400)
                {
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
                // A reply cut off mid-stream still carries every operation that arrived whole.
                // Validate and apply those like any reply; the untouched boundaries stay queued
                // for Generate. A credential echo (Malformed) or a cancel never gets here.
                let recovered = (status != JobStatus::Cancelled
                    && error.kind != ErrorKind::Malformed
                    && !record.partial_text.is_empty())
                .then(|| desegnos_core::complete_truncated_operations(&record.partial_text))
                .flatten();
                record.error = Some(error);
                match recovered {
                    Some(text) => {
                        let before = record.view.validation_events.len();
                        let recovered_status = candidate_status(&text, &mut record, original);
                        let kept = record.view.candidate.as_ref().is_some_and(|candidate| {
                            candidate
                                .requests
                                .iter()
                                .any(|request| !request.operations.is_empty())
                        });
                        if kept && recovered_status != JobStatus::Blocked {
                            // Treated as a partial success, not a failed attempt.
                            record.view.provider_error = None;
                            record.view.failure_kind = None;
                            record.error = None;
                            record.view.message = format!(
                                "The reply was cut off. Kept what arrived complete; Generate again for the rest. {}",
                                record.view.message
                            );
                            recovered_status
                        } else {
                            record.view.candidate = None;
                            record.view.validation_events.truncate(before);
                            record.view.message = record
                                .error
                                .as_ref()
                                .map(ToString::to_string)
                                .unwrap_or_default();
                            status
                        }
                    }
                    None => status,
                }
            }
        };
        record.transition(status);
        self.record_validation_counts(&mut record);
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
            record.view.message = format!(
                "Generation stopped while recording its result: {error} No request will be automatically repeated."
            );
            record.transition(JobStatus::Unknown);
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
        record.view.candidate = None;
        record.view.message = if record.view.authentication == AuthenticationMethod::Chatgpt {
            "Stopped. A submitted request may still consume ChatGPT plan allowance or credits. Late output cannot be applied.".into()
        } else {
            "Stopped. A submitted request may still incur a charge. Late output cannot be applied."
                .into()
        };
        record.transition(JobStatus::Cancelled);
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
            record.view.message = "Account signed out. Generation was cancelled and late output cannot be applied. A submitted request may still consume ChatGPT plan allowance or credits.".into();
            record.transition(JobStatus::Cancelled);
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

async fn credential_key(
    state: &AiState<'_>,
    provider: ProviderId,
    settings: &ProviderSettings,
) -> NativeResult<ApiKey> {
    let credentials = state.lock().map_err(message)?.credentials_for(provider);
    let local_authentication = settings.local_authentication;
    tauri::async_runtime::spawn_blocking(move || {
        provider_key(provider, local_authentication, &credentials)
    })
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
        (id, key, client, probe_id, prior_access)
    };
    let (id, key, client, probe_id, prior_access) = probe;
    let access = client.probe_image_access(&key).await;
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
    let (id, auth, client, credentials, prior_access) = {
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
    let access = client.probe_image_access(&key).await;
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
    let connection_settings = settings.clone();
    let local_authentication = settings.local_authentication;
    let (credential, credential_id) = tauri::async_runtime::spawn_blocking(move || {
        let credential =
            if provider == ProviderId::Local && !connection_settings.local_authentication {
                CredentialInfo {
                    configured: true,
                    storage: desegnos_ai::CredentialLocation::None,
                    masked_identifier: Some("Local server".into()),
                    vault_error: None,
                }
            } else {
                credentials.status()
            };
        let identity = provider_key(
            provider,
            connection_settings.local_authentication,
            &credentials,
        )
        .ok()
        .and_then(|key| catalogue_identity(provider, &connection_settings, &key).ok());
        (credential, identity)
    })
    .await
    .map_err(message)?;
    let (api_catalogue, models_error, models_auth_required, models_provider_error) = {
        let service = state.lock().map_err(message)?;
        match credential_id
            .as_ref()
            .and_then(|id| service.api_models.get(&(provider, id.clone())))
        {
            Some(cache) => match cache {
                Ok(models) => (models.clone(), None, false, None),
                Err(error) => (
                    vec![],
                    Some(desegnos_ai::models_error(
                        provider,
                        local_authentication,
                        error,
                    )),
                    desegnos_ai::models_need_key(provider, local_authentication, error),
                    Some(error.clone()),
                ),
            },
            None => (vec![], None, false, None),
        }
    };
    let chatgpt = chatgpt_state.view().await;
    let catalogue = if authentication == AuthenticationMethod::Chatgpt {
        if chatgpt.accounts.iter().any(|account| {
            Some(account.id) == chatgpt.active_account_id
                && account.connected
                && account.plan_enabled
        }) {
            chatgpt_model_catalogue(&chatgpt.models)
        } else {
            vec![]
        }
    } else {
        api_catalogue
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
        models_error,
        models_provider_error,
        models_auth_required,
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
    refresh_models: Option<bool>,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    if refresh_models.unwrap_or(false) {
        let (authentication, client) = {
            let service = state.lock().map_err(message)?;
            (service.authentication, service.client.clone())
        };
        let view = chatgpt.view().await;
        if authentication == AuthenticationMethod::ApiKey {
            refresh_api_models(&state).await?;
        }
        if authentication == AuthenticationMethod::Chatgpt
            && let Some(account) = view.accounts.iter().find(|account| {
                Some(account.id) == view.active_account_id
                    && account.connected
                    && account.plan_enabled
                    && !account.storage_repair_required
            })
            && let Ok(client) = client
        {
            // Discovery errors remain visible in models_error; retain the settings view.
            let _ = chatgpt
                .manager()?
                .discover_models(account.id, &client, &CancellationToken::new())
                .await;
        }
    }
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
    refresh_api_models(&state).await?;
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn save_ai_settings(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    settings: ProviderSettings,
    connection_only: Option<bool>,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    if connection_only.unwrap_or(false) {
        {
            let mut service = state.lock().map_err(message)?;
            if service.provider != ProviderId::Local {
                return Err("Choose Local before changing its server address.".into());
            }
            service.ensure_switch_allowed()?;
            let endpoint = desegnos_ai::local_endpoint(&settings).map_err(|_| {
                "Use a local server address ending in /v1, such as http://127.0.0.1:11434/v1."
                    .to_owned()
            })?;
            let mut next = service.settings.clone();
            next.endpoint = Some(endpoint);
            next.local_authentication = settings.local_authentication;
            write_json(&service.settings_path(ProviderId::Local), &next)?;
            service.settings = next;
            service.configuration_error = None;
        }
        refresh_api_models(&state).await?;
        return settings_view(&state, &chatgpt).await;
    }
    let account = chatgpt.view().await;
    let (provider, authentication) = {
        let service = state.lock().map_err(message)?;
        (service.provider, service.authentication)
    };
    if authentication == AuthenticationMethod::ApiKey {
        let key = credential_key(&state, provider, &settings).await?;
        api_model_for_key(&state, provider, &settings, &key, &CancellationToken::new())
            .await
            .map_err(message)?;
    }
    {
        let mut service = state.lock().map_err(message)?;
        if service.provider != provider || service.authentication != authentication {
            return Err(
                "The AI connection changed. Review the current settings and save again.".into(),
            );
        }
        service
            .provider
            .validate(&settings, service.authentication)
            .map_err(message)?;
        if service.authentication == AuthenticationMethod::Chatgpt {
            account.selected_model(&settings).map_err(message)?;
        }
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
    let (credentials, provider) = {
        let service = state.lock().map_err(message)?;
        if image.unwrap_or(false) {
            (service.image_credentials.clone(), service.provider)
        } else {
            (service.credentials.clone(), service.provider)
        }
    };
    tauri::async_runtime::spawn_blocking(move || credentials.set_key(key, storage))
        .await
        .map_err(message)?
        .map_err(message)?;
    if provider == ProviderId::Local && !image.unwrap_or(false) {
        state
            .lock()
            .map_err(message)?
            .set_local_authentication(true)?;
    }
    if !image.unwrap_or(false) {
        refresh_api_models(&state).await?;
    }
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
    let (credentials, provider) = {
        let service = state.lock().map_err(message)?;
        if image.unwrap_or(false) {
            (service.image_credentials.clone(), service.provider)
        } else {
            (service.credentials.clone(), service.provider)
        }
    };
    tauri::async_runtime::spawn_blocking(move || credentials.remove_key())
        .await
        .map_err(message)?
        .map_err(message)?;
    if provider == ProviderId::Local && !image.unwrap_or(false) {
        state
            .lock()
            .map_err(message)?
            .set_local_authentication(false)?;
        refresh_api_models(&state).await?;
    }
    settings_view(&state, &chatgpt).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessResult {
    provider_error: Option<ProviderError>,
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
        if let Some(error) = view.models_provider_error {
            return Ok(AccessResult {
                message: error.message(),
                provider_error: Some(error),
            });
        }
        view.selected_model(&settings).map_err(message)?;
        return Ok(AccessResult {
            provider_error: None,
            message: "Connected · ChatGPT plan and model access confirmed.".into(),
        });
    }
    let key = credential_key(&state, provider, &settings).await?;
    let result = async {
        api_model_for_key(&state, provider, &settings, &key, &CancellationToken::new()).await?;
        ProviderClient::with_settings(provider, client, &settings)?
            .test_model_access(&settings, &key, &CancellationToken::new())
            .await
    }
    .await;
    Ok(match result {
        Ok(_) => AccessResult {
            provider_error: None,
            message: "Connected · Model access confirmed.".into(),
        },
        Err(error) => AccessResult {
            message: error.message(),
            provider_error: Some(error),
        },
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
    let (authentication, account_id, provider, frozen_settings) = {
        let workspace = workspace.lock().map_err(message)?;
        let mut service = state.lock().map_err(message)?;
        let record = service.record(workspace.project()?, job_id)?;
        (
            record.view.authentication,
            record.view.chatgpt_account_id,
            record.view.provider,
            record.view.settings.clone(),
        )
    };
    let epoch = account_id.map(|id| chatgpt.epoch(id)).transpose()?;
    let begin = || {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?.clone();
        let mut service = state.lock().map_err(message)?;
        let client =
            ProviderClient::with_settings(provider, service.client.clone()?, &frozen_settings)
                .map_err(message)?;
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
            let mut account_model = None;
            let mut discovery_error = None;
            // Keep preparation local. All credential refresh/model discovery belongs to
            // the registered, cancellable attempt, before any paid generation submission.
            let credentials = async {
                if authentication == AuthenticationMethod::Chatgpt {
                    let account_id = account_id.ok_or(
                        "This ChatGPT request has no bound account. Prepare a new request.",
                    )?;
                    let chatgpt = app.state::<ChatgptState>();
                    let manager = chatgpt.manager()?;
                    let mut available = chatgpt.view().await;
                    if available.models.is_empty() {
                        let client = app
                            .state::<Mutex<GenerationService>>()
                            .lock()
                            .map_err(message)?
                            .client
                            .clone()?;
                        available = manager
                            .discover_models(account_id, &client, &cancellation)
                            .await
                            .map_err(message)?;
                    }
                    if available.active_account_id != Some(account_id) {
                        return Err(
                            "The selected ChatGPT account changed. Prepare the request again."
                                .into(),
                        );
                    }
                    if let Some(error) = available.models_provider_error {
                        let reason = error.message();
                        discovery_error = Some(error);
                        return Err(reason);
                    }
                    account_model = Some(available.selected_model(&request.settings).cloned());
                    manager
                        .bearer(account_id, &cancellation)
                        .await
                        .map_err(message)
                } else {
                    credential_key(
                        &app.state::<Mutex<GenerationService>>(),
                        provider,
                        &request.settings,
                    )
                    .await
                }
            };
            let key = cancellation
                .run_until_cancelled(credentials)
                .await
                .ok_or_else(|| {
                    ProviderError::new(ErrorKind::Cancelled, SubmissionOutcome::NotSubmitted)
                })?
                .map_err(|_| {
                    discovery_error.unwrap_or_else(|| {
                        ProviderError::new(
                            ErrorKind::MissingCredential,
                            SubmissionOutcome::NotSubmitted,
                        )
                    })
                })?;
            let account_model = account_model.transpose()?;
            let api_model = if authentication == AuthenticationMethod::ApiKey {
                Some(
                    api_model_for_key(
                        &app.state::<Mutex<GenerationService>>(),
                        provider,
                        &request.settings,
                        &key,
                        &cancellation,
                    )
                    .await?,
                )
            } else {
                None
            };

            let (shared_app, request_ref, key_ref, cancel_ref, client_ref) =
                (&app, &request, &key, &cancellation, &client);
            let ask = |requirements: &RequirementsSnapshot| {
                let progress_app = shared_app.clone();
                let requirements = requirements.clone();
                let account_model = account_model.clone();
                let api_model = api_model.clone();
                async move {
                    let schema = desegnos_core::result_schema();
                    if let Some(model) = &account_model {
                        let estimate = client_ref.estimate(
                            request_ref.authentication,
                            &requirements,
                            request_ref.generation_mode,
                            &schema,
                            &request_ref.settings,
                        )?;
                        model.validate_input(estimate.input_token_upper_bound)?;
                    }
                    if let Some(model) = &api_model {
                        let estimate = client_ref.estimate(
                            request_ref.authentication,
                            &requirements,
                            request_ref.generation_mode,
                            &schema,
                            &request_ref.settings,
                        )?;
                        model.validate_api_input(
                            &request_ref.settings,
                            estimate.input_token_upper_bound,
                        )?;
                    }
                    client_ref
                        .generate(
                            desegnos_ai::GenerationRequest {
                                authentication: request_ref.authentication,
                                snapshot: &requirements,
                                mode: request_ref.generation_mode,
                                schema: &schema,
                                settings: &request_ref.settings,
                                key: key_ref,
                                cancellation: cancel_ref,
                            },
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
            let mut send = || {
                let response = ask(&request.requirements);
                async move { accept_completed_candidate(response.await) }
            };
            let (result, _) = ask_with_one_unreadable_reply_retry(
                &mut send,
                &cancellation,
                &request.settings,
                request.estimate_usd,
                |usage| {
                    if let Ok(mut service) = app.state::<Mutex<GenerationService>>().lock() {
                        let _ = service.retrying_after_unreadable_reply(job_id, ordinal, usage);
                    }
                },
            )
            .await;
            result
        }
        .await;
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
    record.view.message = "Result discarded.".into();
    record.transition(JobStatus::Discarded);
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
                    .generate_requested_images(&image_requests, &key, &CancellationToken::new())
                    .await
            }
            _ => BTreeMap::new(),
        }
    };
    let mut image_failures = Vec::new();
    let generated = generated
        .into_iter()
        .filter_map(|(name, result)| match result {
            Ok(uri) => Some((name, uri)),
            Err(error) => {
                image_failures.push(format!("Image {name}: {error}"));
                None
            }
        })
        .collect();
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
    let warnings: Vec<_> = record
        .view
        .validation_events
        .iter()
        .filter(|event| matches!(event.disposition.as_str(), "dropped" | "warning"))
        .map(|event| event.message.as_str())
        .collect();
    record.view.message = if warnings.is_empty() {
        "Layers updated.".into()
    } else {
        format!("Layers updated with warnings. {}", warnings.join(" "))
    };
    if !image_failures.is_empty() {
        record
            .view
            .message
            .push_str(&format!(" {}", image_failures.join(" ")));
    }
    record.transition(JobStatus::Applied);
    record.view.accepted_decision_indices = accepted_decision_indices;
    service.persist(record)?;
    Ok(workspace.view())
}

/// Native-only access binding for identity analysis. Credentials never enter an
/// IPC view, project, source snapshot, or persisted analysis record.
pub(super) struct IdentityAccess {
    pub provider: ProviderId,
    pub authentication: AuthenticationMethod,
    pub account_id: Option<Uuid>,
    pub account_epoch: Option<u64>,
    pub settings: ProviderSettings,
    pub client: ProviderClient,
    pub key: ApiKey,
    pub model: Option<desegnos_ai::ChatgptModel>,
    pub api_model: Option<ModelInfo>,
}

impl GenerationService {
    pub(super) fn identity_settings(&self) -> (ProviderId, AuthenticationMethod, ProviderSettings) {
        (self.provider, self.authentication, self.settings.clone())
    }
}

pub(super) struct IdentityAccessFailure {
    pub message: String,
    pub provider_error: Option<Box<ProviderError>>,
}
impl From<String> for IdentityAccessFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            provider_error: None,
        }
    }
}
impl From<&str> for IdentityAccessFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
impl From<ProviderError> for IdentityAccessFailure {
    fn from(error: ProviderError) -> Self {
        Self {
            message: error.message(),
            provider_error: Some(Box::new(error)),
        }
    }
}

pub(super) async fn identity_access(
    state: &Mutex<GenerationService>,
    chatgpt: &ChatgptState,
    cancellation: &CancellationToken,
) -> Result<IdentityAccess, IdentityAccessFailure> {
    let (authentication, settings, client, credentials, provider) = {
        let service = state.lock().map_err(message)?;
        if let Some(error) = service
            .configuration_error
            .as_ref()
            .or(service.authentication_error.as_ref())
        {
            return Err(error.clone().into());
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
    let mut model = None;
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
        if view.active_account_id != Some(account_id) {
            return Err("The selected ChatGPT account changed. Prepare the analysis again.".into());
        }
        if let Some(error) = view.models_provider_error {
            return Err(error.into());
        }
        model = Some(view.selected_model(&settings).map_err(message)?.clone());
        let key = chatgpt
            .manager()?
            .bearer(account_id, cancellation)
            .await
            .map_err(message)?;
        validate_identity_account(&chatgpt.view().await, account_id, &settings.model)?;
        chatgpt.with_epoch(account_id, epoch, || Ok(()))?;
        (Some(account_id), Some(epoch), key)
    } else {
        let local_authentication = settings.local_authentication;
        let key = tauri::async_runtime::spawn_blocking(move || {
            provider_key(provider, local_authentication, &credentials)
        })
        .await
        .map_err(message)?
        .map_err(message)?;
        ProviderClient::with_settings(provider, client.clone(), &settings)
            .map_err(message)?
            .test_model_access(&settings, &key, cancellation)
            .await?;
        (None, None, key)
    };
    if cancellation.is_cancelled() {
        return Err("Identity analysis was cancelled before submission.".into());
    }
    let api_model = if authentication == AuthenticationMethod::ApiKey {
        Some(api_model_for_key(state, provider, &settings, &key, cancellation).await?)
    } else {
        None
    };
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
        provider,
        authentication,
        account_id,
        account_epoch,
        client: ProviderClient::with_settings(provider, client, &settings).map_err(message)?,
        settings,
        key,
        model,
        api_model,
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

#[cfg(test)]
mod unreadable_reply_retry_tests {
    #[test]
    fn page_ids_in_streamed_text_are_found_once_in_order() {
        let a = "11111111-1111-4111-8111-111111111111";
        let b = "22222222-2222-4222-8222-222222222222";
        let text = format!(
            r#"{{"kind":"setPage","pageId":"{a}"}},{{"kind":"insertInBoundary","pageId": "{b}"}},{{"kind":"insertInBoundary","pageId":"{a}"}},{{"kind":"insertInBoundary","pageId":"{b}"}}"#
        );
        assert_eq!(
            super::page_ids_in(&text),
            vec![
                uuid::Uuid::parse_str(b).unwrap(),
                uuid::Uuid::parse_str(a).unwrap()
            ]
        );
        assert!(super::page_ids_in(r#"{"pageId":"1111"#).is_empty());
    }

    use super::*;
    use desegnos_core::{
        Appearance, ElementKind, PendingRequest, Project, Rect, RequestTarget, Viewport,
        apply_generation_output, empty_element,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn usage(count: u64) -> TokenUsage {
        TokenUsage {
            input_tokens: count,
            output_tokens: count,
            total_tokens: count * 2,
        }
    }

    fn response(text: &str, count: u64) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            text: text.into(),
            response_id: "resp_retry_test".into(),
            usage: Some(usage(count)),
        })
    }

    fn settings() -> ProviderSettings {
        ProviderSettings {
            spending_limit_usd: 2.0,
            pricing: Some(desegnos_ai::ModelPricing {
                model: desegnos_ai::MODEL_ID.into(),
                input_usd_per_million: 1.0,
                output_usd_per_million: 1.0,
                request_usd: 0.0,
                image_usd: 0.0,
            }),
            ..ProviderSettings::default()
        }
    }

    fn project_and_element() -> (Project, Uuid) {
        let mut project = Project::new(
            "Retry".into(),
            Viewport {
                width: 800,
                height: 600,
            },
            Appearance::Light,
        )
        .unwrap();
        let page = project.pages[0].id;
        let size = project.pages[0].sizes[0].id;
        let mut element = empty_element(
            ElementKind::Text,
            Rect {
                x: 10.0,
                y: 10.0,
                width: 120.0,
                height: 40.0,
            },
        );
        element.text = "Before".into();
        let element_id = element.id;
        project.pages[0].elements.push(element);
        project.requests.push(PendingRequest {
            boundary_retry_order: Vec::new(),
            id: Uuid::new_v4(),
            text: "Change the text.".into(),
            attachments: vec![],
            targets: vec![RequestTarget::Element {
                page,
                size: Some(size),
                id: element_id,
            }],
            area: None,
            requested_at: "2026-01-01T00:00:00Z".into(),
        });
        project.validate().unwrap();
        (project, element_id)
    }

    fn candidate_json(snapshot: &RequirementsSnapshot, element_id: Uuid) -> String {
        serde_json::json!({
            "jobId": snapshot.job_id,
            "requirementsSha256": snapshot.sha256,
            "summary": "Updated the text.",
            "requests": [{
                "requestId": null,
                "elementId": element_id,
                "status": "implemented",
                "explanation": "Updated the text.",
                "operations": [{
                    "kind": "update",
                    "patches": [{"elementId": element_id, "text": "After"}],
                }],
            }],
            "images": [],
            "pointerClassifications": [],
            "decisionProposals": [],
            "criticism": [],
        })
        .to_string()
    }

    fn prepared(
        settings: ProviderSettings,
    ) -> (GenerationService, Project, Uuid, Uuid, std::path::PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("desegnos-retry-test-{}", Uuid::new_v4()));
        let mut service = GenerationService::from_directory(directory.clone()).unwrap();
        service.settings = settings;
        let (project, element_id) = project_and_element();
        let view = service
            .prepare_requests(&project, None, None, Some(&[element_id]), Some(&[]))
            .unwrap();
        (service, project, view.job_id, element_id, directory)
    }

    #[test]
    fn json_syntax_failures_are_retryable_but_data_and_validation_failures_are_not() {
        let settings = settings();
        assert!(is_unreadable_json("{\"a\": 1,}"));
        assert!(is_unreadable_json("{\"a\": 1"));
        let data_error = "{\"summary\": 42}";
        let validation_error = "{\"requests\":[{\"elementId\":\"out-of-scope\"}]}";
        for text in [data_error, validation_error] {
            assert!(!is_unreadable_json(text));
            assert!(!should_retry_unreadable_reply(
                &response(text, 100),
                false,
                &settings,
                0.01
            ));
        }
        // The same control-character repair the candidate parser uses makes this readable.
        assert!(!is_unreadable_json("{\"html\":\"<p>one\n<p>two\"}"));
    }

    #[test]
    fn truncated_cancelled_and_over_budget_replies_are_not_retried() {
        let settings = settings();
        let malformed = response("{\"a\": 1", 100).unwrap();
        for kind in [
            ErrorKind::Incomplete,
            ErrorKind::OutputLimit,
            ErrorKind::Truncated,
        ] {
            let truncated = Err(ProviderError::new(kind, SubmissionOutcome::Received));
            assert!(!should_retry_unreadable_reply(
                &truncated, false, &settings, 0.01
            ));
        }
        let cancelled = Err(ProviderError::new(
            ErrorKind::Cancelled,
            SubmissionOutcome::NotSubmitted,
        ));
        assert!(!should_retry_unreadable_reply(
            &cancelled, true, &settings, 0.01
        ));
        assert!(!should_retry_unreadable_reply(
            &Ok(malformed.clone()),
            true,
            &settings,
            0.01
        ));
        let too_small = ProviderSettings {
            spending_limit_usd: 0.01,
            pricing: Some(desegnos_ai::ModelPricing {
                model: desegnos_ai::MODEL_ID.into(),
                input_usd_per_million: 1_000_000.0,
                output_usd_per_million: 1_000_000.0,
                request_usd: 0.0,
                image_usd: 0.0,
            }),
            ..ProviderSettings::default()
        };
        assert!(!should_retry_unreadable_reply(
            &Ok(malformed),
            false,
            &too_small,
            0.01
        ));
    }

    #[tokio::test]
    async fn malformed_reply_is_retried_once_and_the_valid_layers_are_applied() {
        let (mut service, project, job_id, element_id, directory) = prepared(settings());
        let requirements = service.jobs.get(&job_id).unwrap().view.requirements.clone();
        let estimate = service.jobs.get(&job_id).unwrap().view.estimate_usd;
        let settings = service.settings.clone();
        let valid = candidate_json(&requirements, element_id);
        let malformed = valid[..valid.len() - 1].to_string();
        let (_, cancellation) = service.begin(&project, job_id, false).unwrap();
        let sends = Arc::new(AtomicUsize::new(0));
        let counter = sends.clone();
        let first = malformed.clone();
        let second = valid.clone();
        let (result, retried) = ask_with_one_unreadable_reply_retry(
            || {
                let index = counter.fetch_add(1, Ordering::SeqCst);
                let text = if index == 0 {
                    first.clone()
                } else {
                    second.clone()
                };
                async move { response(&text, if index == 0 { 100 } else { 300 }) }
            },
            &cancellation,
            &settings,
            estimate,
            |_| {
                service
                    .retrying_after_unreadable_reply(job_id, 1, Some(usage(100)))
                    .unwrap();
            },
        )
        .await;
        assert!(retried);
        assert_eq!(sends.load(Ordering::SeqCst), 2);
        let result = result.unwrap();
        assert_eq!(result.usage.unwrap().total_tokens, 800);
        service
            .finish(&project, Some(&project), job_id, 1, Ok(result))
            .unwrap();
        let view = &service.jobs.get(&job_id).unwrap().view;
        assert!(matches!(view.status, JobStatus::Ready));
        assert_eq!(view.usage.unwrap().total_tokens, 800);
        assert!(
            view.validation_events
                .iter()
                .any(|event| { event.kind == "unreadableReply" && event.disposition == "retried" })
        );
        let applied = apply_generation_output(
            &project,
            view.request_requirements(),
            view.candidate.as_ref().unwrap(),
            &[],
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(applied.pages[0].elements[0].text, "After");
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn both_malformed_replies_fail_after_exactly_two_sends() {
        let (mut service, project, job_id, element_id, directory) = prepared(settings());
        let requirements = service.jobs.get(&job_id).unwrap().view.requirements.clone();
        let estimate = service.jobs.get(&job_id).unwrap().view.estimate_usd;
        let settings = service.settings.clone();
        let valid = candidate_json(&requirements, element_id);
        let malformed = valid[..valid.len() - 1].to_string();
        let (_, cancellation) = service.begin(&project, job_id, false).unwrap();
        let sends = Arc::new(AtomicUsize::new(0));
        let counter = sends.clone();
        let (result, retried) = ask_with_one_unreadable_reply_retry(
            || {
                counter.fetch_add(1, Ordering::SeqCst);
                let text = malformed.clone();
                async move { response(&text, 100) }
            },
            &cancellation,
            &settings,
            estimate,
            |_| {
                service
                    .retrying_after_unreadable_reply(job_id, 1, Some(usage(100)))
                    .unwrap();
            },
        )
        .await;
        assert!(retried);
        assert_eq!(sends.load(Ordering::SeqCst), 2);
        service
            .finish(&project, Some(&project), job_id, 1, result)
            .unwrap();
        let view = &service.jobs.get(&job_id).unwrap().view;
        assert!(matches!(view.status, JobStatus::Blocked));
        assert!(view.message.starts_with("Desegnos rejected the candidate:"));
        assert_eq!(view.usage.unwrap().total_tokens, 400);
        assert!(
            view.validation_events
                .iter()
                .any(|event| { event.kind == "unreadableReply" && event.disposition == "retried" })
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn a_budget_without_room_for_a_second_request_sends_once() {
        let settings = ProviderSettings {
            spending_limit_usd: 0.01,
            pricing: Some(desegnos_ai::ModelPricing {
                model: desegnos_ai::MODEL_ID.into(),
                input_usd_per_million: 1_000_000.0,
                output_usd_per_million: 1_000_000.0,
                request_usd: 0.0,
                image_usd: 0.0,
            }),
            ..ProviderSettings::default()
        };
        let sends = Arc::new(AtomicUsize::new(0));
        let counter = sends.clone();
        let (result, retried) = ask_with_one_unreadable_reply_retry(
            || {
                counter.fetch_add(1, Ordering::SeqCst);
                async move { response("{\"a\": 1", 100) }
            },
            &CancellationToken::new(),
            &settings,
            0.01,
            |_| panic!("must not retry past the spending limit"),
        )
        .await;
        assert!(!retried);
        assert_eq!(sends.load(Ordering::SeqCst), 1);
        assert!(result.is_ok());
    }
}

#[cfg(test)]
mod api_model_cache_tests {
    use super::*;

    #[tokio::test]
    async fn local_catalogues_are_bound_to_server_and_key_mode() {
        let directory =
            std::env::temp_dir().join(format!("desegnos-local-model-cache-{}", Uuid::new_v4()));
        let mut service = GenerationService::from_directory(directory.clone()).unwrap();
        let anonymous = ApiKey::local_anonymous();
        let settings = ProviderId::Local.defaults();
        let other_server = ProviderSettings {
            endpoint: Some("http://127.0.0.1:8080/v1".into()),
            ..settings.clone()
        };
        let authenticated = ApiKey::new("synthetic-local-server-key".into()).unwrap();
        let identity = catalogue_identity(ProviderId::Local, &settings, &anonymous).unwrap();
        assert_ne!(
            identity,
            catalogue_identity(ProviderId::Local, &other_server, &anonymous).unwrap()
        );
        assert_ne!(
            identity,
            catalogue_identity(ProviderId::Local, &settings, &authenticated).unwrap()
        );
        service
            .api_models
            .insert((ProviderId::Local, identity), Ok(Vec::new()));
        // Anonymous mode must not consult the credential store, even when no key exists.
        assert!(
            provider_key(
                ProviderId::Local,
                false,
                &service.credentials_for(ProviderId::Local)
            )
            .is_ok()
        );
        let state = Mutex::new(service);
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            api_catalogue_for_key(
                &state,
                ProviderId::Local,
                &settings,
                &anonymous,
                &cancel,
                false
            )
            .await
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            api_catalogue_for_key(
                &state,
                ProviderId::Local,
                &other_server,
                &anonymous,
                &cancel,
                false
            )
            .await
            .unwrap_err()
            .kind,
            ErrorKind::Cancelled
        );
        drop(state);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn api_model_cache_isolates_credentials_and_providers_and_survives_cancelled_refresh() {
        let directory =
            std::env::temp_dir().join(format!("desegnos-api-model-cache-{}", Uuid::new_v4()));
        let mut service = GenerationService::from_directory(directory.clone()).unwrap();
        let first = ApiKey::new("synthetic-first-provider-key".into()).unwrap();
        let second = ApiKey::new("synthetic-second-provider-key".into()).unwrap();
        let openai = ProviderId::Openai.catalogue();
        let anthropic = ProviderId::Anthropic.catalogue();
        service.api_models.insert(
            (ProviderId::Openai, first.image_access_cache_key()),
            Ok(openai.clone()),
        );
        service.api_models.insert(
            (ProviderId::Anthropic, first.image_access_cache_key()),
            Ok(anthropic.clone()),
        );
        service.api_models.insert(
            (ProviderId::Openai, second.image_access_cache_key()),
            Err(ProviderError::new(
                ErrorKind::Authentication,
                SubmissionOutcome::NotSubmitted,
            )),
        );
        let state = Mutex::new(service);
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            api_catalogue_for_key(
                &state,
                ProviderId::Openai,
                &ProviderSettings::default(),
                &first,
                &cancelled,
                false
            )
            .await
            .unwrap(),
            openai
        );
        assert_eq!(
            api_catalogue_for_key(
                &state,
                ProviderId::Anthropic,
                &ProviderSettings::default(),
                &first,
                &cancelled,
                false
            )
            .await
            .unwrap(),
            anthropic
        );
        for (provider, key) in [(ProviderId::Openai, &second), (ProviderId::Google, &first)] {
            assert_eq!(
                api_catalogue_for_key(
                    &state,
                    provider,
                    &ProviderSettings::default(),
                    key,
                    &cancelled,
                    false
                )
                .await
                .unwrap_err()
                .kind,
                ErrorKind::Cancelled
            );
        }
        assert_eq!(
            api_catalogue_for_key(
                &state,
                ProviderId::Openai,
                &ProviderSettings::default(),
                &first,
                &cancelled,
                true
            )
            .await
            .unwrap_err()
            .kind,
            ErrorKind::Cancelled
        );
        assert_eq!(
            api_catalogue_for_key(
                &state,
                ProviderId::Openai,
                &ProviderSettings::default(),
                &first,
                &cancelled,
                false
            )
            .await
            .unwrap(),
            openai
        );
        drop(state);
        fs::remove_dir_all(directory).unwrap();
    }
}
