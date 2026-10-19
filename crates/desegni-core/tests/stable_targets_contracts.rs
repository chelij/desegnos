use desegni_core::*;
use std::collections::BTreeMap;
use uuid::Uuid;
fn project() -> Project {
    let p = Project::new(
        "Stable selections".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    p.edit_page_source(0,p.home_page_id,&PageSourceEdit{html:Some("<main><article><h2>First</h2></article><article><h2>Second</h2></article><p>Unrelated</p></main>".into()),..Default::default()}).unwrap()
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
        tag: range
            .selector
            .rsplit(" > ")
            .next()
            .unwrap()
            .split(':')
            .next()
            .unwrap()
            .into(),
        label: "Selected".into(),
        selector: range.selector.clone(),
        source_sha256: String::new(),
    }
}
fn bounds(target: &ElementTarget, x: f64) -> TargetBounds {
    TargetBounds {
        target: target.clone(),
        rect: Rect {
            x,
            y: 20.,
            width: 100.,
            height: 80.,
        },
    }
}
fn describe(p: Project, targets: &[ElementTarget]) -> Project {
    let home = p.home_page_id;
    let mut p = p
        .apply(
            p.revision,
            Command::AddElementRegions {
                page_id: home,
                elements: targets.to_vec(),
                bounds: targets
                    .iter()
                    .enumerate()
                    .map(|(i, t)| bounds(t, i as f64 * 150.))
                    .collect(),
                allow_target_removal: false,
            },
        )
        .unwrap();
    let r = p.pages[0].regions.last().unwrap().clone();
    p = p
        .apply(
            p.revision,
            Command::UpdateRegion {
                page_id: home,
                region_id: r.id,
                rect: r.rect,
                prompt: "Make these match".into(),
            },
        )
        .unwrap();
    p
}
fn candidate(p: &Project, snapshot: &RequirementsSnapshot, html: String) -> GenerationCandidate {
    GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Updated selected items".into(),
        regions: p.pages[0]
            .regions
            .iter()
            .filter(|r| !r.completed && !r.prompt.is_empty())
            .map(|r| RegionOutcome {
                region_id: r.id,
                status: RegionStatus::Implemented,
                explanation: "Matched the selected items".into(),
            })
            .collect(),
        pages: vec![GeneratedPage {
            page_id: p.home_page_id,
            content: SiteContent {
                html,
                css: p.pages[0]
                    .content
                    .as_ref()
                    .map(|source| source.css.clone())
                    .unwrap_or_default(),
                javascript: p.pages[0]
                    .content
                    .as_ref()
                    .map(|source| source.javascript.clone())
                    .unwrap_or_default(),
            },
        }],
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],
    }
}
#[test]
fn ids_are_unique_and_survive_text_moves_and_other_requests_applying() {
    let p = project();
    let first = target(&p, "article:nth-of-type(1)");
    let second = target(&p, "article:nth-of-type(2)");
    assert!(!first.id.is_empty());
    assert_ne!(first.id, second.id);
    let p = describe(p, std::slice::from_ref(&first));
    let (p, _) = p
        .edit_element(
            p.revision,
            p.home_page_id,
            &first,
            &ElementEdit::Update {
                texts: vec![TextEdit {
                    index: 0,
                    text: "Revised".into(),
                }],
                attributes: BTreeMap::new(),
                style: None,
            },
        )
        .unwrap();
    p.inspect_element(p.home_page_id, &first).unwrap();
    let (mut p, _) = p
        .edit_element(
            p.revision,
            p.home_page_id,
            &first,
            &ElementEdit::Reorder {
                direction: ElementOrder::After,
            },
        )
        .unwrap();
    assert_eq!(p.pages[0].regions[0].elements[0].id, first.id);
    p.prepare_requirements().unwrap();
    p.pages[0].regions[0].completed = true;
    let mut p = describe(p, std::slice::from_ref(&second));
    p.pages[0].regions[0].completed = false;
    let pending = p.pages[0].regions[0].clone();
    // Simulate the other request applying using its own frozen target scope.
    p.pages[0].regions[0].completed = true;
    let snapshot = p.prepare_requirements().unwrap();
    let html = p.pages[0]
        .content
        .as_ref()
        .unwrap()
        .html
        .replace("Second", "Updated second");
    let c = candidate(&p, &snapshot, html);
    p.pages[0].regions[0].completed = false;
    let applied = p.apply_generation(&snapshot, &c, &[]).unwrap();
    assert_eq!(
        applied.pages[0].regions[0].elements[0].id,
        pending.elements[0].id
    );
    applied.prepare_requirements().unwrap();
}
#[test]
fn multi_target_scope_accepts_only_selected_subtrees_and_preserves_shared_sources() {
    let p = project();
    let a = target(&p, "article:nth-of-type(1)");
    let b = target(&p, "article:nth-of-type(2)");
    let p = describe(p, &[a.clone(), b]);
    let snapshot = p.prepare_requirements().unwrap();
    assert_eq!(snapshot.format_version().unwrap(), 10);
    assert!(snapshot.markdown.contains("\"elements\""));
    let original = &p.pages[0].content.as_ref().unwrap().html;
    let c = candidate(
        &p,
        &snapshot,
        original
            .replace("First", "Matched first")
            .replace("Second", "Matched second"),
    );
    c.validate(&snapshot, &p).unwrap();
    let mut invalid = c.clone();
    invalid.pages[0].content.html = invalid.pages[0]
        .content
        .html
        .replace("Unrelated", "Changed sibling");
    assert!(
        invalid
            .validate(&snapshot, &p)
            .unwrap_err()
            .to_string()
            .contains("outside")
    );
    let mut invalid = c.clone();
    invalid.pages[0].content.css = "p{display:none}".into();
    assert!(invalid.validate(&snapshot, &p).is_err());
    let mut invalid = c.clone();
    invalid.pages[0].content.html = invalid.pages[0]
        .content
        .html
        .replace(&format!(" {}=\"{}\"", ELEMENT_ID_ATTRIBUTE, a.id), "");
    assert!(
        invalid
            .validate(&snapshot, &p)
            .unwrap_err()
            .to_string()
            .contains("removed a selected")
    );
}
#[test]
fn nested_targets_prune_and_bounds_union_is_validated() {
    let p = project();
    let outer = target(&p, "article:nth-of-type(1)");
    let inner = target(&p, "article:nth-of-type(1) > h2:nth-of-type(1)");
    let other = target(&p, "article:nth-of-type(2)");
    let next = p
        .apply(
            p.revision,
            Command::AddElementRegions {
                page_id: p.home_page_id,
                elements: vec![inner.clone(), outer.clone(), other.clone()],
                bounds: vec![
                    bounds(&inner, 10.),
                    bounds(&outer, 0.),
                    bounds(&other, 150.),
                ],
                allow_target_removal: false,
            },
        )
        .unwrap();
    let r = &next.pages[0].regions[0];
    assert_eq!(r.elements.len(), 2);
    assert_eq!(
        r.rect,
        Rect {
            x: 0.,
            y: 20.,
            width: 250.,
            height: 80.
        }
    );
    assert!(
        p.canonical_targets(p.home_page_id, &vec![outer; 9])
            .is_err()
    );
    assert!(
        p.target_bounds_union(
            p.home_page_id,
            std::slice::from_ref(&other),
            &[bounds(&inner, 0.)]
        )
        .is_err()
    );
}
#[test]
fn removed_targets_stay_stale_and_never_fall_back_to_a_new_sibling() {
    let p = project();
    let t = target(&p, "article:nth-of-type(1)");
    let p = describe(p, std::slice::from_ref(&t));
    let (p, _) = p
        .edit_element(p.revision, p.home_page_id, &t, &ElementEdit::Remove)
        .unwrap();
    assert!(
        p.inspect_element(p.home_page_id, &t)
            .unwrap_err()
            .to_string()
            .contains("removed")
    );
    assert!(
        p.prepare_requirements()
            .unwrap_err()
            .to_string()
            .contains("removed")
    );
    assert!(!p.pages[0].regions[0].completed);
    assert_eq!(p.pages[0].regions[0].elements[0].id, t.id);
}
#[test]
fn migration_assigns_and_remaps_without_modifying_original_bytes_or_revision() {
    let mut p = Project::new(
        "Old website".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap();
    p.pages[0].content = Some(SiteContent {
        html: "<main><p>One</p><p>Two</p></main>".into(),
        css: String::new(),
        javascript: String::new(),
    });
    let legacy = ElementTarget {
        id: String::new(),
        selector: "body > main:nth-of-type(1) > p:nth-of-type(2)".into(),
        tag: "p".into(),
        label: "Two".into(),
        source_sha256: p.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    };
    p.pages[0].regions.push(Region {
        id: Uuid::new_v4(),
        rect: Rect {
            x: 0.,
            y: 0.,
            width: 100.,
            height: 50.,
        },
        z_index: 0,
        prompt: "Keep this".into(),
        element: Some(legacy),
        elements: vec![],
        allow_target_removal: false,
        completed: false,
    });
    p.schema_version = 6;
    let bytes = serde_json::to_vec(&p).unwrap();
    let preserved = bytes.clone();
    let decoded = decode_project(&bytes).unwrap();
    assert!(decoded.migrated());
    assert_eq!(decoded.project.revision, p.revision);
    assert_eq!(bytes, preserved);
    let r = &decoded.project.pages[0].regions[0];
    assert!(r.element.is_none());
    assert_eq!(r.elements.len(), 1);
    assert!(!r.elements[0].id.is_empty());
    decoded.project.prepare_requirements().unwrap();
    assert!(
        !serde_json::to_string(&r.elements[0])
            .unwrap()
            .contains("sourceSha256")
    );
    let reopened = decode_project(&serde_json::to_vec(&decoded.project).unwrap()).unwrap();
    assert!(!reopened.migrated());
    assert_eq!(reopened.project, decoded.project);
}
#[test]
fn positional_fallback_is_source_bound_and_stable_ids_are_stripped_on_export() {
    let p = project();
    let mut t = target(&p, "article:nth-of-type(1)");
    t.id.clear();
    t.source_sha256 = p.pages[0].content.as_ref().unwrap().sha256().unwrap();
    p.inspect_element(p.home_page_id, &t).unwrap();
    t.source_sha256 = "0".repeat(64);
    assert!(p.inspect_element(p.home_page_id, &t).is_err());
    let exported = compile_website(&p).unwrap();
    assert!(!String::from_utf8_lossy(&exported.files["index.html"]).contains(ELEMENT_ID_ATTRIBUTE));
}
#[test]
fn multi_delete_is_atomic_undoable_and_replacement_keeps_identity() {
    let p = project();
    let a = target(&p, "article:nth-of-type(1)");
    let b = target(&p, "article:nth-of-type(2)");
    let mut session = Session::new(p.clone()).unwrap();
    let command = Command::DeleteElements {
        page_id: p.home_page_id,
        elements: vec![a.clone(), b],
    };
    assert!(
        session
            .execute(p.revision, command.clone(), |_| Err(Error(
                "Disk full".into()
            )))
            .is_err()
    );
    assert_eq!(session.project(), &p);
    session.execute(p.revision, command, |_| Ok(())).unwrap();
    assert_eq!(
        session.project().pages[0]
            .content
            .as_ref()
            .unwrap()
            .html
            .matches("<article")
            .count(),
        0
    );
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, p.pages);
    let (p, t) = p
        .edit_element(
            p.revision,
            p.home_page_id,
            &a,
            &ElementEdit::Replace {
                html: "<section>Replacement</section>".into(),
                keep_style: false,
            },
        )
        .unwrap();
    assert_eq!(t.unwrap().id, a.id);
    assert_eq!(
        p.inspect_element(p.home_page_id, &a).unwrap().target.tag,
        "section"
    );
}
#[test]
fn removal_requires_explicit_permission_and_duplicate_or_forged_ids_are_rejected() {
    let p = project();
    let a = target(&p, "article:nth-of-type(1)");
    let mut p = describe(p, std::slice::from_ref(&a));
    let original = p.pages[0].content.as_ref().unwrap().html.clone();
    let doc = dom_query::Document::from(original.as_str());
    doc.select(&format!("[{}=\"{}\"]", ELEMENT_ID_ATTRIBUTE, a.id))
        .remove();
    let removed = doc.html().to_string();
    let snapshot = p.prepare_requirements().unwrap();
    assert!(
        candidate(&p, &snapshot, removed.clone())
            .validate(&snapshot, &p)
            .is_err()
    );
    p = p
        .apply(
            p.revision,
            Command::SetRegionTargetRemoval {
                page_id: p.home_page_id,
                region_id: p.pages[0].regions[0].id,
                allow_target_removal: true,
            },
        )
        .unwrap();
    let snapshot = p.prepare_requirements().unwrap();
    candidate(&p, &snapshot, removed)
        .validate(&snapshot, &p)
        .unwrap();
    let forged = original.replace(
        "First",
        &format!("<span data-desegnos-id='{}'>Forged</span>", a.id),
    );
    assert!(
        candidate(&p, &snapshot, forged)
            .validate(&snapshot, &p)
            .is_err()
    );
}

