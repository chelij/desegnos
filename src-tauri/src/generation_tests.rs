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
    service.image_credentials = Arc::new(CredentialManager::isolated_for_tests());
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
    let source = project.pages[0].content.clone().unwrap_or(SiteContent {
        html: String::new(),
        css: String::new(),
        javascript: String::new(),
    });
    let roots = project.pages[0].regions.iter().map(|region| format!("<section data-desegnos-region=\"{}\"><button>{label}</button><details><summary>Explanation</summary><p>Native disclosure.</p></details></section>", region.id)).collect::<String>();
    GenerationCandidate {
        job_id: view.job_id,
        requirements_sha256: view.requirements.sha256.clone(),
        summary: format!("Implemented the requested button and disclosure: {label}."),
        regions: project
            .pages
            .iter()
            .flat_map(|page| {
                page.regions
                    .iter()
                    .filter(|region| !region.prompt.trim().is_empty())
                    .map(|region| RegionOutcome {
                        region_id: region.id,
                        status: RegionStatus::Implemented,
                        explanation: "Implemented using native HTML and browser behavior.".into(),
                    })
            })
            .collect(),
        pages: vec![GeneratedPage {
            page_id: project.home_page_id,
            content: SiteContent {
                html: format!("{}{roots}", source.html),
                css: source.css,
                javascript: source.javascript,
            },
        }],
        component_requests: vec![],
        decision_proposals: vec![DecisionProposal {
            topic: "Control shape".into(),
            value: "Square button".into(),
            scope: Scope::Page(project.home_page_id),
            rationale: "A distinct primary control.".into(),
        }],
        criticism: vec![],

        documents: vec![],
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
fn malformed_foreign_page_and_all_conflicted_output_cannot_become_ready() {
    for condition in ["malformed", "foreign-page", "conflict"] {
        let temp = Temp::new();
        let mut service = temp.service();
        let project = project();
        let prepared = service.prepare(&project).unwrap();
        service.begin(&project, prepared.job_id, false).unwrap();
        let mut generated = candidate(&project, &prepared, condition);
        match condition {
            "foreign-page" => generated.pages[0].page_id = Uuid::new_v4(),
            "conflict" => {
                for region in &mut generated.regions {
                    region.status = RegionStatus::Conflict;
                    region.explanation = "The requested remote service is unavailable.".into();
                }
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
fn omitted_and_invalid_regions_recover_without_applying_unsafe_source() {
    for condition in ["omitted", "outside-scope"] {
        let temp = Temp::new();
        let mut service = temp.service();
        let mut project = project();
        project.generation_mode = GenerationMode::Strict;
        let first = project.pages[0].regions[0].id;
        let second = project.pages[0].regions[1].id;
        // Map both existing areas so recovery can prove what must stay unchanged.
        project.pages[0].content.as_mut().unwrap().html = format!(
            "<main>Existing accepted content</main><section data-desegnos-region=\"{first}\">Original first</section><section data-desegnos-region=\"{second}\">Original second</section>"
        );
        let before = project.clone();
        let prepared = service.prepare(&project).unwrap();
        service.begin(&project, prepared.job_id, false).unwrap();
        let mut generated = candidate(&project, &prepared, "Recovered first");
        generated.pages[0].content.html = format!(
            "<main>UNSAFE OUTSIDE CHANGE</main><section data-desegnos-region=\"{first}\">Recovered first</section><section data-desegnos-region=\"{second}\">Unwanted second</section>"
        );
        if condition == "outside-scope" {
            generated.pages[0].content.html = generated.pages[0].content.html.replace(
                "Unwanted second",
                &format!("<section data-desegnos-region=\"{second}\">Duplicate root</section>"),
            );
        }
        if condition == "omitted" {
            generated.regions.pop();
        }
        service
            .finish(
                &project,
                Some(&project),
                prepared.job_id,
                1,
                Ok(response(&generated, 1)),
            )
            .unwrap();
        let ready = service.latest(&project).unwrap().unwrap();
        status(&ready, JobStatus::Ready);
        assert_eq!(ready.usage, Some(usage(1)));
        assert!(service.active.is_empty());
        assert_eq!(project, before);
        let recovered = ready.candidate.unwrap();
        recovered
            .validate(&prepared.requirements, &project)
            .unwrap();
        assert_eq!(recovered.regions[0].status, RegionStatus::Implemented);
        let second_outcome = recovered
            .regions
            .iter()
            .find(|r| r.region_id == second)
            .unwrap();
        if condition == "omitted" {
            assert_eq!(second_outcome.status, RegionStatus::Implemented);
            assert!(
                ready
                    .validation_events
                    .iter()
                    .any(|e| e.kind == "omittedRegion" && e.region_id == Some(second))
            );
        } else {
            assert_eq!(second_outcome.status, RegionStatus::Conflict);
            assert!(!second_outcome.explanation.is_empty());
            assert!(
                ready
                    .validation_events
                    .iter()
                    .any(|e| e.disposition == "conflicted" && e.region_id == Some(second))
            );
        }
        let applied = project
            .apply_generation(&prepared.requirements, &recovered, &[])
            .unwrap();
        let source = applied.pages[0].content.as_ref().unwrap();
        assert!(source.html.contains("Existing accepted content"));
        assert!(source.html.contains("Recovered first"));
        assert!(source.html.contains("Original second"));
        assert!(!source.html.contains("UNSAFE"));
        assert!(!source.html.contains("Unwanted"));
        assert!(!source.html.contains("inventedid"));
        assert_eq!(
            source.css,
            desegni_core::format_site_content(project.pages[0].content.as_ref().unwrap()).css
        );
        assert_eq!(
            source.javascript,
            project.pages[0].content.as_ref().unwrap().javascript
        );
        assert!(applied.pages[0].regions[0].completed);
        assert_eq!(
            applied.pages[0].regions[1].completed,
            condition == "omitted"
        );
        assert!(applied.applied_job_ids.contains(&prepared.job_id));
    }
}

#[test]
fn changed_source_mode_and_identity_block_old_results_but_retry_current_inputs() {
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
        let (retried, _) = service.begin(&current, prepared.job_id, true).unwrap();
        assert_ne!(retried.job_id, prepared.job_id);
        assert_eq!(retried.base_revision, current.revision);
        retried.requirements.validate_current(&current).unwrap();
        assert_eq!(retried.generation_mode, current.generation_mode);
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
        image_credential: None,
        image_key: CredentialManager::isolated_for_tests().status(),
        main_image_access: None,
        image_key_access: None,
        image_access: None,
        image_access_checking: false,
        provider: ProviderId::Openai,
        providers: provider_catalogue(),
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

#[test]
fn provider_selection_survives_restart_and_keeps_keys_and_settings_separate() {
    let temp = Temp::new();
    let mut service = temp.service();
    service
        .credentials
        .set_key("openai-session-fixture".into(), CredentialStorage::Session)
        .unwrap();
    service
        .select_provider(ProviderId::Anthropic, true)
        .unwrap();
    assert_eq!(service.settings.model, "claude-sonnet-5-5");
    assert!(!service.credentials.status().configured);
    service.credentials = Arc::new(CredentialManager::isolated_for_tests());
    service.image_credentials = Arc::new(CredentialManager::isolated_for_tests());
    service
        .credentials
        .set_key(
            "anthropic-session-fixture".into(),
            CredentialStorage::Session,
        )
        .unwrap();
    service.select_provider(ProviderId::Openai, true).unwrap();
    assert_eq!(service.settings.model, desegni_ai::MODEL_ID);
    assert!(service.credentials.status().configured);
    service
        .select_provider(ProviderId::Anthropic, true)
        .unwrap();
    assert!(service.credentials.status().configured);
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    assert_eq!(prepared.provider, ProviderId::Anthropic);
    assert_eq!(
        prepared.requirements.capabilities,
        Some(ProviderId::Anthropic.capabilities())
    );
    service.select_provider(ProviderId::Google, true).unwrap();
    let mut restarted = GenerationService::from_directory(service.directory.clone()).unwrap();
    assert_eq!(restarted.provider, ProviderId::Google);
    assert_eq!(restarted.settings.model, "gemini-3.8-flash");
    assert_eq!(
        restarted.latest(&project).unwrap().unwrap().provider,
        ProviderId::Anthropic
    );
    assert!(restarted.select_provider(ProviderId::Local, true).is_err());
}

const LEGACY_SELECTION_ARCHIVE: &[u8] =
    include_bytes!("../tests/fixtures/schema6-format9-selection.desegnos");
const LEGACY_SELECTION_JOB: &[u8] =
    include_bytes!("../tests/fixtures/schema6-format9-selection.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacySelectionFixture {
    provenance: String,
    project: serde_json::Value,
    project_markdown: String,
    prepared_record: JobRecord,
    ready_record: JobRecord,
}

fn legacy_selection_fixture() -> LegacySelectionFixture {
    assert_eq!(
        digest(LEGACY_SELECTION_ARCHIVE),
        "f82d969576c6e96e9a82f2e641a707609998e5723cf58d9e4a50b7b5891e1956"
    );
    assert_eq!(
        digest(LEGACY_SELECTION_JOB),
        "d2fe25c4e17dc4a1bdbbb1306d03dda9178f028f33caa069fd102820a1b76b0e"
    );
    let fixture: LegacySelectionFixture = serde_json::from_slice(LEGACY_SELECTION_JOB).unwrap();
    assert!(fixture.provenance.contains("06613a1"));
    assert_eq!(fixture.project["schemaVersion"], 6);
    assert_eq!(
        fixture
            .ready_record
            .view
            .requirements
            .format_version()
            .unwrap(),
        9
    );
    fixture
}

fn install_frozen_job(temp: &Temp, record: JobRecord) -> (Uuid, Uuid, PathBuf) {
    let mut service = temp.service();
    let project_id = record.view.project_id;
    let job_id = record.view.job_id;
    let directory = service.job_directory(project_id, job_id);
    service.persist(record).unwrap();
    write_json(
        &service
            .directory
            .join(project_id.to_string())
            .join("latest.json"),
        &LatestJob { job_id },
    )
    .unwrap();
    (project_id, job_id, directory)
}

fn workspace_journal(directory: &Path) -> (PathBuf, Vec<u8>) {
    let path = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "journal")
        })
        .unwrap();
    let bytes = fs::read(&path).unwrap();
    (path, bytes)
}

#[test]
fn schema_six_ready_format_nine_job_survives_migration_without_relabeling_or_applying_stale_source()
{
    let temp = Temp::new();
    let fixture = legacy_selection_fixture();
    let expected_requirements = fixture.ready_record.view.requirements.clone();
    let expected_candidate = fixture.ready_record.view.candidate.clone().unwrap();
    let expected_attempts = serde_json::to_value(&fixture.ready_record.view.attempts).unwrap();
    let expected_usage = fixture.ready_record.view.usage;
    let (project_id, job_id, job_directory) = install_frozen_job(&temp, fixture.ready_record);
    let markdown_path = job_directory.join("REQUIREMENTS.md");
    let packet = fs::read(&markdown_path).unwrap();
    assert_eq!(packet, expected_requirements.markdown.as_bytes());
    let archive_path = temp.0.join("legacy.desegnos");
    fs::write(&archive_path, LEGACY_SELECTION_ARCHIVE).unwrap();
    let document = desegni_storage::load_project_document(&archive_path).unwrap();
    assert_eq!(document.source_schema_version, 6);
    assert_eq!(document.source_project_markdown, fixture.project_markdown);
    let workspace_directory = temp.0.join("workspace");
    let mut workspace =
        crate::workspace::open_archive_for_tests(workspace_directory.clone(), &archive_path);
    let before = workspace.project().unwrap().clone();
    assert_eq!(before.schema_version, desegni_core::SCHEMA_VERSION);
    assert_eq!(before.id, project_id);
    let region = &before.pages[0].regions[0];
    assert_eq!(
        region.prompt,
        fixture.project["pages"][0]["regions"][0]["prompt"]
            .as_str()
            .unwrap()
    );
    assert!(!region.completed);
    assert_eq!(region.elements.len(), 1);
    assert!(!region.elements[0].id.is_empty());
    assert_eq!(
        region.elements[0].selector,
        fixture.project["pages"][0]["regions"][0]["element"]["selector"]
            .as_str()
            .unwrap()
    );
    before
        .inspect_element(before.home_page_id, &region.elements[0])
        .unwrap();
    let (journal_path, journal_bytes) = workspace_journal(&workspace_directory);
    let mut service = temp.service();
    // This is the same read/maintenance sequence used by get_generation, without a GUI window.
    let view = service
        .latest(workspace.project().unwrap())
        .unwrap()
        .unwrap();
    let view = maintain_dossier(&mut workspace, &mut service, view).unwrap();
    status(&view, JobStatus::Blocked);
    assert!(view.message.contains("stale"));
    assert_eq!(view.requirements, expected_requirements);
    assert_eq!(view.candidate, Some(expected_candidate.clone()));
    assert_eq!(view.usage, expected_usage);
    assert_eq!(
        serde_json::to_value(&view.attempts).unwrap(),
        expected_attempts
    );
    assert!(service.active.is_empty());
    assert_eq!(workspace.project().unwrap(), &before);
    assert_eq!(fs::read(&journal_path).unwrap(), journal_bytes);
    let error = workspace
        .apply_generated(&view.requirements, &expected_candidate, &[])
        .unwrap_err();
    assert!(
        error.contains("changed") || error.contains("stale"),
        "{error}"
    );
    assert_eq!(workspace.project().unwrap(), &before);
    assert_eq!(fs::read(&journal_path).unwrap(), journal_bytes);
    assert_eq!(fs::read(&archive_path).unwrap(), LEGACY_SELECTION_ARCHIVE);
    assert_eq!(fs::read(&markdown_path).unwrap(), packet);
    // Retrying the migrated job submits a fresh packet and preserves the blocked record.
    let job_path = job_directory.join("job.json");
    let job_bytes = fs::read(&job_path).unwrap();
    let (fresh, _) = service.begin(&before, job_id, true).unwrap();
    assert_ne!(fresh.job_id, job_id);
    status(&fresh, JobStatus::Generating);
    assert_eq!(fresh.base_revision, before.revision);
    fresh.requirements.validate_current(&before).unwrap();
    assert_eq!(fresh.requirements.format_version().unwrap(), 22);
    assert_eq!(
        fresh.source_fingerprints.get(&before.pages[0].id),
        Some(&desegni_core::source_fingerprints(
            before.pages[0].content.as_ref()
        ))
    );
    let source_key = format!("source:{}", before.pages[0].id);
    assert_ne!(
        fresh.requirements.dependencies[&source_key],
        expected_requirements.dependencies[&source_key]
    );
    assert!(fresh.requirements.markdown.contains(&region.elements[0].id));
    assert_eq!(fresh.attempts.len(), 1);
    assert_eq!(service.active.len(), 1);
    assert!(service.active.contains_key(&fresh.job_id));
    assert_eq!(fs::read(&job_path).unwrap(), job_bytes);
    let persisted = disk_record(&service, &before, job_id);
    assert_eq!(persisted.view.requirements, expected_requirements);
    assert_eq!(persisted.view.attempts.len(), 1);
    drop(service);
    let mut restarted = temp.service();
    assert_eq!(
        restarted.latest(&before).unwrap().unwrap().job_id,
        fresh.job_id
    );
    let restored = restarted.saved_job(project_id, job_id).unwrap();
    status(&restored, JobStatus::Blocked);
    assert_eq!(restored.requirements, expected_requirements);
    assert_eq!(restored.candidate, Some(expected_candidate));
    assert_eq!(
        serde_json::to_value(&restored.attempts).unwrap(),
        expected_attempts
    );
    assert_eq!(fs::read(markdown_path).unwrap(), packet);
    assert_eq!(fs::read(job_path).unwrap(), job_bytes);
    assert!(restarted.active.is_empty());
}

#[test]
fn schema_six_prepared_format_nine_job_is_preserved_while_begin_submits_current_source() {
    let temp = Temp::new();
    let fixture = legacy_selection_fixture();
    let frozen = fixture.prepared_record.view.requirements.clone();
    let (project_id, job_id, job_directory) = install_frozen_job(&temp, fixture.prepared_record);
    let job_path = job_directory.join("job.json");
    let job_bytes = fs::read(&job_path).unwrap();
    let packet = fs::read(job_directory.join("REQUIREMENTS.md")).unwrap();
    let archive_path = temp.0.join("legacy.desegnos");
    fs::write(&archive_path, LEGACY_SELECTION_ARCHIVE).unwrap();
    let mut workspace =
        crate::workspace::open_archive_for_tests(temp.0.join("workspace"), &archive_path);
    let project = workspace.project().unwrap().clone();
    assert_eq!(project.id, project_id);
    let mut service = temp.service();
    let view = service.latest(&project).unwrap().unwrap();
    let view = maintain_dossier(&mut workspace, &mut service, view).unwrap();
    status(&view, JobStatus::Prepared);
    assert_eq!(view.requirements, frozen);
    assert!(view.attempts.is_empty());
    let (fresh, _) = service.begin(&project, job_id, false).unwrap();
    assert_ne!(fresh.job_id, job_id);
    status(&fresh, JobStatus::Generating);
    assert_eq!(fresh.base_revision, project.revision);
    fresh.requirements.validate_current(&project).unwrap();
    assert_eq!(fresh.requirements.format_version().unwrap(), 22);
    assert_eq!(
        fresh.source_fingerprints.get(&project.pages[0].id),
        Some(&desegni_core::source_fingerprints(
            project.pages[0].content.as_ref()
        ))
    );
    let source_key = format!("source:{}", project.pages[0].id);
    assert_ne!(
        fresh.requirements.dependencies[&source_key],
        frozen.dependencies[&source_key]
    );
    assert!(
        fresh
            .requirements
            .markdown
            .contains(&project.pages[0].regions[0].elements[0].id)
    );
    assert_eq!(fresh.attempts.len(), 1);
    assert_eq!(service.active.len(), 1);
    assert!(service.active.contains_key(&fresh.job_id));
    let old = disk_record(&service, &project, job_id);
    status(&old.view, JobStatus::Prepared);
    assert_eq!(old.view.requirements, frozen);
    assert!(old.view.attempts.is_empty());
    assert!(old.view.candidate.is_none());
    assert_eq!(fs::read(&job_path).unwrap(), job_bytes);
    assert_eq!(
        fs::read(job_directory.join("REQUIREMENTS.md")).unwrap(),
        packet
    );
    assert_eq!(workspace.project().unwrap(), &project);
    assert_eq!(fs::read(&archive_path).unwrap(), LEGACY_SELECTION_ARCHIVE);
}

#[test]
fn saved_context_failed_replies_and_past_jobs_survive_restart_without_changing_latest() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let first = service.prepare(&project).unwrap();
    let mut record = service.record(&project, first.job_id).unwrap();
    let mut context = first.requirements.clone();
    context
        .markdown
        .push_str("\n## Requested context\n\nReference-only elements.\n");
    context.sha256 = digest(context.markdown.as_bytes());
    record.view.context = Some(context.clone());
    service.persist(record).unwrap();
    let directory = service.job_directory(project.id, first.job_id);
    assert_eq!(
        fs::read_to_string(directory.join("CONTEXT.md")).unwrap(),
        context.markdown
    );
    let (_, _) = service.begin(&project, first.job_id, false).unwrap();
    service
        .finish(
            &project,
            Some(&project),
            first.job_id,
            1,
            Ok(ProviderResponse {
                text: "invalid reply with api_key=private-test-secret".into(),
                response_id: "fixture_reply".into(),
                usage: None,
            }),
        )
        .unwrap();
    let failed = service.saved_job(project.id, first.job_id).unwrap();
    assert!(failed.raw_reply.as_ref().unwrap().contains("[redacted]"));
    assert!(
        !fs::read_to_string(directory.join("REPLY.txt"))
            .unwrap()
            .contains("private-test-secret")
    );
    assert!(directory.join("attempt-0-CONTEXT.md").exists());
    let mut record = service.record(&project, first.job_id).unwrap();
    record.view.status = JobStatus::Discarded;
    service.persist(record).unwrap();
    let next = service.prepare(&project).unwrap();
    drop(service);
    let mut reopened = temp.service();
    let history = reopened.past_jobs(project.id).unwrap();
    assert_eq!(history.jobs.len(), 2);
    assert!(!history.jobs[0].first_request.is_empty());
    assert!(
        reopened
            .saved_job(project.id, first.job_id)
            .unwrap()
            .raw_reply
            .is_some()
    );
    assert_eq!(
        reopened.latest(&project).unwrap().unwrap().job_id,
        next.job_id
    );
    assert!(reopened.saved_job(Uuid::new_v4(), first.job_id).is_err());
}

#[test]
fn independent_regions_make_partial_jobs_ready_and_persist_conflicts_after_apply() {
    let temp = Temp::new();
    let mut service = temp.service();
    let mut project = project();
    project.pages[0].content = None;
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    let mut generated = candidate(&project, &prepared, "ready region");
    let ready = generated.regions[0].region_id;
    let conflict = generated.regions[1].region_id;
    generated.regions[1].status = RegionStatus::Conflict;
    generated.regions[1].explanation = "Which private account should this use?".into();
    generated.pages[0].content = SiteContent {
        html: format!(
            "<section data-desegnos-region='{ready}'><p>Ready content</p></section><section data-desegnos-region='{conflict}' data-desegnos-placeholder='conflict'>Account needed</section>"
        ),
        css: String::new(),
        javascript: String::new(),
    };
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(response(&generated, 1)),
        )
        .unwrap();
    let view = service.latest(&project).unwrap().unwrap();
    status(&view, JobStatus::Ready);
    let applied = project
        .apply_generation(&view.requirements, view.candidate.as_ref().unwrap(), &[])
        .unwrap();
    assert_eq!(applied.pending_count(), 1);
    assert!(applied.pages[0].regions[0].completed);
    assert!(!applied.pages[0].regions[1].completed);
    let restored = temp.service().latest(&applied).unwrap().unwrap();
    status(&restored, JobStatus::Applied);
    assert_eq!(
        restored.candidate.unwrap().regions[1].explanation,
        "Which private account should this use?"
    );
}

#[test]
fn stopped_prepared_job_cannot_start_and_remains_stopped_on_disk() {
    let temp = Temp::new();
    let project = project();
    let mut service = temp.service();
    let prepared = service.prepare(&project).unwrap();
    let stopped = service.cancel(&project, prepared.job_id).unwrap();
    status(&stopped, JobStatus::Cancelled);
    assert!(stopped.message.starts_with("Stopped"));
    assert!(service.begin(&project, prepared.job_id, false).is_err());
    assert!(service.active.is_empty());
    assert!(
        disk_record(&service, &project, prepared.job_id)
            .view
            .attempts
            .is_empty()
    );
    status(
        &temp.service().latest(&project).unwrap().unwrap(),
        JobStatus::Cancelled,
    );
}

#[test]
fn stop_after_response_preparation_clears_candidate_and_blocks_late_image_results() {
    let temp = Temp::new();
    let project = project();
    let before = project.clone();
    let mut service = temp.service();
    let prepared = service.prepare(&project).unwrap();
    let (_, token) = service.begin(&project, prepared.job_id, false).unwrap();
    let late = response(&candidate(&project, &prepared, "Late image output"), 1);
    service.cancel(&project, prepared.job_id).unwrap();
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(late.clone()),
        )
        .unwrap();
    let stopped = service.latest(&project).unwrap().unwrap();
    status(&stopped, JobStatus::Cancelled);
    assert!(token.is_cancelled());
    assert!(stopped.candidate.is_none());
    assert_eq!(project, before);
    assert!(stopped.status.retryable());

    // Also cover Stop between Ready and a queued automatic apply.
    service.begin(&project, prepared.job_id, true).unwrap();
    service
        .finish(&project, Some(&project), prepared.job_id, 2, Ok(late))
        .unwrap();
    status(
        &service.latest(&project).unwrap().unwrap(),
        JobStatus::Ready,
    );
    let stopped = service.cancel(&project, prepared.job_id).unwrap();
    status(&stopped, JobStatus::Cancelled);
    assert!(stopped.candidate.is_none());
    assert!(
        disk_record(&service, &project, prepared.job_id)
            .view
            .candidate
            .is_none()
    );
    assert!(
        service.cancel(&project, prepared.job_id).is_ok(),
        "Stop is idempotent"
    );
}

