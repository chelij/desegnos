//! Optional developer tools. Never source shell profiles or search the selected website.
use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn tool_label(tool: &str) -> &str {
    match tool {
        "node" => "Node.js",
        "hugo" => "Hugo",
        other => other,
    }
}

pub fn spawn_message(tool: &str, error: &io::Error) -> String {
    let tool = tool_label(tool);
    match error.kind() {
        io::ErrorKind::NotFound => format!("Needs {tool} to rebuild."),
        io::ErrorKind::PermissionDenied => format!("Allow {tool} to run before rebuilding."),
        _ => format!("Couldn't start {tool}. Check its installation."),
    }
}

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
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            directories: vec![],
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

    fn runnable(&self, tool: &str, root: &Path) -> Result<PathBuf, String> {
        let path = self.resolve(tool).map_err(|e| spawn_message(tool, &e))?;
        let mut child = Command::new(&path)
            .arg("--version")
            .current_dir(root)
            .env("PATH", self.path())
            .env("COREPACK_ENABLE_NETWORK", "0")
            .env("COREPACK_ENABLE_AUTO_PIN", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| spawn_message(tool, &e))?;
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(path),
                Ok(Some(_)) => {
                    return Err(format!(
                        "Needs a working {} installation to rebuild.",
                        tool_label(tool)
                    ));
                }
                Ok(None) if start.elapsed() < Duration::from_secs(2) => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{} didn't respond. Check its installation.",
                        tool_label(tool)
                    ));
                }
            }
        }
    }

    pub fn build_command(&self, root: &Path, args: &[String]) -> Result<Command, String> {
        let tool = args.first().ok_or("No build command found.")?;
        if matches!(tool.as_str(), "npm" | "pnpm" | "yarn") {
            self.runnable("node", root)?;
        }
        let executable = self.runnable(tool, root)?;
        if tool == "node"
            && let Some(script) = args.get(1)
        {
            // Eleventy is a project dependency. Read its declared entry point;
            // don't execute project code just to display a folder choice.
            let script = root.join(script);
            if !script.is_file() {
                return Err("Needs Eleventy to rebuild.".into());
            }
            fs::File::open(script).map_err(|e| spawn_message("Eleventy", &e))?;
        }
        let mut command = Command::new(executable);
        command.args(&args[1..]).env("PATH", self.path());
        Ok(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_tool_and_spawn_errors_are_plain() {
        let search = ToolSearch {
            directories: vec![],
        };
        assert_eq!(
            search
                .build_command(Path::new("."), &["npm".into()])
                .unwrap_err(),
            "Needs Node.js to rebuild."
        );
        assert_eq!(
            spawn_message("hugo", &io::Error::from(io::ErrorKind::NotFound)),
            "Needs Hugo to rebuild."
        );
        assert_eq!(
            spawn_message("node", &io::Error::from(io::ErrorKind::PermissionDenied)),
            "Allow Node.js to run before rebuilding."
        );
        assert_eq!(
            spawn_message("npm", &io::Error::from(io::ErrorKind::Other)),
            "Couldn't start npm. Check its installation."
        );
    }

    #[cfg(unix)]
    #[test]
    fn tools_must_be_executable_and_successfully_respond() {
        use std::os::unix::fs::PermissionsExt;
        let root = env::temp_dir().join(format!("desegnos-tools-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let search = ToolSearch {
            directories: vec![root.clone()],
        };
        for tool in ["node", "npm"] {
            fs::write(root.join(tool), "#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(root.join(tool), fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(
            search
                .build_command(&root, &["npm".into(), "run".into(), "build".into()])
                .is_ok()
        );
        let entry = "node_modules/@11ty/eleventy/custom-entry.cjs";
        let args = ["node".into(), entry.into()];
        assert_eq!(
            search.build_command(&root, &args).unwrap_err(),
            "Needs Eleventy to rebuild."
        );
        fs::create_dir_all(root.join("node_modules/@11ty/eleventy")).unwrap();
        fs::write(
            root.join(entry),
            "throw new Error('must not execute during detection');",
        )
        .unwrap();
        let command = search.build_command(&root, &args).unwrap();
        assert_eq!(command.get_program(), root.join("node").as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [std::ffi::OsStr::new(entry)]
        );
        fs::remove_file(root.join("npm")).unwrap();
        assert_eq!(
            search.build_command(&root, &["npm".into()]).unwrap_err(),
            "Needs npm to rebuild."
        );
        fs::set_permissions(root.join("node"), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            search
                .runnable("node", &root)
                .unwrap_err()
                .starts_with("Allow Node.js")
        );
        fs::set_permissions(root.join("node"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.join("node"), "#!/bin/sh\nexit 1\n").unwrap();
        assert!(search.runnable("node", &root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn windows_resolves_and_probes_package_manager_cmd_files() {
        let root = env::temp_dir().join(format!("desegnos-tools-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        for tool in ["node", "npm"] {
            fs::write(root.join(format!("{tool}.cmd")), "@exit /b 0\r\n").unwrap();
        }
        let search = ToolSearch {
            directories: vec![root.clone()],
        };
        assert!(search.build_command(&root, &["npm".into()]).is_ok());
        fs::remove_file(root.join("npm.cmd")).unwrap();
        assert_eq!(
            search.build_command(&root, &["npm".into()]).unwrap_err(),
            "Needs npm to rebuild."
        );
        fs::remove_dir_all(root).unwrap();
    }
}
