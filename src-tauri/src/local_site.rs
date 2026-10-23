//! The loopback HTTP server behind "Open in browser".
//!
//! One server serves the active project. It binds `127.0.0.1` on a random free port and
//! hangs every route under a random per-run token, so nothing else on the machine can
//! browse the project without learning the prefix. Files come from the same package a
//! preview or an export uses: imported websites keep large media by reference and are
//! decoded per requested range, generated sites are compiled exactly as the export would
//! compile them. Served HTML gains a tiny reload client that long-polls a version
//! endpoint; the workspace view bumps that version when the served project revision
//! changes, so the browser follows the editor. The reload client exists only in served
//! responses, never in the export bytes.

use crate::workspace::Workspace;
use desegni_core::{PreviewFile, Project};
use sha2::{Digest, Sha256};
use http_body_util::Full;
use hyper::{
    Request, Response, StatusCode,
    body::{Bytes, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::{
    collections::BTreeMap,
    convert::Infallible,
    net::{Ipv4Addr, SocketAddr, TcpListener},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::{Notify, watch};
use uuid::Uuid;

/// Reserved child of the token prefix for the reload long-poll.
const RELOAD: &str = "__desegnos_reload";
/// The reload client's own reserved resource.
const RELOAD_CLIENT: &str = "__desegnos_reload.js";
/// Largest total inline text a served site may hold; large media stays referenced.
const TEXT_LIMIT: usize = 64 * 1024 * 1024;
/// How long one long-poll waits for a revision change before answering again.
const RELOAD_HOLD: Duration = Duration::from_secs(25);
/// Quiet period after a revision change before the served package is rebuilt.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(300);

/// The files of one frozen site revision.
pub(crate) struct Package {
    files: BTreeMap<String, Entry>,
}

struct Entry {
    content_type: &'static str,
    body: PreviewFile,
}

#[derive(Clone)]
struct Site {
    token: String,
    version: u64,
    package: Arc<Package>,
    /// The project revision this package was built from.
    served: (Uuid, u64),
    /// The served source this package was built from. It ignores request prompts and
    /// other editor-only state, so those never rebuild or reload the site.
    signature: Option<String>,
}

#[derive(Default)]
struct Observed {
    last: Option<(Uuid, u64)>,
    generation: u64,
}

struct Shared {
    site: Mutex<Option<Site>>,
    versions: watch::Sender<u64>,
    observed: Mutex<Observed>,
    /// Builds the package for a project revision, or `None` when the workspace no longer
    /// holds that project.
    build: Box<dyn Fn(Uuid) -> Option<Arc<Package>> + Send + Sync>,
    /// Fingerprints the served sources; `None` always rebuilds.
    signature: Box<dyn Fn(Uuid) -> Option<String> + Send + Sync>,
}

struct Running {
    address: SocketAddr,
    stop: Arc<Notify>,
}

/// Managed application state: lazily started, at most one server per app.
pub struct LocalSite {
    shared: Arc<Shared>,
    running: Option<Running>,
}

/// Cheap handle the workspace publishes project revisions through.
#[derive(Clone)]
pub struct RevisionWatch(Arc<Shared>);

impl RevisionWatch {
    /// Report the active project revision each time a workspace view is published.
    pub fn observe(&self, project: Option<(Uuid, u64)>) {
        observe(&self.0, project);
    }
}

impl LocalSite {
    #[cfg(test)]
    pub fn new(build: impl Fn(Uuid) -> Option<Arc<Package>> + Send + Sync + 'static) -> Self {
        Self::with_signature(build, |_| None)
    }

    fn with_signature(
        build: impl Fn(Uuid) -> Option<Arc<Package>> + Send + Sync + 'static,
        signature: impl Fn(Uuid) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                site: Mutex::new(None),
                versions: watch::channel(0).0,
                observed: Mutex::new(Observed::default()),
                build: Box::new(build),
                signature: Box::new(signature),
            }),
            running: None,
        }
    }

    /// Wire the server to the live workspace.
    pub fn for_app(app: &AppHandle) -> Self {
        let app = app.clone();
        let sources = app.clone();
        Self::with_signature(
            move |project_id| {
                let project = {
                    let workspace = app.state::<Mutex<Workspace>>();
                    let workspace = workspace.lock().ok()?;
                    let project = workspace.project().ok()?;
                    if project.id != project_id {
                        return None;
                    }
                    project.clone()
                };
                package(&project).ok().map(Arc::new)
            },
            move |project_id| {
                let workspace = sources.state::<Mutex<Workspace>>();
                let workspace = workspace.lock().ok()?;
                let project = workspace.project().ok()?;
                (project.id == project_id).then(|| source_signature(&project))
            },
        )
    }

    pub fn watch(&self) -> RevisionWatch {
        RevisionWatch(Arc::clone(&self.shared))
    }

    /// Serve one project revision and return the address to open, starting the server on
    /// first use. Tests use this without a source signature, so every revision rebuilds.
    #[cfg(test)]
    pub fn open(
        &mut self,
        project_id: Uuid,
        revision: u64,
        package: Arc<Package>,
    ) -> Result<String, String> {
        self.open_with_signature(project_id, revision, package, None)
    }

    fn open_with_signature(
        &mut self,
        project_id: Uuid,
        revision: u64,
        package: Arc<Package>,
        signature: Option<String>,
    ) -> Result<String, String> {
        if self.running.is_none() {
            self.running = Some(start(&self.shared)?);
        }
        let running = self.running.as_ref().expect("the server was just started");
        let token = match locked(&self.shared.site).as_ref() {
            Some(site) => site.token.clone(),
            None => token(),
        };
        let (version, served) = {
            let mut site = locked(&self.shared.site);
            let version = site.as_ref().map_or(1, |site| site.version + 1);
            *site = Some(Site {
                token: token.clone(),
                version,
                package,
                served: (project_id, revision),
                signature,
            });
            (version, (project_id, revision))
        };
        self.shared.versions.send_replace(version);
        locked(&self.shared.observed).last = Some(served);
        Ok(format!(
            "http://127.0.0.1:{}/{token}/",
            running.address.port()
        ))
    }

    /// Close the listener. A later `open` starts a fresh one.
    pub fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop.notify_one();
        }
    }
}

