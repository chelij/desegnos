//! The Rust-to-TypeScript contract: every type, limit and command name the frontend shares with
//! native code is written to `src/generated/`. The test below fails when those files are stale;
//! regenerate them with `npm run generate:contract`.
use std::{any::TypeId, collections::BTreeMap, collections::HashSet, fs, path::PathBuf};
use ts_rs::{Config, TS, TypeVisitor};

const HEADER: &str = "// Generated from Rust by `npm run generate:contract`. Do not edit.\n\n";

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
        crate::updates::UpdateProgress,
        crate::usage::UsageEvent,
        crate::crash_report::CrashReportView,
        crate::crash_report::OpenFailure,
        crate::fonts::CanvasFont,
        crate::image_drop::ImageDrag,
        crate::project_history::ProjectHistoryView,
        crate::project_history::CompactReport,
    ];
    let mut text = String::from(HEADER);
    for declaration in collect.declarations.values() {
        text.push_str(&strip_comments(declaration));
        text.push_str("\n\n");
    }
    text.truncate(text.trim_end().len());
    text.push('\n');
    text
}

fn limits() -> String {
    use desegnos_core::{GridSettings, SizePreset, limits::*};
    let mut text = String::from(HEADER);
    macro_rules! numbers {
        ($($limit:path),* $(,)?) => {
            [$((stringify!($limit).rsplit(':').next().unwrap(), $limit as f64)),*]
        };
    }
    let numbers = numbers![
        GRID_PIXEL_MIN,
        GRID_PIXEL_MAX,
        GRID_PERCENT_MIN,
        GRID_PERCENT_MAX,
        MAX_LAYER_LABEL_CHARS,
        MAX_FONT_WEIGHT,
        MAX_GRID_COLUMNS,
        MAX_ATTACHMENT_TEXT_CHARS,
        MAX_EDITOR_COORDINATE,
        MAX_PASTED_IMAGE_WIDTH,
        PASTED_NOTE_WIDTH,
        PASTED_NOTE_HEIGHT,
        SWATCH_SIZE,
        MAX_IMPORT_PAGES,
        MAX_REQUEST_FILE_BYTES,
        MAX_PROMPT_CHARS,
        MAX_NAME_CHARS,
        desegnos_storage::project_db::MAX_BRANCH_CHARS,
        MAX_NAMED_VIEW_NAME_CHARS,
        MAX_HTTP_URL_BYTES,
        MAX_ELEMENT_CONTEXT,
        AUTOSAVE_INTERVAL_MS,
        MAX_VIEWPORT_SIDE,
        desegnos_core::TEMPLATE_GAP,
    ];
    for (name, value) in numbers {
        text.push_str(&format!("export const {name} = {value};\n"));
    }
    text.push_str(&format!(
        "export const DEFAULT_GRID_SPACING = {};\n",
        GridSettings::default().spacing
    ));
    text.push_str(&format!(
        "export const DEFAULT_LOCAL_ENDPOINT = {:?};\n",
        desegnos_ai::DEFAULT_LOCAL_ENDPOINT
    ));
    text.push_str(&format!(
        "export const TEXT_SIZE_RANGE = {:?} as const;\n",
        [TEXT_SIZE_RANGE.0, TEXT_SIZE_RANGE.1]
    ));
    text.push_str(&format!(
        "export const IMAGE_MIME_TYPES: readonly string[] = {};\n",
        serde_json::to_string(IMAGE_MIME_TYPES).unwrap()
    ));
    text.push_str(&format!(
        "export const NEEDS_AI_SETTINGS_ERROR_KINDS: readonly string[] = {};\n",
        serde_json::to_string(&desegnos_ai::ErrorKind::NEEDS_AI_SETTINGS).unwrap()
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
    let mut text = format!("{HEADER}export type CommandName =\n");
    for name in crate::COMMAND_NAMES {
        text.push_str(&format!("  | '{name}'\n"));
    }
    text.truncate(text.trim_end().len());
    text.push_str(";\n\n");
    text.push_str(&format!(
        "export const IMAGE_DRAG_EVENT = '{}';\n",
        crate::image_drop::IMAGE_DRAG_EVENT
    ));
    text.push_str(&format!(
        "export const UPDATE_PROGRESS_EVENT = '{}';\n",
        crate::updates::UPDATE_PROGRESS_EVENT
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
