//! File access starts only at an operating-system drop, never at a renderer-supplied path.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use std::{fs::File, io::Read, path::Path};
use tauri::{DragDropEvent, Emitter, Window, WindowEvent};

const MAX_BYTES: u64 = 512 * 1024;
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
    message: Option<&'static str>,
}
fn read_picture(path: &Path) -> Result<Picture, &'static str> {
    if !path.is_file() {
        return Err("Drop a picture file onto an image on the page.");
    }
    let file = File::open(path).map_err(|_| "The picture could not be opened.")?;
    let meta = file
        .metadata()
        .map_err(|_| "The picture could not be opened.")?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err("Choose a picture up to 512 KB.");
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The picture could not be read.")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Choose a picture up to 512 KB.");
    }
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
pub fn handle(window: &Window, event: &WindowEvent) {
    if window.label() != "editor" {
        return;
    }
    let WindowEvent::DragDrop(event) = event else {
        return;
    };
    let scale = match window.scale_factor() {
        Ok(scale) if scale > 0.0 => scale,
        _ => return,
    };
    let position = match event {
        DragDropEvent::Enter { position, .. }
        | DragDropEvent::Over { position }
        | DragDropEvent::Drop { position, .. } => Some(Position {
            x: position.x / scale,
            y: position.y / scale,
        }),
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
            message: None,
        },
    );
    if let DragDropEvent::Drop { paths, .. } = event {
        let paths = paths.clone();
        let window = window.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = paths
                .first()
                .ok_or("Drop a picture file onto an image on the page.")
                .and_then(|path| read_picture(path));
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
    fn dropped_picture_bytes_are_bounded_and_paths_are_never_requested_by_the_renderer() {
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
        std::fs::write(&path, "not a picture").unwrap();
        assert!(read_picture(&path).is_err());
        std::fs::write(&path, vec![0; MAX_BYTES as usize + 1]).unwrap();
        assert!(read_picture(&path).is_err());
        assert!(read_picture(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
