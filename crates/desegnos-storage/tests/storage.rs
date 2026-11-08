//! Project parts and the `.desegnos` database round trip.
use desegnos_core::{
    Appearance, BoundaryKind, Command, ElementKind, PageSize, Project, Rect, Session, Viewport,
    digest, empty_element,
};
use desegnos_storage::project_db::now_millis;
use desegnos_storage::{ProjectDatabase, join_project, split_project};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("desegnos-{label}-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn project() -> Project {
    Project::new(
        "Storage".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap()
}

fn apply(project: &Project, command: Command) -> Project {
    project.apply(project.revision, command).unwrap()
}

fn project_with_element() -> Project {
    let project = project();
    let page = project.home_page_id;
    let mut element = empty_element(
        ElementKind::Text,
        Rect {
            x: 20.0,
            y: 30.0,
            width: 200.0,
            height: 40.0,
        },
    );
    element.text = "Saved text".into();
    apply(
        &project,
        Command::InsertElements {
            page_id: page,
            parent_id: None,
            index: 0,
            elements: vec![element],
            picture_data_uri: None,
            size_id: None,
        },
    )
}

#[test]
fn saved_linked_picture_reopens_without_pruning_its_shared_image_bytes() {
    let directory = TempDir::new("linked-picture");
    let path = directory.path("linked.desegnos");
    let mut project = project();
    let (picture, uri) = Project::prepare_picture(
        "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=", 100,
    ).unwrap();
    let mut master = empty_element(
        ElementKind::Image,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
    );
    master.image = Some(picture);
    let definition_id = Uuid::new_v4();
    project.linked.insert(
        definition_id,
        desegnos_core::LinkedDefinition {
            id: definition_id,
            name: "Shared logo".into(),
            master,
        },
    );
    let mut copy = empty_element(
        ElementKind::Image,
        Rect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 100.0,
        },
    );
    copy.link = Some(definition_id);
    project.pages[0].elements.push(copy);
    project.pictures.insert(picture, uri.into());
    project.validate().unwrap();
    let database = ProjectDatabase::create(&path, &project, now_millis()).unwrap();
    drop(database);
    let loaded = desegnos_storage::load_project(&path).unwrap();
    let session = Session::new(loaded).unwrap();
    assert_eq!(
        session.project().picture_bytes(picture),
        project.picture_bytes(picture)
    );
    assert_eq!(session.project().linked, project.linked);
}

#[test]
fn generated_attachment_picture_survives_acceptance_save_reopen_and_export() {
    use desegnos_core::{
        AttachmentKind, GenerationCandidate, PendingRequest, RequestAttachment, RequestTarget,
    };
    use serde_json::{Value, json};
    let directory = TempDir::new("generated-attachment");
    let path = directory.path("picture.desegnos");
    let uri = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    let mut project = project_with_element();
    project.pages[0].elements[0].kind = ElementKind::Box;
    let page = project.pages[0].id;
    let element = project.pages[0].elements[0].id;
    let request_id = Uuid::new_v4();
    project.requests.push(PendingRequest {
        boundary_retry_order: Vec::new(),
        id: request_id,
        text: "Use my attached picture".into(),
        attachments: vec![RequestAttachment {
            id: "attached1".into(),
            name: "my-picture.png".into(),
            kind: AttachmentKind::Image,
            data_uri: Some(uri.into()),
            text: None,
            url: None,
        }],
        targets: vec![RequestTarget::Element {
            page,
            size: None,
            id: element,
        }],
        area: None,
        requested_at: "2026-10-31T00:00:00Z".into(),
    });
    let snapshot = project.prepare_requirements().unwrap();
    let packet = snapshot
        .markdown
        .split("```json\n")
        .skip(1)
        .filter_map(|block| serde_json::from_str::<Value>(block.split("\n```").next()?).ok())
        .find(|block| block.get("requests").is_some())
        .unwrap();
    let picture_id: Uuid =
        serde_json::from_value(packet["requests"][0]["attachments"][0]["pictureId"].clone())
            .unwrap();
    assert!(
        !snapshot.markdown.contains(uri),
        "original image bytes stay local"
    );
    assert!(
        project.pictures.is_empty(),
        "preparation does not edit the live project"
    );
    let reply = json!({
        "jobId": snapshot.job_id, "requirementsSha256": snapshot.sha256, "summary": "Placed the attachment",
        "requests": [{"requestId": request_id, "elementId": element, "status": "implemented", "explanation": "Used the original picture",
            "operations": [{"kind":"replaceChildren", "children":[{
                "kind":"image", "rect":{"x":0,"y":0,"width":20,"height":20}, "image":picture_id
            }]}]}],
        "images":[], "pointerClassifications":[], "decisionProposals":[], "criticism":[]
    });
    let mut unknown = reply.clone();
    unknown["requests"][0]["operations"][0]["children"][0]["image"] = json!(Uuid::new_v4());
    assert!(
        GenerationCandidate::from_json(&unknown.to_string())
            .unwrap()
            .validate(&snapshot, &project)
            .is_err()
    );
    let reply = GenerationCandidate::from_json(&reply.to_string()).unwrap();
    reply.validate(&snapshot, &project).unwrap();
    assert!(project.pictures.is_empty(), "validation is read-only");
    let mut session = Session::new(project.clone()).unwrap();
    session
        .apply_generation(&snapshot, &reply, &[], &Default::default())
        .unwrap();
    let applied = session.project().clone();
    assert_eq!(applied.pictures[&picture_id].as_ref(), uri);
    assert_eq!(
        applied.pages[0].elements[0].children[0].image,
        Some(picture_id)
    );
    assert!(applied.requests.is_empty());
    session.undo().unwrap();
    assert_eq!(session.project().pictures, project.pictures);
    assert_eq!(session.project().requests, project.requests);
    drop(ProjectDatabase::create(&path, &applied, now_millis()).unwrap());
    let reopened = desegnos_storage::load_project(&path).unwrap();
    assert_eq!(reopened.pictures[&picture_id].as_ref(), uri);
    let bytes = reopened.picture_bytes(picture_id).unwrap().1;
    let bundle = desegnos_core::direct_export_bundle(&reopened, &[]).unwrap();
    assert!(
        bundle.files.values().any(|file| file == &bytes),
        "export includes the original attachment"
    );
}

