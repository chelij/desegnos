use desegni_storage::{WebsiteFolder, detect_website_folder};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSelection {
    pub id: Uuid,
    pub name: String,
    pub website: Option<WebsiteFolder>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStatus {
    pub running: bool,
    pub line: String,
    pub error: Option<String>,
    pub folder: Option<FolderSelection>,
}
struct Selection {
    path: PathBuf,
    status: Arc<Mutex<BuildStatus>>,
    stop: Arc<AtomicBool>,
}
#[derive(Default)]
pub struct WebsiteImports {
    selections: BTreeMap<Uuid, Selection>,
}
impl WebsiteImports {
    pub fn select(&mut self, path: PathBuf) -> Result<FolderSelection, String> {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let website = detect_website_folder(&path).map_err(|e| e.to_string())?;
        let id = Uuid::new_v4();
        let view = FolderSelection {
            id,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            website,
        };
        if self.selections.len() >= 16 {
            let oldest = self
                .selections
                .iter()
                .find(|(_, s)| s.status.lock().is_ok_and(|s| !s.running))
                .map(|(id, _)| *id)
                .ok_or("Stop a site build first.")?;
            self.selections.remove(&oldest);
        }
        self.selections.insert(
            id,
            Selection {
                path,
                status: Arc::default(),
                stop: Arc::default(),
            },
        );
        Ok(view)
    }
    pub fn path(&self, id: Uuid) -> Result<PathBuf, String> {
        let selected = self
            .selections
            .get(&id)
            .ok_or("Choose the website folder again.")?;
        if selected.status.lock().map_err(|e| e.to_string())?.running {
            return Err("Stop the build before opening this website.".into());
        }
        Ok(selected.path.clone())
    }
}
#[tauri::command]
pub async fn pick_website_folder(
    app: AppHandle,
    state: State<'_, Mutex<WebsiteImports>>,
) -> Result<Option<FolderSelection>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        crate::file_dialogs::picked(
            app.dialog()
                .file()
                .set_title("Choose website or brand folder")
                .blocking_pick_folder(),
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    picked
        .map(|p| {
            state
                .lock()
                .map_err(|e| e.to_string())?
                .select(p.into_path().map_err(|e| e.to_string())?)
        })
        .transpose()
}
#[tauri::command]
pub fn website_build(
    state: State<'_, Mutex<WebsiteImports>>,
    folder_id: Uuid,
    action: String,
) -> Result<BuildStatus, String> {
    let service = state.lock().map_err(|e| e.to_string())?;
    let selected = service
        .selections
        .get(&folder_id)
        .ok_or("Choose the website folder again.")?;
    if action == "stop" {
        selected.stop.store(true, Ordering::Relaxed);
    } else if action == "start" {
        let mut status = selected.status.lock().map_err(|e| e.to_string())?;
        if status.running {
            return Ok(status.clone());
        }
        let detected = detect_website_folder(&selected.path)
            .map_err(|e| e.to_string())?
            .ok_or("No website found.")?;
        if let Some(reason) = &detected.build_unavailable {
            return Err(reason.clone());
        }
        if let Some(command) = &detected.install_command {
            return Err(format!(
                "Run {command} in this folder first: {}",
                detected.path.display()
            ));
        }
        let command = detected
            .build_command
            .ok_or("No build command found. Build the site in this folder first.")?;
        *status = BuildStatus {
            running: true,
            line: format!("Running {}", command.join(" ")),
            error: None,
            folder: None,
        };
        selected.stop.store(false, Ordering::Relaxed);
        let (path, status, stop) = (
            selected.path.clone(),
            selected.status.clone(),
            selected.stop.clone(),
        );
        std::thread::spawn(move || run_build(folder_id, path, command, status, stop));
    } else if action != "status" {
        return Err("Unknown build action.".into());
    }
    let result = selected.status.lock().map_err(|e| e.to_string())?.clone();
    Ok(result)
}
fn run_build(
    id: Uuid,
    path: PathBuf,
    args: Vec<String>,
    status: Arc<Mutex<BuildStatus>>,
    stop: Arc<AtomicBool>,
) {
    let result = (|| -> Result<(), String> {
        let mut command =
            desegni_storage::system_tools::ToolSearch::default().build_command(&path, &args)?;
        command
            .current_dir(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("CI", "1")
            .env("npm_config_offline", "true")
            .env("COREPACK_ENABLE_NETWORK", "0");
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|e| desegni_storage::system_tools::spawn_message(&args[0], &e))?;
        let readers: Vec<Box<dyn Read + Send>> = vec![
            Box::new(child.stdout.take().expect("stdout")),
            Box::new(child.stderr.take().expect("stderr")),
        ];
        for reader in readers {
            let status = status.clone();
            std::thread::spawn(move || {
                // Limit each read, including a tool that emits an unbroken line.
                let mut reader = BufReader::new(reader);
                loop {
                    let mut bytes = vec![];
                    match reader.by_ref().take(4096).read_until(b'\n', &mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let line = String::from_utf8_lossy(&bytes)
                        .trim()
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(500)
                        .collect::<String>();
                    if !line.is_empty()
                        && let Ok(mut state) = status.lock()
                    {
                        state.line = line;
                    }
                }
            });
        }
        let start = Instant::now();
        loop {
            if stop.load(Ordering::Relaxed) || start.elapsed() >= Duration::from_secs(300) {
                #[cfg(unix)]
                {
                    use rustix::process::{Pid, Signal, kill_process_group};
                    // The unreaped child owns this process group; no external kill program.
                    let _ = kill_process_group(Pid::from_child(&child), Signal::KILL);
                }
                // Uses the native process handle on Windows, even with no taskkill/PATH.
                let _ = child.kill();
                let _ = child.wait();
                return Err(if stop.load(Ordering::Relaxed) {
                    "Build stopped."
                } else {
                    "Build timed out after 5 minutes."
                }
                .into());
            }
            if let Some(exit) = child
                .try_wait()
                .map_err(|_| "Couldn't read the build status.")?
            {
                if !exit.success() {
                    return Err(format!("Build failed ({exit})."));
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    })();
    let folder = detect_website_folder(&path)
        .ok()
        .flatten()
        .map(|website| FolderSelection {
            id,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            website: Some(website),
        });
    if let Ok(mut state) = status.lock() {
        state.running = false;
        state.error = result.err();
        state.folder = folder;
        if state.error.is_none() {
            state.line = "Build complete.".into();
        }
    }
}