#[test]
fn a_finish_record_error_does_not_replace_stopped_with_unknown() {
    let temp = Temp::new();
    let project = project();
    let mut service = temp.service();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    service.cancel(&project, prepared.job_id).unwrap();
    service.handle_finish_error(prepared.job_id, 1, "Storage became unwritable");
    status(&service.jobs[&prepared.job_id].view, JobStatus::Cancelled);
    assert!(service.jobs[&prepared.job_id].view.candidate.is_none());
    assert!(service.active.is_empty());
}

#[test]
fn runtime_image_rejection_is_persisted_per_account_and_credential_without_secrets() {
    let temp = Temp::new();
    let mut service = temp.service();
    service
        .credentials
        .set_key("sk-first-image-account".into(), CredentialStorage::Session)
        .unwrap();
    let first = service.image_cache_id(None).unwrap();
    service
        .store_image_access(
            first.clone(),
            desegni_ai::ImageAccess::new(desegni_ai::ImageAvailability::No, "image_api_rejected"),
        )
        .unwrap();
    service
        .credentials
        .set_key("sk-second-image-account".into(), CredentialStorage::Session)
        .unwrap();
    let second = service.image_cache_id(None).unwrap();
    assert_ne!(first, second);
    assert!(!service.image_access.contains_key(&second));
    let account = Uuid::new_v4();
    service.authentication = AuthenticationMethod::Chatgpt;
    let chatgpt = service.image_cache_id(Some(account)).unwrap();
    service
        .store_image_access(
            chatgpt.clone(),
            desegni_ai::ImageAccess::new(desegni_ai::ImageAvailability::No, "image_api_rejected"),
        )
        .unwrap();
    let saved = fs::read_to_string(service.directory.join("image-api-access.json")).unwrap();
    assert!(!saved.contains("sk-first") && !saved.contains("sk-second"));
    let mut reopened = GenerationService::from_directory(service.directory.clone()).unwrap();
    assert_eq!(
        reopened.image_access[&first].available,
        desegni_ai::ImageAvailability::No
    );
    assert_eq!(
        reopened.image_access[&chatgpt].available,
        desegni_ai::ImageAvailability::No
    );
    reopened
        .image_access_checking
        .insert(chatgpt.clone(), Uuid::new_v4());
    reopened.invalidate_image_account(account).unwrap();
    assert!(!reopened.image_access_checking.contains_key(&chatgpt));
    assert!(!reopened.image_access.contains_key(&chatgpt));
    assert!(reopened.image_access.contains_key(&first));
}

