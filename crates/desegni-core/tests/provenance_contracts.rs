mod support;
use desegni_core::*;
use support::without_editor_ids;
fn project() -> Project {
    let mut project: Project = serde_json::from_value(
        serde_json::from_str::<serde_json::Value>(include_str!("fixtures/requirements8.json"))
            .unwrap()["project"]
            .clone(),
    )
    .unwrap();
    project.schema_version = SCHEMA_VERSION;
    project
}
#[test]
fn found_metadata_survives_generation_and_serialization_but_export_keeps_only_public_credits() {
    let project = project();
    let snapshot = project.prepare_requirements().unwrap();
    let region = project.pages[0].regions[0].id;
    let content = SiteContent {
        html: format!(
            "<main data-desegnos-region='{region}'><h1 data-desegnos-found='https://example.com/history'>A real place</h1><p data-desegnos-found='https://example.com/history'>Known facts.</p><a href='https://example.com/history'>Source and photo credit</a></main>"
        ),
        css: String::new(),
        javascript: String::new(),
    };
    let candidate = GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Added the requested feature.".into(),
        regions: vec![RegionOutcome {
            region_id: region,
            status: RegionStatus::Implemented,
            explanation: "Used the requested public facts.".into(),
        }],
        pages: vec![GeneratedPage {
            page_id: project.home_page_id,
            content: content.clone(),
        }],
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],
    };
    candidate.validate(&snapshot, &project).unwrap();
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    let restored: Project = serde_json::from_slice(&serde_json::to_vec(&applied).unwrap()).unwrap();
    let restored_content = restored.pages[0].content.as_ref().unwrap();
    let readable = format_site_content(&content);
    assert_eq!(without_editor_ids(&restored_content.html), readable.html);
    assert_eq!(restored_content.css, readable.css);
    assert_eq!(restored_content.javascript, content.javascript);
    let found = content.found_sources().unwrap();
    assert_eq!(found.len(), 2);
    let target = ElementTarget {
        id: String::new(),
        selector: found[0].selector.clone(),
        tag: "h1".into(),
        label: "A real place".into(),
        source_sha256: restored_content.sha256().unwrap(),
    };
    assert_eq!(
        restored
            .found_source(project.home_page_id, &target)
            .unwrap()
            .as_deref(),
        Some("https://example.com/history")
    );
    let export = compile_website(&restored).unwrap();
    let html = String::from_utf8(export.files["index.html"].clone()).unwrap();
    assert!(!html.contains(FOUND_CONTENT_ATTRIBUTE));
    assert!(html.contains("href=\"https://example.com/history\""));
    assert!(html.contains("Source and photo credit"));
}
#[test]
fn found_sources_reject_unsafe_local_and_credential_bearing_addresses() {
    for url in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "http://example.com",
        "https://u:p@example.com/",
        "https://127.1/",
        "https://192.168.1.1/",
        "https://[::ffff:127.0.0.1]/",
        "https://localhost/",
        "https://service.local/",
        "https://example.com:8443/",
        "https://example.com/\ninvalid",
    ] {
        assert!(validate_found_source(url).is_err(), "{url}");
    }
    validate_found_source("https://commons.wikimedia.org/wiki/File:Photo.jpg#Summary").unwrap();
}
#[test]
fn placeholder_metrics_count_slots_once_and_measure_legacy_unmarked_labels() {
    let content=SiteContent{html:"<main><section data-desegnos-placeholder='copy'><p>Placeholder <em>copy</em></p></section><p>Image placeholder</p><p>Actual content</p></main>".into(),css:String::new(),javascript:String::new()};
    assert_eq!(
        content.placeholder_metrics().unwrap(),
        PlaceholderMetrics {
            marked_slots: 1,
            legacy_labels: 1,
            total: 2
        }
    );
}
#[test]
fn capabilities_are_frozen_without_granting_search_to_an_adapter_that_lacks_it() {
    let project = project();
    let snapshot = project
        .prepare_requirements_with_capabilities(GenerationCapabilities {
            web_search: false,
            image_downloads: 8,
            image_generation: None,
        })
        .unwrap();
    assert_eq!(snapshot.format_version().unwrap(), 17);
    assert!(snapshot.markdown.contains("\"webSearch\": false"));
    assert!(
        !snapshot
            .markdown
            .contains("Provider web search is available")
    );
    assert!(snapshot.markdown.contains("\"maximumImages\": 8"));
    snapshot.validate_current(&project).unwrap();
    let mut changed = snapshot.clone();
    changed.capabilities.as_mut().unwrap().web_search = true;
    assert!(changed.validate_current(&project).is_err());
}

#[test]
fn local_replacements_clear_origin_metadata_while_style_only_edits_preserve_it() {
    let p = project();
    let page = p.home_page_id;
    let p=p.edit_page_source(p.revision,page,&PageSourceEdit { html:Some("<main><h1 data-desegnos-found='https://example.com/source'>Original</h1><img data-desegnos-found='https://example.com/photo' src='https://example.com/image.jpg' alt='Photo'></main>".into()),..Default::default() }).unwrap();
    for (tag, edit) in [
        (
            "h1",
            ElementEdit::Update {
                texts: vec![TextEdit {
                    index: 0,
                    text: "My own wording".into(),
                }],
                attributes: Default::default(),
                style: None,
            },
        ),
        (
            "img",
            ElementEdit::Update {
                texts: vec![],
                attributes: [(
                    "src".into(),
                    Some("https://example.org/replacement.jpg".into()),
                )]
                .into(),
                style: None,
            },
        ),
    ] {
        let target = ElementTarget {
            id: String::new(),
            selector: format!("body > main:nth-of-type(1) > {tag}:nth-of-type(1)"),
            tag: tag.into(),
            label: tag.into(),
            source_sha256: p.pages[0].content.as_ref().unwrap().sha256().unwrap(),
        };
        let (styled, _) = p
            .edit_element(
                p.revision,
                page,
                &target,
                &ElementEdit::Update {
                    texts: vec![],
                    attributes: Default::default(),
                    style: Some("color:red".into()),
                },
            )
            .unwrap();
        assert_eq!(
            styled.pages[0]
                .content
                .as_ref()
                .unwrap()
                .found_sources()
                .unwrap()
                .len(),
            2
        );
        let (changed, _) = p.edit_element(p.revision, page, &target, &edit).unwrap();
        assert_eq!(
            changed.pages[0]
                .content
                .as_ref()
                .unwrap()
                .found_sources()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            p.pages[0]
                .content
                .as_ref()
                .unwrap()
                .found_sources()
                .unwrap()
                .len(),
            2
        );
    }
}