/// Serve the active project in the user's default browser.
#[tauri::command]
pub async fn open_in_browser(app: AppHandle, window: WebviewWindow) -> Result<String, String> {
    if window.label() != "editor" {
        return Err("Only the Desegnos editor can open a website.".into());
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || open(&app))
        .await
        .map_err(|error| error.to_string())?
}

fn open(app: &AppHandle) -> Result<String, String> {
    let project = {
        let workspace = app.state::<Mutex<Workspace>>();
        let workspace = workspace
            .lock()
            .map_err(|_| "Project access is unavailable.")?;
        workspace.project()?.clone()
    };
    let package = Arc::new(package(&project)?);
    let url = app
        .state::<Mutex<LocalSite>>()
        .lock()
        .map_err(|_| "The local site server is unavailable.")?
        .open_with_signature(
            project.id,
            project.revision,
            package,
            Some(source_signature(&project)),
        )?;
    crate::system_open::open(&url).map_err(|_| {
        format!("The site is running at {url}, but no browser could open it. Open that address manually.")
    })?;
    Ok(url)
}

/// Collect the files the export would produce. Imported websites keep large media as
/// references to bytes the project already holds instead of copying them.
pub(crate) fn package(project: &Project) -> Result<Package, String> {
    let mut files = BTreeMap::new();
    if project.website.is_some() {
        let preview = project
            .website_preview_files(true, TEXT_LIMIT)
            .map_err(|error| error.to_string())?
            .ok_or("This project has no website files to open.")?;
        for (path, body) in preview {
            let content_type = mime(&path);
            files.insert(path, Entry { content_type, body });
        }
    } else {
        let export = desegni_core::compile_website(project).map_err(|error| error.to_string())?;
        for (path, bytes) in export.files {
            let content_type = mime(&path);
            files.insert(
                path,
                Entry {
                    content_type,
                    body: PreviewFile::Inline(bytes.into()),
                },
            );
        }
    }
    Ok(Package { files })
}