#[test]
fn image_key_is_separate_and_selection_prefers_the_active_connection() {
    let temp = Temp::new();
    let mut service = temp.service();
    service
        .credentials
        .set_key("sk-text-only-credential".into(), CredentialStorage::Session)
        .unwrap();
    service
        .image_credentials
        .set_key(
            "sk-separate-image-secret".into(),
            CredentialStorage::Session,
        )
        .unwrap();
    let main = service.image_cache_id(None).unwrap();
    let image = service.image_key_cache_id().unwrap();
    assert_ne!(main, image);
    service
        .store_image_access(
            main.clone(),
            desegni_ai::ImageAccess::new(desegni_ai::ImageAvailability::No, "subscription sharing"),
        )
        .unwrap();
    service
        .store_image_access(
            image.clone(),
            desegni_ai::ImageAccess::new(desegni_ai::ImageAvailability::Yes, "invalid size"),
        )
        .unwrap();
    assert_eq!(service.selected_image_cache_id(None), Some(image.clone()));
    service
        .store_image_access(
            main.clone(),
            desegni_ai::ImageAccess::new(desegni_ai::ImageAvailability::Yes, "invalid size"),
        )
        .unwrap();
    assert_eq!(service.selected_image_cache_id(None), Some(main));
    service.authentication = AuthenticationMethod::Chatgpt;
    assert_eq!(
        service.selected_image_cache_id(Some(Uuid::new_v4())),
        Some(image)
    );
    let saved = fs::read_to_string(service.directory.join("image-api-access.json")).unwrap();
    assert!(!saved.contains("sk-separate-image-secret"));
    assert!(!saved.contains("sk-text-only-credential"));
    service.image_credentials.remove_key().unwrap();
    assert!(service.credentials.status().configured);
    assert!(!service.image_credentials.status().configured);
}

