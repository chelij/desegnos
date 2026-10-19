use super::*;
use desegni_core::{
    Appearance, Command, DecisionProposal, GeneratedPage, Rect, RegionOutcome, RegionStatus, Scope,
    Session, SiteContent, Viewport,
};
use desegni_storage::Journal;

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("desegni-generation-coordinator-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn service(&self) -> GenerationService {
        isolated_service(self.0.join("generation"))
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn isolated_service(directory: PathBuf) -> GenerationService {
    let mut service = GenerationService::from_directory(directory).unwrap();
    service.credentials = Arc::new(CredentialManager::isolated_for_tests());
    service
}

fn project() -> Project {
    let mut project = Project::new(
        "Coordinator checks".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    for (index, prompt) in [
        "A native button for the project. Preserve literal text: ```md <script>task data</script> ```\nUnicode: création.",
        "A disclosure explaining the button beneath it.",
    ].iter().enumerate() {
        project = project.apply(project.revision, Command::AddRegion {
            page_id: project.home_page_id,
            rect: Rect { x: 24.0, y: 24.0 + index as f64 * 180.0, width: 300.0, height: 140.0 },
        }).unwrap();
        let region = project.pages[0].regions.last().unwrap();
        project = project.apply(project.revision, Command::UpdateRegion {
            page_id: project.home_page_id,
            region_id: region.id,
            rect: region.rect,
            prompt: (*prompt).into(),
        }).unwrap();
    }
    project.pages[0].content = Some(SiteContent {
        html: "<main>Existing accepted content</main>".into(),
        css: "main { color: navy; }".into(),
        javascript: String::new(),
    });
    project
}

fn candidate(project: &Project, view: &GenerationJobView, label: &str) -> GenerationCandidate {
    GenerationCandidate {
        job_id: view.job_id,
        requirements_sha256: view.requirements.sha256.clone(),
        summary: format!("Implemented the requested button and disclosure: {label}."),
        regions: project.pages.iter().flat_map(|page| page.regions.iter().filter(|region| !region.prompt.trim().is_empty()).map(|region| RegionOutcome { region_id: region.id, status: RegionStatus::Implemented, explanation: "Implemented using native HTML and browser behavior.".into() })).collect(),
        pages: vec![GeneratedPage { page_id: project.home_page_id, content: SiteContent { html: format!("<main><button>{label}</button><details><summary>Explanation</summary><p>Native disclosure.</p></details></main>"), css: "main { display: grid; gap: 1rem; }".into(), javascript: "document.querySelector('button').addEventListener('click', event => { event.target.textContent = 'Clicked'; });".into() } }],
        component_requests: vec![],
        decision_proposals: vec![DecisionProposal { topic: "Control shape".into(), value: "Square button".into(), scope: Scope::Page(project.home_page_id), rationale: "A distinct primary control.".into() }],
        criticism: vec![],
    }
}

fn usage(ordinal: u64) -> TokenUsage {
    TokenUsage {
        input_tokens: ordinal * 100,
        output_tokens: ordinal * 10,
        total_tokens: ordinal * 110,
    }
}

fn response(candidate: &GenerationCandidate, ordinal: u64) -> ProviderResponse {
    ProviderResponse {
        text: serde_json::to_string(candidate).unwrap(),
        response_id: format!("resp_coordinator_attempt_{ordinal}"),
        usage: Some(usage(ordinal)),
    }
}

fn status(view: &GenerationJobView, expected: JobStatus) {
    assert!(
        view.status == expected,
        "expected {}, received {}: {}",
        serde_json::to_string(&expected).unwrap(),
        serde_json::to_string(&view.status).unwrap(),
        view.message
    );
}

fn disk_record(service: &GenerationService, project: &Project, job_id: Uuid) -> JobRecord {
    read_json(
        &service.job_directory(project.id, job_id).join("job.json"),
        MAX_JOB_BYTES,
    )
    .unwrap()
}

#[test]
fn a_result_without_identity_proposals_recovers_its_failed_local_write_without_provider_retry() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    let mut generated = candidate(&project, &prepared, "Retain the complete generated source");
    generated.decision_proposals.clear();
    let job_path = service
        .job_directory(project.id, prepared.job_id)
        .join("job.json");
    let prior_record = fs::read(&job_path).unwrap();
    fs::remove_file(&job_path).unwrap();
    fs::create_dir(&job_path).unwrap();
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(response(&generated, 1)),
        )
        .unwrap();
    assert!(service.unsaved_jobs.contains(&prepared.job_id));
    assert!(!service.active.contains_key(&prepared.job_id));
    assert!(
        service.jobs[&prepared.job_id]
            .view
            .recommended_decision_indices
            .is_empty()
    );
    let mut session = Session::new(project.clone()).unwrap();
    let journal = Journal::new(&temp.0.join("recovery"), project.id).unwrap();
    journal.append(&project).unwrap();
    session
        .record_generation(&prepared.requirements, &generated, 1, |next| {
            journal.append(next)
        })
        .unwrap();
    assert_eq!(session.project().pages, project.pages);
    let still_unsaved = service.latest(session.project()).unwrap().unwrap();
    status(&still_unsaved, JobStatus::Ready);
    assert!(still_unsaved.message.contains("kept in memory"));
    assert_eq!(still_unsaved.candidate, Some(generated.clone()));
    assert!(
        service.unsaved_jobs.contains(&prepared.job_id),
        "another failed write must retain the dirty marker"
    );
    for _ in 0..MAX_CACHED_JOBS + 2 {
        service.prepare(&self::project()).unwrap();
    }
    assert!(
        service.jobs.contains_key(&prepared.job_id),
        "an unsaved paid result cannot be evicted from memory"
    );
    fs::remove_dir(&job_path).unwrap();
    fs::write(&job_path, prior_record).unwrap();
    let saved = service.latest(session.project()).unwrap().unwrap();
    status(&saved, JobStatus::Ready);
    assert!(!saved.message.contains("kept in memory"));
    assert_eq!(saved.candidate, Some(generated.clone()));
    assert!(!service.unsaved_jobs.contains(&prepared.job_id));
    assert_eq!(
        saved.attempts.len(),
        1,
        "a local write retry cannot create a provider attempt"
    );
    assert_eq!(saved.attempts[0].usage, Some(usage(1)));
    drop(service);
    let restarted = temp.service().latest(session.project()).unwrap().unwrap();
    status(&restarted, JobStatus::Ready);
    assert_eq!(restarted.candidate, Some(generated));
    assert_eq!(restarted.attempts.len(), 1);
}

