use desegni_core::*;
use uuid::Uuid;

const PICTURE_BYTES: usize = 4096;

fn project() -> Project {
    let mut p = Project::new(
        "Element fixes".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Dark,
    )
    .unwrap();
    p.generation_mode = GenerationMode::Strict;
    let picture = format!(
        "data:image/webp;base64,{}",
        "QUFB".repeat(PICTURE_BYTES / 4)
    );
    p.edit_page_source(
        0,
        p.home_page_id,
        &PageSourceEdit {
            html: Some(format!(
                "<main><section class=\"portrait\"><img class=\"portrait-slot\" src=\"{picture}\" alt=\"Me\"><h1>What I can do</h1></section><p>Unrelated</p></main>"
            )),
            css: Some(".portrait-slot{background:linear-gradient(#223129,#22312940)}".into()),
            javascript: Some("document.body.dataset.ready='1'".into()),
        },
    )
    .unwrap()
}

fn target(p: &Project, suffix: &str) -> ElementTarget {
    let map = p.page_source_map(p.home_page_id).unwrap();
    let range = map
        .ranges
        .iter()
        .find(|r| r.selector.ends_with(suffix))
        .unwrap();
    ElementTarget {
        id: range.id.clone(),
        tag: suffix.split(':').next().unwrap().into(),
        label: "Selected".into(),
        selector: range.selector.clone(),
        source_sha256: String::new(),
    }
}

fn describe(p: Project, element: ElementTarget, prompt: &str) -> Project {
    let home = p.home_page_id;
    let p = p
        .apply(
            p.revision,
            Command::AddElementRegions {
                page_id: home,
                elements: vec![element.clone()],
                bounds: vec![TargetBounds {
                    target: element,
                    rect: Rect {
                        x: 40.,
                        y: 300.,
                        width: 200.,
                        height: 200.,
                    },
                }],
                allow_target_removal: false,
            },
        )
        .unwrap();
    let r = p.pages[0].regions.last().unwrap().clone();
    p.apply(
        p.revision,
        Command::UpdateRegion {
            page_id: home,
            region_id: r.id,
            rect: r.rect,
            prompt: prompt.into(),
        },
    )
    .unwrap()
}

fn reply(
    p: &Project,
    s: &RequirementsSnapshot,
    pages: serde_json::Value,
    needs: &[&str],
) -> String {
    serde_json::json!({
        "jobId": s.job_id, "requirementsSha256": s.sha256, "summary": "Removed the frame background",
        "regions": p.pages[0].regions.iter().map(|r| serde_json::json!({"regionId": r.id, "status": "implemented", "explanation": "Done"})).collect::<Vec<_>>(),
        "pages": pages, "needs": needs, "componentRequests": [], "decisionProposals": [], "criticism": []
    })
    .to_string()
}

fn fixture() -> (Project, ElementTarget, RequirementsSnapshot) {
    let p = project();
    let image = target(&p, "img:nth-of-type(1)");
    let p = describe(p, image.clone(), "remove that background");
    let s = p.prepare_requirements().unwrap();
    (p, image, s)
}

#[test]
fn an_element_fix_sends_only_the_selection_stylesheet_and_outline() {
    let (_, image, s) = fixture();
    assert_eq!(s.format_version().unwrap(), 24);
    assert!(s.markdown.contains("## Selected elements"));
    assert!(!s.markdown.contains("## Existing affected page sources"));
    assert!(
        !s.markdown.contains("data:image/"),
        "pictures travel as references"
    );
    assert!(s.markdown.contains("javascriptReadOnly") && s.markdown.contains("dataset.ready"));
    assert!(s.markdown.contains("## Region scripts"));
    assert!(s.markdown.contains(&image.id) && s.markdown.contains("portrait-slot"));
    assert!(s.markdown.contains("\"outline\"") && s.markdown.contains("What I can do"));
    let schema = result_schema(&s);
    assert!(schema["properties"]["pages"]["items"]["properties"]["elements"].is_object());
    assert_eq!(schema["properties"]["needs"]["maxItems"], 32);
}