#[test]
fn screen_project_keeps_sizes_content_history_and_web_export() {
    let directory = TempDir::new("legacy-screen");
    let path = directory.path("legacy.desegnos");
    let mut old = project_with_element();
    old.template = desegnos_core::CanvasTemplate::AppScreen;
    old.pages[0].name = "Screen 1".into();
    old.pages[0].output = desegnos_core::OutputType::Webpage;
    old.pages[0].sizes[0].kind = BoundaryKind::Screen;
    old.pages[0].sizes[0].width = 390;
    old.pages[0].sizes[0].height = 844;
    let json = serde_json::to_string(&old).unwrap();
    assert!(json.contains("\"output\":\"webpage\""));
    assert!(json.contains("\"kind\":\"screen\""));
    let old: Project = serde_json::from_str(&json).unwrap();
    let expected_parts = split_project(&old).unwrap();
    let database = ProjectDatabase::create(&path, &old, now_millis() - 60_000).unwrap();
    let original_version = database.load().unwrap().version;
    drop(database);

    let database = ProjectDatabase::open(&path).unwrap();
    let loaded = database.load().unwrap().project;
    assert_eq!(loaded, old);
    assert_eq!(split_project(&loaded).unwrap(), expected_parts);
    let legacy_export = desegnos_core::direct_export_bundle(&loaded, &[]).unwrap();
    assert!(legacy_export.files.contains_key("index.html"));
    let mut page = loaded.clone();
    page.pages[0].output = desegnos_core::OutputType::Webpage;
    let page_export = desegnos_core::direct_export_bundle(&page, &[]).unwrap();
    assert_eq!(legacy_export.files, page_export.files);
    assert!(
        page_export
            .files
            .values()
            .any(|bytes| String::from_utf8_lossy(bytes).contains("overflow:auto"))
    );

    let mut edited = loaded.clone();
    edited.pages[0].elements[0].text = "Edited in the unified Page editor".into();
    database.save(&edited, &[], Some("Edit page")).unwrap();
    drop(database);
    let database = ProjectDatabase::open(&path).unwrap();
    assert_eq!(database.load().unwrap().project, edited);
    assert_eq!(database.restore(original_version).unwrap(), old);
}

#[test]
fn a_blank_untitled_canvas_saves_and_its_first_boundary_becomes_home() {
    let blank = Project::blank("Untitled project".into(), Appearance::Light).unwrap();
    assert!(blank.pages.is_empty());
    let parts = split_project(&blank).unwrap();
    assert_eq!(join_project(&parts).unwrap(), blank);

    let directory = TempDir::new("blank");
    let path = directory.path("blank.desegnos");
    {
        let database = ProjectDatabase::create(&path, &blank, now_millis()).unwrap();
        drop(database);
    }
    let loaded = ProjectDatabase::open(&path).unwrap().load().unwrap();
    assert_eq!(loaded.project, blank, "an empty canvas keeps its file");

    let first = apply(
        &blank,
        Command::AddCanvasPage {
            name: "Home".into(),
            path: "/".into(),
            size: PageSize {
                id: Uuid::new_v4(),
                label: "Desktop".into(),
                width: 1440,
                height: 900,
                kind: BoundaryKind::Page,
                x: 0.0,
                y: 0.0,
            },
            moves: vec![],
            reference: vec![],
        },
    );
    assert_eq!(first.home_page_id, first.pages[0].id);
    assert_eq!(first.pages[0].path, "/");
    let parts = split_project(&first).unwrap();
    assert_eq!(join_project(&parts).unwrap(), first);
}

