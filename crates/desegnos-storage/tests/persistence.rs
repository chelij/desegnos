use desegnos_core::*;
use desegnos_storage::*;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::PathBuf,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn without_editor_ids(html: &str) -> String {
    let mut result = html.to_owned();
    while let Some(start) = result.find(" data-desegnos-id=\"") {
        let end = result[start + 19..]
            .find('"')
            .expect("closing editor attribute")
            + start
            + 20;
        result.replace_range(start..end, "");
    }
    result
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("desegnos-storage-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn project() -> Project {
    Project::new(
        "Saved project".into(),
        Viewport {
            width: 390,
            height: 844,
        },
        Appearance::Dark,
    )
    .unwrap()
}

#[test]
fn project_filename_writes_the_current_archive_marker() {
    let temp = Temp::new();
    let original = project();
    for extension in PROJECT_FILE_EXTENSIONS {
        let path = temp.0.join(format!("site.{extension}"));
        let handle = ProjectFile::acquire(&path).unwrap();
        handle.save(&original).unwrap();
        assert_eq!(handle.path(), path);
        assert_eq!(handle.load().unwrap(), original);
        let mut archive = ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let mut manifest = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&manifest).unwrap()["format"],
            "desegnos"
        );
        let changed = original
            .apply(
                0,
                Command::RenameProject {
                    name: "Updated project".into(),
                },
            )
            .unwrap();
        handle.save(&changed).unwrap();
        assert_eq!(handle.path(), path);
        assert_eq!(handle.load().unwrap(), changed);
        assert_eq!(
            load_project(&path.with_file_name(format!("site.{extension}.previous"))).unwrap(),
            original
        );
    }
    assert!(ProjectFile::acquire(&temp.0.join("site.zip")).is_err());
}

#[test]
fn archive_contains_real_generated_notes_and_round_trips_decisions() {
    let temp = Temp::new();
    let mut p = project();
    p = p
        .apply(
            0,
            Command::SetDecision {
                topic: "Brand identity".into(),
                value: "Green and cream".into(),
                scope: Scope::Project,
                protected: true,
            },
        )
        .unwrap();
    let path = temp.0.join("site.desegnos");
    let handle = ProjectFile::acquire(&path).unwrap();
    handle.save(&p).unwrap();
    assert_eq!(handle.load().unwrap(), p);
    let mut archive = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut entry = archive.by_name("PROJECT.md").unwrap();
    let mut actual = String::new();
    std::io::Read::read_to_string(&mut entry, &mut actual).unwrap();
    assert_eq!(actual, p.project_markdown().unwrap());
    assert!(actual.contains("Green and cream"));
}

#[test]
fn writer_lock_excludes_second_writer_and_releases_on_drop() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let first = ProjectFile::acquire(&path).unwrap();
    assert!(ProjectFile::acquire(&path).is_err());
    drop(first);
    ProjectFile::acquire(&path).unwrap();
}

#[test]
fn second_save_keeps_the_previous_valid_project() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let handle = ProjectFile::acquire(&path).unwrap();
    let p = project();
    handle.save(&p).unwrap();
    let next = p
        .apply(
            0,
            Command::RenameProject {
                name: "Second revision".into(),
            },
        )
        .unwrap();
    handle.save(&next).unwrap();
    assert_eq!(handle.load().unwrap(), next);
    assert_eq!(
        load_project(&temp.0.join("site.desegnos.previous")).unwrap(),
        p
    );
}

#[test]
fn recovery_ignores_only_an_incomplete_final_write() {
    let temp = Temp::new();
    let p = project();
    let journal = Journal::new(&temp.0, p.id).unwrap();
    journal.append(&p).unwrap();
    let next = p.apply(0, Command::SetGrid { enabled: true }).unwrap();
    journal.append(&next).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(journal.path())
        .unwrap()
        .write_all(b"{\"partial\"")
        .unwrap();
    assert_eq!(Journal::recover(journal.path()).unwrap(), Some(next));
}

