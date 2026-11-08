use crate::native_files::LockExt;
use base64::Engine;
use desegnos_core::{ResolvedFont, resolve_font, standalone_face};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn faces() -> &'static Mutex<HashMap<String, ResolvedFont>> {
    static FACES: OnceLock<Mutex<HashMap<String, ResolvedFont>>> = OnceLock::new();
    FACES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasFont {
    id: String,
    bundled: bool,
    fell_back: bool,
    italic: bool,
}

#[tauri::command]
pub fn resolve_canvas_font(family: String, weight: u16, italic: bool) -> CanvasFont {
    let face = resolve_font(&family, weight, italic);
    let result = CanvasFont {
        id: face.id.clone(),
        bundled: face.bundled,
        fell_back: face.fell_back,
        italic: face.italic,
    };
    faces().locked().entry(face.id.clone()).or_insert(face);
    result
}

#[tauri::command]
pub fn list_canvas_fonts() -> Vec<String> {
    desegnos_core::font_families()
}

#[tauri::command]
pub fn load_canvas_font(id: String) -> Result<String, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> Result<String, String> {
        let faces = faces().locked();
        let face = faces
            .get(&id)
            .ok_or("Resolve the font before loading it.")?;
        let bytes = standalone_face(&face.bytes, face.index).ok_or("Invalid font collection.")?;
        Ok(format!(
            "data:font/ttf;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    })
}
