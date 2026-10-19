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
    lock.try_lock().map_err(|_| {
        "Another window is updating recent projects. Open or save again to update the list."
            .to_owned()
    })?;
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
    entries.retain(|entry| entry.path != path);
    entries.insert(0, RecentProject { path, name });
    entries.truncate(10);
    let json = serde_json::to_string(&entries).map_err(|error| error.to_string())?;
    desegni_storage::write_markdown(&directory.join("recent-projects.json"), &json)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recents_are_deduplicated_bounded_and_corruption_is_preserved() {
        let directory =
            std::env::temp_dir().join(format!("desegnos-recents-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        for index in 0..12 {
            let path = directory.join(format!("{index}.desegni"));
            std::fs::write(&path, b"test-owned source").unwrap();
            remember(&directory, &path, &format!("Site {index}")).unwrap();
        }
        let entries = read(&directory).unwrap();
        assert_eq!(entries.len(), 10);
        assert_eq!(entries[0].name, "Site 11");
        remember(&directory, &directory.join("5.desegni"), "Renamed site").unwrap();
        let entries = read(&directory).unwrap();
        assert_eq!(entries.len(), 10);
        assert_eq!(entries[0].name, "Renamed site");
        let path = directory.join("recent-projects.json");
        std::fs::write(&path, b"broken catalogue").unwrap();
        assert!(read(&directory).is_err());
        assert!(remember(&directory, &directory.join("5.desegni"), "Keep source").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken catalogue");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
