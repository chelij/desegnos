use crate::{
    ai_access::{AccessFailure, Connection, active_account, resolve},
    ai_job::{
        Failure, interrupted_notice, lock, signed_out_notice, stopped_notice, unsaved_cancel_notice,
    },
    chatgpt::ChatgptState,
    native_files::{NativeResult, message, regular_file},
    workspace::{Workspace, WorkspaceView},
};

use desegnos_ai::{
    AuthenticationMethod, CancellationToken, ErrorKind, ProviderError, ProviderId,
    ProviderResponse, ProviderSettings, SubmissionOutcome, TokenUsage,
};
use desegnos_core::{GenerationCandidate, GenerationMode, Project, RequirementsSnapshot, digest};
use desegnos_storage::write_text;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{AppHandle, Manager, State};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

type AiState<'a> = State<'a, Mutex<GenerationService>>;
#[path = "generation_job_store.rs"]
mod job_store;
use crate::ai_settings::{AiSettingsStore, AiState as SettingsState, refresh_image_access};
type WorkspaceState<'a> = State<'a, Mutex<Workspace>>;
const MAX_ATTEMPTS: usize = 20;
use job_store::{MAX_JOB_BYTES, read_job_record, read_json, write_json};

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
        private_directory(directory, RECORD)?;
        write_text(
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
                    kind: error.kind,
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
                    kind: error.kind,
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

fn retry_covers_budget(
    settings: &ProviderSettings,
    estimate_usd: f64,
    usage: Option<TokenUsage>,
) -> bool {
    let first = usage
        .and_then(|usage| settings.usage_cost_usd(usage))
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

#[derive(ts_rs::TS, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[ts(rename = "GenerationStatus")]
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

#[derive(ts_rs::TS, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationAttempt {
    ordinal: usize,
    status: JobStatus,
    started_at: String,
    finished_at: Option<String>,
    response_id: Option<String>,
    usage: Option<TokenUsage>,
}

impl GenerationAttempt {
    fn note_receipt(
        &mut self,
        result: &Result<ProviderResponse, ProviderError>,
    ) -> Option<TokenUsage> {
        let (response_id, usage) = match result {
            Ok(response) => (Some(response.response_id.clone()), response.usage),
            Err(error) => (error.response_id.clone(), error.usage),
        };
        if response_id.is_some() {
            self.response_id = response_id;
        }
        if usage.is_some() {
            self.usage = usage;
        }
        usage
    }
}

#[derive(ts_rs::TS, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GenerationTarget {
    page_id: Uuid,
    element_id: Uuid,
}

/// One sequential part of a request whose editable documents exceed the source budget.
#[derive(ts_rs::TS, Clone, Serialize, Deserialize)]
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

#[derive(ts_rs::TS, Clone, Serialize, Deserialize)]
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

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize)]
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

pub struct GenerationService {
    store: job_store::GenerationJobStore,
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationJobSummary {
    job_id: Uuid,
    created_at: Option<String>,
    status: JobStatus,
    usage: Option<TokenUsage>,
    summary: String,
    first_request: String,
}
#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationHistory {
    jobs: Vec<GenerationJobSummary>,
    skipped: usize,
    truncated: bool,
}

impl GenerationService {
    pub fn initialize(data: &Path) -> Self {
        Self::from_directory(data.join("generation"))
    }

    fn from_directory(directory: PathBuf) -> Self {
        Self {
            store: job_store::GenerationJobStore::new(directory),
        }
    }

