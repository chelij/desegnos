use super::*;
use desegni_ai::{ApiKey, ErrorKind, OpenAiClient, ProviderError, ProviderSettings};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("desegnos-identity-native-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn service(&self) -> IdentityService {
        IdentityService::from_directory(self.0.join("identity")).unwrap()
    }
    fn source(&self) -> IdentitySource {
        let path = self.0.join("studio-website");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("IDENTITY.md"), "# Studio identity\nBrand name: Saka\nTone: direct and welcoming\nLanguage: Indonesian\n").unwrap();
        fs::write(
            path.join("style.css"),
            ":root{--brand:#16324F}body{font-family:system-ui;background:#fff}",
        )
        .unwrap();
        fs::write(path.join("logo.svg"), "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 20 20'><rect width='20' height='20' fill='#16324F'/></svg>").unwrap();
        normalize_source(collect_identity_source(&path).unwrap()).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn access(authentication: AuthenticationMethod, account: Option<Uuid>) -> IdentityAccess {
    IdentityAccess {
        authentication,
        account_id: account,
        account_epoch: account.map(|_| 0),
        settings: ProviderSettings::default(),
        client: OpenAiClient::new().unwrap(),
        key: ApiKey::new("sk-private-test-key-that-never-enters-identity-records".into()).unwrap(),
    }
}
fn reserve(service: &mut IdentityService, source: &IdentitySource) -> AnalysisView {
    service.retain_source(source.clone()).unwrap();
    service
        .reserve(source.id, source.files.iter().map(|file| file.id).collect())
        .unwrap()
        .0
}
fn candidate(record: &AnalysisRecord) -> IdentityCandidate {
    let evidence = record.file_ids.clone();
    IdentityCandidate {
        job_id: record.requirements.job_id,
        requirements_sha256: record.requirements.sha256.clone(),
        summary: "Reusable studio identity from the supplied source.".into(),
        brand_name: "Saka".into(),
        tone: "Direct and welcoming".into(),
        findings: vec![
            IdentityFinding {
                topic: "Brand name".into(),
                value: "Saka".into(),
                evidence: evidence.clone(),
                confidence: Confidence::High,
            },
            IdentityFinding {
                topic: "Colours".into(),
                value: "Deep blue #16324F with white".into(),
                evidence: evidence.clone(),
                confidence: Confidence::High,
            },
            IdentityFinding {
                topic: "Tone".into(),
                value: "Direct and welcoming".into(),
                evidence,
                confidence: Confidence::Medium,
            },
        ],
        logo_asset_ids: record
            .source
            .files
            .iter()
            .filter_map(|file| file.asset.as_ref().map(|asset| asset.id))
            .collect(),
        warnings: vec![],
    }
}
fn response(value: &IdentityCandidate) -> ProviderResponse {
    ProviderResponse {
        text: serde_json::to_string(value).unwrap(),
        response_id: "resp_identity_native_fixture".into(),
        usage: None,
    }
}
fn complete(service: &mut IdentityService, source: &IdentitySource) -> AnalysisView {
    let view = reserve(service, source);
    service
        .begin(view.job_id, &access(AuthenticationMethod::ApiKey, None))
        .unwrap();
    let value = candidate(service.latest.as_ref().unwrap());
    service.finish(view.job_id, Ok(response(&value))).unwrap();
    service.latest.as_ref().unwrap().view()
}

#[test]
fn reservation_is_durable_before_auth_and_double_submission_is_rejected() {
    let temp = Temp::new();
    let mut service = temp.service();
    let source = temp.source();
    let view = reserve(&mut service, &source);
    assert_eq!(view.status, AnalysisStatus::Analyzing);
    let disk: AnalysisRecord =
        read_record(&service.record_path(view.job_id), MAX_RECORD_BYTES).unwrap();
    assert!(!disk.submitted);
    assert!(disk.authentication.is_none());
    assert!(disk.candidate.is_none());
    assert!(service.reserve(source.id, view.file_ids.clone()).is_err());
    let public = serde_json::to_value(&view).unwrap();
    assert!(
        public["source"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file.get("text").is_none() && file.get("textBytes").is_some())
    );
    assert!(!serde_json::to_string(&view).unwrap().contains("--brand"));
    assert!(
        service
            .begin(view.job_id, &access(AuthenticationMethod::ApiKey, None))
            .is_ok()
    );
    let disk: AnalysisRecord =
        read_record(&service.record_path(view.job_id), MAX_RECORD_BYTES).unwrap();
    assert!(disk.submitted);
    assert_eq!(disk.authentication, Some(AuthenticationMethod::ApiKey));
    assert!(
        !serde_json::to_string(&disk)
            .unwrap()
            .contains("sk-private-test-key")
    );
}

