//! Small helpers shared by the native services: private directories, regular-file checks,
//! size-limited JSON reads and writes, and poison-tolerant locking.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::Path,
    sync::{Mutex, MutexGuard, PoisonError},
};

pub type NativeResult<T> = Result<T, String>;

/// Lock shared state even if another thread panicked while holding it. Refusing to lock would
/// leave a running job with no recorded outcome and every command on that service failing.
pub trait LockExt<T> {
    fn locked(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Make sure `path` is a real directory (not a link), creating it private to the user.
pub fn private_directory(path: &Path, noun: &str) -> NativeResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            return if metadata.is_dir() && !metadata.file_type().is_symlink() {
                Ok(())
            } else {
                Err(format!(
                    "The {noun} folder is not a regular directory. The original data was preserved."
                ))
            };
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(message(error)),
    }
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(message)
}

pub fn regular_directory(path: &Path, noun: &str) -> NativeResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(message)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!("The {noun} folder is not a regular directory."));
    }
    Ok(())
}

/// A regular file with no other names, so a record cannot alias something else.
pub fn regular_file(path: &Path, noun: &str) -> NativeResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(message)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "A {noun} is not a regular file. The original was preserved."
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err(format!(
                "A {noun} has unexpected filesystem aliases. The original was preserved."
            ));
        }
    }
    Ok(())
}

pub fn read_bytes(path: &Path, limit: u64, noun: &str) -> NativeResult<Vec<u8>> {
    regular_file(path, noun)?;
    let file = File::open(path).map_err(message)?;
    let too_large = || format!("A {noun} exceeds its size limit. The original was preserved.");
    if file.metadata().map_err(message)?.len() > limit {
        return Err(too_large());
    }
    let mut bytes = vec![];
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(message)?;
    if bytes.len() as u64 > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

pub fn read_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    limit: u64,
    noun: &str,
) -> NativeResult<T> {
    let bytes = read_bytes(path, limit, noun)?;
    serde_json::from_slice(&bytes).map_err(|_| {
        format!("A {noun} is damaged or uses an unsupported format. The original was preserved.")
    })
}

pub fn write_json(path: &Path, value: &impl Serialize, limit: u64, noun: &str) -> NativeResult<()> {
    if path.exists() {
        regular_file(path, noun)?;
    }
    let bytes = serde_json::to_string(value).map_err(message)?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "The complete {noun} exceeds its local storage limit."
        ));
    }
    desegnos_storage::write_text(path, &bytes).map_err(message)
}

/// Open (creating if needed) a lock file readable only by the user.
pub fn open_lock_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