#[test]
fn hostile_archive_paths_are_rejected_without_extracting_files() {
    let temp = Temp::new();
    let path = temp.0.join("unsafe.desegnos");
    let mut archive = ZipWriter::new(fs::File::create(&path).unwrap());
    for name in ["manifest.json", "project.json", "../escape"] {
        archive
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"{}").unwrap();
    }
    archive.finish().unwrap();
    assert!(load_project(&path).is_err());
    assert!(!temp.0.parent().unwrap().join("escape").exists());
}

#[test]
fn invalid_data_does_not_replace_valid_save() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let handle = ProjectFile::acquire(&path).unwrap();
    let mut p = project();
    handle.save(&p).unwrap();
    let before = fs::read(&path).unwrap();
    p.pages.clear();
    assert!(handle.save(&p).is_err());
    assert_eq!(before, fs::read(path).unwrap());
}

#[test]
fn journal_rejects_other_projects_and_non_forward_revisions_before_writing() {
    let temp = Temp::new();
    let p = project();
    let journal = Journal::new(&temp.0, p.id).unwrap();
    assert!(journal.append(&project()).is_err());
    journal.append(&p).unwrap();
    let next = p.apply(0, Command::SetGrid { enabled: true }).unwrap();
    journal.append(&next).unwrap();
    let before = fs::read(journal.path()).unwrap();
    assert!(journal.append(&p).is_err());
    assert!(journal.append(&next).is_err());
    assert_eq!(fs::read(journal.path()).unwrap(), before);
    assert_eq!(Journal::recover(journal.path()).unwrap(), Some(next));
}

#[test]
fn appending_after_an_incomplete_write_keeps_recovery_valid() {
    let temp = Temp::new();
    let p = project();
    let journal = Journal::new(&temp.0, p.id).unwrap();
    journal.append(&p).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(journal.path())
        .unwrap()
        .write_all(b"{\"partial\"")
        .unwrap();
    let next = p.apply(0, Command::SetGrid { enabled: true }).unwrap();
    journal.append(&next).unwrap();
    assert_eq!(Journal::recover(journal.path()).unwrap(), Some(next));
}

#[cfg(unix)]
#[test]
fn a_project_symlink_uses_the_target_writer_lock() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let first = ProjectFile::acquire(&path).unwrap();
    first.save(&project()).unwrap();
    let alias = temp.0.join("alias.desegnos");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert!(ProjectFile::acquire(&alias).is_err());
    drop(first);
    let through_alias = ProjectFile::acquire(&alias).unwrap();
    assert_eq!(through_alias.path(), path.canonicalize().unwrap());
}

#[cfg(unix)]
#[test]
fn a_previous_save_symlink_cannot_overwrite_an_unrelated_file() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let handle = ProjectFile::acquire(&path).unwrap();
    let p = project();
    handle.save(&p).unwrap();
    let unrelated = temp.0.join("unrelated.txt");
    fs::write(&unrelated, b"Keep this actual data").unwrap();
    std::os::unix::fs::symlink(&unrelated, temp.0.join("site.desegnos.previous")).unwrap();
    handle
        .save(&p.apply(0, Command::SetGrid { enabled: true }).unwrap())
        .unwrap();
    assert_eq!(fs::read(unrelated).unwrap(), b"Keep this actual data");
    assert_eq!(
        load_project(&temp.0.join("site.desegnos.previous")).unwrap(),
        p
    );
}

#[test]
fn a_damaged_existing_archive_is_preserved_when_saving() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let handle = ProjectFile::acquire(&path).unwrap();
    let p = project();
    handle.save(&p).unwrap();
    handle
        .save(&p.apply(0, Command::SetGrid { enabled: true }).unwrap())
        .unwrap();
    let backup = fs::read(temp.0.join("site.desegnos.previous")).unwrap();
    fs::write(&path, b"damaged archive").unwrap();
    assert!(handle.save(&p).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"damaged archive");
    assert_eq!(
        fs::read(temp.0.join("site.desegnos.previous")).unwrap(),
        backup
    );
}

