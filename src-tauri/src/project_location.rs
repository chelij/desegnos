use crate::native_files::{LockExt, SMALL_RECORD_BYTES, lock_for_update, read_json};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

/// App-level settings, kept beside the project files. Older files only hold the default
/// project location; a missing start-page choice means the start page is shown.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Settings {
    default_project_location: Option<PathBuf>,
    show_start_page: Option<bool>,
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLocationView {
    path: PathBuf,
    system_default: PathBuf,
    customized: bool,
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsView {
    show_start_page: bool,
}

pub fn default_from_documents(documents: &Path) -> PathBuf {
    documents.join("Desegnos")
}

fn read_settings(directory: &Path) -> Result<Settings, String> {
    let path = directory.join("project-settings.json");
    if !path.exists() {
        return Ok(Settings::default());
    }
    read_json(&path, SMALL_RECORD_BYTES, "project settings file")
}

/// Read, change and write the settings under one lock so two windows cannot drop each
/// other's choice.
fn update_settings(
    directory: &Path,
    change: impl FnOnce(&mut Settings),
) -> Result<Settings, String> {
    let _lock = lock_for_update(&directory.join("project-settings.lock"), "project settings")?;
    let mut settings = read_settings(directory)?;
    change(&mut settings);
    let json = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    desegnos_storage::write_text(&directory.join("project-settings.json"), &json)
        .map_err(|e| e.to_string())?;
    Ok(settings)
}

pub fn read(directory: &Path, default: &Path) -> Result<PathBuf, String> {
    let settings = read_settings(directory)?;
    let path = settings
        .default_project_location
        .unwrap_or_else(|| default.to_owned());
    validate(&path)?;
    Ok(path)
}

fn validate(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path.as_os_str().len() > 4096
        || path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
    {
        return Err("Choose a folder for your projects.".into());
    }
    Ok(())
}

fn view(state: &crate::workspace::Workspace) -> Result<ProjectLocationView, String> {
    let path = read(state.directory(), state.system_project_location())?;
    Ok(ProjectLocationView {
        customized: path != state.system_project_location(),
        path,
        system_default: state.system_project_location().to_owned(),
    })
}

#[tauri::command]
pub fn get_project_location(
    state: State<'_, Mutex<crate::workspace::Workspace>>,
) -> Result<ProjectLocationView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> Result<ProjectLocationView, String> {
        let state = state.locked();
        view(&state)
    })
}

#[tauri::command]
pub fn get_app_settings(
    state: State<'_, Mutex<crate::workspace::Workspace>>,
) -> Result<AppSettingsView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> Result<AppSettingsView, String> {
        let state = state.locked();
        let settings = read_settings(state.directory())?;
        Ok(AppSettingsView {
            show_start_page: settings.show_start_page.unwrap_or(true),
        })
    })
}

#[tauri::command]
pub fn set_show_start_page(
    state: State<'_, Mutex<crate::workspace::Workspace>>,
    value: bool,
) -> Result<AppSettingsView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> Result<AppSettingsView, String> {
        let state = state.locked();
        update_settings(state.directory(), |settings| {
            settings.show_start_page = Some(value);
        })?;
        Ok(AppSettingsView {
            show_start_page: value,
        })
    })
}

#[tauri::command]
pub async fn pick_project_location(
    app: AppHandle,
) -> Result<Option<PathBuf>, crate::native_error::NativeFailure> {
    let result: Result<Option<PathBuf>, String> = async {
        tauri::async_runtime::spawn_blocking(move || -> Result<Option<PathBuf>, String> {
            let Some(picked) = crate::file_dialogs::picked(
                app.dialog()
                    .file()
                    .set_title("Choose project location")
                    .blocking_pick_folder(),
            )?
            else {
                return Ok(None);
            };
            picked.into_path().map(Some).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::Command.failure())
}

#[tauri::command]
pub fn set_project_location(
    state: State<'_, Mutex<crate::workspace::Workspace>>,
    path: Option<PathBuf>,
) -> Result<ProjectLocationView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::SaveLocation.run(
        || -> Result<ProjectLocationView, String> {
            let state = state.locked();
            if let Some(path) = &path {
                validate(path)?;
                if !path.is_dir() {
                    return Err("Choose an existing parent folder.".into());
                }
            }
            update_settings(state.directory(), |settings| {
                settings.default_project_location = path;
            })?;
            view(&state)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn documents_default_and_persisted_override_and_reset() {
        let root =
            std::env::temp_dir().join(format!("desegnos-locations-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let default = default_from_documents(&root.join("Documents"));
        assert_eq!(
            read(&root, &default).unwrap(),
            root.join("Documents/Desegnos")
        );
        let settings = root.join("project-settings.json");
        std::fs::write(
            &settings,
            serde_json::to_vec(&Settings {
                default_project_location: Some(root.clone()),
                show_start_page: None,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(read(&root, &default).unwrap(), root);
        std::fs::write(&settings, b"{\"defaultProjectLocation\":null}").unwrap();
        assert_eq!(read(&root, &default).unwrap(), default);
        std::fs::write(
            &settings,
            b"{\"defaultProjectLocation\":null,\"showStartPage\":false}",
        )
        .unwrap();
        assert!(!read_settings(&root).unwrap().show_start_page.unwrap());
        update_settings(&root, |settings| settings.show_start_page = Some(true)).unwrap();
        let reread = read_settings(&root).unwrap();
        assert_eq!(reread.show_start_page, Some(true));
        assert!(
            reread.default_project_location.is_none(),
            "the location stays unset"
        );
        std::fs::write(&settings, b"broken").unwrap();
        assert!(read(&root, &default).is_err());
        assert_eq!(std::fs::read(&settings).unwrap(), b"broken");
        std::fs::remove_dir_all(root).unwrap();
    }
}