/// Fingerprint everything the served package is compiled from. Requests, prompts,
/// selections and other editor-only state are deliberately absent, so saving a request
/// never rebuilds or reloads the site.
pub(crate) fn source_signature(project: &Project) -> String {
    let mut hash = Sha256::new();
    feed(&mut hash, &project.name);
    for page in &project.pages {
        feed(&mut hash, &page.path);
        hash.update([u8::from(page.included)]);
        if let Some(content) = &page.content {
            feed(&mut hash, &content.html);
            feed(&mut hash, &content.css);
            feed(&mut hash, &content.javascript);
        }
    }
    for (id, picture) in &project.pictures {
        feed(&mut hash, &id.to_string());
        feed(&mut hash, picture);
    }
    if let Some(identity) = &project.imported_identity {
        for asset in &identity.assets {
            feed(&mut hash, &asset.id.to_string());
            feed(&mut hash, &asset.sha256);
            feed(&mut hash, &asset.data_uri);
        }
        for photo in &identity.photos {
            feed(&mut hash, &photo.id.to_string());
            feed(&mut hash, &photo.sha256);
            feed(&mut hash, &photo.data_uri);
        }
    }
    if let Some(site) = &project.website {
        for (path, encoded) in &site.files {
            feed(&mut hash, path);
            feed(&mut hash, encoded);
        }
        for (page_id, path) in &site.page_files {
            feed(&mut hash, &page_id.to_string());
            feed(&mut hash, path);
        }
    }
    format!("{:x}", hash.finalize())
}

fn feed(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value.as_bytes());
}