#[test]
fn a_reply_rebuilds_the_whole_page_and_passes_normal_validation() {
    let (p, image, s) = fixture();
    let original = p.pages[0].content.clone().unwrap();
    let html = format!(
        "<img class=\"portrait-slot\" style=\"background:none\" data-desegnos-id=\"{}\" src=\"desegnos-asset:{}\" alt=\"Me\">",
        image.id,
        s.markdown
            .split("desegnos-asset:")
            .filter_map(|rest| rest.get(..36))
            .find(|id| Uuid::parse_str(id).is_ok())
            .unwrap()
    );
    let text = reply(
        &p,
        &s,
        serde_json::json!([{"pageId": p.home_page_id, "elements": [{"id": image.id, "html": html}]}]),
        &[],
    );
    let mut candidate = GenerationCandidate::from_reply(&text, &s, &p).unwrap();
    candidate.resolve_project_assets(&p).unwrap();
    candidate.validate(&s, &p).unwrap();
    let page = &candidate.pages[0].content;
    assert_eq!(page.css, original.css);
    assert!(page.html.contains("background:none"));
    assert_eq!(page.javascript, original.javascript);
    assert!(page.html.contains("data:image/webp;base64,") && page.html.contains("Unrelated"));
    let expanded = serde_json::to_string(&candidate).unwrap();
    assert_eq!(
        GenerationCandidate::from_reply(&expanded, &s, &p).unwrap(),
        candidate,
        "image preparation hands on expanded results"
    );
}

#[test]
fn a_reply_cannot_touch_unselected_elements_or_leave_open_questions() {
    let (p, _, s) = fixture();
    let heading = target(&p, "h1:nth-of-type(1)");
    let text = reply(
        &p,
        &s,
        serde_json::json!([{"pageId": p.home_page_id, "elements": [{"id": heading.id, "html": "<h1>Hacked</h1>"}]}]),
        &[],
    );
    assert!(GenerationCandidate::from_reply(&text, &s, &p).is_err());
    let asking = reply(&p, &s, serde_json::json!([]), &[&heading.id]);
    assert!(GenerationCandidate::from_reply(&asking, &s, &p).is_err());
}

#[test]
fn asking_for_another_element_gets_one_automatic_follow_up() {
    let (p, image, s) = fixture();
    let heading = target(&p, "h1:nth-of-type(1)");
    let asking = reply(&p, &s, serde_json::json!([]), &[&heading.id, "missing-id"]);
    let follow_up = s
        .context_follow_up(&asking, &p)
        .unwrap()
        .expect("a follow-up");
    assert!(
        follow_up.markdown.starts_with(&s.markdown)
            && follow_up.markdown.contains("What I can do</h1>")
    );
    assert!(follow_up.markdown.contains("\"missing\": true"));
    assert_ne!(follow_up.sha256, s.sha256);
    assert_eq!(
        result_schema(&follow_up)["properties"]["needs"]["maxItems"],
        0
    );
    assert!(
        follow_up.context_follow_up(&asking, &p).unwrap().is_none(),
        "only one follow-up"
    );
    let final_reply = reply(
        &p,
        &follow_up,
        serde_json::json!([{"pageId": p.home_page_id, "elements": []}]),
        &[],
    );
    let adopted = s.adopt_follow_up_reply(&follow_up, &final_reply).unwrap();
    let candidate = GenerationCandidate::from_reply(&adopted, &s, &p).unwrap();
    assert_eq!(candidate.requirements_sha256, s.sha256);
    candidate.validate(&s, &p).unwrap();
    let stale = reply(&p, &s, serde_json::json!([]), &[]);
    assert!(s.adopt_follow_up_reply(&follow_up, &stale).is_err());
    let _ = (image, Uuid::nil());
}

#[test]
fn new_areas_and_phone_fixes_still_send_the_full_page() {
    let p = project();
    let home = p.home_page_id;
    let p = p
        .apply(
            p.revision,
            Command::AddRegion {
                page_id: home,
                rect: Rect {
                    x: 0.,
                    y: 0.,
                    width: 100.,
                    height: 100.,
                },
            },
        )
        .unwrap();
    let r = p.pages[0].regions[0].clone();
    let p = p
        .apply(
            p.revision,
            Command::UpdateRegion {
                page_id: home,
                region_id: r.id,
                rect: r.rect,
                prompt: "Add a footer".into(),
            },
        )
        .unwrap();
    let s = p.prepare_requirements().unwrap();
    assert_eq!(s.format_version().unwrap(), 24);
    assert!(s.markdown.contains("## Existing affected page sources"));
}

