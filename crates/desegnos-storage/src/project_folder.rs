//! A project file lives in its own folder. The folder holds the `.desegnos` file and nothing
//! else; saving never touches neighbouring files.
use crate::io;
use desegnos_core::{Error as CoreError, Result};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub fn sanitize_project_name(name: &str) -> String {
    let mut clean: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(80)
        .collect();
    clean = clean.trim_matches([' ', '.']).to_owned();
    if clean.is_empty() {
        clean = "Website".into();
    }
    let stem = clean.split('.').next().unwrap_or("").to_uppercase();
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || (stem.len() >= 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&&stem[3..]))
        || clean.to_lowercase().starts_with(".desegnos")
    {
        clean.insert(0, '_');
    }
    // Bound bytes as well as characters for common filesystem component limits.
    while clean.len() > 180 {
        clean.pop();
    }
    clean.trim_end_matches([' ', '.']).to_owned()
}

/// Reserve with create_dir, so simultaneous creates and case-insensitive collisions converge.
pub fn create_project_folder(parent: &Path, name: &str) -> Result<PathBuf> {
    if !parent.is_absolute() || parent.components().any(|c| c == Component::ParentDir) {
        return Err(CoreError("Choose an absolute project location.".into()));
    }
    fs::create_dir_all(parent).map_err(io)?;
    let parent = parent.canonicalize().map_err(io)?;
    let name = sanitize_project_name(name);
    for number in 1..10_000 {
        let name = if number == 1 {
            name.clone()
        } else {
            format!("{name} {number}")
        };
        // Use portable collision rules even on a case-sensitive host.
        if fs::read_dir(&parent).map_err(io)?.any(|entry| {
            entry.is_ok_and(|e| {
                e.file_name().to_string_lossy().to_lowercase() == name.to_lowercase()
            })
        }) {
            continue;
        }
        let folder = parent.join(&name);
        match fs::create_dir(&folder) {
            Ok(()) => return Ok(folder.join(format!("{name}.desegnos"))),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io(error)),
        }
    }
    Err(CoreError(
        "Too many projects share this name. Choose another name.".into(),
    ))
}

pub fn project_in_folder(folder: &Path) -> Result<Option<PathBuf>> {
    if !folder.is_dir() {
        return Ok(None);
    }
    let mut found = None;
    for entry in fs::read_dir(folder).map_err(io)? {
        let entry = entry.map_err(io)?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("desegnos") {
            if found.is_some() {
                return Ok(None);
            }
            found = Some(path);
        }
    }
    Ok(found)
}
