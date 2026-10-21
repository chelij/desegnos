//! Local record of how the editor is used: control names, shortcuts and command outcomes.
//! It never holds request text, page content, file paths or credentials, and nothing is
//! sent anywhere. One JSON line per event in `usage/YYYY-MM-DD.jsonl` beside the recovery
//! files, at most 2 MiB per day, kept for 30 days. `scripts/usage-report.mjs` reads it.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, WebviewWindow};

const MAX_EVENTS: usize = 500;
const MAX_DAY_BYTES: u64 = 2 * 1024 * 1024;
const KEEP_DAYS: i64 = 30;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageEvent {
    /// Milliseconds since the Unix epoch.
    pub t: u64,
    /// click, key, typing, command or session.
    pub kind: String,
    pub name: String,
    /// Where it happened (dialog, panel or canvas), or the edit type of a command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
}

fn short(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.chars().count() <= limit && !value.chars().any(char::is_control)
}

impl UsageEvent {
    fn valid(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "click" | "key" | "typing" | "command" | "session"
        ) && short(&self.name, 80)
            && self.detail.as_deref().is_none_or(|value| short(value, 120))
    }
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's civil_from_days).
fn date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn append(directory: &Path, today: i64, events: &[UsageEvent]) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let path = directory.join(format!("{}.jsonl", date(today)));
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() > MAX_DAY_BYTES) {
        return Ok(());
    }
    let mut lines = String::new();
    for event in events {
        lines.push_str(&serde_json::to_string(event).map_err(|error| error.to_string())?);
        lines.push('\n');
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&path)
        .and_then(|mut file| file.write_all(lines.as_bytes()))
        .map_err(|error| error.to_string())?;
    let oldest = date(today - KEEP_DAYS);
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.len() == 16 && name.ends_with(".jsonl") && name[..10] < *oldest {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

#[tauri::command]
pub fn record_usage(window: WebviewWindow, events: Vec<UsageEvent>) -> Result<(), String> {
    if window.label() != "editor" {
        return Err("Only the Desegnos editor records usage.".into());
    }
    if events.len() > MAX_EVENTS || !events.iter().all(UsageEvent::valid) {
        return Err("The usage record was not in the expected form.".into());
    }
    let today = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs() as i64
        / 86_400;
    let directory = window
        .app_handle()
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("usage");
    append(&directory, today, &events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_lines_and_retention() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(20_747), "2026-10-21");
        let directory =
            std::env::temp_dir().join(format!("desegnos-usage-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(format!("{}.jsonl", date(20_700))), "{}\n").unwrap();
        let event = UsageEvent {
            t: 1,
            kind: "click".into(),
            name: "Generate".into(),
            detail: None,
            count: None,
            ms: None,
            ok: None,
        };
        assert!(event.valid());
        append(&directory, 20_747, &[event]).unwrap();
        let today = fs::read_to_string(directory.join("2026-10-21.jsonl")).unwrap();
        assert_eq!(
            today,
            "{\"t\":1,\"kind\":\"click\",\"name\":\"Generate\"}\n"
        );
        assert!(!directory.join(format!("{}.jsonl", date(20_700))).exists());
        let bad = UsageEvent {
            t: 1,
            kind: "typed-text".into(),
            name: "x".into(),
            detail: None,
            count: None,
            ms: None,
            ok: None,
        };
        assert!(!bad.valid());
        fs::remove_dir_all(directory).unwrap();
    }
}