#[test]
fn a_late_cancelled_attempt_write_failure_preserves_the_newer_running_retry_and_usage() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    let (_, first_token) = service.begin(&project, prepared.job_id, false).unwrap();
    service.cancel(&project, prepared.job_id).unwrap();
    let (retried, second_token) = service.begin(&project, prepared.job_id, true).unwrap();
    assert!(first_token.is_cancelled());
    let job_path = service
        .job_directory(project.id, prepared.job_id)
        .join("job.json");
    let prior_record = fs::read(&job_path).unwrap();
    fs::remove_file(&job_path).unwrap();
    fs::create_dir(&job_path).unwrap();
    let old_result = candidate(
        &project,
        &prepared,
        "Cancelled result must remain unavailable",
    );
    let error = service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(response(&old_result, 1)),
        )
        .unwrap_err();
    service.handle_finish_error(prepared.job_id, 1, &error);
    assert_eq!(service.active[&prepared.job_id].0, 2);
    assert!(!second_token.is_cancelled());
    let running = &service.jobs[&prepared.job_id].view;
    status(running, JobStatus::Generating);
    assert!(running.candidate.is_none());
    assert!(running.attempts[0].status == JobStatus::Cancelled);
    assert_eq!(running.attempts[0].usage, Some(usage(1)));
    assert_eq!(
        running.attempts[0].response_id.as_deref(),
        Some("resp_coordinator_attempt_1")
    );
    assert!(service.unsaved_jobs.contains(&prepared.job_id));
    fs::remove_dir(&job_path).unwrap();
    fs::write(&job_path, prior_record).unwrap();
    let recovered_accounting = service.latest(&project).unwrap().unwrap();
    status(&recovered_accounting, JobStatus::Generating);
    assert_eq!(recovered_accounting.attempts[0].usage, Some(usage(1)));
    assert!(!service.unsaved_jobs.contains(&prepared.job_id));
    let accepted_result = candidate(&project, &retried, "Newer retry completes normally");
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            2,
            Ok(response(&accepted_result, 2)),
        )
        .unwrap();
    let finished = service.latest(&project).unwrap().unwrap();
    status(&finished, JobStatus::Ready);
    assert_eq!(finished.candidate, Some(accepted_result.clone()));
    assert_eq!(finished.attempts.len(), 2);
    assert_eq!(finished.attempts[0].usage, Some(usage(1)));
    assert_eq!(finished.attempts[1].usage, Some(usage(2)));
    assert!(finished.attempts[0].status == JobStatus::Cancelled);
    assert!(finished.attempts[1].status == JobStatus::Ready);
    assert!(!service.active.contains_key(&prepared.job_id));
    drop(service);
    let restarted = temp.service().latest(&project).unwrap().unwrap();
    assert_eq!(restarted.candidate, Some(accepted_result));
    assert_eq!(restarted.attempts[0].usage, Some(usage(1)));
    assert_eq!(restarted.attempts[1].usage, Some(usage(2)));
}

#[test]
fn prepare_freezes_exact_requirements_and_settings_before_any_submission() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    status(&prepared, JobStatus::Prepared);
    assert!(prepared.attempts.is_empty());
    assert!(service.active.is_empty());
    assert!(prepared.candidate.is_none());
    let requirements_path = service
        .job_directory(project.id, prepared.job_id)
        .join("REQUIREMENTS.md");
    let frozen = fs::read(&requirements_path).unwrap();
    assert_eq!(frozen, prepared.requirements.markdown.as_bytes());
    assert_eq!(digest(&frozen), prepared.requirements.sha256);
    assert!(String::from_utf8_lossy(&frozen).contains("création"));
    assert!(String::from_utf8_lossy(&frozen).contains("<script>task data</script>"));
    assert_eq!(prepared.generation_mode, project.generation_mode);
    assert!(
        service.prepare(&project).is_err(),
        "a second active request requires an explicit choice"
    );
    service.settings.max_output_tokens = 1024;
    let (submitted, _) = service.begin(&project, prepared.job_id, false).unwrap();
    status(&submitted, JobStatus::Generating);
    assert_eq!(submitted.settings, prepared.settings);
    assert_eq!(submitted.requirements, prepared.requirements);
    assert_eq!(fs::read(requirements_path).unwrap(), frozen);
    let durable = disk_record(&service, &project, submitted.job_id);
    status(&durable.view, JobStatus::Generating);
    assert_eq!(durable.view.attempts.len(), 1);
    assert!(durable.view.attempts[0].status == JobStatus::Generating);
    assert!(!service.credentials.status().configured);
}

#[test]
fn chatgpt_job_keeps_account_and_method_after_preference_change_and_restart() {
    let temp = Temp::new();
    let project = project();
    let account_id = Uuid::new_v4();
    let mut service = temp.service();
    assert_eq!(service.authentication, AuthenticationMethod::ApiKey);
    service
        .set_authentication(AuthenticationMethod::Chatgpt)
        .unwrap();
    assert!(service.prepare_bound(&project, None).is_err());
    assert!(service.prepare_bound(&project, Some(Uuid::nil())).is_err());
    let prepared = service.prepare_bound(&project, Some(account_id)).unwrap();
    assert_eq!(prepared.authentication, AuthenticationMethod::Chatgpt);
    assert_eq!(prepared.chatgpt_account_id, Some(account_id));
    assert_eq!(prepared.estimate_usd, 0.0);
    assert!(prepared.message.contains("plan allowance or credits"));
    service
        .set_authentication(AuthenticationMethod::ApiKey)
        .unwrap();
    drop(service);
    let mut restarted = temp.service();
    assert_eq!(restarted.authentication, AuthenticationMethod::ApiKey);
    let (submitted, _) = restarted.begin(&project, prepared.job_id, false).unwrap();
    assert_eq!(submitted.authentication, AuthenticationMethod::Chatgpt);
    assert_eq!(submitted.chatgpt_account_id, Some(account_id));
    assert_eq!(submitted.requirements, prepared.requirements);
    assert_eq!(submitted.generation_mode, prepared.generation_mode);
    assert!(!restarted.credentials.status().configured);
}

