//! A loopback static server for preview exports and Open in browser.
//!
//! One folder is served at a time, on `127.0.0.1` at a random free port, under a random token
//! prefix. Nothing else on the machine can browse it without learning the prefix, there is no
//! directory listing, and only GET and HEAD are answered. Files are read from the folder on
//! demand, so a preview never copies its own output.
use http_body_util::Full;
use hyper::{
    Request, Response, StatusCode,
    body::{Bytes, Incoming},
    header::HeaderValue,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::{
    convert::Infallible,
    net::{Ipv4Addr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Notify;
use uuid::Uuid;

/// The largest file the loopback server answers with.
use desegnos_core::limits::MAX_LOCAL_PREVIEW_BYTES as MAX_FILE_BYTES;

struct Running {
    stop: Arc<Notify>,
}

#[derive(Default)]
pub struct LocalSite {
    running: Option<Running>,
}

impl LocalSite {
    pub fn serve(&mut self, folder: &Path, limit: usize) -> Result<String, String> {
        let folder = folder
            .canonicalize()
            .map_err(|error| format!("The export folder could not be read: {error}"))?;
        if !folder.is_dir() {
            return Err("The export folder is not a directory.".into());
        }
        self.stop();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| format!("The local site server could not start: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("The local site server could not start: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("The local site server could not start: {error}"))?;
        let token = token();
        let stop = Arc::new(Notify::new());
        let shared = Arc::new(Shared {
            token: token.clone(),
            folder: Arc::new(folder),
            limit: (limit as u64).clamp(1, MAX_FILE_BYTES),
        });
        let stopper = Arc::clone(&stop);
        tauri::async_runtime::spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                return;
            };
            loop {
                let accepted = tokio::select! {
                    _ = stopper.notified() => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((stream, _)) = accepted else {
                    break;
                };
                let shared = Arc::clone(&shared);
                tauri::async_runtime::spawn(async move {
                    let service = service_fn(move |request| {
                        let shared = Arc::clone(&shared);
                        async move { Ok::<_, Infallible>(answer(request, &shared, address).await) }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        self.running = Some(Running { stop });
        Ok(format!("http://127.0.0.1:{}/{token}/", address.port()))
    }

    pub fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop.notify_one();
        }
    }
}

struct Shared {
    token: String,
    folder: Arc<PathBuf>,
    limit: u64,
}

fn token() -> String {
    // Two version-4 identifiers: 244 random bits, unguessable within a session.
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

async fn answer(
    request: Request<Incoming>,
    shared: &Arc<Shared>,
    address: SocketAddr,
) -> Response<Full<Bytes>> {
    if !trusted_host(&request, address) {
        return missing();
    }
    if !matches!(request.method().as_str(), "GET" | "HEAD") {
        return method_not_allowed();
    }
    let path = request.uri().path();
    let Some(rest) = path.strip_prefix(&format!("/{}", shared.token)) else {
        return missing();
    };
    if !rest.is_empty() && !rest.starts_with('/') {
        return missing();
    }
    let rest = rest.trim_start_matches('/');
    let Some(path) = decode_path(rest) else {
        return missing();
    };
    // A fetched page may keep percent escapes in its snapshot file name; try both spellings.
    let Some(file) = lookup(&shared.folder, &path).or_else(|| lookup(&shared.folder, rest)) else {
        return missing();
    };
    let metadata = match std::fs::symlink_metadata(&file) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        _ => return missing(),
    };
    if metadata.len() > shared.limit {
        return missing();
    }
    let head = request.method() == "HEAD";
    let bytes = if head {
        Vec::new()
    } else {
        match std::fs::read(&file) {
            Ok(bytes) if bytes.len() as u64 <= shared.limit => bytes,
            _ => return missing(),
        }
    };
    let path = file.to_string_lossy();
    let content_type = if head {
        content_type(&path, &read_prefix(&file))
    } else {
        content_type(&path, &bytes)
    };
    let mut response = response(StatusCode::OK, content_type, bytes);
    if head {
        response
            .headers_mut()
            .insert("content-length", header(&metadata.len().to_string()));
    }
    response
}

/// Resolve a decoded path inside the folder: exact file, or a directory's index.html. There is
/// never a listing, and every component must be an ordinary name.
fn lookup(root: &Path, path: &str) -> Option<PathBuf> {
    let path = path.trim_matches('/');
    let mut candidates: Vec<String> = Vec::new();
    if !path.is_empty() {
        candidates.push(path.to_owned());
    }
    candidates.push(if path.is_empty() {
        "index.html".into()
    } else {
        format!("{path}/index.html")
    });
    if !path.is_empty() && !path.rsplit('/').next().unwrap_or_default().contains('.') {
        // Fetched pages keep their original extensionless route (`/about` -> `about.html`).
        candidates.push(format!("{path}.html"));
    }
    candidates.into_iter().find_map(|name| {
        let mut file = root.to_path_buf();
        for component in name.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return None;
            }
            file.push(component);
        }
        file.is_file().then_some(file)
    })
}

/// Decode percent escapes and reject anything that could address another file.
pub fn decode_path(path: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(path.len());
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            (high * 16 + low) as u8
        } else {
            byte
        });
    }
    let value = String::from_utf8(decoded).ok()?;
    let rejected = value.contains(['\\', '\0'])
        || value.chars().any(char::is_control)
        || value.split('/').any(|part| part == ".." || part == ".");
    (!rejected).then_some(value)
}

fn trusted_host(request: &Request<Incoming>, address: SocketAddr) -> bool {
    request
        .headers()
        .get("host")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| host == format!("127.0.0.1:{}", address.port()))
}

