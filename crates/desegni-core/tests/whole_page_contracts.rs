use desegni_core::*;
use uuid::Uuid;

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}
fn add(mut p: Project, bounds: Rect, prompt: &str) -> Project {
    p = p
        .apply(
            p.revision,
            Command::AddRegion {
                page_id: p.home_page_id,
                rect: bounds,
            },
        )
        .unwrap();
    let region = p.pages[0].regions.last().unwrap();
    p.apply(
        p.revision,
        Command::UpdateRegion {
            page_id: p.home_page_id,
            region_id: region.id,
            rect: region.rect,
            prompt: prompt.into(),
        },
    )
    .unwrap()
}
fn fixture() -> Project {
    let mut p = Project::new(
        "Whole page".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap();
    p.pages[0].content = Some(SiteContent {
        html: "<main data-desegnos-id=\"main0001\"><p data-desegnos-id=\"keep0001\">Protected copy</p><footer data-desegnos-id=\"tail0001\">Tail</footer></main>".into(),
        css: "main { color: green; }".into(), javascript: String::new(),
    });
    add(p, rect(0., -8., 1440., 912.), "make it grayscale, no green")
}
fn candidate(p: &Project, s: &RequirementsSnapshot) -> GenerationCandidate {
    GenerationCandidate {
        job_id: s.job_id,
        requirements_sha256: s.sha256.clone(),
        summary: "Grayscale page".into(),
        regions: p.pages[0]
            .regions
            .iter()
            .map(|r| RegionOutcome {
                region_id: r.id,
                status: RegionStatus::Implemented,
                explanation: "Implemented".into(),
            })
            .collect(),
        pages: vec![GeneratedPage {
            page_id: p.home_page_id,
            content: SiteContent {
                html: format!(
                    "<section>{}</section>",
                    p.pages[0]
                        .content
                        .as_ref()
                        .unwrap()
                        .html
                        .replace("<main ", "<article ")
                        .replace("</main>", "</article>")
                ),
                css: "article { color: gray; min-height: 100vh; }".into(),
                javascript: "document.body.dataset.ready = 'yes';".into(),
            },
        }],
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],
    }
}
#[test]
fn coverage_classifies_the_reported_viewport_small_and_tall_areas() {
    let mut p = fixture();
    assert!(p.pages[0].regions[0].is_whole_page(&p.pages[0]));
    for (bounds, expected) in [
        (rect(0., 0., 1368., 810.), true),
        (rect(0., 0., 1367., 810.), false),
        (rect(40., 40., 300., 200.), false),
        (rect(1440., 0., 1440., 900.), false),
        (rect(0., 900., 1440., 900.), false),
        (rect(0., -10., 1440., 2800.), true),
    ] {
        p.pages[0].regions[0].rect = bounds;
        assert_eq!(
            p.pages[0].regions[0].is_whole_page(&p.pages[0]),
            expected,
            "{bounds:?}"
        );
    }
}
#[test]
fn measured_content_height_and_page_width_drive_detection() {
    let mut p = fixture();
    let target = ElementTarget {
        id: "main0001".into(),
        selector: "body > main:nth-of-type(1)".into(),
        tag: "main".into(),
        label: "Page".into(),
        source_sha256: String::new(),
    };
    let id = p.pages[0].regions[0].id;
    p = p
        .refresh_element_target_bounds(
            p.home_page_id,
            p.viewport,
            &[ElementTargetBounds {
                region_id: id,
                bounds: vec![TargetBounds {
                    target,
                    rect: rect(0., 0., 1440., 3000.),
                }],
            }],
        )
        .unwrap();
    // Not near the viewport top, but covers 90% of measured page content.
    p.pages[0].regions[0].rect = rect(0., 200., 1440., 2800.);
    assert!(p.pages[0].regions[0].is_whole_page(&p.pages[0]));
    p.pages[0].regions[0].area_bounds[0].rect.width = 1800.;
    assert!(!p.pages[0].regions[0].is_whole_page(&p.pages[0]));
}
#[test]
fn explicit_toggle_forces_scope_and_remembers_auto_detection_opt_out() {
    let mut p = fixture();
    let id = p.pages[0].regions[0].id;
    p = p
        .apply(
            p.revision,
            Command::SetRegionWholePage {
                page_id: p.home_page_id,
                region_id: id,
                whole_page: false,
            },
        )
        .unwrap();
    assert!(!p.pages[0].regions[0].is_whole_page(&p.pages[0]));
    let saved = serde_json::to_string(&p).unwrap();
    let restored: Project = serde_json::from_str(&saved).unwrap();
    assert!(!restored.pages[0].regions[0].is_whole_page(&restored.pages[0]));
    p.pages[0].regions[0].rect = rect(20., 20., 100., 100.);
    p = p
        .apply(
            p.revision,
            Command::SetRegionWholePage {
                page_id: p.home_page_id,
                region_id: id,
                whole_page: true,
            },
        )
        .unwrap();
    assert!(p.pages[0].regions[0].is_whole_page(&p.pages[0]));
    let mut legacy = serde_json::to_value(&p.pages[0].regions[0]).unwrap();
    legacy.as_object_mut().unwrap().remove("wholePage");
    legacy.as_object_mut().unwrap().remove("wholePageDisabled");
    let legacy: Region = serde_json::from_value(legacy).unwrap();
    assert!(!legacy.whole_page && !legacy.whole_page_disabled);
}
#[test]
fn page_job_accepts_structure_css_and_javascript_and_recovery_keeps_them() {
    let p = fixture();
    let s = p.prepare_requirements().unwrap();
    assert_eq!(s.format_version().unwrap(), 18);
    assert!(s.markdown.contains("\"scope\": \"page\""));
    assert!(s.markdown.contains(
        "change the whole page to satisfy this request; keep content and IDs unless asked"
    ));
    assert!(s.markdown.contains("Protected copy"));
    assert!(!s.is_element_fix());
    let c = candidate(&p, &s);
    c.validate(&s, &p).unwrap();
    let (recovered, events) =
        recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
    assert_eq!(recovered, c);
    assert!(
        !events
            .iter()
            .any(|e| e.kind == "sharedCss" || e.kind == "sharedJavascript")
    );
}
#[test]
fn page_job_rejects_silent_id_loss_and_remote_code() {
    let p = fixture();
    let s = p.prepare_requirements().unwrap();
    let c = candidate(&p, &s);
    for html in [
        c.pages[0]
            .content
            .html
            .replace(" data-desegnos-id=\"tail0001\"", ""),
        c.pages[0].content.html.replace("tail0001", "made0001"),
    ] {
        let mut bad = c.clone();
        bad.pages[0].content.html = html;
        assert!(bad.validate(&s, &p).is_err());
    }
    let mut bad = c.clone();
    bad.pages[0]
        .content
        .html
        .push_str("<script src=\"https://example.com/code.js\"></script>");
    assert!(bad.validate(&s, &p).is_err());
    // Recovery must reject unsafe page edits, never silently turn them into area placeholders.
    let mut bad = c;
    bad.pages[0].content.html = "<p>Lost the page</p>".into();
    assert!(recover_generation_reply(&serde_json::to_string(&bad).unwrap(), &s, &p).is_err());
}
#[test]
fn explicit_removal_permission_does_not_override_protected_content() {
    let mut p = fixture();
    p.pages[0].regions[0].prompt = "remove tail0001".into();
    let s = p.prepare_requirements().unwrap();
    let mut c = candidate(&p, &s);
    c.pages[0].content.html = c.pages[0]
        .content
        .html
        .replace("<footer data-desegnos-id=\"tail0001\">Tail</footer>", "");
    c.validate(&s, &p).unwrap();
    p = p
        .apply(
            p.revision,
            Command::SetDecision {
                topic: "Required footer".into(),
                value: "Tail".into(),
                scope: Scope::Page(p.home_page_id),
                protected: true,
            },
        )
        .unwrap();
    let s = p.prepare_requirements().unwrap();
    c.job_id = s.job_id;
    c.requirements_sha256 = s.sha256.clone();
    assert!(
        c.validate(&s, &p)
            .unwrap_err()
            .to_string()
            .contains("protected")
    );
}
#[test]
fn mixed_page_and_area_job_gives_shared_code_to_page_request() {
    let p = add(fixture(), rect(30., 100., 200., 100.), "Add a local note");
    let s = p.prepare_requirements().unwrap();
    let mut c = candidate(&p, &s);
    let area = p.pages[0].regions[1].id;
    c.pages[0].content.html.push_str(&format!(
        "<aside data-desegnos-region=\"{area}\">Local note</aside>"
    ));
    assert!(
        s.markdown
            .contains("page-scoped request owns shared CSS/JavaScript")
    );
    assert!(s.markdown.contains("\"scope\": \"area\""));
    c.validate(&s, &p).unwrap();
    let (recovered, _) =
        recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
    assert_eq!(recovered.pages[0].content, c.pages[0].content);
    // When the page request conflicts it grants no shared-code authority.
    c.regions[0].status = RegionStatus::Conflict;
    assert!(c.validate(&s, &p).is_err());
}
#[test]
fn ordinary_areas_on_other_pages_keep_area_scope() {
    let mut p = fixture();
    p = p
        .apply(
            p.revision,
            Command::AddPage {
                name: "Other".into(),
                path: "/other".into(),
            },
        )
        .unwrap();
    let other = p.pages[1].id;
    p = p
        .apply(
            p.revision,
            Command::AddRegion {
                page_id: other,
                rect: rect(20., 20., 200., 100.),
            },
        )
        .unwrap();
    let region = p.pages[1].regions[0].clone();
    p = p
        .apply(
            p.revision,
            Command::UpdateRegion {
                page_id: other,
                region_id: region.id,
                rect: region.rect,
                prompt: "Local copy".into(),
            },
        )
        .unwrap();
    p.pages[1].content = Some(p.pages[0].content.clone().unwrap());
    let s = p.prepare_requirements().unwrap();
    let mut c = candidate(&p, &s);
    c.regions.push(RegionOutcome {
        region_id: region.id,
        status: RegionStatus::Implemented,
        explanation: "Local".into(),
    });
    c.pages.push(GeneratedPage {
        page_id: other,
        content: SiteContent {
            html: "<p>Replaced unrelated content</p>".into(),
            css: String::new(),
            javascript: String::new(),
        },
    });
    assert!(c.validate(&s, &p).is_err());
    // Unrequested pages cannot be supplied by a page-scoped job either.
    c.pages[1].page_id = Uuid::new_v4();
    assert!(c.validate(&s, &p).is_err());
}