fn token() -> String {
    // Two version-4 identifiers: 244 random bits, unguessable within a session.
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn start(shared: &Arc<Shared>) -> Result<Running, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .map_err(|error| format!("The local site server could not start: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("The local site server could not start: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("The local site server could not start: {error}"))?;
    let stop = Arc::new(Notify::new());
    let stopper = Arc::clone(&stop);
    let shared = Arc::clone(shared);
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
    Ok(Running { address, stop })
}

/// Rebuild the served package when the served project's revision settles.
fn observe(shared: &Arc<Shared>, project: Option<(Uuid, u64)>) {
    let (generation, serving) = {
        let mut observed = locked(&shared.observed);
        if observed.last == project {
            return;
        }
        observed.last = project;
        observed.generation += 1;
        (observed.generation, locked(&shared.site).is_some())
    };
    let (Some((project_id, revision)), true) = (project, serving) else {
        return;
    };
    let served = locked(&shared.site)
        .as_ref()
        .is_some_and(|site| site.served.0 == project_id);
    if !served {
        return;
    }
    let shared = Arc::clone(shared);
    std::thread::spawn(move || {
        std::thread::sleep(RELOAD_DEBOUNCE);
        // A later change supersedes this one; its own thread rebuilds.
        if locked(&shared.observed).generation != generation {
            return;
        }
        // A prompt save or another editor-only change leaves the served sources alone.
        let signature = (shared.signature)(project_id);
        if let Some(signature) = &signature {
            let current = locked(&shared.site);
            if current
                .as_ref()
                .is_some_and(|site| site.served.0 == project_id && site.signature.as_ref() == Some(signature))
            {
                return;
            }
        }
        let Some(package) = (shared.build)(project_id) else {
            return;
        };
        if locked(&shared.observed).last != Some((project_id, revision)) {
            return;
        }
        let mut current = locked(&shared.site);
        let Some(site) = current.as_mut() else {
            return;
        };
        if site.served.0 != project_id {
            return;
        }
        site.package = package;
        site.served = (project_id, revision);
        site.signature = signature;
        site.version += 1;
        let version = site.version;
        drop(current);
        shared.versions.send_replace(version);
    });
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
    let Some(site) = locked(&shared.site).clone() else {
        return missing();
    };
    let path = request.uri().path();
    let Some(rest) = path.strip_prefix(&format!("/{}", site.token)) else {
        return missing();
    };
    if !rest.is_empty() && !rest.starts_with('/') {
        return missing();
    }
    let rest = rest.trim_start_matches('/');
    if rest == RELOAD {
        return reload(shared, &request).await;
    }
    if rest == RELOAD_CLIENT {
        return text(
            StatusCode::OK,
            "text/javascript; charset=utf-8",
            reload_client(&site.token),
        );
    }
    let Some(path) = decode_path(rest) else {
        return missing();
    };
    if let Some((_, entry)) = lookup(&site.package, &path) {
        return file(&site, entry, &request, StatusCode::OK);
    }
    match site.package.files.get_key_value("404.html") {
        Some((_, entry)) => file(&site, entry, &request, StatusCode::NOT_FOUND),
        None => missing(),
    }
}

async fn reload(shared: &Arc<Shared>, request: &Request<Incoming>) -> Response<Full<Bytes>> {
    let mut versions = shared.versions.subscribe();
    let current = *versions.borrow();
    let since = request
        .uri()
        .query()
        .and_then(|query| query.split('&').find_map(|pair| pair.strip_prefix("since=")))
        .and_then(|value| value.parse::<u64>().ok());
    let version = if since == Some(current) {
        let _ = tokio::time::timeout(RELOAD_HOLD, versions.changed()).await;
        *versions.borrow()
    } else {
        current
    };
    let head = request.method() == "HEAD";
    let mut response = text(
        StatusCode::OK,
        "text/plain; charset=utf-8",
        if head {
            String::new()
        } else {
            version.to_string()
        },
    );
    if head {
        response.headers_mut().insert(
            "content-length",
            header(&version.to_string().len().to_string()),
        );
    }
    response
}

/// Serve one package entry, decoding only the requested byte range.
fn file(
    site: &Site,
    entry: &Entry,
    request: &Request<Incoming>,
    status: StatusCode,
) -> Response<Full<Bytes>> {
    let Some(len) = entry.body.decoded_len() else {
        return missing();
    };
    let head = request.method() == "HEAD";
    let html = entry.content_type.starts_with("text/html");
    match requested_range(request.headers().get("range"), len) {
        RequestedRange::Unsatisfiable => {
            let mut response = response(StatusCode::RANGE_NOT_SATISFIABLE, entry.content_type, vec![]);
            response
                .headers_mut()
                .insert("content-range", header(&format!("bytes */{len}")));
            response
        }
        RequestedRange::Satisfiable(start, end) => {
            let body = if head {
                vec![]
            } else {
                entry.body.read_range(start, end).unwrap_or_default()
            };
            let mut response = response(StatusCode::PARTIAL_CONTENT, entry.content_type, body);
            response
                .headers_mut()
                .insert("content-range", header(&format!("bytes {start}-{end}/{len}")));
            response.headers_mut().insert(
                "content-length",
                header(&(end - start + 1).to_string()),
            );
            response
        }
        RequestedRange::None => {
            // HTML always decodes so that HEAD reports the length of the decorated
            // document a GET would return; other files stay undecoded for HEAD.
            let decorated = if html && len > 0 {
                Some(with_reload(
                    entry.body.read_range(0, len - 1).unwrap_or_default(),
                    &site.token,
                ))
            } else {
                None
            };
            let (body, content_length) = match decorated {
                Some(body) => {
                    let length = body.len();
                    (if head { vec![] } else { body }, length)
                }
                None => (
                    if head || len == 0 {
                        vec![]
                    } else {
                        entry.body.read_range(0, len - 1).unwrap_or_default()
                    },
                    len,
                ),
            };
            let mut response = response(status, entry.content_type, body);
            if head {
                response
                    .headers_mut()
                    .insert("content-length", header(&content_length.to_string()));
            }
            response
        }
    }
}

/// Resolve a decoded path against the package: exact file, directory index, or the
/// matching `.html` page.
fn lookup<'a>(package: &'a Package, path: &str) -> Option<(&'a String, &'a Entry)> {
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
    if !path.is_empty() {
        candidates.push(format!("{path}.html"));
    }
    candidates
        .iter()
        .find_map(|name| package.files.get_key_value(name))
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

/// The reload client is served, never exported: it long-polls the version endpoint and
/// reloads the page when the editor publishes a new revision.
fn reload_client(token: &str) -> String {
    format!(
        r#"(function () {{
  var url = "/{token}/{RELOAD}";
  var version = null;
  function poll() {{
    var target = version === null ? url : url + "?since=" + version;
    fetch(target, {{ cache: "no-store" }}).then(function (response) {{
      if (!response.ok) {{ throw new Error("reload " + response.status); }}
      return response.text();
    }}).then(function (text) {{
      var next = Number(text);
      if (version !== null && next !== version) {{ location.reload(); return; }}
      version = next;
      setTimeout(poll, 250);
    }}).catch(function () {{ setTimeout(poll, 2000); }});
  }}
  poll();
}})();"#
    )
}

/// Add the reload client to a served HTML document.
fn with_reload(bytes: Vec<u8>, token: &str) -> Vec<u8> {
    match String::from_utf8(bytes) {
        Ok(mut html) => {
            let tag = format!("<script src=\"/{token}/{RELOAD_CLIENT}\"></script>");
            match html.to_ascii_lowercase().rfind("</body>") {
                Some(position) => html.insert_str(position, &tag),
                None => html.push_str(&tag),
            }
            html.into_bytes()
        }
        Err(error) => error.into_bytes(),
    }
}