#[test]
fn split_and_join_round_trip_and_unchanged_parts_keep_their_hash() {
    let project = project_with_element();
    let parts = split_project(&project).unwrap();
    assert!(parts.contains_key("settings"));
    assert!(parts.contains_key("reference"));
    assert!(parts.contains_key("pointers"));
    assert!(parts.contains_key("brief"));
    assert!(parts.contains_key("identity"));
    assert!(parts.contains_key("dossier"));
    assert!(
        parts.keys().any(|name| name.starts_with("boundary/")),
        "a boundary part is written"
    );
    let joined = join_project(&parts).unwrap();
    assert_eq!(joined, project);

    let again = split_project(&joined).unwrap();
    assert_eq!(parts, again, "equal content hashes equally");

    // Changing the project name changes only the settings part.
    let mut renamed = project.clone();
    renamed.name = "Renamed".into();
    let renamed_parts = split_project(&renamed).unwrap();
    let boundary = parts
        .keys()
        .find(|name| name.starts_with("boundary/"))
        .unwrap();
    assert_eq!(
        digest(parts[boundary].bytes()),
        digest(renamed_parts[boundary].bytes())
    );
    assert_ne!(
        digest(parts["settings"].bytes()),
        digest(renamed_parts["settings"].bytes())
    );

    // Changing one boundary changes its part and leaves settings alone.
    let mut edited = project.clone();
    edited.pages[0].elements[0].text = "Edited text".into();
    let edited_parts = split_project(&edited).unwrap();
    assert_ne!(
        digest(parts[boundary].bytes()),
        digest(edited_parts[boundary].bytes())
    );
    assert_eq!(
        digest(parts["settings"].bytes()),
        digest(edited_parts["settings"].bytes())
    );
}

fn create_database(directory: &TempDir, project: &Project) -> ProjectDatabase {
    ProjectDatabase::create(
        &directory.path("project.desegnos"),
        project,
        now_millis() - 60_000,
    )
    .unwrap()
}

#[test]
fn save_amends_recent_versions_and_restore_creates_a_new_one() {
    let directory = TempDir::new("save");
    let project = project_with_element();
    let database = create_database(&directory, &project);
    let created = database.load().unwrap();
    assert_eq!(created.project, project);
    assert_eq!(created.branch, desegnos_storage::DEFAULT_BRANCH);
    assert_eq!(database.history().unwrap().versions.len(), 1);

    let mut one = project.clone();
    one.name = "One".into();
    let saved_one = database.save(&one, &[], Some("First save")).unwrap();
    assert_ne!(saved_one.id, created.version);
    assert_eq!(database.load().unwrap().project.name, "One");

    let mut two = project.clone();
    two.name = "Two".into();
    let saved_two = database.save(&two, &[], Some("Second save")).unwrap();
    assert_eq!(
        saved_two.id, saved_one.id,
        "a save seconds later amends the previous version"
    );
    assert!(saved_two.label.contains("First save"));
    assert!(saved_two.label.contains("Second save"));
    let history = database.history().unwrap();
    assert_eq!(history.versions.len(), 2);
    assert_eq!(history.versions[0].id, saved_one.id);
    assert!(history.versions[0].head);

    let restored = database.restore(created.version).unwrap();
    assert_eq!(restored.name, "Storage");
    let after = database.load().unwrap();
    assert_eq!(after.project.name, "Storage");
    let history = database.history().unwrap();
    assert_eq!(history.versions.len(), 3);
    assert_eq!(history.versions[0].label, "Restored a version");
    assert_eq!(history.versions[0].parent, Some(saved_one.id));
}

#[test]
fn restoring_a_version_is_one_undo_step_in_the_session() {
    let project = project_with_element();
    let mut renamed = project.clone();
    renamed.name = "Later".into();
    let mut session = Session::new(renamed).unwrap();
    let before = session.history().position;
    session.restore(project.clone()).unwrap();
    assert_eq!(session.history().position, before + 1);
    assert_eq!(session.project().name, "Storage");
    session.undo().unwrap();
    assert_eq!(session.project().name, "Later");
    session.redo().unwrap();
    assert_eq!(session.project().name, "Storage");
}