#[test]
fn frozen_selected_files_are_exact_bounded_evidence_and_svg_never_becomes_an_image_request() {
    let temp = Temp::new();
    let source = temp.source();
    let file = source
        .files
        .iter()
        .find(|file| file.name == "IDENTITY.md")
        .unwrap();
    let (snapshot, images) = freeze(&source, &[file.id], Uuid::new_v4()).unwrap();
    assert!(images.is_empty());
    assert!(snapshot.markdown.contains("Brand name: Saka"));
    assert!(!snapshot.markdown.contains("--brand"));
    assert!(!snapshot.markdown.contains("data:image/"));
    assert_eq!(snapshot.sha256, digest(snapshot.markdown.as_bytes()));
    assert!(freeze(&source, &[file.id, file.id], Uuid::new_v4()).is_err());
    assert!(freeze(&source, &[Uuid::new_v4()], Uuid::new_v4()).is_err());
    let (_, images) = freeze(
        &source,
        &source.files.iter().map(|file| file.id).collect::<Vec<_>>(),
        Uuid::new_v4(),
    )
    .unwrap();
    assert!(images.is_empty());
    let mut oversized = source.clone();
    oversized.files[0].text = Some("x".repeat(256 * 1024));
    oversized.files[1].text = Some("y".repeat(256 * 1024));
    oversized.sha256 = digest(&serde_json::to_vec(&oversized.files).unwrap());
    oversized.id = source_id(&oversized.sha256);
    assert!(
        freeze(
            &oversized,
            &oversized
                .files
                .iter()
                .map(|file| file.id)
                .collect::<Vec<_>>(),
            Uuid::new_v4()
        )
        .unwrap_err()
        .contains("512 KiB")
    );
}

#[test]
fn candidate_schema_echo_evidence_and_assets_are_validated_before_receipt() {
    for case in [
        "job",
        "hash",
        "evidence",
        "duplicateEvidence",
        "logo",
        "duplicateTopic",
        "unknownField",
        "unknownConfidence",
    ] {
        let temp = Temp::new();
        let mut service = temp.service();
        let source = temp.source();
        let view = reserve(&mut service, &source);
        service
            .begin(view.job_id, &access(AuthenticationMethod::ApiKey, None))
            .unwrap();
        let mut value = serde_json::to_value(candidate(service.latest.as_ref().unwrap())).unwrap();
        match case {
            "job" => value["jobId"] = Uuid::new_v4().to_string().into(),
            "hash" => value["requirementsSha256"] = "0".repeat(64).into(),
            "evidence" => value["findings"][0]["evidence"][0] = Uuid::new_v4().to_string().into(),
            "duplicateEvidence" => {
                let id = value["findings"][0]["evidence"][0].clone();
                value["findings"][0]["evidence"]
                    .as_array_mut()
                    .unwrap()
                    .push(id);
            }
            "logo" => value["logoAssetIds"][0] = Uuid::new_v4().to_string().into(),
            "duplicateTopic" => {
                value["findings"][1]["topic"] = value["findings"][0]["topic"].clone()
            }
            "unknownField" => value["extraInstructions"] = "Execute untrusted source".into(),
            "unknownConfidence" => value["findings"][0]["confidence"] = "certain".into(),
            _ => unreachable!(),
        }
        service
            .finish(
                view.job_id,
                Ok(ProviderResponse {
                    text: value.to_string(),
                    response_id: "resp_invalid_candidate".into(),
                    usage: None,
                }),
            )
            .unwrap();
        let record = service.latest.as_ref().unwrap();
        assert_eq!(record.status, AnalysisStatus::Failed, "{case}");
        assert!(record.candidate.is_none());
        assert!(
            service
                .project_from_analysis(
                    view.job_id,
                    &[0],
                    &[],
                    "Cannot create".into(),
                    Viewport {
                        width: 1440,
                        height: 900
                    }
                )
                .is_err()
        );
    }
    let schema = candidate_schema();
    assert_eq!(schema["additionalProperties"], false);
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("jobId"))
    );
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("requirementsSha256"))
    );
}

