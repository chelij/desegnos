use desegni_core::*;
use serde_json::{Value, json};
use uuid::Uuid;

fn fixture(existing: bool) -> (Project, RequirementsSnapshot, GenerationCandidate) {
    let saved: Value = serde_json::from_str(include_str!("fixtures/requirements8.json")).unwrap();
    let mut project: Project = serde_json::from_value(saved["project"].clone()).unwrap();
    project.schema_version = SCHEMA_VERSION;
    let template = project.pages[0].regions[0].clone();
    project.pages[0].regions = (0..3)
        .map(|_| {
            let mut r = template.clone();
            r.id = Uuid::new_v4();
            r
        })
        .collect();
    let html = project.pages[0]
        .regions
        .iter()
        .map(|r| {
            format!(
                "<section data-desegnos-region='{}'>Original</section>",
                r.id
            )
        })
        .collect::<String>();
    project.pages[0].content = existing.then(|| SiteContent {
        html,
        css: "body{color:navy}".into(),
        javascript: "document.body.dataset.ready='1';".into(),
    });
    let snapshot = project.prepare_requirements().unwrap();
    let candidate = GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Updated areas".into(),
        regions: project.pages[0]
            .regions
            .iter()
            .map(|r| RegionOutcome {
                region_id: r.id,
                status: RegionStatus::Implemented,
                explanation: "Updated".into(),
            })
            .collect(),
        pages: vec![GeneratedPage {
            page_id: project.pages[0].id,
            content: SiteContent {
                html: project.pages[0]
                    .regions
                    .iter()
                    .map(|r| format!("<section data-desegnos-region='{}'>Updated</section>", r.id))
                    .collect(),
                css: project.pages[0]
                    .content
                    .as_ref()
                    .map_or(String::new(), |c| c.css.clone()),
                javascript: project.pages[0]
                    .content
                    .as_ref()
                    .map_or(String::new(), |c| c.javascript.clone()),
            },
        }],
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],
    };
    (project, snapshot, candidate)
}

fn recover(
    c: &GenerationCandidate,
    s: &RequirementsSnapshot,
    p: &Project,
) -> (GenerationCandidate, Vec<ValidationEvent>) {
    recover_generation_reply(&serde_json::to_string(c).unwrap(), s, p).unwrap()
}

#[test]
fn real_empty_css_partial_reply_preserves_two_ready_areas_and_conflict() {
    let (p, s, mut c) = fixture(true);
    let id = c.regions[2].region_id;
    c.regions[2].status = RegionStatus::Conflict;
    c.regions[2].explanation = "Choose a photo".into();
    c.pages[0].content.html = c.pages[0].content.html.replace(
        &format!("<section data-desegnos-region='{id}'>Updated</section>"),
        &format!("<section data-desegnos-region='{id}'>Original</section>"),
    );
    c.pages[0].content.css.clear();
    let (accepted, notes) = recover(&c, &s, &p);
    assert_eq!(
        accepted.pages[0].content.css,
        p.pages[0].content.as_ref().unwrap().css
    );
    assert!(
        notes
            .iter()
            .any(|n| n.message == "Kept the stylesheet unchanged (the AI omitted it).")
    );
    let applied = p.apply_generation(&s, &accepted, &[]).unwrap();
    assert!(applied.pages[0].regions[0].completed);
    assert!(applied.pages[0].regions[1].completed);
    assert!(!applied.pages[0].regions[2].completed);
    assert_eq!(accepted.regions[2].explanation, "Choose a photo");
}

#[test]
fn shared_rule_dropped_while_region_rule_kept() {
    let (p, s, mut c) = fixture(true);
    let id = c.regions[0].region_id;
    c.pages[0].content.css.push_str(&format!(
        "\n[data-desegnos-region=\"{id}\"]{{color:red}} .unrelated{{color:blue}}"
    ));
    let (accepted, notes) = recover(&c, &s, &p);
    assert!(accepted.pages[0].content.css.contains("color: red"));
    assert!(!accepted.pages[0].content.css.contains("unrelated"));
    assert!(
        notes
            .iter()
            .any(|n| n.kind == "sharedCss" && n.disposition == "salvaged")
    );
    accepted.validate(&s, &p).unwrap();
}

