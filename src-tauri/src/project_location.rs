use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Settings {
    default_project_location: Option<PathBuf>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLocationView {
    path: PathBuf,
    system_default: PathBuf,
    customized: bool,
}

pub fn default_from_documents(documents: &Path) -> PathBuf {
    documents.join("Desegnos")
}

pub fn read(directory: &Path, default: &Path) -> Result<PathBuf, String> {
    let file = match File::open(directory.join("project-settings.json")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(default.to_owned()),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65_536 {
        return Err("The project settings file is too large. Its original was preserved.".into());
    }
    let settings: Settings = serde_json::from_slice(&bytes).map_err(|_| {
        "Project settings could not be read. Their original was preserved.".to_owned()
    })?;
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
) -> Result<ProjectLocationView, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    view(&state)
}

#[tauri::command]
pub async fn pick_project_location(app: AppHandle) -> Result<Option<PathBuf>, String> {
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

#[tauri::command]
pub fn set_project_location(
    state: State<'_, Mutex<crate::workspace::Workspace>>,
    path: Option<PathBuf>,
) -> Result<ProjectLocationView, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    if let Some(path) = &path {
        validate(path)?;
        if !path.is_dir() {
            return Err("Choose an existing parent folder.".into());
        }
    }
    // A lock prevents concurrent windows from replacing each other's setting.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state.directory().join("project-settings.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|_| "Another window is updating project settings. Try again.".to_owned())?;
    let json = serde_json::to_string(&Settings {
        default_project_location: path,
    })
    .map_err(|e| e.to_string())?;
    desegnos_storage::write_markdown(&state.directory().join("project-settings.json"), &json)
        .map_err(|e| e.to_string())?;
    view(&state)
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
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(read(&root, &default).unwrap(), root);
        std::fs::write(&settings, b"{\"defaultProjectLocation\":null}").unwrap();
        assert_eq!(read(&root, &default).unwrap(), default);
        std::fs::write(&settings, b"broken").unwrap();
        assert!(read(&root, &default).is_err());
        assert_eq!(std::fs::read(&settings).unwrap(), b"broken");
        std::fs::remove_dir_all(root).unwrap();
    }
}
