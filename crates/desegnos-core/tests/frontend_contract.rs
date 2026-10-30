//! The editor reads Rust JSON directly. These checks keep every field it requires on the wire,
//! including empty collections and null optionals, for a fresh project and every start template.
use desegnos_core::{
    Appearance, BoundaryKind, CanvasTemplate, Command, ElementKind, ElementLayout, ElementMeta,
    ElementOverride, ElementShadow, ElementStroke, ElementStyle, ElementTextStyle, ExportRecord,
    ExportedPage, InputType, LinkedDefinition, OutputType, PageSize, PendingRequest, Pointer,
    PointerSource, PointerTarget, Project, Rect, ReferenceItem, ReferenceKind, RequestAttachment,
    RequestRecord, RequestTarget, TextAlign, empty_element,
};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn require(value: &Value, path: &str, keys: &[&str]) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{path} must be an object, got {value}"));
    for key in keys {
        assert!(
            object.contains_key(*key),
            "{path} is missing {key}; serialized value: {value}"
        );
    }
}

fn require_kind(value: &Value, path: &str) {
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{path}.kind must be a string, got {value}"));
    assert!(!kind.is_empty(), "{path}.kind must not be empty");
}

fn check_style(value: &Value, path: &str) {
    require(
        value,
        path,
        &["fill", "stroke", "radius", "opacity", "shadow", "textStyle"],
    );
    require(
        &value["stroke"],
        &format!("{path}.stroke"),
        &["color", "width"],
    );
    require(
        &value["shadow"],
        &format!("{path}.shadow"),
        &["x", "y", "blur", "color"],
    );
    require(
        &value["textStyle"],
        &format!("{path}.textStyle"),
        &[
            "fontFamily",
            "size",
            "weight",
            "italic",
            "lineHeight",
            "letterSpacing",
            "color",
            "align",
        ],
    );
}

fn check_element(value: &Value, path: &str) {
    require(
        value,
        path,
        &[
            "id",
            "kind",
            "name",
            "rect",
            "rotation",
            "visible",
            "locked",
            "style",
            "text",
            "image",
            "url",
            "layout",
            "overrides",
            "children",
            "meta",
        ],
    );
    require_kind(value, path);
    require(
        &value["rect"],
        &format!("{path}.rect"),
        &["x", "y", "width", "height"],
    );
    check_style(&value["style"], &format!("{path}.style"));
    require(&value["layout"], &format!("{path}.layout"), &["kind"]);
    let meta = &value["meta"];
    require(meta, &format!("{path}.meta"), &["requests", "context"]);
    for (index, record) in meta["requests"]
        .as_array()
        .expect("requests array")
        .iter()
        .enumerate()
    {
        require(
            record,
            &format!("{path}.meta.requests[{index}]"),
            &[
                "id",
                "prompt",
                "attachments",
                "createdAt",
                "outcome",
                "jobId",
            ],
        );
    }
    for (index, over) in value["overrides"]
        .as_object()
        .expect("overrides object")
        .values()
        .enumerate()
    {
        require(
            over,
            &format!("{path}.overrides[{index}]"),
            &["hidden", "dismissed"],
        );
    }
    for (index, child) in value["children"]
        .as_array()
        .expect("children array")
        .iter()
        .enumerate()
    {
        check_element(child, &format!("{path}.children[{index}]"));
    }
}

