//! Move only platform-default app folders. Overrides are resolved by data_location first.
use std::{fs, io, path::Path};

const OLD_IDENTIFIER: &str = "io.github.chelij.desegni";
const NEW_IDENTIFIER: &str = "io.github.chelij.desegnos";

pub fn migrate_default(destination: &Path) -> io::Result<()> {
    if destination
        .file_name()
        .is_none_or(|name| name != NEW_IDENTIFIER)
    {
        return Ok(());
    }
    move_directory(&destination.with_file_name(OLD_IDENTIFIER), destination)
}

fn exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn move_directory(source: &Path, destination: &Path) -> io::Result<()> {
    move_with(source, destination, |from, to| fs::rename(from, to))
}

fn move_with(
    source: &Path,
    destination: &Path,
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    if exists(destination)? || !exists(source)? {
        return Ok(());
    }
    if !fs::symlink_metadata(source)?.file_type().is_dir() {
        return Err(io::Error::other(
            "The old app folder is not a regular directory.",
        ));
    }
    match rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
            copy_directory(source, destination)
        }
        Err(error) => Err(error),
    }
}

fn copy_directory(source: &Path, destination: &Path) -> io::Result<()> {
    // Publish the complete copy at once. A failed copy cannot make the next startup
    // mistake a partial destination for a completed migration. Never delete old data.
    let staging =
        destination.with_file_name(format!(".desegnos-migration-{}", uuid::Uuid::new_v4()));
    let result = copy_tree(source, &staging).and_then(|()| fs::rename(&staging, destination));
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    fs::write(
        source.join("MOVED.txt"),
        format!(
            "Desegnos now uses {}.\nThis folder is the retained pre-migration copy; new changes are saved at the new location.\n",
            destination.display()
        ),
    )
}

fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else if kind.is_symlink() {
            copy_link(&entry.path(), &target)?;
        } else {
            return Err(io::Error::other(
                "The old app folder contains an unsupported special file.",
            ));
        }
    }
    fs::set_permissions(destination, fs::metadata(source)?.permissions())
}

#[cfg(unix)]
fn copy_link(source: &Path, destination: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, destination)
}

#[cfg(windows)]
fn copy_link(source: &Path, destination: &Path) -> io::Result<()> {
    let target = fs::read_link(source)?;
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("desegnos-migration-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn source(&self) -> PathBuf {
            self.0.join(OLD_IDENTIFIER)
        }
        fn destination(&self) -> PathBuf {
            self.0.join(NEW_IDENTIFIER)
        }
        fn seed(&self) {
            fs::create_dir_all(self.source().join("nested")).unwrap();
            fs::write(self.source().join("nested/data"), "original").unwrap();
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn default_directory_moves_once_and_preserves_existing_destination() {
        let temp = Temp::new();
        migrate_default(&temp.destination()).unwrap(); // neither exists
        temp.seed();
        migrate_default(&temp.destination()).unwrap();
        assert!(!temp.source().exists());
        assert_eq!(
            fs::read_to_string(temp.destination().join("nested/data")).unwrap(),
            "original"
        );
        temp.seed();
        fs::write(temp.source().join("nested/data"), "old").unwrap();
        migrate_default(&temp.destination()).unwrap();
        assert_eq!(
            fs::read_to_string(temp.destination().join("nested/data")).unwrap(),
            "original"
        );
        assert!(temp.source().exists());
    }

    #[test]
    fn cross_device_copy_is_complete_and_retains_the_old_folder_with_a_note() {
        let temp = Temp::new();
        temp.seed();
        move_with(&temp.source(), &temp.destination(), |_, _| {
            Err(io::ErrorKind::CrossesDevices.into())
        })
        .unwrap();
        assert_eq!(
            fs::read(temp.source().join("nested/data")).unwrap(),
            fs::read(temp.destination().join("nested/data")).unwrap()
        );
        assert!(
            fs::read_to_string(temp.source().join("MOVED.txt"))
                .unwrap()
                .contains(&temp.destination().display().to_string())
        );
        migrate_default(&temp.destination()).unwrap();
    }

    #[test]
    fn other_rename_errors_do_not_copy_or_remove_anything() {
        let temp = Temp::new();
        temp.seed();
        assert!(
            move_with(&temp.source(), &temp.destination(), |_, _| Err(
                io::ErrorKind::PermissionDenied.into()
            ))
            .is_err()
        );
        assert!(temp.source().join("nested/data").exists());
        assert!(!temp.destination().exists());
    }

    #[cfg(unix)]
    #[test]
    fn failed_copy_never_publishes_a_partial_destination() {
        let temp = Temp::new();
        temp.seed();
        std::os::unix::net::UnixListener::bind(temp.source().join("socket")).unwrap();
        assert!(copy_directory(&temp.source(), &temp.destination()).is_err());
        assert!(!temp.destination().exists());
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
    }
}
