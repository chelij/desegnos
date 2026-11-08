//! Import a website as canvas elements.
//!
//! Native code fetches a public site or reads a local folder/ZIP with the same bounded, no-code
//! rules as brand import, serves the snapshot from a loopback server, and turns the browser's
//! rendered-tree captures into project elements. Page code never runs: the served page is
//! rendered inside a script-disabled nested frame, and only our extraction wrapper runs.
use crate::local_site::{LocalSite, decode_path as decode_site_path};
use crate::native_files::{NativeResult, message};
use crate::workspace::Workspace;
use base64::{Engine, engine::general_purpose::STANDARD};
use desegnos_ai::WebUrl as Url;
use desegnos_core::WebCapture;
use desegnos_storage::{WebsiteSnapshot, new_import_folder, write_website_snapshot};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

type ImportState<'a> = State<'a, Mutex<WebsiteImportState>>;
type WorkspaceState<'a> = State<'a, Mutex<Workspace>>;

use desegnos_core::limits::{
    DEFAULT_PICTURE_WIDTH, MAX_IMPORT_CAPTURES, MAX_IMPORT_PAGES,
    MAX_WEBSITE_FILE_BYTES as MAX_CAPTURE_IMAGE_BYTES,
};
const MAX_IMPORTED_PICTURES: usize = 160;
const MAX_IMPORTED_PICTURE_TOTAL: usize = 64 * 1024 * 1024;
const SERVED_FILE_LIMIT: usize = 128 * 1024 * 1024;

struct ImportSource {
    folder: PathBuf,
    base_url: String,
    pages: Vec<ImportPage>,
}

struct ImportPage {
    file: String,
    route: String,
    title: String,
}

/// The one website snapshot currently served for rendering.
#[derive(Default)]
pub struct WebsiteImportState {
    revision: u64,
    site: LocalSite,
    source: Option<ImportSource>,
}

impl Drop for WebsiteImportState {
    fn drop(&mut self) {
        self.clear();
    }
}

impl WebsiteImportState {
    fn clear(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.site.stop();
        if let Some(source) = self.source.take() {
            let _ = fs::remove_dir_all(source.folder);
        }
    }
}

#[derive(ts_rs::TS, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsiteImportPageView {
    route: String,
    title: String,
    url: String,
}

#[derive(ts_rs::TS, Clone, Serialize)]
#[ts(rename = "WebsiteImportSourceView")]
#[serde(rename_all = "camelCase")]
pub struct WebsiteImportView {
    id: Uuid,
    name: String,
    base_url: String,
    extractor_url: String,
    pages: Vec<WebsiteImportPageView>,
    total_pages: usize,
    identity_source: Option<crate::identity::IdentitySourceManifest>,
    identity_error: Option<String>,
    skipped: Vec<String>,
}

#[derive(ts_rs::TS, Serialize)]
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
    File,
}