#[test]
fn paste_assigns_fresh_editor_ids_without_rebinding_existing_requests() {
    let p = project();
    let root = target(&p, "article:nth-of-type(1)");
    let child = target(&p, "article:nth-of-type(1) > h2:nth-of-type(1)");
    let p = describe(p, std::slice::from_ref(&root));
    let styles = [root.clone(), child]
        .into_iter()
        .map(|target| ElementStyleEdit {
            target,
            style: "display:block; color:rgb(0,0,0)".into(),
            pseudo_styles: vec![],
        })
        .collect::<Vec<_>>();
    let clipboard = p
        .copy_layer(
            p.home_page_id,
            &LayerTarget::Element {
                target: root.clone(),
            },
            &styles,
        )
        .unwrap();
    let (p, selection) = p
        .paste_layer(p.revision, p.home_page_id, &clipboard, Some(&root))
        .unwrap();
    let pasted = selection.element.unwrap();
    assert_ne!(pasted.id, root.id);
    assert_eq!(p.pages[0].regions[0].elements[0].id, root.id);
    p.inspect_element(p.home_page_id, &root).unwrap();
    p.inspect_element(p.home_page_id, &pasted).unwrap();
    let doc = dom_query::Document::from(p.pages[0].content.as_ref().unwrap().html.as_str());
    let ids = doc.select(&format!("[{ELEMENT_ID_ATTRIBUTE}]"));
    let mut seen = std::collections::BTreeSet::new();
    for node in ids.nodes() {
        assert!(seen.insert(node.attr(ELEMENT_ID_ATTRIBUTE).unwrap().to_string()));
    }
}