#[test]
fn old_records_default_to_api_key_and_invalid_account_bindings_are_preserved() {
    let temp = Temp::new();
    let project = project();
    let mut service = temp.service();
    let prepared = service.prepare(&project).unwrap();
    let path = service
        .job_directory(project.id, prepared.job_id)
        .join("job.json");
    let mut legacy =
        serde_json::to_value(disk_record(&service, &project, prepared.job_id)).unwrap();
    let view = legacy["view"].as_object_mut().unwrap();
    for field in ["authentication", "chatgptAccountId", "failureKind"] {
        view.remove(field);
    }
    write_json(&path, &legacy).unwrap();
    drop(service);
    let mut restarted = temp.service();
    let restored = restarted.latest(&project).unwrap().unwrap();
    assert_eq!(restored.authentication, AuthenticationMethod::ApiKey);
    assert!(restored.chatgpt_account_id.is_none());
    assert!(restored.failure_kind.is_none());
    legacy["view"]["authentication"] = serde_json::json!("chatgpt");
    write_json(&path, &legacy).unwrap();
    let original = fs::read(&path).unwrap();
    drop(restarted);
    let mut restarted = temp.service();
    assert!(restarted.latest(&project).is_err());
    assert_eq!(fs::read(path).unwrap(), original);
    assert!(restarted.active.is_empty());
}

#[test]
fn damaged_authentication_preference_requires_explicit_selection_without_repairing_it() {
    let temp = Temp::new();
    let mut service = temp.service();
    service
        .set_authentication(AuthenticationMethod::Chatgpt)
        .unwrap();
    let path = service.directory.join("authentication.json");
    let damaged = b"An interrupted preference that must be preserved.";
    fs::write(&path, damaged).unwrap();
    drop(service);
    let mut restarted = temp.service();
    assert!(restarted.authentication_error.is_some());
    assert!(restarted.prepare(&project()).is_err());
    assert_eq!(fs::read(&path).unwrap(), damaged);
    restarted
        .set_authentication(AuthenticationMethod::ApiKey)
        .unwrap();
    assert!(restarted.authentication_error.is_none());
    restarted.prepare(&project()).unwrap();
}

#[test]
fn plan_usage_failure_is_durable_and_explicit_retry_keeps_the_same_account() {
    let temp = Temp::new();
    let project = project();
    let account_id = Uuid::new_v4();
    let mut service = temp.service();
    service
        .set_authentication(AuthenticationMethod::Chatgpt)
        .unwrap();
    let prepared = service.prepare_bound(&project, Some(account_id)).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    let mut error = ProviderError::new(ErrorKind::ChatgptUsageLimit, SubmissionOutcome::Rejected)
        .with_authentication(AuthenticationMethod::Chatgpt);
    error.code = Some("subscription_sharing_usage_limit_exceeded".into());
    error.http_status = Some(429);
    error.request_id = Some("req_native_plan_limit".into());
    service
        .finish(&project, Some(&project), prepared.job_id, 1, Err(error))
        .unwrap();
    drop(service);
    let mut restarted = temp.service();
    let failed = restarted.latest(&project).unwrap().unwrap();
    status(&failed, JobStatus::Failed);
    assert_eq!(failed.failure_kind, Some(ErrorKind::ChatgptUsageLimit));
    assert_eq!(failed.attempts.len(), 1);
    assert!(failed.candidate.is_none());
    assert!(restarted.active.is_empty());
    assert!(restarted.begin(&project, prepared.job_id, false).is_err());
    let (retried, _) = restarted.begin(&project, prepared.job_id, true).unwrap();
    assert_eq!(retried.authentication, AuthenticationMethod::Chatgpt);
    assert_eq!(retried.chatgpt_account_id, Some(account_id));
    assert_eq!(retried.requirements, prepared.requirements);
    assert_eq!(retried.attempts.len(), 2);
    assert!(retried.failure_kind.is_none());
}

#[test]
fn signing_out_cancels_only_bound_jobs_and_blocks_late_candidates() {
    let temp = Temp::new();
    let project = project();
    let account_id = Uuid::new_v4();
    let mut service = temp.service();
    service
        .set_authentication(AuthenticationMethod::Chatgpt)
        .unwrap();
    let prepared = service.prepare_bound(&project, Some(account_id)).unwrap();
    let (_, token) = service.begin(&project, prepared.job_id, false).unwrap();
    service.cancel_account(Uuid::new_v4()).unwrap();
    assert!(!token.is_cancelled());
    service.cancel_account(account_id).unwrap();
    assert!(token.is_cancelled());
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(response(
                &candidate(&project, &prepared, "Late signed-out source"),
                1,
            )),
        )
        .unwrap();
    let cancelled = service.latest(&project).unwrap().unwrap();
    status(&cancelled, JobStatus::Cancelled);
    assert!(cancelled.candidate.is_none());
    assert_eq!(cancelled.attempts.len(), 1);
}

#[test]
fn durable_begin_failure_does_not_authorize_submission_or_advance_attempts() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    let path = service
        .job_directory(project.id, prepared.job_id)
        .join("job.json");
    let original = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(service.begin(&project, prepared.job_id, false).is_err());
    assert!(service.active.is_empty());
    let retained = &service.jobs[&prepared.job_id].view;
    status(retained, JobStatus::Prepared);
    assert!(retained.attempts.is_empty());
    fs::remove_dir(&path).unwrap();
    fs::write(&path, original).unwrap();
    let (submitted, _) = service.begin(&project, prepared.job_id, false).unwrap();
    assert_eq!(submitted.attempts.len(), 1);
    assert_eq!(submitted.attempts[0].ordinal, 1);
}

