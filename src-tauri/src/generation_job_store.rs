use super::{GenerationHistory, GenerationJobSummary, GenerationJobView, JobRecord, JobStatus};
use crate::{
    ai_job::ActiveJobs,
    native_files::{
        self, NativeResult, message, private_directory, read_bytes, regular_directory, regular_file,
    },
};
use desegnos_core::{Project, digest};
use desegnos_storage::write_text;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
pub(super) const MAX_JOB_BYTES: u64 = 768 * 1024 * 1024;
const MAX_CACHED_JOBS: usize = 16;

const RECORD: &str = "generation record";

pub(super) fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> NativeResult<T> {
    native_files::read_json(path, limit, RECORD)
}

pub(super) fn write_json(path: &Path, value: &impl Serialize) -> NativeResult<()> {
    native_files::write_json(path, value, MAX_JOB_BYTES, RECORD)
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

/// A stored job is absent, not an error, when it was frozen by another requirements format or
/// cannot be decoded as one. The current format has no compatibility path, so such records are
/// never shown, resubmitted or upgraded; their files stay untouched on disk.
pub(super) fn read_job_record(directory: &Path) -> NativeResult<Option<JobRecord>> {
    let path = directory.join("job.json");
    if !path.exists() {
        return Ok(None);
    }
    let bytes = read_bytes(&path, MAX_JOB_BYTES, RECORD)?;
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

pub(super) struct GenerationJobStore {
    pub(super) directory: PathBuf,
    pub(super) jobs: BTreeMap<Uuid, JobRecord>,
    pub(super) unsaved_jobs: BTreeSet<Uuid>,
    pub(super) active: ActiveJobs,
}

impl GenerationJobStore {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            jobs: BTreeMap::new(),
            unsaved_jobs: BTreeSet::new(),
            active: ActiveJobs::default(),
        }
    }
    pub(super) fn job_directory(&self, project_id: Uuid, job_id: Uuid) -> PathBuf {
        self.directory
            .join(project_id.to_string())
            .join(job_id.to_string())
    }

    pub(super) fn persist(&mut self, record: JobRecord) -> NativeResult<GenerationJobView> {
        let directory = self.job_directory(record.view.project_id, record.view.job_id);
        private_directory(&directory, RECORD)?;
        // The on-disk REQUIREMENTS.md is the job's first packet; later parts are frozen in the
        // job record with the summaries they carried forward.
        let primary = record
            .view
            .batches
            .first()
            .map_or(&record.view.requirements, |batch| &batch.requirements);
        let markdown = directory.join("REQUIREMENTS.md");
        if markdown.exists() {
            regular_file(&markdown, RECORD)?;
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
            write_text(&markdown, &primary.markdown).map_err(message)?;
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
                regular_file(&path, RECORD)?;
                if fs::metadata(&path).map_err(message)?.len()
                    > desegnos_core::MAX_REQUIREMENTS_BYTES as u64
                    || fs::read(&path).map_err(message)? != context.markdown.as_bytes()
                {
                    return Err(
                        "The saved follow-up context changed. The original was preserved.".into(),
                    );
                }
            } else {
                write_text(&path, &context.markdown).map_err(message)?;
            }
        }
        if let Some(reply) = &record.view.raw_reply {
            if reply.len() > 64 * 1024 {
                return Err("The failed reply exceeds its diagnostic limit.".into());
            }
            write_text(&directory.join("REPLY.txt"), reply).map_err(message)?;
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
    pub(super) fn saved_job(
        &self,
        project_id: Uuid,
        job_id: Uuid,
    ) -> NativeResult<Option<GenerationJobView>> {
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

    pub(super) fn history_path(&self, project_id: Uuid) -> PathBuf {
        self.directory
            .join(project_id.to_string())
            .join("request-history.json")
    }

    /// The derived history index is absent when it was written by another requirements format or
    /// cannot be decoded; the next save replaces it with a current-format index.
    pub(super) fn saved_history(
        &self,
        project_id: Uuid,
    ) -> NativeResult<Vec<desegnos_core::EarlierRequest>> {
        let path = self.history_path(project_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = read_bytes(&path, MAX_JOB_BYTES, RECORD)?;
        let Ok(index) = serde_json::from_slice::<RequestHistoryIndex>(&bytes) else {
            return Ok(Vec::new());
        };
        if index.format != desegnos_core::CURRENT_REQUIREMENTS_FORMAT {
            return Ok(Vec::new());
        }
        Ok(index.entries)
    }

    pub(super) fn history_entries(
        &self,
        view: &GenerationJobView,
    ) -> Vec<desegnos_core::EarlierRequest> {
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
                    view.validation_events
                        .iter()
                        .rev()
                        .find(|e| e.disposition == "conflicted" && e.element_id.is_none())
                })
            {
                desegnos_core::record_validation_rejection(
                    entry,
                    match event.kind {
                        desegnos_core::ValidationEventKind::Element => {
                            desegnos_core::RejectionKind::OutsideFrame
                        }
                        desegnos_core::ValidationEventKind::Pointer => {
                            desegnos_core::RejectionKind::Pointer
                        }
                        desegnos_core::ValidationEventKind::Image => {
                            desegnos_core::RejectionKind::Image
                        }
                        _ => desegnos_core::RejectionKind::Other,
                    },
                    &event.message,
                );
            }
        }
        entries
    }

    pub(super) fn request_history(
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
                regular_directory(&directory, RECORD)?;
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
        private_directory(path.parent().ok_or("Missing history directory.")?, RECORD)?;
        desegnos_core::compact_request_history(&mut entries);
        let index = RequestHistoryIndex {
            format: desegnos_core::CURRENT_REQUIREMENTS_FORMAT,
            entries,
        };
        write_json(&path, &index)?;
        Ok(index.entries)
    }

    pub(super) fn past_jobs(&self, project_id: Uuid) -> NativeResult<GenerationHistory> {
        let directory = self.directory.join(project_id.to_string());
        if !directory.exists() {
            return Ok(GenerationHistory {
                jobs: vec![],
                skipped: 0,
                truncated: false,
            });
        }
        regular_directory(&directory, RECORD)?;
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
                    let first_request = view.requirements.first_request_text();
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

    pub(super) fn persist_retaining(
        &mut self,
        record: JobRecord,
    ) -> NativeResult<GenerationJobView> {
        match self.persist(record.clone()) {
            Ok(view) => Ok(view),
            Err(error) => {
                self.unsaved_jobs.insert(record.view.job_id);
                self.jobs.insert(record.view.job_id, record);
                Err(error)
            }
        }
    }

    pub(super) fn trim_cache(&mut self, protected_job_id: Uuid) {
        while self.jobs.len() > MAX_CACHED_JOBS {
            let expired = self
                .jobs
                .keys()
                .find(|id| {
                    **id != protected_job_id
                        && !self.active.contains(**id)
                        && !self.unsaved_jobs.contains(id)
                })
                .copied();
            let Some(expired) = expired else {
                break;
            };
            self.jobs.remove(&expired);
        }
    }
}