#[test]
fn real_accessible_wrapper_is_valid_without_repair() {
    let (p, s, mut c) = fixture(false);
    c.pages[0].content.html = format!(
        "<main aria-label=\"Cheli's Library\">{}</main>",
        c.pages[0].content.html
    );
    let (accepted, events) = recover(&c, &s, &p);
    assert!(accepted.is_ready());
    assert!(s.validation_repair(&events).unwrap().is_none());
}

#[test]
fn scope_failure_salvages_others_and_requests_one_precise_repair() {
    let (p, s, mut c) = fixture(false);
    let id = c.regions[1].region_id;
    c.pages[0].content.html = c.pages[0].content.html.replace(
        &format!("<section data-desegnos-region='{id}'>"),
        &format!("<section data-desegnos-region='{id}' data-desegnos-id='inventedid'>"),
    );
    let (accepted, events) = recover(&c, &s, &p);
    assert_eq!(
        accepted
            .regions
            .iter()
            .filter(|r| r.status == RegionStatus::Implemented)
            .count(),
        2
    );
    assert!(accepted.can_apply(&s));
    let follow = s.validation_repair(&events).unwrap().unwrap();
    assert!(follow.markdown.contains(&id.to_string()));
    assert!(follow.markdown.contains("editor ID"));
    assert!(follow.markdown.contains("data-desegnos-id"));
    assert!(follow.markdown.contains("Excerpt:"));
    assert!(follow.validation_repair(&events).unwrap().is_none());
    assert_eq!(MAX_VALIDATION_REPAIR_ROUNDS, 1);
}

#[test]
fn repair_output_is_validated_and_cannot_overwrite_accepted_areas() {
    let (p, s, mut bad) = fixture(false);
    let id = bad.regions[1].region_id;
    bad.pages[0].content.html = bad.pages[0].content.html.replace(
        &format!("<section data-desegnos-region='{id}'>Updated</section>"),
        "",
    );
    let (first, events) = recover(&bad, &s, &p);
    let follow = s.validation_repair(&events).unwrap().unwrap();
    let mut second = first.clone();
    second.requirements_sha256 = follow.sha256.clone();
    second.regions[1].status = RegionStatus::Implemented;
    second.pages[0].content.html = second
        .regions
        .iter()
        .map(|region| {
            format!(
                "<section data-desegnos-region=\"{}\">{}</section>",
                region.region_id,
                if region.region_id == id {
                    "Repaired"
                } else {
                    "Unwanted second-round change"
                }
            )
        })
        .collect();
    let adopted = s
        .adopt_follow_up_reply(&follow, &serde_json::to_string(&second).unwrap())
        .unwrap();
    let (second, _) = recover_generation_reply(&adopted, &s, &p).unwrap();
    let merged = merge_validation_repair(&first, &second, &s, &p).unwrap();
    merged.validate(&s, &p).unwrap();
    assert!(merged.is_ready());
    assert!(merged.pages[0].content.html.contains("Repaired"));
    assert!(merged.pages[0].content.html.contains("Updated"));
    assert!(!merged.pages[0].content.html.contains("Unwanted"));
}

#[test]
fn missing_fields_pages_and_regions_preserve_originals() {
    let (p, s, c) = fixture(true);
    let mut v = serde_json::to_value(&c).unwrap();
    v["pages"][0]["content"]
        .as_object_mut()
        .unwrap()
        .remove("css");
    v["pages"][0]["content"]["html"] = "".into();
    let (kept, events) = recover_generation_reply(&v.to_string(), &s, &p).unwrap();
    assert_eq!(kept.pages[0].content, p.pages[0].content.clone().unwrap());
    assert_eq!(
        events.iter().filter(|e| e.kind == "omittedField").count(),
        2
    );
    v["pages"] = json!([]);
    v["regions"].as_array_mut().unwrap().pop();
    let (kept, events) = recover_generation_reply(&v.to_string(), &s, &p).unwrap();
    assert!(kept.is_ready());
    assert!(events.iter().any(|e| e.kind == "omittedRegion"));
    assert!(events.iter().any(|e| e.kind == "omittedPage"));
}

