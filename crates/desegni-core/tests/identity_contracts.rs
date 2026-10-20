mod support;
use base64::{Engine, engine::general_purpose::STANDARD};
use desegni_core::*;
use support::without_editor_ids;
use uuid::Uuid;

fn blank() -> Project {
    Project::new(
        "New identity".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap()
}
fn image(mime: &str, bytes: &[u8]) -> IdentityAsset {
    IdentityAsset {
        id: Uuid::new_v4(),
        name: "Studio logo".into(),
        mime: mime.into(),
        data_uri: format!("data:{mime};base64,{}", STANDARD.encode(bytes)),
        sha256: digest(bytes),
    }
}
fn identity() -> ImportedIdentity {
    ImportedIdentity { source_name: "Studio website".into(), source_sha256: digest(b"the exact imported source"), summary: "Studio uses deep blue #16324F and Indonesian, with a rounded wordmark.".into(), assets: vec![image("image/svg+xml", br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><defs><linearGradient id="brand"><stop stop-color="#16324F"/></linearGradient></defs><rect width="20" height="20" fill="url(#brand)"/><title>Studio &amp; Company</title></svg>"##)] }
}
fn described(mut project: Project) -> Project {
    let page_id = project.home_page_id;
    project = project
        .apply(
            project.revision,
            Command::AddRegion {
                page_id,
                rect: Rect {
                    x: 20.0,
                    y: 20.0,
                    width: 300.0,
                    height: 160.0,
                },
            },
        )
        .unwrap();
    let region = project.pages[0].regions[0].clone();
    project
        .apply(
            project.revision,
            Command::UpdateRegion {
                page_id,
                region_id: region.id,
                rect: region.rect,
                prompt: "Create the welcome heading and use the imported logo.".into(),
            },
        )
        .unwrap()
}

#[test]
fn identity_initialization_keeps_the_new_project_blank_light_and_without_history() {
    let project = blank();
    let imported = identity();
    let initialized = project.set_initial_identity(imported.clone()).unwrap();
    assert_eq!(initialized.revision, 0);
    assert_eq!(initialized.pages, project.pages);
    assert_eq!(initialized.imported_identity, Some(imported));
    assert_eq!(initialized.appearance, Appearance::Light);
    let session = Session::new(initialized.clone()).unwrap();
    assert!(!session.can_undo());
    assert!(initialized.set_initial_identity(identity()).is_err());
    assert!(
        described(project.clone())
            .set_initial_identity(identity())
            .is_err()
    );
    let dark = Project::new("Dark".into(), project.viewport, Appearance::Dark).unwrap();
    assert!(dark.set_initial_identity(identity()).is_err());
    let mut generated = project.clone();
    generated.pages[0].content = Some(SiteContent {
        html: "<p>Existing page</p>".into(),
        css: String::new(),
        javascript: String::new(),
    });
    assert!(generated.set_initial_identity(identity()).is_err());
}

#[test]
fn portable_image_validation_checks_bounds_canonical_encoding_hash_and_declared_format() {
    let valid = identity().assets.remove(0);
    valid.validate().unwrap();
    for case in [
        "id",
        "mime",
        "uri",
        "hash",
        "upperHash",
        "noncanonical",
        "empty",
        "oversized",
        "disguised",
    ] {
        let mut invalid = valid.clone();
        match case {
            "id" => invalid.id = Uuid::nil(),
            "mime" => invalid.mime = "text/html".into(),
            "uri" => invalid.data_uri = "https://example.com/logo.svg".into(),
            "hash" => invalid.sha256 = "0".repeat(64),
            "upperHash" => invalid.sha256 = invalid.sha256.to_ascii_uppercase(),
            "noncanonical" => invalid.data_uri.push('\n'),
            "empty" => {
                invalid.data_uri = "data:image/svg+xml;base64,".into();
                invalid.sha256 = digest(b"");
            }
            "oversized" => {
                invalid = image("image/svg+xml", &vec![b'x'; MAX_IDENTITY_ASSET_BYTES + 1])
            }
            "disguised" => {
                invalid = image("image/png", b"<svg xmlns='http://www.w3.org/2000/svg'/>")
            }
            _ => unreachable!(),
        }
        assert!(invalid.validate().is_err(), "{case}");
    }
    let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
    image("image/png", &png).validate().unwrap();
    let gif = STANDARD
        .decode("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==")
        .unwrap();
    image("image/gif", &gif).validate().unwrap();
}

#[test]
fn svg_identity_images_cannot_carry_active_content_or_external_references() {
    image("image/svg+xml", br##"<svg xmlns="http://www.w3.org/2000/svg"><style>.logo{fill:url(&quot;#brand&quot;)} .accent{fill:#16324F}</style><defs><linearGradient id="brand"/></defs><rect class="logo" width="20" height="20"/></svg>"##).validate().unwrap();
    image("image/svg+xml", br##"<svg xmlns="http://www.w3.org/2000/svg"><style><![CDATA[.logo{fill:#16324F}]]></style><circle class="logo" r="5"/></svg>"##).validate().unwrap();
    for source in [
        "<svg><style>@import 'relative.css';</style></svg>",
        "<svg><style>.logo{fill:url(relative.svg)}</style></svg>",
        "<svg><style>.logo{fill:url(&#104;ttps://example.com/paint)}</style></svg>",
        "<svg><script>alert(1)</script></svg>",
        "<svg onload='alert(1)'/>",
        "<svg><foreignObject><p>HTML</p></foreignObject></svg>",
        "<svg><image href='https://example.com/logo.png'/></svg>",
        "<svg><use href='https://example.com/logo.svg#id'/></svg>",
        "<svg><use href='&#104;ttps://example.com/logo.svg#id'/></svg>",
        "<svg><rect fill='url(https://example.com/image)'/></svg>",
        "<svg><rect style='fill:u\\72l(https://example.com/image)'/></svg>",
        "<!DOCTYPE svg [<!ENTITY source SYSTEM 'file:///etc/passwd'>]><svg>&source;</svg>",
        "<?xml-stylesheet href='http://example.com/style.css'?><svg/>",
        "<svg><animate attributeName='href' to='http://example.com'/></svg>",
        "<svg></svg><svg/>",
        "<svg><g></svg>",
        "<svg>",
    ] {
        assert!(
            image("image/svg+xml", source.as_bytes())
                .validate()
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn imported_identity_validates_source_and_asset_uniqueness_before_attachment() {
    for case in [
        "source",
        "summary",
        "fingerprint",
        "count",
        "duplicate",
        "collision",
    ] {
        let mut imported = identity();
        let project = blank();
        match case {
            "source" => imported.source_name.clear(),
            "summary" => imported.summary.clear(),
            "fingerprint" => imported.source_sha256 = "not a hash".into(),
            "count" => {
                imported.assets = (0..=MAX_IDENTITY_ASSETS)
                    .map(|_| identity().assets.remove(0))
                    .collect();
            }
            "duplicate" => imported.assets.push(imported.assets[0].clone()),
            "collision" => imported.assets[0].id = project.home_page_id,
            _ => unreachable!(),
        }
        assert!(project.set_initial_identity(imported).is_err(), "{case}");
    }
    let mut summary_only = identity();
    summary_only.assets.clear();
    blank().set_initial_identity(summary_only).unwrap();
}

#[test]
fn requirements_reference_original_identity_images_without_transcribing_their_bytes() {
    let imported = identity();
    let project = described(blank().set_initial_identity(imported.clone()).unwrap());
    let markdown = project.project_markdown().unwrap();
    assert!(markdown.contains(&imported.summary));
    assert!(markdown.contains(&imported.assets[0].sha256));
    assert!(markdown.contains(&imported.assets[0].id.to_string()));
    assert!(!markdown.contains("data:image/"));
    let snapshot = project.prepare_requirements().unwrap();
    assert!(snapshot.markdown.contains("Requirements format: 10"));
    assert!(!snapshot.markdown.contains(&imported.assets[0].data_uri));
    assert!(
        snapshot
            .markdown
            .contains(&project_asset_reference(imported.assets[0].id))
    );
    assert!(snapshot.markdown.contains(&imported.summary));
    assert!(snapshot.markdown.contains(&imported.source_sha256));
    snapshot.validate_current(&project).unwrap();
    for case in ["summary", "source", "image", "removed"] {
        let mut changed = project.clone();
        match case {
            "summary" => changed
                .imported_identity
                .as_mut()
                .unwrap()
                .summary
                .push_str(" and a warm accent"),
            "source" => {
                changed.imported_identity.as_mut().unwrap().source_sha256 = digest(b"other source")
            }
            "image" => {
                changed.imported_identity.as_mut().unwrap().assets[0] = image(
                    "image/svg+xml",
                    b"<svg xmlns='http://www.w3.org/2000/svg'><circle r='5'/></svg>",
                )
            }
            "removed" => changed.imported_identity = None,
            _ => unreachable!(),
        }
        assert!(snapshot.validate_current(&changed).is_err(), "{case}");
    }
    assert!(
        described(blank())
            .prepare_requirements()
            .unwrap()
            .markdown
            .contains("Requirements format: 10")
    );
}

#[test]
fn identity_survives_serialization_and_undo_redo_without_reimporting() {
    let imported = identity();
    let project = blank().set_initial_identity(imported.clone()).unwrap();
    assert_eq!(
        decode_project(&serde_json::to_vec(&project).unwrap())
            .unwrap()
            .project,
        project
    );
    let mut session = Session::new(project).unwrap();
    session
        .execute(
            0,
            Command::SetDecision {
                topic: "Palette".into(),
                value: "Deep blue and white".into(),
                scope: Scope::Project,
                protected: true,
            },
            |_| Ok(()),
        )
        .unwrap();
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().imported_identity, Some(imported.clone()));
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().imported_identity, Some(imported));
}

#[test]
fn persisted_raster_headers_enforce_dimensions_before_any_preview_or_import() {
    let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
    for width in [0_u32, 16_385, u32::MAX] {
        let mut bytes = png.clone();
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        assert!(image("image/png", &bytes).validate().is_err());
    }
    let gif = STANDARD
        .decode("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==")
        .unwrap();
    for height in [0_u16, 16_385, u16::MAX] {
        let mut bytes = gif.clone();
        bytes[8..10].copy_from_slice(&height.to_le_bytes());
        assert!(image("image/gif", &bytes).validate().is_err());
    }
    let jpeg = |width: u16, height: u16| {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xc0, 0, 11, 8];
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes.extend([1, 1, 0x11, 0, 0xff, 0xd9]);
        bytes
    };
    image("image/jpeg", &jpeg(128, 64)).validate().unwrap();
    for (width, height) in [(0, 1), (1, 0), (16_385, 1), (1, 16_385)] {
        assert!(
            image("image/jpeg", &jpeg(width, height))
                .validate()
                .is_err()
        );
    }
    let mut short = jpeg(128, 64);
    short[4..6].copy_from_slice(&8_u16.to_be_bytes());
    assert!(image("image/jpeg", &short).validate().is_err());
    let webp = |kind: &[u8; 4], data: &[u8]| {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((12_u32 + data.len() as u32).to_le_bytes());
        bytes.extend(b"WEBP");
        bytes.extend(kind);
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(data);
        bytes
    };
    let mut extended = vec![0; 10];
    extended[4..7].copy_from_slice(&[127, 0, 0]);
    extended[7..10].copy_from_slice(&[63, 0, 0]);
    image("image/webp", &webp(b"VP8X", &extended))
        .validate()
        .unwrap();
    extended[4..7].copy_from_slice(&[0, 64, 0]);
    assert!(
        image("image/webp", &webp(b"VP8X", &extended))
            .validate()
            .is_err()
    );
    let mut lossless = vec![0x2f];
    lossless.extend(((127_u32) | (63_u32 << 14)).to_le_bytes());
    image("image/webp", &webp(b"VP8L", &lossless))
        .validate()
        .unwrap();
    assert!(
        image("image/webp", &webp(b"VP8L", &[0x2f, 0, 0]))
            .validate()
            .is_err()
    );
    let mut lossy = vec![0, 0, 0, 0x9d, 1, 0x2a];
    lossy.extend(128_u16.to_le_bytes());
    lossy.extend(64_u16.to_le_bytes());
    image("image/webp", &webp(b"VP8 ", &lossy))
        .validate()
        .unwrap();
    lossy[6..8].copy_from_slice(&0_u16.to_le_bytes());
    assert!(
        image("image/webp", &webp(b"VP8 ", &lossy))
            .validate()
            .is_err()
    );
}

fn asset_candidate(
    project: &Project,
    html: String,
    css: String,
) -> (RequirementsSnapshot, GenerationCandidate) {
    let snapshot = project.prepare_requirements().unwrap();
    let candidate = GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Used the requested logo.".into(),
        regions: vec![RegionOutcome {
            region_id: project.pages[0].regions[0].id,
            status: RegionStatus::Implemented,
            explanation: "Used the project image.".into(),
        }],
        pages: vec![GeneratedPage {
            page_id: project.home_page_id,
            content: SiteContent {
                html,
                css,
                javascript: String::new(),
            },
        }],
        component_requests: vec![],
        decision_proposals: vec![],
        criticism: vec![],
    };
    (snapshot, candidate)
}

#[test]
fn stable_project_image_references_resolve_exactly_export_and_undo_without_transcription() {
    let mut imported = identity();
    let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jMZkAAAAASUVORK5CYII=").unwrap();
    imported.assets.push(image("image/png", &png));
    let project = described(blank().set_initial_identity(imported.clone()).unwrap());
    let svg_ref = project_asset_reference(imported.assets[0].id);
    let png_ref = project_asset_reference(imported.assets[1].id);
    let (snapshot, mut candidate) = asset_candidate(
        &project,
        format!(
            "<main data-desegnos-region='{}'><img src='{svg_ref}' alt='Studio'><img src='{png_ref}' alt='Studio raster'></main>",
            project.pages[0].regions[0].id
        ),
        format!("main{{background-image:url('{png_ref}')}}"),
    );
    assert!(!snapshot.markdown.contains("data:image/"));
    assert!(candidate.validate(&snapshot, &project).is_err());
    assert_eq!(candidate.resolve_project_assets(&project).unwrap(), 3);
    assert_eq!(candidate.resolve_project_assets(&project).unwrap(), 0);
    candidate.validate(&snapshot, &project).unwrap();
    let expected = candidate.pages[0].content.clone();
    assert!(expected.html.contains(&imported.assets[0].data_uri));
    assert!(expected.html.contains(&imported.assets[1].data_uri));
    assert!(expected.css.contains(&imported.assets[1].data_uri));
    let mut session = Session::new(project.clone()).unwrap();
    session
        .apply_generation(&snapshot, &candidate, &[], |_| Ok(()))
        .unwrap();
    let exported = compile_website(session.project()).unwrap();
    for asset in &imported.assets {
        assert!(
            exported
                .files
                .values()
                .any(|bytes| digest(bytes) == asset.sha256)
        );
    }
    let next_source = session
        .project()
        .generation_source_with_asset_references(&expected);
    assert!(next_source.html.contains(&svg_ref));
    assert!(next_source.css.contains(&png_ref));
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, project.pages);
    session.redo(|_| Ok(())).unwrap();
    let restored = session.project().pages[0].content.as_ref().unwrap();
    assert_eq!(without_editor_ids(&restored.html), expected.html);
    assert_eq!(restored.css, expected.css);
    assert_eq!(restored.javascript, expected.javascript);
}

#[test]
fn invalid_project_references_and_damaged_image_bytes_cannot_apply_or_partially_resolve() {
    let imported = identity();
    let project = described(blank().set_initial_identity(imported.clone()).unwrap());
    let valid = project_asset_reference(imported.assets[0].id);
    for invalid in [
        project_asset_reference(Uuid::new_v4()),
        "desegnos-asset:malformed".into(),
    ] {
        let (_, mut candidate) = asset_candidate(
            &project,
            format!("<img src='{valid}'><img src='{invalid}'>"),
            String::new(),
        );
        let before = candidate.clone();
        assert!(candidate.resolve_project_assets(&project).is_err());
        assert_eq!(candidate, before);
    }
    let (snapshot, candidate) = asset_candidate(
        &project,
        format!(
            "<main data-desegnos-region='{}'><img src='data:image/png;base64,aGVsbG8=Z'></main>",
            project.pages[0].regions[0].id
        ),
        String::new(),
    );
    assert!(
        candidate
            .validate(&snapshot, &project)
            .unwrap_err()
            .0
            .contains("damaged image")
    );
    assert!(
        project
            .apply_generation(&snapshot, &candidate, &[])
            .is_err()
    );
}

#[test]
fn flexible_and_strict_reject_unsolicited_content_outside_new_request_roots() {
    for mode in [GenerationMode::Fuzzy, GenerationMode::Strict] {
        let mut project = described(blank());
        project.generation_mode = mode;
        let root = format!(
            "<main data-desegnos-region='{}'><h1>Welcome</h1></main>",
            project.pages[0].regions[0].id
        );
        let (snapshot, valid) = asset_candidate(&project, root.clone(), String::new());
        valid.validate(&snapshot, &project).unwrap();
        for extra in [
            "<header>Draft · Made for you</header>",
            "<p>Note: replace these items later</p>",
            "<nav><a href='#'>New section</a></nav>",
        ] {
            let mut candidate = valid.clone();
            candidate.pages[0].content.html = format!("{extra}{root}");
            assert!(candidate.validate(&snapshot, &project).is_err());
            assert!(
                project
                    .apply_generation(&snapshot, &candidate, &[])
                    .is_err()
            );
        }
    }
}

#[test]
fn import_context_merges_rules_into_an_open_project_without_overwriting_protected_ones() {
    use desegni_core::*;
    let project = Project::new(
        "Bakery".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap();
    let project = project
        .apply(
            project.revision,
            Command::SetDecision {
                topic: "Primary colour".into(),
                value: "#000000".into(),
                scope: Scope::Project,
                protected: true,
            },
        )
        .unwrap();
    let project = project
        .apply(
            project.revision,
            Command::SetDecision {
                topic: "Heading font".into(),
                value: "Arial".into(),
                scope: Scope::Project,
                protected: false,
            },
        )
        .unwrap();
    let context = ImportedContext {
        source_name: "crumb.example".into(),
        source_sha256: "a".repeat(64),
        summary: "Brand name: Crumb & Co".into(),
        findings: vec![
            ("Brand name".into(), "Crumb & Co".into()),
            ("Tone".into(), "Warm and rustic".into()),
            ("Primary colour".into(), "#c0392b".into()),
            ("heading font".into(), "Georgia, serif".into()),
            ("Accent colour".into(), "#fdf6e3".into()),
        ],
        assets: vec![],
    };
    let next = project
        .import_context(project.revision, context.clone())
        .unwrap();
    assert_eq!(next.revision, project.revision + 1);
    assert_eq!(next.brief.brand_name, "Crumb & Co");
    assert_eq!(next.brief.tone, "Warm and rustic");
    let rule = |topic: &str| {
        next.decisions
            .iter()
            .find(|d| d.topic.eq_ignore_ascii_case(topic))
            .unwrap()
            .value
            .clone()
    };
    assert_eq!(
        rule("Primary colour"),
        "#000000",
        "protected rules are kept"
    );
    assert_eq!(rule("Heading font"), "Georgia, serif");
    assert_eq!(rule("Accent colour"), "#fdf6e3");
    assert_eq!(
        next.decisions
            .iter()
            .filter(|d| d.topic.eq_ignore_ascii_case("heading font"))
            .count(),
        1
    );
    assert_eq!(
        next.imported_identity.as_ref().unwrap().source_name,
        "crumb.example"
    );
    // A second import keeps the first summary and replaces the source.
    let again = next
        .import_context(
            next.revision,
            ImportedContext {
                source_name: "second".into(),
                ..context.clone()
            },
        )
        .unwrap();
    assert!(
        again
            .imported_identity
            .as_ref()
            .unwrap()
            .summary
            .contains("Crumb & Co\nBrand name")
    );
    assert!(
        project
            .import_context(project.revision + 1, context.clone())
            .is_err(),
        "stale revisions are refused"
    );
    assert!(
        project
            .import_context(
                project.revision,
                ImportedContext {
                    findings: vec![],
                    ..context
                }
            )
            .is_err(),
        "empty imports are refused"
    );
}