#[test]
fn alternatives_keep_independent_heads() {
    let directory = TempDir::new("branches");
    let database = create_database(&directory, &project());
    let mut dark = database.load().unwrap().project;
    dark.name = "Dark".into();
    database.save(&dark, &[], Some("Main dark")).unwrap();

    let branch = database.create_branch("Dark version").unwrap();
    assert_eq!(branch, "Dark version");
    assert_eq!(database.branch().unwrap(), "Dark version");
    let mut darker = database.load().unwrap().project;
    darker.name = "Darker".into();
    database.save(&darker, &[], Some("Darker")).unwrap();

    let main = database
        .switch_branch(desegnos_storage::DEFAULT_BRANCH)
        .unwrap();
    assert_eq!(main.project.name, "Dark");
    assert_eq!(main.branch, desegnos_storage::DEFAULT_BRANCH);
    let alternative = database.switch_branch("Dark version").unwrap();
    assert_eq!(alternative.project.name, "Darker");
    let history = database.history().unwrap();
    assert_eq!(history.branch, "Dark version");
    assert!(
        history
            .branches
            .iter()
            .any(|item| item.name == "Dark version" && item.current)
    );
    assert!(
        history
            .branches
            .iter()
            .any(|item| item.name == "Main" && item.primary)
    );

    assert!(
        database.delete_branch("Main").is_err(),
        "the main branch stays"
    );
    assert!(
        database.delete_branch("Dark version").is_err(),
        "the current branch stays"
    );
    database.switch_branch("Main").unwrap();
    database.delete_branch("Dark version").unwrap();
    assert!(
        database.switch_branch("Dark version").is_err(),
        "a deleted alternative is gone"
    );
}

#[test]
fn autosave_is_detected_as_recovery_and_cleared_by_save() {
    let directory = TempDir::new("autosave");
    let project = project_with_element();
    let database = create_database(&directory, &project);
    assert!(database.autosave_work().unwrap().is_none());

    let mut changed = project.clone();
    changed.name = "Unsaved".into();
    database.autosave(&changed, now_millis()).unwrap();
    let recovery = database.autosave_work().unwrap().expect("unsaved work");
    assert_eq!(recovery.project.name, "Unsaved");
    assert_eq!(database.load().unwrap().project.name, "Storage");

    database
        .save(&changed, &[], Some("Saved after recovery"))
        .unwrap();
    assert!(database.autosave_work().unwrap().is_none());
    assert_eq!(database.load().unwrap().project.name, "Unsaved");
}

#[test]
fn a_copy_has_one_version_and_no_history() {
    let directory = TempDir::new("copy");
    let project = project_with_element();
    let source = create_database(&directory, &project);
    let mut changed = project.clone();
    changed.name = "Copy source".into();
    source.save(&changed, &[], Some("Before copy")).unwrap();

    let copy_path = directory.path("copy.desegnos");
    let copy =
        ProjectDatabase::create(&copy_path, &source.load().unwrap().project, now_millis()).unwrap();
    let history = copy.history().unwrap();
    assert_eq!(history.versions.len(), 1);
    assert_eq!(history.versions[0].label, "Created the project");
    assert_eq!(copy.load().unwrap().project.name, "Copy source");
}

#[test]
fn a_second_writer_is_refused_while_the_file_is_open() {
    let directory = TempDir::new("lock");
    let path = directory.path("project.desegnos");
    let _first = ProjectDatabase::create(&path, &project(), now_millis()).unwrap();
    let error = match ProjectDatabase::open(&path) {
        Ok(_) => panic!("a second writer was allowed"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("already open"),
        "unexpected error: {error}"
    );
}

#[test]
fn an_open_saved_project_leaves_only_its_file_in_the_folder() {
    let directory = TempDir::new("one-file");
    let path = directory.path("Bakery site.desegnos");
    let project = project_with_element();
    let database = ProjectDatabase::create(&path, &project, now_millis()).unwrap();
    database.save(&project, &[], Some("Saved")).unwrap();

    let visible: Vec<String> = fs::read_dir(&directory.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    assert_eq!(visible, vec!["Bakery site.desegnos"]);
}

#[test]
fn autosave_reads_without_the_writing_lock() {
    let directory = TempDir::new("recovery");
    let path = directory.path("project.desegnos");
    let project = project_with_element();
    let database = ProjectDatabase::create(&path, &project, now_millis()).unwrap();
    let mut changed = project.clone();
    changed.name = "Crash recovery".into();
    database.autosave(&changed, now_millis()).unwrap();
    drop(database);
    let recovered = desegnos_storage::read_autosave(&path).unwrap().unwrap();
    assert_eq!(recovered.project.name, "Crash recovery");
    assert_eq!(
        desegnos_storage::load_project(&path).unwrap().name,
        "Storage"
    );
}
