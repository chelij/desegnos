mod support;
use desegni_core::*;
use serde_json::{Value, json};
use std::cell::Cell;
use support::without_editor_ids;
use uuid::Uuid;

fn source(label: &str) -> SiteContent {
    SiteContent {
        html: format!("<main><button id=\"counter\">{label}</button></main>"),
        css: "main { display: grid; gap: 1rem; }".into(),
        javascript: "document.querySelector('#counter').addEventListener('click', event => { event.target.textContent = 'Clicked'; });".into(),
    }
}

fn describe(mut project: Project, page_id: Uuid, prompt: &str) -> Project {
    project = project
        .apply(
            project.revision,
            Command::AddRegion {
                page_id,
                rect: Rect {
                    x: 24.0,
                    y: 32.0,
                    width: 280.0,
                    height: 120.0,
                },
            },
        )
        .unwrap();
    let region = project.page(page_id).unwrap().regions.last().unwrap();
    project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id,
                region_id: region.id,
                rect: region.rect,
                prompt: prompt.into(),
            },
        )
        .unwrap()
}

fn fixture() -> (Project, RequirementsSnapshot, GenerationCandidate) {
    let mut project = Project::new(
        "Generation contract".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    let home = project.home_page_id;
    project = describe(project, home, "An accessible native counter control.");
    project = describe(project, home, "Explain the counter below its button.");
    for (name, path) in [("Details", "/details"), ("Untouched", "/untouched")] {
        project = project
            .apply(
                project.revision,
                Command::AddPage {
                    name: name.into(),
                    path: path.into(),
                },
            )
            .unwrap();
    }
    let details = project.pages[1].id;
    project = describe(
        project,
        details,
        "A native disclosure explaining how the counter works.",
    );
    project.pages[0].content = Some(source("Existing home"));
    let first_area = project.pages[0].regions[0].id;
    let second_area = project.pages[0].regions[1].id;
    project.pages[0].content.as_mut().unwrap().html = format!(
        "<main><button id=\"counter\" data-desegnos-region=\"{first_area}\">Existing home</button><p data-desegnos-region=\"{second_area}\">Existing explanation</p></main>"
    );
    project.pages[2].content = Some(source("PRIVATE UNREQUESTED SOURCE"));
    project.extensions.insert(
        "org.example.preserved".into(),
        ExtensionRecord {
            schema_version: 7,
            data: json!({"opaque":[1,"keep",null]}),
        },
    );
    let snapshot = project.prepare_requirements().unwrap();
    let regions: Vec<_> = project
        .pages
        .iter()
        .flat_map(|page| {
            page.regions.iter().map(|region| RegionOutcome {
                region_id: region.id,
                status: RegionStatus::Implemented,
                explanation: "Implemented using native browser behavior.".into(),
            })
        })
        .collect();
    let candidate = GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Implemented the counter and its explanation on the requested pages.".into(),
        regions,
        pages: vec![
            GeneratedPage {
                page_id: home,
                content: SiteContent {
                    html: format!(
                        "<main><button id=\"counter\" data-desegnos-region=\"{first_area}\">Count</button><p data-desegnos-region=\"{second_area}\">Click to change the label.</p></main>"
                    ),
                    ..source("Count")
                },
            },
            GeneratedPage {
                page_id: details,
                content: SiteContent {
                    html: format!(
                        "<details data-desegnos-region=\"{}\"><summary>Counter behavior</summary><p>Click to change the label.</p></details>",
                        project.pages[1].regions[0].id
                    ),
                    css: String::new(),
                    javascript: String::new(),
                },
            },
        ],
        component_requests: vec![ComponentRequest {
            region_id: project.pages[0].regions[0].id,
            capability: "Interactive counter button".into(),
            resolution: ComponentResolution::Native,
            reason: "HTML button and a DOM click listener are available.".into(),
        }],
        decision_proposals: vec![
            DecisionProposal {
                topic: "Control shape".into(),
                value: "Square counter button".into(),
                scope: Scope::Page(home),
                rationale: "A clear shape for the primary control.".into(),
            },
            DecisionProposal {
                topic: "Voice".into(),
                value: "Plain language".into(),
                scope: Scope::Project,
                rationale: "Keep explanations consistent.".into(),
            },
        ],
        criticism: vec![DesignCriticism {
            region_id: Some(project.pages[0].regions[0].id),
            message: "The control and explanation need clear separation.".into(),
            suggestion: "Keep a consistent gap between them.".into(),
        }],
    };
    (project, snapshot, candidate)
}

#[test]
fn drafts_allow_explicit_placeholders_and_portable_image_downloads_without_package_authority() {
    let (project, snapshot, mut candidate) = fixture();
    assert!(snapshot.markdown.contains("clearly defined placeholders"));
    assert!(snapshot.markdown.contains("system font fallbacks"));
    assert!(snapshot.markdown.contains("data-desegnos-download"));
    assert!(snapshot.markdown.contains("\"packageInstalls\": false"));
    candidate.pages[0].content.html = "<a href='https://example.com/link'>Source</a><img id='portable' DATA-DESEGNOS-DOWNLOAD src='https://example.com/pixel.png' alt='Sample garden'><img id='missing' data-desegnos-download src='https://example.com/missing.png' alt='Sample workshop' srcset='https://example.com/bypass.png 2x'>".into();
    candidate.pages[0].content.html = format!(
        "<main><section data-desegnos-region=\"{}\">{}</section><p data-desegnos-region=\"{}\">Click to change the label.</p></main>",
        project.pages[0].regions[0].id,
        candidate.pages[0].content.html,
        project.pages[0].regions[1].id
    );
    assert_eq!(candidate.image_download_urls().unwrap().len(), 2);
    let pixel = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jMZkAAAAASUVORK5CYII=";
    let missing = candidate
        .embed_downloaded_images(&std::collections::BTreeMap::from([(
            "https://example.com/pixel.png".into(),
            pixel.into(),
        )]))
        .unwrap();
    assert_eq!(missing, 1);
    assert!(candidate.image_download_urls().unwrap().is_empty());
    candidate.validate(&snapshot, &project).unwrap();
    let html = &candidate.pages[0].content.html;
    assert!(html.contains(pixel));
    assert!(html.contains("data-desegnos-image-source=\"https://example.com/pixel.png\""));
    assert!(html.contains("Image placeholder: Sample workshop"));
    assert!(!html.contains("srcset="));
    let document = dom_query::Document::from(html.as_str());
    let missing_src = document.select("#missing").attr("src").unwrap();
    use base64::Engine;
    let placeholder = base64::engine::general_purpose::STANDARD
        .decode(missing_src.split_once(',').unwrap().1)
        .unwrap();
    let placeholder = String::from_utf8(placeholder).unwrap();
    assert_eq!(placeholder.matches("<text ").count(), 1);
    assert!(!placeholder.contains("Replace with"));
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    let exported = compile_website(&applied).unwrap();
    assert!(exported.files.keys().any(|name| name.ends_with(".png")));
    let output = String::from_utf8(exported.files["index.html"].clone()).unwrap();
    assert!(!output.contains("src=\"https:"));
    assert!(output.contains("Image placeholder: Sample workshop"));
}

