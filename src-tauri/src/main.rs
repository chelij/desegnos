#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(feature = "native-tests", not(debug_assertions)))]
compile_error!("Native test automation must not be compiled into release artifacts.");

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
#[cfg(target_os = "linux")]
mod linux_startup;
mod local_site;
#[cfg(all(target_os = "linux", feature = "portal-dialogs"))]
mod portal;
mod preview;
mod preview_health;
mod project_location;
mod recent;
mod release;
mod request_zip;
mod system_open;
mod usage;
mod website_import;
mod workspace;

use generation::*;
use preview_health::{editor_heartbeat, reload_workspace};
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
        .register_uri_scheme_protocol("desegni-preview", |context, request| {
            if context.webview_label() != "editor" {
                return tauri::http::Response::builder()
                    .status(403)
                    .body(Vec::new())
                    .expect("Fixed preview response is valid");
            }
            match context
                .app_handle()
                .state::<std::sync::Mutex<preview::PreviewRegistry>>()
                .lock()
            {
                Ok(registry) => registry.serve(&request),
                Err(_) => tauri::http::Response::builder()
                    .status(503)
                    .body(Vec::new())
                    .expect("Fixed preview response is valid"),
            }
        })
        .setup(|app| {
            // Startup storage failures come first: nothing is managed until the folder that
            // holds Desegnos's own files has been proven usable.
            file_dialogs::note_missing_portal();
            let mut data = match data_location::start(app.handle()) {
                Ok(data) => data,
                Err(problem) => quit(&problem),
            };
            crash_report::start(data.path());
            app.manage(std::sync::Mutex::new(local_site::LocalSite::for_app(
                app.handle(),
            )));
            let (workspace, generation, identity) = loop {
                match initialize(app.handle(), data.path()) {
                    Ok(services) => break services,
                    Err(problem) => match data_location::recover(app.handle(), problem) {
                        Ok(next) => data = next,
                        Err(problem) => quit(&problem),
                    },
                }
            };
            app.manage(data);
            app.manage(std::sync::Mutex::new(workspace));
            app.manage(std::sync::Mutex::new(export::ExportService::default()));
            app.manage(chatgpt::ChatgptState::initialize(
                generation.chatgpt_directory(),
            ));
            app.manage(std::sync::Mutex::new(generation));
            app.manage(std::sync::Mutex::new(identity));
            app.manage(std::sync::Mutex::new(preview::PreviewRegistry::default()));
            app.manage(preview_health::PreviewHealth::default());
            app.manage(std::sync::Mutex::new(
                website_import::WebsiteImports::default(),
            ));
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
            #[cfg(target_os = "linux")]
            preview::configure_window(&window)?;
            preview_health::install(app.handle())?;
            window.show()?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "reload-workspace"
                && let Err(error) = preview_health::reload(app)
            {
                use tauri_plugin_dialog::DialogExt;
                eprintln!("Editor reload failed: {error}");
                app.dialog()
                    .message(
                        "The editor could not reload. Restart Desegnos to recover your changes.",
                    )
                    .title("Workspace recovery")
                    .show(|_| {});
            }
        })
        .invoke_handler(tauri::generate_handler![
            request_zip::unpack_request_zip,
            export::export_website,
            usage::record_usage,
            workspace::open_preview_link,
            local_site::open_in_browser,
            export::open_export_folder,
            identity::pick_identity_source,
            identity::fetch_identity_source,
            identity::prepare_identity_selection,
            workspace::close_project,
            workspace::switch_project,
            workspace::import_context,
            website_import::pick_website_folder,
            website_import::website_build,
            workspace::edit_website_folder,
            workspace::prepare_website_import,
            workspace::apply_website_import,
            workspace::project_site_import_source,
            workspace::overwrite_project_site,
            identity::website_brand_source,
            identity::analyze_identity,
            identity::get_identity_analysis,
            identity::cancel_identity_analysis,
            get_workspace,
            project_location::get_project_location,
            project_location::pick_project_location,
            project_location::set_project_location,
            move_into_project_folder,
            get_recent_projects,
            recent::forget_recent_project,
            open_recent_project,
            create_project,
            edit_project,
            inspect_element,
            get_frame_targets,
            get_frame_documents,
            ensure_element_id,
            refresh_element_bounds,
            refresh_element_target_bounds,
            edit_element,
            edit_elements,
            element_matches,
            edit_element_across_pages,
            undo_element,
            edit_page_source,
            get_page_source_map,
            open_found_source,
            copy_layer,
            copy_elements,
            clipboard::copy_text,
            cut_layer,
            paste_layer,
            undo_project,
            redo_project,
            end_history_step,
            open_project,
            save_project,
            save_project_checked,
            resolve_save_conflict,
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
            generation_draft,
            create_preview,
            release_preview,
            editor_heartbeat,
            reload_workspace,
            crash_report::pending_crash_report,
            crash_report::open_crash_report,
            crash_report::dismiss_crash_report,
            crash_report::record_frontend_crash
        ])
        .build(tauri::generate_context!())
        .expect("Desegnos could not start its native application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                crash_report::finish();
                if let Ok(mut site) = app
                    .state::<std::sync::Mutex<local_site::LocalSite>>()
                    .lock()
                {
                    site.stop();
                }
            }
            if let tauri::RunEvent::ExitRequested { api, .. } = event
                && app
                    .state::<preview_health::PreviewHealth>()
                    .recovering
                    .load(std::sync::atomic::Ordering::SeqCst)
            {
                api.prevent_exit();
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
