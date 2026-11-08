//! Opt-in crash reports. Nothing is sent automatically: a crash is written to the data
//! folder, scrubbed, shown in the editor, and only opened as a prefilled GitHub issue when
//! the user asks for it.
//!
//! A panic hook records the crash without touching the UI. Startup also notices an unclean
//! exit through a small marker file, so a closed window is not silently forgotten.

use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Write as _},
    fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::data_location::DataDirectory;
use crate::release::REPOSITORY;
use tauri::State;

const REPORT_DIRECTORY: &str = "crash-reports";
const PENDING_FILE: &str = "pending.json";
const RUNNING_FILE: &str = "running";
const ISSUE_LABEL: &str = "crash";
const MAX_URL_BYTES: usize = 7500;
const MAX_MESSAGE_BYTES: usize = 2000;
const MAX_LOCATION_BYTES: usize = 500;
const MAX_BACKTRACE_BYTES: usize = 8 * 1024;
const TRUNCATED: &str = "\n…truncated";

const KIND_PANIC: &str = "panic";
const KIND_UNEXPECTED: &str = "unexpected-exit";
const KIND_FRONTEND: &str = "frontend";

/// The chosen data folder, set as soon as startup proves it. Panics before that only use the
/// default hook; nothing is written.
static DATA: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingReport {
    pub kind: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub time: String,
    pub message: String,
    pub location: String,
    pub backtrace: String,
}

#[derive(ts_rs::TS, Debug, Serialize)]
#[ts(rename = "PendingCrash")]
#[serde(rename_all = "camelCase")]
pub struct CrashReportView {
    pub kind: String,
    pub summary: String,
    pub body: String,
}

/// A failed open keeps the issue address so the editor can offer to copy it.
#[derive(ts_rs::TS, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenFailure {
    message: String,
    url: Option<String>,
}

impl OpenFailure {
    fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            url: None,
        }
    }

    fn with_url(message: impl Into<String>, url: &str) -> Self {
        Self {
            message: message.into(),
            url: Some(url.to_owned()),
        }
    }
}

impl fmt::Display for OpenFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    pid: u32,
    started_at: String,
}

impl PendingReport {
    fn new(kind: &str, message: &str, location: &str, backtrace: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            time: now(),
            message: capped(message, MAX_MESSAGE_BYTES),
            location: capped(location, MAX_LOCATION_BYTES),
            backtrace: capped(backtrace, MAX_BACKTRACE_BYTES),
        }
    }
}

/// Installs the recording hook, keeping the usual stderr output. Call once, early in `main`.
pub fn install_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(folder) = DATA.get() {
            // The hook itself must never panic, and must never recurse into itself.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                record_panic(folder, info);
            }));
        }
        default(info);
    }));
}

/// Remembers the data folder and starts the unclean-exit marker. Called once startup has
/// proven the folder writable.
pub fn start(folder: &Path) {
    // Already set means a second start in this process; the first folder stays.
    let _ = DATA.set(folder.to_owned());
    if let Err(error) = begin_session_with(folder, process_alive) {
        eprintln!("Could not record the start of this session for crash detection: {error}");
    }
}

/// Removes this run's marker after a normal exit. A marker owned by another running
/// instance is left alone.
pub fn finish() {
    let Some(folder) = DATA.get() else {
        return;
    };
    let marker = report_directory(folder).join(RUNNING_FILE);
    if read_marker(&marker).is_some_and(|marker| marker.pid == std::process::id()) {
        let _ = fs::remove_file(marker);
    }
}

fn install_start(folder: &Path) -> io::Result<()> {
    fs::create_dir_all(report_directory(folder))
}

fn report_directory(folder: &Path) -> PathBuf {
    folder.join(REPORT_DIRECTORY)
}

fn pending_path(folder: &Path) -> PathBuf {
    report_directory(folder).join(PENDING_FILE)
}

fn record_panic(folder: &Path, info: &std::panic::PanicHookInfo<'_>) {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
        .unwrap_or("Unknown error");
    let location = info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_default();
    let backtrace = capture_backtrace();
    let report = PendingReport::new(KIND_PANIC, message, &location, &backtrace);
    let _ = install_start(folder);
    let _ = write_pending(folder, &report);
}

