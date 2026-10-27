//! Launcher-order checks use only fake executables in an isolated directory.
#[allow(dead_code)]
#[path = "../src/system_open.rs"]
mod system_open;

use desegnos_storage::system_tools::ToolSearch;
use std::{
    ffi::OsStr,
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

struct Fixture {
    directory: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("desegnos-open-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        Self {
            log: directory.join("launchers.log"),
            directory,
        }
    }

    fn launcher(&self, name: &str, status: i32) {
        let path = self.directory.join(name);
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$0\" \"$@\" >> '{}'\nexit {status}\n",
                self.log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn open(&self, url: &str) -> io::Result<()> {
        let search = ToolSearch::from_directories([self.directory.clone()]);
        system_open::open_with_search(OsStr::new(url), &search)
    }

    fn log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn ran(&self, launcher: &str) -> bool {
        self.log()
            .lines()
            .any(|line| Path::new(line).file_name() == Some(OsStr::new(launcher)))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn the_first_successful_launcher_runs_and_later_ones_do_not() {
    let fixture = Fixture::new();
    fixture.launcher("xdg-open", 0);
    fixture.launcher("gio", 0);
    fixture.open("http://127.0.0.1:9/site/").expect("opens");
    assert!(fixture.ran("xdg-open"));
    assert!(!fixture.ran("gio"));
    assert!(fixture.log().contains("http://127.0.0.1:9/site/"));
}

#[test]
fn a_missing_launcher_falls_through_to_the_next_one() {
    let fixture = Fixture::new();
    fixture.launcher("gio", 0);
    fixture.open("http://127.0.0.1:9/site/").expect("opens");
    assert!(fixture.ran("gio"));
    assert!(!fixture.ran("xdg-open"));
}

#[test]
fn failing_launchers_produce_an_error_that_names_the_target() {
    let fixture = Fixture::new();
    fixture.launcher("xdg-open", 7);
    fixture.launcher("gio", 3);
    let error = fixture
        .open("http://127.0.0.1:9/site/")
        .expect_err("no launcher works");
    assert!(error.to_string().contains("http://127.0.0.1:9/site/"));
    assert!(fixture.ran("xdg-open"));
    assert!(fixture.ran("gio"));
}