#[test]
fn unknown_extension_data_survives_save_reopen_and_recovery() {
    let temp = Temp::new();
    let mut p = project();
    p.extensions.insert(
        "org.example.future".into(),
        ExtensionRecord {
            schema_version: 9,
            data: serde_json::json!({"nested": [{"data": [1, "unchanged", null]}]}),
        },
    );
    let handle = ProjectFile::acquire(&temp.0.join("site.desegnos")).unwrap();
    handle.save(&p).unwrap();
    assert_eq!(handle.load().unwrap().extensions, p.extensions);
    let journal = Journal::new(&temp.0, p.id).unwrap();
    journal.append(&p).unwrap();
    assert_eq!(
        Journal::recover(journal.path())
            .unwrap()
            .unwrap()
            .extensions,
        p.extensions
    );
}

#[test]
fn failed_journal_replacement_can_retry_the_same_transaction() {
    let temp = Temp::new();
    let p = project();
    let journal = Journal::new(&temp.0, p.id).unwrap();
    journal.append(&p).unwrap();
    let parked = temp.0.join("parked.journal");
    fs::rename(journal.path(), &parked).unwrap();
    fs::create_dir(journal.path()).unwrap();
    let next = p.apply(0, Command::SetGrid { enabled: true }).unwrap();
    assert!(journal.append(&next).is_err());
    assert_eq!(Journal::recover(&parked).unwrap(), Some(p));
    fs::remove_dir(journal.path()).unwrap();
    fs::rename(&parked, journal.path()).unwrap();
    journal.append(&next).unwrap();
    assert_eq!(Journal::recover(journal.path()).unwrap(), Some(next));
}

#[test]
fn failed_backup_preserves_the_saved_project_and_cleans_temporary_files() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let handle = ProjectFile::acquire(&path).unwrap();
    let p = project();
    handle.save(&p).unwrap();
    fs::create_dir(temp.0.join("site.desegnos.previous")).unwrap();
    let next = p.apply(0, Command::SetGrid { enabled: true }).unwrap();
    assert!(handle.save(&next).is_err());
    assert_eq!(handle.load().unwrap(), p);
    assert!(fs::read_dir(&temp.0).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".desegnos-")
    }));
}

#[cfg(unix)]
#[test]
fn hard_link_aliases_are_refused_for_writing() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let handle = ProjectFile::acquire(&path).unwrap();
    handle.save(&project()).unwrap();
    drop(handle);
    let alias = temp.0.join("alias.desegnos");
    fs::hard_link(&path, &alias).unwrap();
    assert!(ProjectFile::acquire(&path).is_err());
    assert!(ProjectFile::acquire(&alias).is_err());
    assert_eq!(fs::read(path).unwrap(), fs::read(alias).unwrap());
}

#[cfg(unix)]
#[test]
fn project_notes_and_recovery_snapshots_are_private_files() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new();
    let p = project();
    let path = temp.0.join("site.desegnos");
    let handle = ProjectFile::acquire(&path).unwrap();
    handle.save(&p).unwrap();
    handle
        .save(&p.apply(0, Command::SetGrid { enabled: true }).unwrap())
        .unwrap();
    let journal = Journal::new(&temp.0, p.id).unwrap();
    journal.append(&p).unwrap();
    for file in [
        &path,
        &temp.0.join("site.desegnos.previous"),
        journal.path(),
    ] {
        assert_eq!(fs::metadata(file).unwrap().permissions().mode() & 0o077, 0);
    }
}

fn archive_entries(source: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(source)).unwrap();
    ["manifest.json", "project.json", "PROJECT.md"]
        .into_iter()
        .map(|name| {
            let mut data = Vec::new();
            archive
                .by_name(name)
                .unwrap()
                .read_to_end(&mut data)
                .unwrap();
            (name.to_owned(), data)
        })
        .collect()
}

fn write_entries(path: &std::path::Path, entries: &BTreeMap<String, Vec<u8>>) {
    let mut archive = ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        archive
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap();
}

fn update_manifest_hashes(entries: &mut BTreeMap<String, Vec<u8>>) {
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&entries["manifest.json"]).unwrap();
    for name in ["project.json", "PROJECT.md"] {
        manifest["files"][name] = digest(&entries[name]).into();
    }
    entries.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
}