fn check_project(project: &Project) {
    let value = serde_json::to_value(project).expect("a project serializes");
    require(
        &value,
        "project",
        &[
            "schemaVersion",
            "id",
            "name",
            "revision",
            "viewport",
            "appearance",
            "generationMode",
            "showGrid",
            "gridSettings",
            "latestGeneration",
            "homePageId",
            "pages",
            "pictures",
            "reference",
            "pointers",
            "linked",
            "requests",
            "brief",
            "decisions",
            "exceptions",
            "appliedJobIds",
            "extensions",
        ],
    );
    for (index, request) in value["requests"]
        .as_array()
        .expect("requests array")
        .iter()
        .enumerate()
    {
        require(
            request,
            &format!("project.requests[{index}]"),
            &["id", "text", "attachments", "targets", "requestedAt"],
        );
    }
    require(&value["viewport"], "project.viewport", &["width", "height"]);
    require(
        &value["gridSettings"],
        "project.gridSettings",
        &["spacing", "unit", "snap", "rulers"],
    );
    require(
        &value["brief"],
        "project.brief",
        &["purpose", "audience", "brandName", "tone"],
    );
    for (page_index, page) in value["pages"]
        .as_array()
        .expect("pages array")
        .iter()
        .enumerate()
    {
        let page_path = format!("project.pages[{page_index}]");
        require(
            page,
            &page_path,
            &[
                "id",
                "name",
                "path",
                "included",
                "output",
                "sizes",
                "background",
                "elements",
            ],
        );
        for (size_index, size) in page["sizes"]
            .as_array()
            .expect("sizes array")
            .iter()
            .enumerate()
        {
            require(
                size,
                &format!("{page_path}.sizes[{size_index}]"),
                &["id", "label", "width", "height", "kind", "x", "y"],
            );
        }
        for (element_index, element) in page["elements"]
            .as_array()
            .expect("elements array")
            .iter()
            .enumerate()
        {
            check_element(element, &format!("{page_path}.elements[{element_index}]"));
        }
    }
    for (index, item) in value["reference"]
        .as_array()
        .expect("reference array")
        .iter()
        .enumerate()
    {
        let path = format!("project.reference[{index}]");
        require(item, &path, &["id", "rect", "kind"]);
        let kind = item["kind"].as_object().expect("reference kind object");
        assert!(
            kind.get("kind").and_then(Value::as_str).is_some(),
            "{path}.kind.kind must be a string"
        );
    }
    for (index, pointer) in value["pointers"]
        .as_array()
        .expect("pointers array")
        .iter()
        .enumerate()
    {
        let path = format!("project.pointers[{index}]");
        require(
            pointer,
            &path,
            &["id", "from", "to", "comment", "kind", "wiring"],
        );
        assert!(
            pointer["wiring"].is_null(),
            "{path}.wiring must be serialized as null"
        );
    }
    for (id, definition) in value["linked"].as_object().expect("linked object") {
        check_element(
            &definition["master"],
            &format!("project.linked[{id}].master"),
        );
        require(
            definition,
            &format!("project.linked[{id}]"),
            &["id", "name", "master"],
        );
    }
    if let Some(export) = value.get("lastExport").filter(|value| !value.is_null()) {
        require(
            export,
            "project.lastExport",
            &[
                "id",
                "version",
                "destination",
                "exportedAt",
                "pages",
                "pointers",
            ],
        );
        for (index, page) in export["pages"]
            .as_array()
            .expect("export pages")
            .iter()
            .enumerate()
        {
            require(
                page,
                &format!("project.lastExport.pages[{index}]"),
                &["pageId", "path", "output"],
            );
        }
        assert!(
            export["pointers"].is_object(),
            "project.lastExport.pointers must be an object"
        );
    }
}

fn page_size(id: Uuid) -> PageSize {
    PageSize {
        id,
        label: "Desktop".into(),
        width: 1440,
        height: 900,
        kind: BoundaryKind::Page,
        x: 0.0,
        y: 0.0,
    }
}

fn with_page(mut project: Project) -> Project {
    if project.pages.is_empty() {
        project = project
            .apply(
                project.revision,
                Command::AddCanvasPage {
                    name: "Home".into(),
                    path: "/".into(),
                    size: page_size(Uuid::new_v4()),
                    moves: vec![],
                    reference: vec![],
                },
            )
            .expect("a blank canvas accepts its first boundary");
    }
    project
}

fn sample_kinds() -> Vec<ElementKind> {
    vec![
        ElementKind::Box,
        ElementKind::Group,
        ElementKind::Text,
        ElementKind::Ellipse,
        ElementKind::Line,
        ElementKind::Image,
        ElementKind::Button,
        ElementKind::Link,
        ElementKind::Input {
            input_type: InputType::Email,
        },
        ElementKind::Textarea,
        ElementKind::Select {
            options: vec!["One".into(), "Two".into()],
        },
        ElementKind::Checkbox,
        ElementKind::Radio,
        ElementKind::Video,
        ElementKind::Audio,
        ElementKind::Embed,
        ElementKind::Code {
            language: "html".into(),
            source: "<div>Hello</div>".into(),
        },
    ]
}