fn capture_backtrace() -> String {
    let backtrace = std::backtrace::Backtrace::force_capture();
    let mut writer = CappedString::new(MAX_BACKTRACE_BYTES.saturating_sub(TRUNCATED.len()));
    let _ = write!(writer, "{backtrace}");
    let mut value = writer.value;
    if writer.cut {
        value.push_str(TRUNCATED);
    }
    value
}

/// Start-of-run bookkeeping: report a previous unclean exit, then mark this run. If another
/// instance owns a live marker, this run reports nothing and leaves that marker untouched.
fn begin_session_with(folder: &Path, alive: impl Fn(u32) -> bool) -> io::Result<()> {
    install_start(folder)?;
    if settle_previous(folder, &alive)? {
        return Ok(());
    }
    write_marker(folder)
}

fn settle_previous(folder: &Path, alive: &dyn Fn(u32) -> bool) -> io::Result<bool> {
    let marker = report_directory(folder).join(RUNNING_FILE);
    let Some(previous) = read_marker(&marker) else {
        return Ok(false);
    };
    if previous.pid != std::process::id() && alive(previous.pid) {
        return Ok(true);
    }
    if !pending_path(folder).exists() {
        let report = PendingReport::new(KIND_UNEXPECTED, "Desegnos closed unexpectedly.", "", "");
        let _ = write_pending(folder, &report);
    }
    let _ = fs::remove_file(marker);
    Ok(false)
}

fn write_marker(folder: &Path) -> io::Result<()> {
    let marker = Marker {
        pid: std::process::id(),
        started_at: now(),
    };
    let bytes = serde_json::to_vec(&marker).map_err(io::Error::other)?;
    write_atomic(&report_directory(folder).join(RUNNING_FILE), &bytes)
}

fn read_marker(path: &Path) -> Option<Marker> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(target_os = "linux")]
fn process_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// Without a portable process probe, assume another instance is alive. That can miss an
/// unexpected exit, but it never reports one that did not happen.
#[cfg(not(target_os = "linux"))]
fn process_alive(_: u32) -> bool {
    true
}

