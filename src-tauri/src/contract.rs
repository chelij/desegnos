//! The Rust-to-TypeScript contract: every type, limit and command name the frontend shares with
//! native code is written to `src/generated/`. The test below fails when those files are stale;
//! regenerate them with `npm run generate:contract`.
use std::{any::TypeId, collections::BTreeMap, collections::HashSet, fs, path::PathBuf};
use ts_rs::{Config, TS, TypeVisitor};

struct Collect<'a> {
    config: &'a Config,
    seen: HashSet<TypeId>,
    declarations: BTreeMap<String, String>,
}

impl TypeVisitor for Collect<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if T::output_path().is_none() || !self.seen.insert(TypeId::of::<T>()) {
            return;
        }
        self.declarations.insert(
            T::ident(self.config),
            format!("export {}", T::decl(self.config)),
        );
        T::visit_dependencies(self);
    }
}

fn types() -> String {
    let config = Config::new().with_large_int("number");
    let mut collect = Collect {
        config: &config,
        seen: HashSet::new(),
        declarations: BTreeMap::new(),
    };
    macro_rules! roots { ($($root:ty),* $(,)?) => { $(collect.visit::<$root>();)* }; }
    roots![
        crate::native_error::NativeFailure,
        desegnos_core::Command,
        desegnos_core::WebCapture,
        desegnos_core::SizePreset,
        desegnos_ai::ProviderError,
        crate::workspace::WorkspaceView,
        crate::workspace::SaveProjectOutcome,
        crate::workspace::RecentProjectsView,
        crate::ai_settings::AiSettingsView,
        crate::generation::GenerationJobView,
        crate::generation::GenerationHistory,
        crate::ai_settings::AccessResult,
        crate::export::ExportJobView,
        crate::export::ExportCheck,
        crate::export::PickDestination,
        crate::identity::AnalysisView,
        crate::identity::IdentitySelection,
        crate::identity::IdentitySourceManifest,
        crate::project_location::ProjectLocationView,
        crate::project_location::AppSettingsView,
        crate::website_import::WebsiteImportView,
        crate::website_import::WebsiteImportResultView,
        crate::updates::UpdateCheck,
        crate::crash_report::CrashReportView,
        crate::crash_report::OpenFailure,
        crate::fonts::CanvasFont,
        crate::image_drop::ImageDrag,
        desegnos_storage::project_db::ProjectHistoryView,
        desegnos_storage::project_db::CompactReport,
    ];
    let mut text =
        String::from("// Generated from Rust by `npm run generate:contract`. Do not edit.\n\n");
    let comments = strip_comments;
    for declaration in collect.declarations.values() {
        text.push_str(&comments(declaration));
        text.push_str("\n\n");
    }
    text.truncate(text.trim_end().len());
    text.push('\n');
    text
}

