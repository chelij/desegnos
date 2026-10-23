use desegni_core::*;
use serde_json::{Value, json};
use uuid::Uuid;

fn fixture(
    mode: GenerationMode,
    round: usize,
) -> (Project, RequirementsSnapshot, GenerationCandidate) {
    // Source captured read-only from job b65c3445/6b41b05f, both retained provider pages.
    let mut source = include_str!("fixtures/flexible-library.json").to_string();
    // The job retains image references, not original picture bytes. Substitute a local
    // one-pixel image for each reference; HTML structure, IDs, CSS and JS stay exact.
    let ids: Vec<_> = source
        .split("desegnos-asset:")
        .skip(1)
        .map(|p| p[..36].to_string())
        .collect();
    for id in ids {
        source = source.replace(&format!("desegnos-asset:{id}"), "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==");
    }
    let mut data: Value = serde_json::from_str(&source).unwrap();
    data["page"] = data["pages"][round].clone();
    let mut p = Project::new(
        "Library regression".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Dark,
    )
    .unwrap();
    p.generation_mode = mode;
    let page_id: Uuid = serde_json::from_value(data["page"]["pageId"].clone()).unwrap();
    p.home_page_id = page_id;
    p.pages[0].id = page_id;
    p.pages[0].content = Some(serde_json::from_value(data["base"].clone()).unwrap());
    // Register all pre-existing roots, including completed boxes outside these two requests.
    let html = &p.pages[0].content.as_ref().unwrap().html;
    let mut ids = Vec::new();
    for part in html.split("data-desegnos-region=\"").skip(1) {
        let id: Uuid = part.split('"').next().unwrap().parse().unwrap();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    for id in ids {
        p = p
            .apply(
                p.revision,
                Command::AddRegion {
                    page_id,
                    rect: Rect {
                        x: 20.,
                        y: 20.,
                        width: 100.,
                        height: 100.,
                    },
                },
            )
            .unwrap();
        let r = p.pages[0].regions.last_mut().unwrap();
        r.id = id;
        r.completed = true;
    }
    for request in data["requests"].as_array().unwrap() {
        let id: Uuid = serde_json::from_value(request["regionId"].clone()).unwrap();
        if !p.pages[0].regions.iter().any(|r| r.id == id) {
            p = p
                .apply(
                    p.revision,
                    Command::AddRegion {
                        page_id,
                        rect: serde_json::from_value(request["placementHint"].clone()).unwrap(),
                    },
                )
                .unwrap();
            p.pages[0].regions.last_mut().unwrap().id = id;
        }
        let r = p.pages[0].regions.iter_mut().find(|r| r.id == id).unwrap();
        r.rect = serde_json::from_value(request["placementHint"].clone()).unwrap();
        r.prompt = request["prompt"].as_str().unwrap().into();
        r.completed = false;
        r.whole_page_disabled = true;
    }
    let s = p.prepare_requirements().unwrap();
    let c = GenerationCandidate::from_json(&json!({
        "jobId":s.job_id,"requirementsSha256":s.sha256,"summary":"Library updated",
        "regions":data["requests"].as_array().unwrap().iter().map(|r| json!({"regionId":r["regionId"],"status":"implemented","explanation":"Updated"})).collect::<Vec<_>>(),
        "pages":[data["page"]],"componentRequests":[],"decisionProposals":[],"criticism":[]
    }).to_string()).unwrap();
    (p, s, c)
}

#[test]
fn real_library_candidates_apply_flexible_as_one_undoable_full_source_change() {
    for round in 0..2 {
        let (p, s, c) = fixture(GenerationMode::Fuzzy, round);
        assert!(s.is_flexible());
        assert!(!s.is_element_fix());
        assert!(
            s.markdown
                .contains("never move it to another part of the page")
        );
        assert!(!s.markdown.contains("8 CSS pixels"));
        assert!(!s.markdown.contains("authoritative location"));
        assert!(!s.markdown.contains("Shared JavaScript cannot change"));
        let (accepted, events) =
            recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
        assert!(accepted.is_ready(), "{events:?}");
        assert!(
            events
                .iter()
                .any(|e| e.kind == "outsideChanges" && e.disposition == "advisory")
        );
        assert_eq!(accepted.pages[0].content.css, c.pages[0].content.css);
        assert_eq!(
            accepted.pages[0].content.javascript,
            c.pages[0].content.javascript
        );
        assert!(
            retain_generation_drafts(
                &serde_json::to_string(&c).unwrap(),
                &s,
                &p,
                Some(&accepted),
                &events
            )
            .is_empty()
        );
        let applied = p.apply_generation(&s, &accepted, &[]).unwrap();
        let mut session = Session::new(p.clone()).unwrap();
        let mut writes = 0;
        session
            .apply_generation(&s, &accepted, &[], |_| {
                writes += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(writes, 1);
        session.undo(|_| Ok(())).unwrap();
        assert_eq!(session.project().pages[0].content, p.pages[0].content);
        assert!(!session.can_undo());
        applied.validate().unwrap();
        validate_page_source(applied.pages[0].content.as_ref().unwrap()).unwrap();
    }
}

#[test]
fn real_library_candidate_still_reports_scope_changes_in_precise() {
    for round in 0..2 {
        let (p, s, c) = fixture(GenerationMode::Strict, round);
        assert!(!s.is_flexible());
        let (accepted, events) =
            recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
        assert!(!accepted.is_ready());
        assert!(
            events.iter().any(|e| matches!(
                e.kind.as_str(),
                "cssScope" | "sharedCss" | "outsideScope" | "elementIdentity"
            )),
            "{events:?}"
        );
    }
}

fn small_fixture(mode: GenerationMode) -> (Project, RequirementsSnapshot, GenerationCandidate) {
    let mut p = Project::new(
        "Integrity".into(),
        Viewport {
            width: 1000,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    p.generation_mode = mode;
    for _ in 0..2 {
        p = p
            .apply(
                p.revision,
                Command::AddRegion {
                    page_id: p.home_page_id,
                    rect: Rect {
                        x: 20.,
                        y: 20.,
                        width: 100.,
                        height: 100.,
                    },
                },
            )
            .unwrap();
        let region = p.pages[0].regions.last_mut().unwrap();
        region.prompt = "Update the item".into();
        region.whole_page_disabled = true;
    }
    let id = p.pages[0].regions[0].id;
    let pending = p.pages[0].regions[1].id;
    p.pages[0].content = Some(SiteContent {
        html: format!(
            "<main data-desegnos-id='main0001'><section data-desegnos-id='root0001' data-desegnos-region='{id}'><p data-desegnos-id='item0001'>Original</p></section><aside data-desegnos-id='aside001' data-desegnos-region='{pending}'>Pending</aside></main>"
        ),
        css: "p { color: navy; }".into(),
        javascript: "document.body.dataset.ready = 'yes';".into(),
    });
    let s = p
        .prepare_targeted_requirements(GenerationCapabilities::default(), &[id])
        .unwrap();
    let c = GenerationCandidate::from_json(&json!({"jobId":s.job_id,"requirementsSha256":s.sha256,"summary":"Updated",
        "regions":[{"regionId":id,"status":"implemented","explanation":"Updated"}],
        "pages":[{"pageId":p.home_page_id,"content":p.pages[0].content}],"componentRequests":[],"decisionProposals":[],"criticism":[]}).to_string()).unwrap();
    (p, s, c)
}

#[test]
fn duplicate_existing_ids_keep_best_original_even_when_copy_is_first_in_both_modes() {
    for mode in [GenerationMode::Fuzzy, GenerationMode::Strict] {
        let (p, s, mut c) = small_fixture(mode);
        c.pages[0].content.html = c.pages[0].content.html.replace("<p data-desegnos-id='item0001'>Original</p>",
            "<span data-desegnos-id='item0001'>Copy</span><p data-desegnos-id='item0001'>Changed</p><p data-desegnos-id='item0001'>Original</p><p data-desegnos-id='newcopy1'>New</p><p data-desegnos-id='newcopy1'>New copy</p>");
        let (accepted, events) =
            recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
        assert!(accepted.is_ready(), "{events:?}");
        assert!(
            events
                .iter()
                .any(|e| e.kind == "elementIdentity" && e.disposition == "repaired")
        );
        let result = p.apply_generation(&s, &accepted, &[]).unwrap();
        let map = result.page_source_map(p.home_page_id).unwrap();
        let original = map.ranges.iter().find(|r| r.id == "item0001").unwrap();
        assert!(
            original.selector.ends_with("p:nth-of-type(2)"),
            "{}",
            original.selector
        );
        let ids: std::collections::BTreeSet<_> = map.ranges.iter().map(|r| &r.id).collect();
        assert_eq!(ids.len(), map.ranges.len());
    }
}

#[test]
fn removed_pending_markers_are_restored_in_both_modes() {
    for mode in [GenerationMode::Fuzzy, GenerationMode::Strict] {
        let (p, s, mut c) = small_fixture(mode);
        let pending = p.pages[0].regions[1].id;
        c.pages[0].content.html = c.pages[0]
            .content
            .html
            .replace(&format!(" data-desegnos-region='{pending}'"), "");
        let (accepted, events) =
            recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
        assert!(accepted.is_ready(), "{events:?}");
        assert!(
            events
                .iter()
                .any(|e| e.kind == "regionRoot" && e.disposition == "repaired")
        );
        let result = p.apply_generation(&s, &accepted, &[]).unwrap();
        assert!(
            result.pages[0]
                .content
                .as_ref()
                .unwrap()
                .html
                .contains(&pending.to_string())
        );
        assert!(!result.pages[0].regions[1].completed);
    }
}

#[test]
fn duplicate_region_roots_are_repaired_in_both_modes() {
    for mode in [GenerationMode::Fuzzy, GenerationMode::Strict] {
        let (p, s, mut c) = small_fixture(mode);
        let id = p.pages[0].regions[0].id;
        c.pages[0].content.html = c.pages[0].content.html.replace(
            "</section>",
            &format!("<div data-desegnos-region='{id}'>Copy</div></section>"),
        );
        let (accepted, events) =
            recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
        assert!(accepted.is_ready(), "{events:?}");
        assert!(
            events
                .iter()
                .any(|e| e.kind == "regionRoot" && e.disposition == "repaired")
        );
        p.apply_generation(&s, &accepted, &[]).unwrap();
    }
}

#[test]
fn flexible_preserves_full_shared_code_and_reports_diff_without_blocking() {
    let (p, s, mut c) = small_fixture(GenerationMode::Fuzzy);
    c.pages[0].content.css = "p { color: red; } aside { background: white; }".into();
    c.pages[0].content.javascript = "document.body.dataset.ready = 'updated';".into();
    c.pages[0].content.html = c.pages[0].content.html.replace("Pending", "Related update");
    let (accepted, events) =
        recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
    assert_eq!(accepted, c);
    let message = &events
        .iter()
        .find(|e| e.kind == "outsideChanges")
        .unwrap()
        .message;
    assert!(
        message.contains("page styles, page script, Box 2"),
        "{message}"
    );
    p.apply_generation(&s, &accepted, &[]).unwrap();
}

#[test]
fn broken_css_html_and_javascript_still_fail_in_flexible() {
    let (p, s, c) = small_fixture(GenerationMode::Fuzzy);
    for field in ["html", "css", "javascript"] {
        let mut value = serde_json::to_value(&c).unwrap();
        value["pages"][0]["content"][field] = match field {
            "html" => "<div a='unterminated>".into(),
            "css" => "p { color: red; /*".into(),
            _ => "const broken = ;".into(),
        };
        let result = recover_generation_reply(&value.to_string(), &s, &p);
        assert!(
            result.is_err() || !result.as_ref().unwrap().0.is_ready(),
            "{field}: {result:?}"
        );
    }
}

#[test]
fn runtime_repair_keeps_shared_script_fixes_in_both_modes() {
    for mode in [GenerationMode::Fuzzy, GenerationMode::Strict] {
        let (mut p, _, mut c) = small_fixture(mode);
        let region_id = p.pages[0].regions[0].id;
        p = p
            .apply(
                p.revision,
                Command::SetRegionRuntimeErrors {
                    page_id: p.home_page_id,
                    region_id,
                    errors: vec!["TypeError: missing chat handler".into()],
                },
            )
            .unwrap();
        let s = p
            .prepare_targeted_requirements(GenerationCapabilities::default(), &[region_id])
            .unwrap();
        c.job_id = s.job_id;
        c.requirements_sha256 = s.sha256.clone();
        c.pages[0].content.javascript = "document.body.dataset.fixed = 'yes';".into();
        let (accepted, events) =
            recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
        assert!(accepted.is_ready(), "{events:?}");
        assert_eq!(
            accepted.pages[0].content.javascript,
            c.pages[0].content.javascript
        );
        p.apply_generation(&s, &accepted, &[]).unwrap();
    }
}