fn read_report(folder: &Path) -> Option<PendingReport> {
    let bytes = fs::read(pending_path(folder)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_pending(folder: &Path, report: &PendingReport) -> io::Result<()> {
    install_start(folder)?;
    let bytes = serde_json::to_vec(report).map_err(io::Error::other)?;
    write_atomic(&pending_path(folder), &bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".pending-{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temporary, bytes)?;
    #[cfg(windows)]
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    let result = fs::rename(&temporary, path);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn capped(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

struct CappedString {
    value: String,
    limit: usize,
    cut: bool,
}

impl CappedString {
    fn new(limit: usize) -> Self {
        Self {
            value: String::with_capacity(limit.min(1024)),
            limit,
            cut: false,
        }
    }
}

impl fmt::Write for CappedString {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let room = self.limit.saturating_sub(self.value.len());
        if room == 0 {
            self.cut = true;
            return Ok(());
        }
        if text.len() <= room {
            self.value.push_str(text);
            return Ok(());
        }
        let mut end = room;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.value.push_str(&text[..end]);
        self.cut = true;
        Ok(())
    }
}

fn view(report: &PendingReport) -> CrashReportView {
    CrashReportView {
        kind: report.kind.clone(),
        summary: summary(report),
        body: body(report),
    }
}

fn summary(report: &PendingReport) -> String {
    let first = report.message.lines().next().unwrap_or("").trim();
    let value = match report.kind.as_str() {
        KIND_UNEXPECTED => "Desegnos closed unexpectedly.".to_owned(),
        KIND_FRONTEND => format!("Editor error: {first}"),
        _ => format!("Crash: {first}"),
    };
    capped(&value, 120)
}

fn body(report: &PendingReport) -> String {
    let mut body = String::from("## Desegnos crash report\n\n");
    let _ = writeln!(body, "- Version: {}", report.version);
    let _ = writeln!(body, "- System: {} {}", report.os, report.arch);
    let _ = writeln!(body, "- Time: {}", report.time);
    let _ = writeln!(body, "- Kind: {}", kind_label(&report.kind));
    let _ = writeln!(body, "\n### What happened\n{}", scrub(&report.message));
    if !report.location.is_empty() {
        let _ = writeln!(body, "\n### Where\n{}", scrub(&report.location));
    }
    if !report.backtrace.is_empty() {
        let _ = writeln!(
            body,
            "\n### Backtrace\n```\n{}\n```",
            scrub(&report.backtrace)
        );
    }
    body
}

fn kind_label(kind: &str) -> &str {
    match kind {
        KIND_UNEXPECTED => "unexpected exit",
        KIND_FRONTEND => "editor error",
        _ => "crash",
    }
}

/// The prefilled issue. The body is cut at the end (the backtrace sits last) until the whole
/// address fits comfortably inside a browser's limits.
fn issue_url(report: &PendingReport) -> String {
    let title = format!("Desegnos crash: {}", summary(report));
    let fixed = format!(
        "{REPOSITORY}/issues/new?title={}&labels={}&body=",
        encode(&title),
        encode(ISSUE_LABEL)
    );
    let full = body(report);
    let fitted = fit_body(&fixed, &full);
    format!("{fixed}{}", encode(&fitted))
}

fn fit_body(fixed: &str, body: &str) -> String {
    if fixed.len() + encode(body).len() <= MAX_URL_BYTES {
        return body.to_owned();
    }
    let boundaries: Vec<usize> = body
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(body.len()))
        .collect();
    let mut low = 0;
    let mut high = boundaries.len() - 1;
    while low < high {
        let middle = (low + high).div_ceil(2);
        let trial = format!("{}{TRUNCATED}", &body[..boundaries[middle]]);
        if fixed.len() + encode(&trial).len() <= MAX_URL_BYTES {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    format!("{}{TRUNCATED}", &body[..boundaries[low]])
}

fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Removes personal paths and credentials before anything leaves the machine.
pub fn scrub(text: &str) -> String {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty());
    let username = std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
        .filter(|value| value.chars().count() >= 2)
        .or_else(|| {
            home.as_deref()
                .and_then(|home| Path::new(home).file_name())
                .map(|name| name.to_string_lossy().into_owned())
        });
    scrub_with(text, home.as_deref(), username.as_deref())
}

fn scrub_with(text: &str, home: Option<&str>, username: Option<&str>) -> String {
    let mut value = text.to_owned();
    if let Some(home) = home {
        value = value.replace(home, "~");
        let alternate = home.replace('/', "\\");
        if alternate != home {
            value = value.replace(&alternate, "~");
        }
    }
    value = redact_paths(&value);
    if let Some(username) = username {
        value = replace_word(&value, username, "<user>");
    }
    value = redact_urls(&value);
    value = redact_emails(&value);
    value = redact_bearer(&value);
    value = redact_jwts(&value);
    value = redact_openai_keys(&value);
    value = redact_named_secrets(&value);
    redact_long_runs(&value)
}

fn replace_word(text: &str, needle: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let boundary =
        |value: Option<char>| value.is_none_or(|value| !value.is_alphanumeric() && value != '_');
    while let Some(index) = rest.find(needle) {
        let after = rest[index + needle.len()..].chars().next();
        if boundary(rest[..index].chars().next_back()) && boundary(after) {
            out.push_str(&rest[..index]);
            out.push_str(replacement);
            rest = &rest[index + needle.len()..];
        } else {
            let end = index + needle.len();
            out.push_str(&rest[..end]);
            rest = &rest[end..];
        }
    }
    out.push_str(rest);
    out
}

fn redact_paths(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut copied = 0;
    while index < text.len() {
        if let Some(end) = path_at(text, bytes, index) {
            let token = &text[index..end];
            let name = token
                .trim_end_matches(|character: char| character.is_ascii_digit() || character == ':')
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(token);
            if !name.is_empty() && name != "/" {
                out.push_str(&text[copied..index]);
                out.push_str(name);
                copied = end;
            }
            index = end;
        } else {
            index += text[index..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out.push_str(&text[copied..]);
    out
}

fn path_at(text: &str, bytes: &[u8], index: usize) -> Option<usize> {
    let start = text[index..].chars().next()?;
    let home_path = start == '~' && bytes.get(index + 1) == Some(&b'/');
    let absolute = start == '/';
    let windows = start.is_ascii_alphabetic()
        && bytes.get(index + 1) == Some(&b':')
        && matches!(bytes.get(index + 2), Some(b'/' | b'\\'));
    if !home_path && !absolute && !windows {
        return None;
    }
    if index > 0 {
        let previous = bytes[index - 1];
        if !matches!(
            previous,
            b' ' | b'\t'
                | b'\n'
                | b'\r'
                | b'"'
                | b'\''
                | b'('
                | b'['
                | b'{'
                | b'='
                | b','
                | b';'
                | b':'
                | b'<'
                | b'>'
        ) {
            return None;
        }
    }
    // URLs and protocol-relative addresses are handled by their own pass.
    if absolute {
        if text[..index].to_ascii_lowercase().ends_with("http:")
            || text[..index].to_ascii_lowercase().ends_with("https:")
        {
            return None;
        }
        if bytes.get(index + 1) == Some(&b'/') {
            return None;
        }
    }
    let mut end = index;
    while end < text.len() {
        let byte = bytes[end];
        if byte.is_ascii_whitespace()
            || matches!(
                byte,
                b'"' | b'\''
                    | b'<'
                    | b'>'
                    | b'('
                    | b')'
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b','
                    | b';'
                    | b'|'
            )
        {
            break;
        }
        end += 1;
    }
    let token = &text[index..end];
    let separators = token.matches(['/', '\\']).count();
    (separators >= 2 || (home_path && separators >= 1)).then_some(end)
}

fn redact_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if let Some((scheme, authority_end, token_end)) = url_at(text, index) {
            out.push_str(scheme);
            out.push_str("://");
            let authority = &text[index + scheme.len() + 3..authority_end];
            let host = authority.rsplit('@').next().unwrap_or(authority);
            out.push_str(if host.is_empty() {
                "<redacted-host>"
            } else {
                host
            });
            index = token_end;
        } else {
            let character = text[index..].chars().next().expect("in range");
            out.push(character);
            index += character.len_utf8();
        }
    }
    out
}

fn url_at(text: &str, index: usize) -> Option<(&'static str, usize, usize)> {
    let bytes = &text.as_bytes()[index..];
    let scheme = if bytes.len() >= 7 && bytes[..7].eq_ignore_ascii_case(b"http://") {
        "http"
    } else if bytes.len() >= 8 && bytes[..8].eq_ignore_ascii_case(b"https://") {
        "https"
    } else {
        return None;
    };
    let start = index + scheme.len() + 3;
    let all = text.as_bytes();
    let mut authority_end = start;
    while authority_end < all.len() {
        let byte = all[authority_end];
        if byte.is_ascii_whitespace()
            || matches!(
                byte,
                b'/' | b'?' | b'#' | b'"' | b'\'' | b'<' | b'>' | b'(' | b')' | b'[' | b']' | b'`'
            )
        {
            break;
        }
        authority_end += 1;
    }
    let mut token_end = authority_end;
    while token_end < all.len() {
        let byte = all[token_end];
        if byte.is_ascii_whitespace()
            || matches!(byte, b'"' | b'\'' | b'<' | b'>' | b')' | b']' | b'`')
        {
            break;
        }
        token_end += 1;
    }
    Some((scheme, authority_end, token_end))
}

fn redact_emails(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut copied = 0;
    while index < text.len() {
        if bytes[index] == b'@' {
            let mut start = index;
            while start > 0 && is_email_local(bytes[start - 1]) {
                start -= 1;
            }
            let mut end = index + 1;
            while end < bytes.len() && is_email_domain(bytes[end]) {
                end += 1;
            }
            let domain = &bytes[index + 1..end];
            if start < index && domain.contains(&b'.') && !domain.starts_with(b".") {
                out.push_str(&text[copied..start]);
                out.push_str("<email>");
                copied = end;
                index = end;
                continue;
            }
        }
        index += 1;
    }
    out.push_str(&text[copied..]);
    out
}

fn is_email_local(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_email_domain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')
}

fn redact_bearer(text: &str) -> String {
    redact_keyword_token(text, "bearer", "Bearer <token>")
}

fn redact_keyword_token(text: &str, keyword: &str, replacement: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut copied = 0;
    while index + keyword.len() <= text.len() {
        if !bytes[index..index + keyword.len()].eq_ignore_ascii_case(keyword.as_bytes()) {
            index += 1;
            continue;
        }
        let before = text[..index].chars().next_back();
        let after_word = index + keyword.len();
        if before.is_some_and(|value| value.is_alphanumeric() || value == '_') {
            index += 1;
            continue;
        }
        let mut token_start = after_word;
        while token_start < bytes.len() && bytes[token_start].is_ascii_whitespace() {
            token_start += 1;
        }
        if token_start == after_word {
            index += 1;
            continue;
        }
        let mut end = token_start;
        while end < bytes.len() && is_token_byte(bytes[end]) {
            end += 1;
        }
        if end == token_start {
            index += 1;
            continue;
        }
        out.push_str(&text[copied..index]);
        out.push_str(replacement);
        copied = end;
        index = end;
    }
    out.push_str(&text[copied..]);
    out
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'~' | b'+' | b'/' | b'=')
}

fn redact_jwts(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut copied = 0;
    while index < text.len() {
        if bytes[index..].starts_with(b"eyJ") {
            let mut end = index;
            let mut dots = 0;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'.' | b'-' | b'_'))
            {
                if bytes[end] == b'.' {
                    dots += 1;
                }
                end += 1;
            }
            if end - index >= 20 && dots >= 2 {
                out.push_str(&text[copied..index]);
                out.push_str("<token>");
                copied = end;
                index = end;
                continue;
            }
        }
        index += 1;
    }
    out.push_str(&text[copied..]);
    out
}

fn redact_openai_keys(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut copied = 0;
    while index < text.len() {
        if bytes[index..].starts_with(b"sk-") {
            let mut end = index + 3;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'_' | b'-'))
            {
                end += 1;
            }
            if end - index >= 3 + 10 {
                out.push_str(&text[copied..index]);
                out.push_str("<key>");
                copied = end;
                index = end;
                continue;
            }
        }
        index += 1;
    }
    out.push_str(&text[copied..]);
    out
}