#[test]
fn validation_repair_gate_skips_stop_limit_provider_error_and_second_round() {
    let allowed = |stopped, rounds, provider_failed, estimate| {
        validation_repair_allowed(
            JobStatus::Generating,
            stopped,
            rounds,
            provider_failed,
            AuthenticationMethod::ApiKey,
            0.6,
            estimate,
            1.0,
        )
    };
    assert!(allowed(false, 0, false, Some(0.3)));
    assert!(
        !allowed(true, 0, false, Some(0.3)),
        "Stop forbids another send"
    );
    assert!(
        !allowed(false, 0, true, Some(0.3)),
        "Provider errors never trigger validation repair"
    );
    assert!(
        !allowed(false, 0, false, Some(0.5)),
        "Combined estimate exceeds the limit"
    );
    assert!(
        !allowed(false, 0, false, None),
        "Unknown price cannot authorize spending"
    );
    assert!(
        !allowed(false, 1, false, Some(0.3)),
        "One repair per job, including explicit retries"
    );
    assert!(!validation_repair_allowed(
        JobStatus::Cancelled,
        false,
        0,
        false,
        AuthenticationMethod::ApiKey,
        0.6,
        Some(0.3),
        1.0
    ));
    assert_eq!(
        total_usage(Some(usage(1)), Some(usage(2)))
            .unwrap()
            .total_tokens,
        330
    );
}

