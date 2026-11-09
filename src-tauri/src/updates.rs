//! Signed updates through Tauri's configured updater endpoint. The editor intentionally
//! checks quietly after startup as well as on request. Installation always requires a
//! user action and the same unsaved-work confirmation as closing the window.

use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::release::REPOSITORY;

pub const UPDATE_PROGRESS_EVENT: &str = "desegnos://update-progress";
const FAILED: &str = "Couldn't check for updates. Check your connection and try again.";
const INSTALL_FAILED: &str = "Couldn't install the update. Try again or download it from GitHub.";

#[derive(Default)]
pub struct UpdateState(pub tokio::sync::Mutex<Option<Update>>);

#[derive(ts_rs::TS, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current: String,
    pub latest: Option<Release>,
    pub newer: bool,
    pub install_supported: bool,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize)]
#[ts(rename = "UpdateRelease")]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub notes: Option<String>,
    pub url: String,
}

#[derive(ts_rs::TS, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub percent: Option<u8>,
    pub installing: bool,
}

fn install_supported(app: &tauri::AppHandle) -> bool {
    // Self-update is offered only for AppImage on Linux. Package-managed installs use the
    // release page instead of overwriting a binary owned by the package manager.
    #[cfg(target_os = "linux")]
    {
        app.env().appimage.is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        cfg!(any(target_os = "macos", target_os = "windows"))
    }
}

fn download_percent(downloaded: u64, total: Option<u64>) -> Option<u8> {
    total
        .filter(|total| *total > 0)
        .map(|total| ((u128::from(downloaded) * 100 / u128::from(total)).min(100)) as u8)
}

#[tauri::command]
pub async fn check_for_updates(
    app: tauri::AppHandle,
    state: tauri::State<'_, UpdateState>,
) -> Result<UpdateCheck, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command
        .run_async(async {
            let mut pending = state
                .0
                .try_lock()
                .map_err(|_| "An update is already in progress.")?;
            // Retain metadata even if this manifest has no installer for our target.
            let metadata = Arc::new(Mutex::new(None));
            let captured = metadata.clone();
            let before_exit = app.clone();
            let updater = app
                .updater_builder()
                .timeout(Duration::from_secs(10))
                .on_before_exit(move || crate::shutdown::finish(&before_exit))
                .version_comparator(move |current, release| {
                    let newer = release.version > current;
                    *captured.lock().unwrap() = Some((
                        Release {
                            version: release.version.to_string(),
                            notes: release.notes,
                            url: format!("{REPOSITORY}/releases/latest"),
                        },
                        newer,
                    ));
                    newer
                })
                .build()
                .map_err(|_| FAILED)?;
            let update = match updater.check().await {
                Ok(update) => update,
                Err(
                    tauri_plugin_updater::Error::TargetNotFound(_)
                    | tauri_plugin_updater::Error::TargetsNotFound(_),
                ) => None,
                Err(_) => return Err(FAILED.to_owned()),
            };
            let metadata = metadata.lock().unwrap().take();
            let (latest, newer) = match metadata {
                Some((release, newer)) => (Some(release), newer),
                None => (None, false), // e.g. an endpoint returning HTTP 204
            };
            let supported = install_supported(&app) && update.is_some();
            *pending = if supported { update } else { None };
            Ok(UpdateCheck {
                current: app.package_info().version.to_string(),
                latest,
                newer,
                install_supported: supported,
            })
        })
        .await
}

/// The editor confirms before invoking: Windows installers can exit the app themselves.
#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, UpdateState>,
    version: String,
) -> Result<(), crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command
        .run_async(async {
            let pending = state
                .0
                .try_lock()
                .map_err(|_| "An update is already in progress.")?;
            let update = pending
                .as_ref()
                .filter(|update| update.version == version)
                .ok_or("Check for updates again before installing.")?;
            if !install_supported(&app) {
                return Err("Download this update from GitHub.".to_owned());
            }
            let mut downloaded = 0u64;
            let mut previous = None;
            update
                .download_and_install(
                    |chunk, total| {
                        downloaded = downloaded.saturating_add(chunk as u64);
                        let percent = download_percent(downloaded, total);
                        if percent != previous {
                            previous = percent;
                            let _ = app.emit_to(
                                "editor",
                                UPDATE_PROGRESS_EVENT,
                                UpdateProgress {
                                    percent,
                                    installing: false,
                                },
                            );
                        }
                    },
                    || {
                        let _ = app.emit_to(
                            "editor",
                            UPDATE_PROGRESS_EVENT,
                            UpdateProgress {
                                percent: None,
                                installing: true,
                            },
                        );
                    },
                )
                .await
                .map_err(|_| INSTALL_FAILED)?;
            crate::shutdown::finish(&app);
            app.restart();
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::download_percent;

    #[test]
    fn download_progress_handles_missing_lengths_and_overflow() {
        assert_eq!(download_percent(42, Some(100)), Some(42));
        assert_eq!(download_percent(101, Some(100)), Some(100));
        assert_eq!(download_percent(u64::MAX, Some(u64::MAX)), Some(100));
        assert_eq!(download_percent(42, None), None);
        assert_eq!(download_percent(42, Some(0)), None);
    }
}
