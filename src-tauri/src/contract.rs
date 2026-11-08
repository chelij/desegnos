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
        crate::project_history::ProjectHistoryView,
        crate::project_history::CompactReport,
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
        ("MAX_IMPORT_PAGES", MAX_IMPORT_PAGES as f64),
        ("MAX_IMPORT_CAPTURES", MAX_IMPORT_CAPTURES as f64),
        ("MAX_WEBSITE_FILE_BYTES", MAX_WEBSITE_FILE_BYTES as f64),
        (
            "MAX_IDENTITY_SOURCE_BYTES",
            MAX_IDENTITY_SOURCE_BYTES as f64,
        ),
        ("MAX_REQUEST_FILE_BYTES", MAX_REQUEST_FILE_BYTES as f64),
        ("DEFAULT_PICTURE_WIDTH", DEFAULT_PICTURE_WIDTH as f64),
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

fn generated_files() -> Vec<(&'static str, String)> {
    vec![
        ("types.ts", types()),
        ("limits.ts", limits()),
        ("commands.ts", commands()),
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
}
