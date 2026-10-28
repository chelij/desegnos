use base64::Engine;
use desegnos_core::{ResolvedFont, resolve_font};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn faces() -> &'static Mutex<HashMap<String, ResolvedFont>> {
    static FACES: OnceLock<Mutex<HashMap<String, ResolvedFont>>> = OnceLock::new();
    FACES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasFont {
    id: String,
    bundled: bool,
    fell_back: bool,
    italic: bool,
}

#[tauri::command]
pub fn resolve_canvas_font(
    family: String,
    weight: u16,
    italic: bool,
) -> Result<CanvasFont, String> {
    let face = resolve_font(&family, weight, italic);
    let result = CanvasFont {
        id: face.id.clone(),
        bundled: face.bundled,
        fell_back: face.fell_back,
        italic: face.italic,
    };
    faces()
        .lock()
        .map_err(|e| e.to_string())?
        .entry(face.id.clone())
        .or_insert(face);
    Ok(result)
}

#[tauri::command]
pub fn list_canvas_fonts() -> Vec<String> {
    desegnos_core::font_families()
}

#[tauri::command]
pub fn load_canvas_font(id: String) -> Result<String, String> {
    let faces = faces().lock().map_err(|e| e.to_string())?;
    let face = faces
        .get(&id)
        .ok_or("Resolve the font before loading it.")?;
    let bytes = standalone_face(&face.bytes, face.index).ok_or("Invalid font collection.")?;
    Ok(format!(
        "data:font/ttf;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

// FontFace loads the first collection member. Give it a standalone SFNT directory pointing
// at the selected member's tables, so TTC/OTC files select exactly the face fontdb chose.
fn standalone_face(bytes: &[u8], index: u32) -> Option<Vec<u8>> {
    if bytes.get(..4)? != b"ttcf" {
        return Some(bytes.to_vec());
    }
    let read_u32 = |at| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let offset = read_u32(12 + index as usize * 4)? as usize;
    let count = u16::from_be_bytes(bytes.get(offset + 4..offset + 6)?.try_into().ok()?) as usize;
    let header_len = 12 + count * 16;
    let mut result = bytes.get(offset..offset + header_len)?.to_vec();
    for table in 0..count {
        let at = 12 + table * 16 + 8;
        let original = read_u32(offset + at)?;
        result[at..at + 4].copy_from_slice(&original.checked_add(header_len as u32)?.to_be_bytes());
    }
    result.extend_from_slice(bytes);
    Some(result)
}