#[test]
fn explicit_page_selection_uses_full_source_and_honors_target_removal() {
    let mut p = fixture();
    p.pages[0].regions[0].whole_page = true;
    p.pages[0].regions[0].elements = vec![ElementTarget {
        id: "tail0001".into(),
        selector: "body > main:nth-of-type(1) > footer:nth-of-type(1)".into(),
        tag: "footer".into(),
        label: "Tail".into(),
        source_sha256: String::new(),
    }];
    p.pages[0].regions[0].prompt = "Remove the selected footer and make the page grayscale".into();
    let s = p.prepare_requirements().unwrap();
    assert!(!s.is_element_fix());
    assert!(s.markdown.contains("Protected copy"));
    let mut c = candidate(&p, &s);
    c.pages[0].content.html = c.pages[0]
        .content
        .html
        .replace("<footer data-desegnos-id=\"tail0001\">Tail</footer>", "");
    assert!(c.validate(&s, &p).is_err());
    p.pages[0].regions[0].allow_target_removal = true;
    let s = p.prepare_requirements().unwrap();
    c.job_id = s.job_id;
    c.requirements_sha256 = s.sha256.clone();
    c.validate(&s, &p).unwrap();
}

#[test]
fn page_request_can_clear_shared_code_in_recovery() {
    let mut p = fixture();
    p.pages[0].content.as_mut().unwrap().javascript = "document.body.dataset.ready = 'old';".into();
    let s = p.prepare_requirements().unwrap();
    let mut c = candidate(&p, &s);
    c.pages[0].content.css.clear();
    c.pages[0].content.javascript.clear();
    let (recovered, _) =
        recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
    assert_eq!(recovered.pages[0].content, c.pages[0].content);
}