#[test]
fn strict_output_schema_and_decoder_match_the_candidate_contract() {
    fn check_schema(value: &Value) {
        assert!(
            value.get("maxLength").is_none(),
            "Use the documented provider schema subset; native validation owns text limits."
        );
        if value["type"] == "object" {
            assert_eq!(value["additionalProperties"], false);
            let properties = value["properties"].as_object().unwrap();
            let required = value["required"].as_array().unwrap();
            assert_eq!(properties.len(), required.len());
            for key in properties.keys() {
                assert!(required.contains(&Value::String(key.clone())));
            }
        }
        match value {
            Value::Object(fields) => fields.values().for_each(check_schema),
            Value::Array(values) => values.iter().for_each(check_schema),
            _ => {}
        }
    }
    let (project, snapshot, candidate) = fixture();
    check_schema(&candidate_schema());
    let encoded = serde_json::to_string(&candidate).unwrap();
    let decoded = GenerationCandidate::from_json(&encoded).unwrap();
    assert_eq!(decoded, candidate);
    decoded.validate(&snapshot, &project).unwrap();
    assert!(decoded.is_ready());
    let mut missing_content = serde_json::to_value(&project).unwrap();
    missing_content["pages"][0]
        .as_object_mut()
        .unwrap()
        .remove("content");
    assert!(serde_json::from_value::<Project>(missing_content).is_err());
    for job_ids in [vec![Uuid::nil()], vec![candidate.job_id, candidate.job_id]] {
        let mut invalid = project.clone();
        invalid.applied_job_ids = job_ids;
        assert!(invalid.validate().is_err());
    }
    for condition in [
        "unknown-command",
        "missing-source",
        "missing-nullable",
        "bad-status",
        "bad-scope",
        "duplicate-field",
        "truncated",
    ] {
        let mut value = serde_json::to_value(&candidate).unwrap();
        let encoded = match condition {
            "unknown-command" => {
                value["commands"] = json!(["install untrusted package"]);
                value.to_string()
            }
            "missing-source" => {
                value["pages"][0]["content"]
                    .as_object_mut()
                    .unwrap()
                    .remove("javascript");
                value.to_string()
            }
            "missing-nullable" => {
                value["criticism"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("regionId");
                value.to_string()
            }
            "bad-status" => {
                value["regions"][0]["status"] = "done".into();
                value.to_string()
            }
            "bad-scope" => {
                value["decisionProposals"][0]["scope"]["privilege"] = "global".into();
                value.to_string()
            }
            "duplicate-field" => {
                encoded.replacen("\"summary\":", "\"summary\":\"duplicate\",\"summary\":", 1)
            }
            "truncated" => encoded[..encoded.len() - 1].into(),
            _ => unreachable!(),
        };
        assert!(
            GenerationCandidate::from_json(&encoded).is_err(),
            "{condition}"
        );
    }
}

#[test]
fn every_frozen_target_and_affected_page_must_be_accounted_for_exactly_once() {
    let (project, snapshot, candidate) = fixture();
    for condition in [
        "job",
        "digest",
        "missing-area",
        "duplicate-area",
        "extra-area",
        "missing-page",
        "duplicate-page",
        "extra-page",
        "component-scope",
        "criticism-scope",
        "decision-page",
        "decision-area",
    ] {
        let mut invalid = candidate.clone();
        match condition {
            "job" => invalid.job_id = Uuid::new_v4(),
            "digest" => invalid.requirements_sha256 = "0".repeat(64),
            "missing-area" => {
                invalid.regions.pop();
            }
            "duplicate-area" => invalid.regions.push(invalid.regions[0].clone()),
            "extra-area" => invalid.regions[0].region_id = Uuid::new_v4(),
            "missing-page" => {
                invalid.pages.pop();
            }
            "duplicate-page" => invalid.pages.push(invalid.pages[0].clone()),
            "extra-page" => invalid.pages[1].page_id = project.pages[2].id,
            "component-scope" => invalid.component_requests[0].region_id = Uuid::new_v4(),
            "criticism-scope" => invalid.criticism[0].region_id = Some(Uuid::new_v4()),
            "decision-page" => {
                invalid.decision_proposals[0].scope = Scope::Page(project.pages[2].id)
            }
            "decision-area" => invalid.decision_proposals[0].scope = Scope::Region(Uuid::new_v4()),
            _ => unreachable!(),
        }
        assert!(
            invalid.validate(&snapshot, &project).is_err(),
            "{condition}"
        );
        assert!(
            project.apply_generation(&snapshot, &invalid, &[]).is_err(),
            "{condition}"
        );
    }
    assert!(project.applied_job_ids.is_empty());
}

#[test]
fn current_conflicts_keep_other_pages_applicable() {
    let (project, snapshot, mut candidate) = fixture();
    candidate.component_requests[0].resolution = ComponentResolution::Unavailable;
    candidate.component_requests[0].reason =
        "This requested hosted payment capability is unavailable without a configured service."
            .into();
    // A finished draft keeps a marked placeholder for the missing service and tells the user.
    candidate.validate(&snapshot, &project).unwrap();
    let read = GenerationCandidate::from_reply(
        &serde_json::to_string(&candidate).unwrap(),
        &snapshot,
        &project,
    )
    .unwrap();
    assert!(
        read.criticism
            .iter()
            .any(|item| item.message.contains("isn't connected yet"))
    );
    candidate.regions[0].status = RegionStatus::Conflict;
    candidate.regions[0].explanation = candidate.component_requests[0].reason.clone();
    assert!(
        candidate.validate(&snapshot, &project).is_err(),
        "a conflicted page cannot be supplied as complete"
    );
    candidate.pages.remove(0);
    candidate.regions[1].status = RegionStatus::Conflict;
    candidate.validate(&snapshot, &project).unwrap();
    assert!(!candidate.is_ready());
    assert!(
        candidate
            .ensure_ready()
            .unwrap_err()
            .0
            .contains("hosted payment")
    );
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    assert_eq!(applied.pending_count(), 2);
    assert!(candidate.can_apply(&snapshot));
    candidate.regions[0].explanation.clear();
    assert!(
        candidate.validate(&snapshot, &project).is_err(),
        "conflicts require an explanation"
    );
}

#[test]
fn affected_source_mode_and_identity_are_frozen_but_unrelated_edits_remain_valid() {
    let (project, snapshot, candidate) = fixture();
    assert!(snapshot.markdown.contains("Requirements format: 19"));
    assert_eq!(snapshot.markdown.matches("\"generationMode\"").count(), 1);
    assert!(snapshot.markdown.contains("Existing home"));
    assert!(!snapshot.markdown.contains("PRIVATE UNREQUESTED SOURCE"));
    assert!(snapshot.markdown.contains(&digest(
        project.pages[0].content.as_ref().unwrap().html.as_bytes()
    )));
    assert!(
        snapshot
            .markdown
            .contains("No external packages, React components")
    );
    assert!(snapshot.markdown.contains("Structured result contract"));
    for condition in [
        "html",
        "css",
        "javascript",
        "mode",
        "identity",
        "prompt",
        "route",
    ] {
        let mut changed = project.clone();
        match condition {
            "html" => changed.pages[0]
                .content
                .as_mut()
                .unwrap()
                .html
                .push_str("<p>New content</p>"),
            "css" => changed.pages[0]
                .content
                .as_mut()
                .unwrap()
                .css
                .push_str("button{color:red}"),
            "javascript" => changed.pages[0]
                .content
                .as_mut()
                .unwrap()
                .javascript
                .push_str("\nconsole.log('changed');"),
            "mode" => changed.generation_mode = GenerationMode::Strict,
            "identity" => changed.brief.brand_name = "Changed identity".into(),
            "prompt" => changed.pages[0].regions[0]
                .prompt
                .push_str(" Additional requirement."),
            "route" => changed.pages[1].path = "/changed".into(),
            _ => unreachable!(),
        }
        changed.revision += 1;
        assert!(
            candidate.validate(&snapshot, &changed).is_err(),
            "{condition}"
        );
    }
    let mut unrelated = project.clone();
    unrelated.pages[2].content = Some(source("Unrelated edit"));
    unrelated.show_grid = true;
    unrelated.revision += 1;
    candidate.validate(&snapshot, &unrelated).unwrap();
    let next = unrelated
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    assert_eq!(next.pages[2], unrelated.pages[2]);
    let mut tampered = snapshot.clone();
    tampered
        .dependencies
        .retain(|key, _| !key.starts_with("source:"));
    assert!(tampered.validate_current(&project).is_err());
}

#[test]
fn selected_decisions_and_source_commit_together_without_replacing_accepted_identity() {
    let (mut project, _, mut candidate) = fixture();
    project = project
        .apply(
            project.revision,
            Command::SetDecision {
                topic: "Voice".into(),
                value: "Keep the existing voice".into(),
                scope: Scope::Project,
                protected: true,
            },
        )
        .unwrap();
    let snapshot = project.prepare_requirements().unwrap();
    candidate.job_id = snapshot.job_id;
    candidate.requirements_sha256 = snapshot.sha256.clone();
    candidate.validate(&snapshot, &project).unwrap();
    assert!(
        project
            .apply_generation(&snapshot, &candidate, &[1])
            .is_err()
    );
    for invalid_selection in [vec![2], vec![0, 0]] {
        assert!(
            project
                .apply_generation(&snapshot, &candidate, &invalid_selection)
                .is_err()
        );
    }
    let next = project
        .apply_generation(&snapshot, &candidate, &[0])
        .unwrap();
    assert_eq!(next.revision, project.revision + 1);
    // The AI's one-line code is kept in a readable layout.
    let readable = format_site_content(&candidate.pages[0].content);
    assert_eq!(
        without_editor_ids(&next.pages[0].content.as_ref().unwrap().html),
        readable.html
    );
    assert_eq!(next.pages[0].content.as_ref().unwrap().css, readable.css);
    assert_eq!(
        next.pages[0].content.as_ref().unwrap().javascript,
        candidate.pages[0].content.javascript
    );
    let readable = format_site_content(&candidate.pages[1].content);
    assert_eq!(
        without_editor_ids(&next.pages[1].content.as_ref().unwrap().html),
        readable.html
    );
    assert_eq!(next.pages[1].content.as_ref().unwrap().css, readable.css);
    assert_eq!(
        next.pages[1].content.as_ref().unwrap().javascript,
        candidate.pages[1].content.javascript
    );
    assert_eq!(next.pages[2], project.pages[2]);
    assert_eq!(next.extensions, project.extensions);
    assert_eq!(next.decisions[0], project.decisions[0]);
    assert_eq!(next.decisions.len(), 2);
    assert_eq!(next.decisions[1].topic, "Control shape");
    assert_eq!(next.decisions[1].accepted_at_revision, next.revision);
    assert!(
        next.project_markdown()
            .unwrap()
            .contains("Square counter button")
    );
    assert_eq!(next.applied_job_ids, vec![candidate.job_id]);
    assert!(project.applied_job_ids.is_empty());
}

#[test]
fn decision_conflicts_follow_overlapping_scopes_and_do_not_block_disjoint_choices() {
    let (mut project, _, mut candidate) = fixture();
    let home = project.home_page_id;
    let details = project.pages[1].id;
    let region = project.pages[0].regions[0].id;
    project = project
        .apply(
            project.revision,
            Command::SetDecision {
                topic: "Control shape".into(),
                value: "Round control".into(),
                scope: Scope::Page(home),
                protected: false,
            },
        )
        .unwrap();
    let snapshot = project.prepare_requirements().unwrap();
    candidate.job_id = snapshot.job_id;
    candidate.requirements_sha256 = snapshot.sha256.clone();
    for scope in [Scope::Project, Scope::Page(home), Scope::Region(region)] {
        candidate.decision_proposals[0].scope = scope;
        assert!(
            project
                .apply_generation(&snapshot, &candidate, &[0])
                .is_err()
        );
    }
    candidate.decision_proposals[0].scope = Scope::Page(details);
    project
        .apply_generation(&snapshot, &candidate, &[0])
        .unwrap();
    candidate.decision_proposals[0].scope = Scope::Page(home);
    candidate.decision_proposals[0].value = "Round control".into();
    let unchanged_decision = project
        .apply_generation(&snapshot, &candidate, &[0])
        .unwrap();
    assert_eq!(unchanged_decision.decisions, project.decisions);
    candidate.decision_proposals = vec![
        DecisionProposal {
            topic: "New topic".into(),
            value: "One".into(),
            scope: Scope::Project,
            rationale: "A proposed global choice.".into(),
        },
        DecisionProposal {
            topic: "New topic".into(),
            value: "Two".into(),
            scope: Scope::Region(region),
            rationale: "An incompatible local choice.".into(),
        },
    ];
    assert!(
        project
            .apply_generation(&snapshot, &candidate, &[0, 1])
            .is_err()
    );
    candidate.decision_proposals[1].scope = Scope::Project;
    assert!(
        candidate.validate(&snapshot, &project).is_err(),
        "duplicate topics in one scope are invalid even if not selected"
    );
}

#[test]
fn generation_is_durable_before_commit_and_idempotent_through_undo_redo() {
    let (project, snapshot, candidate) = fixture();
    let mut session = Session::new(project.clone()).unwrap();
    let writes = Cell::new(0);
    assert!(
        session
            .apply_generation(&snapshot, &candidate, &[0], |next| {
                assert_eq!(next.revision, project.revision + 1);
                assert_eq!(next.applied_job_ids, vec![candidate.job_id]);
                assert_eq!(next.decisions.len(), 1);
                Err(Error("Durable storage unavailable".into()))
            })
            .is_err()
    );
    assert_eq!(session.project(), &project);
    assert!(!session.can_undo());
    session
        .apply_generation(&snapshot, &candidate, &[0], |_| {
            writes.set(writes.get() + 1);
            Ok(())
        })
        .unwrap();
    let applied = session.project().clone();
    session
        .apply_generation(&snapshot, &candidate, &[0], |_| {
            panic!("duplicate Apply must not write another revision")
        })
        .unwrap();
    assert_eq!(session.project(), &applied);
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, project.pages);
    assert_eq!(session.project().decisions, project.decisions);
    assert_eq!(session.project().applied_job_ids, vec![candidate.job_id]);
    assert_eq!(session.project().revision, applied.revision + 1);
    let undone = session.project().clone();
    session
        .apply_generation(&snapshot, &candidate, &[0], |_| {
            panic!("undo cannot re-enable paid job application")
        })
        .unwrap();
    assert_eq!(session.project(), &undone);
    assert!(session.can_redo());
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, applied.pages);
    assert_eq!(session.project().decisions, applied.decisions);
    assert_eq!(session.project().revision, undone.revision + 1);
    assert_eq!(writes.get(), 1);
}