#[test]
fn format_twelve_omits_schema_text_and_only_accepts_selected_id_css_additions() {
    let (p, image, s) = fixture();
    assert!(!s.markdown.contains("\"additionalProperties\""));
    let schema = result_schema(&s);
    assert_eq!(
        schema["properties"]["pages"]["items"]["properties"]["css"]["type"],
        "string"
    );
    for css in [
        format!("[data-desegnos-id=\"{}\"]{{border:0}}", image.id),
        format!(
            "@media(max-width:600px){{[data-desegnos-id=\"{}\"]:hover::before{{content:'x'}}}}",
            image.id
        ),
    ] {
        let text = reply(
            &p,
            &s,
            serde_json::json!([{"pageId":p.home_page_id,"elements":[],"css":css}]),
            &[],
        );
        GenerationCandidate::from_reply(&text, &s, &p)
            .unwrap()
            .validate(&s, &p)
            .unwrap();
    }
    for css in [
        "body{display:none}".to_owned(),
        format!("[data-desegnos-id=\"{}\"],body{{color:red}}", image.id),
        format!("[data-desegnos-id=\"{}\"] + p{{color:red}}", image.id),
        format!("[data-desegnos-id=\"{}\"] p{{color:red}}", image.id),
        format!("[data-desegnos-id=\"{}\"]{{& + p{{color:red}}}}", image.id),
        "@font-face{font-family:x;src:url(https://example.com/font)}".to_owned(),
        "@media(max-width:600px){body{display:none}}".to_owned(),
    ] {
        let text = reply(
            &p,
            &s,
            serde_json::json!([{"pageId":p.home_page_id,"elements":[],"css":css}]),
            &[],
        );
        assert!(
            GenerationCandidate::from_reply(&text, &s, &p)
                .unwrap()
                .validate(&s, &p)
                .is_err()
        );
    }
}

#[test]
fn format_fourteen_sends_compact_sources_and_keeps_results_readable() {
    let p = project();
    // A page stored in the readable layout Desegnos keeps after a generation.
    let readable = format_site_content(p.pages[0].content.as_ref().unwrap());
    assert!(readable.html.contains("\n  <section"));
    let p = p
        .edit_page_source(
            p.revision,
            p.home_page_id,
            &PageSourceEdit {
                html: Some(readable.html.clone()),
                css: Some(readable.css.clone()),
                javascript: None,
            },
        )
        .unwrap();
    let image = target(&p, "img:nth-of-type(1)");
    let p = describe(p, image.clone(), "remove that background");
    let s = p.prepare_requirements().unwrap();
    assert_eq!(s.format_version().unwrap(), 24);
    assert!(s.is_element_fix());
    let sent = s.markdown.split("\n## Selected elements\n").nth(1).unwrap();
    assert!(
        !sent.contains("\\n  <") && !sent.contains("{\\n"),
        "layout whitespace was sent"
    );
    let addition = format!("[data-desegnos-id=\"{}\"]{{border:0}}", image.id);
    let html = format!(
        "<img class=\"portrait-slot\" style=\"background:none\" data-desegnos-id=\"{}\" src=\"desegnos-asset:{}\" alt=\"Me\">",
        image.id,
        sent.split("desegnos-asset:")
            .filter_map(|rest| rest.get(..36))
            .find(|id| Uuid::parse_str(id).is_ok())
            .unwrap()
    );
    let text = reply(
        &p,
        &s,
        serde_json::json!([{"pageId": p.home_page_id, "elements": [{"id": image.id, "html": html}], "css": addition}]),
        &[],
    );
    let mut candidate = GenerationCandidate::from_reply(&text, &s, &p).unwrap();
    candidate.resolve_project_assets(&p).unwrap();
    candidate.validate(&s, &p).unwrap();
    let applied = p.apply_generation(&s, &candidate, &[]).unwrap();
    let stored = applied.pages[0].content.as_ref().unwrap();
    assert!(stored.html.contains("background:none") && stored.html.contains("\n  <p"));
    assert!(stored.css.contains("border: 0") && stored.css.contains('\n'));
    assert_eq!(stored.javascript, readable.javascript);
    // A later request sends the same page compact again.
    assert_eq!(
        compact_site_content(stored),
        compact_site_content(&format_site_content(stored))
    );
}