enum RequestedRange {
    None,
    Satisfiable(usize, usize),
    Unsatisfiable,
}

fn requested_range(value: Option<&hyper::header::HeaderValue>, len: usize) -> RequestedRange {
    let Some(spec) = value
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("bytes="))
    else {
        return RequestedRange::None;
    };
    if spec.contains(',') {
        return RequestedRange::None;
    }
    let Some((start, end)) = spec.split_once('-') else {
        return RequestedRange::Unsatisfiable;
    };
    if len == 0 {
        return RequestedRange::Unsatisfiable;
    }
    if start.is_empty() {
        let Ok(suffix) = end.parse::<usize>() else {
            return RequestedRange::Unsatisfiable;
        };
        if suffix == 0 {
            return RequestedRange::Unsatisfiable;
        }
        return RequestedRange::Satisfiable(len - suffix.min(len), len - 1);
    }
    let Ok(start) = start.parse::<usize>() else {
        return RequestedRange::Unsatisfiable;
    };
    let end = if end.is_empty() {
        len - 1
    } else {
        let Ok(end) = end.parse::<usize>() else {
            return RequestedRange::Unsatisfiable;
        };
        end
    };
    if start > end || end >= len {
        RequestedRange::Unsatisfiable
    } else {
        RequestedRange::Satisfiable(start, end)
    }
}

fn response(
    status: StatusCode,
    content_type: &'static str,
    body: Vec<u8>,
) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .header("x-content-type-options", "nosniff")
        .header("cache-control", "no-store")
        .header("accept-ranges", "bytes")
        .header("referrer-policy", "no-referrer")
        .body(Full::new(Bytes::from(body)))
        .expect("Local site response headers are fixed and valid")
}

fn text(status: StatusCode, content_type: &'static str, body: String) -> Response<Full<Bytes>> {
    response(status, content_type, body.into_bytes())
}

fn missing() -> Response<Full<Bytes>> {
    text(
        StatusCode::NOT_FOUND,
        "text/plain; charset=utf-8",
        "Not found.".into(),
    )
}

fn method_not_allowed() -> Response<Full<Bytes>> {
    let mut response = text(
        StatusCode::METHOD_NOT_ALLOWED,
        "text/plain; charset=utf-8",
        "Only GET and HEAD are served.".into(),
    );
    response
        .headers_mut()
        .insert("allow", header("GET, HEAD"));
    response
}

fn header(value: &str) -> hyper::header::HeaderValue {
    value
        .parse()
        .expect("Local site header values are valid")
}