#[test]
fn global_generation_mode_round_trips_in_archives_and_durable_recovery() {
    let temp = Temp::new();
    let original = project();
    let strict = original
        .apply(
            0,
            Command::SetGenerationMode {
                mode: GenerationMode::Strict,
            },
        )
        .unwrap();
    let file = ProjectFile::acquire(&temp.0.join("strict.desegnos")).unwrap();
    file.save(&strict).unwrap();
    assert_eq!(file.load().unwrap().generation_mode, GenerationMode::Strict);
    let journal = Journal::new(&temp.0, strict.id).unwrap();
    journal.append(&strict).unwrap();
    assert_eq!(
        Journal::recover(journal.path())
            .unwrap()
            .unwrap()
            .generation_mode,
        GenerationMode::Strict
    );
}

#[test]
fn signed_placements_grid_settings_and_generated_dossier_round_trip_through_real_storage() {
    let temp = Temp::new();
    let (p, _, _) = generation_fixture();
    let region = &p.pages[0].regions[0];
    let moved = p
        .apply(
            p.revision,
            Command::UpdateRegion {
                page_id: p.home_page_id,
                region_id: region.id,
                rect: Rect {
                    x: -800.0,
                    y: 1400.0,
                    width: 600.0,
                    height: 200.0,
                },
                prompt: region.prompt.clone(),
            },
        )
        .unwrap();
    let configured = moved
        .apply(
            moved.revision,
            Command::SetGridSettings {
                settings: GridSettings {
                    spacing: 5.0,
                    unit: GridUnit::Percent,
                    snap: true,
                    rulers: false,
                },
            },
        )
        .unwrap();
    let snapshot = configured.prepare_requirements().unwrap();
    let (_, _, mut candidate) = generation_fixture();
    candidate.job_id = snapshot.job_id;
    candidate.requirements_sha256 = snapshot.sha256.clone();
    candidate.pages[0].content.html = candidate.pages[0].content.html.replace(
        &candidate.regions[0].region_id.to_string(),
        &configured.pages[0].regions[0].id.to_string(),
    );
    candidate.regions[0].region_id = configured.pages[0].regions[0].id;
    candidate.pages[0].page_id = configured.home_page_id;
    candidate.decision_proposals[0].scope = Scope::Page(configured.home_page_id);
    candidate.criticism = vec![DesignCriticism {
        region_id: Some(candidate.regions[0].region_id),
        message: "The disclosure needs a legible heading.".into(),
        suggestion: "Keep the summary visible and concise.".into(),
    }];
    let mut session = Session::new(configured.clone()).unwrap();
    let journal = Journal::new(&temp.0, configured.id).unwrap();
    journal.append(&configured).unwrap();
    session
        .record_generation(&snapshot, &candidate, 2, |next| journal.append(next))
        .unwrap();
    assert_eq!(
        Journal::recover(journal.path()).unwrap().as_ref(),
        Some(session.project())
    );
    let path = temp.0.join("dossier.desegnos");
    let file = ProjectFile::acquire(&path).unwrap();
    file.save(session.project()).unwrap();
    let draft = file.load().unwrap();
    assert_eq!(&draft, session.project());
    candidate.validate(&snapshot, &draft).unwrap();
    let entries = archive_entries(&fs::read(&path).unwrap());
    let md = std::str::from_utf8(&entries["PROJECT.md"]).unwrap();
    assert!(md.contains(&candidate.summary));
    assert!(md.contains("legible heading"));
    assert!(md.contains("\"applicationStatus\": \"draft\""));
    session
        .apply_generation(&snapshot, &candidate, &[0], |next| journal.append(next))
        .unwrap();
    session.undo(|next| journal.append(next)).unwrap();
    let undone = Journal::recover(journal.path()).unwrap().unwrap();
    assert_eq!(
        undone
            .latest_generation
            .as_ref()
            .unwrap()
            .application_status,
        GenerationApplicationStatus::Undone
    );
    assert_eq!(undone.pages, configured.pages);
    assert_eq!(undone.grid_settings, configured.grid_settings);
    session.redo(|next| journal.append(next)).unwrap();
    file.save(session.project()).unwrap();
    assert_eq!(file.load().unwrap(), *session.project());
    assert_eq!(
        load_project(&temp.0.join("dossier.desegnos.previous")).unwrap(),
        draft
    );
}