#[test]
fn malformed_reply_has_repair_context_then_explicit_conflicts_without_source() {
    let (p, s, _) = fixture(false);
    let error = recover_generation_reply("{bad", &s, &p).unwrap_err();
    let event = ValidationEvent {
        kind: "replyStructure".into(),
        disposition: "conflicted".into(),
        region_id: None,
        message: error.to_string(),
    };
    assert!(s.validation_repair(&[event]).unwrap().is_some());
    let conflicts = conflicted_generation(&s, &p, &error.to_string()).unwrap();
    assert!(!conflicts.can_apply(&s));
    assert_eq!(conflicts.regions.len(), 3);
    assert!(conflicts.pages.is_empty());
}

#[test]
fn older_requirements_bytes_are_untouched_and_recovery_is_disabled() {
    for bytes in [
        include_str!("fixtures/requirements8.json"),
        include_str!("fixtures/requirements9.json"),
    ] {
        let fixture: Value = serde_json::from_str(bytes).unwrap();
        let snapshot: RequirementsSnapshot =
            serde_json::from_value(fixture["requirements"].clone()).unwrap();
        let original = serde_json::to_vec(&snapshot).unwrap();
        assert!(
            snapshot
                .validation_repair(&[ValidationEvent {
                    kind: "replyStructure".into(),
                    disposition: "conflicted".into(),
                    region_id: None,
                    message: "Invalid format".into()
                }])
                .unwrap()
                .is_none()
        );
        assert_eq!(serde_json::to_vec(&snapshot).unwrap(), original);
        assert_eq!(snapshot.sha256, digest(snapshot.markdown.as_bytes()));
    }
}

#[test]
fn dropped_css_dependency_conflicts_the_affected_area_instead_of_stripping_its_design() {
    let (p, s, mut c) = fixture(false);
    let id = c.regions[0].region_id;
    c.pages[0].content.html = c.pages[0].content.html.replace(
        &format!("data-desegnos-region='{id}'"),
        &format!("data-desegnos-region='{id}' class='card'"),
    );
    c.pages[0].content.css = ".card{color:red}".into();
    let (accepted, events) = recover(&c, &s, &p);
    assert_eq!(accepted.regions[0].status, RegionStatus::Conflict);
    assert_eq!(
        accepted
            .regions
            .iter()
            .filter(|r| r.status == RegionStatus::Implemented)
            .count(),
        2
    );
    assert!(events.iter().any(|e| e.region_id == Some(id)
        && e.message.contains(".card")
        && e.message.contains("Excerpt:")));
    assert!(s.validation_repair(&events).unwrap().is_some());
}

#[test]
fn mixed_media_query_preserves_scoped_rule_and_drops_unrelated_rule() {
    let (p, s, mut c) = fixture(true);
    let id = c.regions[0].region_id;
    c.pages[0].content.css.push_str(&format!("@media (max-width:600px){{[data-desegnos-region=\"{id}\"]{{color:red}}.unrelated{{color:blue}}}}"));
    let (accepted, _) = recover(&c, &s, &p);
    assert!(accepted.pages[0].content.css.contains("@media"));
    assert!(accepted.pages[0].content.css.contains("color: red"));
    assert!(!accepted.pages[0].content.css.contains("unrelated"));
}

#[test]
fn invalid_page_schema_keeps_other_pages_applicable() {
    let (mut p, _, mut c) = fixture(true);
    let mut other = p.pages[0].clone();
    other.id = Uuid::new_v4();
    other.name = "Other".into();
    other.path = "/other".into();
    for r in &mut other.regions {
        r.id = Uuid::new_v4();
    }
    p.pages.push(other.clone());
    let s = p.prepare_requirements().unwrap();
    c.job_id = s.job_id;
    c.requirements_sha256 = s.sha256.clone();
    c.regions
        .extend(other.regions.iter().map(|r| RegionOutcome {
            region_id: r.id,
            status: RegionStatus::Implemented,
            explanation: "Updated".into(),
        }));
    let mut raw = serde_json::to_value(c).unwrap();
    raw["pages"].as_array_mut().unwrap().push(
        json!({"pageId":other.id,"content":{"html":"<p>Broken</p>","css":42,"javascript":""}}),
    );
    let (accepted, events) = recover_generation_reply(&raw.to_string(), &s, &p).unwrap();
    assert_eq!(accepted.pages.len(), 1);
    assert_eq!(accepted.pages[0].page_id, p.home_page_id);
    assert_eq!(
        accepted
            .regions
            .iter()
            .filter(|r| r.status == RegionStatus::Implemented)
            .count(),
        3
    );
    assert!(
        events
            .iter()
            .any(|e| e.kind == "pageSource" && e.message.contains(&other.id.to_string()))
    );
}

