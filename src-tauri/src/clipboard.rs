use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

// The core validates the typed layer packet separately. This adapter bounds
// platform text transfers without exposing general clipboard commands to JS.
const MAX_CLIPBOARD_BYTES: usize = 8 * desegni_core::MAX_SITE_SOURCE_BYTES;

fn check_size(text: &str) -> Result<(), String> {
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err("This item is too large to copy. Copy a smaller group.".into());
    }
    Ok(())
}

/// Invoke from a blocking worker, outside any workspace lock.
pub fn write(app: &AppHandle, text: String) -> Result<(), String> {
    check_size(&text)?;
    app.clipboard()
        .write_text(text)
        .map_err(|_| "This item could not be copied. Try again.".into())
}

/// Invoke from a blocking worker, outside any workspace lock. The official
/// adapter warns that reading WebView-owned text on the GUI thread can deadlock
/// Linux. Its API returns a complete String, so the byte limit applies after
/// the platform transfer and before the caller parses any layer packet.
pub fn read(app: &AppHandle) -> Result<String, String> {
    let text = app
        .clipboard()
        .read_text()
        .map_err(|_| "The clipboard could not be read. Copy the item again.")?;
    check_size(&text)?;
    Ok(text)
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