#[test]
fn validation_notes_and_content_free_counters_survive_reopening() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    service.begin(&project, prepared.job_id, false).unwrap();
    let mut generated = candidate(&project, &prepared, "Private fixture label");
    generated.pages[0].content.css.clear();
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(response(&generated, 1)),
        )
        .unwrap();
    let view = service.saved_job(project.id, prepared.job_id).unwrap();
    assert!(
        view.validation_events
            .iter()
            .any(|event| event.kind == "omittedField" && event.disposition == "normalised")
    );
    let bytes = fs::read_to_string(service.directory.join("validation-counts.json")).unwrap();
    let counts: BTreeMap<String, u64> = serde_json::from_str(&bytes).unwrap();
    assert_eq!(counts["omittedField:normalised"], 1);
    assert!(!bytes.contains("Private fixture label"));
    assert!(!bytes.contains(&prepared.job_id.to_string()));
    assert!(view.validation_counts_recorded);
    service
        .finish(
            &project,
            Some(&project),
            prepared.job_id,
            1,
            Ok(response(&generated, 1)),
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(service.directory.join("validation-counts.json")).unwrap(),
        bytes,
        "A repeated completion must not count failures twice"
    );
}

#[test]
fn retained_drafts_survive_restart_past_history_and_dismissal() {
    let temp = Temp::new();
    let project = project();
    let mut service = temp.service();
    let prepared = service.prepare(&project).unwrap();
    let mut record = service.jobs[&prepared.job_id].clone();
    let drafts = desegni_core::retain_generation_drafts(
        "{\"pages\":[{\"pageId\":\"bad-id\",\"content\":{\"html\":\"<p>Paid work</p>\",\"css\":\"\",\"javascript\":\"\"}}]}",
        &record.view.requirements,
        &project,
        None,
        &[],
    );
    record.view.drafts = drafts.clone();
    service.persist(record).unwrap();
    drop(service);
    let mut restarted = temp.service();
    let loaded = restarted.saved_job(project.id, prepared.job_id).unwrap();
    assert_eq!(loaded.drafts[0].code, drafts[0].code);
    assert!(
        restarted
            .past_jobs(project.id)
            .unwrap()
            .jobs
            .iter()
            .any(|job| job.job_id == prepared.job_id)
    );
    let mut record = restarted
        .jobs
        .get(&prepared.job_id)
        .cloned()
        .unwrap_or(JobRecord {
            version: 1,
            view: loaded,
            error: None,
        });
    record.view.drafts[0].dismissed = true;
    restarted.persist(record).unwrap();
    drop(restarted);
    let loaded = temp
        .service()
        .saved_job(project.id, prepared.job_id)
        .unwrap();
    assert!(loaded.drafts[0].dismissed);
    assert_eq!(loaded.drafts[0].code, drafts[0].code);
}