#[test]
fn source_candidate_and_requirements_limits_fail_without_truncation() {
    let (project, snapshot, candidate) = fixture();
    assert!(GenerationCandidate::from_json(&" ".repeat(MAX_GENERATION_BYTES + 1)).is_err());
    for condition in ["source", "page", "null", "summary", "regions"] {
        let mut invalid = candidate.clone();
        match condition {
            "source" => invalid.pages[0].content.html = "é".repeat(MAX_SITE_SOURCE_BYTES / 2 + 1),
            "page" => {
                invalid.pages[0].content.html = "h".repeat(MAX_SITE_SOURCE_BYTES);
                invalid.pages[0].content.css = "c".repeat(MAX_SITE_SOURCE_BYTES);
                invalid.pages[0].content.javascript = "j".into();
            }
            "null" => invalid.pages[0].content.javascript.push('\0'),
            "summary" => invalid.summary = "a".repeat(16_385),
            "regions" => {
                invalid.regions = vec![invalid.regions[0].clone(); MAX_GENERATION_REGIONS + 1]
            }
            _ => unreachable!(),
        }
        assert!(
            invalid.validate(&snapshot, &project).is_err(),
            "{condition}"
        );
    }
    let mut excessive = project.clone();
    excessive.pages[0].regions = (0..MAX_GENERATION_REGIONS + 1)
        .map(|_| Region {
            shape: Default::default(),
            area_bounds: vec![],
            element: None,
            elements: vec![],
            allow_target_removal: false,
            mobile_only: false,
            whole_page: false,
            whole_page_disabled: false,
            attachments: vec![],
            completed: false,
            id: Uuid::new_v4(),
            rect: excessive.pages[0].regions[0].rect,
            z_index: 0,
            prompt: "Keep the exact requirement.".into(),
        })
        .collect();
    assert!(excessive.prepare_requirements().is_err());
    let mut large_sources = project.clone();
    for index in 0..5 {
        if index >= large_sources.pages.len() {
            large_sources = large_sources
                .apply(
                    large_sources.revision,
                    Command::AddPage {
                        name: format!("Large {index}"),
                        path: format!("/large-{index}"),
                    },
                )
                .unwrap();
        }
        let id = large_sources.pages[index].id;
        if large_sources.pages[index].regions.is_empty() {
            large_sources = describe(large_sources, id, "Update the existing content.");
        }
        large_sources.pages[index].content = Some(SiteContent {
            html: "h".repeat(MAX_SITE_SOURCE_BYTES),
            css: "c".repeat(MAX_SITE_SOURCE_BYTES),
            javascript: String::new(),
        });
    }
    let before = large_sources.clone();
    assert!(
        large_sources
            .prepare_requirements()
            .unwrap_err()
            .0
            .contains("no content was truncated")
    );
    assert_eq!(large_sources, before);
}

