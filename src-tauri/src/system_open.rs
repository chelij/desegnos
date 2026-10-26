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
fn open_with(target: &OsStr, search: &desegnos_storage::system_tools::ToolSearch) -> io::Result<()> {
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
        format!(
            "No working system opener could open {}.",
            target.to_string_lossy()
        ),
    ))
}

/// Test-only seam so launcher tests can restrict the search to fake executables.
#[cfg(test)]
#[allow(dead_code)]
pub fn open_with_search(
    target: &OsStr,
    search: &desegnos_storage::system_tools::ToolSearch,
) -> io::Result<()> {
    open_with(target, search)
}
