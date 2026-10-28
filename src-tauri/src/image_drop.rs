//! File access starts only at an operating-system drop, never at a renderer-supplied path.
//! Drag positions stay in the physical pixels the window reports; the canvas converts them.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use std::{fs::File, io::Read, path::Path};
use tauri::{DragDropEvent, Emitter, Window, WindowEvent};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Picture {
    name: String,
    mime: &'static str,
    data: String,
}
#[derive(Clone, Serialize)]
struct Position {
    x: f64,
    y: f64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImageDrag {
    phase: &'static str,
    id: String,
    position: Option<Position>,
    picture: Option<Picture>,
    files: Vec<Picture>,
    file_errors: Vec<String>,
    message: Option<&'static str>,
}
fn read_picture(path: &Path) -> Result<Picture, &'static str> {
    if !path.is_file() {
        return Err("Drop a picture file onto an image on the page.");
    }
    let mut file = File::open(path).map_err(|_| "The picture could not be opened.")?;
    let meta = file
        .metadata()
        .map_err(|_| "The picture could not be opened.")?;
    if !meta.is_file() {
        return Err("Drop a picture file onto an image on the page.");
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(12)
        .read_to_end(&mut bytes)
        .map_err(|_| "The picture could not be read.")?;
    // The frontend decodes the actual bytes and checks dimensions before applying.
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        return Err("Choose a PNG, JPEG, WebP or GIF picture.");
    };
    file.read_to_end(&mut bytes)
        .map_err(|_| "The picture could not be read.")?;
    Ok(Picture {
        name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Picture")
            .into(),
        mime,
        data: STANDARD.encode(bytes),
    })
}
fn read_request_file(path: &Path) -> Result<Picture, &'static str> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid filename.")?;
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
        return read_picture(path);
    }
    let mime = match ext.as_str() {
        "zip" => "application/zip",
        "txt" | "md" | "csv" | "json" | "html" | "htm" => "text/plain",
        _ => return Err("Unsupported attachment."),
    };
    let file = File::open(path).map_err(|_| "The file could not be opened.")?;
    if !file.metadata().is_ok_and(|meta| meta.is_file()) {
        return Err("Choose a file.");
    }
    let limit = desegnos_storage::request_zip::MAX_ARCHIVE_BYTES;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The file could not be read.")?;
    if bytes.len() > limit {
        return Err("The file exceeds the 32 MiB limit.");
    }
    Ok(Picture {
        name: name.into(),
        mime,
        data: STANDARD.encode(bytes),
    })
}
pub fn handle(window: &Window, event: &WindowEvent) {
    if window.label() != "editor" {
        return;
    }
    let WindowEvent::DragDrop(event) = event else {
        return;
    };
    let position = match event {
        DragDropEvent::Enter { position, .. }
        | DragDropEvent::Over { position }
        | DragDropEvent::Drop { position, .. } => {
            // Wry reports physical pixels on Windows but logical points on macOS and Linux.
            // Normalize to physical pixels; the canvas divides by the webview pixel ratio.
            #[cfg(target_os = "windows")]
            let scale = 1.0;
            #[cfg(not(target_os = "windows"))]
            let scale = match window.scale_factor() {
                Ok(scale) if scale > 0.0 => scale,
                _ => return,
            };
            Some(Position {
                x: position.x * scale,
                y: position.y * scale,
            })
        }
        _ => None,
    };
    let id = uuid::Uuid::new_v4().to_string();
    let phase = match event {
        DragDropEvent::Drop { .. } => "pending",
        DragDropEvent::Leave => "leave",
        _ => "over",
    };
    let _ = window.emit(
        "desegnos://image-drag",
        ImageDrag {
            phase,
            id: id.clone(),
            position,
            picture: None,
            files: vec![],
            file_errors: vec![],
            message: None,
        },
    );
    if let DragDropEvent::Drop { paths, .. } = event {
        let paths = paths.clone();
        let window = window.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut files = Vec::new();
            let mut file_errors = Vec::new();
            for path in &paths {
                match read_request_file(path) {
                    Ok(file) => files.push(file),
                    Err(error) => file_errors.push(format!(
                        "{}: {error}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )),
                }
            }
            let result = files
                .first()
                .filter(|file| file.mime.starts_with("image/"))
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| {
                    paths
                        .first()
                        .ok_or("Drop a picture file onto an image on the page.")
                        .and_then(|path| read_picture(path))
                });
            let (picture, message) = match result {
                Ok(picture) => (Some(picture), None),
                Err(message) => (None, Some(message)),
            };
            let _ = window.emit(
                "desegnos://image-drag",
                ImageDrag {
                    phase: "drop",
                    id,
                    position: None,
                    picture,
                    files,
                    file_errors,
                    message,
                },
            );
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dropped_zips_and_text_reach_requests_and_zip_reads_are_bounded() {
        let root =
            std::env::temp_dir().join(format!("desegnos-request-drop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("site.ZIP");
        std::fs::write(&path, b"PK\x03\x04archive").unwrap();
        let file = read_request_file(&path).unwrap();
        assert_eq!(file.mime, "application/zip");
        assert_eq!(STANDARD.decode(file.data).unwrap(), b"PK\x03\x04archive");
        assert!(read_picture(&path).is_err());
        File::create(&path)
            .unwrap()
            .set_len(desegnos_storage::request_zip::MAX_ARCHIVE_BYTES as u64 + 1)
            .unwrap();
        assert!(read_request_file(&path).is_err());
        let text = root.join("notes.md");
        std::fs::write(&text, "notes").unwrap();
        assert_eq!(read_request_file(&text).unwrap().mime, "text/plain");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn dropped_picture_originals_have_no_file_size_limit_and_require_an_os_path() {
        let root =
            std::env::temp_dir().join(format!("desegnos-image-drop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("picture.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\nuser picture bytes").unwrap();
        let picture = read_picture(&path).unwrap();
        assert_eq!(picture.mime, "image/png");
        assert_eq!(picture.name, "picture.png");
        assert_eq!(
            STANDARD.decode(picture.data).unwrap(),
            std::fs::read(&path).unwrap()
        );
        // Large originals are accepted; the editor optimizes them on Generate.
        let mut large = vec![0; 33 * 1024 * 1024];
        large[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        std::fs::write(&path, &large).unwrap();
        assert_eq!(
            STANDARD.decode(read_picture(&path).unwrap().data).unwrap(),
            large
        );
        std::fs::write(&path, "not a picture").unwrap();
        assert!(read_picture(&path).is_err());
        assert!(read_picture(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
