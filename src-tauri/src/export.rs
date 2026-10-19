//! Native export authority: frozen revision, destination picker, verified receipt.
use desegni_core::{Project, compile_website};
use desegni_storage::{WrittenWebsiteExport, write_website_export};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

type NativeResult<T> = Result<T, String>;
#[derive(Default)]
pub struct ExportService {
    active: bool,
    last: Option<WrittenWebsiteExport>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    path: String,
    index_path: String,
    page_count: usize,
    file_count: usize,
    warnings: Vec<String>,
}
fn trusted(window: &WebviewWindow) -> NativeResult<()> {
    if window.label() == "editor" {
        Ok(())
    } else {
        Err("Only the Desegnos editor can export websites.".into())
    }
}
fn snapshot(
    state: &Mutex<crate::workspace::Workspace>,
    project_id: Uuid,
    revision: u64,
) -> NativeResult<Project> {
    let workspace = state.lock().map_err(|_| "Project access is unavailable.")?;
    let project = workspace.project()?;
    if project.id != project_id || project.revision != revision {
        return Err("The website changed. Export the current version again.".into());
    }
    Ok(project.clone())
}
#[tauri::command]
pub async fn export_website(
    app: AppHandle,
    window: WebviewWindow,
    workspace: State<'_, Mutex<crate::workspace::Workspace>>,
    service: State<'_, Mutex<ExportService>>,
    project_id: Uuid,
    revision: u64,
) -> NativeResult<Option<ExportResult>> {
    trusted(&window)?;
    let project = snapshot(&workspace, project_id, revision)?;
    {
        let mut state = service
            .lock()
            .map_err(|_| "Export access is unavailable.")?;
        if state.active {
            return Err("A website export is already in progress.".into());
        }
        state.active = true;
    }
    let result = perform(app, &workspace, project).await;
    let mut state = service
        .lock()
        .map_err(|_| "Export access is unavailable.")?;
    state.active = false;
    match result? {
        None => Ok(None),
        Some(written) => {
            let result = ExportResult {
                path: written.path.to_string_lossy().into_owned(),
                index_path: written
                    .path
                    .join("index.html")
                    .to_string_lossy()
                    .into_owned(),
                page_count: written.page_count,
                file_count: written.file_count,
                warnings: written.warnings.clone(),
            };
            state.last = Some(written);
            Ok(Some(result))
        }
    }
}
async fn perform(
    app: AppHandle,
    workspace: &Mutex<crate::workspace::Workspace>,
    project: Project,
) -> NativeResult<Option<WrittenWebsiteExport>> {
    let before = project.clone();
    let compiled = tauri::async_runtime::spawn_blocking(move || compile_website(&before))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose where to export your website")
            .blocking_pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    snapshot(workspace, project.id, project.revision)?;
    let parent = picked.into_path().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        write_website_export(&parent, &project.name, &compiled)
    })
    .await
    .map_err(|e| e.to_string())?
    .map(Some)
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn open_export_folder(
    window: WebviewWindow,
    service: State<'_, Mutex<ExportService>>,
    path: String,
) -> NativeResult<()> {
    trusted(&window)?;
    // Renderer-supplied paths are comparison-only; the OS receives the native
    // receipt's exact path after checking the output identity and file hashes.
    let state = service
        .lock()
        .map_err(|_| "Export access is unavailable.")?;
    let written = state
        .last
        .as_ref()
        .ok_or("Export a website before opening its folder.")?;
    if written.path.to_string_lossy() != path {
        return Err("That folder is not the most recent website export.".into());
    }
    written.verify().map_err(|e| e.to_string())?;
    open::that_detached(&written.path).map_err(|_| "The export is complete, but the system could not open its folder. Use the displayed path in your file manager.".into())
}