fn behavior_fixture(code: &str) -> (Project, RequirementsSnapshot, String) {
    let p = project();
    let p = p.edit_page_source(p.revision, p.home_page_id, &PageSourceEdit {
        html: Some("<main><section class=\"shelf\"><article data-date=\"2025-01-01\"><h1>Older project</h1></article><article data-date=\"2026-10-22\"><h1>Newest project</h1></article></section><p>Unrelated region</p></main>".into()),
        ..Default::default()
    }).unwrap();
    let shelf = target(&p, "section:nth-of-type(1)");
    let p = describe(p, shelf.clone(), "want this to autosort newest at the top");
    let s = p.prepare_requirements().unwrap();
    let region = p.pages[0].regions.last().unwrap();
    let content = compact_site_content(p.pages[0].content.as_ref().unwrap());
    // Keep the selected source and all retained descendant IDs exactly as supplied.
    let doc = dom_query::Document::from(content.html.as_str());
    let selected = doc.select(&format!("[data-desegnos-id=\"{}\"]", shelf.id));
    selected.nodes()[0].append_html(format!(
        "<script type=\"application/x-desegnos-region\" data-desegnos-script=\"{}\">{code}</script>", region.id
    ));
    let text = reply(
        &p,
        &s,
        serde_json::json!([{
            "pageId": p.home_page_id,
            "elements": [{"id":shelf.id,"html":selected.nodes()[0].html().to_string()}], "css":""
        }]),
        &[],
    );
    (p, s, text)
}

const AUTOSORT: &str = "const cards = Array.from(root.querySelectorAll('[data-date]')); cards.sort((a, b) => Date.parse(b.dataset.date) - Date.parse(a.dataset.date)); cards.forEach(card => root.appendChild(card));";

#[test]
fn local_sorting_is_accepted_and_undo_removes_its_owned_source() {
    let (p, s, text) = behavior_fixture(AUTOSORT);
    let (candidate, events) = recover_generation_reply(&text, &s, &p).unwrap();
    assert!(
        events.iter().all(|e| e.disposition != "conflicted"),
        "{events:?}"
    );
    assert_eq!(candidate.regions[0].status, RegionStatus::Implemented);
    candidate.validate(&s, &p).unwrap();
    assert_eq!(
        candidate.pages[0].content.javascript,
        p.pages[0].content.as_ref().unwrap().javascript
    );
    let original = p.pages[0].content.clone();
    let mut session = Session::new(p).unwrap();
    session
        .apply_generation(&s, &candidate, &[], |_| Ok(()))
        .unwrap();
    assert!(
        session.project().pages[0]
            .content
            .as_ref()
            .unwrap()
            .html
            .contains("data-desegnos-script")
    );
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages[0].content, original);
}

const CURSOR_PORTRAIT: &str = r#"
    const portrait = root.querySelector('img');
    document.addEventListener('pointermove', event => {
        const box = root.getBoundingClientRect();
        const direction = event.clientX < box.left + box.width / 2 ? 'left' : 'right';
        portrait.src = direction + '.png';
        portrait.style.transform = 'translateY(' + event.clientY / window.innerHeight + 'px)';
    });
"#;

#[test]
fn cursor_portrait_can_listen_across_the_page_and_write_only_its_root() {
    let (p, s, text) = behavior_fixture(CURSOR_PORTRAIT);
    let (candidate, events) = recover_generation_reply(&text, &s, &p).unwrap();
    assert_eq!(
        candidate.regions[0].status,
        RegionStatus::Implemented,
        "{events:?}"
    );
    candidate.validate(&s, &p).unwrap();
    let applied = p.apply_generation(&s, &candidate, &[]).unwrap();
    assert!(
        applied.pages[0]
            .content
            .as_ref()
            .unwrap()
            .html
            .contains(CURSOR_PORTRAIT)
    );
}

