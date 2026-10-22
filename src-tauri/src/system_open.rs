//! OS opening with observable missing-launcher failures and a path/URL fallback.
use std::{ffi::OsStr, io};

pub fn open(target: impl AsRef<OsStr>) -> io::Result<()> {
    #[cfg(not(target_os = "linux"))]
    return open::that_detached(target);

    #[cfg(target_os = "linux")]
    {
        use desegni_storage::system_tools::ToolSearch;
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let search = ToolSearch::default();
        for template in open::commands(target) {
            let Some(tool) = template.get_program().to_str() else {
                continue;
            };
            let Ok(path) = search.resolve(tool) else {
                continue;
            };
            let Ok(mut child) = Command::new(path)
                .args(template.get_args())
                .env("PATH", search.path())
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
            "No working system opener is available.",
        ))
    }
}