fn limits() -> String {
    use desegnos_core::{GridSettings, SizePreset, limits::*};
    let mut text =
        String::from("// Generated from Rust by `npm run generate:contract`. Do not edit.\n\n");
    let numbers = [
        ("GRID_PIXEL_MIN", GRID_PIXEL_MIN as f64),
        ("GRID_PIXEL_MAX", GRID_PIXEL_MAX as f64),
        ("GRID_PERCENT_MIN", GRID_PERCENT_MIN as f64),
        ("GRID_PERCENT_MAX", GRID_PERCENT_MAX as f64),
        ("MAX_LAYER_LABEL_CHARS", MAX_LAYER_LABEL_CHARS as f64),
        ("MAX_FONT_WEIGHT", MAX_FONT_WEIGHT as f64),
        ("MAX_GRID_COLUMNS", MAX_GRID_COLUMNS as f64),
        (
            "MAX_ATTACHMENT_TEXT_CHARS",
            MAX_ATTACHMENT_TEXT_CHARS as f64,
        ),
        ("MAX_EDITOR_COORDINATE", MAX_EDITOR_COORDINATE as f64),
        ("MAX_PASTED_IMAGE_WIDTH", MAX_PASTED_IMAGE_WIDTH as f64),
        ("PASTED_NOTE_WIDTH", PASTED_NOTE_WIDTH as f64),
        ("PASTED_NOTE_HEIGHT", PASTED_NOTE_HEIGHT as f64),
        ("SWATCH_SIZE", SWATCH_SIZE as f64),
        ("MAX_IMPORT_PAGES", MAX_IMPORT_PAGES as f64),
        ("MAX_REQUEST_FILE_BYTES", MAX_REQUEST_FILE_BYTES as f64),
        ("MAX_PROMPT_CHARS", MAX_PROMPT_CHARS as f64),
        ("MAX_NAME_CHARS", MAX_NAME_CHARS as f64),
        (
            "MAX_BRANCH_CHARS",
            desegnos_storage::project_db::MAX_BRANCH_CHARS as f64,
        ),
        (
            "MAX_NAMED_VIEW_NAME_CHARS",
            MAX_NAMED_VIEW_NAME_CHARS as f64,
        ),
        ("MAX_HTTP_URL_BYTES", MAX_HTTP_URL_BYTES as f64),
        ("MAX_ATTACHMENTS", MAX_ELEMENT_CONTEXT as f64),
        ("MAX_VIEWPORT_SIDE", MAX_VIEWPORT_SIDE as f64),
        ("DEFAULT_GRID_SPACING", GridSettings::default().spacing),
        ("TEMPLATE_GAP", desegnos_core::TEMPLATE_GAP as f64),
    ];
    for (name, value) in numbers {
        text.push_str(&format!("export const {name} = {value};\n"));
    }
    text.push_str(&format!(
        "export const TEXT_SIZE_RANGE = {:?} as const;\n",
        [TEXT_SIZE_RANGE.0, TEXT_SIZE_RANGE.1]
    ));
    text.push_str(&format!(
        "export const IMAGE_MIME_TYPES: readonly string[] = {};\n",
        serde_json::to_string(IMAGE_MIME_TYPES).unwrap()
    ));
    text.push_str(&format!(
        "export const RESERVED_ROUTE_NAMES: readonly string[] = {};\n",
        serde_json::to_string(RESERVED_ROUTE_NAMES).unwrap()
    ));
    text.push_str("export const SIZE_LABELS = [\n");
    for &(width, label) in desegnos_core::SIZE_LABELS {
        text.push_str(&format!("  {{ min: {width}, label: {:?} }},\n", label));
    }
    text.push_str("] as const;\n");
    text.push_str("export const ELEMENT_DEFAULTS = {\n");
    for name in [
        "box", "group", "text", "ellipse", "line", "image", "button", "link", "input", "textarea",
        "select", "checkbox", "radio", "video", "audio", "embed", "code",
    ] {
        let value = match name {
            "input" => serde_json::json!({"kind": name, "inputType": "text"}),
            "select" => serde_json::json!({"kind": name, "options": []}),
            "code" => serde_json::json!({"kind": name, "language": "", "source": ""}),
            _ => serde_json::json!({"kind": name}),
        };
        let kind: desegnos_core::ElementKind = serde_json::from_value(value).unwrap();
        let (width, height) = desegnos_core::default_element_size(&kind);
        text.push_str(&format!("  {name}: {{ label: {:?}, name: {:?}, text: {:?}, width: {width}, height: {height} }},\n", kind.label(), desegnos_core::default_layer_name_base(&kind), desegnos_core::default_element_text(&kind)));
    }
    text.push_str("} as const;\nexport const TEMPLATE_DEFAULTS = {\n");
    for template in [
        desegnos_core::CanvasTemplate::Blank,
        desegnos_core::CanvasTemplate::Website,
        desegnos_core::CanvasTemplate::AppScreen,
        desegnos_core::CanvasTemplate::SlideDeck,
        desegnos_core::CanvasTemplate::SocialImage,
    ] {
        let pages = template.pages();
        let fallback = desegnos_core::CanvasTemplate::Website.pages();
        let page = pages.first().unwrap_or(&fallback[0]);
        let size = &page.sizes[0];
        let page_size = if page.output == desegnos_core::OutputType::Webpage {
            size
        } else {
            &fallback[0].sizes[0]
        };
        let key = serde_json::to_value(template).unwrap();
        text.push_str(&format!(
            "  {}: {{ output: {}, kind: {}, width: {}, height: {}, page: {{ width: {}, height: {}, kind: {} }} }},\n",
            key,
            serde_json::to_value(page.output).unwrap(),
            serde_json::to_value(size.kind).unwrap(),
            size.width,
            size.height,
            page_size.width,
            page_size.height,
            serde_json::to_value(page_size.kind).unwrap()
        ));
    }
    text.push_str("} as const;\n");
    text.push_str("\nexport const SIZE_PRESETS = {\n");
    for (key, preset) in [
        ("phone", SizePreset::Phone),
        ("tablet", SizePreset::Tablet),
        ("desktop", SizePreset::Desktop),
    ] {
        let (name, width, height) = preset.dimensions(0, 0);
        text.push_str(&format!(
            "  {key}: {{ name: \"{name}\", width: {width}, height: {height} }},\n"
        ));
    }
    text.push_str("} as const;\n");
    text
}