#[test]
fn outside_writes_and_global_event_cancellation_are_accepted() {
    for action in [
        "document.body.style.background = 'red';",
        "const body = document.body; body.classList.add('changed');",
        "document.documentElement.setAttribute('data-changed', 'yes');",
        "document.body.appendChild(root.querySelector('img'));",
        "document.querySelector('p').textContent = 'changed';",
        "document.querySelectorAll('p').forEach(node => node.remove());",
        "const nodes = Array.from(document.querySelectorAll('p')); nodes.forEach(node => node.remove());",
        "function change(node) { node.textContent = 'changed'; } change(document.body);",
        "function outsideNode() { return document.body; } outsideNode().style.color = 'red';",
        "const change = node => node.remove(); document.querySelectorAll('p').forEach(change);",
        "const holder = { node: document.body }; holder.node.style.color = 'red';",
        "root.closest('main').classList.add('changed');",
        "event.target.style.color = 'red';",
        "event.preventDefault();",
        "event.stopPropagation();",
        "window.innerWidth = 0;",
        "event.currentTarget.top;",
    ] {
        let code = CURSOR_PORTRAIT.replace("const box =", &format!("{action} const box ="));
        let (p, s, text) = behavior_fixture(&code);
        let (candidate, events) = recover_generation_reply(&text, &s, &p).unwrap();
        assert_eq!(candidate.pages.len(), 1, "{action}: {events:?}");
        assert_eq!(candidate.regions[0].status, RegionStatus::Implemented);
        candidate.validate(&s, &p).unwrap();
    }
}

#[test]
fn root_queries_without_matching_descendants_are_allowed() {
    // The page has an outside paragraph, but a root query cannot return it.
    let (p, s, text) = behavior_fixture("root.querySelector('p')?.remove();");
    let (candidate, _) = recover_generation_reply(&text, &s, &p).unwrap();
    assert_eq!(candidate.pages.len(), 1);
    assert_eq!(candidate.regions[0].status, RegionStatus::Implemented);
    candidate.validate(&s, &p).unwrap();
}

#[test]
fn region_scripts_can_use_page_and_browser_apis() {
    for code in [
        "document.querySelector('p').remove();",
        "root.parentNode.querySelector('p').remove();", // Reaches the paragraph outside this shelf.
        "root.parentElement.remove();",
        "root.querySelectorAll('h1').forEach(card => card.ownerDocument.body.remove());",
        "import('https://example.com/remote.js');",
        "fetch('https://example.com/remote.js');",
        "eval('root.remove()');",
        "new Function('root.remove()')();",
        "document.write('outside');",
        "document.cookie;",
        "window.top;",
        "window.parent;",
        "const key = 'constructor'; root[key]('return document')();",
        "Math.random = () => 0;",
        "Math.random.call = () => 0;",
        "const shared = Math; shared.random = () => 0;",
        "const shared = Number; shared.parseInt = () => 0;",
        "const globalEvent = event; globalEvent.target.remove();",
    ] {
        let (p, s, text) = behavior_fixture(code);
        let (candidate, events) = recover_generation_reply(&text, &s, &p).unwrap();
        assert_eq!(candidate.pages.len(), 1, "{code}: {events:?}");
        assert_eq!(candidate.regions[0].status, RegionStatus::Implemented);
        candidate.validate(&s, &p).unwrap();
    }
}

#[test]
fn malformed_region_script_is_retained_as_an_unapplied_draft() {
    let (p, s, text) = behavior_fixture("const = ;");
    let (candidate, events) = recover_generation_reply(&text, &s, &p).unwrap();
    assert!(candidate.pages.is_empty());
    let drafts = retain_generation_drafts(&text, &s, &p, Some(&candidate), &events);
    assert_eq!(drafts.len(), 1);
    assert!(!drafts[0].applied);
}