#[test]
fn successful_generation_builds_durable_draft_notes_and_request_history_without_applying_source_or_choices()
 {
    let (project, snapshot, candidate) = fixture();
    let mut session = Session::new(project.clone()).unwrap();
    session
        .record_generation(&snapshot, &candidate, 1, |next| {
            assert_eq!(next.pages, project.pages);
            assert_eq!(next.decisions, project.decisions);
            assert_eq!(next.revision, project.revision + 1);
            let notes = next.latest_generation.as_ref().unwrap();
            assert_eq!(notes.summary, candidate.summary);
            assert_eq!(notes.candidate_sha256, candidate.sha256().unwrap());
            assert_eq!(notes.base_revision, snapshot.base_revision);
            assert_eq!(notes.attempt_ordinal, 1);
            assert_eq!(notes.application_status, GenerationApplicationStatus::Draft);
            assert!(notes.accepted_decision_indices.is_empty());
            Ok(())
        })
        .unwrap();
    assert!(
        !session.can_undo(),
        "automatic notes are not an extra user edit"
    );
    assert!(
        session
            .project()
            .project_markdown()
            .unwrap()
            .contains(&candidate.summary)
    );
    assert!(
        session
            .project()
            .project_markdown()
            .unwrap()
            .contains("only the accepted decisions above are requirements")
    );
    candidate.validate(&snapshot, session.project()).unwrap();
    let history = earlier_requests(
        &snapshot,
        0,
        "drafted",
        &candidate.summary,
        &candidate.regions,
    );
    let next_request = session
        .project()
        .prepare_requirements()
        .unwrap()
        .with_earlier_requests(session.project(), &history, 7200)
        .unwrap();
    assert!(
        next_request
            .markdown
            .contains("## Earlier requests for these areas")
    );
    assert!(
        !next_request
            .markdown
            .contains("## Previous generation notes")
    );
    assert!(next_request.markdown.contains("\"outcome\":\"drafted\""));
    assert!(next_request.dossier_context.is_some());
    next_request.validate_current(session.project()).unwrap();
    let before = session.project().clone();
    session
        .record_generation(&snapshot, &candidate, 1, |_| {
            panic!("exact record retry must not write again")
        })
        .unwrap();
    assert_eq!(session.project(), &before);
    session
        .apply_generation(&snapshot, &candidate, &[0], |_| Ok(()))
        .unwrap();
    assert!(
        session.project().pages[0]
            .content
            .as_ref()
            .unwrap()
            .html
            .contains("Count")
    );
    let notes = session.project().latest_generation.as_ref().unwrap();
    assert_eq!(
        notes.application_status,
        GenerationApplicationStatus::Applied
    );
    assert_eq!(notes.accepted_decision_indices, vec![0]);
    assert_eq!(notes.applied_at_revision, Some(session.project().revision));
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, project.pages);
    assert_eq!(session.project().decisions, project.decisions);
    assert_eq!(
        session
            .project()
            .latest_generation
            .as_ref()
            .unwrap()
            .application_status,
        GenerationApplicationStatus::Undone
    );
    assert!(
        session
            .project()
            .applied_job_ids
            .contains(&candidate.job_id)
    );
    session.redo(|_| Ok(())).unwrap();
    let restored = session.project().latest_generation.as_ref().unwrap();
    assert_eq!(
        restored.application_status,
        GenerationApplicationStatus::Applied
    );
    assert_eq!(
        restored.applied_at_revision,
        Some(session.project().revision)
    );
    assert_eq!(restored.accepted_decision_indices, vec![0]);
}