#[test]
fn native_multi_target_commands_keep_the_existing_command_discriminator_and_casing() {
    let p = project();
    let target = target(&p, "article:nth-of-type(1)");
    for (kind, extra) in [
        (
            "add_element_regions",
            serde_json::json!({"elements":[target.clone()],"bounds":[bounds(&target,0.)]}),
        ),
        (
            "retarget_region",
            serde_json::json!({"region_id":Uuid::new_v4(),"elements":[target.clone()],"bounds":[bounds(&target,0.)]}),
        ),
        (
            "set_region_target_removal",
            serde_json::json!({"region_id":Uuid::new_v4(),"allow_target_removal":true}),
        ),
        (
            "delete_elements",
            serde_json::json!({"elements":[target.clone()]}),
        ),
    ] {
        let mut value = extra;
        value["type"] = serde_json::json!(kind);
        value["page_id"] = serde_json::json!(p.home_page_id);
        let command: Command = serde_json::from_value(value).unwrap();
        let serialized = serde_json::to_value(command).unwrap();
        assert_eq!(serialized["type"], kind);
        assert!(serialized.get("kind").is_none());
    }
}

#[test]
fn targeted_generation_rejects_embedded_global_style_script_and_head_bypasses() {
    let p = project();
    let selected = target(&p, "article:nth-of-type(1)");
    let p = describe(p, std::slice::from_ref(&selected));
    let snapshot = p.prepare_requirements().unwrap();
    let html = p.pages[0].content.as_ref().unwrap().html.clone();
    for injection in [
        "<style>p{display:none}</style>",
        "<script>document.querySelector('p').remove()</script>",
        "<link rel='stylesheet' href='https://example.com/global.css'>",
        "<button onclick=\"document.querySelector('p').remove()\">Do it</button>",
    ] {
        let altered = html.replacen("First", &format!("First{injection}"), 1);
        assert!(
            candidate(&p, &snapshot, altered)
                .validate(&snapshot, &p)
                .is_err(),
            "{injection}"
        );
    }
    let altered = format!(
        "<html style='color:red'><head><style>p{{display:none}}</style></head><body>{html}</body></html>"
    );
    assert!(
        candidate(&p, &snapshot, altered)
            .validate(&snapshot, &p)
            .is_err()
    );
    let altered = format!("<html style='color:red'><body>{html}</body></html>");
    assert!(
        candidate(&p, &snapshot, altered)
            .validate(&snapshot, &p)
            .is_err()
    );
    let safe = html.replacen("<h2 ", "<h2 style='color:red' ", 1);
    candidate(&p, &snapshot, safe)
        .validate(&snapshot, &p)
        .unwrap();
}