    fn latest(&mut self, project: &Project) -> NativeResult<Option<GenerationJobView>> {
        let latest = self
            .store
            .directory
            .join(project.id.to_string())
            .join("latest.json");
        if !latest.exists() {
            return Ok(None);
        }
        let latest: LatestJob = read_json(&latest, 1024)?;
        let stored = if let Some(record) = self.store.jobs.get(&latest.job_id) {
            Some(record.clone())
        } else {
            read_job_record(&self.store.job_directory(project.id, latest.job_id))?
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
            || record.view.areas.len() > desegnos_core::limits::MAX_GENERATION_REQUESTS
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
            return self.store.persist(record).map(Some);
        }
        if matches!(
            record.view.status,
            JobStatus::Generating | JobStatus::Applying
        ) && !self.store.active.contains(record.view.job_id)
        {
            record.view.message = interrupted_notice("This generation");
            record.transition(JobStatus::Unknown);
            return self.store.persist(record).map(Some);
        }
        if record.view.status == JobStatus::Ready
            && let Err(error) = resolve_current(&record.view, project)
        {
            record.view.message = format!(
                "This candidate is stale: {error} Undo the changed requirements to reuse this candidate, or discard it and generate from the current requirements."
            );
            record.transition(JobStatus::Blocked);
            return self.store.persist(record).map(Some);
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
            return self.store.persist(record).map(Some);
        }
        if self.store.unsaved_jobs.contains(&record.view.job_id) {
            match self.store.persist_retaining(record.clone()) {
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
        self.store.jobs.insert(record.view.job_id, record.clone());
        self.store.trim_cache(record.view.job_id);
        Ok(Some(record.view))
    }

    fn record(&mut self, project: &Project, job_id: Uuid) -> NativeResult<JobRecord> {
        let view = self
            .latest(project)?
            .filter(|view| view.job_id == job_id)
            .ok_or("This is no longer the active generation request for the project.")?;
        self.store
            .jobs
            .get(&view.job_id)
            .cloned()
            .ok_or_else(|| "The generation record is unavailable.".into())
    }

    fn prepare_requests(
        &mut self,
        ai: &AiSettingsStore,
        project: &Project,
        account_id: Option<Uuid>,
        previous: Option<&GenerationJobView>,
        element_ids: Option<&[Uuid]>,
        reference_ids: Option<&[Uuid]>,
    ) -> NativeResult<GenerationJobView> {
        let provider = previous.map_or(ai.provider, |v| v.provider);
        let authentication = previous.map_or(ai.authentication, |v| v.authentication);
        let settings = previous.map_or_else(|| ai.settings.clone(), |v| v.settings.clone());
        if let Some(error) = &ai.configuration_error {
            return Err(error.clone());
        }
        if let Some(error) = &ai.authentication_error {
            return Err(error.clone());
        }
        if (authentication == AuthenticationMethod::Chatgpt) != account_id.is_some()
            || account_id.is_some_and(|id| id.is_nil())
        {
            return Err("Choose a connected ChatGPT account with plan usage enabled before preparing this request.".into());
        }
        ai.client.as_ref().map_err(Clone::clone)?;
        if self.latest(project)?.is_some_and(|view| {
            view.status.active() && previous.is_none_or(|old| old.job_id != view.job_id)
        }) {
            return Err("Apply or discard the existing generation review, or cancel its running request, before starting another.".into());
        }
        let capabilities = provider.capabilities_for(
            &settings,
            ai.selected_image_cache_id(account_id)
                .as_ref()
                .and_then(|id| ai.image_access.get(id)),
        );
        let history = self.store.request_history(project)?;
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
        let estimate_usd = ai
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
        let view = self.store.persist(record)?;
        write_json(
            &self
                .store
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
        ai: &AiSettingsStore,
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
                ai,
                project,
                record.view.chatgpt_account_id,
                Some(&record.view),
                None,
                Some(&record.view.requirements.reference_ids),
            )?;
            return self.begin(ai, project, fresh.job_id, false);
        }
        record
            .view
            .requirements
            .validate_current(project)
            .map_err(message)?;
        let estimate = ai
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
        if self.store.active.len() >= 4 {
            return Err("Four provider requests are already running. Wait for or cancel one before sending another.".into());
        }
        if let Some(candidate) = &record.view.candidate {
            write_json(
                &self.store.job_directory(project.id, job_id).join(format!(
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
                    .store
                    .job_directory(project.id, job_id)
                    .join(format!("attempt-{}-error.json", record.view.attempts.len())),
                error,
            )?;
        }
        let directory = self.store.job_directory(project.id, job_id);
        if let Some(context) = &record.view.context {
            write_text(
                &directory.join(format!("attempt-{}-CONTEXT.md", record.view.attempts.len())),
                &context.markdown,
            )
            .map_err(message)?;
            let path = directory.join("CONTEXT.md");
            if path.exists() {
                regular_file(&path, "generation record")?;
                fs::remove_file(path).map_err(message)?;
            }
        }
        if let Some(reply) = &record.view.raw_reply {
            write_text(
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
            regular_file(&reply_path, "generation record")?;
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
        let view = self.store.persist(record)?;
        let cancellation = self.store.active.start(job_id, ordinal);
        Ok((view, cancellation))
    }

    fn progress(&mut self, job_id: Uuid, ordinal: usize, progress: desegnos_ai::ProviderProgress) {
        if !self.store.active.is_current(job_id, ordinal) {
            return;
        }
        if let Some(record) = self.store.jobs.get_mut(&job_id) {
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
    ) {
        if !self.store.active.is_current(job_id, ordinal) {
            return;
        }
        let Some(record) = self.store.jobs.get_mut(&job_id) else {
            return;
        };
        record.view.message = UNREADABLE_REPLY_RETRY_MESSAGE.into();
        record.view.received_bytes = 0;
        record.view.pages_started.clear();
        record.partial_text.clear();
        record.view.usage = usage;
        record.view.validation_events.push(desegnos_core::ValidationEvent {
            kind: desegnos_core::ValidationEventKind::UnreadableReply,
            disposition: "retried".into(),
            element_id: None,
            message: "The AI reply was not readable JSON, so Desegnos sent the same frozen request once more.".into(),
        });
        let record = record.clone();
        if let Err(error) = self.store.persist_retaining(record)
            && let Some(record) = self.store.jobs.get_mut(&job_id)
        {
            record.view.message = format!(
                "{} Its local record could not be saved: {error}",
                record.view.message
            );
        }
    }

    fn finish(
        &mut self,
        original: &Project,
        job_id: Uuid,
        ordinal: usize,
        result: Result<ProviderResponse, ProviderError>,
    ) -> NativeResult<()> {
        if !self.store.active.is_registered(job_id, ordinal) {
            let mut record = if let Some(record) = self.store.jobs.get(&job_id) {
                record.clone()
            } else {
                read_json::<JobRecord>(
                    &self
                        .store
                        .job_directory(original.id, job_id)
                        .join("job.json"),
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
                let usage = attempt.note_receipt(&result);
                if usage.is_some() && latest {
                    record.view.usage = usage;
                }
                attempt.finished_at.get_or_insert_with(now);
                if latest {
                    self.record_validation_counts(&mut record);
                }
                self.store.persist_retaining(record)?;
            }
            return Ok(());
        }
        let cancelled = self.store.active.is_cancelled(job_id);
        let Some(mut record) = self.store.jobs.get(&job_id).cloned() else {
            return Err("The active generation record is unavailable.".into());
        };
        if record.view.status != JobStatus::Generating || cancelled {
            if let Some(attempt) = record.view.attempts.last_mut() {
                let usage = attempt.note_receipt(&result);
                if usage.is_some() {
                    record.view.usage = usage;
                }
                self.record_validation_counts(&mut record);
                record.transition(record.view.status);
                self.store.persist_retaining(record)?;
            }
            self.store.active.remove(job_id);
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
                    &self.store.job_directory(original.id, job_id),
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
                let status = match Failure::of(&error) {
                    Failure::Cancelled => JobStatus::Cancelled,
                    Failure::Unknown => JobStatus::Unknown,
                    Failure::Failed => JobStatus::Failed,
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
        self.store.persist_retaining(record)?;
        self.store.active.remove(job_id);
        Ok(())
    }

    fn record_validation_counts(&self, record: &mut JobRecord) {
        if record.view.validation_counts_recorded || record.view.validation_events.is_empty() {
            return;
        }
        let counter_path = self.store.directory.join("validation-counts.json");
        let mut counts: BTreeMap<String, u64> = if counter_path.exists() {
            match read_json(&counter_path, 64 * 1024) {
                Ok(counts) => counts,
                Err(error) => {
                    record
                        .view
                        .validation_events
                        .push(desegnos_core::ValidationEvent {
                            kind: desegnos_core::ValidationEventKind::TelemetryStorage,
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
                    kind: desegnos_core::ValidationEventKind::TelemetryStorage,
                    disposition: "conflicted".into(),
                    element_id: None,
                    message: format!("Could not save local validation counts: {error}"),
                });
        }
        record.view.validation_counts_recorded = true;
    }

    fn handle_finish_error(&mut self, job_id: Uuid, ordinal: usize, error: &str) {
        if !self.store.active.is_registered(job_id, ordinal) {
            return;
        }
        if let Some(record) = self.store.jobs.get_mut(&job_id)
            && record.view.status != JobStatus::Cancelled
        {
            record.view.message = format!(
                "Generation stopped while recording its result: {error} No request will be automatically repeated."
            );
            record.transition(JobStatus::Unknown);
        }
        self.store.active.remove(job_id);
    }

    fn cancel(&mut self, project: &Project, job_id: Uuid) -> NativeResult<GenerationJobView> {
        self.store.active.cancel(job_id);
        let mut record = if let Some(record) = self
            .store
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
        record.view.message = stopped_notice(record.view.authentication);
        Ok(self.record_cancelled(record))
    }

    /// Keep a cancelled job even when its record cannot be saved: late output stays blocked
    /// and the message says the receipt is missing.
    fn record_cancelled(&mut self, mut record: JobRecord) -> GenerationJobView {
        record.view.candidate = None;
        record.transition(JobStatus::Cancelled);
        let job_id = record.view.job_id;
        self.store.jobs.insert(job_id, record.clone());
        match self.store.persist(record.clone()) {
            Ok(view) => view,
            Err(error) => {
                record.view.message = unsaved_cancel_notice(&error);
                let view = record.view.clone();
                self.store.jobs.insert(job_id, record);
                view
            }
        }
    }

    pub(crate) fn bind_switch_guard(&mut self, guard: crate::ai_job::SwitchGuard) {
        self.store.active.bind_switch_guard(guard);
    }

    pub(super) fn cancel_account(&mut self, account_id: Uuid) -> NativeResult<()> {
        let jobs: Vec<_> = self
            .store
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
            self.store.active.cancel(job_id);
            let mut record = self
                .store
                .jobs
                .get(&job_id)
                .cloned()
                .ok_or("The account's generation record is unavailable.")?;
            record.view.message = signed_out_notice(AuthenticationMethod::Chatgpt);
            self.record_cancelled(record);
        }
        Ok(())
    }
}

#[tauri::command]
pub async fn prepare_generation(
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    settings: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    element_ids: Option<Vec<Uuid>>,
    reference_ids: Option<Vec<Uuid>>,
) -> NativeResult<GenerationJobView> {
    refresh_image_access(&settings, &chatgpt, false).await?;
    let (authentication, provider) = {
        let service = settings.lock().map_err(message)?;
        (service.authentication, service.provider)
    };
    let account_id = if authentication == AuthenticationMethod::Chatgpt {
        Some(active_account(&chatgpt, "generating").await?)
    } else {
        None
    };
    let workspace = workspace.lock().map_err(message)?;
    let project = workspace.project()?;
    let ai = settings.lock().map_err(message)?;
    let mut service = state.lock().map_err(message)?;
    if ai.authentication != authentication || ai.provider != provider {
        return Err("The generation method changed while preparing this request. Review it and prepare again.".into());
    }
    let references = reference_ids.as_deref();
    if let Some(ids) = element_ids {
        service.prepare_requests(&ai, project, account_id, None, Some(&ids), references)
    } else {
        service.prepare_requests(&ai, project, account_id, None, None, references)
    }
}

#[tauri::command]
pub fn get_generation(
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    project_id: Uuid,
) -> NativeResult<Option<GenerationJobView>> {
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
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    project_id: Uuid,
) -> NativeResult<GenerationHistory> {
    if workspace.lock().map_err(message)?.project()?.id != project_id {
        return Err("The active project changed.".into());
    }
    state.lock().map_err(message)?.store.past_jobs(project_id)
}

#[tauri::command]
pub fn get_past_generation(
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    project_id: Uuid,
    job_id: Uuid,
) -> NativeResult<GenerationJobView> {
    if workspace.lock().map_err(message)?.project()?.id != project_id {
        return Err("The active project changed.".into());
    }
    state
        .lock()
        .map_err(message)?
        .store
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
        service.store.persist(record)?;
    }
    Ok(view)
}

#[allow(clippy::too_many_arguments)]
async fn submit(
    app: AppHandle,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    job_id: Uuid,
    retry: bool,
) -> NativeResult<GenerationJobView> {
    let account_id = {
        let workspace = workspace.lock().map_err(message)?;
        let mut service = state.lock().map_err(message)?;
        service
            .record(workspace.project()?, job_id)?
            .view
            .chatgpt_account_id
    };
    let binding = chatgpt.binding(account_id)?;
    let (original, view, cancellation) = chatgpt.with_binding(binding, || {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?.clone();
        let settings = app.state::<Mutex<AiSettingsStore>>();
        let ai = settings.lock().map_err(message)?;
        let mut service = state.lock().map_err(message)?;
        let (view, cancellation) = service.begin(&ai, &project, job_id, retry)?;
        Ok((project, view, cancellation))
    })?;
    let job_id = view.job_id;
    let request = view.clone();
    tauri::async_runtime::spawn(async move {
        let ordinal = request.attempts.len();
        let result = attempt(&app, &request, ordinal, &cancellation).await;
        record_attempt(&app, &original, job_id, ordinal, result);
    });
    Ok(view)
}

/// Resolve access inside the registered, cancellable attempt, then send the frozen request.
/// Credential refresh and model discovery happen here, before any paid submission.
async fn attempt(
    app: &AppHandle,
    request: &GenerationJobView,
    ordinal: usize,
    cancellation: &CancellationToken,
) -> Result<ProviderResponse, ProviderError> {
    let job_id = request.job_id;
    let connection = Connection {
        provider: request.provider,
        authentication: request.authentication,
        settings: request.settings.clone(),
        account_id: request.chatgpt_account_id,
    };
    let access = resolve(
        &app.state::<Mutex<AiSettingsStore>>(),
        &app.state::<ChatgptState>(),
        &connection,
        cancellation,
    )
    .await
    .map_err(AccessFailure::into_provider_error)?;
    let schema = desegnos_core::result_schema();
    let (access, schema) = (&access, &schema);
    let ask = |requirements: &RequirementsSnapshot| {
        let progress_app = app.clone();
        let requirements = requirements.clone();
        async move {
            access.check(&requirements, request.generation_mode, schema)?;
            access
                .client
                .generate(
                    desegnos_ai::GenerationRequest {
                        authentication: access.authentication,
                        snapshot: &requirements,
                        mode: request.generation_mode,
                        schema,
                        settings: &access.settings,
                        key: &access.key,
                        cancellation,
                    },
                    move |progress| {
                        lock(&progress_app.state::<Mutex<GenerationService>>())
                            .progress(job_id, ordinal, progress);
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
        cancellation,
        &request.settings,
        request.estimate_usd,
        |usage| {
            lock(&app.state::<Mutex<GenerationService>>())
                .retrying_after_unreadable_reply(job_id, ordinal, usage);
        },
    )
    .await;
    result
}

/// Store an attempt's outcome and, when the project is still open, its dossier. A poisoned
/// lock must not leave the job stuck in Generating, so both locks recover.
fn record_attempt(
    app: &AppHandle,
    original: &Project,
    job_id: Uuid,
    ordinal: usize,
    result: Result<ProviderResponse, ProviderError>,
) {
    let workspace_state = app.state::<Mutex<Workspace>>();
    let service_state = app.state::<Mutex<GenerationService>>();
    let mut workspace = lock(&workspace_state);
    let mut service = lock(&service_state);
    let still_open = workspace
        .project()
        .is_ok_and(|project| project.id == original.id);
    if let Err(error) = service.finish(original, job_id, ordinal, result) {
        service.handle_finish_error(job_id, ordinal, &error);
    } else if still_open && let Some(record) = service.store.jobs.get(&job_id) {
        let view = record.view.clone();
        if let Err(error) = maintain_dossier(&mut workspace, &mut service, view)
            && let Some(record) = service.store.jobs.get_mut(&job_id)
        {
            record.view.dossier_error = Some(format!(
                "The result is preserved, but its project dossier could not be saved: {error}"
            ));
        }
    }
}

#[tauri::command]
pub async fn start_generation(
    app: AppHandle,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    job_id: Uuid,
) -> NativeResult<GenerationJobView> {
    submit(app, workspace, state, chatgpt, job_id, false).await
}

#[tauri::command]
pub async fn retry_generation(
    app: AppHandle,
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    job_id: Uuid,
) -> NativeResult<GenerationJobView> {
    submit(app, workspace, state, chatgpt, job_id, true).await
}

#[tauri::command]
pub fn cancel_generation(
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    job_id: Uuid,
) -> NativeResult<GenerationJobView> {
    let workspace = workspace.lock().map_err(message)?;
    let mut service = state.lock().map_err(message)?;
    service.cancel(workspace.project()?, job_id)
}

#[tauri::command]
pub fn discard_generation(
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    job_id: Uuid,
) -> NativeResult<GenerationJobView> {
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
    service.store.persist(record)
}

#[tauri::command]
pub async fn apply_generation(
    workspace: WorkspaceState<'_>,
    state: AiState<'_>,
    settings: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    job_id: Uuid,
    accepted_decision_indices: Vec<usize>,
) -> NativeResult<WorkspaceView> {
    let prepared = {
        let workspace = workspace.lock().map_err(message)?;
        let project = workspace.project()?;
        if project.applied_job_ids.contains(&job_id) {
            return Ok(workspace.view());
        }
        let ai = settings.lock().map_err(message)?;
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
            Some(ai.client.clone())
        };
        let authentication = record.view.authentication;
        let provider = record.view.provider;
        let account_id = record.view.chatgpt_account_id;
        let image_key = if image_requests.is_empty() || provider != ProviderId::Openai {
            None
        } else {
            Some(ai.image_credentials.key())
        };
        let main_key = if image_requests.is_empty() || provider != ProviderId::Openai {
            None
        } else if authentication == AuthenticationMethod::ApiKey {
            Some(ai.credentials.key())
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
    let mut image_failures = Vec::new();
    let generated = if image_requests.is_empty() {
        BTreeMap::new()
    } else {
        let key = match (image_key, main_key) {
            (Some(Ok(key)), _) | (_, Some(Ok(key))) => Ok(key),
            (Some(Err(error)), _) | (_, Some(Err(error))) => Err(message(error)),
            (None, None) if authentication == AuthenticationMethod::Chatgpt => match account_id {
                Some(account) => chatgpt
                    .manager()?
                    .bearer(account, &CancellationToken::new())
                    .await
                    .map_err(message),
                None => Err("This ChatGPT request has no bound account.".to_owned()),
            },
            (None, None) => Err("No key is available for picture generation.".to_owned()),
        };
        match (client, key) {
            (Some(Ok(client)), Ok(key)) => {
                client
                    .generate_requested_images(&image_requests, &key, &CancellationToken::new())
                    .await
            }
            (client, key) => {
                let reason = key
                    .err()
                    .or(client.and_then(Result::err))
                    .unwrap_or_default();
                image_failures.extend(
                    image_requests
                        .iter()
                        .map(|(name, _)| format!("Image {name}: {reason}")),
                );
                BTreeMap::new()
            }
        }
    };
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
    service.store.persist(record)?;
    Ok(workspace.view())
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
    ) -> (
        GenerationService,
        AiSettingsStore,
        Project,
        Uuid,
        Uuid,
        std::path::PathBuf,
    ) {
        let directory =
            std::env::temp_dir().join(format!("desegnos-retry-test-{}", Uuid::new_v4()));
        let mut service = GenerationService::from_directory(directory.clone());
        let mut ai = AiSettingsStore::from_directory(directory.clone()).unwrap();
        ai.settings = settings;
        let (project, element_id) = project_and_element();
        let view = service
            .prepare_requests(&ai, &project, None, None, Some(&[element_id]), Some(&[]))
            .unwrap();
        (service, ai, project, view.job_id, element_id, directory)
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
        let (mut service, ai, project, job_id, element_id, directory) = prepared(settings());
        let requirements = service
            .store
            .jobs
            .get(&job_id)
            .unwrap()
            .view
            .requirements
            .clone();
        let estimate = service.store.jobs.get(&job_id).unwrap().view.estimate_usd;
        let settings = ai.settings.clone();
        let valid = candidate_json(&requirements, element_id);
        let malformed = valid[..valid.len() - 1].to_string();
        let (_, cancellation) = service.begin(&ai, &project, job_id, false).unwrap();
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
                service.retrying_after_unreadable_reply(job_id, 1, Some(usage(100)));
            },
        )
        .await;
        assert!(retried);
        assert_eq!(sends.load(Ordering::SeqCst), 2);
        let result = result.unwrap();
        assert_eq!(result.usage.unwrap().total_tokens, 800);
        service.finish(&project, job_id, 1, Ok(result)).unwrap();
        let view = &service.store.jobs.get(&job_id).unwrap().view;
        assert!(matches!(view.status, JobStatus::Ready));
        assert_eq!(view.usage.unwrap().total_tokens, 800);
        assert!(view.validation_events.iter().any(|event| {
            event.kind == desegnos_core::ValidationEventKind::UnreadableReply
                && event.disposition == "retried"
        }));
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
        let (mut service, ai, project, job_id, element_id, directory) = prepared(settings());
        let requirements = service
            .store
            .jobs
            .get(&job_id)
            .unwrap()
            .view
            .requirements
            .clone();
        let estimate = service.store.jobs.get(&job_id).unwrap().view.estimate_usd;
        let settings = ai.settings.clone();
        let valid = candidate_json(&requirements, element_id);
        let malformed = valid[..valid.len() - 1].to_string();
        let (_, cancellation) = service.begin(&ai, &project, job_id, false).unwrap();
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
                service.retrying_after_unreadable_reply(job_id, 1, Some(usage(100)));
            },
        )
        .await;
        assert!(retried);
        assert_eq!(sends.load(Ordering::SeqCst), 2);
        service.finish(&project, job_id, 1, result).unwrap();
        let view = &service.store.jobs.get(&job_id).unwrap().view;
        assert!(matches!(view.status, JobStatus::Blocked));
        assert!(view.message.starts_with("Desegnos rejected the candidate:"));
        assert_eq!(view.usage.unwrap().total_tokens, 400);
        assert!(view.validation_events.iter().any(|event| {
            event.kind == desegnos_core::ValidationEventKind::UnreadableReply
                && event.disposition == "retried"
        }));
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