#[test]
fn start_and_retry_refresh_source_revision_and_request_regions_after_manual_edits() {
    for retry in [false, true] {
        let temp = Temp::new();
        let mut service = temp.service();
        let original = project();
        let prepared = service.prepare(&original).unwrap();
        if retry {
            service.begin(&original, prepared.job_id, false).unwrap();
            service.cancel(&original, prepared.job_id).unwrap();
        }
        let frozen = fs::read(
            service
                .job_directory(original.id, prepared.job_id)
                .join("REQUIREMENTS.md"),
        )
        .unwrap();
        let mut edited = original.clone();
        edited.revision += 1;
        edited.pages[0].content = Some(SiteContent {
            html: "<main><h1>My edited heading</h1><img src=\"https://example.com/new.png\" alt=\"New photo\"></main>".into(),
            css: "main { color: red; transform: translate(20px, 30px); width: 640px; }".into(),
            javascript: String::new(),
        });
        edited.pages[0].regions[0].prompt = "Keep my heading and new photo".into();
        edited.pages[0].regions[0].rect.x += 16.;
        edited.pages[0].regions[0].rect.width += 24.;
        let (submitted, _) = service.begin(&edited, prepared.job_id, retry).unwrap();
        assert_ne!(submitted.job_id, prepared.job_id);
        assert_eq!(submitted.base_revision, edited.revision);
        submitted.requirements.validate_current(&edited).unwrap();
        assert!(
            submitted
                .requirements
                .markdown
                .contains("My edited heading")
        );
        assert!(
            submitted
                .requirements
                .markdown
                .contains("Keep my heading and new photo")
        );
        assert!(
            submitted
                .requirements
                .request_history
                .contains("Changes you made since the last result")
        );
        assert_eq!(submitted.areas[0].rect, edited.pages[0].regions[0].rect);
        assert_eq!(
            fs::read(
                service
                    .job_directory(original.id, prepared.job_id)
                    .join("REQUIREMENTS.md")
            )
            .unwrap(),
            frozen
        );
        assert_eq!(
            service
                .saved_job(original.id, prepared.job_id)
                .unwrap()
                .requirements,
            prepared.requirements
        );
    }
}