#[test]
fn automatic_notes_preserve_user_history_and_failed_writes_leave_the_session_unchanged() {
    let (project, _, mut candidate) = fixture();
    let mut session = Session::new(project.clone()).unwrap();
    session
        .execute(
            project.revision,
            Command::SetGrid { enabled: false },
            |_| Ok(()),
        )
        .unwrap();
    session.undo(|_| Ok(())).unwrap();
    assert!(session.can_redo());
    assert!(!session.can_undo());
    let snapshot = session.project().prepare_requirements().unwrap();
    candidate.job_id = snapshot.job_id;
    candidate.requirements_sha256 = snapshot.sha256.clone();
    candidate.component_requests.clear();
    candidate.decision_proposals.clear();
    candidate.criticism.clear();
    candidate.validate(&snapshot, session.project()).unwrap();
    let before = session.project().clone();
    let write_attempted = Cell::new(false);
    assert!(
        session
            .record_generation(&snapshot, &candidate, 1, |_| {
                write_attempted.set(true);
                Err(Error("Disk full".into()))
            })
            .is_err()
    );
    assert!(write_attempted.get());
    assert_eq!(session.project(), &before);
    assert!(session.can_redo());
    assert!(!session.can_undo());
    session
        .record_generation(&snapshot, &candidate, 1, |_| Ok(()))
        .unwrap();
    assert!(session.can_redo());
    assert!(!session.can_undo());
    let notes = session.project().latest_generation.clone();
    session.redo(|_| Ok(())).unwrap();
    assert!(!session.project().show_grid);
    assert_eq!(session.project().latest_generation, notes);
    session.undo(|_| Ok(())).unwrap();
    assert!(session.project().show_grid);
    assert_eq!(session.project().latest_generation, notes);
    candidate.validate(&snapshot, session.project()).unwrap();
}