fn extractor_html(script: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Importing page</title>\
<style>html,body{{margin:0;padding:0;background:#fff}}#stage{{display:block}}</style></head>\
<body><div id=\"stage\"></div><script>{script}</script></body></html>"
    )
}

fn page_url(base_url: &str, file: &str) -> String {
    let mut url = Url::parse(base_url).expect("local server URL");
    url.path_segments_mut()
        .expect("local server path")
        .pop_if_empty()
        .extend(file.split('/'));
    url.to_string()
}

fn begin_read(state: &Mutex<WebsiteImportState>) -> NativeResult<u64> {
    let mut state = state.lock().map_err(message)?;
    state.revision = state.revision.wrapping_add(1);
    Ok(state.revision)
}

fn install(
    state: &Mutex<WebsiteImportState>,
    revision: u64,
    snapshot: WebsiteSnapshot,
    identity: &Mutex<crate::identity::IdentityService>,
    project: Option<desegnos_storage::IdentitySource>,
    extractor: &str,
) -> NativeResult<WebsiteImportView> {
    let mut state = state.lock().map_err(message)?;
    if state.revision != revision {
        return Err("The import was stopped.".into());
    }
    let (identity_source, identity_error) = match identity
        .lock()
        .map_err(message)?
        .retain_import_snapshot(&snapshot, project)
    {
        Ok(source) => (Some(source), None),
        Err(reason) => (None, Some(reason)),
    };
    let folder = new_import_folder().map_err(message)?;
    write_website_snapshot(&folder, &snapshot).map_err(message)?;
    fs::write(
        folder.join("__desegnos_extract.html"),
        extractor_html(extractor),
    )
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
            file: page.file.clone(),
            route: page.route.clone(),
            title: page.title.clone(),
        })
        .collect::<Vec<_>>();
    let total_pages = pages.len();
    pages.truncate(MAX_IMPORT_PAGES);
    state.clear();
    let base_url = state
        .site
        .serve(&folder, SERVED_FILE_LIMIT)
        .map_err(message)?;
    let id = Uuid::new_v4();
    let view = WebsiteImportView {
        id,
        name: name.clone(),
        total_pages,
        identity_source,
        identity_error,
        skipped: snapshot.skipped.clone(),
        base_url: base_url.clone(),
        extractor_url: format!("{base_url}__desegnos_extract.html"),
        pages: pages
            .iter()
            .map(|page| WebsiteImportPageView {
                route: page.route.clone(),
                title: page.title.clone(),
                url: page_url(&base_url, &page.file),
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

#[tauri::command]
pub async fn prepare_website_import_url(
    state: ImportState<'_>,
    identity: State<'_, Mutex<crate::identity::IdentityService>>,
    url: String,
    extractor: String,
) -> NativeResult<WebsiteImportView> {
    let revision = begin_read(&state)?;
    let fetcher = desegnos_ai::WebsiteFetcher::new();
    #[cfg(feature = "native-tests")]
    let fetcher = match std::env::var("DESEGNOS_NATIVE_TEST_WEBSITE_ORIGIN") {
        Ok(origin) => desegnos_ai::WebsiteFetcher::for_fixture(
            &desegnos_ai::WebUrl::parse(&origin).map_err(message)?,
        )?,
        Err(std::env::VarError::NotPresent) => fetcher,
        Err(error) => return Err(message(error)),
    };
    let snapshot = fetcher.fetch_website_snapshot(&url).await?;
    install(&state, revision, snapshot, &identity, None, &extractor)
}

#[tauri::command]
pub async fn pick_website_import_source(
    app: AppHandle,
    state: ImportState<'_>,
    identity: State<'_, Mutex<crate::identity::IdentityService>>,
    kind: WebsiteSourceKind,
    extractor: String,
) -> NativeResult<Option<WebsiteImportView>> {
    let revision = begin_read(&state)?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file();
        let picked = crate::file_dialogs::picked(match kind {
            WebsiteSourceKind::Folder => dialog.blocking_pick_folder().map(|path| vec![path]),
            WebsiteSourceKind::File => dialog
                .add_filter("HTML files", &["html", "htm"])
                .add_filter("Desegnos project", &["desegnos"])
                .blocking_pick_files(),
            WebsiteSourceKind::Zip => dialog
                .add_filter("Website ZIP", &["zip"])
                .blocking_pick_file()
                .map(|path| vec![path]),
        })?;
        let Some(picked) = picked else {
            return Ok(None);
        };
        let paths = picked
            .into_iter()
            .map(|path| path.into_path().map_err(message))
            .collect::<NativeResult<Vec<_>>>()?;
        if matches!(kind, WebsiteSourceKind::File)
            && paths.len() == 1
            && paths[0]
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("desegnos"))
        {
            let source = desegnos_storage::collect_identity_source(&paths[0]).map_err(message)?;
            let snapshot = WebsiteSnapshot {
                name: source.name.clone(),
                ..Default::default()
            };
            return Ok(Some((snapshot, Some(source))));
        }
        let snapshot = if matches!(kind, WebsiteSourceKind::File) {
            desegnos_storage::website_snapshot::collect_import_html(&paths)
        } else {
            desegnos_storage::website_snapshot::collect_import_snapshot(
                paths.first().ok_or("Choose a website folder or ZIP.")?,
            )
        }
        .map_err(message)?;
        Ok::<_, String>(Some((snapshot, None)))
    })
    .await
    .map_err(message)??;
    picked
        .map(|(snapshot, project)| {
            install(&state, revision, snapshot, &identity, project, &extractor)
        })
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

/// A site path with no empty, `.` or `..` segment.
fn decode_path(path: &str) -> Option<String> {
    decode_site_path(path)
        .filter(|value| !value.is_empty() && value.split('/').all(|part| !part.is_empty()))
}

fn local_image(folder: &Path, base_url: &str, source: &str) -> Option<Vec<u8>> {
    let url = Url::parse(source).ok()?;
    let base = Url::parse(base_url).ok()?;
    let rest = url
        .path()
        .strip_prefix(base.path())?
        .trim_start_matches('/');
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

#[tauri::command]
pub async fn import_website_pages(
    state: ImportState<'_>,
    workspace: WorkspaceState<'_>,
    captures: Vec<WebCapture>,
) -> NativeResult<WebsiteImportResultView> {
    if captures.is_empty() {
        return Err("Render at least one page before importing it.".into());
    }
    if captures.len() > MAX_IMPORT_CAPTURES {
        return Err(format!(
            "A website import renders at most {MAX_IMPORT_PAGES} pages at two widths."
        ));
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
            source
                .pages
                .iter()
                .map(|page| (page_url(&source.base_url, &page.file), page.route.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    };
    for capture in &captures {
        if pages.get(&capture.url) != Some(&capture.route) {
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
    let fetcher = desegnos_ai::WebsiteFetcher::new();
    #[cfg(feature = "native-tests")]
    let fetcher = match std::env::var("DESEGNOS_NATIVE_TEST_WEBSITE_ORIGIN") {
        Ok(origin) => desegnos_ai::WebsiteFetcher::for_fixture(
            &desegnos_ai::WebUrl::parse(&origin).map_err(message)?,
        )?,
        Err(std::env::VarError::NotPresent) => fetcher,
        Err(error) => return Err(message(error)),
    };
    let mut picture_ids = BTreeMap::new();
    let mut pictures = BTreeMap::new();
    let mut total = 0usize;
    let mut warnings = Vec::new();
    for source in sources {
        let resolved = if source.starts_with("data:image/") {
            parse_data_uri(&source)
        } else if source.starts_with(&base_url) {
            let path = Url::parse(&source)
                .map(|url| url.path().to_owned())
                .unwrap_or_else(|_| source.clone());
            local_image(&folder, &base_url, &source)
                .and_then(|bytes| image_mime(&path).map(|mime| (mime.to_owned(), bytes)))
        } else {
            match fetcher.fetch_website_image(&source).await {
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
            warnings.push(format!(
                "A picture over {} MiB was left out: {source}",
                MAX_CAPTURE_IMAGE_BYTES / 1024 / 1024
            ));
            continue;
        }
        total = total.saturating_add(bytes.len());
        if total > MAX_IMPORTED_PICTURE_TOTAL {
            warnings.push("Picture downloads reached the import size limit.".into());
            break;
        }
        let uri = data_uri(&mime, &bytes);
        let (picture, stored) =
            desegnos_core::Project::prepare_picture(&uri, DEFAULT_PICTURE_WIDTH)
                .map_err(message)?;
        picture_ids.insert(source, picture);
        pictures.insert(picture, stored);
    }
    let project = workspace
        .lock()
        .map_err(message)?
        .project()
        .map_err(message)?
        .clone();
    let imported =
        desegnos_core::import_website(&captures, &picture_ids, &project).map_err(message)?;
    let page_count = imported.pages.len();
    let layers = imported.pages.iter().map(element_count).sum::<usize>();
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