fn populated(mut project: Project) -> Project {
    let size_id = project.pages[0].base_size_id();
    let kinds = sample_kinds();
    for (index, kind) in kinds.into_iter().enumerate() {
        let mut element = empty_element(kind, rect(10.0 + index as f64 * 12.0, 20.0, 120.0, 40.0));
        if index == 0 {
            element.meta = ElementMeta {
                requests: vec![RequestRecord {
                    id: Uuid::new_v4(),
                    prompt: "Make a card".into(),
                    attachments: vec![RequestAttachment {
                        id: "attachment".into(),
                        name: "note.txt".into(),
                        kind: desegnos_core::AttachmentKind::Text,
                        data_uri: None,
                        preview: None,
                        text: Some("hello".into()),
                        url: None,
                    }],
                    created_at: "2026-10-27T00:00:00Z".into(),
                    outcome: None,
                    job_id: None,
                }],
                context: vec![],
            };
            element.overrides.insert(
                size_id,
                ElementOverride {
                    rect: Some(rect(12.0, 22.0, 100.0, 30.0)),
                    style: Some(ElementStyle {
                        flip_x: false,
                        flip_y: false,
                        sizing: Default::default(),
                        padding: None,
                        references: Default::default(),
                        fill: Some("#ffffff".into()),
                        stroke: ElementStroke {
                            color: None,
                            width: 0.0,
                        },
                        radius: 4.0,
                        opacity: 1.0,
                        shadow: ElementShadow {
                            x: 0.0,
                            y: 0.0,
                            blur: 0.0,
                            color: None,
                        },
                        text_style: ElementTextStyle {
                            font_family: "system-ui".into(),
                            size: 16.0,
                            weight: 400,
                            italic: false,
                            line_height: 1.4,
                            letter_spacing: 0.0,
                            color: "#111111".into(),
                            align: TextAlign::Left,
                        },
                    }),
                    layout: Some(ElementLayout::Free),
                    hidden: false,
                    dismissed: false,
                },
            );
        }
        project.pages[0].elements.push(element);
    }
    let request_target = project.pages[0].elements[0].id;
    project.requests.push(PendingRequest {
        id: Uuid::new_v4(),
        text: "Make a card".into(),
        attachments: vec![],
        targets: vec![RequestTarget::Element {
            page: project.pages[0].id,
            size: Some(size_id),
            id: request_target,
        }],
        area: None,
        requested_at: "2026-10-27T00:00:00Z".into(),
    });

    let master = empty_element(ElementKind::Box, rect(0.0, 0.0, 80.0, 40.0));
    let linked_id = Uuid::new_v4();
    project.linked.insert(
        linked_id,
        LinkedDefinition {
            id: linked_id,
            name: "Card".into(),
            master,
        },
    );

    let reference_id = Uuid::new_v4();
    project.reference.push(ReferenceItem {
        id: reference_id,
        rect: rect(-200.0, 40.0, 180.0, 120.0),
        kind: ReferenceKind::Note {
            text: "Remember this".into(),
        },
    });
    project.pointers.push(Pointer {
        id: Uuid::new_v4(),
        from: PointerSource::Boundary {
            page_id: project.home_page_id,
            size_id: None,
        },
        to: PointerTarget::Reference {
            item_id: reference_id,
        },
        comment: "Read this note".into(),
        kind: desegnos_core::PointerKind::Reference,
        wiring: None,
        play: None,
        play_comment: String::new(),
    });
    project.last_export = Some(ExportRecord {
        id: Uuid::new_v4(),
        project_id: project.id,
        version: 1,
        destination: "/tmp/desegnos-export".into(),
        behaviours: BTreeMap::new(),
        exported_at: "2026-10-27T00:00:00Z".into(),
        pages: vec![ExportedPage {
            page_id: project.home_page_id,
            path: "/".into(),
            output: OutputType::Webpage,
        }],
        pointers: BTreeMap::new(),
    });
    project
}

#[test]
fn every_start_template_and_a_fresh_untitled_project_serialize_required_frontend_fields() {
    for template in [
        CanvasTemplate::Blank,
        CanvasTemplate::Website,
        CanvasTemplate::AppScreen,
        CanvasTemplate::SlideDeck,
        CanvasTemplate::SocialImage,
    ] {
        let project = populated(with_page(
            Project::untitled("Canvas".into(), Appearance::Light, template).unwrap(),
        ));
        check_project(&project);
    }
}

#[test]
fn elements_keep_their_kind_fields_flat_for_the_editor() {
    let project = populated(with_page(
        Project::untitled("Canvas".into(), Appearance::Light, CanvasTemplate::Blank).unwrap(),
    ));
    let value: Value = serde_json::to_value(&project).unwrap();
    let elements = value["pages"][0]["elements"].as_array().unwrap();
    assert_eq!(elements[0]["kind"], "box");
    let input = elements
        .iter()
        .find(|element| element["kind"] == "input")
        .expect("an input element");
    assert_eq!(input["inputType"], "email");
    let select = elements
        .iter()
        .find(|element| element["kind"] == "select")
        .expect("a select element");
    assert_eq!(select["options"][0], "One");
    let code = elements
        .iter()
        .find(|element| element["kind"] == "code")
        .expect("a code element");
    assert_eq!(code["language"], "html");
    assert_eq!(code["source"], "<div>Hello</div>");
}
