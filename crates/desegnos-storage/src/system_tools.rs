//! Optional developer tools. Never source shell profiles or search the selected website.
use std::{env, ffi::OsString, fs, io, path::PathBuf};

#[derive(Clone)]
pub struct ToolSearch {
    directories: Vec<PathBuf>,
}
impl Default for ToolSearch {
    fn default() -> Self {
        let directories: Vec<_> = env::var_os("PATH")
            .map(|path| {
                env::split_paths(&path)
                    .filter(|p| p.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        // GUI launches often omit shell-managed tools. These are optional candidates,
        // never requirements; inherited absolute PATH entries retain precedence.
        #[cfg(unix)]
        let directories = {
            let mut directories = directories;
            directories
                .extend(["/usr/local/bin", "/usr/bin", "/bin", "/snap/bin"].map(PathBuf::from));
            #[cfg(target_os = "macos")]
            directories.push(PathBuf::from("/opt/homebrew/bin"));
            if let Some(home) = env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
            {
                directories.extend(
                    [
                        ".local/bin",
                        ".volta/bin",
                        ".asdf/shims",
                        ".local/share/mise/shims",
                        ".bun/bin",
                    ]
                    .map(|p| home.join(p)),
                );
                let nvm = env::var_os("NVM_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".nvm"));
                let fnm = env::var_os("FNM_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".local/share/fnm"));
                for (root, suffix) in [
                    (nvm.join("versions/node"), "bin"),
                    (fnm.join("node-versions"), "installation/bin"),
                ] {
                    if !root.is_absolute() {
                        continue;
                    }
                    if let Ok(entries) = fs::read_dir(root) {
                        let mut versions: Vec<_> =
                            entries.flatten().map(|e| e.path().join(suffix)).collect();
                        versions.sort();
                        directories.extend(versions.into_iter().rev());
                    }
                }
            }
            directories
        };
        Self { directories }
    }
}
impl ToolSearch {
    /// Search exactly these directories. Callers that already know where to look
    /// (such as tests with controlled launchers) avoid the inherited PATH.
    pub fn from_directories(directories: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            directories: directories.into_iter().collect(),
        }
    }

    pub fn path(&self) -> OsString {
        env::join_paths(&self.directories).unwrap_or_default()
    }

    pub fn resolve(&self, tool: &str) -> io::Result<PathBuf> {
        #[cfg(unix)]
        let mut denied = false;
        #[cfg(not(unix))]
        let denied = false;
        for directory in &self.directories {
            #[cfg(windows)]
            let names = [
                format!("{tool}.exe"),
                format!("{tool}.cmd"),
                format!("{tool}.bat"),
                tool.into(),
            ];
            #[cfg(not(windows))]
            let names = [tool.to_string()];
            for name in names {
                let path = directory.join(name);
                if !path.is_file() {
                    continue;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if fs::metadata(&path)?.permissions().mode() & 0o111 == 0 {
                        denied = true;
                        continue;
                    }
                }
                return Ok(path);
            }
        }
        Err(io::Error::from(if denied {
            io::ErrorKind::PermissionDenied
        } else {
            io::ErrorKind::NotFound
        }))
    }
}
