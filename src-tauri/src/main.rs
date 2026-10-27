#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(not(debug_assertions), dev))]
compile_error!("Release builds must embed the frontend: enable the custom-protocol feature.");

mod chatgpt;
mod clipboard;
mod crash_report;
mod data_location;
mod export;
mod file_dialogs;
mod generation;
mod identity;
mod image_drop;
mod local_site;
#[cfg(target_os = "linux")]
mod linux_startup;
#[cfg(all(target_os = "linux", feature = "portal-dialogs"))]
mod portal;
mod project_location;
mod recent;
mod release;
mod request_zip;
mod system_open;
mod updates;
mod usage;
mod website_import;
mod workspace;

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
            let (workspace, generation, identity) = loop {
                match initialize(app.handle(), data.path()) {
                    Ok(services) => break services,
                    Err(problem) => match data_location::recover(app.handle(), problem) {
                        Ok(next) => data = next,
                        Err(problem) => quit(&problem),
                    },
                }
            };
            let exports = export::ExportService::initialize(data.path());
            app.manage(data);
            app.manage(std::sync::Mutex::new(workspace));
            app.manage(std::sync::Mutex::new(exports));
            app.manage(std::sync::Mutex::new(local_site::LocalSite::default()));
            app.manage(std::sync::Mutex::new(website_import::WebsiteImportState::default()));
            app.manage(chatgpt::ChatgptState::initialize(
                generation.chatgpt_directory(),
            ));
            app.manage(std::sync::Mutex::new(generation));
            app.manage(std::sync::Mutex::new(identity));
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
        .invoke_handler(tauri::generate_handler![
            request_zip::unpack_request_zip,
            export::prepare_export,
            export::start_export,
            export::cancel_export,
            export::get_export_job,
            export::recheck_export,
            export::keep_pointer_code,
            export::open_in_browser,
            export::pick_export_destination,
            export::open_export_folder,
            export::close_preview,
            usage::record_usage,
            workspace::open_preview_link,
            identity::pick_identity_source,
            identity::fetch_identity_source,
            website_import::prepare_website_import_url,
            website_import::pick_website_import_source,
            website_import::cancel_website_import,
            website_import::import_website_pages,
            identity::prepare_identity_selection,
            workspace::close_project,
            workspace::switch_project,
            workspace::import_context,
            identity::analyze_identity,
            identity::get_identity_analysis,
            identity::cancel_identity_analysis,
            get_workspace,
            project_location::get_project_location,
            project_location::get_app_settings,
            project_location::set_show_start_page,
            project_location::pick_project_location,
            project_location::set_project_location,
            move_into_project_folder,
            get_recent_projects,
            recent::forget_recent_project,
            recent::set_recent_project_pin,
            open_recent_project,
            new_untitled_project,
            save_untitled_project,
            pick_project_save_path,
            edit_project,
            clipboard::copy_text,
            undo_project,
            redo_project,
            end_history_step,
            open_project,
            save_project,
            save_project_checked,
            resolve_save_conflict,
            save_project_copy,
            compact_history,
            autosave_project,
            get_project_history,
            restore_version,
            switch_alternative,
            create_alternative,
            rename_alternative,
            delete_alternative,
            prepare_requirements,
            save_requirements,
            recover_project,
            dismiss_recovery,
            get_project_notes,
            get_ai_settings,
            save_ai_settings,
            set_ai_provider,
            set_ai_key,
            remove_ai_key,
            chatgpt::set_ai_authentication,
            chatgpt::begin_chatgpt_sign_in,
            chatgpt::cancel_chatgpt_sign_in,
            chatgpt::select_chatgpt_account,
            chatgpt::retry_chatgpt_secure_storage,
            chatgpt::sign_out_chatgpt,
            chatgpt::acknowledge_chatgpt_welcome,
            chatgpt::open_chatgpt_usage,
            test_ai_access,
            prepare_generation,
            get_generation,
            list_generations,
            get_past_generation,
            start_generation,
            retry_generation,
            cancel_generation,
            discard_generation,
            apply_generation,
            crash_report::pending_crash_report,
            crash_report::open_crash_report,
            crash_report::dismiss_crash_report,
            crash_report::record_frontend_crash,
            updates::check_for_updates
        ])
        .build(tauri::generate_context!())
        .expect("Desegnos could not start its native application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                crash_report::finish();
            }
        });
}

/// Every service that keeps Desegnos's own files, built before anything is managed.
type Services = (Workspace, GenerationService, identity::IdentityService);

/// Builds the services on the chosen data folder. A storage failure can then be retried with
/// another folder without leaving half-managed state behind.
fn initialize(
    app: &tauri::AppHandle,
    data: &std::path::Path,
) -> Result<Services, data_location::Problem> {
    let workspace = Workspace::initialize(app, data.to_owned())
        .map_err(|error| data_location::Problem::error(data, error.as_ref()))?;
    let generation =
        GenerationService::initialize(data).map_err(|_| data_location::Problem::unusable(data))?;
    let identity = identity::IdentityService::initialize(data);
    Ok((workspace, generation, identity))
}

/// Reports the startup failure on stderr and stops, as the error dialog's Quit offers.
fn quit(problem: &data_location::Problem) -> ! {
    crash_report::finish();
    eprintln!("{problem}");
    std::process::exit(1);
}