/// Content types mirror the preview package's mapping by extension.
fn mime(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "json" => "application/json",
        "xml" => "application/xml",
        "webmanifest" => "application/manifest+json",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use desegni_core::{Appearance, SiteContent, Viewport};
    use std::{
        io::{Read, Write},
        net::TcpStream,
    };

    fn generated() -> Project {
        let mut project = Project::new(
            "Browser checks".into(),
            Viewport {
                width: 1440,
                height: 900,
            },
            Appearance::Light,
        )
        .unwrap();
        project.pages[0].content = Some(SiteContent {
            html: "<h1>Home</h1><a href=\"/about/\">About</a>".into(),
            css: "h1{color:navy}".into(),
            javascript: "document.body.dataset.ready='yes';".into(),
        });
        project
    }

    fn package_from(files: &[(&str, &str)]) -> Package {
        Package {
            files: files
                .iter()
                .map(|(path, body)| {
                    (
                        path.to_string(),
                        Entry {
                            content_type: mime(path),
                            body: PreviewFile::Inline(body.as_bytes().to_vec().into()),
                        },
                    )
                })
                .collect(),
        }
    }

    fn address(url: &str) -> (SocketAddr, String) {
        let rest = url.strip_prefix("http://").expect("loopback url");
        let (authority, token) = rest.split_once('/').expect("token prefix");
        (
            authority.parse().expect("loopback address"),
            token.trim_end_matches('/').to_owned(),
        )
    }

    fn request(
        address: SocketAddr,
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
    ) -> (u16, BTreeMap<String, String>, Vec<u8>) {
        let mut stream = TcpStream::connect(address).expect("server accepts loopback");
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut message = format!("{method} {target} HTTP/1.1\r\n");
        if !headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("host")) {
            message.push_str(&format!("Host: 127.0.0.1:{}\r\n", address.port()));
        }
        message.push_str("Connection: close\r\n");
        for (name, value) in headers {
            message.push_str(&format!("{name}: {value}\r\n"));
        }
        message.push_str("\r\n");
        stream.write_all(message.as_bytes()).unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        let split = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("response headers end");
        let head = String::from_utf8_lossy(&bytes[..split]).into_owned();
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse().ok())
            .expect("status line");
        let headers = lines
            .filter_map(|line| line.split_once(": "))
            .map(|(name, value)| (name.to_ascii_lowercase(), value.to_owned()))
            .collect();
        (status, headers, bytes[split + 4..].to_vec())
    }

    fn served(files: &[(&str, &str)]) -> (LocalSite, SocketAddr, String) {
        let mut site = LocalSite::new(|_| None);
        let url = site
            .open(Uuid::new_v4(), 1, Arc::new(package_from(files)))
            .unwrap();
        let (address, token) = address(&url);
        (site, address, token)
    }

    #[test]
    fn every_route_needs_the_token_and_only_get_and_head_are_served() {
        let (_site, address, token) =
            served(&[("index.html", "<html><body>Home</body></html>")]);
        for target in [
            "/".to_owned(),
            "/index.html".to_owned(),
            format!("/{}", &token[..8]),
        ] {
            let (status, _, body) = request(address, "GET", &target, &[]);
            assert_eq!(status, 404, "{target}");
            assert!(!String::from_utf8_lossy(&body).contains("Home"));
        }
        assert_eq!(
            request(address, "GET", &format!("/{token}/"), &[]).0,
            200
        );
        assert_eq!(
            request(address, "POST", &format!("/{token}/"), &[]).0,
            405
        );
        // A foreign Host header never reaches the site, even with the right token.
        assert_eq!(
            request(
                address,
                "GET",
                &format!("/{token}/"),
                &[("host", "example.com")]
            )
            .0,
            404
        );
    }

    #[test]
    fn traversal_and_encoded_paths_are_rejected() {
        let (_site, address, token) = served(&[(
            "index.html",
            "<html><body>Home</body></html>",
        )]);
        for path in [
            "../Cargo.toml",
            "%2e%2e/Cargo.toml",
            "..%2fCargo.toml",
            "%2e%2e%2fcrates/desegni-core/Cargo.toml",
            "images/%2e%2e/%2e%2e/Cargo.toml",
            "index.html%00.css",
            "..%5cCargo.toml",
        ] {
            let (status, _, body) = request(address, "GET", &format!("/{token}/{path}"), &[]);
            assert_eq!(status, 404, "{path}");
            assert!(!String::from_utf8_lossy(&body).contains("workspace"));
        }
    }

    #[test]
    fn directory_routes_serve_index_and_missing_routes_use_the_404_document() {
        let (_site, address, token) = served(&[
            ("index.html", "<html><body>Home</body></html>"),
            ("about/index.html", "<html><body>About</body></html>"),
            ("styles/site.css", "h1{color:navy}"),
            ("404.html", "<html><body>Gone</body></html>"),
        ]);
        for path in ["", "about/", "about"] {
            let (status, headers, body) =
                request(address, "GET", &format!("/{token}/{path}"), &[]);
            assert_eq!(status, 200, "{path}");
            assert_eq!(headers["content-type"], "text/html; charset=utf-8");
            let text = String::from_utf8_lossy(&body);
            assert!(
                text.contains(if path.starts_with("about") { "About" } else { "Home" }),
                "{path}: {text}"
            );
        }
        let (status, headers, body) =
            request(address, "GET", &format!("/{token}/styles/site.css"), &[]);
        assert_eq!(status, 200);
        assert_eq!(headers["content-type"], "text/css; charset=utf-8");
        assert_eq!(body, b"h1{color:navy}");
        let (status, headers, body) = request(address, "HEAD", &format!("/{token}/styles/site.css"), &[]);
        assert_eq!(status, 200);
        assert!(body.is_empty());
        assert_eq!(headers["content-length"], "h1{color:navy}".len().to_string());
        let (_, _, document) = request(address, "GET", &format!("/{token}/"), &[]);
        let (status, headers, head) = request(address, "HEAD", &format!("/{token}/"), &[]);
        assert_eq!(status, 200);
        assert!(head.is_empty());
        assert_eq!(headers["content-length"], document.len().to_string());
        let (status, headers, body) =
            request(address, "GET", &format!("/{token}/missing"), &[]);
        assert_eq!(status, 404);
        assert_eq!(headers["content-type"], "text/html; charset=utf-8");
        assert!(String::from_utf8_lossy(&body).contains("Gone"));
    }

    #[test]
    fn large_media_is_served_by_reference_with_range_requests() {
        let media: Vec<u8> = (0..300 * 1024).map(|index| (index % 251) as u8).collect();
        let project = desegni_core::import_website_files(
            "Portfolio",
            &BTreeMap::from([
                (
                    "index.html".into(),
                    b"<!doctype html><html><body><video src=\"portfolio/clip.mp4\"></video></body></html>".to_vec(),
                ),
                ("portfolio/clip.mp4".into(), media),
            ]),
        )
        .unwrap();
        let package = Arc::new(package(&project).unwrap());
        let mut site = LocalSite::new(|_| None);
        let url = site.open(project.id, project.revision, package).unwrap();
        let (address, token) = address(&url);
        let target = format!("/{token}/portfolio/clip.mp4");
        let clip = 300 * 1024;
        let (status, headers, body) = request(address, "HEAD", &target, &[]);
        assert_eq!(status, 200);
        assert_eq!(headers["content-type"], "video/mp4");
        assert_eq!(headers["accept-ranges"], "bytes");
        assert_eq!(headers["content-length"], clip.to_string());
        assert!(body.is_empty());
        let expected = |start: usize, end: usize| -> Vec<u8> {
            (start..=end).map(|index| (index % 251) as u8).collect()
        };
        let (status, headers, body) =
            request(address, "GET", &target, &[("range", "bytes=1000-1999")]);
        assert_eq!(status, 206);
        assert_eq!(headers["content-range"], format!("bytes 1000-1999/{clip}"));
        assert_eq!(headers["content-length"], "1000");
        assert_eq!(body, expected(1000, 1999));
        let (status, _, body) = request(address, "GET", &target, &[("range", "bytes=-8")]);
        assert_eq!(status, 206);
        assert_eq!(body, expected(clip - 8, clip - 1));
        let (status, headers, _) =
            request(address, "GET", &target, &[("range", "bytes=0-999999999")]);
        assert_eq!(status, 416);
        assert_eq!(headers["content-range"], format!("bytes */{clip}"));
    }

    #[test]
    fn source_signature_ignores_request_prompts_and_tracks_documents() {
        use desegni_core::Command;
        let mut project = generated();
        let page_id = project.pages[0].id;
        let rect = desegni_core::Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
        let added = project.apply(project.revision, Command::AddRegion { page_id, rect }).unwrap();
        let region_id = added.pages[0].regions[0].id;
        let typed = added
            .apply(added.revision, Command::UpdateRegion { page_id, region_id, rect, prompt: "First".into() })
            .unwrap();
        let retyped = typed
            .apply(typed.revision, Command::UpdateRegion { page_id, region_id, rect, prompt: "A much longer prompt typed by hand".into() })
            .unwrap();
        assert_ne!(typed.revision, retyped.revision);
        assert_eq!(source_signature(&typed), source_signature(&retyped), "a prompt save must not rebuild the site");
        let mut edited = retyped.clone();
        edited.pages[0].content.as_mut().unwrap().html.push_str("<p>More</p>");
        assert_ne!(source_signature(&typed), source_signature(&edited));
        let mut framed = typed.clone();
        framed.website = Some(desegni_core::ImportedWebsite {
            page_files: BTreeMap::new(),
            files: BTreeMap::from([("play/game.html".to_string(), "<p>Game</p>".into())]),
            framework: None,
            skipped: vec![],
        });
        let unchanged_prompt = framed
            .apply(framed.revision, Command::UpdateRegion { page_id, region_id, rect, prompt: "Another prompt".into() })
            .unwrap();
        assert_eq!(source_signature(&framed), source_signature(&unchanged_prompt));
        let mut changed_site = framed.clone();
        changed_site.website.as_mut().unwrap().files.insert("play/game.html".to_string(), "<p>Game two</p>".into());
        assert_ne!(source_signature(&framed), source_signature(&changed_site));
    }

    #[test]
    fn prompt_only_revisions_do_not_rebuild_or_bump_the_served_site() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let project = generated();
        let builds = Arc::new(AtomicUsize::new(0));
        let sources = Arc::new(Mutex::new(source_signature(&project)));
        let counted = Arc::clone(&builds);
        let served = Arc::clone(&sources);
        let mut site = LocalSite::with_signature(
            move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
                Some(Arc::new(package_from(&[("index.html", "<html><body>Rebuilt</body></html>")])))
            },
            move |_| Some(locked(&served).clone()),
        );
        let url = site
            .open_with_signature(
                project.id,
                project.revision,
                Arc::new(package_from(&[("index.html", "<html><body>First</body></html>")])),
                Some(source_signature(&project)),
            )
            .expect("site starts");
        let (address, token) = address(&url);
        let reload = format!("/{token}/{RELOAD}");
        assert_eq!(request(address, "GET", &reload, &[]).2, b"1");
        // Saving a request prompt advances the revision but changes no served source.
        site.watch().observe(Some((project.id, project.revision + 1)));
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(request(address, "GET", &reload, &[]).2, b"1", "a prompt-only save must not reload the browser");
        assert_eq!(builds.load(Ordering::SeqCst), 0, "a prompt-only save must not rebuild the package");
        // Editing page code does rebuild once.
        *locked(&sources) = "changed-document".into();
        site.watch().observe(Some((project.id, project.revision + 2)));
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(request(address, "GET", &format!("{reload}?since=1"), &[]).2, b"2");
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert!(String::from_utf8_lossy(&request(address, "GET", &format!("/{token}/"), &[]).2).contains("Rebuilt"));
    }

    #[test]
    fn reload_endpoint_bumps_after_the_served_revision_changes() {
        let project = generated();
        let first = Arc::new(package_from(&[("index.html", "<html><body>First</body></html>")]));
        let second = Arc::new(package_from(&[("index.html", "<html><body>Second</body></html>")]));
        let rebuilt = Arc::clone(&second);
        let mut site = LocalSite::new(move |_| Some(Arc::clone(&rebuilt)));
        let url = site
            .open(project.id, project.revision, first)
            .expect("site starts");
        let (address, token) = address(&url);
        let reload = format!("/{token}/{RELOAD}");
        assert_eq!(request(address, "GET", &reload, &[]).2, b"1");
        // Another project's revision does not reload this site.
        site.watch().observe(Some((Uuid::new_v4(), 9)));
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(request(address, "GET", &reload, &[]).2, b"1");
        site.watch()
            .observe(Some((project.id, project.revision + 1)));
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(request(address, "GET", &format!("{reload}?since=1"), &[]).2, b"2");
        assert!(
            String::from_utf8_lossy(&request(address, "GET", &format!("/{token}/"), &[]).2)
                .contains("Second")
        );
    }

    #[test]
    fn reload_client_is_served_but_never_exported() {
        let project = generated();
        let export = desegni_core::compile_website(&project).unwrap();
        assert!(
            export
                .files
                .values()
                .all(|bytes| !String::from_utf8_lossy(bytes).contains(RELOAD)),
            "the exported site must not contain the reload client"
        );
        let package = Arc::new(package(&project).unwrap());
        let marker_in_package = package
            .files
            .values()
            .filter(|entry| entry.content_type == "text/html; charset=utf-8")
            .any(|entry| {
                let len = entry.body.decoded_len().expect("inline html");
                let html = entry.body.read_range(0, len - 1).expect("inline html");
                String::from_utf8_lossy(&html).contains(RELOAD)
            });
        assert!(
            !marker_in_package,
            "the package itself must stay free of the reload client"
        );
        let mut site = LocalSite::new(|_| None);
        let url = site
            .open(project.id, project.revision, package)
            .expect("site starts");
        let (address, token) = address(&url);
        let (status, headers, body) = request(address, "GET", &format!("/{token}/"), &[]);
        assert_eq!(status, 200);
        assert_eq!(headers["content-type"], "text/html; charset=utf-8");
        let html = String::from_utf8(body).unwrap();
        assert!(html.contains(&format!("/{token}/{RELOAD}.js")));
        let (status, headers, script) =
            request(address, "GET", &format!("/{token}/{RELOAD}.js"), &[]);
        assert_eq!(status, 200);
        assert_eq!(headers["content-type"], "text/javascript; charset=utf-8");
        assert!(String::from_utf8_lossy(&script).contains("location.reload"));
        assert_eq!(
            request(address, "GET", &format!("/{RELOAD}.js"), &[]).0,
            404
        );
    }
}
