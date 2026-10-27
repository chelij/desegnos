//! Import a website as canvas elements.
//!
//! Native code fetches a public site or reads a local folder/ZIP with the same bounded, no-code
//! rules as brand import, serves the snapshot from a loopback server, and turns the browser's
//! rendered-tree captures into project elements. Page code never runs: the served page is
//! rendered inside a script-disabled nested frame, and only our extraction wrapper runs.
use crate::local_site::ImportSite;
use crate::workspace::Workspace;
use base64::{Engine, engine::general_purpose::STANDARD};
use desegnos_core::WebCapture;
use desegnos_storage::{
    WebsiteSnapshot, collect_website_snapshot, new_import_folder, write_website_snapshot,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use desegnos_ai::WebUrl as Url;
use uuid::Uuid;

type NativeResult<T> = Result<T, String>;
type ImportState<'a> = State<'a, Mutex<WebsiteImportState>>;
type WorkspaceState<'a> = State<'a, Mutex<Workspace>>;

const MAX_CAPTURE_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMPORTED_PICTURES: usize = 160;
const MAX_IMPORTED_PICTURE_TOTAL: usize = 64 * 1024 * 1024;
const SERVED_FILE_LIMIT: usize = 128 * 1024 * 1024;

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn trusted(window: &WebviewWindow) -> NativeResult<()> {
    if window.label() == "editor" {
        Ok(())
    } else {
        Err("Only the Desegnos editor can import a website.".into())
    }
}

struct ImportSource {
    folder: PathBuf,
    base_url: String,
    pages: Vec<ImportPage>,
}

struct ImportPage {
    route: String,
    title: String,
}

/// The one website snapshot currently served for rendering.
#[derive(Default)]
pub struct WebsiteImportState {
    site: ImportSite,
    source: Option<ImportSource>,
}

impl Drop for WebsiteImportState {
    fn drop(&mut self) {
        self.clear();
    }
}

impl WebsiteImportState {
    fn clear(&mut self) {
        self.site.stop();
        if let Some(source) = self.source.take() {
            let _ = fs::remove_dir_all(source.folder);
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsiteImportPageView {
    route: String,
    title: String,
    url: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsiteImportView {
    id: Uuid,
    name: String,
    base_url: String,
    extractor_url: String,
    pages: Vec<WebsiteImportPageView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsiteImportResultView {
    workspace: crate::workspace::WorkspaceView,
    pages: usize,
    layers: usize,
    pictures: usize,
    warnings: Vec<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WebsiteSourceKind {
    Folder,
    Zip,
}

fn extractor_html(script: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Importing page</title>\
<style>html,body{{margin:0;padding:0;background:#fff}}#stage{{display:block}}</style></head>\
<body><div id=\"stage\"></div><script>{script}</script></body></html>"
    )
}

fn install(
    state: &Mutex<WebsiteImportState>,
    snapshot: WebsiteSnapshot,
    extractor: &str,
) -> NativeResult<WebsiteImportView> {
    let folder = new_import_folder().map_err(message)?;
    write_website_snapshot(&folder, &snapshot).map_err(message)?;
    fs::write(folder.join("__desegnos_extract.html"), extractor_html(extractor))
        .map_err(message)?;
    let name = snapshot.name.clone();
    let mut seen_routes = BTreeSet::new();
    let mut pages = snapshot
        .pages
        .iter()
        .filter(|page| {
            let key = page.route.trim_end_matches('/').to_ascii_lowercase();
            seen_routes.insert(if key.is_empty() { "/".into() } else { key })
        })
        .map(|page| ImportPage {
            route: page.route.clone(),
            title: page.title.clone(),
        })
        .collect::<Vec<_>>();
    pages.truncate(30);
    let mut state = state.lock().map_err(message)?;
    state.clear();
    let base_url = state
        .site
        .serve(&folder, SERVED_FILE_LIMIT)
        .map_err(message)?;
    let id = Uuid::new_v4();
    let view = WebsiteImportView {
        id,
        name: name.clone(),
        base_url: base_url.clone(),
        extractor_url: format!("{base_url}__desegnos_extract.html"),
        pages: pages
            .iter()
            .map(|page| WebsiteImportPageView {
                route: page.route.clone(),
                title: page.title.clone(),
                url: format!("{base_url}{}", page.route.trim_start_matches('/')),
            })
            .collect(),
    };
    state.source = Some(ImportSource {
        folder,
        base_url,
        pages,
    });
    Ok(view)
}

/// Fetch a public website address, serve its snapshot and list the pages to render.
#[tauri::command]
pub async fn prepare_website_import_url(
    window: WebviewWindow,
    state: ImportState<'_>,
    url: String,
    extractor: String,
) -> NativeResult<WebsiteImportView> {
    trusted(&window)?;
    #[cfg(feature = "native-tests")]
    let fixture = std::env::var("DESEGNOS_NATIVE_TEST_WEBSITE_ORIGIN")
        .ok()
        .and_then(|value| desegnos_ai::WebUrl::parse(&value).ok());
    #[cfg(not(feature = "native-tests"))]
    let fixture: Option<desegnos_ai::WebUrl> = None;
    let snapshot = desegnos_ai::fetch_website_snapshot(&url, fixture.as_ref()).await?;
    install(
        &state,
        WebsiteSnapshot {
            name: snapshot.name,
            files: snapshot.files,
            pages: snapshot
                .pages
                .into_iter()
                .map(|page| desegnos_storage::WebsitePage {
                    route: page.route,
                    title: page.title,
                })
                .collect(),
            skipped: snapshot.skipped,
        },
        &extractor,
    )
}

/// Choose a website folder or ZIP archive, serve it and list its HTML pages.
#[tauri::command]
pub async fn pick_website_import_source(
    app: AppHandle,
    window: WebviewWindow,
    state: ImportState<'_>,
    kind: WebsiteSourceKind,
    extractor: String,
) -> NativeResult<Option<WebsiteImportView>> {
    trusted(&window)?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file();
        let picked = crate::file_dialogs::picked(match kind {
            WebsiteSourceKind::Folder => dialog.blocking_pick_folder().map(|path| vec![path]),
            WebsiteSourceKind::Zip => dialog
                .add_filter("Website ZIP", &["zip"])
                .blocking_pick_file()
                .map(|path| vec![path]),
        })?;
        let Some(picked) = picked else { return Ok(None) };
        let path = picked
            .into_iter()
            .next()
            .ok_or("Choose a website folder or ZIP.")?
            .into_path()
            .map_err(message)?;
        let snapshot = collect_website_snapshot(&path).map_err(message)?;
        Ok::<_, String>(Some(snapshot))
    })
    .await
    .map_err(message)??;
    picked
        .map(|snapshot| install(&state, snapshot, &extractor))
        .transpose()
}

#[tauri::command]
pub fn cancel_website_import(state: ImportState<'_>) -> NativeResult<()> {
    state.lock().map_err(message)?.clear();
    Ok(())
}

fn image_mime(name: &str) -> Option<&'static str> {
    match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "svg" => Some("image/svg+xml"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

fn decode_path(path: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(path.len());
    let mut chars = path.bytes();
    while let Some(byte) = chars.next() {
        bytes.push(if byte == b'%' {
            let high = (chars.next()? as char).to_digit(16)?;
            let low = (chars.next()? as char).to_digit(16)?;
            (high * 16 + low) as u8
        } else {
            byte
        });
    }
    let value = String::from_utf8(bytes).ok()?;
    if value.is_empty()
        || value.contains(['\\', '\0'])
        || value.chars().any(char::is_control)
        || value.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    Some(value)
}

fn local_image(folder: &Path, base_url: &str, source: &str) -> Option<Vec<u8>> {
    let url = Url::parse(source).ok()?;
    let base = Url::parse(base_url).ok()?;
    let rest = url.path().strip_prefix(base.path())?.trim_start_matches('/');
    let path = decode_path(rest)?;
    let mut file = folder.to_path_buf();
    for part in path.split('/') {
        file.push(part);
    }
    let canonical = file.canonicalize().ok()?;
    if !canonical.starts_with(folder) {
        return None;
    }
    let metadata = fs::metadata(&canonical).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CAPTURE_IMAGE_BYTES as u64 {
        return None;
    }
    fs::read(canonical).ok()
}

fn data_uri(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

fn element_count(page: &desegnos_core::Page) -> usize {
    fn count(elements: &[desegnos_core::Element]) -> usize {
        elements
            .iter()
            .map(|element| 1 + count(&element.children))
            .sum()
    }
    count(&page.elements)
}

/// Turn the browser captures into canvas elements and apply the whole import as one undo step.
#[tauri::command]
pub async fn import_website_pages(
    window: WebviewWindow,
    state: ImportState<'_>,
    workspace: WorkspaceState<'_>,
    captures: Vec<WebCapture>,
) -> NativeResult<WebsiteImportResultView> {
    trusted(&window)?;
    if captures.is_empty() {
        return Err("Render at least one page before importing it.".into());
    }
    if captures.len() > 60 {
        return Err("A website import renders at most 30 pages.".into());
    }
    let (folder, base_url, pages) = {
        let state = state.lock().map_err(message)?;
        let source = state
            .source
            .as_ref()
            .ok_or("Read a website or choose a folder before importing.")?;
        (
            source.folder.clone(),
            source.base_url.clone(),
            source.pages.iter().map(|page| page.route.clone()).collect::<Vec<_>>(),
        )
    };
    let allowed = pages.into_iter().collect::<BTreeSet<_>>();
    for capture in &captures {
        let Some(rest) = capture.url.strip_prefix(&base_url) else {
            return Err("A captured page is not part of the website being imported.".into());
        };
        let route = if rest.is_empty() {
            "/".to_owned()
        } else {
            format!("/{rest}")
        };
        if !allowed.contains(&route) {
            return Err("A captured page is not part of the website being imported.".into());
        }
    }
    let sources = desegnos_core::web_capture_image_sources(&captures);
    if sources.len() > MAX_IMPORTED_PICTURES {
        return Err(format!(
            "This import uses {} pictures; the limit is {MAX_IMPORTED_PICTURES}. Choose a smaller site.",
            sources.len()
        ));
    }
    #[cfg(feature = "native-tests")]
    let fixture = std::env::var("DESEGNOS_NATIVE_TEST_WEBSITE_ORIGIN")
        .ok()
        .and_then(|value| desegnos_ai::WebUrl::parse(&value).ok());
    #[cfg(not(feature = "native-tests"))]
    let fixture: Option<desegnos_ai::WebUrl> = None;
    let mut picture_ids = BTreeMap::new();
    let mut pictures = BTreeMap::new();
    let mut total = 0usize;
    let mut warnings = Vec::new();
    for source in sources {
        let resolved = if source.starts_with("data:image/") {
            parse_data_uri(&source)
        } else if source.starts_with(&base_url) {
            let path = Url::parse(&source).map(|url| url.path().to_owned()).unwrap_or_else(|_| source.clone());
            local_image(&folder, &base_url, &source)
                .and_then(|bytes| image_mime(&path).map(|mime| (mime.to_owned(), bytes)))
        } else {
            match desegnos_ai::fetch_website_image(&source, fixture.as_ref()).await {
                Ok((bytes, mime)) => Some((mime, bytes)),
                Err(reason) => {
                    warnings.push(reason);
                    None
                }
            }
        };
        let Some((mime, bytes)) = resolved else {
            continue;
        };
        if bytes.is_empty() || bytes.len() > MAX_CAPTURE_IMAGE_BYTES {
            warnings.push(format!("A picture over 8 MiB was left out: {source}"));
            continue;
        }
        total = total.saturating_add(bytes.len());
        if total > MAX_IMPORTED_PICTURE_TOTAL {
            warnings.push("Picture downloads reached the import size limit.".into());
            break;
        }
        let uri = data_uri(&mime, &bytes);
        let (picture, stored) = desegnos_core::Project::prepare_picture(&uri, 1440).map_err(message)?;
        picture_ids.insert(source, picture);
        pictures.insert(picture, stored);
    }
    let project = workspace
        .lock()
        .map_err(message)?
        .project()
        .map_err(message)?
        .clone();
    let imported = desegnos_core::import_website(&captures, &picture_ids, &project).map_err(message)?;
    let page_count = imported.pages.len();
    let layers = imported
        .pages
        .iter()
        .map(element_count)
        .sum::<usize>();
    warnings.extend(imported.warnings.iter().cloned());
    let picture_count = pictures.len();
    let view = workspace
        .lock()
        .map_err(message)?
        .import_website(imported, pictures)?;
    state.lock().map_err(message)?.clear();
    Ok(WebsiteImportResultView {
        workspace: view,
        pages: page_count,
        layers,
        pictures: picture_count,
        warnings,
    })
}

fn parse_data_uri(uri: &str) -> Option<(String, Vec<u8>)> {
    let (header, encoded) = uri.strip_prefix("data:")?.split_once(";base64,")?;
    let bytes = STANDARD.decode(encoded).ok()?;
    Some((header.to_owned(), bytes))
}
