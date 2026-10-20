use desegni_core::*;
use uuid::Uuid;

const PICTURE_BYTES: usize = 4096;

fn project() -> Project {
    let p = Project::new(
        "Element fixes".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Dark,
    )
    .unwrap();
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
    assert_eq!(s.format_version().unwrap(), ELEMENT_FIX_FORMAT);
    assert!(s.markdown.contains("## Selected elements"));
    assert!(!s.markdown.contains("## Existing affected page sources"));
    assert!(
        !s.markdown.contains("data:image/"),
        "pictures travel as references"
    );
    let at = s.markdown.find("dataset.ready");
    let heading = s.markdown[..at.unwrap_or(0)]
        .rfind("\n## ")
        .map(|i| &s.markdown[i..i + 60])
        .unwrap_or("");
    assert!(at.is_none(), "scripts are not sent: {heading}");
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
    assert_eq!(s.format_version().unwrap(), 10);
    assert!(s.markdown.contains("## Existing affected page sources"));
}
