//! Desegnos storage: the project database, folders, exports and import sources.
pub mod identity_source;
pub mod project_db;
pub mod project_folder;
pub mod project_parts;
pub mod request_zip;
mod source_files;
pub mod system_tools;
pub mod website_export;
pub mod website_snapshot;
use desegnos_core::{Error, Result};
pub use identity_source::{
    IdentitySource, IdentitySourceAsset, IdentitySourceFile, SkipReason, SkippedFile,
    collect_identity_source, identity_source_from_files,
};
pub use project_db::{
    AutosaveProject, BranchSummary, CompactReport, FileStamp, LoadedProject, ProjectDatabase,
    ProjectHistoryView, SaveFailure, SaveFailureKind, VersionSummary, load_project, read_autosave,
    remove_project_files,
};
pub use project_folder::{create_project_folder, project_in_folder, sanitize_project_name};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
};
pub use website_export::{
    WrittenWebsiteExport, read_export_folder, write_export_into, write_export_named,
};
pub use website_snapshot::{
    WebsitePage, WebsiteSnapshot, new_import_folder, write_website_snapshot,
};

pub const PROJECT_FILE_EXTENSIONS: [&str; 1] = ["desegnos"];

pub(crate) fn io(error: impl std::fmt::Display) -> Error {
    Error(error.to_string())
}

pub(crate) fn ensure(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error(message.into()))
    }
}

pub fn write_text(path: &Path, text: &str) -> Result<()> {
    atomic_write(
        path,
        |file| file.write_all(text.as_bytes()).map_err(io),
        true,
    )
}

fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut File) -> Result<()>,
    backup: bool,
) -> Result<()> {
    atomic_write_unsynced(path, write, backup)?;
    sync_parent(path)
}

/// `atomic_write` without the directory fsync, so a batch of files in one directory pays for a
/// single one. The caller must sync the directory before the files are referenced.
fn atomic_write_unsynced(
    path: &Path,
    write: impl FnOnce(&mut File) -> Result<()>,
    backup: bool,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = parent.join(format!(".desegnos-{}.tmp", uuid::Uuid::new_v4()));
    let operation = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(io)?;
        write(&mut file)?;
        file.sync_all().map_err(io)?;
        drop(file);
        if backup && path.exists() {
            backup_file(path)?;
        }
        fs::rename(&temporary, path).map_err(io)
    })();
    match operation {
        Err(error) => {
            if let Err(cleanup) = fs::remove_file(&temporary) {
                if cleanup.kind() != std::io::ErrorKind::NotFound {
                    return Err(Error(format!(
                        "{error}; temporary file cleanup failed: {cleanup}"
                    )));
                }
            }
            Err(error)
        }
        result => result,
    }
}

fn backup_file(path: &Path) -> Result<()> {
    let previous = path.with_file_name(format!(
        "{}.previous",
        path.file_name()
            .ok_or_else(|| Error("Invalid filename.".into()))?
            .to_string_lossy()
    ));
    atomic_write(
        &previous,
        |backup| {
            std::io::copy(&mut File::open(path).map_err(io)?, backup).map_err(io)?;
            Ok(())
        },
        false,
    )
}

fn sync_parent(path: &Path) -> Result<()> {
    sync_directory(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    )
}

fn sync_directory(directory: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(directory).map_err(io)?.sync_all().map_err(io)?;
    // Portable Rust has no directory flush operation on Windows.
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}
