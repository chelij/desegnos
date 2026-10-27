use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecentProject {
    pub path: PathBuf,
    pub name: String,
}

pub fn read(directory: &Path) -> Result<Vec<RecentProject>, String> {
    let path = directory.join("recent-projects.json");
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = vec![];
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 65_536 {
        return Err("The recent-project list is too large. Its original is preserved.".into());
    }
    let entries: Vec<RecentProject> = serde_json::from_slice(&bytes)
        .map_err(|_| "The recent-project list is damaged. Its original is preserved.".to_owned())?;
    if entries.len() > 10
        || entries.iter().any(|entry| {
            !entry.path.is_absolute()
                || entry.path.as_os_str().len() > 4096
                || entry.name.trim().is_empty()
                || entry.name.chars().count() > 120
                || entry.name.chars().any(char::is_control)
        })
    {
        return Err(
            "The recent-project list contains invalid entries. Its original is preserved.".into(),
        );
    }
    Ok(entries)
}

pub fn remember(directory: &Path, path: &Path, name: &str) -> Result<(), String> {
    remember_destination(directory, path, name, None)
}

pub fn moved(directory: &Path, path: &Path, name: &str, former: &Path) -> Result<(), String> {
    remember_destination(directory, path, name, Some(former))
}

fn remember_destination(
    directory: &Path,
    path: &Path,
    name: &str,
    former: Option<&Path>,
) -> Result<(), String> {
    let _lock = lock(directory)?;
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    if path.as_os_str().len() > 4096 {
        return Err("This project path is too long for the recent list.".into());
    }
    let name: String = name
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(120)
        .collect();
    let mut entries = read(directory)?;
    entries.retain(|entry| entry.path != path && Some(entry.path.as_path()) != former);
    entries.insert(0, RecentProject { path, name });
    entries.truncate(10);
    write(directory, &entries)
}

fn lock(directory: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options
        .open(directory.join("recent-projects.lock"))
        .map_err(|error| error.to_string())?;
    // A child process forked by another thread can inherit this descriptor for the instant
    // before exec closes it, so a single try can race. Retry briefly before reporting a real
    // writer, and stay bounded so the UI is never held for long.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(std::fs::TryLockError::WouldBlock)
                if std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(error) => {
                return Err(format!(
                    "Another window is updating recent projects. Try again to update the list. {error}"
                ));
            }
        }
    }
}

fn write(directory: &Path, entries: &[RecentProject]) -> Result<(), String> {
    let json = serde_json::to_string(entries).map_err(|error| error.to_string())?;
    desegnos_storage::write_markdown(&directory.join("recent-projects.json"), &json)
        .map_err(|error| error.to_string())
}

pub fn forget(directory: &Path, path: &Path) -> Result<Vec<RecentProject>, String> {
    let _lock = lock(directory)?;
    let mut entries = read(directory)?;
    let previous_len = entries.len();
    // Match the stored path without accessing the project: it may have been moved or deleted.
    entries.retain(|entry| entry.path != path);
    if entries.len() != previous_len {
        write(directory, &entries)?;
    }
    Ok(entries)
}

#[tauri::command]
pub fn forget_recent_project(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, std::sync::Mutex<crate::workspace::Workspace>>,
    path: PathBuf,
) -> Result<Vec<RecentProject>, String> {
    if window.label() != "editor" {
        return Err("Only the editor can remove recent projects.".into());
    }
    let state = state.lock().map_err(|error| error.to_string())?;
    forget(state.directory(), &path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forgetting_preserves_projects_and_other_entries_and_handles_missing_paths() {
        let directory =
            std::env::temp_dir().join(format!("desegnos-forget-recents-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let project = directory.join("Keep project.desegnos");
        let folder = directory.join("Keep folder");
        let missing = directory.join("Missing.desegnos");
        std::fs::write(&project, b"original project").unwrap();
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("index.html"), b"original website").unwrap();
        std::fs::write(&missing, b"moved project").unwrap();
        remember(&directory, &project, "Keep project").unwrap();
        remember(&directory, &folder, "Keep folder").unwrap();
        remember(&directory, &missing, "Missing").unwrap();
        std::fs::remove_file(&missing).unwrap();

        let remaining = forget(&directory, &project).unwrap();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].path, missing);
        assert_eq!(remaining[1].path, folder);
        assert_eq!(std::fs::read(&project).unwrap(), b"original project");
        let remaining = forget(&directory, &missing).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].path, folder);
        assert!(!missing.exists());
        assert!(forget(&directory, &folder).unwrap().is_empty());
        assert_eq!(
            std::fs::read(folder.join("index.html")).unwrap(),
            b"original website"
        );
        let catalogue = directory.join("recent-projects.json");
        let before = std::fs::read(&catalogue).unwrap();
        assert!(forget(&directory, &missing).unwrap().is_empty());
        assert_eq!(std::fs::read(&catalogue).unwrap(), before);

        let held = lock(&directory).unwrap();
        assert!(forget(&directory, &project).is_err());
        assert_eq!(std::fs::read(&catalogue).unwrap(), before);
        drop(held);
        std::fs::write(&catalogue, b"damaged catalogue").unwrap();
        assert!(forget(&directory, &project).is_err());
        assert_eq!(std::fs::read(&catalogue).unwrap(), b"damaged catalogue");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recents_are_deduplicated_bounded_and_corruption_is_preserved() {
        let directory =
            std::env::temp_dir().join(format!("desegnos-recents-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        for index in 0..12 {
            let path = directory.join(format!("{index}.desegnos"));
            std::fs::write(&path, b"test-owned source").unwrap();
            remember(&directory, &path, &format!("Site {index}")).unwrap();
        }
        let entries = read(&directory).unwrap();
        assert_eq!(entries.len(), 10);
        assert_eq!(entries[0].name, "Site 11");
        remember(&directory, &directory.join("5.desegnos"), "Renamed site").unwrap();
        let entries = read(&directory).unwrap();
        assert_eq!(entries.len(), 10);
        assert_eq!(entries[0].name, "Renamed site");
        moved(
            &directory,
            &directory.join("6.desegnos"),
            "Moved site",
            &directory.join("5.desegnos"),
        )
        .unwrap();
        let entries = read(&directory).unwrap();
        assert_eq!(entries[0].name, "Moved site");
        assert!(
            !entries
                .iter()
                .any(|entry| entry.path == directory.join("5.desegnos"))
        );
        let path = directory.join("recent-projects.json");
        std::fs::write(&path, b"broken catalogue").unwrap();
        assert!(read(&directory).is_err());
        assert!(remember(&directory, &directory.join("5.desegnos"), "Keep source").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken catalogue");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
