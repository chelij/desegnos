#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(feature = "native-tests", not(debug_assertions)))]
compile_error!("Native test automation must not be compiled into release artifacts.");

#[cfg(all(not(debug_assertions), dev))]
compile_error!("Release builds must embed the frontend: enable the custom-protocol feature.");

mod chatgpt;
mod clipboard;
mod export;
mod generation;
mod identity;
mod image_drop;
#[cfg(target_os = "linux")]
mod linux_startup;
mod preview;
mod preview_health;
mod project_location;
mod recent;
mod system_open;
mod usage;
mod workspace;
mod website_import;

use generation::*;
use preview_health::{editor_heartbeat, reload_workspace};
use tauri::Manager;
use workspace::*;

fn main() {
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
            app.manage(std::sync::Mutex::new(Workspace::initialize(app.handle())?));
            app.manage(std::sync::Mutex::new(export::ExportService::default()));
            let generation =
                GenerationService::initialize(app.handle()).map_err(std::io::Error::other)?;
            app.manage(chatgpt::ChatgptState::initialize(
                generation.chatgpt_directory(),
            ));
            app.manage(std::sync::Mutex::new(generation));
            app.manage(std::sync::Mutex::new(
                identity::IdentityService::initialize(app.handle())
                    .map_err(std::io::Error::other)?,
            ));
            app.manage(std::sync::Mutex::new(preview::PreviewRegistry::default()));
            app.manage(preview_health::PreviewHealth::default());
            app.manage(std::sync::Mutex::new(website_import::WebsiteImports::default()));
            #[cfg(target_os = "linux")]
            if let Some(window) = app.get_webview_window("editor") {
                preview::configure_window(&window)?;
            }
            preview_health::install(app.handle())?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "reload-workspace"
                && let Err(error) = preview_health::reload(app)
            {
                use tauri_plugin_dialog::DialogExt;
                app.dialog()
                    .message(error)
                    .title("Workspace recovery")
                    .show(|_| {});
            }
        })
        .invoke_handler(tauri::generate_handler![
            export::export_website,
            usage::record_usage,
            workspace::open_preview_link,
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
            refresh_element_bounds,
            refresh_element_target_bounds,
            edit_element,
            undo_element,
            edit_page_source,
            get_page_source_map,
            open_found_source,
            copy_layer,
            cut_layer,
            paste_layer,
            undo_project,
            redo_project,
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
            create_preview,
            release_preview,
            editor_heartbeat,
            reload_workspace
        ])
        .build(tauri::generate_context!())
        .expect("Desegnos could not start its native application")
        .run(|app, event| {
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
