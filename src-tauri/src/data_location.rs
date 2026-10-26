//! Chooses the folder that holds Desegnos's own data and proves it is writable.
//!
//! Startup must not continue silently when that folder cannot be used: the editor would fail
//! at its first save. [`start`] resolves the folder, checks it, and asks for another one when
//! needed, before any editor state exists. [`directory`] answers the same question for
//! commands that run later.

use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, Runtime};

/// Overrides the data folder for one run, for tests and portable installs.
pub const ENVIRONMENT_VARIABLE: &str = "DESEGNOS_DATA_DIR";

/// The saved choice lives beside the app's own configuration, not inside the data folder,
/// so it is still readable when the data folder itself is unusable.
const SETTINGS_FILE: &str = "data-location.json";

const WRITE_REFUSED: &str = "Desegnos can't write to its data folder.";
const DISK_FULL: &str = "The disk holding Desegnos's data folder is full.";
const READ_ONLY: &str = "Desegnos's data folder is on a read-only disk.";
const UNUSABLE: &str = "Desegnos can't use its data folder.";

/// A folder Desegnos cannot use. The message is plain words followed by the folder alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem(String);

impl Problem {
    fn at(folder: &Path, reason: &str) -> Self {
        Self(format!("{reason}\n{}", folder.display()))
    }

    fn plain(reason: &str) -> Self {
        Self(reason.to_owned())
    }

    /// A folder the probe or an initializer could not use, keeping the operating system's reason.
    pub fn io(folder: &Path, error: &io::Error) -> Self {
        Self::at(folder, describe(error))
    }

    /// A startup failure that is not a filesystem error still names the folder it came from.
    pub fn error(folder: &Path, error: &(dyn std::error::Error + 'static)) -> Self {
        match error.downcast_ref::<io::Error>() {
            Some(error) => Self::io(folder, error),
            None => Self::unusable(folder),
        }
    }

    /// An initializer failed for a reason it described in its own words.
    pub fn unusable(folder: &Path) -> Self {
        Self::at(folder, UNUSABLE)
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Problem {}

/// The data folder chosen for this run, kept so later commands use the same folder.
pub struct DataDirectory(PathBuf);

impl DataDirectory {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// The data folder of the running editor. Commands only run after [`start`] succeeded, so the
/// managed choice is the answer; the platform default is a last resort before startup finishes.
pub fn directory<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.try_state::<DataDirectory>()
        .map(|state| state.0.clone())
        .unwrap_or_else(|| app.path().app_local_data_dir().unwrap_or_default())
}

/// Resolves the configured folder and checks it, asking for another one when it is unusable.
pub fn start<R: Runtime>(app: &AppHandle<R>) -> Result<DataDirectory, Problem> {
    let config = app.path().app_config_dir().ok();
    let default = app
        .path()
        .app_local_data_dir()
        .map_err(|_| Problem::plain(UNUSABLE))?;
    let folder = resolve_migrating(
        &default,
        config.as_deref(),
        app.path().app_data_dir().ok().as_deref(),
        app.path().app_cache_dir().ok().as_deref(),
        std::env::var_os(ENVIRONMENT_VARIABLE).as_deref(),
    )?;
    match probe(&folder) {
        Ok(()) => Ok(DataDirectory(folder)),
        Err(error) => recover(app, Problem::io(&folder, &error)),
    }
}

fn resolve_migrating(
    default: &Path,
    config: Option<&Path>,
    roaming: Option<&Path>,
    cache: Option<&Path>,
    environment: Option<&OsStr>,
) -> Result<PathBuf, Problem> {
    // Read the old saved choice before moving anything. On macOS config and data
    // share a folder, so migrating config blindly would also move overridden data.
    let old_config = config.map(|path| path.with_file_name("io.github.chelij.desegni"));
    let config_to_read = config
        .filter(|path| path.exists())
        .or(old_config.as_deref());
    let folder = choose(default, config_to_read, environment);
    if let Some(config) = config {
        if config != default || folder == default {
            crate::rename_migration::migrate_default(config)
                .map_err(|error| Problem::io(config, &error))?;
        }
    }
    if folder == default {
        crate::rename_migration::migrate_default(default)
            .map_err(|error| Problem::io(default, &error))?;
        // Windows also has a separate roaming data folder; Linux/macOS deduplicate
        // naturally because an already migrated destination is left untouched.
        if let Some(roaming) = roaming {
            crate::rename_migration::migrate_default(roaming)
                .map_err(|error| Problem::io(roaming, &error))?;
        }
    }
    if let Some(cache) = cache {
        crate::rename_migration::migrate_default(cache)
            .map_err(|error| Problem::io(cache, &error))?;
    }
    Ok(folder)
}

/// Asks for a usable folder after a storage failure. Returns the error if the user quits.
pub fn recover<R: Runtime>(app: &AppHandle<R>, initial: Problem) -> Result<DataDirectory, Problem> {
    let config = app.path().app_config_dir().ok();
    let mut problem = initial;
    loop {
        let Some(folder) = ask(app, &problem) else {
            return Err(problem);
        };
        match probe(&folder) {
            Ok(()) => {
                remember(config.as_deref(), &folder);
                return Ok(DataDirectory(folder));
            }
            Err(error) => problem = Problem::io(&folder, &error),
        }
    }
}

/// Proves the folder can hold Desegnos's own files: create it, write a new file, flush it to
/// disk, and remove it again.
pub fn probe(folder: &Path) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let name = format!(
        ".desegnos-write-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    );
    let path = folder.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let written = file.write_all(b"desegnos").and_then(|()| file.sync_all());
    drop(file);
    // Remove the probe even when writing failed; a leftover empty file is not a reason to quit.
    let removed = fs::remove_file(&path);
    written?;
    removed
}

/// The saved choice, then the platform default. The environment variable wins because it is
/// set deliberately for this run.
fn choose(default: &Path, config: Option<&Path>, environment: Option<&OsStr>) -> PathBuf {
    environment
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| config.and_then(saved))
        .map_or_else(|| default.to_owned(), absolute)
}

fn absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    match std::env::current_dir() {
        Ok(dir) => dir.join(path),
        Err(_) => path,
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    data_directory: PathBuf,
}

fn saved(config: &Path) -> Option<PathBuf> {
    let text = fs::read(config.join(SETTINGS_FILE)).ok()?;
    let saved: Saved = serde_json::from_slice(&text).ok()?;
    (!saved.data_directory.as_os_str().is_empty()).then_some(saved.data_directory)
}

/// Saves the chosen folder for the next start. A configuration folder that cannot be written
/// only means the choice lasts for this run, which is not worth another interruption.
fn remember(config: Option<&Path>, folder: &Path) {
    let Some(config) = config else {
        return;
    };
    let Ok(text) = serde_json::to_vec(&Saved {
        data_directory: folder.to_owned(),
    }) else {
        return;
    };
    let _ = fs::create_dir_all(config).and_then(|()| fs::write(config.join(SETTINGS_FILE), text));
}

fn ask<R: Runtime>(app: &AppHandle<R>, problem: &Problem) -> Option<PathBuf> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
    let choose_another = app
        .dialog()
        .message(problem.to_string())
        .title("Desegnos data folder")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Choose another folder…".to_owned(),
            "Quit".to_owned(),
        ))
        .blocking_show();
    if !choose_another {
        return None;
    }
    app.dialog()
        .file()
        .set_title("Choose a data folder")
        .blocking_pick_folder()?
        .into_path()
        .ok()
}