#[test]
fn cancellation_and_signout_block_late_output_and_allow_only_explicit_new_jobs() {
    let temp = Temp::new();
    let mut service = temp.service();
    let source = temp.source();
    let view = reserve(&mut service, &source);
    let before_auth = candidate(service.latest.as_ref().unwrap());
    service.cancel_account(Uuid::new_v4()).unwrap();
    assert_eq!(
        service.latest.as_ref().unwrap().status,
        AnalysisStatus::Cancelled
    );
    service
        .finish(view.job_id, Ok(response(&before_auth)))
        .unwrap();
    assert!(service.latest.as_ref().unwrap().candidate.is_none());
    let next = service.reserve(source.id, view.file_ids.clone()).unwrap().0;
    assert_ne!(next.job_id, view.job_id);
    let account = Uuid::new_v4();
    service
        .begin(
            next.job_id,
            &access(AuthenticationMethod::Chatgpt, Some(account)),
        )
        .unwrap();
    service.cancel_account(Uuid::new_v4()).unwrap();
    assert_eq!(
        service.latest.as_ref().unwrap().status,
        AnalysisStatus::Analyzing
    );
    let late = candidate(service.latest.as_ref().unwrap());
    service.cancel_account(account).unwrap();
    service.finish(next.job_id, Ok(response(&late))).unwrap();
    assert_eq!(
        service.latest.as_ref().unwrap().status,
        AnalysisStatus::Cancelled
    );
    assert!(
        service
            .begin(
                next.job_id,
                &access(AuthenticationMethod::Chatgpt, Some(account))
            )
            .is_err()
    );
}

#[test]
fn restart_retains_complete_review_and_marks_interrupted_intents_uncertain_without_replay() {
    for submitted in [false, true] {
        let temp = Temp::new();
        let source = temp.source();
        let mut service = temp.service();
        let view = reserve(&mut service, &source);
        if submitted {
            service
                .begin(view.job_id, &access(AuthenticationMethod::ApiKey, None))
                .unwrap();
        }
        drop(service);
        let mut restored = temp.service();
        let record = restored.latest.as_ref().unwrap();
        assert_eq!(record.status, AnalysisStatus::Unknown);
        assert!(record.candidate.is_none());
        assert!(restored.active.is_none());
        assert_eq!(record.source, source);
        assert_eq!(record.file_ids, view.file_ids);
        assert!(record.message.contains("not be sent again automatically"));
        let retried = restored.reserve(source.id, view.file_ids).unwrap().0;
        assert_ne!(retried.job_id, view.job_id);
    }
    let temp = Temp::new();
    let source = temp.source();
    let mut service = temp.service();
    let view = complete(&mut service, &source);
    drop(service);
    let restored = temp.service();
    assert_eq!(
        restored.latest.as_ref().unwrap().status,
        AnalysisStatus::Complete
    );
    assert_eq!(restored.latest.as_ref().unwrap().candidate, view.candidate);
    assert!(restored.active.is_none());
}