#[test]
fn a_user_edit_rebases_only_over_automatic_notes_and_never_over_another_user_edit() {
    let (project, snapshot, candidate) = fixture();
    let mut session = Session::new(project.clone()).unwrap();
    let renderer_revision = project.revision;
    session
        .record_generation(&snapshot, &candidate, 1, |_| Ok(()))
        .unwrap();
    let automatic_revision = session.project().revision;
    assert!(automatic_revision > renderer_revision);
    session.assert_revision(renderer_revision).unwrap();
    let command = Command::UpdateRegion {
        page_id: project.home_page_id,
        region_id: project.pages[0].regions[0].id,
        rect: project.pages[0].regions[0].rect,
        prompt: "The user can continue editing while generated notes arrive.".into(),
    };
    assert!(
        session
            .execute(renderer_revision, command.clone(), |_| Err(Error(
                "Disk full".into()
            )))
            .is_err()
    );
    assert_eq!(session.project().revision, automatic_revision);
    session.assert_revision(renderer_revision).unwrap();
    session
        .execute(renderer_revision, command, |_| Ok(()))
        .unwrap();
    assert_eq!(session.project().revision, automatic_revision + 1);
    assert!(
        session.project().pages[0].regions[0]
            .prompt
            .contains("continue editing")
    );
    assert!(session.assert_revision(renderer_revision).is_err());
    assert!(session.assert_revision(automatic_revision).is_err());
    assert!(
        session
            .assert_revision(session.project().revision + 1)
            .is_err()
    );
    assert!(
        session
            .execute(
                renderer_revision,
                Command::SetGrid { enabled: false },
                |_| panic!("stale user edits cannot reach durability")
            )
            .is_err()
    );
    session.undo(|_| Ok(())).unwrap();
    assert!(session.assert_revision(automatic_revision + 1).is_err());
    session.redo(|_| Ok(())).unwrap();
    assert!(session.assert_revision(automatic_revision + 2).is_err());
}

#[test]
fn explicit_retry_revises_a_draft_but_older_attempts_and_applied_rewrites_are_rejected() {
    let (project, snapshot, mut complete) = fixture();
    let mut blocked = complete.clone();
    blocked.regions[0].status = RegionStatus::Conflict;
    blocked.regions[1].status = RegionStatus::Conflict;
    blocked.regions[0].explanation = "The original description needs clarification.".into();
    blocked.pages.remove(0);
    blocked.summary = "The first attempt identified a conflict.".into();
    blocked.validate(&snapshot, &project).unwrap();
    let draft = project.record_generation(&snapshot, &blocked, 1).unwrap();
    complete.summary = "The explicit retry implemented both requests.".into();
    let revised = draft.record_generation(&snapshot, &complete, 2).unwrap();
    assert_eq!(revised.revision, draft.revision + 1);
    assert_eq!(
        revised.latest_generation.as_ref().unwrap().attempt_ordinal,
        2
    );
    assert_eq!(
        revised.latest_generation.as_ref().unwrap().summary,
        complete.summary
    );
    complete.validate(&snapshot, &revised).unwrap();
    assert!(revised.record_generation(&snapshot, &blocked, 1).is_err());
    let mut different = complete.clone();
    different.pages[0]
        .content
        .html
        .push_str("<p>A different response</p>");
    assert!(revised.record_generation(&snapshot, &different, 2).is_err());
    assert!(
        revised
            .apply_generation(&snapshot, &different, &[])
            .is_err()
    );
    assert!(project.record_generation(&snapshot, &complete, 0).is_err());
    let applied = revised.apply_generation(&snapshot, &complete, &[]).unwrap();
    assert!(applied.record_generation(&snapshot, &different, 3).is_err());
    assert_eq!(
        applied.record_generation(&snapshot, &complete, 2).unwrap(),
        applied
    );
}

#[test]
fn stale_or_invalid_generation_does_not_rewrite_current_project_notes() {
    let (project, snapshot, candidate) = fixture();
    let mut changed = project.clone();
    changed.pages[0].regions[0]
        .prompt
        .push_str(" New instructions.");
    changed.revision += 1;
    let before = changed.clone();
    assert!(changed.record_generation(&snapshot, &candidate, 1).is_err());
    assert_eq!(changed, before);
    let mut invalid = candidate.clone();
    invalid.regions.pop();
    assert!(project.record_generation(&snapshot, &invalid, 1).is_err());
    assert!(project.latest_generation.is_none());
}

#[test]
fn future_requirements_filter_previous_generated_context_to_the_requested_scope() {
    let (project, snapshot, mut candidate) = fixture();
    let details = project.pages[1].id;
    let details_region = project.pages[1].regions[0].id;
    candidate.summary = "PRIVATE CROSS PAGE SUMMARY".into();
    candidate.decision_proposals.push(DecisionProposal {
        topic: "Private detail identity".into(),
        value: "PRIVATE DETAIL VALUE".into(),
        scope: Scope::Page(details),
        rationale: "PRIVATE DETAIL RATIONALE".into(),
    });
    candidate.criticism.push(DesignCriticism {
        region_id: Some(details_region),
        message: "PRIVATE DETAIL CRITICISM".into(),
        suggestion: "PRIVATE DETAIL SUGGESTION".into(),
    });
    candidate.criticism.push(DesignCriticism {
        region_id: None,
        message: "PRIVATE CROSS PAGE CRITICISM".into(),
        suggestion: "PRIVATE CROSS PAGE SUGGESTION".into(),
    });
    let recorded = project.record_generation(&snapshot, &candidate, 1).unwrap();
    let rect = recorded.pages[1].regions[0].rect;
    let home_only = recorded
        .apply(
            recorded.revision,
            Command::UpdateRegion {
                page_id: details,
                region_id: details_region,
                rect,
                prompt: String::new(),
            },
        )
        .unwrap();
    let history = earlier_requests(
        &snapshot,
        1,
        "drafted",
        &candidate.summary,
        &candidate.regions,
    );
    // The job-wide summary is relevant while all of its original targets are requested.
    let all_scopes = recorded
        .prepare_requirements()
        .unwrap()
        .with_earlier_requests(&recorded, &history, 7200)
        .unwrap();
    assert!(all_scopes.request_history.contains(&candidate.summary));
    let future = home_only
        .prepare_requirements()
        .unwrap()
        .with_earlier_requests(&home_only, &history, 7200)
        .unwrap();
    future.validate_current(&home_only).unwrap();
    assert!(
        future
            .markdown
            .contains("## Earlier requests for these areas")
    );
    assert!(!future.markdown.contains("## Previous generation notes"));
    assert!(
        future
            .request_history
            .contains("An accessible native counter control.")
    );
    assert!(
        future
            .request_history
            .contains("Explain the counter below its button.")
    );
    assert!(
        !future
            .request_history
            .contains("A native disclosure explaining how the counter works.")
    );
    assert_eq!(future.request_history.matches("\"request\"").count(), 2);
    assert!(future.request_history.contains("\"summary\":null"));
    let context = future.dossier_context.as_ref().unwrap();
    assert_eq!(context.decision_proposals[0].value, "Square counter button");
    assert_eq!(context.decision_proposals[1].value, "Plain language");
    assert!(context.summary.is_none());
    for private in ["PRIVATE CROSS PAGE", "PRIVATE DETAIL"] {
        assert!(!future.markdown.contains(private));
    }
    assert_eq!(future.dossier_context.as_ref().unwrap().targets.len(), 2);
    let mut tampered = future.clone();
    tampered
        .dossier_context
        .as_mut()
        .unwrap()
        .decision_proposals[0]
        .value = "Invented identity".into();
    assert!(tampered.validate_current(&home_only).is_err());
}