#[test]
fn repair_draft_uses_current_source_instead_of_its_stale_base_and_retained_code() {
    let temp = Temp::new();
    let mut service = temp.service();
    let original = project();
    let prepared = service.prepare(&original).unwrap();
    let mut record = service.record(&original, prepared.job_id).unwrap();
    record.view.status = JobStatus::Discarded;
    service.persist(record).unwrap();
    let mut edited = original.clone();
    edited.revision += 1;
    edited.pages[0].content.as_mut().unwrap().html =
        "<main>Manual text and deleted old elements</main>".into();
    let draft = desegni_core::GenerationDraft {
        id: Uuid::new_v4(),
        page_id: Some(original.home_page_id),
        region_ids: vec![original.pages[0].regions[0].id],
        code: "STALE DRAFT SOURCE MUST NOT BE SENT".into(),
        content: original.pages[0].content.clone(),
        base_sha256: Some("stale-base".into()),
        base_content: original.pages[0].content.clone(),
        base_editor_ids: vec![],
        reason: "Keep the change inside the requested area".into(),
        dismissed: false,
        applied: false,
    };
    let repaired = service
        .prepare_targeted(&edited, None, Some(draft))
        .unwrap();
    repaired.requirements.validate_current(&edited).unwrap();
    assert_eq!(repaired.base_revision, edited.revision);
    assert_eq!(repaired.requirements.region_count, 1);
    assert!(
        repaired
            .requirements
            .markdown
            .contains("Manual text and deleted old elements")
    );
    assert!(!repaired.requirements.markdown.contains("stale-base"));
    assert!(
        !repaired
            .requirements
            .markdown
            .contains("STALE DRAFT SOURCE")
    );
    assert!(
        repaired
            .requirements
            .request_history
            .contains("Changes you made since the last result")
    );
}