#[test]
fn changed_frozen_markdown_is_preserved_and_refused_before_submission() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    let path = service
        .job_directory(project.id, prepared.job_id)
        .join("REQUIREMENTS.md");
    let changed = b"A separately edited request that was never disclosed or approved.\n";
    fs::write(&path, changed).unwrap();
    assert!(
        service
            .begin(&project, prepared.job_id, false)
            .err()
            .unwrap()
            .contains("changed on disk")
    );
    assert_eq!(fs::read(&path).unwrap(), changed);
    assert!(service.active.is_empty());
    status(&service.jobs[&prepared.job_id].view, JobStatus::Prepared);
    assert!(service.jobs[&prepared.job_id].view.attempts.is_empty());
    fs::write(&path, prepared.requirements.markdown.as_bytes()).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
}

#[test]
fn interrupted_submission_recovers_unknown_and_requires_an_explicit_retry() {
    let temp = Temp::new();
    let project = project();
    let prepared;
    {
        let mut service = temp.service();
        prepared = service.prepare(&project).unwrap();
        service.begin(&project, prepared.job_id, false).unwrap();
    }
    let mut restarted = temp.service();
    let interrupted = restarted.latest(&project).unwrap().unwrap();
    status(&interrupted, JobStatus::Unknown);
    assert!(restarted.active.is_empty());
    assert_eq!(interrupted.attempts.len(), 1);
    assert!(interrupted.attempts[0].status == JobStatus::Unknown);
    assert!(interrupted.attempts[0].finished_at.is_some());
    assert_eq!(interrupted.requirements, prepared.requirements);
    assert!(restarted.begin(&project, prepared.job_id, false).is_err());
    let (retried, _) = restarted.begin(&project, prepared.job_id, true).unwrap();
    assert_eq!(retried.attempts.len(), 2);
    assert_eq!(retried.attempts[0].ordinal, 1);
    assert!(retried.attempts[0].status == JobStatus::Unknown);
    assert_eq!(retried.attempts[1].ordinal, 2);
    assert_eq!(retried.requirements, prepared.requirements);
}

#[test]
fn cancelled_completion_preserves_usage_without_replacing_the_new_latest_job() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let old = service.prepare(&project).unwrap();
    let (_, cancellation) = service.begin(&project, old.job_id, false).unwrap();
    service.cancel(&project, old.job_id).unwrap();
    assert!(cancellation.is_cancelled());
    let current = service.prepare(&project).unwrap();
    service.begin(&project, current.job_id, false).unwrap();
    service
        .finish(
            &project,
            Some(&project),
            old.job_id,
            1,
            Ok(response(
                &candidate(&project, &old, "Old cancelled source"),
                1,
            )),
        )
        .unwrap();
    let latest = service.latest(&project).unwrap().unwrap();
    assert_eq!(latest.job_id, current.job_id);
    status(&latest, JobStatus::Generating);
    let retained = disk_record(&service, &project, old.job_id);
    status(&retained.view, JobStatus::Cancelled);
    assert!(retained.view.candidate.is_none());
    assert_eq!(retained.view.attempts[0].usage, Some(usage(1)));
    assert_eq!(
        retained.view.attempts[0].response_id.as_deref(),
        Some("resp_coordinator_attempt_1")
    );
    assert!(!service.active.contains_key(&old.job_id));
    assert!(service.active.contains_key(&current.job_id));
    drop(service);
    let latest = temp.service().latest(&project).unwrap().unwrap();
    assert_eq!(latest.job_id, current.job_id);
    status(&latest, JobStatus::Unknown);
}

#[test]
fn late_cancelled_attempt_retains_its_usage_without_touching_the_retried_attempt() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    service.cancel(&project, prepared.job_id).unwrap();
    let (retried, new_token) = service.begin(&project, prepared.job_id, true).unwrap();
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(response(
                &candidate(&project, &prepared, "Cancelled output"),
                1,
            )),
        )
        .unwrap();
    let retained = service.record(&project, prepared.job_id).unwrap().view;
    status(&retained, JobStatus::Generating);
    assert_eq!(retained.attempts.len(), 2);
    assert!(retained.attempts[0].status == JobStatus::Cancelled);
    assert_eq!(retained.attempts[0].usage, Some(usage(1)));
    assert_eq!(
        retained.attempts[0].response_id.as_deref(),
        Some("resp_coordinator_attempt_1")
    );
    assert!(retained.attempts[1].status == JobStatus::Generating);
    assert!(retained.attempts[1].usage.is_none());
    assert!(retained.candidate.is_none());
    assert!(!new_token.is_cancelled());
    assert_eq!(service.active[&prepared.job_id].0, 2);
    let accepted = candidate(&project, &retried, "Retried output");
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            2,
            Ok(response(&accepted, 2)),
        )
        .unwrap();
    let finished = service.latest(&project).unwrap().unwrap();
    status(&finished, JobStatus::Ready);
    assert_eq!(finished.candidate, Some(accepted));
    assert_eq!(finished.attempts[0].usage, Some(usage(1)));
    assert_eq!(finished.attempts[1].usage, Some(usage(2)));
    assert_eq!(finished.usage, Some(usage(2)));
    assert!(service.active.is_empty());
}

#[test]
fn cancelled_error_completion_keeps_reported_paid_attempt_metadata() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    service.cancel(&project, prepared.job_id).unwrap();
    let mut error = ProviderError::new(ErrorKind::Cancelled, SubmissionOutcome::Unknown);
    error.response_id = Some("resp_cancelled_but_billed".into());
    error.usage = Some(usage(1));
    service
        .finish(&project, Some(&project), prepared.job_id, 1, Err(error))
        .unwrap();
    let cancelled = service.latest(&project).unwrap().unwrap();
    status(&cancelled, JobStatus::Cancelled);
    assert!(cancelled.candidate.is_none());
    assert!(cancelled.attempts[0].status == JobStatus::Cancelled);
    assert_eq!(
        cancelled.attempts[0].response_id.as_deref(),
        Some("resp_cancelled_but_billed")
    );
    assert_eq!(cancelled.attempts[0].usage, Some(usage(1)));
    assert!(service.active.is_empty());
}