fn redact_named_secrets(text: &str) -> String {
    let mut value = text.to_owned();
    for name in [
        "api_key",
        "apikey",
        "access_token",
        "refresh_token",
        "password",
        "secret",
        "token",
        "key",
    ] {
        value = redact_parameter(&value, name);
    }
    value
}

fn redact_parameter(text: &str, name: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut copied = 0;
    while index + name.len() <= text.len() {
        if !bytes[index..index + name.len()].eq_ignore_ascii_case(name.as_bytes()) {
            index += 1;
            continue;
        }
        let before = text[..index].chars().next_back();
        if before.is_some_and(|value| value.is_alphanumeric() || value == '_') {
            index += 1;
            continue;
        }
        let mut equals = index + name.len();
        while equals < bytes.len() && bytes[equals] == b' ' {
            equals += 1;
        }
        if bytes.get(equals) != Some(&b'=') {
            index += 1;
            continue;
        }
        let mut end = equals + 1;
        while end < bytes.len()
            && !bytes[end].is_ascii_whitespace()
            && !matches!(
                bytes[end],
                b'&' | b'"' | b'\'' | b'<' | b'>' | b'#' | b',' | b';'
            )
        {
            end += 1;
        }
        if end == equals + 1 {
            index += 1;
            continue;
        }
        out.push_str(&text[copied..index]);
        out.push_str(name);
        out.push_str("=<redacted>");
        copied = end;
        index = end;
    }
    out.push_str(&text[copied..]);
    out
}