// Current local requests own their behavior inside the region root; shared JS stays unchanged.
fn generation_fixture() -> (Project, RequirementsSnapshot, GenerationCandidate) {
    let mut p = project();
    p = p
        .apply(
            p.revision,
            Command::AddRegion {
                page_id: p.home_page_id,
                rect: Rect {
                    x: 24.0,
                    y: 24.0,
                    width: 280.0,
                    height: 140.0,
                },
            },
        )
        .unwrap();
    let region = &p.pages[0].regions[0];
    p = p
        .apply(
            p.revision,
            Command::UpdateRegion {
                page_id: p.home_page_id,
                region_id: region.id,
                rect: region.rect,
                prompt: "A native disclosure about the project.".into(),
            },
        )
        .unwrap();
    p.extensions.insert(
        "org.example.generation-history".into(),
        ExtensionRecord {
            schema_version: 4,
            data: serde_json::json!({"opaque":["keep", 42]}),
        },
    );
    let snapshot = p.prepare_requirements().unwrap();
    let candidate = GenerationCandidate {
        job_id: snapshot.job_id,
        requirements_sha256: snapshot.sha256.clone(),
        summary: "Added the requested native disclosure.".into(),
        regions: vec![RegionOutcome {
            region_id: p.pages[0].regions[0].id,
            status: RegionStatus::Implemented,
            explanation: "Uses native details and summary elements.".into(),
        }],
        pages: vec![GeneratedPage {
            page_id: p.home_page_id,
            content: SiteContent {
                html: format!(
                    "<details data-desegnos-region=\"{}\"><summary>About this project</summary><p>Saved generated source.</p><script type=\"application/x-desegnos-region\" data-desegnos-script=\"{}\">root.addEventListener('toggle', () => {{}});</script></details>",
                    p.pages[0].regions[0].id, p.pages[0].regions[0].id
                ),
                css: "details { padding: 1rem; }".into(),
                javascript: String::new(),
            },
        }],
        component_requests: vec![],
        decision_proposals: vec![DecisionProposal {
            topic: "Explanation format".into(),
            value: "Native disclosure".into(),
            scope: Scope::Page(p.home_page_id),
            rationale: "The detail is optional to read.".into(),
        }],
        criticism: vec![],
        outside_changes: vec![],

        documents: vec![],
    };
    (p, snapshot, candidate)
}

#[test]
fn generated_sources_selected_decisions_and_applied_jobs_survive_save_undo_recovery_and_reopen() {
    let temp = Temp::new();
    let (p, snapshot, candidate) = generation_fixture();
    let mut session = Session::new(p.clone()).unwrap();
    let journal = Journal::new(&temp.0, p.id).unwrap();
    journal.append(&p).unwrap();
    session
        .apply_generation(&snapshot, &candidate, &[0], |next| journal.append(next))
        .unwrap();
    let applied = session.project().clone();
    assert_eq!(
        Journal::recover(journal.path()).unwrap(),
        Some(applied.clone())
    );
    let path = temp.0.join("generated.desegnos");
    let file = ProjectFile::acquire(&path).unwrap();
    file.save(&applied).unwrap();
    let reopened = file.load().unwrap();
    assert_eq!(reopened, applied);
    let accepted = reopened.pages[0].content.as_ref().unwrap();
    let readable = format_site_content(&candidate.pages[0].content);
    assert_eq!(without_editor_ids(&accepted.html), readable.html);
    assert_eq!(accepted.css, readable.css);
    assert_eq!(accepted.javascript, candidate.pages[0].content.javascript);
    assert!(
        reopened
            .project_markdown()
            .unwrap()
            .contains("Native disclosure")
    );
    let mut restarted = Session::new(reopened).unwrap();
    restarted
        .apply_generation(&snapshot, &candidate, &[0], |_| {
            panic!("saved job must not apply twice")
        })
        .unwrap();
    assert!(!restarted.can_undo());
    assert_eq!(restarted.project(), &applied);
    session.undo(|next| journal.append(next)).unwrap();
    let undone = Journal::recover(journal.path()).unwrap().unwrap();
    assert_eq!(undone.pages, p.pages);
    assert_eq!(undone.decisions, p.decisions);
    assert_eq!(undone.applied_job_ids, vec![candidate.job_id]);
    assert_eq!(undone.extensions, p.extensions);
    let mut recovered = Session::new(undone.clone()).unwrap();
    recovered
        .apply_generation(&snapshot, &candidate, &[0], |_| {
            panic!("undo and recovery must retain job idempotency")
        })
        .unwrap();
    assert_eq!(recovered.project(), &undone);
    session.redo(|next| journal.append(next)).unwrap();
    assert_eq!(session.project().pages, applied.pages);
    file.save(session.project()).unwrap();
    assert_eq!(file.load().unwrap(), *session.project());
    assert_eq!(
        load_project(&temp.0.join("generated.desegnos.previous")).unwrap(),
        applied
    );
    let saved_bytes = fs::read(&path).unwrap();
    let mut oversized = session.project().clone();
    oversized.pages[0].content.as_mut().unwrap().html = "x".repeat(MAX_SITE_SOURCE_BYTES + 1);
    assert!(file.save(&oversized).is_err());
    assert_eq!(fs::read(&path).unwrap(), saved_bytes);
}

