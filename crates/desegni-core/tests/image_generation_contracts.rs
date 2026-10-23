use desegni_core::*;
use serde_json::json;
use std::collections::BTreeMap;

fn fixture() -> (Project, RequirementsSnapshot, GenerationCandidate) {
    let old: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/requirements8.json")).unwrap();
    let mut project: Project = serde_json::from_value(old["project"].clone()).unwrap();
    project.schema_version = SCHEMA_VERSION;
    project.generation_mode = GenerationMode::Strict;
    // This fixture has one new rectangular area.
    project.pages[0].content = None;
    let snapshot = project
        .prepare_requirements_with_capabilities(GenerationCapabilities {
            image_generation: Some(ImageGenerationCapability::default()),
            ..Default::default()
        })
        .unwrap();
    let id = project.pages[0].regions[0].id;
    let candidate = GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Image draft".into(),
        regions: vec![RegionOutcome {
            region_id: id,
            status: RegionStatus::Implemented,
            explanation: "Requested native image generation.".into(),
        }],
        pages: vec![GeneratedPage {
            page_id: project.home_page_id,
            content: SiteContent {
                html: format!(
                    "<section data-desegnos-region='{id}'><img data-desegnos-generate='Portrait reclining on a shelf' data-desegnos-background='transparent' src='#' alt='Reclining portrait'></section>"
                ),
                css: String::new(),
                javascript: String::new(),
            },
        }],
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],

        documents: vec![],
    };
    (project, snapshot, candidate)
}

#[test]
fn new_capabilities_and_contract_are_explicit_and_legacy_serialization_is_identical() {
    let (project, snapshot, _) = fixture();
    assert_eq!(snapshot.format_version().unwrap(), 22);
    assert!(snapshot.markdown.contains("data-desegnos-generate"));
    assert!(
        snapshot
            .markdown
            .contains("Deliver every implementable region")
    );
    assert!(
        !snapshot
            .markdown
            .contains("A page with an unresolved region conflict cannot")
    );
    assert_eq!(
        serde_json::to_value(snapshot.capabilities.unwrap()).unwrap()["imageGeneration"],
        json!({"maxImages":2,"sizes":["1024x1024"]})
    );
    let plain = project.prepare_requirements().unwrap();
    assert!(!plain.markdown.contains("## Native image generation"));
    assert_eq!(
        serde_json::to_value(GenerationCapabilities::default()).unwrap(),
        json!({"webSearch":true,"imageDownloads":8})
    );
    for fixture in [
        include_str!("fixtures/requirements8.json"),
        include_str!("fixtures/requirements9.json"),
    ] {
        let value: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let mut project: Project = serde_json::from_value(value["project"].clone()).unwrap();
        project.schema_version = SCHEMA_VERSION;
        let snapshot: RequirementsSnapshot =
            serde_json::from_value(value["requirements"].clone()).unwrap();
        let before = serde_json::to_vec(&snapshot).unwrap();
        snapshot.validate_current(&project).unwrap();
        assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);
        assert!(!snapshot.markdown.contains("data-desegnos-generate"));
    }
}

#[test]
fn image_request_validation_rejects_bad_prompts_sizes_sources_and_unavailable_capabilities() {
    let (project, snapshot, candidate) = fixture();
    candidate.validate(&snapshot, &project).unwrap();
    let request = &candidate.image_generation_requests().unwrap()[0];
    assert!(request.transparent);
    assert_eq!(request.size, "1024x1024");
    assert_eq!(request.reference_image(&snapshot, &project).unwrap(), None);
    assert!(
        project
            .apply_generation(&snapshot, &candidate, &[])
            .is_err(),
        "unprepared images cannot apply"
    );
    for marker in [
        "data-desegnos-generate=''",
        "data-desegnos-generate='edit' data-desegnos-size='9000x9000'",
        "data-desegnos-generate='edit' data-desegnos-source='https://example.com/p.png'",
        "data-desegnos-generate='edit' data-desegnos-background='javascript'",
        "data-desegnos-generate='edit' data-desegnos-download",
    ] {
        let mut invalid = candidate.clone();
        invalid.pages[0].content.html = format!("<img {marker} src='#'>");
        assert!(invalid.image_generation_requests().is_err(), "{marker}");
    }
    let mut invalid = candidate.clone();
    invalid.pages[0].content.html =
        "<img data-desegnos-generate='edit' data-desegnos-source='unknown-attachment'>".into();
    assert!(
        invalid.image_generation_requests().unwrap()[0]
            .reference_image(&snapshot, &project)
            .is_err()
    );
    let unsupported = project.prepare_requirements().unwrap();
    let mut invalid = candidate;
    invalid.job_id = unsupported.job_id;
    invalid.requirements_sha256 = unsupported.sha256.clone();
    assert!(invalid.validate(&unsupported, &project).is_err());
}

