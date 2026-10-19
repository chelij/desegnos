use desegni_core::{Appearance, Command, Project, Viewport};
use desegni_storage::{ProjectFile, SaveFailureKind, load_project};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("desegnos-safe-save-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn project(name: &str) -> Project {
    Project::new(
        name.into(),
        Viewport {
            width: 1280,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap()
}
fn external_write(directory: &Path, path: &Path, project: &Project) {
    let temporary = directory.join(format!("external-{}.desegnos", Uuid::new_v4()));
    let file = ProjectFile::acquire(&temporary).unwrap();
    file.save(project).unwrap();
    fs::copy(&temporary, path).unwrap();
}

#[test]
fn a_matching_opened_stamp_saves_and_advances_only_after_success() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let original = project("Original");
    {
        let file = ProjectFile::acquire(&path).unwrap();
        assert!(file.stamp().unwrap().is_none());
        file.save_checked(&original).unwrap();
    }
    let file = ProjectFile::acquire(&path).unwrap();
    assert!(file.stamp().unwrap().is_none());
    let opened = file.load_document().unwrap();
    assert_eq!(opened.project, original);
    let before = file.stamp().unwrap().unwrap();
    let next = original
        .apply(
            0,
            Command::RenameProject {
                name: "Saved edit".into(),
            },
        )
        .unwrap();
    file.save_checked(&next).unwrap();
    let after = file.stamp().unwrap().unwrap();
    assert_eq!(after.project_id, before.project_id);
    assert_eq!(after.revision, 1);
    assert_ne!(after.manifest_sha256, before.manifest_sha256);
    assert_eq!(load_project(&path).unwrap(), next);
    assert_eq!(
        load_project(&path.with_file_name("site.desegnos.previous")).unwrap(),
        original
    );
}

#[test]
fn same_revision_external_change_is_detected_and_never_rebaselined_by_verification() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let original = project("Original");
    let file = ProjectFile::acquire(&path).unwrap();
    file.save(&original).unwrap();
    let recorded = file.stamp().unwrap();
    let mut divergent = original.clone();
    divergent.name = "Different bytes, same revision".into();
    external_write(&temp.0, &path, &divergent);
    let actual_bytes = fs::read(&path).unwrap();
    assert!(file.saved_document().is_err());
    assert_eq!(file.stamp().unwrap(), recorded);
    let failure = file.save_checked(&original).unwrap_err();
    assert_eq!(failure.kind, SaveFailureKind::Changed);
    assert_eq!(failure.expected, recorded);
    assert_ne!(failure.actual, recorded);
    assert_eq!(fs::read(&path).unwrap(), actual_bytes);
    assert!(!path.with_file_name("site.desegnos.previous").exists());
}

#[test]
fn explicit_replace_backs_up_the_observed_change_but_a_second_change_expires_it() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let original = project("Original");
    let file = ProjectFile::acquire(&path).unwrap();
    file.save(&original).unwrap();
    let changed = original
        .apply(
            0,
            Command::RenameProject {
                name: "Other window".into(),
            },
        )
        .unwrap();
    external_write(&temp.0, &path, &changed);
    let observed = file.save_checked(&original).unwrap_err().actual.unwrap();
    let changed_again = changed
        .apply(
            1,
            Command::RenameProject {
                name: "Other window again".into(),
            },
        )
        .unwrap();
    external_write(&temp.0, &path, &changed_again);
    let external_bytes = fs::read(&path).unwrap();
    let failure = file
        .save_replacing(&original, &observed, false)
        .unwrap_err();
    assert_eq!(failure.kind, SaveFailureKind::Changed);
    assert_eq!(fs::read(&path).unwrap(), external_bytes);
    let current = failure.actual.unwrap();
    file.save_replacing(&original, &current, false).unwrap();
    assert_eq!(load_project(&path).unwrap(), original);
    assert_eq!(
        fs::read(path.with_file_name("site.desegnos.previous")).unwrap(),
        external_bytes
    );
}

#[test]
fn different_projects_require_explicit_permission_even_if_the_stamp_matches() {
    let temp = Temp::new();
    let path = temp.0.join("other.desegnos");
    let other = project("Other project");
    external_write(&temp.0, &path, &other);
    let bytes = fs::read(&path).unwrap();
    let file = ProjectFile::acquire(&path).unwrap();
    let incoming = project("My project");
    let failure = file.save_checked(&incoming).unwrap_err();
    assert_eq!(failure.kind, SaveFailureKind::DifferentProject);
    let observed = failure.actual.unwrap();
    assert_eq!(
        file.save_replacing(&incoming, &observed, false)
            .unwrap_err()
            .kind,
        SaveFailureKind::DifferentProject
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    file.save_replacing(&incoming, &observed, true).unwrap();
    assert_eq!(load_project(&path).unwrap(), incoming);
    assert_eq!(
        fs::read(path.with_file_name("other.desegnos.previous")).unwrap(),
        bytes
    );
}

#[test]
fn missing_or_non_project_files_are_never_replaced_by_stale_permission() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let original = project("Original");
    let file = ProjectFile::acquire(&path).unwrap();
    file.save(&original).unwrap();
    let observed = file.stamp().unwrap().unwrap();
    fs::remove_file(&path).unwrap();
    assert_eq!(
        file.save_checked(&original).unwrap_err().kind,
        SaveFailureKind::Unreadable
    );
    assert!(!path.exists());
    fs::write(&path, b"This is a document belonging to someone else").unwrap();
    assert_eq!(
        file.save_replacing(&original, &observed, true)
            .unwrap_err()
            .kind,
        SaveFailureKind::Unreadable
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        b"This is a document belonging to someone else"
    );
}

#[test]
fn acquiring_an_existing_path_does_not_implicitly_authorize_overwrite() {
    let temp = Temp::new();
    let path = temp.0.join("site.desegnos");
    let original = project("Original");
    external_write(&temp.0, &path, &original);
    let file = ProjectFile::acquire(&path).unwrap();
    assert_eq!(
        file.save_checked(&original).unwrap_err().kind,
        SaveFailureKind::Changed
    );
    file.load_document().unwrap();
    file.save_checked(&original).unwrap();
}