#[test]
fn empty_generated_html_is_rejected_in_legacy_and_current_packets() {
    for fixture in [
        include_str!("fixtures/requirements8.json"),
        include_str!("fixtures/requirements9.json"),
    ] {
        let fixture: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let mut p: Project = serde_json::from_value(fixture["project"].clone()).unwrap();
        p.schema_version = SCHEMA_VERSION;
        let old: RequirementsSnapshot =
            serde_json::from_value(fixture["requirements"].clone()).unwrap();
        for snapshot in [old, p.prepare_requirements().unwrap()] {
            for html in ["", "  \n\t"] {
                let c = candidate(&p, &snapshot, html.into());
                assert!(
                    c.validate(&snapshot, &p)
                        .unwrap_err()
                        .to_string()
                        .contains("Generated page HTML")
                );
                assert!(
                    GenerationCandidate::from_json(&serde_json::to_string(&c).unwrap()).is_err()
                );
            }
        }
    }
}

#[test]
fn migration_preserves_frozen_receipts_but_never_rebinds_them_to_changed_source() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/requirements9.json")).unwrap();
    let mut original: Project = serde_json::from_value(golden["project"].clone()).unwrap();
    original.schema_version = SCHEMA_VERSION;
    original.pages[0].content = Some(SiteContent {
        html: "<h1>Before upgrade</h1>".into(),
        css: String::new(),
        javascript: String::new(),
    });
    let snapshot = original.prepare_requirements().unwrap();
    let receipt = serde_json::to_vec(&snapshot).unwrap();
    let id = original.pages[0].regions[0].id;
    let c = candidate(
        &original,
        &snapshot,
        format!("<h1 data-desegnos-region='{id}'>Generated earlier</h1>"),
    );
    c.validate(&snapshot, &original).unwrap();
    original.schema_version = 6;
    let decoded = decode_project(&serde_json::to_vec(&original).unwrap()).unwrap();
    let preserved = decoded.project.clone();
    assert!(snapshot.validate_current(&preserved).is_err());
    assert!(preserved.apply_generation(&snapshot, &c, &[]).is_err());
    assert_eq!(decoded.project, preserved);
    assert_eq!(serde_json::to_vec(&snapshot).unwrap(), receipt);
    let new = preserved.prepare_requirements().unwrap();
    assert_ne!(new.sha256, snapshot.sha256);
    assert_eq!(new.format_version().unwrap(), 10);
    assert_eq!(
        preserved.pages[0].regions[0].prompt,
        original.pages[0].regions[0].prompt
    );
}