fn header(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).unwrap_or_else(|_| HeaderValue::from_static("0"))
}

fn response(status: StatusCode, content_type: HeaderValue, body: Vec<u8>) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(body)));
    *response.status_mut() = status;
    response.headers_mut().insert("content-type", content_type);
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

fn missing() -> Response<Full<Bytes>> {
    response(
        StatusCode::NOT_FOUND,
        HeaderValue::from_static("text/plain; charset=utf-8"),
        b"Not found".to_vec(),
    )
}

fn method_not_allowed() -> Response<Full<Bytes>> {
    response(
        StatusCode::METHOD_NOT_ALLOWED,
        HeaderValue::from_static("text/plain; charset=utf-8"),
        b"Method not allowed".to_vec(),
    )
}

/// Read the first kilobyte without loading the rest of the file, for charset sniffing on HEAD.
fn read_prefix(path: &Path) -> Vec<u8> {
    use std::io::Read;
    let mut prefix = Vec::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(1024).read_to_end(&mut prefix);
    }
    prefix
}

fn content_type(path: &str, bytes: &[u8]) -> HeaderValue {
    let mime = mime(path);
    if mime == "text/html; charset=utf-8" {
        return header(&format!("text/html; charset={}", html_charset(bytes)));
    }
    HeaderValue::from_static(mime)
}

/// The charset an HTML file declares: a byte-order mark, then a `<meta>` declaration in the first
/// kilobyte, then UTF-8 when the bytes are valid, then windows-1252 for older pages.
fn html_charset(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return "utf-8".into();
    }
    if bytes.starts_with(&[0xff, 0xfe]) {
        return "utf-16le".into();
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        return "utf-16be".into();
    }
    if let Some(charset) = declared_charset(bytes) {
        return charset;
    }
    if std::str::from_utf8(bytes).is_ok() {
        "utf-8".into()
    } else {
        "windows-1252".into()
    }
}

fn declared_charset(bytes: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    let mut rest = head.as_str();
    while let Some(start) = rest.find("<meta") {
        let tag = &rest[start..];
        let end = tag.find('>').unwrap_or(tag.len());
        if let Some(charset) = meta_charset(&tag[..end]) {
            return Some(charset);
        }
        rest = &rest[start + end..];
    }
    None
}

