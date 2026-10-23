use base64::{Engine, engine::general_purpose::STANDARD};

#[tauri::command]
pub async fn unpack_request_zip(
    name: String,
    data: String,
    remaining: usize,
) -> Result<desegni_storage::request_zip::ZipAttachments, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let limit = desegni_storage::request_zip::MAX_ARCHIVE_BYTES;
        if data.len() > limit.div_ceil(3) * 4 {
            return Err("This ZIP is too large. Choose a ZIP smaller than 32 MB.".into());
        }
        let bytes = STANDARD
            .decode(data)
            .map_err(|_| "The ZIP could not be read.")?;
        if bytes.len() > limit {
            return Err("This ZIP is too large. Choose a ZIP smaller than 32 MB.".into());
        }
        desegni_storage::request_zip::unpack_request_zip(
            std::io::Cursor::new(bytes),
            &name,
            remaining.min(16),
        )
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}
