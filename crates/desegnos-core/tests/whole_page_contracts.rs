use desegnos_core::*;
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
    p.generation_mode = GenerationMode::Strict;
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

        documents: vec![],
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
}
#[test]
fn page_job_accepts_structure_css_and_javascript_and_recovery_keeps_them() {
    let p = fixture();
    let s = p.prepare_requirements().unwrap();
    assert_eq!(s.format_version().unwrap(), 26);
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

#[test]
fn covering_base_follows_request_layer_order_and_array_order_ties() {
    let mut p = add(fixture(), rect(30., 40., 200., 100.), "Local overlay");
    let base = p.pages[0].regions[0].id;
    let overlay = p.pages[0].regions[1].id;
    assert!(p.pages[0].regions[0].is_whole_page(&p.pages[0]));
    // Actual layer commands must change scope, not merely the order of prompt text.
    p = p
        .apply(
            p.revision,
            Command::ArrangeRegion {
                page_id: p.home_page_id,
                region_id: base,
                direction: LayerDirection::Front,
            },
        )
        .unwrap();
    assert!(!p.pages[0].regions[0].is_whole_page(&p.pages[0]));
    assert_eq!(p.pages[0].request_layers()[0].id, overlay);
    p = p
        .apply(
            p.revision,
            Command::ArrangeRegion {
                page_id: p.home_page_id,
                region_id: base,
                direction: LayerDirection::Back,
            },
        )
        .unwrap();
    assert!(p.pages[0].regions[0].is_whole_page(&p.pages[0]));
    p.pages[0].regions[1].z_index = p.pages[0].regions[0].z_index;
    assert_eq!(p.pages[0].request_layers()[0].id, base);
    p.pages[0].regions.swap(0, 1);
    assert_eq!(p.pages[0].request_layers()[0].id, overlay);
    assert!(!p.pages[0].regions[1].is_whole_page(&p.pages[0]));
    // Explicit permission still overrides layer placement.
    p.pages[0].regions[1].whole_page = true;
    assert!(p.pages[0].regions[1].is_whole_page(&p.pages[0]));
}

#[test]
fn overlapping_requests_are_presented_bottom_to_top_with_local_higher_layer_precedence() {
    let mut p = add(fixture(), rect(10., 20., 200., 100.), "Make this box red");
    p = add(p, rect(100., 20., 200., 100.), "Make this box blue");
    p.pages[0].regions[0].z_index = -4;
    p.pages[0].regions[1].z_index = 8;
    p.pages[0].regions[2].z_index = 2;
    let bottom = p.pages[0].regions[0].id;
    let top = p.pages[0].regions[1].id;
    let middle = p.pages[0].regions[2].id;
    let s = p.prepare_requirements().unwrap();
    let heading = |id| s.markdown.find(&format!("### Area {id}\n")).unwrap();
    assert!(heading(bottom) < heading(middle) && heading(middle) < heading(top));
    assert!(
        s.markdown
            .contains("where boxes overlap, the higher layer wins for its own box only")
    );
    assert!(s.markdown.contains("\"layerOrder\": 0"));
    assert!(s.markdown.contains("\"layerOrder\": 2"));
    s.validate_current(&p).unwrap();
}

#[test]
fn below_fold_layout_intent_needs_full_width_and_respects_opt_out() {
    let mut p = add(
        fixture(),
        rect(0., 964., 1440., 140.),
        "too long tail, I want one page one screen only",
    );
    assert!(p.pages[0].regions[1].is_whole_page(&p.pages[0]));
    p.pages[0].regions[1].rect.width = 300.;
    assert!(!p.pages[0].regions[1].is_whole_page(&p.pages[0]));
    p.pages[0].regions[1].rect.width = 1440.;
    p.pages[0].regions[1].prompt = "Add a note below the page".into();
    assert!(!p.pages[0].regions[1].is_whole_page(&p.pages[0]));
    p.pages[0].regions[1].prompt = "too long tail, one page, one screen only".into();
    assert!(p.pages[0].regions[1].is_whole_page(&p.pages[0]));
    p.pages[0].regions[1].whole_page_disabled = true;
    assert!(!p.pages[0].regions[1].is_whole_page(&p.pages[0]));
}

fn real_layered_case() -> Project {
    let mut p = fixture();
    p.pages[0].regions[0].prompt = "make it greyscale, not green".into();
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGOwaQoAIgYIBQAdRgQ5DejjuQAAAABJRU5ErkJggg==";
    p.pages[0].content.as_mut().unwrap().html = p.pages[0].content.as_ref().unwrap().html.replace(
        "</main>",
        &format!("<img data-desegnos-id=\"image001\" src=\"{png}\" alt=\"Product\"></main>"),
    );
    p.store_inline_images().unwrap();
    p = add(
        p,
        rect(0., 964., 1440., 140.),
        "too long tail, I want one page one screen only",
    );
    p = p
        .apply(
            p.revision,
            Command::AddElementRegion {
                page_id: p.home_page_id,
                rect: rect(60., 100., 200., 200.),
                element: ElementTarget {
                    id: "image001".into(),
                    selector: "body > main:nth-of-type(1) > img:nth-of-type(1)".into(),
                    tag: "img".into(),
                    label: "Product".into(),
                    source_sha256: String::new(),
                },
            },
        )
        .unwrap();
    p.pages[0].regions[2].prompt = "replace with transparent background version".into();
    p
}

#[test]
fn real_case_combines_base_layout_and_image_edit_without_frozen_stylesheets() {
    let p = real_layered_case();
    assert!(p.pages[0].regions[0].is_whole_page(&p.pages[0]));
    assert!(p.pages[0].regions[1].is_whole_page(&p.pages[0]));
    assert!(!p.pages[0].regions[2].is_whole_page(&p.pages[0]));
    let s = p
        .prepare_requirements_with_capabilities(GenerationCapabilities {
            image_generation: Some(ImageGenerationCapability::default()),
            ..Default::default()
        })
        .unwrap();
    assert!(!s.is_element_fix());
    assert!(s.markdown.contains(
        "Element-fix rules and partial-page/conflict rules constrain only their own local regions"
    ));
    assert!(!s.markdown.contains("Image pixels cannot be edited"));
    assert!(s.markdown.contains("native image editing is available"));
    let mut c = candidate(&p, &s);
    let source = p.pages[0]
        .content
        .as_ref()
        .unwrap()
        .html
        .split("src=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(source.starts_with(PROJECT_ASSET_SCHEME));
    c.pages[0].content.html = c.pages[0].content.html.replace(&format!("src=\"{source}\""), &format!("src=\"#\" data-desegnos-generate=\"Replace the product background with transparency\" data-desegnos-source=\"{source}\" data-desegnos-background=\"transparent\""));
    c.pages[0].content.css = "article { color: gray; height: 100vh; display: grid; grid-template-rows: 1fr auto; } img { max-height: 40vh; }".into();
    c.validate(&s, &p).unwrap();
    let image_requests = c.image_generation_requests().unwrap();
    assert_eq!(image_requests.len(), 1);
    assert!(image_requests[0].transparent);
    assert!(image_requests[0].reference_image(&s, &p).unwrap().is_some());
    let (recovered, events) =
        recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
    assert_eq!(recovered.pages[0].content, c.pages[0].content);
    assert!(events.is_empty());
    assert!(
        recovered
            .regions
            .iter()
            .all(|r| r.status == RegionStatus::Implemented)
    );
}

#[test]
fn conflicted_image_freezes_only_its_element_not_page_styles_or_structure() {
    let p = real_layered_case();
    let s = p.prepare_requirements().unwrap();
    let mut c = candidate(&p, &s);
    c.regions[2].status = RegionStatus::Conflict;
    c.regions[2].explanation = "No transparent replacement is available".into();
    // The image is unchanged, while its ancestor's tag, global CSS and JavaScript change.
    c.validate(&s, &p).unwrap();
    let (recovered, _) =
        recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
    assert_eq!(recovered.pages[0].content, c.pages[0].content);
    assert_eq!(recovered.regions[0].status, RegionStatus::Implemented);
    assert_eq!(recovered.regions[1].status, RegionStatus::Implemented);
    assert_eq!(recovered.regions[2].status, RegionStatus::Conflict);
    c.pages[0].content.html = c.pages[0]
        .content
        .html
        .replace("alt=\"Product\"", "alt=\"Changed\"");
    assert!(c.validate(&s, &p).is_err());
}

#[test]
fn changing_an_unsubmitted_lower_layer_invalidates_frozen_scope() {
    let mut p = fixture();
    // An empty request is visible in Layers but absent from this generation's targets.
    let mut empty = p.pages[0].regions[0].clone();
    empty.id = Uuid::new_v4();
    empty.prompt.clear();
    empty.z_index = 2;
    p.pages[0].regions.push(empty);
    let s = p.prepare_requirements().unwrap();
    s.validate_current(&p).unwrap();
    // Adding/moving an unrelated layer above the base must not discard ready work.
    p.pages[0].regions[1].z_index = 3;
    s.validate_current(&p).unwrap();
    p.pages[0].regions[1].z_index = -1;
    assert!(!p.pages[0].regions[0].is_whole_page(&p.pages[0]));
    assert!(s.validate_current(&p).is_err());
}

#[test]
fn unmapped_local_conflict_does_not_freeze_page_layout() {
    let p = add(
        fixture(),
        rect(40., 100., 200., 100.),
        "Needs an unavailable service",
    );
    let s = p.prepare_requirements().unwrap();
    let mut c = candidate(&p, &s);
    c.regions[1].status = RegionStatus::Conflict;
    c.pages[0].content.html.push_str(&format!("<section data-desegnos-region=\"{}\" data-desegnos-placeholder=\"conflict\">Needs service access</section>", p.pages[0].regions[1].id));
    c.validate(&s, &p).unwrap();
    let (recovered, _) =
        recover_generation_reply(&serde_json::to_string(&c).unwrap(), &s, &p).unwrap();
    assert_eq!(recovered.pages[0].content, c.pages[0].content);
}