#[test]
fn cancellation_stops_the_request_even_when_its_local_record_cannot_be_written() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    let (_, token) = service.begin(&project, prepared.job_id, false).unwrap();
    let path = service
        .job_directory(project.id, prepared.job_id)
        .join("job.json");
    let original = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let cancelled = service.cancel(&project, prepared.job_id).unwrap();
    status(&cancelled, JobStatus::Cancelled);
    assert!(token.is_cancelled());
    assert!(cancelled.message.contains("could not be saved"));
    assert!(cancelled.message.contains("Restore writable storage"));
    status(&service.jobs[&prepared.job_id].view, JobStatus::Cancelled);
    assert!(service.jobs[&prepared.job_id].view.candidate.is_none());
    assert!(service.ensure_switch_allowed().is_ok());
    fs::remove_dir(&path).unwrap();
    fs::write(&path, original).unwrap();
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(response(
                &candidate(&project, &prepared, "Cancelled late source"),
                1,
            )),
        )
        .unwrap();
    let retained = disk_record(&service, &project, prepared.job_id);
    status(&retained.view, JobStatus::Cancelled);
    assert!(retained.view.candidate.is_none());
    assert_eq!(retained.view.attempts[0].usage, Some(usage(1)));
    assert!(service.active.is_empty());
    drop(service);
    status(
        &temp.service().latest(&project).unwrap().unwrap(),
        JobStatus::Cancelled,
    );
}

#[test]
fn malformed_out_of_scope_and_conflicted_output_cannot_become_ready() {
    for condition in ["malformed", "omitted", "foreign-page", "conflict"] {
        let temp = Temp::new();
        let mut service = temp.service();
        let project = project();
        let prepared = service.prepare(&project).unwrap();
        service.begin(&project, prepared.job_id, false).unwrap();
        let mut generated = candidate(&project, &prepared, condition);
        match condition {
            "omitted" => {
                generated.regions.pop();
            }
            "foreign-page" => generated.pages[0].page_id = Uuid::new_v4(),
            "conflict" => {
                generated.regions[0].status = RegionStatus::Conflict;
                generated.regions[0].explanation =
                    "The requested remote service is unavailable.".into();
                generated.pages.clear();
            }
            "malformed" => {}
            _ => unreachable!(),
        }
        let mut provider_response = response(&generated, 1);
        if condition == "malformed" {
            provider_response.text.pop();
        }
        service
            .finish(
                &project,
                Some(&project),
                prepared.job_id,
                1,
                Ok(provider_response),
            )
            .unwrap();
        let blocked = service.latest(&project).unwrap().unwrap();
        status(&blocked, JobStatus::Blocked);
        assert_eq!(blocked.candidate.is_some(), condition == "conflict");
        assert_eq!(blocked.usage, Some(usage(1)));
        assert!(service.active.is_empty());
        assert!(project.applied_job_ids.is_empty());
        assert_eq!(
            project.pages[0].content.as_ref().unwrap().html,
            "<main>Existing accepted content</main>"
        );
    }
}

#[test]
fn changed_source_mode_and_identity_block_completed_results_and_stale_retries() {
    for condition in ["source", "mode", "identity"] {
        let temp = Temp::new();
        let mut service = temp.service();
        let original = project();
        let prepared = service.prepare(&original).unwrap();
        service.begin(&original, prepared.job_id, false).unwrap();
        let mut current = original.clone();
        match condition {
            "source" => current.pages[0]
                .content
                .as_mut()
                .unwrap()
                .html
                .push_str("<p>New accepted content</p>"),
            "mode" => current.generation_mode = GenerationMode::Strict,
            "identity" => current.brief.brand_name = "A changed identity".into(),
            _ => unreachable!(),
        }
        current.revision += 1;
        service
            .finish(
                &original,
                Some(&current),
                prepared.job_id,
                1,
                Ok(response(
                    &candidate(&original, &prepared, "Stale output"),
                    1,
                )),
            )
            .unwrap();
        let blocked = service.latest(&current).unwrap().unwrap();
        status(&blocked, JobStatus::Blocked);
        assert!(blocked.candidate.is_some());
        assert!(blocked.message.contains("stale"));
        assert!(service.begin(&current, prepared.job_id, true).is_err());
        assert_eq!(service.jobs[&prepared.job_id].view.attempts.len(), 1);
        assert!(current.applied_job_ids.is_empty());
    }
}

#[test]
fn provider_details_survive_restart_and_are_archived_before_explicit_retry() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    let error: ProviderError = serde_json::from_value(serde_json::json!({
        "kind":"requestRejected","outcome":"received","responseId":null,"usage":null,
        "authentication":"apiKey","httpStatus":200,"code":"unsupported_value","param":"input[0].content",
        "requestId":"req_json_diagnostic","responseFormat":{"contentType":"application/json","bodyKind":"jsonError","bodyBytes":123}
    })).unwrap();
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Err(error.clone()),
        )
        .unwrap();
    drop(service);
    let mut restarted = temp.service();
    let failed = restarted.latest(&project).unwrap().unwrap();
    status(&failed, JobStatus::Failed);
    assert!(failed.candidate.is_none());
    assert_eq!(failed.provider_error, Some(error.clone()));
    assert_eq!(failed.received_bytes, 123);
    assert_eq!(failed.attempts.len(), 1);
    assert_eq!(
        restarted.latest(&project).unwrap().unwrap().attempts.len(),
        1
    );
    let (retried, _) = restarted.begin(&project, prepared.job_id, true).unwrap();
    assert_eq!(retried.provider_error, None);
    assert_eq!(retried.failure_kind, None);
    assert_eq!(retried.requirements, prepared.requirements);
    assert_eq!(retried.attempts.len(), 2);
    let archived: ProviderError = read_json(
        &restarted
            .job_directory(project.id, prepared.job_id)
            .join("attempt-1-error.json"),
        65_536,
    )
    .unwrap();
    assert_eq!(archived, error);
}

