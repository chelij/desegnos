use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

// The core validates the typed layer packet separately. This adapter bounds
// platform text transfers without exposing general clipboard commands to JS.
use desegnos_core::limits::MAX_CLIPBOARD_BYTES;

fn check_size(text: &str) -> Result<(), String> {
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err("This item is too large to copy. Copy a smaller group.".into());
    }
    Ok(())
}

/// Copy a short piece of app-owned text, such as a local site address, at the user's request.
#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    write(&app, text)
}

fn write(app: &AppHandle, text: String) -> Result<(), String> {
    check_size(&text)?;
    app.clipboard()
        .write_text(text)
        .map_err(|_| "This item could not be copied. Try again.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_budget_counts_utf8_bytes_and_never_truncates() {
        let exact = "é".repeat(MAX_CLIPBOARD_BYTES / 2);
        assert!(check_size(&exact).is_ok());
        let oversized = format!("{exact}a");
        assert!(check_size(&oversized).is_err());
        assert_eq!(oversized.len(), MAX_CLIPBOARD_BYTES + 1);
        assert_eq!(MAX_CLIPBOARD_BYTES, 8 * 1024 * 1024);
    }
}
