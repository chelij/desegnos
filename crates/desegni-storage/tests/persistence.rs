use desegni_core::*;
use desegni_storage::*;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::PathBuf,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("desegni-storage-{}", uuid::Uuid::new_v4()));
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
fn new_and_legacy_filenames_round_trip_without_changing_the_archive_protocol() {
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
            "desegni"
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
    let path = temp.0.join("site.desegni");
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
    let path = temp.0.join("site.desegni");
    let first = ProjectFile::acquire(&path).unwrap();
    assert!(ProjectFile::acquire(&path).is_err());
    drop(first);
    ProjectFile::acquire(&path).unwrap();
}

#[test]
fn second_save_keeps_the_previous_valid_project() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegni");
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
        load_project(&temp.0.join("site.desegni.previous")).unwrap(),
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
    let path = temp.0.join("unsafe.desegni");
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
    let path = temp.0.join("site.desegni");
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
    let path = temp.0.join("site.desegni");
    let first = ProjectFile::acquire(&path).unwrap();
    first.save(&project()).unwrap();
    let alias = temp.0.join("alias.desegni");
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
    let path = temp.0.join("site.desegni");
    let handle = ProjectFile::acquire(&path).unwrap();
    let p = project();
    handle.save(&p).unwrap();
    let unrelated = temp.0.join("unrelated.txt");
    fs::write(&unrelated, b"Keep this actual data").unwrap();
    std::os::unix::fs::symlink(&unrelated, temp.0.join("site.desegni.previous")).unwrap();
    handle
        .save(&p.apply(0, Command::SetGrid { enabled: true }).unwrap())
        .unwrap();
    assert_eq!(fs::read(unrelated).unwrap(), b"Keep this actual data");
    assert_eq!(
        load_project(&temp.0.join("site.desegni.previous")).unwrap(),
        p
    );
}

#[test]
fn a_damaged_existing_archive_is_preserved_when_saving() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegni");
    let handle = ProjectFile::acquire(&path).unwrap();
    let p = project();
    handle.save(&p).unwrap();
    handle
        .save(&p.apply(0, Command::SetGrid { enabled: true }).unwrap())
        .unwrap();
    let backup = fs::read(temp.0.join("site.desegni.previous")).unwrap();
    fs::write(&path, b"damaged archive").unwrap();
    assert!(handle.save(&p).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"damaged archive");
    assert_eq!(
        fs::read(temp.0.join("site.desegni.previous")).unwrap(),
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
    let handle = ProjectFile::acquire(&temp.0.join("site.desegni")).unwrap();
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
    let path = temp.0.join("site.desegni");
    let handle = ProjectFile::acquire(&path).unwrap();
    let p = project();
    handle.save(&p).unwrap();
    fs::create_dir(temp.0.join("site.desegni.previous")).unwrap();
    let next = p.apply(0, Command::SetGrid { enabled: true }).unwrap();
    assert!(handle.save(&next).is_err());
    assert_eq!(handle.load().unwrap(), p);
    assert!(fs::read_dir(&temp.0).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".desegni-")
    }));
}

#[cfg(unix)]
#[test]
fn hard_link_aliases_are_refused_for_writing() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegni");
    let handle = ProjectFile::acquire(&path).unwrap();
    handle.save(&project()).unwrap();
    drop(handle);
    let alias = temp.0.join("alias.desegni");
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
    let path = temp.0.join("site.desegni");
    let handle = ProjectFile::acquire(&path).unwrap();
    handle.save(&p).unwrap();
    handle
        .save(&p.apply(0, Command::SetGrid { enabled: true }).unwrap())
        .unwrap();
    let journal = Journal::new(&temp.0, p.id).unwrap();
    journal.append(&p).unwrap();
    for file in [&path, &temp.0.join("site.desegni.previous"), journal.path()] {
        assert_eq!(fs::metadata(file).unwrap().permissions().mode() & 0o077, 0);
    }
}

