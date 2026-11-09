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
mod testing;
mod updates;
mod usage;
mod website_import;
mod workspace;

include!(concat!(env!("OUT_DIR"), "/registered_commands.rs"));

use tauri::Manager;

fn main() {
    crash_report::install_hook();
    #[cfg(target_os = "linux")]
    if let Err(error) = linux_startup::prepare() {
        eprintln!("Desegnos could not prepare its graphics startup: {error}");
        std::process::exit(1);
    }
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::UpdateState::default())
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
            let services = loop {
                match Services::initialize(app.handle(), data.path()) {
                    Ok(services) => break services,
                    Err(problem) => match data_location::recover(app.handle(), problem) {
                        Ok(next) => data = next,
                        Err(problem) => quit(&problem),
                    },
                }
            };
            app.manage(data);
            app.manage(std::sync::Mutex::new(services.workspace));
            app.manage(std::sync::Mutex::new(services.exports));
            app.manage(std::sync::Mutex::new(local_site::LocalSite::default()));
            app.manage(std::sync::Mutex::new(
                website_import::WebsiteImportState::default(),
            ));
            app.manage(chatgpt::ChatgptState::initialize(
                services.settings.chatgpt_directory(),
            ));
            app.manage(std::sync::Mutex::new(services.settings));
            app.manage(std::sync::Mutex::new(services.generation));
            app.manage(std::sync::Mutex::new(services.identity));
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

/// Every service that keeps Desegnos's own files, built before anything is managed. They share
/// one `SwitchGuard`, so a running AI job in any of them blocks switching projects.
struct Services {
    workspace: workspace::Workspace,
    settings: ai_settings::AiSettingsStore,
    generation: generation::GenerationService,
    identity: identity::IdentityService,
    exports: export::ExportService,
}

impl Services {
    /// Builds the services on the chosen data folder. A storage failure can then be retried with
    /// another folder without leaving half-managed state behind.
    fn initialize(
        app: &tauri::AppHandle,
        data: &std::path::Path,
    ) -> Result<Self, data_location::Problem> {
        let workspace = workspace::Workspace::initialize(app, data.to_owned())
            .map_err(|error| data_location::Problem::error(data, error.as_ref()))?;
        let guard = workspace.switch_guard.clone();
        Ok(Self {
            settings: ai_settings::AiSettingsStore::initialize(data, guard.clone())
                .map_err(|_| data_location::Problem::unusable(data))?,
            generation: generation::GenerationService::initialize(data, guard.clone()),
            identity: identity::IdentityService::initialize(data, guard.clone()),
            exports: export::ExportService::initialize(data, guard),
            workspace,
        })
    }
}

/// Reports the startup failure on stderr and stops, as the error dialog's Quit offers.
fn quit(problem: &data_location::Problem) -> ! {
    crash_report::finish();
    eprintln!("{problem}");
    std::process::exit(1);
}