#[test]
fn scalar_reply_and_scalar_page_content_become_structural_errors_or_conflicts() {
    let (p, s, c) = fixture(false);
    assert!(
        recover_generation_reply("[]", &s, &p)
            .unwrap_err()
            .to_string()
            .contains("JSON object")
    );
    let mut raw = serde_json::to_value(c).unwrap();
    raw["pages"][0]["content"] = 42.into();
    let (accepted, events) = recover_generation_reply(&raw.to_string(), &s, &p).unwrap();
    assert!(accepted.pages.is_empty());
    assert!(
        accepted
            .regions
            .iter()
            .all(|r| r.status == RegionStatus::Conflict)
    );
    assert!(s.validation_repair(&events).unwrap().is_some());
}

#[test]
fn formatting_outside_regions_is_equivalent_but_visible_text_is_preserved() {
    let (mut p, _, mut c) = fixture(true);
    p.pages[0]
        .content
        .as_mut()
        .unwrap()
        .html
        .push_str("<aside class='menu' aria-label='Menu'><img alt='Logo' src='#'></aside>");
    c.pages[0].content.html.push_str(
        "<aside aria-label=\"Menu\" class=\"menu\"><img src=\"#\" alt=\"Logo\" /></aside>",
    );
    let s = p.prepare_requirements().unwrap();
    c.job_id = s.job_id;
    c.requirements_sha256 = s.sha256.clone();
    let (accepted, _) = recover(&c, &s, &p);
    assert!(accepted.is_ready());
    accepted.validate(&s, &p).unwrap();
    // A real outside change is discarded by reconstruction, preserving the source text.
    c.pages[0].content.html = c.pages[0].content.html.replace("Menu", "Changed");
    let (accepted, events) = recover(&c, &s, &p);
    assert!(accepted.is_ready());
    assert!(accepted.pages[0].content.html.contains("Menu"));
    assert!(!accepted.pages[0].content.html.contains("Changed"));
    assert!(events.iter().any(|e| e.kind == "outsideScope"));
}

#[test]
fn invalid_remote_code_never_survives_region_salvage() {
    let (p, s, mut c) = fixture(false);
    let id = c.regions[0].region_id;
    c.pages[0].content.html = c.pages[0].content.html.replace(&format!("<section data-desegnos-region='{id}'>Updated</section>"), &format!("<section data-desegnos-region='{id}'><script src='https://example.org/code.js'></script></section>"));
    let (accepted, _) = recover(&c, &s, &p);
    assert_eq!(accepted.regions[0].status, RegionStatus::Conflict);
    assert!(!accepted.pages[0].content.html.contains("example.org"));
    assert_eq!(
        accepted
            .regions
            .iter()
            .filter(|r| r.status == RegionStatus::Implemented)
            .count(),
        2
    );
}

fn retained_css_draft() -> (Project, RequirementsSnapshot, GenerationDraft) {
    let (project, snapshot, mut candidate) = fixture(true);
    candidate.pages[0].content.css = "body{color:red}".into();
    let reply = serde_json::to_string(&candidate).unwrap();
    let (safe, events) = recover_generation_reply(&reply, &snapshot, &project).unwrap();
    let drafts = retain_generation_drafts(&reply, &snapshot, &project, Some(&safe), &events);
    assert!(!drafts.is_empty());
    (project, snapshot, drafts.into_iter().next().unwrap())
}

#[test]
fn retained_draft_preserves_exact_source_and_reloads() {
    let (project, _, draft) = retained_css_draft();
    let bytes = serde_json::to_vec(&draft).unwrap();
    let loaded: GenerationDraft = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(loaded.id, draft.id);
    assert_eq!(loaded.content, draft.content);
    assert_eq!(loaded.code, draft.code);
    assert!(!loaded.reason.is_empty());
    loaded.check(&project).unwrap();
}