fn describe(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::PermissionDenied => WRITE_REFUSED,
        io::ErrorKind::StorageFull => DISK_FULL,
        io::ErrorKind::ReadOnlyFilesystem => READ_ONLY,
        _ => disk_error(error).unwrap_or(UNUSABLE),
    }
}

/// Raw numbers catch filesystems whose error arrives without the newer `ErrorKind` variants.
#[cfg(unix)]
fn disk_error(error: &io::Error) -> Option<&'static str> {
    match error.raw_os_error() {
        Some(28) => Some(DISK_FULL),
        Some(30) => Some(READ_ONLY),
        _ => None,
    }
}

#[cfg(not(unix))]
fn disk_error(_: &io::Error) -> Option<&'static str> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("desegnos-data-location-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn probe_writes_and_removes_its_file() {
        let temp = Temp::new();
        let folder = temp.0.join("data");
        probe(&folder).unwrap();
        let left: Vec<_> = fs::read_dir(&folder).unwrap().collect();
        assert!(left.is_empty(), "the probe left files behind");
    }

    #[test]
    fn probe_creates_missing_folders_and_survives_existing_files() {
        let temp = Temp::new();
        fs::write(temp.0.join("data"), "not a folder").unwrap();
        assert!(probe(&temp.0.join("data")).is_err());
        fs::remove_file(temp.0.join("data")).unwrap();
        probe(&temp.0.join("deeply/nested/data")).unwrap();
        assert!(temp.0.join("deeply/nested/data").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn probe_reports_an_unwritable_folder() {
        use std::os::unix::fs::PermissionsExt;
        let temp = Temp::new();
        let folder = temp.0.join("sealed");
        fs::create_dir(&folder).unwrap();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o500)).unwrap();
        // A privileged account (root, or a user with CAP_DAC_OVERRIDE) can still write here.
        let Err(error) = probe(&folder) else {
            return;
        };
        assert_eq!(
            Problem::io(&folder, &error).to_string(),
            format!("{WRITE_REFUSED}\n{}", folder.display())
        );
    }

    #[test]
    fn full_and_read_only_disks_get_their_own_words() {
        let folder = Path::new("/mnt/data");
        assert_eq!(
            Problem::io(folder, &io::Error::from(io::ErrorKind::StorageFull)).to_string(),
            format!("{DISK_FULL}\n/mnt/data")
        );
        assert_eq!(
            Problem::io(folder, &io::Error::from(io::ErrorKind::ReadOnlyFilesystem)).to_string(),
            format!("{READ_ONLY}\n/mnt/data")
        );
        assert_eq!(
            Problem::io(folder, &io::Error::from(io::ErrorKind::PermissionDenied)).to_string(),
            format!("{WRITE_REFUSED}\n/mnt/data")
        );
        assert_eq!(
            Problem::io(folder, &io::Error::from(io::ErrorKind::InvalidInput)).to_string(),
            format!("{UNUSABLE}\n/mnt/data")
        );
    }

    #[cfg(unix)]
    #[test]
    fn raw_disk_numbers_match_the_named_kinds() {
        let folder = Path::new("/mnt/data");
        assert_eq!(
            Problem::io(folder, &io::Error::from_raw_os_error(28)).to_string(),
            format!("{DISK_FULL}\n/mnt/data")
        );
        assert_eq!(
            Problem::io(folder, &io::Error::from_raw_os_error(30)).to_string(),
            format!("{READ_ONLY}\n/mnt/data")
        );
    }

    #[test]
    fn a_startup_error_keeps_the_folder() {
        let folder = Path::new("/home/someone/.local/share/desegnos");
        let io_error = io::Error::from(io::ErrorKind::PermissionDenied);
        assert!(
            Problem::error(folder, &io_error)
                .to_string()
                .ends_with(&format!("{WRITE_REFUSED}\n{}", folder.display()))
        );
        let other = "The recovery record could not be read.";
        assert_eq!(
            Problem::error(folder, &io::Error::other(other)).to_string(),
            format!("{UNUSABLE}\n{}", folder.display())
        );
    }

    #[test]
    fn startup_migrates_defaults_and_reads_a_legacy_saved_override_first() {
        let temp = Temp::new();
        let default = temp.0.join("data/io.github.chelij.desegnos");
        let config = temp.0.join("config/io.github.chelij.desegnos");
        let old_data = default.with_file_name("io.github.chelij.desegni");
        let old_config = config.with_file_name("io.github.chelij.desegni");
        fs::create_dir_all(&old_data).unwrap();
        fs::write(old_data.join("work"), "preserve").unwrap();
        let custom = temp.0.join("custom");
        remember(Some(&old_config), &custom);
        assert_eq!(
            resolve_migrating(&default, Some(&config), None, None, None).unwrap(),
            custom
        );
        assert!(old_data.join("work").exists());
        assert!(!default.exists());
        assert!(config.join(SETTINGS_FILE).exists());
        fs::remove_file(config.join(SETTINGS_FILE)).unwrap();
        assert_eq!(
            resolve_migrating(&default, Some(&config), None, None, None).unwrap(),
            default
        );
        assert!(default.join("work").exists());
        assert!(!old_data.exists());
    }

    #[test]
    fn environment_override_does_not_move_a_shared_config_data_folder() {
        let temp = Temp::new();
        let default = temp.0.join("io.github.chelij.desegnos");
        let old = default.with_file_name("io.github.chelij.desegni");
        fs::create_dir(&old).unwrap();
        let portable = temp.0.join("portable");
        assert_eq!(
            resolve_migrating(
                &default,
                Some(&default),
                Some(&default),
                None,
                Some(portable.as_os_str())
            )
            .unwrap(),
            portable
        );
        assert!(old.exists());
        assert!(!default.exists());
    }

    #[test]
    fn the_environment_wins_over_a_saved_choice() {
        let temp = Temp::new();
        let config = temp.0.join("config");
        let default = temp.0.join("default");
        let chosen = temp.0.join("chosen");
        assert_eq!(
            choose(&default, Some(&config), None),
            default,
            "no override uses the platform default"
        );

        remember(Some(&config), &chosen);
        assert_eq!(
            choose(&default, Some(&config), None),
            chosen,
            "a saved choice replaces the platform default"
        );
        assert_eq!(
            choose(
                &default,
                Some(&config),
                Some(OsStr::new(&temp.0.join("portable")))
            ),
            temp.0.join("portable"),
            "the environment variable replaces a saved choice"
        );
        assert_eq!(
            choose(&default, Some(&config), Some(OsStr::new(""))),
            chosen,
            "an empty variable is not an override"
        );
        assert_eq!(
            choose(&default, None, None),
            default,
            "an unreadable configuration folder keeps the default"
        );
    }

    #[test]
    fn a_broken_or_empty_saved_choice_is_ignored() {
        let temp = Temp::new();
        let config = temp.0.join("config");
        let default = temp.0.join("default");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join(SETTINGS_FILE), "{not json").unwrap();
        assert_eq!(choose(&default, Some(&config), None), default);
        fs::write(config.join(SETTINGS_FILE), r#"{"dataDirectory":""}"#).unwrap();
        assert_eq!(choose(&default, Some(&config), None), default);
    }

    #[test]
    fn a_relative_choice_becomes_absolute() {
        let temp = Temp::new();
        let default = temp.0.join("default");
        let chosen = choose(&default, None, Some(OsStr::new("portable-data")));
        assert!(chosen.is_absolute());
        assert!(chosen.ends_with("portable-data"));
    }
}
