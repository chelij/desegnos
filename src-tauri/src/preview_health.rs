use crate::preview::PreviewRegistry;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Manager, WebviewWindow, WebviewWindowBuilder,
    menu::{Menu, MenuItem, Submenu},
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

pub struct PreviewHealth {
    last_heartbeat: Mutex<Instant>,
    pub recovering: AtomicBool,
    warned: AtomicBool,
}

impl Default for PreviewHealth {
    fn default() -> Self {
        Self {
            last_heartbeat: Mutex::new(Instant::now()),
            recovering: AtomicBool::new(false),
            warned: AtomicBool::new(false),
        }
    }
}

pub fn reload(app: &AppHandle) -> Result<(), String> {
    let health = app.state::<PreviewHealth>();
    if health.recovering.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let result = (|| {
        app.state::<Mutex<PreviewRegistry>>()
            .lock()
            .map_err(|error| error.to_string())?
            .clear();
        let window = app
            .get_webview_window("editor")
            .ok_or("The editor window is unavailable.")?;
        let scale = window.scale_factor().unwrap_or(1.0);
        let size = window
            .inner_size()
            .ok()
            .map(|size| size.to_logical::<f64>(scale));
        let position = window
            .outer_position()
            .ok()
            .map(|position| position.to_logical::<f64>(scale));
        window.destroy().map_err(|error| error.to_string())?;
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let callback_app = handle.clone();
            if let Err(error) = handle.run_on_main_thread(move || {
                let result = (|| {
                    let config = callback_app.config().app.windows.iter().find(|config| config.label == "editor").ok_or("The editor configuration is unavailable.")?;
                    let mut builder = WebviewWindowBuilder::from_config(&callback_app, config).map_err(|error| error.to_string())?;
                    if let Some(size) = size { builder = builder.inner_size(size.width, size.height); }
                    if let Some(position) = position { builder = builder.position(position.x, position.y); }
                    let window = builder.build().map_err(|error| error.to_string())?;
                    #[cfg(target_os = "linux")]
                    crate::preview::configure_window(&window).map_err(|error| error.to_string())?;
                    #[cfg(not(target_os = "linux"))]
                    let _ = window;
                    Ok::<_, String>(())
                })();
                let health = callback_app.state::<PreviewHealth>();
                if let Ok(mut last) = health.last_heartbeat.lock() { *last = Instant::now(); }
                health.warned.store(false, Ordering::SeqCst);
                health.recovering.store(false, Ordering::SeqCst);
                if let Err(error) = result {
                    eprintln!("Editor recovery failed: {error}");
                    callback_app.dialog().message("The editor could not reopen. Restart Desegnos to recover your changes.").title("Workspace recovery").kind(MessageDialogKind::Error).show(|_| {});
                }
            }) {
                eprintln!("Editor reload could not be scheduled: {error}");
                handle.state::<PreviewHealth>().recovering.store(false, Ordering::SeqCst);
                handle.dialog().message("The editor could not reload. Restart Desegnos to recover your changes.").title("Workspace recovery").kind(MessageDialogKind::Error).show(|_| {});
            }
        });
        Ok::<_, String>(())
    })();
    if result.is_err() {
        health.recovering.store(false, Ordering::SeqCst);
    }
    result
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let reload_item = MenuItem::with_id(
        app,
        "reload-workspace",
        "Reload workspace (pause website scripts)",
        true,
        Some("CmdOrCtrl+Shift+R"),
    )?;
    let workspace = Submenu::with_items(app, "Workspace", true, &[&reload_item])?;
    app.set_menu(Menu::with_items(app, &[&workspace])?)?;
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let health = app.state::<PreviewHealth>();
            if app.get_webview_window("editor").is_none()
                && !health.recovering.load(Ordering::SeqCst)
            {
                break;
            }
            if health.recovering.load(Ordering::SeqCst) {
                continue;
            }
            let stale = health
                .last_heartbeat
                .lock()
                .is_ok_and(|last| last.elapsed() >= Duration::from_secs(12));
            let scripts_enabled = app
                .state::<Mutex<PreviewRegistry>>()
                .lock()
                .is_ok_and(|registry| registry.scripts_enabled());
            if stale && scripts_enabled && !health.warned.swap(true, Ordering::SeqCst) {
                let response_app = app.clone();
                app.dialog().message("This page stopped responding. Reload the editor to recover your changes.").title("Website preview stopped responding").kind(MessageDialogKind::Warning).buttons(MessageDialogButtons::OkCancelCustom("Reload workspace".into(), "Keep waiting".into())).show(move |accepted| {
                if accepted && let Err(error) = reload(&response_app) {
                    eprintln!("Editor reload failed: {error}");
                    response_app.dialog().message("The editor could not reload. Restart Desegnos to recover your changes.").title("Workspace recovery").kind(MessageDialogKind::Error).show(|_| {});
                }
            });
            }
        }
    });
    Ok(())
}

#[tauri::command(async)]
pub fn editor_heartbeat(
    window: WebviewWindow,
    health: tauri::State<'_, PreviewHealth>,
) -> Result<(), String> {
    if window.label() != "editor" {
        return Err("Only the editor can report workspace health.".into());
    }
    *health
        .last_heartbeat
        .lock()
        .map_err(|error| error.to_string())? = Instant::now();
    health.warned.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn reload_workspace(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if window.label() != "editor" {
        return Err("Only the editor can reload its workspace.".into());
    }
    reload(&app)
}