#[test]
fn failed_generation_journal_commit_retries_without_losing_state_or_advancing_revision() {
    let temp = Temp::new();
    let (p, snapshot, candidate) = generation_fixture();
    let journal = Journal::new(&temp.0, p.id).unwrap();
    journal.append(&p).unwrap();
    let mut session = Session::new(p.clone()).unwrap();
    fs::remove_file(journal.path()).unwrap();
    fs::create_dir(journal.path()).unwrap();
    assert!(
        session
            .apply_generation(&snapshot, &candidate, &[0], |next| journal.append(next))
            .is_err()
    );
    assert_eq!(session.project(), &p);
    assert!(!session.can_undo());
    assert!(session.project().applied_job_ids.is_empty());
    fs::remove_dir(journal.path()).unwrap();
    session
        .apply_generation(&snapshot, &candidate, &[0], |next| journal.append(next))
        .unwrap();
    assert_eq!(session.project().revision, p.revision + 1);
    assert_eq!(
        Journal::recover(journal.path()).unwrap().as_ref(),
        Some(session.project())
    );
    session
        .apply_generation(&snapshot, &candidate, &[0], |_| {
            panic!("acknowledged transaction cannot repeat")
        })
        .unwrap();
}

#[test]
fn direct_element_edits_survive_archive_reopen_recovery_and_exact_source_undo() {
    let temp = Temp::new();
    let mut p = project();
    p.pages[0].content = Some(SiteContent {
        html: "<button><span>Original</span></button>".into(),
        css: "button{color:red}".into(),
        javascript: "const original = true;".into(),
    });
    let page = p.home_page_id;
    let target = ElementTarget {
        id: String::new(),
        selector: "body > button:nth-of-type(1)".into(),
        tag: "button".into(),
        label: "Original".into(),
        source_sha256: p.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    };
    let original = p.pages.clone();
    let journal = Journal::new(&temp.0, p.id).unwrap();
    journal.append(&p).unwrap();
    let mut session = Session::new(p).unwrap();
    session
        .edit_element(
            0,
            page,
            &target,
            &ElementEdit::Update {
                texts: vec![TextEdit {
                    index: 0,
                    text: "Locally edited 日本語".into(),
                }],
                attributes: BTreeMap::new(),
                style: Some("translate:20px 10px".into()),
            },
            |next| {
                journal
                    .append(next)
                    .map_err(|error| Error(error.to_string()))
            },
        )
        .unwrap();
    let edited = session.project().clone();
    let file = ProjectFile::acquire(&temp.0.join("direct.desegnos")).unwrap();
    file.save(&edited).unwrap();
    assert_eq!(file.load().unwrap(), edited);
    assert_eq!(
        Journal::recover(journal.path()).unwrap(),
        Some(edited.clone())
    );
    session
        .undo(|next| {
            journal
                .append(next)
                .map_err(|error| Error(error.to_string()))
        })
        .unwrap();
    assert_eq!(session.project().pages, original);
    session
        .redo(|next| {
            journal
                .append(next)
                .map_err(|error| Error(error.to_string()))
        })
        .unwrap();
    assert_eq!(session.project().pages, edited.pages);
    assert_eq!(
        Journal::recover(journal.path()).unwrap(),
        Some(session.project().clone())
    );
}