#[test]
fn selected_creation_is_blank_light_and_carries_only_approved_findings_and_images() {
    let temp = Temp::new();
    let mut service = temp.service();
    let source = temp.source();
    let view = complete(&mut service, &source);
    let logo = view.candidate.as_ref().unwrap().logo_asset_ids[0];
    let project = service
        .project_from_analysis(
            view.job_id,
            &[0],
            &[logo],
            "New studio".into(),
            Viewport {
                width: 390,
                height: 844,
            },
        )
        .unwrap();
    assert_eq!(project.revision, 0);
    assert_eq!(project.pages.len(), 1);
    assert_eq!(project.appearance, Appearance::Light);
    assert_eq!(project.pages[0].appearance, Appearance::Light);
    assert!(project.pages[0].content.is_none());
    assert!(project.pages[0].regions.is_empty());
    assert_eq!(project.brief.brand_name, "Saka");
    assert!(project.brief.tone.is_empty());
    assert_eq!(project.decisions.len(), 1);
    assert_eq!(project.decisions[0].accepted_at_revision, 0);
    let imported = project.imported_identity.as_ref().unwrap();
    assert_eq!(imported.source_sha256, source.sha256);
    assert_eq!(imported.assets[0].id, logo);
    assert!(!imported.summary.contains("Colours"));
    assert!(!imported.summary.contains("Tone:"));
    assert!(
        !desegni_core::Session::new(project.clone())
            .unwrap()
            .can_undo()
    );
    for (findings, assets) in [
        (vec![0, 0], vec![]),
        (vec![99], vec![]),
        (vec![], vec![]),
        (vec![0], vec![Uuid::new_v4()]),
        (vec![0], vec![logo, logo]),
    ] {
        assert!(
            service
                .project_from_analysis(
                    view.job_id,
                    &findings,
                    &assets,
                    "Invalid".into(),
                    project.viewport
                )
                .is_err()
        );
    }
    let file = desegni_storage::ProjectFile::acquire(&temp.0.join("new.desegni")).unwrap();
    file.save(&project).unwrap();
    assert_eq!(file.load().unwrap(), project);
}

#[test]
fn failed_receipt_never_exposes_a_candidate_and_credential_echo_errors_store_no_raw_response() {
    let temp = Temp::new();
    let mut service = temp.service();
    let source = temp.source();
    let view = reserve(&mut service, &source);
    service
        .begin(view.job_id, &access(AuthenticationMethod::ApiKey, None))
        .unwrap();
    let value = candidate(service.latest.as_ref().unwrap());
    let path = service.record_path(view.job_id);
    let parked = temp.0.join("preserved-intent.json");
    fs::rename(&path, &parked).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(service.finish(view.job_id, Ok(response(&value))).is_err());
    assert_eq!(
        service.latest.as_ref().unwrap().status,
        AnalysisStatus::Unknown
    );
    assert!(service.latest.as_ref().unwrap().candidate.is_none());
    fs::remove_dir(&path).unwrap();
    fs::rename(parked, &path).unwrap();
    drop(service);
    let mut service = temp.service();
    assert_eq!(
        service.latest.as_ref().unwrap().status,
        AnalysisStatus::Unknown
    );
    let view = service.reserve(source.id, view.file_ids).unwrap().0;
    service
        .begin(view.job_id, &access(AuthenticationMethod::ApiKey, None))
        .unwrap();
    service
        .finish(
            view.job_id,
            Err(ProviderError::new(
                ErrorKind::Malformed,
                SubmissionOutcome::Received,
            )),
        )
        .unwrap();
    assert_eq!(
        service.latest.as_ref().unwrap().status,
        AnalysisStatus::Failed
    );
    let saved = fs::read_to_string(service.record_path(view.job_id)).unwrap();
    assert!(!saved.contains("sk-private-test-key"));
    assert!(service.latest.as_ref().unwrap().candidate.is_none());
}

