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
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

struct Running {
    folder: Arc<PathBuf>,
    stop: Arc<Notify>,
}

/// Managed application state: at most one served folder at a time.
#[derive(Default)]
pub struct LocalSite {
    running: Option<Running>,
}

/// A second loopback server, for rendering imported websites while an export preview is open.
#[derive(Default)]
pub struct ImportSite(LocalSite);

impl ImportSite {
    pub fn serve(&mut self, folder: &Path, limit: usize) -> Result<String, String> {
        self.0.serve(folder, limit)
    }

    pub fn stop(&mut self) {
        self.0.stop();
    }
}

impl LocalSite {
    /// Serve a folder and return the URL to open. An earlier site is closed first.
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
        let folder = Arc::new(folder);
        let shared = Arc::new(Shared {
            token: token.clone(),
            folder: Arc::clone(&folder),
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
        self.running = Some(Running { folder, stop });
        Ok(format!("http://127.0.0.1:{}/{token}/", address.port()))
    }

    /// Close the listener. A later `serve` starts a fresh one.
    pub fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop.notify_one();
            let _ = running.folder;
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
    let mut response = response(StatusCode::OK, mime(&file.to_string_lossy()), bytes);
    if head {
        response.headers_mut().insert(
            "content-length",
            header(&metadata.len().to_string()),
        );
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
fn decode_path(path: &str) -> Option<String> {
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
        || value
            .split('/')
            .any(|part| part == ".." || part == ".");
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

fn response(status: StatusCode, content_type: &'static str, body: Vec<u8>) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(body)));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

fn missing() -> Response<Full<Bytes>> {
    response(
        StatusCode::NOT_FOUND,
        "text/plain; charset=utf-8",
        b"Not found".to_vec(),
    )
}

fn method_not_allowed() -> Response<Full<Bytes>> {
    response(
        StatusCode::METHOD_NOT_ALLOWED,
        "text/plain; charset=utf-8",
        b"Method not allowed".to_vec(),
    )
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
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