#[test]
fn image_failure_keeps_the_ready_region_and_renders_a_local_placeholder() {
    let (project, snapshot, mut candidate) = fixture();
    assert_eq!(
        candidate.embed_generated_images(&BTreeMap::new()).unwrap(),
        1
    );
    candidate.validate(&snapshot, &project).unwrap();
    assert!(candidate.image_generation_requests().unwrap().is_empty());
    assert!(
        candidate.pages[0]
            .content
            .html
            .contains("data-desegnos-placeholder")
    );
    assert!(
        candidate.pages[0]
            .content
            .html
            .contains("Image generation failed")
    );
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    assert_eq!(applied.pending_count(), 0);
    assert!(!applied.pictures.is_empty(), "placeholder is portable");
    assert!(
        applied.pages[0]
            .content
            .as_ref()
            .unwrap()
            .html
            .contains("desegnos-asset:")
    );
}

#[test]
fn partial_apply_finishes_ready_region_and_retains_conflict_and_explanation() {
    let (mut project, _, mut candidate) = fixture();
    // Add a second request through the public command boundary.
    let page_id = project.home_page_id;
    project = project
        .apply(
            project.revision,
            Command::AddRegion {
                page_id,
                rect: Rect {
                    x: 10.,
                    y: 200.,
                    width: 100.,
                    height: 100.,
                },
            },
        )
        .unwrap();
    let region = project.pages[0].regions.last().unwrap().clone();
    project = project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id,
                region_id: region.id,
                rect: region.rect,
                prompt: "Needs private account details".into(),
            },
        )
        .unwrap();
    let snapshot = project.prepare_requirements().unwrap();
    candidate.job_id = snapshot.job_id;
    candidate.requirements_sha256 = snapshot.sha256.clone();
    candidate.regions.push(RegionOutcome {
        region_id: region.id,
        status: RegionStatus::Conflict,
        explanation: "Which account should this use?".into(),
    });
    candidate.pages[0].content.html.push_str(&format!("<section data-desegnos-region='{}' data-desegnos-placeholder='conflict'>Account details needed</section>",region.id));
    validate_page_source(&candidate.pages[0].content).unwrap();
    // Finish the candidate before image embedding serializes it as a complete document.
    candidate.embed_generated_images(&BTreeMap::new()).unwrap();
    candidate.validate(&snapshot, &project).unwrap();
    assert!(!candidate.is_ready());
    assert!(candidate.can_apply(&snapshot));
    assert!(
        result_schema(&snapshot)["properties"]["pages"]["description"]
            .as_str()
            .unwrap()
            .contains("implemented regions")
    );
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    assert!(applied.pages[0].regions[0].completed);
    assert!(!applied.pages[0].regions[1].completed);
    assert_eq!(applied.pending_count(), 1);
    assert_eq!(
        applied
            .latest_generation
            .as_ref()
            .unwrap()
            .criticism
            .iter()
            .find(|note| note.region_id == Some(region.id))
            .unwrap()
            .message,
        "Which account should this use?"
    );
    assert_eq!(
        applied
            .apply_generation(&snapshot, &candidate, &[])
            .unwrap(),
        applied,
        "repeat apply is idempotent"
    );
    let mut corrupted = candidate.clone();
    corrupted.pages[0].content.html = corrupted.pages[0]
        .content
        .html
        .replace(
            "data-desegnos-placeholder=\"conflict\"",
            "data-wrong=\"conflict\"",
        )
        .replace(
            "data-desegnos-placeholder='conflict'",
            "data-wrong='conflict'",
        );
    assert!(corrupted.validate(&snapshot, &project).is_err());
}

const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGOwaQoAIgYIBQAdRgQ5DejjuQAAAABJRU5ErkJggg==";

#[test]
fn successful_generated_picture_is_stored_and_can_be_reused_as_an_edit_reference() {
    let (project, snapshot, mut candidate) = fixture();
    let request = candidate.image_generation_requests().unwrap().remove(0);
    candidate
        .embed_generated_images(&BTreeMap::from([(request, PNG.into())]))
        .unwrap();
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    let id = *applied.pictures.keys().next().unwrap();
    let edit = ImageGenerationRequest {
        mask: None,
        prompt: "Recline on a shelf".into(),
        source: Some(project_asset_reference(id)),
        size: "1024x1024".into(),
        transparent: true,
    };
    assert_eq!(
        edit.reference_image(&snapshot, &applied)
            .unwrap()
            .as_deref(),
        Some(PNG)
    );
    assert!(
        applied.pages[0]
            .content
            .as_ref()
            .unwrap()
            .html
            .contains(&project_asset_reference(id))
    );
}

#[test]
fn edit_reference_uses_only_an_attachment_from_the_frozen_request() {
    let (mut project, _, _) = fixture();
    project.pages[0].regions[0]
        .attachments
        .push(RequestAttachment {
            id: "portrait1".into(),
            name: "Portrait.png".into(),
            kind: AttachmentKind::Image,
            data_uri: Some(PNG.into()),
            preview: None,
            text: None,
            url: None,
        });
    let snapshot = project.prepare_requirements().unwrap();
    let request = ImageGenerationRequest {
        mask: None,
        prompt: "Recline on a shelf".into(),
        source: Some("portrait1".into()),
        size: "1024x1024".into(),
        transparent: true,
    };
    assert_eq!(
        request
            .reference_image(&snapshot, &project)
            .unwrap()
            .as_deref(),
        Some(PNG)
    );
    let mut foreign = snapshot;
    foreign
        .dependencies
        .retain(|key, _| !key.starts_with("region:"));
    assert!(request.reference_image(&foreign, &project).is_err());
}

