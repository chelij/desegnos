use desegni_core::*;
use serde_json::json;
use uuid::Uuid;

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn project(shape: RegionShape) -> Project {
    let p = Project::new(
        "Area shapes".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    let mut p = p
        .apply(
            p.revision,
            Command::AddShapedRegion {
                page_id: p.home_page_id,
                rect: rect(0., 0., 100., 100.),
                shape,
            },
        )
        .unwrap();
    p.pages[0].regions[0].prompt = "Change only the marked area".into();
    p
}

fn target(p: &Project, suffix: &str) -> ElementTarget {
    let range = p
        .page_source_map(p.home_page_id)
        .unwrap()
        .ranges
        .into_iter()
        .find(|r| r.selector.ends_with(suffix))
        .unwrap();
    ElementTarget {
        id: range.id,
        tag: suffix.split(':').next().unwrap().into(),
        label: suffix.into(),
        selector: range.selector,
        source_sha256: String::new(),
    }
}

fn measured(shape: RegionShape) -> Project {
    let p = project(shape);
    let p = p
        .edit_page_source(
            p.revision,
            p.home_page_id,
            &PageSourceEdit {
                html: Some("<main><p>Center</p><p>Corner</p></main>".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let bounds = vec![
        TargetBounds {
            target: target(&p, "main:nth-of-type(1)"),
            rect: rect(0., 0., 100., 100.),
        },
        TargetBounds {
            target: target(&p, "p:nth-of-type(1)"),
            rect: rect(45., 45., 10., 10.),
        },
        TargetBounds {
            target: target(&p, "p:nth-of-type(2)"),
            rect: rect(0., 0., 5., 5.),
        },
    ];
    p.refresh_element_target_bounds(
        p.home_page_id,
        p.viewport,
        &[ElementTargetBounds {
            region_id: p.pages[0].regions[0].id,
            bounds,
        }],
    )
    .unwrap()
}

fn candidate(p: &Project, snapshot: &RequirementsSnapshot, html: String) -> GenerationCandidate {
    GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Changed the area".into(),
        regions: vec![RegionOutcome {
            region_id: p.pages[0].regions[0].id,
            status: RegionStatus::Implemented,
            explanation: "Updated in-area content".into(),
        }],
        pages: vec![GeneratedPage {
            page_id: p.home_page_id,
            content: SiteContent {
                html,
                css: p.pages[0].content.as_ref().unwrap().css.clone(),
                javascript: p.pages[0].content.as_ref().unwrap().javascript.clone(),
            },
        }],
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],
    }
}

#[test]
fn missing_or_explicit_rect_shape_roundtrips_to_exact_legacy_json() {
    let legacy = format!(
        "{{\"id\":\"{}\",\"rect\":{{\"x\":0.0,\"y\":0.0,\"width\":100.0,\"height\":100.0}},\"zIndex\":0,\"prompt\":\"Keep\"}}",
        Uuid::new_v4()
    );
    let region: Region = serde_json::from_str(&legacy).unwrap();
    assert_eq!(region.shape, RegionShape::Rect);
    assert!(region.area_bounds.is_empty());
    assert_eq!(serde_json::to_string(&region).unwrap(), legacy);
    let mut explicit = serde_json::from_str::<serde_json::Value>(&legacy).unwrap();
    explicit["shape"] = json!({"kind":"rect"});
    assert_eq!(serde_json::from_value::<Region>(explicit).unwrap(), region);
}

#[test]
fn shapes_roundtrip_in_schema_eight_without_a_migration() {
    for shape in [
        RegionShape::Ellipse,
        RegionShape::Freehand {
            points: vec![[0., 0.], [1., 0.], [0.5, 1.]],
        },
    ] {
        let p = project(shape.clone());
        let decoded = decode_project(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert!(!decoded.migrated());
        assert_eq!(decoded.project, p);
        assert_eq!(decoded.project.pages[0].regions[0].shape, shape);
    }
}

#[test]
fn freehand_validates_counts_finite_coordinates_and_fraction_bounds() {
    for count in [3, 200, 500] {
        RegionShape::Freehand {
            points: vec![[0., 1.]; count],
        }
        .validate()
        .unwrap();
    }
    for count in [0, 1, 2, 501] {
        assert!(
            RegionShape::Freehand {
                points: vec![[0., 1.]; count]
            }
            .validate()
            .is_err()
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.001, 1.001] {
        for axis in [0, 1] {
            let mut points = vec![[0., 0.], [1., 0.], [0.5, 1.]];
            points[1][axis] = value;
            assert!(RegionShape::Freehand { points }.validate().is_err());
        }
    }
    for value in [
        json!({"kind":"triangle"}),
        json!({"kind":"freehand","points":[[0,0,1],[1,0],[0,1]]}),
    ] {
        assert!(serde_json::from_value::<RegionShape>(value).is_err());
    }
    let mut p = project(RegionShape::Ellipse);
    p.pages[0].regions[0].shape = RegionShape::Freehand { points: vec![] };
    assert!(p.validate().is_err());
}

#[test]
fn ellipse_scope_excludes_box_corners_and_includes_crossings_and_enclosure() {
    let p = project(RegionShape::Ellipse);
    let r = &p.pages[0].regions[0];
    for box_ in [
        rect(45., 45., 10., 10.),
        rect(-10., 42., 120., 0.1),
        rect(-100., -100., 300., 300.),
        rect(100., 49., 5., 2.),
    ] {
        assert!(r.overlaps_rect(box_), "{box_:?}");
    }
    for box_ in [
        rect(0., 0., 5., 5.),
        rect(95., 95., 5., 5.),
        rect(101., 45., 5., 5.),
    ] {
        assert!(!r.overlaps_rect(box_), "{box_:?}");
    }
}

#[test]
fn concave_freehand_scope_uses_polygon_edges_and_the_closing_edge() {
    let p = project(RegionShape::Freehand {
        points: vec![
            [0., 0.],
            [1., 0.],
            [1., 0.3],
            [0.3, 0.3],
            [0.3, 1.],
            [0., 1.],
        ],
    });
    let r = &p.pages[0].regions[0];
    assert!(!r.overlaps_rect(rect(70., 70., 10., 10.)));
    assert!(r.overlaps_rect(rect(10., 70., 10., 10.)));
    assert!(r.overlaps_rect(rect(-2., 50., 3., 1.)));
    assert!(r.overlaps_rect(rect(-2., 10., 104., 1.)));
}

#[test]
fn moving_and_resizing_only_changes_bounds_not_relative_points() {
    let p = project(RegionShape::Freehand {
        points: vec![[0., 0.], [1., 0.], [0.5, 1.]],
    });
    let r = &p.pages[0].regions[0];
    let moved = p
        .apply(
            p.revision,
            Command::UpdateRegion {
                page_id: p.home_page_id,
                region_id: r.id,
                rect: rect(-20., 30., 200., 50.),
                prompt: r.prompt.clone(),
            },
        )
        .unwrap();
    let after = &moved.pages[0].regions[0];
    assert_eq!(after.shape, r.shape);
    assert_eq!(
        after.shape.polygon(after.rect),
        vec![[-20., 30.], [180., 30.], [80., 80.]]
    );
}

#[test]
fn rectangle_requirements_keep_format_fourteen_and_frozen_legacy_validation() {
    let p = project(RegionShape::Rect);
    let snapshot = p.prepare_requirements().unwrap();
    assert_eq!(snapshot.format_version().unwrap(), 18);
    assert!(!snapshot.markdown.contains("elementsInArea"));
    assert!(!snapshot.markdown.contains("\"shape\""));
    let decoded = decode_project(&serde_json::to_vec(&p).unwrap())
        .unwrap()
        .project;
    snapshot.validate_current(&decoded).unwrap();
}

#[test]
fn shaped_requirements_describe_ellipse_and_rounded_page_pixel_polygon() {
    let p = measured(RegionShape::Ellipse);
    let snapshot = p.prepare_requirements().unwrap();
    assert_eq!(snapshot.format_version().unwrap(), 18);
    assert!(snapshot.markdown.contains("ellipse inscribed in the box"));
    snapshot.validate_current(&p).unwrap();
    let area: serde_json::Value = serde_json::from_str(
        snapshot
            .markdown
            .split("### Area ")
            .nth(1)
            .unwrap()
            .split("```json\n")
            .nth(1)
            .unwrap()
            .split("\n```")
            .next()
            .unwrap(),
    )
    .unwrap();
    let ids = area["elementsInArea"].as_array().unwrap();
    assert!(ids.contains(&json!(target(&p, "p:nth-of-type(1)").id)));
    assert!(!ids.contains(&json!(target(&p, "p:nth-of-type(2)").id)));
    let mut p = project(RegionShape::Freehand {
        points: vec![[0., 0.], [1., 0.], [0.5, 1.]],
    });
    p.pages[0].regions[0].rect = rect(-10.4, 20.4, 31., 19.);
    let snapshot = p.prepare_requirements().unwrap();
    let area: serde_json::Value = serde_json::from_str(
        snapshot
            .markdown
            .split("### Area ")
            .nth(1)
            .unwrap()
            .split("```json\n")
            .nth(1)
            .unwrap()
            .split("\n```")
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        area["shape"]["points"],
        json!([[-10., 20.], [21., 20.], [5., 39.]])
    );
    let mut legacy = snapshot.clone();
    legacy.markdown =
        legacy
            .markdown
            .replacen("Requirements format: 18", "Requirements format: 14", 1);
    legacy.sha256 = digest(legacy.markdown.as_bytes());
    assert!(legacy.validate_current(&p).is_err());
}

#[test]
fn measured_shaped_scope_allows_center_edits_and_protects_corner_descendants() {
    for shape in [
        RegionShape::Ellipse,
        RegionShape::Freehand {
            points: vec![[0.5, 0.], [1., 0.5], [0.5, 1.], [0., 0.5]],
        },
    ] {
        let p = measured(shape);
        let snapshot = p.prepare_requirements().unwrap();
        let source = &p.pages[0].content.as_ref().unwrap().html;
        let marker = format!(
            "<section data-desegnos-region=\"{}\"></section>",
            p.pages[0].regions[0].id
        );
        let good = candidate(
            &p,
            &snapshot,
            format!("{}{marker}", source.replace("Center", "Changed")),
        );
        good.validate(&snapshot, &p).unwrap();
        let corner_id = target(&p, "p:nth-of-type(2)").id;
        let marked_corner = source
            .replace(
                &format!("data-desegnos-id=\"{corner_id}\""),
                &format!(
                    "data-desegnos-id=\"{corner_id}\" data-desegnos-region=\"{}\"",
                    p.pages[0].regions[0].id
                ),
            )
            .replace("Corner", "Out of scope");
        assert!(
            candidate(&p, &snapshot, marked_corner)
                .validate(&snapshot, &p)
                .is_err()
        );
        let bad = candidate(
            &p,
            &snapshot,
            format!("{}{marker}", source.replace("Corner", "Outside changed")),
        );
        assert!(
            bad.validate(&snapshot, &p)
                .unwrap_err()
                .0
                .contains("outside the requested shape")
        );
        let removed = candidate(
            &p,
            &snapshot,
            format!(
                "{}{marker}",
                source.replace(
                    &format!(
                        "<p data-desegnos-id=\"{}\">Corner</p>",
                        target(&p, "p:nth-of-type(2)").id
                    ),
                    ""
                )
            ),
        );
        assert!(removed.validate(&snapshot, &p).is_err());
        let mut css = good.clone();
        css.pages[0].content.css.push_str("body{color:red}");
        assert!(css.validate(&snapshot, &p).is_err());
        let mut js = good.clone();
        js.pages[0].content.javascript = "alert('outside')".into();
        assert!(js.validate(&snapshot, &p).is_err());
        let mut scoped_css = good.clone();
        scoped_css.pages[0].content.css.push_str(&format!(
            "[data-desegnos-id=\"{}\"]{{color:red}}",
            target(&p, "p:nth-of-type(1)").id
        ));
        scoped_css.validate(&snapshot, &p).unwrap();
    }
}

#[test]
fn measuring_an_area_is_idempotent_and_rejects_forged_or_duplicate_elements() {
    let p = measured(RegionShape::Ellipse);
    let region = &p.pages[0].regions[0];
    let entry = ElementTargetBounds {
        region_id: region.id,
        bounds: region.area_bounds.clone(),
    };
    let again = p
        .refresh_element_target_bounds(p.home_page_id, p.viewport, std::slice::from_ref(&entry))
        .unwrap();
    assert_eq!(p, again);
    let mut duplicate = entry.clone();
    duplicate.bounds.push(duplicate.bounds[0].clone());
    assert!(
        p.refresh_element_target_bounds(p.home_page_id, p.viewport, &[duplicate])
            .is_err()
    );
    let mut forged = entry;
    forged.bounds[0].target.id = "nonexistent1".into();
    assert!(
        p.refresh_element_target_bounds(p.home_page_id, p.viewport, &[forged])
            .is_err()
    );
}

#[test]
fn copying_and_pasting_preserves_shape_without_copying_page_measurements() {
    let p = measured(RegionShape::Ellipse);
    let region = &p.pages[0].regions[0];
    let clipboard = p
        .copy_layer(
            p.home_page_id,
            &LayerTarget::Request {
                region_id: region.id,
            },
            &[],
        )
        .unwrap();
    let (pasted, _) = p
        .paste_layer(p.revision, p.home_page_id, &clipboard, None)
        .unwrap();
    let copy = pasted.pages[0].regions.last().unwrap();
    assert_eq!(copy.shape, region.shape);
    assert!(copy.area_bounds.is_empty());
}

#[test]
fn element_targets_keep_their_subtree_and_removal_rules_beside_a_shaped_request() {
    let p = measured(RegionShape::Ellipse);
    let corner = target(&p, "p:nth-of-type(2)");
    let mut p = p
        .apply(
            p.revision,
            Command::AddElementRegion {
                page_id: p.home_page_id,
                rect: rect(0., 0., 5., 5.),
                element: corner.clone(),
            },
        )
        .unwrap();
    p.pages[0].regions[1].prompt = "Update this selected element too".into();
    let snapshot = p.prepare_requirements().unwrap();
    let source = &p.pages[0].content.as_ref().unwrap().html;
    let marker = format!(
        "<section data-desegnos-region=\"{}\"></section>",
        p.pages[0].regions[0].id
    );
    let mut good = candidate(
        &p,
        &snapshot,
        format!(
            "{}{marker}",
            source
                .replace("Center", "New center")
                .replace("Corner", "New corner")
        ),
    );
    good.regions.push(RegionOutcome {
        region_id: p.pages[0].regions[1].id,
        status: RegionStatus::Implemented,
        explanation: "Changed the selected element".into(),
    });
    good.validate(&snapshot, &p).unwrap();
    let mut removed = good.clone();
    removed.pages[0].content.html = removed.pages[0].content.html.replace(
        &format!("<p data-desegnos-id=\"{}\">New corner</p>", corner.id),
        "",
    );
    assert!(removed.validate(&snapshot, &p).is_err());
    let center = target(&p, "p:nth-of-type(1)");
    let mut smuggled = good.clone();
    smuggled.pages[0].content.html = format!(
        "<main data-desegnos-id=\"{}\"><p data-desegnos-id=\"{}\"><span data-desegnos-id=\"{}\">New center</span>New corner</p></main>{marker}",
        target(&p, "main:nth-of-type(1)").id,
        corner.id,
        center.id
    );
    assert!(smuggled.validate(&snapshot, &p).is_err());
    assert_eq!(p.pages[0].regions[1].shape, RegionShape::Rect);
    assert!(p.pages[0].regions[1].area_bounds.is_empty());
}