#[test]
fn retained_draft_apply_is_one_durable_undo_step_and_cannot_replay() {
    let (project, snapshot, draft) = retained_css_draft();
    let original = project.pages[0].content.clone();
    let mut session = Session::new(project).unwrap();
    let mut writes = 0;
    session
        .apply_generation_draft(&snapshot, &draft, |_| {
            writes += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(writes, 1);
    assert_eq!(
        session.project().pages[0].content.as_ref().unwrap().css,
        "body{color:red}"
    );
    assert!(session.project().applied_job_ids.contains(&draft.id));
    assert_eq!(
        session.project().latest_generation.as_ref().unwrap().job_id,
        draft.id
    );
    session
        .apply_generation_draft(&snapshot, &draft, |_| panic!("replayed"))
        .unwrap();
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages[0].content, original);
    assert!(!session.can_undo());
    assert!(session.project().applied_job_ids.contains(&draft.id));
}

#[test]
fn blocked_draft_source_remains_copyable_and_cannot_apply() {
    let (project, snapshot, draft) = retained_css_draft();
    for html in [
        "<script src='https://example.com/code.js'></script>",
        "<button onclick=\"import('https://example.com/code.js')\">Run</button>",
        "<p data-desegnos-id='newidentity'>Invented ID</p>",
        "<div><span></div>",
    ] {
        let mut unsafe_draft = draft.clone();
        unsafe_draft.content.as_mut().unwrap().html = html.into();
        unsafe_draft.code = html.into();
        assert!(unsafe_draft.check(&project).is_err(), "{html}");
        assert!(
            project
                .apply_generation_draft(&snapshot, &unsafe_draft)
                .is_err()
        );
        let reloaded: GenerationDraft =
            serde_json::from_slice(&serde_json::to_vec(&unsafe_draft).unwrap()).unwrap();
        assert_eq!(reloaded.code, html);
    }
}

#[test]
fn stale_draft_revalidates_current_page_and_restored_source_fits_again() {
    let (project, snapshot, draft) = retained_css_draft();
    let mut changed = project.clone();
    changed.revision += 1;
    changed.pages[0]
        .content
        .as_mut()
        .unwrap()
        .html
        .push_str("<p>Later edit</p>");
    assert!(
        draft
            .check(&changed)
            .unwrap_err()
            .to_string()
            .contains("no longer fits")
    );
    assert!(changed.apply_generation_draft(&snapshot, &draft).is_err());
    changed.pages[0].content = project.pages[0].content.clone();
    draft.check(&changed).unwrap();
    // Revision-only changes elsewhere do not make this page stale.
    changed.apply_generation_draft(&snapshot, &draft).unwrap();
}

#[test]
fn dismissed_draft_is_retained_but_cannot_apply() {
    let (project, snapshot, mut draft) = retained_css_draft();
    let code = draft.code.clone();
    draft.dismissed = true;
    let loaded: GenerationDraft =
        serde_json::from_slice(&serde_json::to_vec(&draft).unwrap()).unwrap();
    assert!(loaded.dismissed);
    assert_eq!(loaded.code, code);
    assert!(project.apply_generation_draft(&snapshot, &loaded).is_err());
}

#[test]
fn protected_items_are_named_in_draft_confirmation() {
    let (mut project, _, draft) = retained_css_draft();
    project.decisions.push(Decision {
        id: Uuid::new_v4(),
        topic: "Brand heading".into(),
        value: "Keep the title".into(),
        scope: Scope::Project,
        protected: true,
        accepted_at_revision: project.revision,
    });
    assert!(draft.confirmation(&project).contains("Brand heading"));
}

#[test]
fn local_dom_javascript_is_overridable_but_code_loaders_are_copy_only() {
    let (project, snapshot, draft) = retained_css_draft();
    let mut local = draft.clone();
    local.content.as_mut().unwrap().javascript = "document.querySelector('button').addEventListener('click', () => { document.body.classList.toggle('open'); });".into();
    project.apply_generation_draft(&snapshot, &local).unwrap();
    for javascript in [
        "import('https://example.com/code.js')",
        "eval('alert(1)')",
        "document.createElement('script').src='https://example.com/code.js'",
        "document['createElement']('script')",
        "const run = document.querySelector.constructor; run('alert(1)')()",
    ] {
        let mut blocked = draft.clone();
        blocked.content.as_mut().unwrap().javascript = javascript.into();
        assert!(blocked.check(&project).is_err(), "{javascript}");
        assert!(project.apply_generation_draft(&snapshot, &blocked).is_err());
    }
}