#[test]
fn older_error_reply_limits_do_not_claim_the_site_was_too_large_or_rewrite_history() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    let error: ProviderError = serde_json::from_value(serde_json::json!({
        "kind":"outputLimit","outcome":"unknown","responseId":null,"usage":null,"httpStatus":200,
        "responseFormat":{"contentType":null,"bodyKind":"unavailable","bodyBytes":0}
    }))
    .unwrap();
    service
        .finish(&project, Some(&project), prepared.job_id, 1, Err(error))
        .unwrap();
    let path = service
        .job_directory(project.id, prepared.job_id)
        .join("job.json");
    let mut legacy: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let format = legacy["error"]["responseFormat"].as_object_mut().unwrap();
    format.remove("bodyTruncated");
    format.remove("limitBytes");
    let bytes = serde_json::to_vec(&legacy).unwrap();
    fs::write(&path, &bytes).unwrap();
    drop(service);
    let mut restarted = temp.service();
    for _ in 0..2 {
        let view = restarted.latest(&project).unwrap().unwrap();
        assert!(view.message.contains("error-reply reader's 64 KB limit"));
        assert!(view.message.contains("site size and usage are unknown"));
        status(&view, JobStatus::Unknown);
        assert!(view.candidate.is_none());
        assert_eq!(view.requirements, prepared.requirements);
        assert_eq!(view.attempts.len(), 1);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn older_completed_rejections_keep_their_unknown_check_and_original_history() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    let error: ProviderError = serde_json::from_value(serde_json::json!({
        "kind":"malformed","outcome":"received","responseId":"resp_older_completed",
        "usage":{"inputTokens":4452,"outputTokens":3073,"totalTokens":7525},"httpStatus":200,
        "responseFormat":{"contentType":null,"bodyKind":"eventStream","bodyBytes":789659}
    }))
    .unwrap();
    service
        .finish(&project, Some(&project), prepared.job_id, 1, Err(error))
        .unwrap();
    let path = service
        .job_directory(project.id, prepared.job_id)
        .join("job.json");
    let bytes = fs::read(&path).unwrap();
    drop(service);
    let mut restarted = temp.service();
    for _ in 0..2 {
        let view = restarted.latest(&project).unwrap().unwrap();
        assert!(view.message.contains("exact failed check was not recorded"));
        status(&view, JobStatus::Failed);
        assert!(view.candidate.is_none());
        assert_eq!(view.usage.unwrap().total_tokens, 7525);
        assert_eq!(view.requirements, prepared.requirements);
        assert_eq!(view.attempts.len(), 1);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn correlation_details_survive_restart_and_are_archived_before_explicit_retry() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    let error: ProviderError = serde_json::from_value(serde_json::json!({
        "kind":"malformed","outcome":"received","responseId":"resp_rejected_final","usage":null,"httpStatus":200,
        "responseFormat":{"contentType":null,"bodyKind":"eventStream","bodyBytes":512,"rejection":"textLocation",
            "correlation":{"terminalOutput":"array","terminalItems":1,"terminalTextParts":2,
                "streamedParts":3,"finalizedItems":2,"emptyAddedItems":1,"cause":"contentPartAbsent",
                "outputIndex":9,"contentIndex":7}}
    })).unwrap();
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Err(error.clone()),
        )
        .unwrap();
    drop(service);
    let mut restarted = temp.service();
    let view = restarted.latest(&project).unwrap().unwrap();
    assert!(view.message.contains("streamed reply did not match"));
    assert_eq!(view.provider_error, Some(error.clone()));
    assert!(view.candidate.is_none());
    assert_eq!(view.attempts.len(), 1);
    restarted.begin(&project, prepared.job_id, true).unwrap();
    let archived: ProviderError = read_json(
        &restarted
            .job_directory(project.id, prepared.job_id)
            .join("attempt-1-error.json"),
        65_536,
    )
    .unwrap();
    assert_eq!(archived, error);
    assert_eq!(
        restarted.latest(&project).unwrap().unwrap().requirements,
        prepared.requirements
    );
}

#[test]
fn older_non_stream_failures_get_an_honest_message_without_rewriting_the_record() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Err(ProviderError::new(
                ErrorKind::Malformed,
                SubmissionOutcome::Unknown,
            )),
        )
        .unwrap();
    let path = service
        .job_directory(project.id, prepared.job_id)
        .join("job.json");
    let mut legacy: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    legacy["view"]
        .as_object_mut()
        .unwrap()
        .remove("providerError");
    legacy["error"]
        .as_object_mut()
        .unwrap()
        .remove("responseFormat");
    let bytes = serde_json::to_vec(&legacy).unwrap();
    fs::write(&path, &bytes).unwrap();
    drop(service);
    let mut restarted = temp.service();
    for _ in 0..2 {
        let view = restarted.latest(&project).unwrap().unwrap();
        status(&view, JobStatus::Unknown);
        assert!(view.message.contains("non-stream response"));
        assert!(view.message.contains("did not save response details"));
        assert_eq!(view.attempts.len(), 1);
        assert!(view.candidate.is_none());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn uncertain_provider_errors_and_twenty_attempt_limit_never_trigger_automatic_retry() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    for ordinal in 1..=MAX_ATTEMPTS {
        let (attempt, _) = service
            .begin(&project, prepared.job_id, ordinal > 1)
            .unwrap();
        assert_eq!(attempt.attempts.len(), ordinal);
        let mut error = ProviderError::new(ErrorKind::Transport, SubmissionOutcome::Unknown);
        error.response_id = Some(format!("resp_uncertain_{ordinal}"));
        error.usage = Some(usage(ordinal as u64));
        service
            .finish(
                &project,
                Some(&project),
                prepared.job_id,
                ordinal,
                Err(error),
            )
            .unwrap();
        let interrupted = service.latest(&project).unwrap().unwrap();
        status(&interrupted, JobStatus::Unknown);
        assert!(service.active.is_empty());
        assert_eq!(interrupted.attempts.len(), ordinal);
        for (index, prior) in interrupted.attempts.iter().enumerate() {
            assert!(prior.status == JobStatus::Unknown);
            assert_eq!(prior.usage, Some(usage(index as u64 + 1)));
        }
    }
    assert!(
        service
            .begin(&project, prepared.job_id, true)
            .err()
            .unwrap()
            .contains("twenty-attempt")
    );
    assert_eq!(
        disk_record(&service, &project, prepared.job_id)
            .view
            .attempts
            .len(),
        MAX_ATTEMPTS
    );
    drop(service);
    let mut restarted = temp.service();
    status(
        &restarted.latest(&project).unwrap().unwrap(),
        JobStatus::Unknown,
    );
    assert!(restarted.active.is_empty());
    assert!(restarted.begin(&project, prepared.job_id, true).is_err());
}