#[test]
fn disk_tampering_and_unselected_evidence_cannot_create_or_resume_a_valid_identity() {
    let temp = Temp::new();
    let mut service = temp.service();
    let source = temp.source();
    let view = complete(&mut service, &source);
    let path = service.record_path(view.job_id);
    let mut record = service.latest.as_ref().unwrap().clone();
    record.source.files[0].text = Some("Forged source".into());
    write_record(&path, &record).unwrap();
    let tampered_directory = service.directory.clone();
    assert!(service.validate_record(&record).is_err());
    assert!(
        service
            .project_from_analysis(
                Uuid::new_v4(),
                &[0],
                &[],
                "Missing".into(),
                Viewport {
                    width: 1440,
                    height: 900
                }
            )
            .is_err()
    );
    drop(service);
    assert!(IdentityService::from_directory(tampered_directory).is_err());
    let mut service = IdentityService::from_directory(temp.0.join("identity-other")).unwrap();
    let file = source
        .files
        .iter()
        .find(|file| file.name == "IDENTITY.md")
        .unwrap();
    service.retain_source(source.clone()).unwrap();
    let view = service.reserve(source.id, vec![file.id]).unwrap().0;
    service
        .begin(view.job_id, &access(AuthenticationMethod::ApiKey, None))
        .unwrap();
    let mut value = candidate(service.latest.as_ref().unwrap());
    value.findings[0].evidence.push(
        source
            .files
            .iter()
            .find(|other| other.id != file.id)
            .unwrap()
            .id,
    );
    value.logo_asset_ids.clear();
    service.finish(view.job_id, Ok(response(&value))).unwrap();
    assert_eq!(
        service.latest.as_ref().unwrap().status,
        AnalysisStatus::Failed
    );
}

#[test]
fn second_process_cannot_repair_or_replace_a_live_analysis_intent() {
    let temp = Temp::new();
    let mut first = temp.service();
    let source = temp.source();
    let view = reserve(&mut first, &source);
    let record = fs::read(first.record_path(view.job_id)).unwrap();
    let pointer = fs::read(first.directory.join("latest.json")).unwrap();
    let mut second = IdentityService::recover_or_report(first.directory.clone());
    assert!(second.ownership.is_none());
    assert!(
        second
            .startup_error
            .as_ref()
            .unwrap()
            .contains("another Desegnos window")
    );
    second.retain_source(source.clone()).unwrap();
    assert!(second.reserve(source.id, view.file_ids).is_err());
    assert_eq!(fs::read(first.record_path(view.job_id)).unwrap(), record);
    assert_eq!(
        fs::read(first.directory.join("latest.json")).unwrap(),
        pointer
    );
    assert_eq!(
        first.latest.as_ref().unwrap().status,
        AnalysisStatus::Analyzing
    );
    drop(second);
    drop(first);
    let reopened = temp.service();
    assert_eq!(
        reopened.latest.as_ref().unwrap().status,
        AnalysisStatus::Unknown
    );
    assert!(reopened.active.is_none());
}

#[test]
fn damaged_identity_receipt_keeps_editor_startup_available_and_original_data_intact() {
    let temp = Temp::new();
    let mut first = temp.service();
    let source = temp.source();
    let view = complete(&mut first, &source);
    let path = first.record_path(view.job_id);
    let directory = first.directory.clone();
    drop(first);
    fs::write(&path, b"damaged saved receipt").unwrap();
    let mut restored = IdentityService::recover_or_report(directory);
    assert!(restored.ownership.is_some());
    assert!(restored.startup_error.is_some());
    assert!(restored.latest.is_none());
    assert_eq!(fs::read(&path).unwrap(), b"damaged saved receipt");
    restored.retain_source(source.clone()).unwrap();
    restored
        .reserve(source.id, source.files.iter().map(|file| file.id).collect())
        .unwrap();
    assert!(restored.startup_error.is_none());
    assert_eq!(fs::read(&path).unwrap(), b"damaged saved receipt");
}

#[test]
fn cancellation_receipt_failure_still_revokes_active_analysis_and_rejects_late_output() {
    let temp = Temp::new();
    let mut service = temp.service();
    let source = temp.source();
    let view = reserve(&mut service, &source);
    let account = Uuid::new_v4();
    service
        .begin(
            view.job_id,
            &access(AuthenticationMethod::Chatgpt, Some(account)),
        )
        .unwrap();
    let candidate = candidate(service.latest.as_ref().unwrap());
    let token = service.active.as_ref().unwrap().1.clone();
    let path = service.record_path(view.job_id);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(service.cancel_account(account).is_err());
    assert!(token.is_cancelled());
    assert!(service.active.is_none());
    assert_eq!(
        service.latest.as_ref().unwrap().status,
        AnalysisStatus::Cancelled
    );
    service
        .finish(view.job_id, Ok(response(&candidate)))
        .unwrap();
    assert!(service.latest.as_ref().unwrap().candidate.is_none());
}