/// The value of a `charset` declaration inside one meta tag, as a safe header token.
fn meta_charset(tag: &str) -> Option<String> {
    let mut cursor = tag;
    while let Some(index) = cursor.find("charset") {
        let after = cursor[index + "charset".len()..].trim_start();
        if let Some(after) = after.strip_prefix('=') {
            let after = after.trim_start();
            let after = after.strip_prefix(['"', '\'']).unwrap_or(after);
            let value: String = after
                .chars()
                .take_while(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':')
                })
                .collect();
            if !value.is_empty() {
                return Some(value);
            }
        }
        cursor = &cursor[index + "charset".len()..];
    }
    None
}

fn mime(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "pdf" => "application/pdf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::{content_type, html_charset, mime};

    #[test]
    fn extension_matching_ignores_case() {
        assert_eq!(mime("INDEX.HTM"), "text/html; charset=utf-8");
        assert_eq!(mime("PHOTO.JPG"), "image/jpeg");
        assert_eq!(mime("LOGO.GIF"), "image/gif");
        assert_eq!(mime("styles.CSS"), "text/css; charset=utf-8");
        assert_eq!(mime("photo.unknown"), "application/octet-stream");
    }

    #[test]
    fn html_charset_comes_from_a_byte_order_mark_first() {
        let mut bytes = b"<meta charset=\"windows-1252\"><p>caf\xe9</p>".to_vec();
        bytes.splice(0..0, [0xef, 0xbb, 0xbf]);
        assert_eq!(html_charset(&bytes), "utf-8");
        assert_eq!(
            html_charset(&[0xff, 0xfe, b'<', 0, b'p', 0, b'>', 0]),
            "utf-16le"
        );
    }

    #[test]
    fn html_charset_reads_a_meta_charset() {
        assert_eq!(
            html_charset(b"<!DOCTYPE html><meta charset=\"windows-1252\"><p>caf\xe9</p>"),
            "windows-1252"
        );
        assert_eq!(
            html_charset(b"<META CHARSET='ISO-8859-1'><p>caf\xe9</p>"),
            "iso-8859-1"
        );
    }

    #[test]
    fn html_charset_reads_a_http_equiv_declaration() {
        let page = b"<head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1252\"></head>";
        assert_eq!(html_charset(page), "windows-1252");
    }

    #[test]
    fn html_charset_falls_back_to_utf8_then_windows_1252() {
        assert_eq!(html_charset(b"<p>caf\xc3\xa9</p>"), "utf-8");
        assert_eq!(html_charset(b"<p>caf\xe9</p>"), "windows-1252");
    }

    #[test]
    fn html_charset_ignores_a_declaration_past_the_first_kilobyte() {
        let mut page = vec![b' '; 1100];
        page.extend_from_slice(b"<meta charset=\"windows-1252\"><p>caf\xc3\xa9</p>");
        assert_eq!(html_charset(&page), "utf-8");
    }

    #[test]
    fn content_type_sends_the_detected_charset_for_html_only() {
        let page =
            b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1252\">";
        assert_eq!(
            content_type("PAGE.HTM", page).to_str().unwrap(),
            "text/html; charset=windows-1252"
        );
        assert_eq!(
            content_type("PAGE.HTM", b"<p>ok</p>").to_str().unwrap(),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type("PHOTO.JPG", page).to_str().unwrap(),
            "image/jpeg"
        );
    }

    #[test]
    fn serves_uppercase_html_with_its_declared_charset() {
        use std::io::{Read, Write};
        let folder =
            std::env::temp_dir().join(format!("desegnos-local-site-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("INDEX.HTM"),
            b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1252\"><p>caf\xe9</p>",
        )
        .unwrap();
        let mut site = super::LocalSite::default();
        let url = site.serve(&folder, 1024 * 1024).unwrap();
        let rest = url
            .strip_prefix("http://127.0.0.1:")
            .and_then(|rest| rest.split_once('/'))
            .unwrap();
        let port: u16 = rest.0.parse().unwrap();
        let token = rest.1.trim_end_matches('/');
        let mut stream =
            std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET /{token}/INDEX.HTM HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let response = String::from_utf8_lossy(&response).to_ascii_lowercase();
        assert!(response.starts_with("http/1.1 200"), "{response}");
        assert!(
            response.contains("content-type: text/html; charset=windows-1252"),
            "{response}"
        );
        site.stop();
        std::fs::remove_dir_all(&folder).ok();
    }
}