#[test]
fn cache_eviction_does_not_make_a_persisted_latest_terminal_job_unavailable() {
    let temp = Temp::new();
    let mut service = temp.service();
    let mut records = Vec::new();
    for _ in 0..=MAX_CACHED_JOBS {
        let project = project();
        let prepared = service.prepare(&project).unwrap();
        let mut record = service.record(&project, prepared.job_id).unwrap();
        record.view.status = JobStatus::Failed;
        record.view.message = "A controlled failed attempt; no request was sent.".into();
        service.persist(record).unwrap();
        records.push((project, prepared.job_id));
    }
    assert!(service.jobs.len() <= MAX_CACHED_JOBS);
    let (project, job_id) = records.iter().min_by_key(|(_, job_id)| *job_id).unwrap();
    let loaded = service.record(project, *job_id).unwrap();
    assert_eq!(loaded.view.job_id, *job_id);
    status(&loaded.view, JobStatus::Failed);
    let (retried, _) = service.begin(project, *job_id, true).unwrap();
    assert_eq!(retried.attempts.len(), 1);
    status(&retried, JobStatus::Generating);
    assert!(service.jobs.len() <= MAX_CACHED_JOBS);
}

#[test]
fn durable_applied_job_identity_recovers_completion_without_reapplying_after_undo() {
    let temp = Temp::new();
    let mut service = temp.service();
    let original = project();
    let prepared = service.prepare(&original).unwrap();
    service.begin(&original, prepared.job_id, false).unwrap();
    let generated = candidate(&original, &prepared, "Accepted output");
    service
        .finish(
            &original,
            Some(&original),
            prepared.job_id,
            1,
            Ok(response(&generated, 1)),
        )
        .unwrap();
    let mut record = service.record(&original, prepared.job_id).unwrap();
    status(&record.view, JobStatus::Ready);
    record.view.status = JobStatus::Applying;
    record.view.accepted_decision_indices = vec![0];
    service.persist(record).unwrap();
    let journal = Journal::new(&temp.0.join("recovery"), original.id).unwrap();
    journal.append(&original).unwrap();
    let mut session = Session::new(original.clone()).unwrap();
    session
        .apply_generation(&prepared.requirements, &generated, &[0], |next| {
            journal.append(next)
        })
        .unwrap();
    let committed = Journal::recover(journal.path()).unwrap().unwrap();
    drop(service);
    let mut restarted = temp.service();
    let reconciled = restarted.latest(&committed).unwrap().unwrap();
    status(&reconciled, JobStatus::Applied);
    assert_eq!(reconciled.accepted_decision_indices, vec![0]);
    assert!(restarted.active.is_empty());
    assert!(restarted.begin(&committed, prepared.job_id, true).is_err());
    assert!(restarted.begin(&committed, prepared.job_id, false).is_err());
    session.undo(|next| journal.append(next)).unwrap();
    let undone = Journal::recover(journal.path()).unwrap().unwrap();
    assert_eq!(undone.pages, original.pages);
    assert_eq!(undone.decisions, original.decisions);
    assert_eq!(undone.applied_job_ids, vec![prepared.job_id]);
    status(
        &restarted.latest(&undone).unwrap().unwrap(),
        JobStatus::Applied,
    );
    let mut reopened = Session::new(undone.clone()).unwrap();
    reopened
        .apply_generation(&prepared.requirements, &generated, &[0], |_| {
            panic!("recovery/undo must not duplicate acknowledged application")
        })
        .unwrap();
    assert_eq!(reopened.project(), &undone);
}

#[test]
fn failed_local_ai_configuration_preserves_files_and_does_not_block_local_editing() {
    let temp = Temp::new();
    let generation = temp.0.join("generation");
    fs::create_dir(&generation).unwrap();
    let invalid = b"Incomplete AI settings that must not be silently repaired.\n";
    fs::write(generation.join("settings.json"), invalid).unwrap();
    let mut service = temp.service();
    assert!(service.configuration_error.is_some());
    let project = project();
    assert!(service.prepare(&project).is_err());
    assert_eq!(fs::read(generation.join("settings.json")).unwrap(), invalid);
    let journal = Journal::new(&temp.0.join("recovery"), project.id).unwrap();
    journal.append(&project).unwrap();
    let mut session = Session::new(project.clone()).unwrap();
    session
        .execute(
            project.revision,
            Command::SetGrid { enabled: true },
            |next| journal.append(next),
        )
        .unwrap();
    assert!(session.project().show_grid);
    assert_eq!(
        Journal::recover(journal.path()).unwrap().as_ref(),
        Some(session.project())
    );
    assert!(service.active.is_empty());
    assert!(service.jobs.is_empty());
}

#[test]
fn declining_a_ready_candidate_preserves_paid_history_and_the_accepted_project() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let accepted = project.clone();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(response(
                &candidate(&project, &prepared, "Declined output"),
                1,
            )),
        )
        .unwrap();
    let mut declined = service.record(&project, prepared.job_id).unwrap();
    status(&declined.view, JobStatus::Ready);
    declined.view.status = JobStatus::Discarded;
    declined.view.message = "The user explicitly declined this candidate.".into();
    service.persist(declined).unwrap();
    assert_eq!(project, accepted);
    assert!(service.begin(&project, prepared.job_id, false).is_err());
    assert!(service.begin(&project, prepared.job_id, true).is_err());
    let next = service.prepare(&project).unwrap();
    assert_ne!(next.job_id, prepared.job_id);
    status(&next, JobStatus::Prepared);
    assert!(next.attempts.is_empty());
    let retained = disk_record(&service, &project, prepared.job_id);
    status(&retained.view, JobStatus::Discarded);
    assert_eq!(retained.view.attempts[0].usage, Some(usage(1)));
    assert_eq!(retained.view.requirements, prepared.requirements);
    assert_eq!(
        fs::read(
            service
                .job_directory(project.id, prepared.job_id)
                .join("REQUIREMENTS.md")
        )
        .unwrap(),
        prepared.requirements.markdown.as_bytes()
    );
}