#[test]
fn old_capability_packet_keeps_unavailable_components_blocked() {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/requirements9.json")).unwrap();
    let mut project: Project = serde_json::from_value(value["project"].clone()).unwrap();
    project.schema_version = SCHEMA_VERSION;
    let snapshot: RequirementsSnapshot =
        serde_json::from_value(value["requirements"].clone()).unwrap();
    let region = project.pages[0].regions[0].id;
    let candidate = GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Legacy service placeholder".into(),
        regions: vec![RegionOutcome {
            region_id: region,
            status: RegionStatus::Implemented,
            explanation: "Draft with missing service.".into(),
        }],
        pages: vec![GeneratedPage {
            page_id: project.home_page_id,
            content: SiteContent {
                html: format!(
                    "<section data-desegnos-region='{region}'><p>Service placeholder</p></section>"
                ),
                css: String::new(),
                javascript: String::new(),
            },
        }],
        component_requests: vec![ComponentRequest {
            region_id: region,
            capability: "Private service".into(),
            resolution: ComponentResolution::Unavailable,
            reason: "No configuration supplied.".into(),
        }],
        decision_proposals: vec![],
        criticism: vec![],

        documents: vec![],
    };
    candidate.validate(&snapshot, &project).unwrap();
    assert!(!candidate.can_apply(&snapshot));
    assert!(
        project
            .apply_generation(&snapshot, &candidate, &[])
            .is_err()
    );
}

#[test]
fn mixed_existing_page_preserves_conflicted_source_and_shared_behavior() {
    let (mut project, _, mut candidate) = fixture();
    let ready = project.pages[0].regions[0].id;
    let page_id = project.home_page_id;
    project = project
        .apply(
            project.revision,
            Command::AddRegion {
                page_id,
                rect: Rect {
                    x: 10.,
                    y: 200.,
                    width: 100.,
                    height: 100.,
                },
            },
        )
        .unwrap();
    let region = project.pages[0].regions.last().unwrap().clone();
    project = project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id,
                region_id: region.id,
                rect: region.rect,
                prompt: "Needs details".into(),
            },
        )
        .unwrap();
    let conflict = region.id;
    let html = format!(
        "<section data-desegnos-region='{ready}'>Old ready content</section><section data-desegnos-region='{conflict}'>Keep this portrait</section>"
    );
    project.pages[0].content = Some(SiteContent {
        html: html.clone(),
        css: String::new(),
        javascript: String::new(),
    });
    let snapshot = project.prepare_requirements().unwrap();
    candidate.job_id = snapshot.job_id;
    candidate.requirements_sha256 = snapshot.sha256.clone();
    candidate.regions.push(RegionOutcome {
        region_id: conflict,
        status: RegionStatus::Conflict,
        explanation: "Image editing unavailable for this connection.".into(),
    });
    candidate.pages[0].content.html = html.replace("Old ready content", "New ready content");
    candidate.validate(&snapshot, &project).unwrap();
    let applied = project
        .apply_generation(&snapshot, &candidate, &[])
        .unwrap();
    assert_eq!(applied.pending_count(), 1);
    assert!(
        applied.pages[0]
            .content
            .as_ref()
            .unwrap()
            .html
            .contains("Keep this portrait")
    );
    let mut invalid = candidate.clone();
    invalid.pages[0].content.html = invalid.pages[0]
        .content
        .html
        .replace("Keep this portrait", "Removed portrait");
    assert!(invalid.validate(&snapshot, &project).is_err());
    let mut invalid = candidate;
    invalid.pages[0].content.css = "section { display:none }".into();
    assert!(invalid.validate(&snapshot, &project).is_err());
}

#[test]
fn masks_require_a_source_and_resolve_only_native_references() {
    let (project, snapshot, candidate) = fixture();
    for attributes in [
        "data-desegnos-mask='mask1'",
        "data-desegnos-source='source1' data-desegnos-mask='https://example.com/mask.png'",
        "data-desegnos-source='source1' data-desegnos-mask='../mask.png'",
    ] {
        let mut invalid = candidate.clone();
        invalid.pages[0].content.html = invalid.pages[0].content.html.replace(
            "data-desegnos-generate=",
            &format!("{attributes} data-desegnos-generate="),
        );
        assert!(invalid.image_generation_requests().is_err());
    }
    let request = ImageGenerationRequest {
        prompt: "Edit".into(),
        source: Some("source1".into()),
        mask: Some("unknown-mask".into()),
        size: "1024x1024".into(),
        transparent: true,
    };
    assert!(request.mask_image(&snapshot, &project).is_err());
}
