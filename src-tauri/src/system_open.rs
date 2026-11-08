//! OS opening with observable missing-launcher failures and a path/URL fallback.
use std::{ffi::OsStr, io};

pub fn open(target: impl AsRef<OsStr>) -> io::Result<()> {
    #[cfg(not(target_os = "linux"))]
    return open::that_detached(target);

    #[cfg(target_os = "linux")]
    {
        use desegnos_storage::system_tools::ToolSearch;
        open_with(target.as_ref(), &ToolSearch::default())
    }
}

#[cfg(target_os = "linux")]
fn open_with(
    target: &OsStr,
    search: &desegnos_storage::system_tools::ToolSearch,
) -> io::Result<()> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for template in open::commands(target) {
        let Some(tool) = template.get_program().to_str() else {
            continue;
        };
        // Never skip an installed launcher just because the storage search missed it.
        // Falling back to the program name lets the inherited PATH resolve it.
        let program = search
            .resolve(tool)
            .unwrap_or_else(|_| std::path::PathBuf::from(tool));
        let Ok(mut child) = Command::new(program)
            .args(template.get_args())
            .env("PATH", search.path()?)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(_)) => break, // Try the next installed OS launcher.
                Ok(None) if start.elapsed() < Duration::from_secs(2) => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Ok(None) => {
                    // Some launchers wait for the browser. Reap without blocking the UI.
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Ok(());
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "No working system opener could open {}.",
            target.to_string_lossy()
        ),
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    //! Launcher-order checks use only fake executables in an isolated directory.
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
            super::open_with(OsStr::new(url), &search)
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
}