#[test]
fn session_credentials_do_not_enter_disclosure_job_files_or_returned_settings() {
    let temp = Temp::new();
    let mut service = temp.service();
    let sentinel = "coordinator-test-private-sentinel-never-sent-to-a-server";
    service
        .credentials
        .set_key(sentinel.into(), CredentialStorage::Session)
        .unwrap();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    let view = AiSettingsView {
        settings: service.settings.clone(),
        credential: service.credentials.status(),
        catalogue: model_catalogue(),
        configuration_error: service.configuration_error.clone(),
        authentication: AuthenticationMethod::ApiKey,
        chatgpt: ChatgptView::unavailable("Isolated API-key check"),
    };
    assert!(!serde_json::to_string(&view).unwrap().contains(sentinel));
    assert!(!serde_json::to_string(&prepared).unwrap().contains(sentinel));
    let directory = service.job_directory(project.id, prepared.job_id);
    for filename in ["job.json", "REQUIREMENTS.md"] {
        assert!(
            !String::from_utf8(fs::read(directory.join(filename)).unwrap())
                .unwrap()
                .contains(sentinel)
        );
    }
    service.credentials.remove_key().unwrap();
    assert!(!service.credentials.status().configured);
}

#[test]
fn paused_preview_csp_authorizes_only_its_own_frozen_session() {
    use tauri::http::{Request, StatusCode};
    let mut project = project();
    let mut registry = PreviewRegistry::default();
    let enabled = registry
        .register(&project, None, project.home_page_id, true)
        .unwrap();
    let old_script = enabled.url.replace("index.html", "site.js");
    project.pages[0].content.as_mut().unwrap().html =
        format!("<main>Paused</main><script src=\"{old_script}\"></script>");
    let paused = registry
        .register(&project, None, project.home_page_id, false)
        .unwrap();
    let document = registry.serve(&Request::get(&paused.url).body(vec![]).unwrap());
    assert_eq!(document.status(), StatusCode::OK);
    let policy = document.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(policy.contains(&paused.session_id.to_string()));
    assert!(!policy.contains(&enabled.session_id.to_string()));
    assert!(!policy.contains("script-src desegni-preview: "));
    assert_eq!(
        registry
            .serve(&Request::get(old_script).body(vec![]).unwrap())
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        registry
            .serve(
                &Request::get(paused.url.replace("index.html", "site.js"))
                    .body(vec![])
                    .unwrap()
            )
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn undoing_a_stale_dependency_restores_review_without_another_paid_attempt() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    let result = candidate(&project, &prepared, "Complete candidate");
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(response(&result, 1)),
        )
        .unwrap();
    let mut changed = project.clone();
    changed.pages[0].regions[0]
        .prompt
        .push_str(" Changed after sending.");
    status(
        &service.latest(&changed).unwrap().unwrap(),
        JobStatus::Blocked,
    );
    let restored = service.latest(&project).unwrap().unwrap();
    status(&restored, JobStatus::Ready);
    assert_eq!(restored.attempts.len(), 1);
    assert_eq!(restored.requirements, prepared.requirements);
    assert_eq!(restored.candidate, Some(result));
    assert!(service.active.is_empty());
}

#[test]
fn frozen_drawing_areas_reject_invalid_geometry_and_rebuild_legacy_metadata_only_when_current() {
    let temp = Temp::new();
    let project = project();
    let mut service = temp.service();
    let prepared = service.prepare(&project).unwrap();
    assert_eq!(prepared.areas.len(), prepared.requirements.region_count);
    let original = service.jobs[&prepared.job_id].clone();
    for corrupt in [0, 1, 2] {
        let mut record = original.clone();
        match corrupt {
            0 => record.view.areas[0].rect.width = -1.0,
            1 => record.view.areas[0].region_id = Uuid::new_v4(),
            _ => record.view.areas.push(record.view.areas[0].clone()),
        }
        service.jobs.insert(prepared.job_id, record);
        assert!(service.latest(&project).is_err());
    }
    let mut legacy = original.clone();
    legacy.view.areas.clear();
    service.jobs.insert(prepared.job_id, legacy.clone());
    assert_eq!(
        service.latest(&project).unwrap().unwrap().areas.len(),
        original.view.areas.len()
    );
    let mut changed = project.clone();
    changed.pages[0].regions[0].prompt.push_str(" Changed.");
    changed.revision += 1;
    service.jobs.insert(prepared.job_id, legacy);
    assert!(service.latest(&changed).unwrap().unwrap().areas.is_empty());
}

#[test]
fn identity_access_rechecks_account_selection_plan_storage_and_model_after_async_access() {
    let account = Uuid::new_v4();
    let model = ProviderSettings::default().model;
    let mut view = ChatgptView::unavailable("isolated check");
    view.active_account_id = Some(account);
    view.accounts.push(desegni_ai::ChatgptAccount {
        id: account,
        label: "A".into(),
        email: None,
        connected: true,
        plan_enabled: true,
        storage: desegni_ai::CredentialLocation::Session,
        storage_repair_required: false,
    });
    view.models.push(desegni_ai::ChatgptModel {
        id: model.clone(),
        name: model.clone(),
    });
    validate_identity_account(&view, account, &model).unwrap();
    for condition in ["selection", "disconnected", "plan", "repair", "model"] {
        let mut changed = view.clone();
        match condition {
            "selection" => changed.active_account_id = Some(Uuid::new_v4()),
            "disconnected" => changed.accounts[0].connected = false,
            "plan" => changed.accounts[0].plan_enabled = false,
            "repair" => changed.accounts[0].storage_repair_required = true,
            "model" => changed.models.clear(),
            _ => unreachable!(),
        }
        assert!(
            validate_identity_account(&changed, account, &model).is_err(),
            "{condition}"
        );
    }
}

#[cfg(all(debug_assertions, feature = "native-tests"))]
#[test]
fn benchmark_capture_is_opt_in_and_preserves_exact_output_per_attempt() {
    let temp = Temp::new();
    let directory = temp.0.join("benchmark-output");
    let text = "{\n  \"summary\": \"Exact Unicode café\"\n}\n";
    capture_benchmark_output(&directory, 1, text, false).unwrap();
    assert!(!directory.exists());
    capture_benchmark_output(&directory, 1, text, true).unwrap();
    capture_benchmark_output(&directory, 2, "Second attempt", true).unwrap();
    assert_eq!(
        fs::read_to_string(directory.join("provider-output-1.txt")).unwrap(),
        text
    );
    assert_eq!(
        fs::read_to_string(directory.join("provider-output-2.txt")).unwrap(),
        "Second attempt"
    );
}