#[test]
fn safe_default_identity_choices_respect_existing_rules_and_overlapping_proposals() {
    let (project, _, mut candidate) = fixture();
    let protected = project
        .apply(
            project.revision,
            Command::SetDecision {
                topic: "Voice".into(),
                value: "Keep this accepted voice".into(),
                scope: Scope::Project,
                protected: true,
            },
        )
        .unwrap();
    let snapshot = protected.prepare_requirements().unwrap();
    candidate.job_id = snapshot.job_id;
    candidate.requirements_sha256 = snapshot.sha256.clone();
    candidate.decision_proposals.extend([
        DecisionProposal {
            topic: " control shape ".into(),
            value: "Round control".into(),
            scope: Scope::Region(project.pages[0].regions[0].id),
            rationale: "An alternative the user may choose instead.".into(),
        },
        DecisionProposal {
            topic: "Control shape".into(),
            value: "Round control".into(),
            scope: Scope::Page(project.pages[1].id),
            rationale: "A separate page can make a separate choice.".into(),
        },
    ]);
    let selected = protected
        .compatible_decision_proposals(&snapshot, &candidate)
        .unwrap();
    assert_eq!(selected, vec![0, 3]);
    let applied = protected
        .apply_generation(&snapshot, &candidate, &selected)
        .unwrap();
    assert_eq!(applied.decisions[0], protected.decisions[0]);
    assert!(applied.decisions[0].protected);
    assert_eq!(
        applied
            .latest_generation
            .as_ref()
            .unwrap()
            .accepted_decision_indices,
        vec![0, 3]
    );
    let user_override = protected
        .apply_generation(&snapshot, &candidate, &[2, 3])
        .unwrap();
    assert!(
        user_override
            .decisions
            .iter()
            .filter(|decision| decision.topic.eq_ignore_ascii_case("control shape"))
            .all(|decision| decision.value == "Round control")
    );
    assert!(
        protected
            .apply_generation(&snapshot, &candidate, &[1])
            .is_err()
    );
}

#[test]
fn historical_notes_remain_valid_when_an_area_is_removed_and_cannot_claim_unaccepted_authority() {
    let (project, snapshot, candidate) = fixture();
    let recorded = project.record_generation(&snapshot, &candidate, 1).unwrap();
    let removed = recorded
        .apply(
            recorded.revision,
            Command::RemoveRegion {
                page_id: project.home_page_id,
                region_id: project.pages[0].regions[0].id,
            },
        )
        .unwrap();
    removed.validate().unwrap();
    assert_eq!(removed.latest_generation, recorded.latest_generation);
    assert!(snapshot.validate_current(&removed).is_err());
    for condition in [
        "accepted-draft",
        "future",
        "hash",
        "unknown-target",
        "duplicate-target",
    ] {
        let mut invalid = recorded.clone();
        let notes = invalid.latest_generation.as_mut().unwrap();
        match condition {
            "accepted-draft" => notes.accepted_decision_indices = vec![0],
            "future" => notes.recorded_at_revision = invalid.revision + 1,
            "hash" => notes.candidate_sha256 = "wrong".into(),
            "unknown-target" => notes.criticism[0].region_id = Some(Uuid::new_v4()),
            "duplicate-target" => notes.targets.push(notes.targets[0].clone()),
            _ => unreachable!(),
        }
        assert!(invalid.validate().is_err(), "{condition}");
    }
}

#[test]
fn canvas_updates_complete_only_submitted_requests_and_undo_restores_them() {
    let (mut project, snapshot, candidate) = fixture();
    let home = project.home_page_id;
    project = project
        .apply(
            project.revision,
            Command::AddRegion {
                page_id: home,
                rect: Rect {
                    x: -300.0,
                    y: 900.0,
                    width: 200.0,
                    height: 100.0,
                },
            },
        )
        .unwrap();
    let empty = project.pages[0].regions.last().unwrap().id;
    let mut session = Session::new(project.clone()).unwrap();
    session
        .apply_generation(&snapshot, &candidate, &[], |_| Ok(()))
        .unwrap();
    let applied = session.project();
    assert_eq!(applied.pending_count(), 0);
    assert!(applied.prepare_requirements().is_err());
    assert!(
        applied
            .pages
            .iter()
            .flat_map(|page| &page.regions)
            .filter(|r| r.id != empty)
            .all(|r| r.completed)
    );
    assert!(!applied.pages[0].regions.last().unwrap().completed);
    assert_eq!(applied.decisions, project.decisions);
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, project.pages);
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pending_count(), 0);
}