fn identity_project() -> Project {
    let bytes = b"<svg xmlns='http://www.w3.org/2000/svg'/>";
    let identity = ImportedIdentity {
        pages: vec![],
        source_name: "Original studio website.zip".into(),
        source_sha256: digest(b"selected website input"),
        summary: "Use the reviewed blue logo and Indonesian language.".into(),
        assets: vec![IdentityAsset {
            id: uuid::Uuid::new_v4(),
            name: "Reviewed logo".into(),
            mime: "image/svg+xml".into(),
            data_uri:
                "data:image/svg+xml;base64,PHN2ZyB4bWxucz0naHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmcnLz4="
                    .into(),
            sha256: digest(bytes),
        }],
        photos: vec![],
        content: vec![],
    };
    Project::new(
        "Imported studio identity".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap()
    .set_initial_identity(identity)
    .unwrap()
}

#[test]
fn imported_identity_round_trips_archive_backup_journal_and_recovery_with_embedded_images() {
    let temp = Temp::new();
    let original = identity_project();
    let path = temp.0.join("portable-identity.desegnos");
    let file = ProjectFile::acquire(&path).unwrap();
    file.save(&original).unwrap();
    let saved_bytes = fs::read(&path).unwrap();
    let entries = archive_entries(&saved_bytes);
    let notes = std::str::from_utf8(&entries["PROJECT.md"]).unwrap();
    assert!(notes.contains("Reviewed logo"));
    assert!(!notes.contains("data:image/"));
    assert_eq!(file.load().unwrap(), original);
    let journal = Journal::new(&temp.0, original.id).unwrap();
    journal.append(&original).unwrap();
    let mut session = Session::new(original.clone()).unwrap();
    session
        .execute(
            0,
            Command::RenameProject {
                name: "New site, same identity".into(),
            },
            |next| journal.append(next),
        )
        .unwrap();
    session.undo(|next| journal.append(next)).unwrap();
    assert_eq!(
        session.project().imported_identity,
        original.imported_identity
    );
    session.redo(|next| journal.append(next)).unwrap();
    file.save(session.project()).unwrap();
    assert_eq!(
        Journal::recover(journal.path()).unwrap(),
        Some(session.project().clone())
    );
    assert_eq!(file.load().unwrap(), *session.project());
    assert_eq!(
        fs::read(temp.0.join("portable-identity.desegnos.previous")).unwrap(),
        saved_bytes
    );
}

#[test]
fn imported_asset_integrity_is_checked_even_when_archive_hashes_are_recomputed() {
    let temp = Temp::new();
    let project = identity_project();
    let path = temp.0.join("asset-integrity.desegnos");
    let file = ProjectFile::acquire(&path).unwrap();
    file.save(&project).unwrap();
    let mut entries = archive_entries(&fs::read(&path).unwrap());
    let mut model: serde_json::Value = serde_json::from_slice(&entries["project.json"]).unwrap();
    model["importedIdentity"]["assets"][0]["sha256"] = "0".repeat(64).into();
    entries.insert("project.json".into(), serde_json::to_vec(&model).unwrap());
    update_manifest_hashes(&mut entries);
    write_entries(&path, &entries);
    let before = fs::read(&path).unwrap();
    assert!(file.load().is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn typed_source_with_found_content_round_trips_journal_archive_and_export_without_metadata_leaks() {
    let temp = Temp::new();
    let original = project();
    let id = original.home_page_id;
    let mut session = Session::new(original.clone()).unwrap();
    let journal = Journal::new(&temp.0, original.id).unwrap();
    journal.append(&original).unwrap();
    let html = "<main>\n<h1 data-desegnos-found='https://example.com/history'>Authored 💡</h1>\n<a href='https://example.com/history'>Photo credit</a>\n</main>";
    session
        .edit_page_source(
            original.revision,
            id,
            &PageSourceEdit {
                html: Some(html.into()),
                css: Some("h1 {color: navy;}".into()),
                javascript: Some("document.querySelector('h1').title = 'Local';".into()),
            },
            |next| journal.append(next),
        )
        .unwrap();
    let accepted = session.project().clone();
    let path = temp.0.join("typed.desegnos");
    let file = ProjectFile::acquire(&path).unwrap();
    file.save(&accepted).unwrap();
    assert_eq!(file.load().unwrap(), accepted);
    assert_eq!(
        Journal::recover(journal.path()).unwrap(),
        Some(accepted.clone())
    );
    assert_eq!(
        without_editor_ids(&accepted.pages[0].content.as_ref().unwrap().html),
        html
    );
    let export = compile_website(&accepted).unwrap();
    let exported = String::from_utf8(export.files["index.html"].clone()).unwrap();
    assert!(!exported.contains("data-desegnos-found"));
    assert!(!exported.contains(ELEMENT_ID_ATTRIBUTE));
    assert!(exported.contains("Photo credit"));
    session.undo(|next| journal.append(next)).unwrap();
    assert_eq!(session.project().pages, original.pages);
    session.redo(|next| journal.append(next)).unwrap();
    assert_eq!(session.project().pages, accepted.pages);
}

#[test]
fn deliberately_cleared_html_round_trips_without_fabricating_content() {
    let temp = Temp::new();
    let p = project();
    let id = p.home_page_id;
    let mut session = Session::new(p).unwrap();
    let original = session.project().clone();
    session
        .edit_page_source(
            0,
            id,
            &PageSourceEdit {
                html: Some("<main>Before clearing</main>".into()),
                ..Default::default()
            },
            |_| Ok(()),
        )
        .unwrap();
    let before = session.project().clone();
    session
        .edit_page_source(
            before.revision,
            id,
            &PageSourceEdit {
                html: Some(String::new()),
                ..Default::default()
            },
            |_| Ok(()),
        )
        .unwrap();
    let empty = session.project().clone();
    assert_eq!(empty.pages[0].content.as_ref().unwrap().html, "");
    let file = ProjectFile::acquire(&temp.0.join("cleared.desegnos")).unwrap();
    file.save(&empty).unwrap();
    assert_eq!(file.load().unwrap(), empty);
    let journal = Journal::new(&temp.0, empty.id).unwrap();
    journal.append(&empty).unwrap();
    assert_eq!(
        Journal::recover(journal.path()).unwrap(),
        Some(empty.clone())
    );
    session.undo(|_| Ok(())).unwrap();
    // Consecutive page-source edits to one page share their history step.
    assert_eq!(session.project().pages, original.pages);
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, empty.pages);
}

#[test]
fn fractional_request_sizes_round_trip_exactly_through_the_journal_fingerprint() {
    // Regression: zoomed placement can store sizes like 191.98843383789062. Without exact
    // float parsing, reading them back changed the last digit and recovery reported a
    // failed integrity check on an intact journal.
    let raw =
        r#"{"x":12.5,"y":304.0078125,"width":191.98843383789062,"height":191.98843383789062}"#;
    let rect: desegnos_core::Rect = serde_json::from_str(raw).unwrap();
    assert_eq!(serde_json::to_string(&rect).unwrap(), raw);
}

#[test]
fn different_project_versions_are_rejected_without_changing_the_archive() {
    let temp = Temp::new();
    let file = ProjectFile::acquire(&temp.0.join("unsupported.desegnos")).unwrap();
    file.save(&project()).unwrap();
    let original = archive_entries(&fs::read(file.path()).unwrap());
    for version in [SCHEMA_VERSION - 1, SCHEMA_VERSION + 1] {
        let mut entries = original.clone();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        manifest["schemaVersion"] = version.into();
        entries.insert(
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        write_entries(file.path(), &entries);
        let before = fs::read(file.path()).unwrap();
        assert!(file.load().is_err());
        assert_eq!(fs::read(file.path()).unwrap(), before);
    }
}