#[test]
fn negated_removal_prompt_does_not_authorize_id_loss() {
    let mut p = fixture();
    p.pages[0].regions[0].prompt = "Do not remove tail0001; make it grayscale".into();
    let s = p.prepare_requirements().unwrap();
    let mut c = candidate(&p, &s);
    c.pages[0].content.html = c.pages[0]
        .content
        .html
        .replace("<footer data-desegnos-id=\"tail0001\">Tail</footer>", "");
    assert!(c.validate(&s, &p).is_err());
}

#[test]
fn protected_selected_content_survives_whole_page_structure_authority() {
    let mut p = fixture();
    p.pages[0].regions[0].whole_page = true;
    p.pages[0].regions[0].elements = vec![ElementTarget {
        id: "tail0001".into(),
        selector: "body > main:nth-of-type(1) > footer:nth-of-type(1)".into(),
        tag: "footer".into(),
        label: "Tail".into(),
        source_sha256: String::new(),
    }];
    p = p
        .apply(
            p.revision,
            Command::SetDecision {
                topic: "Required footer".into(),
                value: "Preserve its content".into(),
                scope: Scope::Region(p.pages[0].regions[0].id),
                protected: true,
            },
        )
        .unwrap();
    let s = p.prepare_requirements().unwrap();
    let mut c = candidate(&p, &s);
    c.validate(&s, &p).unwrap();
    c.pages[0].content.html = c.pages[0]
        .content
        .html
        .replace(">Tail</footer>", ">Changed</footer>");
    assert!(
        c.validate(&s, &p)
            .unwrap_err()
            .to_string()
            .contains("protected")
    );
}