fn commands() -> String {
    let mut text = String::from(
        "// Generated from Rust by `npm run generate:contract`. Do not edit.\n\nexport type CommandName =\n",
    );
    for line in include_str!("../commands.txt")
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        text.push_str(&format!(
            "  | '{}'\n",
            line.trim().rsplit("::").next().unwrap()
        ));
    }
    text.truncate(text.trim_end().len());
    text.push_str(";\n\n");
    text.push_str(&format!(
        "export const IMAGE_DRAG_EVENT = '{}';\n",
        crate::image_drop::IMAGE_DRAG_EVENT
    ));
    text
}

fn element_fact_cases() -> String {
    use desegnos_core::{
        ElementKind, Rect, default_element_text, default_layer_name_base, empty_element,
        is_default_layer_name,
    };
    let mut cases = Vec::new();
    for kind in [
        ElementKind::Box,
        ElementKind::Text,
        ElementKind::Button,
        ElementKind::Code {
            language: "css".into(),
            source: String::new(),
        },
    ] {
        let kind_json = serde_json::to_value(&kind).unwrap();
        let base = default_layer_name_base(&kind);
        for name in [
            None,
            Some(format!("{base} 1")),
            Some(format!("{base} 01")),
            Some(format!("{base} 0")),
            Some(format!("{base} 12x")),
            Some("Custom name".into()),
        ] {
            for text in [
                "".to_owned(),
                default_element_text(&kind).to_owned(),
                "Custom text\nSecond line".into(),
                "😀".repeat(90),
            ] {
                let mut element = empty_element(
                    kind.clone(),
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    },
                );
                element.name = name.clone();
                element.text = text;
                cases.push(serde_json::json!({
                    "element": { "kind": kind_json["kind"], "language": kind_json.get("language"), "name": element.name, "text": element.text },
                    "label": element.label(), "isDefault": is_default_layer_name(&element)
                }));
            }
        }
    }
    format!(
        "[\n{}\n]\n",
        cases
            .iter()
            .map(|case| format!("  {case}"))
            .collect::<Vec<_>>()
            .join(",\n")
    )
}

fn generated_files() -> Vec<(&'static str, String)> {
    vec![
        ("types.ts", types()),
        ("limits.ts", limits()),
        ("commands.ts", commands()),
        ("elementFacts.json", element_fact_cases()),
    ]
}

#[test]
fn generated_typescript_is_current() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/generated");
    let regenerate = std::env::var_os("UPDATE_GENERATED").is_some();
    let mut stale = Vec::new();
    for (name, text) in generated_files() {
        let path = folder.join(name);
        if fs::read_to_string(&path).ok().as_deref() == Some(&text) {
            continue;
        }
        if regenerate {
            fs::create_dir_all(&folder).unwrap();
            fs::write(&path, text).unwrap();
        } else {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "src/generated/{stale:?} is stale; run `npm run generate:contract`"
    );
}

/// Drops the doc comments ts-rs copies in, and starts each union member on its own line.
fn strip_comments(declaration: &str) -> String {
    let mut out = String::new();
    let mut rest = declaration;
    while let Some(start) = rest.find("/**") {
        out.push_str(&rest[..start]);
        let end = rest[start..]
            .find("*/")
            .map_or(rest.len(), |end| start + end + 2);
        rest = rest[end..].trim_start_matches('\n');
    }
    out.push_str(rest);
    out.replace("} | {", "}\n  | {")
        .replace("\" | \"", "\"\n  | \"")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}