#[test]
fn element_requests_freeze_exact_source_and_selector_and_reject_stale_targets() {
    let (project, _, _) = fixture();
    let mut project = project;
    for page in &mut project.pages {
        for region in &mut page.regions {
            region.completed = true;
        }
    }
    let home = project.home_page_id;
    let element = ElementTarget {
        id: String::new(),
        selector: "body > main:nth-of-type(1) > button:nth-of-type(1)".into(),
        tag: "button".into(),
        label: "Existing home".into(),
        source_sha256: project.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    };
    let rect = Rect {
        x: 12.5,
        y: 20.0,
        width: 80.0,
        height: 32.0,
    };
    let add = Command::AddElementRegion {
        page_id: home,
        rect,
        element: element.clone(),
    };
    project = project.apply(project.revision, add.clone()).unwrap();
    let id = project.pages[0].regions.last().unwrap().id;
    project = project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id: home,
                region_id: id,
                rect,
                prompt: "Make this button green".into(),
            },
        )
        .unwrap();
    let snapshot = project.prepare_requirements().unwrap();
    assert_eq!(snapshot.region_count, 1);
    assert!(snapshot.markdown.contains(&element.selector));
    assert!(snapshot.markdown.contains(&element.source_sha256));
    snapshot.validate_current(&project).unwrap();
    project.pages[0].content = Some(source("Changed source"));
    assert!(project.apply(project.revision, add).is_err());
    assert!(snapshot.validate_current(&project).is_err());
    assert!(project.prepare_requirements().is_err());
    let mut invalid = element;
    invalid.selector = "body > script; parent".into();
    assert!(invalid.validate().is_err());
}

#[test]
fn selecting_another_element_archives_only_empty_selections_and_preserves_ids_for_undo() {
    let (mut project, _, _) = fixture();
    let page_id = project.home_page_id;
    let element = ElementTarget {
        id: String::new(),
        selector: "body".into(),
        tag: "body".into(),
        label: String::new(),
        source_sha256: project.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    };
    let command = Command::AddElementRegion {
        page_id,
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 120.0,
        },
        element,
    };
    project = project.apply(project.revision, command.clone()).unwrap();
    let empty = project.pages[0].regions.last().unwrap().id;
    project = project.apply(project.revision, command).unwrap();
    assert!(
        project.pages[0]
            .regions
            .iter()
            .find(|r| r.id == empty)
            .unwrap()
            .completed
    );
    assert!(!project.pages[0].regions.last().unwrap().completed);
    assert!(
        project.pages[0]
            .regions
            .iter()
            .filter(|r| !r.prompt.is_empty())
            .all(|r| !r.completed)
    );
}

#[test]
fn direct_local_source_edits_invalidate_an_already_frozen_generation() {
    let (project, snapshot, candidate) = fixture();
    let page = &project.pages[0];
    let target = ElementTarget {
        id: String::new(),
        selector: "body > main:nth-of-type(1)".into(),
        tag: "main".into(),
        label: "Existing home".into(),
        source_sha256: page.content.as_ref().unwrap().sha256().unwrap(),
    };
    let (edited, _) = project
        .edit_element(
            project.revision,
            page.id,
            &target,
            &ElementEdit::Update {
                texts: vec![],
                attributes: Default::default(),
                style: Some("padding:24px".into()),
            },
        )
        .unwrap();
    assert!(edited.apply_generation(&snapshot, &candidate, &[]).is_err());
    assert_eq!(
        edited.pages[0].content.as_ref().unwrap().javascript,
        page.content.as_ref().unwrap().javascript
    );
}

#[test]
fn selected_element_generation_preserves_surrounding_content_in_both_modes() {
    for mode in [GenerationMode::Fuzzy, GenerationMode::Strict] {
        let (mut project, _, _) = fixture();
        project.generation_mode = mode;
        for page in &mut project.pages {
            for region in &mut page.regions {
                region.completed = true;
            }
        }
        project.pages[0].content.as_mut().unwrap().html =
            "<main><button>Existing home</button><footer data-desegnos-region='f19de1f2-ec4c-425c-8813-284f77ed31b1'>Keep this exact footer</footer></main>"
                .into();
        let target = ElementTarget {
            id: String::new(),
            selector: "body > main:nth-of-type(1) > button:nth-of-type(1)".into(),
            tag: "button".into(),
            label: "Existing home".into(),
            source_sha256: project.pages[0].content.as_ref().unwrap().sha256().unwrap(),
        };
        let home = project.home_page_id;
        let rect = Rect {
            x: 24.0,
            y: 32.0,
            width: 280.0,
            height: 120.0,
        };
        project = project
            .apply(
                project.revision,
                Command::AddElementRegion {
                    page_id: home,
                    rect,
                    element: target,
                },
            )
            .unwrap();
        let id = project.pages[0].regions.last().unwrap().id;
        project = project
            .apply(
                project.revision,
                Command::UpdateRegion {
                    page_id: home,
                    region_id: id,
                    rect,
                    prompt: "Make this button green".into(),
                },
            )
            .unwrap();
        let snapshot = project.prepare_requirements().unwrap();
        let valid = GenerationCandidate {
            job_id: snapshot.job_id,
            requirements_sha256: snapshot.sha256.clone(),
            summary: "Changed the button.".into(),
            regions: vec![RegionOutcome {
                region_id: id,
                status: RegionStatus::Implemented,
                explanation: "Updated the requested button.".into(),
            }],
            pages: vec![GeneratedPage {
                page_id: home,
                content: SiteContent {
                    html: format!(
                        "<main><button data-desegnos-region='{id}' style='color:green'>Existing home</button><footer data-desegnos-region='f19de1f2-ec4c-425c-8813-284f77ed31b1'>Keep this exact footer</footer></main>"
                    ),
                    ..project.pages[0].content.clone().unwrap()
                },
            }],
            component_requests: vec![],
            decision_proposals: vec![],
            criticism: vec![],
        };
        valid.validate(&snapshot, &project).unwrap();
        let mut changed = valid.clone();
        changed.pages[0].content.html = changed.pages[0]
            .content
            .html
            .replace("Keep this exact footer", "Unrequested marketing slogan");
        assert!(changed.validate(&snapshot, &project).is_err());
        assert!(project.apply_generation(&snapshot, &changed, &[]).is_err());
    }
}