#[test]
fn export_runs_region_functions_after_unchanged_page_script() {
    let (p, s, text) = behavior_fixture("root.querySelector('h1').classList.toggle('active');");
    let (candidate, _) = recover_generation_reply(&text, &s, &p).unwrap();
    let p = p.apply_generation(&s, &candidate, &[]).unwrap();
    let export = compile_website(&p).unwrap();
    let html = String::from_utf8(export.files["index.html"].clone()).unwrap();
    assert!(html.find("assets/page-").unwrap() < html.find("assets/regions-").unwrap());
    let (_, bytes) = export
        .files
        .iter()
        .find(|(name, _)| name.starts_with("assets/regions-"))
        .unwrap();
    let runtime = std::str::from_utf8(bytes).unwrap();
    assert!(
        runtime.contains("function(root, window, document, setTimeout, requestAnimationFrame)")
            && runtime.contains("block.parentElement")
    );
    assert!(
        runtime.contains("})(regionRoot, window, document, window.setTimeout.bind(window), window.requestAnimationFrame.bind(window))")
            && runtime.contains("catch (error)")
    );
    assert!(runtime.contains("root.querySelector('h1')"));
    let page_js = export
        .files
        .iter()
        .find(|(name, _)| name.starts_with("assets/page-"))
        .unwrap()
        .1;
    assert_eq!(
        page_js,
        p.pages[0].content.as_ref().unwrap().javascript.as_bytes()
    );
}

#[test]
fn area_behavior_preserves_shared_javascript() {
    let (mut p, _, _) = behavior_fixture(AUTOSORT);
    let region = &mut p.pages[0].regions[0];
    let id = region.id;
    region.element = None;
    region.elements.clear();
    let doc = dom_query::Document::from(p.pages[0].content.as_ref().unwrap().html.as_str());
    doc.select("section").nodes()[0].set_attr("data-desegnos-region", &id.to_string());
    p.pages[0].content.as_mut().unwrap().html =
        doc.select("body").nodes()[0].inner_html().to_string();
    let s = p.prepare_requirements().unwrap();
    assert!(!s.is_element_fix());
    let mut content = p.pages[0].content.clone().unwrap();
    let doc = dom_query::Document::from(content.html.as_str());
    doc.select("section").nodes()[0].append_html(format!("<script type=\"application/x-desegnos-region\" data-desegnos-script=\"{id}\">{AUTOSORT}</script>"));
    content.html = doc.select("body").nodes()[0].inner_html().to_string();
    let text = serde_json::json!({
        "jobId":s.job_id,"requirementsSha256":s.sha256,"summary":"Sort projects locally",
        "regions":[{"regionId":id,"status":"implemented","explanation":"Sorted"}],
        "pages":[{"pageId":p.home_page_id,"content":content}],
        "componentRequests":[],"decisionProposals":[],"criticism":[]
    })
    .to_string();
    let (candidate, _) = recover_generation_reply(&text, &s, &p).unwrap();
    assert_eq!(candidate.regions[0].status, RegionStatus::Implemented);
    assert_eq!(
        candidate.pages[0].content.javascript,
        p.pages[0].content.as_ref().unwrap().javascript
    );
    candidate.validate(&s, &p).unwrap();
}

#[test]
fn script_ownership_cannot_be_assigned_to_another_request() {
    let (p, s, text) = behavior_fixture(AUTOSORT);
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let html = value["pages"][0]["elements"][0]["html"].as_str().unwrap();
    value["pages"][0]["elements"][0]["html"] = html
        .replace(
            &format!("data-desegnos-script=\"{}\"", p.pages[0].regions[0].id),
            &format!("data-desegnos-script=\"{}\"", Uuid::new_v4()),
        )
        .into();
    let text = value.to_string();
    let (candidate, events) = recover_generation_reply(&text, &s, &p).unwrap();
    assert!(candidate.pages.is_empty());
    assert_eq!(
        retain_generation_drafts(&text, &s, &p, Some(&candidate), &events).len(),
        1
    );
}

#[test]
fn element_fix_shared_script_reference_is_bounded_and_never_written_back() {
    let (mut p, _, _) = behavior_fixture(AUTOSORT);
    let original = format!(
        "  // retained shared source\n{}",
        "window.shared = 1;\n".repeat(1000)
    );
    p.pages[0].content.as_mut().unwrap().javascript = original.clone();
    let s = p.prepare_requirements().unwrap();
    assert!(s.markdown.contains("javascriptReadOnly"));
    assert!(s.markdown.contains("\"truncated\": true"));
    assert_eq!(p.pages[0].content.as_ref().unwrap().javascript, original);
}