const SCHEMA_ONE: &[u8] = include_bytes!("fixtures/schema1-mixed-generation.desegni");
const SCHEMA_TWO: &[u8] = include_bytes!("fixtures/schema2-global-generation.desegni");
const SCHEMA_THREE: &[u8] = include_bytes!("fixtures/schema3-guide-generation.desegni");

fn legacy_entries() -> BTreeMap<String, Vec<u8>> {
    archive_entries(SCHEMA_ONE)
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
fn real_schema_one_archive_migrates_without_losing_original_data_or_changing_source_bytes() {
    assert_eq!(
        digest(SCHEMA_ONE),
        "f6ab68d745746f287d13f55f0835c8135f09513c73a62ccb737c17a6f914e050"
    );
    let temp = Temp::new();
    let path = temp.0.join("legacy.desegni");
    fs::write(&path, SCHEMA_ONE).unwrap();
    let file = ProjectFile::acquire(&path).unwrap();
    let document = file.load_document().unwrap();
    assert!(document.migrated());
    assert_eq!(document.source_schema_version, 1);
    assert_eq!(document.project.schema_version, SCHEMA_VERSION);
    assert_eq!(document.project.generation_mode, GenerationMode::Fuzzy);
    let entries = legacy_entries();
    let original: serde_json::Value = serde_json::from_slice(&entries["project.json"]).unwrap();
    let record = &document.project.extensions[LEGACY_GENERATION_NAMESPACE];
    assert_eq!(record.schema_version, 1);
    let expected_modes: Vec<_> = original["pages"].as_array().unwrap().iter().flat_map(|page| {
        page["regions"].as_array().unwrap().iter().map(move |region| serde_json::json!({"pageId":page["id"],"regionId":region["id"],"mode":region["mode"]}))
    }).collect();
    assert_eq!(
        record.data,
        serde_json::json!({"sourceSchemaVersion":1,"perAreaModes":expected_modes})
    );
    let mut reconstructed = serde_json::to_value(&document.project).unwrap();
    reconstructed["schemaVersion"] = 1.into();
    reconstructed
        .as_object_mut()
        .unwrap()
        .remove("generationMode");
    reconstructed
        .as_object_mut()
        .unwrap()
        .remove("appliedJobIds");
    reconstructed
        .as_object_mut()
        .unwrap()
        .remove("gridSettings");
    reconstructed
        .as_object_mut()
        .unwrap()
        .remove("latestGeneration");
    reconstructed["extensions"]
        .as_object_mut()
        .unwrap()
        .remove(LEGACY_GENERATION_NAMESPACE);
    for (page, old_page) in reconstructed["pages"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(original["pages"].as_array().unwrap())
    {
        page.as_object_mut().unwrap().remove("content");
        for (region, old_region) in page["regions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(old_page["regions"].as_array().unwrap())
        {
            region["mode"] = old_region["mode"].clone();
        }
    }
    assert_eq!(reconstructed, original);
    assert_eq!(
        document.source_project_markdown.as_bytes(),
        entries["PROJECT.md"]
    );
    assert_eq!(fs::read(&path).unwrap(), SCHEMA_ONE);
    file.save(&document.project).unwrap();
    assert_eq!(
        fs::read(temp.0.join("legacy.desegni.previous")).unwrap(),
        SCHEMA_ONE
    );
    let saved = file.load_document().unwrap();
    assert!(!saved.migrated());
    assert_eq!(saved.project, document.project);
}

#[test]
fn migration_preserves_unknown_extensions_even_when_legacy_namespace_is_taken() {
    let temp = Temp::new();
    let path = temp.0.join("extensions.desegni");
    let mut entries = legacy_entries();
    let mut original: serde_json::Value = serde_json::from_slice(&entries["project.json"]).unwrap();
    let preserved = serde_json::json!({"schemaVersion":9,"data":{"owned":"Retain this existing plugin record"}});
    original["extensions"][LEGACY_GENERATION_NAMESPACE] = preserved.clone();
    original["extensions"]["org.example.unrelated"] = preserved.clone();
    entries.insert(
        "project.json".into(),
        serde_json::to_vec(&original).unwrap(),
    );
    update_manifest_hashes(&mut entries);
    write_entries(&path, &entries);
    let migrated = load_project(&path).unwrap();
    for namespace in [LEGACY_GENERATION_NAMESPACE, "org.example.unrelated"] {
        assert_eq!(
            serde_json::to_value(&migrated.extensions[namespace]).unwrap(),
            preserved
        );
    }
    assert_eq!(
        migrated.extensions[&format!("{LEGACY_GENERATION_NAMESPACE}.1")].schema_version,
        1
    );
    assert_eq!(
        migrated.extensions[&format!("{LEGACY_GENERATION_NAMESPACE}.1")].data["perAreaModes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn migration_validates_source_integrity_projection_and_supported_versions_first() {
    for condition in [
        "project-hash",
        "notes-projection",
        "newer-schema",
        "invalid-old-mode",
    ] {
        let temp = Temp::new();
        let path = temp.0.join("damaged.desegni");
        let mut entries = legacy_entries();
        match condition {
            "project-hash" => entries.get_mut("project.json").unwrap().push(b' '),
            "notes-projection" => {
                entries
                    .get_mut("PROJECT.md")
                    .unwrap()
                    .extend_from_slice(b"\nUnaccepted invented decision\n");
                update_manifest_hashes(&mut entries);
            }
            "newer-schema" => {
                let mut manifest: serde_json::Value =
                    serde_json::from_slice(&entries["manifest.json"]).unwrap();
                manifest["schemaVersion"] = (SCHEMA_VERSION + 1).into();
                entries.insert(
                    "manifest.json".into(),
                    serde_json::to_vec(&manifest).unwrap(),
                );
            }
            "invalid-old-mode" => {
                let mut project: serde_json::Value =
                    serde_json::from_slice(&entries["project.json"]).unwrap();
                project["pages"][0]["regions"][0]["mode"] = "invented".into();
                entries.insert("project.json".into(), serde_json::to_vec(&project).unwrap());
                update_manifest_hashes(&mut entries);
            }
            _ => unreachable!(),
        }
        write_entries(&path, &entries);
        let original = fs::read(&path).unwrap();
        assert!(load_project(&path).is_err(), "{condition}");
        assert_eq!(fs::read(&path).unwrap(), original, "{condition}");
    }
}

#[test]
fn version_one_journal_hashes_are_checked_before_migration_and_duplicates_are_rejected() {
    let temp = Temp::new();
    let entries = legacy_entries();
    let source = std::str::from_utf8(&entries["project.json"]).unwrap();
    let original_hash = digest(source.as_bytes());
    let path = temp.0.join("legacy.journal");
    fs::write(
        &path,
        format!("{{\"sha256\":\"{original_hash}\",\"project\":{source}}}\n"),
    )
    .unwrap();
    let document = Journal::recover_document(&path).unwrap().unwrap();
    assert_eq!(document.source_sha256, original_hash);
    assert!(document.migrated());
    assert_eq!(document.project.generation_mode, GenerationMode::Fuzzy);
    assert_eq!(
        document.project.extensions[LEGACY_GENERATION_NAMESPACE].data["perAreaModes"][0]["mode"],
        "strict"
    );
    let duplicate = source.replacen(
        "\"name\":",
        "\"name\":\"Do not accept duplicate source fields\",\"name\":",
        1,
    );
    fs::write(
        &path,
        format!("{{\"sha256\":\"{original_hash}\",\"project\":{duplicate}}}\n"),
    )
    .unwrap();
    assert!(Journal::recover(&path).is_err());
    fs::write(
        &path,
        format!("{{\"sha256\":\"wrong\",\"project\":{source}}}\n"),
    )
    .unwrap();
    assert!(Journal::recover(&path).is_err());
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
    let file = ProjectFile::acquire(&temp.0.join("strict.desegni")).unwrap();
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
fn real_schema_two_archive_preserves_its_global_setting_and_all_data_before_explicit_save() {
    assert_eq!(
        digest(SCHEMA_TWO),
        "f828598377bebee41c6c8a00f884d8888cdfc3f76c49c8a95b7228b8004834b2"
    );
    let temp = Temp::new();
    let path = temp.0.join("schema2.desegni");
    fs::write(&path, SCHEMA_TWO).unwrap();
    let file = ProjectFile::acquire(&path).unwrap();
    let document = file.load_document().unwrap();
    assert!(document.migrated());
    assert_eq!(document.source_schema_version, 2);
    assert_eq!(document.migration_notice(), Some(CONTENT_MIGRATION_NOTICE));
    assert!(!document.migration_notice().unwrap().contains("per-area"));
    assert_eq!(document.project.schema_version, SCHEMA_VERSION);
    assert_eq!(document.project.generation_mode, GenerationMode::Strict);
    assert!(document.project.applied_job_ids.is_empty());
    assert!(
        document
            .project
            .pages
            .iter()
            .all(|page| page.content.is_none())
    );
    let entries = archive_entries(SCHEMA_TWO);
    let original: serde_json::Value = serde_json::from_slice(&entries["project.json"]).unwrap();
    let mut reconstructed = serde_json::to_value(&document.project).unwrap();
    reconstructed["schemaVersion"] = 2.into();
    reconstructed
        .as_object_mut()
        .unwrap()
        .remove("appliedJobIds");
    reconstructed
        .as_object_mut()
        .unwrap()
        .remove("gridSettings");
    reconstructed
        .as_object_mut()
        .unwrap()
        .remove("latestGeneration");
    for page in reconstructed["pages"].as_array_mut().unwrap() {
        page.as_object_mut().unwrap().remove("content");
    }
    assert_eq!(reconstructed, original);
    assert_eq!(
        document.source_project_markdown.as_bytes(),
        entries["PROJECT.md"]
    );
    assert_eq!(fs::read(&path).unwrap(), SCHEMA_TWO);
    file.save(&document.project).unwrap();
    assert_eq!(
        fs::read(temp.0.join("schema2.desegni.previous")).unwrap(),
        SCHEMA_TWO
    );
    let saved = file.load_document().unwrap();
    assert!(!saved.migrated());
    assert_eq!(saved.migration_notice(), None);
    assert_eq!(saved.project, document.project);
}

#[test]
fn schema_two_integrity_notes_and_original_journal_hashes_are_checked_before_conversion() {
    let entries = archive_entries(SCHEMA_TWO);
    for condition in [
        "project-hash",
        "notes-projection",
        "invalid-mode",
        "mixed-schema",
        "unknown-field",
    ] {
        let temp = Temp::new();
        let path = temp.0.join("damaged.desegni");
        let mut changed = entries.clone();
        match condition {
            "project-hash" => changed.get_mut("project.json").unwrap().push(b' '),
            "notes-projection" => {
                changed
                    .get_mut("PROJECT.md")
                    .unwrap()
                    .extend_from_slice(b"\nInvented acceptance\n");
                update_manifest_hashes(&mut changed);
            }
            "mixed-schema" => {
                let mut manifest: serde_json::Value =
                    serde_json::from_slice(&changed["manifest.json"]).unwrap();
                manifest["schemaVersion"] = 1.into();
                changed.insert(
                    "manifest.json".into(),
                    serde_json::to_vec(&manifest).unwrap(),
                );
            }
            "invalid-mode" | "unknown-field" => {
                let mut source: serde_json::Value =
                    serde_json::from_slice(&changed["project.json"]).unwrap();
                if condition == "invalid-mode" {
                    source["generationMode"] = "invented".into();
                } else {
                    source["appliedJobIds"] = serde_json::json!([]);
                }
                changed.insert("project.json".into(), serde_json::to_vec(&source).unwrap());
                update_manifest_hashes(&mut changed);
            }
            _ => unreachable!(),
        }
        write_entries(&path, &changed);
        let original = fs::read(&path).unwrap();
        assert!(load_project_document(&path).is_err(), "{condition}");
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    let temp = Temp::new();
    let source = std::str::from_utf8(&entries["project.json"]).unwrap();
    assert_eq!(
        source.as_bytes(),
        include_bytes!("fixtures/schema2-global-generation.project.json")
    );
    let original_hash = digest(source.as_bytes());
    let path = temp.0.join("schema2.journal");
    let journal_bytes =
        format!("{{\"sha256\":\"{original_hash}\",\"project\":{source}}}\n").into_bytes();
    fs::write(&path, &journal_bytes).unwrap();
    let document = Journal::recover_document(&path).unwrap().unwrap();
    assert_eq!(document.source_sha256, original_hash);
    assert_eq!(document.source_schema_version, 2);
    assert_eq!(document.migration_notice(), Some(CONTENT_MIGRATION_NOTICE));
    assert_eq!(document.project.generation_mode, GenerationMode::Strict);
    assert_eq!(fs::read(&path).unwrap(), journal_bytes);
    let duplicate = source.replacen(
        "\"generationMode\":",
        "\"generationMode\":\"fuzzy\",\"generationMode\":",
        1,
    );
    for (hash, value) in [
        (original_hash.as_str(), duplicate.as_str()),
        ("wrong", source),
    ] {
        fs::write(
            &path,
            format!("{{\"sha256\":\"{hash}\",\"project\":{value}}}\n"),
        )
        .unwrap();
        assert!(Journal::recover(&path).is_err());
    }
}

#[test]
fn real_schema_three_preserves_all_original_data_notes_and_backup_before_explicit_upgrade() {
    assert_eq!(
        digest(SCHEMA_THREE),
        "6b821194721b72f4c1030345984dc7deaa6ac5aae9b997c9ecf753efc844a66c"
    );
    let temp = Temp::new();
    let path = temp.0.join("schema3.desegni");
    fs::write(&path, SCHEMA_THREE).unwrap();
    let file = ProjectFile::acquire(&path).unwrap();
    let document = file.load_document().unwrap();
    assert!(document.migrated());
    assert_eq!(document.source_schema_version, 3);
    assert_eq!(document.migration_notice(), Some(CANVAS_MIGRATION_NOTICE));
    assert_eq!(document.project.schema_version, SCHEMA_VERSION);
    assert!(
        !document.project.show_grid,
        "the saved user's grid choice is retained"
    );
    assert_eq!(document.project.grid_settings, GridSettings::default());
    assert!(document.project.latest_generation.is_none());
    assert!(!document.project.applied_job_ids.is_empty());
    assert!(
        document
            .project
            .pages
            .iter()
            .any(|page| page.content.is_some())
    );
    let entries = archive_entries(SCHEMA_THREE);
    assert_eq!(
        entries["project.json"],
        include_bytes!("fixtures/schema3-guide-generation.project.json")
    );
    let original: serde_json::Value = serde_json::from_slice(&entries["project.json"]).unwrap();
    let mut reconstructed = serde_json::to_value(&document.project).unwrap();
    reconstructed["schemaVersion"] = 3.into();
    reconstructed
        .as_object_mut()
        .unwrap()
        .remove("gridSettings");
    reconstructed
        .as_object_mut()
        .unwrap()
        .remove("latestGeneration");
    assert_eq!(reconstructed, original);
    assert_eq!(
        document.source_project_markdown.as_bytes(),
        entries["PROJECT.md"]
    );
    assert_eq!(fs::read(&path).unwrap(), SCHEMA_THREE);
    let frozen: RequirementsSnapshot = serde_json::from_slice(include_bytes!(
        "fixtures/schema3-guide-generation.requirements.json"
    ))
    .unwrap();
    assert!(frozen.dossier_context.is_none());
    assert!(frozen.markdown.contains("Requirements format: 3"));
    frozen.validate_current(&document.project).unwrap();
    file.save(&document.project).unwrap();
    assert_eq!(
        fs::read(temp.0.join("schema3.desegni.previous")).unwrap(),
        SCHEMA_THREE
    );
    let reopened = file.load_document().unwrap();
    assert!(!reopened.migrated());
    assert_eq!(reopened.project, document.project);
    frozen.validate_current(&reopened.project).unwrap();
    let new_request = reopened.project.prepare_requirements().unwrap();
    assert!(new_request.markdown.contains("Requirements format: 9"));
    new_request.validate_current(&reopened.project).unwrap();
}

#[test]
fn schema_three_integrity_notes_unknown_fields_and_recovery_hashes_are_verified_before_migration() {
    let entries = archive_entries(SCHEMA_THREE);
    for condition in [
        "project-hash",
        "notes-projection",
        "mixed-schema",
        "unknown-field",
        "missing-source",
    ] {
        let temp = Temp::new();
        let path = temp.0.join("damaged.desegni");
        let mut changed = entries.clone();
        match condition {
            "project-hash" => changed.get_mut("project.json").unwrap().push(b' '),
            "notes-projection" => {
                changed
                    .get_mut("PROJECT.md")
                    .unwrap()
                    .extend_from_slice(b"\nInvented generation authority\n");
                update_manifest_hashes(&mut changed);
            }
            "mixed-schema" => {
                let mut manifest: serde_json::Value =
                    serde_json::from_slice(&changed["manifest.json"]).unwrap();
                manifest["schemaVersion"] = 4.into();
                changed.insert(
                    "manifest.json".into(),
                    serde_json::to_vec(&manifest).unwrap(),
                );
            }
            "unknown-field" | "missing-source" => {
                let mut source: serde_json::Value =
                    serde_json::from_slice(&changed["project.json"]).unwrap();
                if condition == "unknown-field" {
                    source["gridSettings"] = serde_json::json!({"spacing": 8, "unit": "pixels", "snap": true, "rulers": true});
                } else {
                    source["pages"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("content");
                }
                changed.insert("project.json".into(), serde_json::to_vec(&source).unwrap());
                update_manifest_hashes(&mut changed);
            }
            _ => unreachable!(),
        }
        write_entries(&path, &changed);
        let original = fs::read(&path).unwrap();
        assert!(load_project_document(&path).is_err(), "{condition}");
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    let temp = Temp::new();
    let source = std::str::from_utf8(&entries["project.json"]).unwrap();
    let original_hash = digest(source.as_bytes());
    let path = temp.0.join("schema3.journal");
    let bytes = format!("{{\"sha256\":\"{original_hash}\",\"project\":{source}}}\n").into_bytes();
    fs::write(&path, &bytes).unwrap();
    let recovered = Journal::recover_document(&path).unwrap().unwrap();
    assert_eq!(recovered.source_sha256, original_hash);
    assert_eq!(recovered.source_schema_version, 3);
    assert!(!recovered.project.show_grid);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let duplicate = source.replacen("\"showGrid\":", "\"showGrid\":true,\"showGrid\":", 1);
    for (hash, value) in [
        (original_hash.as_str(), duplicate.as_str()),
        ("wrong", source),
    ] {
        fs::write(
            &path,
            format!("{{\"sha256\":\"{hash}\",\"project\":{value}}}\n"),
        )
        .unwrap();
        assert!(Journal::recover(&path).is_err());
    }
    let mut grid_on = entries;
    let mut original: serde_json::Value = serde_json::from_slice(&grid_on["project.json"]).unwrap();
    original["showGrid"] = true.into();
    grid_on.insert(
        "project.json".into(),
        serde_json::to_vec(&original).unwrap(),
    );
    update_manifest_hashes(&mut grid_on);
    let path = temp.0.join("grid-on.desegni");
    write_entries(&path, &grid_on);
    assert!(load_project(&path).unwrap().show_grid);
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
    let path = temp.0.join("dossier.desegni");
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
        load_project(&temp.0.join("dossier.desegni.previous")).unwrap(),
        draft
    );
}

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
                    "<details data-desegnos-region=\"{}\"><summary>About this project</summary><p>Saved generated source.</p></details>",
                    p.pages[0].regions[0].id
                ),
                css: "details { padding: 1rem; }".into(),
                javascript:
                    "document.querySelector('details').addEventListener('toggle', () => {});".into(),
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
    let path = temp.0.join("generated.desegni");
    let file = ProjectFile::acquire(&path).unwrap();
    file.save(&applied).unwrap();
    let reopened = file.load().unwrap();
    assert_eq!(reopened, applied);
    assert_eq!(
        reopened.pages[0].content,
        Some(candidate.pages[0].content.clone())
    );
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
        load_project(&temp.0.join("generated.desegni.previous")).unwrap(),
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
fn schema_four_upgrade_preserves_newer_pending_requests_and_exact_original_backup() {
    for newer_edit in [false, true] {
        let temp = Temp::new();
        let (project, snapshot, candidate) = generation_fixture();
        let mut project = project
            .apply_generation(&snapshot, &candidate, &[])
            .unwrap();
        for page in &mut project.pages {
            for region in &mut page.regions {
                region.completed = false;
            }
        }
        if newer_edit {
            let area = &project.pages[0].regions[0];
            project = project
                .apply(
                    project.revision,
                    Command::UpdateRegion {
                        page_id: project.home_page_id,
                        region_id: area.id,
                        rect: area.rect,
                        prompt: "A newer request that has never been generated".into(),
                    },
                )
                .unwrap();
        }
        let path = temp.0.join("version-four.desegni");
        let file = ProjectFile::acquire(&path).unwrap();
        file.save(&project).unwrap();
        let mut entries = archive_entries(&fs::read(&path).unwrap());
        let mut source = project.clone();
        source.schema_version = 4;
        entries.insert("project.json".into(), serde_json::to_vec(&source).unwrap());
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        manifest["schemaVersion"] = 4.into();
        entries.insert(
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        update_manifest_hashes(&mut entries);
        write_entries(&path, &entries);
        let original = fs::read(&path).unwrap();
        let document = file.load_document().unwrap();
        assert_eq!(document.source_schema_version, 4);
        assert_eq!(document.migration_notice(), Some(ELEMENT_MIGRATION_NOTICE));
        assert_eq!(document.project.pages[0].regions[0].completed, !newer_edit);
        assert_eq!(
            document.project.pages[0].regions[0].prompt,
            project.pages[0].regions[0].prompt
        );
        if newer_edit {
            assert_eq!(document.project.pending_count(), 1);
            document.project.prepare_requirements().unwrap();
        }
        assert_eq!(fs::read(&path).unwrap(), original);
        file.save(&document.project).unwrap();
        assert_eq!(
            fs::read(temp.0.join("version-four.desegni.previous")).unwrap(),
            original
        );
        assert_eq!(file.load_document().unwrap().project, document.project);
    }
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
    let file = ProjectFile::acquire(&temp.0.join("direct.desegni")).unwrap();
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
    let path = temp.0.join("portable-identity.desegni");
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
        fs::read(temp.0.join("portable-identity.desegni.previous")).unwrap(),
        saved_bytes
    );
}

#[test]
fn schema_five_integrity_notes_frozen_requirements_and_exact_backup_are_preserved_before_upgrade() {
    let temp = Temp::new();
    let (mut project, _, _) = generation_fixture();
    // Keep genuine element request metadata, which distinguishes schema 5 from 4.
    project.pages[0].content = Some(SiteContent {
        html: "<h1>Existing site</h1>".into(),
        css: String::new(),
        javascript: String::new(),
    });
    let target = ElementTarget {
        selector: "body > h1:nth-of-type(1)".into(),
        tag: "h1".into(),
        label: "Existing site".into(),
        source_sha256: project.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    };
    project.pages[0].regions[0].element = Some(target);
    let frozen = project.prepare_requirements().unwrap();
    assert!(frozen.markdown.contains("Requirements format: 9"));
    let path = temp.0.join("schema-five.desegni");
    let file = ProjectFile::acquire(&path).unwrap();
    file.save(&project).unwrap();
    let mut entries = archive_entries(&fs::read(&path).unwrap());
    let mut source = project.clone();
    source.schema_version = 5;
    let source_bytes = serde_json::to_vec(&source).unwrap();
    entries.insert("project.json".into(), source_bytes.clone());
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&entries["manifest.json"]).unwrap();
    manifest["schemaVersion"] = 5.into();
    entries.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    update_manifest_hashes(&mut entries);
    write_entries(&path, &entries);
    let original_bytes = fs::read(&path).unwrap();
    let document = file.load_document().unwrap();
    assert_eq!(document.source_schema_version, 5);
    assert_eq!(document.migration_notice(), Some(IDENTITY_MIGRATION_NOTICE));
    assert_eq!(document.source_sha256, digest(&source_bytes));
    assert_eq!(
        document.source_project_markdown.as_bytes(),
        entries["PROJECT.md"]
    );
    assert_eq!(document.project, project);
    assert!(document.project.imported_identity.is_none());
    frozen.validate_current(&document.project).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original_bytes);
    file.save(&document.project).unwrap();
    assert_eq!(
        fs::read(temp.0.join("schema-five.desegni.previous")).unwrap(),
        original_bytes
    );
    assert_eq!(
        file.load_document().unwrap().source_schema_version,
        SCHEMA_VERSION
    );

    let recovery_path = temp.0.join("schema-five.journal");
    let source_text = std::str::from_utf8(&source_bytes).unwrap();
    let recovery_bytes = format!(
        "{{\"sha256\":\"{}\",\"project\":{source_text}}}\n",
        digest(&source_bytes)
    );
    fs::write(&recovery_path, &recovery_bytes).unwrap();
    let recovered = Journal::recover_document(&recovery_path).unwrap().unwrap();
    assert_eq!(recovered.source_schema_version, 5);
    assert_eq!(recovered.project, project);
    assert_eq!(fs::read(&recovery_path).unwrap(), recovery_bytes.as_bytes());

    for case in ["notes", "legacyIdentity", "hash"] {
        let mut altered = entries.clone();
        match case {
            "notes" => {
                altered
                    .get_mut("PROJECT.md")
                    .unwrap()
                    .extend_from_slice(b"\nForged accepted identity\n");
                update_manifest_hashes(&mut altered);
            }
            "legacyIdentity" => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&altered["project.json"]).unwrap();
                value["importedIdentity"] = serde_json::Value::Null;
                altered.insert("project.json".into(), serde_json::to_vec(&value).unwrap());
                update_manifest_hashes(&mut altered);
            }
            "hash" => {
                altered.get_mut("project.json").unwrap().push(b' ');
            }
            _ => unreachable!(),
        }
        let altered_path = temp.0.join(format!("invalid-{case}.desegni"));
        write_entries(&altered_path, &altered);
        let before = fs::read(&altered_path).unwrap();
        assert!(load_project_document(&altered_path).is_err(), "{case}");
        assert_eq!(fs::read(&altered_path).unwrap(), before);
    }
}

#[test]
fn imported_asset_integrity_is_checked_even_when_archive_hashes_are_recomputed() {
    let temp = Temp::new();
    let project = identity_project();
    let path = temp.0.join("asset-integrity.desegni");
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
    assert_eq!(accepted.pages[0].content.as_ref().unwrap().html, html);
    let export = compile_website(&accepted).unwrap();
    let exported = String::from_utf8(export.files["index.html"].clone()).unwrap();
    assert!(!exported.contains("data-desegnos-found"));
    assert!(exported.contains("Photo credit"));
    session.undo(|next| journal.append(next)).unwrap();
    assert_eq!(session.project().pages, original.pages);
    session.redo(|next| journal.append(next)).unwrap();
    assert_eq!(session.project().pages, accepted.pages);
}