fn redact_long_runs(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut copied = 0;
    while index < text.len() {
        if is_run_byte(bytes[index]) {
            let mut end = index;
            while end < bytes.len() && is_run_byte(bytes[end]) {
                end += 1;
            }
            if end - index >= 32 {
                out.push_str(&text[copied..index]);
                out.push_str("<redacted>");
                copied = end;
            }
            index = end;
        } else {
            index += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

fn is_run_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'_' | b'-')
}

/// A pending report for the editor, scrubbed for review before the user opens it.
pub fn pending_at(folder: &Path) -> Option<CrashReportView> {
    read_report(folder).as_ref().map(view)
}

/// Deletes a pending report without opening anything.
pub fn dismiss_at(folder: &Path) -> io::Result<()> {
    match fs::remove_file(pending_path(folder)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Opens the prefilled GitHub issue, then deletes the local report. A failed open keeps the
/// report so the user can try again.
pub fn open_at(folder: &Path) -> Result<(), OpenFailure> {
    let Some(report) = read_report(folder) else {
        return Err(OpenFailure::plain("There is no crash report to send."));
    };
    let url = issue_url(&report);
    crate::system_open::open(&url).map_err(|_| {
        OpenFailure::with_url(
            "Couldn't open your browser. Copy this address to report the crash.",
            &url,
        )
    })?;
    let _ = dismiss_at(folder);
    Ok(())
}

fn record_frontend_at(folder: &Path, message: &str, stack: &str) -> io::Result<()> {
    let report = PendingReport::new(KIND_FRONTEND, message, "", stack);
    write_pending(folder, &report)
}

#[tauri::command]
pub fn pending_crash_report(data: State<'_, DataDirectory>) -> Option<CrashReportView> {
    pending_at(data.path())
}

#[tauri::command]
pub fn dismiss_crash_report(
    data: State<'_, DataDirectory>,
) -> Result<(), crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> Result<(), String> {
        dismiss_at(data.path()).map_err(|_| {
            "The crash report could not be removed. Check that Desegnos can write its data folder."
                .to_owned()
        })
    })
}

#[tauri::command]
pub fn open_crash_report(
    data: State<'_, DataDirectory>,
) -> Result<(), crate::native_error::NativeFailure> {
    open_at(data.path()).map_err(crate::native_error::NativeErrorKind::Command.failure())
}

#[tauri::command]
pub fn record_frontend_crash(
    data: State<'_, DataDirectory>,
    message: String,
    stack: String,
) -> Result<(), crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command.run(|| -> Result<(), String> {
        record_frontend_at(
            data.path(),
            &capped(&message, MAX_MESSAGE_BYTES),
            &capped(&stack, MAX_BACKTRACE_BYTES),
        )
        .map_err(|_| {
            "The crash report could not be saved. Check that Desegnos can write its data folder."
                .to_owned()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("desegnos-crash-report-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample() -> PendingReport {
        PendingReport::new(
            KIND_PANIC,
            "It broke",
            "src/main.rs:1:2",
            "frame one\nframe two",
        )
    }

    #[test]
    fn home_paths_usernames_and_project_paths_are_removed() {
        let text = "alice hit /home/alice/Projects/site/index.html:12 and ~/Projects/app/main.ts plus /home/alice/.config/desegnos";
        let scrubbed = scrub_with(text, Some("/home/alice"), Some("alice"));
        assert!(!scrubbed.contains("/home/alice"), "{scrubbed}");
        assert!(!scrubbed.contains("alice"), "{scrubbed}");
        assert!(scrubbed.contains("index.html"), "{scrubbed}");
        assert!(scrubbed.contains("main.ts"), "{scrubbed}");
        assert!(!scrubbed.contains("Projects/site"), "{scrubbed}");
    }

    #[test]
    fn secrets_are_removed() {
        let long = "a".repeat(64);
        let text = format!(
            "sk-abcdefghijklmnop Bearer abcdefghijklmnop eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.signature key=secretvalue {long}"
        );
        let scrubbed = scrub_with(&text, None, None);
        assert!(!scrubbed.contains("sk-abc"), "{scrubbed}");
        assert!(!scrubbed.contains("abcdefghijklmnop"), "{scrubbed}");
        assert!(!scrubbed.contains("eyJ"), "{scrubbed}");
        assert!(!scrubbed.contains("secretvalue"), "{scrubbed}");
        assert!(!scrubbed.contains(&long), "{scrubbed}");
        assert!(scrubbed.contains("<key>"), "{scrubbed}");
        assert!(scrubbed.contains("<token>"), "{scrubbed}");
        assert!(scrubbed.contains("<redacted>"), "{scrubbed}");
    }

    #[test]
    fn emails_and_url_queries_are_removed() {
        let text = "Mail alice@example.com or https://example.com/path?key=secret&token=abc#frag";
        let scrubbed = scrub_with(text, None, None);
        assert!(scrubbed.contains("<email>"), "{scrubbed}");
        assert!(scrubbed.contains("https://example.com"), "{scrubbed}");
        assert!(!scrubbed.contains("alice@"), "{scrubbed}");
        assert!(!scrubbed.contains("secret"), "{scrubbed}");
        assert!(!scrubbed.contains("token=abc"), "{scrubbed}");
        assert!(!scrubbed.contains("/path"), "{scrubbed}");
    }

    #[test]
    fn a_big_report_still_fits_in_the_issue_address() {
        let report = PendingReport::new(
            KIND_PANIC,
            "It broke",
            "src/main.rs:1:2",
            &"frame with some detail\n".repeat(4000),
        );
        let url = issue_url(&report);
        assert!(url.len() <= MAX_URL_BYTES, "{} bytes", url.len());
        assert!(
            url.starts_with(&format!("{REPOSITORY}/issues/new?")),
            "{url}"
        );
        assert!(url.contains("labels=crash"), "{url}");
        assert!(url.contains("truncated"), "{url}");
    }

    #[test]
    fn a_normal_report_keeps_its_whole_body() {
        let url = issue_url(&sample());
        assert!(url.len() <= MAX_URL_BYTES);
        assert!(!url.contains("truncated"));
    }

    #[test]
    fn a_dead_instance_leaves_an_unexpected_exit_report() {
        let temp = Temp::new();
        write_marker(&temp.0).unwrap();
        fs::write(
            report_directory(&temp.0).join(RUNNING_FILE),
            br#"{"pid":999999,"started_at":"before"}"#,
        )
        .unwrap();
        begin_session_with(&temp.0, |_| false).unwrap();
        let report = read_report(&temp.0).expect("an unexpected exit is recorded");
        assert_eq!(report.kind, KIND_UNEXPECTED);
        assert!(report.backtrace.is_empty());
        let marker = read_marker(&report_directory(&temp.0).join(RUNNING_FILE)).unwrap();
        assert_eq!(marker.pid, std::process::id());
    }

    #[test]
    fn a_live_other_instance_does_not_create_a_report() {
        let temp = Temp::new();
        install_start(&temp.0).unwrap();
        fs::write(
            report_directory(&temp.0).join(RUNNING_FILE),
            br#"{"pid":4242,"started_at":"before"}"#,
        )
        .unwrap();
        begin_session_with(&temp.0, |pid| pid == 4242).unwrap();
        assert!(pending_at(&temp.0).is_none());
        let marker = read_marker(&report_directory(&temp.0).join(RUNNING_FILE)).unwrap();
        assert_eq!(marker.pid, 4242, "the other instance keeps its marker");
    }

    #[test]
    fn an_existing_report_is_not_replaced_by_an_unexpected_exit() {
        let temp = Temp::new();
        record_frontend_at(&temp.0, "Editor failed", "stack").unwrap();
        fs::write(
            report_directory(&temp.0).join(RUNNING_FILE),
            br#"{"pid":999999,"started_at":"before"}"#,
        )
        .unwrap();
        begin_session_with(&temp.0, |_| false).unwrap();
        let report = read_report(&temp.0).unwrap();
        assert_eq!(report.kind, KIND_FRONTEND);
        assert_eq!(report.message, "Editor failed");
    }

    #[test]
    fn the_panic_writer_records_a_report() {
        let temp = Temp::new();
        record_panic_at(&temp.0, "boom", "src/lib.rs:10:4", "0: frame");
        let report = read_report(&temp.0).unwrap();
        assert_eq!(report.kind, KIND_PANIC);
        assert_eq!(report.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(report.message, "boom");
        assert_eq!(report.location, "src/lib.rs:10:4");
        assert_eq!(report.backtrace, "0: frame");
        assert!(report.os == std::env::consts::OS);
        assert!(!report.time.is_empty());
    }

    #[test]
    fn dismissing_removes_only_the_pending_report() {
        let temp = Temp::new();
        record_frontend_at(&temp.0, "Editor failed", "stack").unwrap();
        assert!(pending_at(&temp.0).is_some());
        dismiss_at(&temp.0).unwrap();
        assert!(pending_at(&temp.0).is_none());
        dismiss_at(&temp.0).unwrap();
    }

    fn record_panic_at(folder: &Path, message: &str, location: &str, backtrace: &str) {
        let report = PendingReport::new(KIND_PANIC, message, location, backtrace);
        write_pending(folder, &report).unwrap();
    }
}