#[test]
fn runtime_repairs_prepare_only_the_repair_and_rebind_to_current_source() {
    let temp = Temp::new();
    let mut service = temp.service();
    let mut project = project();
    let page_id = project.pages[0].id;
    let region_id = project.pages[0].regions[0].id;
    project = project
        .apply(
            project.revision,
            Command::SetRegionRuntimeErrors {
                page_id,
                region_id,
                errors: vec!["TypeError: pause is null".into()],
            },
        )
        .unwrap();
    let prepared = service
        .prepare_requests(&project, None, None, None, Some(&[region_id]))
        .unwrap();
    assert_eq!(prepared.areas.len(), 1);
    assert_eq!(prepared.areas[0].region_id, region_id);
    assert!(
        prepared
            .requirements
            .markdown
            .contains("TypeError: pause is null")
    );
    project.pages[0].content.as_mut().unwrap().javascript = "const pause = null;".into();
    project.revision += 1;
    let (fresh, _) = service.begin(&project, prepared.job_id, false).unwrap();
    assert_ne!(fresh.job_id, prepared.job_id);
    assert_eq!(fresh.areas.len(), 1);
    assert_eq!(fresh.areas[0].region_id, region_id);
    assert!(fresh.requirements.markdown.contains("const pause = null;"));
    fresh.requirements.validate_current(&project).unwrap();
}

#[test]
fn discarded_validator_conflicts_survive_history_index_and_reach_each_retry_area() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    let mut result = candidate(&project, &prepared, "Rejected");
    // Exact technical messages from the two-area cf37f66e saved job.
    let reasons = [
        "Region script needs one uniquely owned root per request.",
        "Editor element IDs must be valid and unique within the page.",
    ];
    let mut record = service.record(&project, prepared.job_id).unwrap();
    for (region, reason) in result.regions.iter_mut().zip(reasons) {
        region.status = RegionStatus::Conflict;
        region.explanation = reason.into();
        record
            .view
            .validation_events
            .push(desegni_core::ValidationEvent {
                kind: desegni_core::validation_failure_kind(reason).into(),
                disposition: "conflicted".into(),
                region_id: Some(region.region_id),
                message: reason.into(),
            });
    }
    result.pages.clear();
    record.view.candidate = Some(result);
    record.view.status = JobStatus::Discarded;
    let view = service.persist(record).unwrap();
    // Simulate an index written by the old version after discarding a blocked job.
    let mut legacy = service.history_entries(&view);
    for entry in &mut legacy {
        entry.outcome = "rejected by user".into();
        entry.validation_rejection = None;
    }
    write_json(&service.history_path(project.id), &legacy).unwrap();
    drop(service);
    let mut service = temp.service();
    let retry = service.prepare(&project).unwrap();
    for reason in reasons {
        assert!(retry.requirements.request_history.contains(&format!(
            "Your previous attempt for this area was rejected because: {reason}"
        )));
    }
    assert!(
        retry
            .requirements
            .request_history
            .contains("Avoid this by:")
    );
    assert!(
        !retry
            .requirements
            .request_history
            .contains("rejected by user")
    );
    retry.requirements.validate_current(&project).unwrap();
    let entries = service.request_history(&project).unwrap();
    assert!(
        entries
            .iter()
            .filter(|e| e.job_id == prepared.job_id)
            .all(
                |e| e.outcome == "rejected by the app's checks" && e.validation_rejection.is_some()
            )
    );
}

#[test]
fn history_keeps_provider_failure_and_user_rejection_separate_from_validation() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let prepared = service.prepare(&project).unwrap();
    let mut view = prepared.clone();
    view.status = JobStatus::Discarded;
    view.candidate = Some(candidate(&project, &prepared, "Ready result"));
    assert!(
        service
            .history_entries(&view)
            .iter()
            .all(|e| e.outcome == "rejected by user" && e.validation_rejection.is_none())
    );
    view.status = JobStatus::Failed;
    view.candidate = None;
    assert!(
        service
            .history_entries(&view)
            .iter()
            .all(|e| e.outcome == "failed" && e.validation_rejection.is_none())
    );
}

#[test]
fn whole_reply_validator_rejection_is_not_a_provider_failure() {
    let temp = Temp::new();
    let mut service = temp.service();
    let project = project();
    let mut view = service.prepare(&project).unwrap();
    view.status = JobStatus::Discarded;
    view.validation_events.push(desegni_core::ValidationEvent {
        kind: "replyStructure".into(),
        disposition: "conflicted".into(),
        region_id: None,
        message: "The reply contains duplicate or unrequested pages.".into(),
    });
    assert!(
        service.history_entries(&view).iter().all(
            |e| e.outcome == "rejected by the app's checks" && e.validation_rejection.is_some()
        )
    );
    view.validation_events.clear();
    view.failure_kind = Some(ErrorKind::Cancelled);
    assert!(
        service
            .history_entries(&view)
            .iter()
            .all(|e| e.outcome == "failed" && e.validation_rejection.is_none())
    );
}
