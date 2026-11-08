#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(not(debug_assertions), dev))]
compile_error!("Release builds must embed the frontend: enable the custom-protocol feature.");

mod ai_access;
mod ai_job;
mod ai_settings;
mod chatgpt;
mod clipboard;
#[cfg(test)]
mod contract;
mod crash_report;
mod data_location;
mod export;
mod file_dialogs;
mod fonts;
mod generation;
mod identity;
mod image_drop;
#[cfg(target_os = "linux")]
mod linux_startup;
mod local_site;
mod native_error;
mod native_files;
#[cfg(all(target_os = "linux", feature = "portal-dialogs"))]
mod portal;
mod project_history;
mod project_location;
mod recent;
mod release;
mod shutdown;
mod system_open;
mod updates;
mod usage;
mod website_import;
mod workspace;

include!(concat!(env!("OUT_DIR"), "/registered_commands.rs"));

use generation::*;
use tauri::Manager;
use workspace::*;

fn main() {
    crash_report::install_hook();
    #[cfg(target_os = "linux")]
    if let Err(error) = linux_startup::prepare() {
        eprintln!("Desegnos could not prepare its graphics startup: {error}");
        std::process::exit(1);
    }
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init());
    #[cfg(feature = "native-tests")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());
    builder
        .on_window_event(image_drop::handle)
        .setup(|app| {
            // Startup storage failures come first: nothing is managed until the folder that
            // holds Desegnos's own files has been proven usable.
            file_dialogs::note_missing_portal();
            let mut data = match data_location::start(app.handle()) {
                Ok(data) => data,
                Err(problem) => quit(&problem),
            };
            crash_report::start(data.path());
            let (workspace, settings, generation, identity) = loop {
                match initialize(app.handle(), data.path()) {
                    Ok(services) => break services,
                    Err(problem) => match data_location::recover(app.handle(), problem) {
                        Ok(next) => data = next,
                        Err(problem) => quit(&problem),
                    },
                }
            };
            let mut exports = export::ExportService::initialize(data.path());
            exports.bind_switch_guard(workspace.switch_guard.clone());
            app.manage(data);
            app.manage(std::sync::Mutex::new(workspace));
            app.manage(std::sync::Mutex::new(exports));
            app.manage(std::sync::Mutex::new(local_site::LocalSite::default()));
            app.manage(std::sync::Mutex::new(
                website_import::WebsiteImportState::default(),
            ));
            app.manage(chatgpt::ChatgptState::initialize(
                settings.chatgpt_directory(),
            ));
            app.manage(std::sync::Mutex::new(settings));
            app.manage(std::sync::Mutex::new(generation));
            app.manage(std::sync::Mutex::new(identity));
            shutdown::install(app.handle().clone())?;
            // The editor window is created only now, so its commands work from the first render.
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "editor")
                .cloned()
                .ok_or("The editor window is not configured.")?;
            let window =
                tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?.build()?;
            window.show()?;
            Ok(())
        })
        .invoke_handler(registered_commands!())
        .build(tauri::generate_context!())
        .expect("Desegnos could not start its native application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                crash_report::finish();
            }
        });
}

/// Every service that keeps Desegnos's own files, built before anything is managed.
type Services = (
    Workspace,
    ai_settings::AiSettingsStore,
    GenerationService,
    identity::IdentityService,
);

/// Builds the services on the chosen data folder. A storage failure can then be retried with
/// another folder without leaving half-managed state behind.
fn initialize(
    app: &tauri::AppHandle,
    data: &std::path::Path,
) -> Result<Services, data_location::Problem> {
    let workspace = Workspace::initialize(app, data.to_owned())
        .map_err(|error| data_location::Problem::error(data, error.as_ref()))?;
    let mut settings = ai_settings::AiSettingsStore::initialize(data)
        .map_err(|_| data_location::Problem::unusable(data))?;
    settings.switch_guard = workspace.switch_guard.clone();
    let mut generation = GenerationService::initialize(data);
    generation.bind_switch_guard(workspace.switch_guard.clone());
    let mut identity = identity::IdentityService::initialize(data);
    identity.bind_switch_guard(workspace.switch_guard.clone());
    Ok((workspace, settings, generation, identity))
}

/// Reports the startup failure on stderr and stops, as the error dialog's Quit offers.
fn quit(problem: &data_location::Problem) -> ! {
    crash_report::finish();
    eprintln!("{problem}");
    std::process::exit(1);
}
