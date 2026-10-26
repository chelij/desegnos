use desegnos_core::{GenerationCandidate, PreviewFile, Project};
use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Weak},
};
use tauri::http::{HeaderValue, Request, Response, StatusCode};
use uuid::Uuid;

#[cfg(target_os = "linux")]
pub fn configure_window(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|webview| {
        #[cfg(feature = "native-tests")]
        use webkit2gtk::SettingsExt;
        use webkit2gtk::{
            EditorStateExt, HitTestResultExt, SecurityManagerExt, WebContextExt, WebViewExt,
        };
        webview.inner().connect_context_menu(|view, _, _, hit| {
            // HitTestResult labels every text input editable, including inert
            // canvas previews. Only a genuinely focused editor field keeps
            // native editing menus; canvas menus belong to Desegnos.
            !hit.context_is_editable()
                || !view
                    .editor_state()
                    .is_some_and(|state| state.is_paste_available())
        });
        if let Some(manager) = webview
            .inner()
            .context()
            .and_then(|context| context.security_manager())
        {
            manager.register_uri_scheme_as_cors_enabled("desegnos-preview");
        }
        #[cfg(feature = "native-tests")]
        if let Some(settings) = webview.inner().settings() {
            settings.set_enable_write_console_messages_to_stdout(true);
        }
    })
}

const MAX_PREVIEW_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PREVIEW_SESSIONS: usize = 8;
/// Site files are served by their own session under both platform URL forms; https is for
/// pages that embed an external site. Both stay on the preview scheme and hosts, which only
/// this application answers.
const EMBED_SOURCES: &str =
    "desegnos-preview: http://*.desegnos-preview.localhost http://desegnos-preview.localhost https:";
/// The policy a generated page is served with before its session adds its own script and
/// style sources. Frames and fetches reach this session's files and an https embed.
fn preview_policy() -> String {
    format!(
        "default-src 'none'; script-src 'none'; script-src-attr 'none'; style-src 'unsafe-inline'; img-src data:; font-src 'none'; connect-src {EMBED_SOURCES}; frame-src {EMBED_SOURCES}; child-src {EMBED_SOURCES}; object-src 'none'; base-uri 'none'; form-action 'none'; media-src 'none'; worker-src 'none'"
    )
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDescriptor {
    pub url: String,
    pub session_id: Uuid,
    pub source_sha256: Option<String>,
    pub content: Option<desegnos_core::SiteContent>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub website: bool,
}

struct Resource {
    content_type: &'static str,
    body: PreviewFile,
}

/// One rendered site, shared by every session previewing the same project revision.
struct SitePackage {
    files: BTreeMap<String, Resource>,
}

struct PreviewSession {
    resources: BTreeMap<(Uuid, &'static str), Resource>,
    site: Option<Arc<SitePackage>>,
    extras: BTreeMap<String, Resource>,
    bridges: BTreeMap<String, String>,
    scripts_enabled: bool,
}

#[derive(Default)]
pub struct PreviewRegistry {
    sessions: BTreeMap<Uuid, PreviewSession>,
    order: VecDeque<Uuid>,
    packages: BTreeMap<(Uuid, u64, bool), Weak<SitePackage>>,
}

fn attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn resource_url(session_id: Uuid, page_id: Uuid) -> String {
    let prefix = if cfg!(target_os = "windows") {
        "http://desegnos-preview.localhost"
    } else {
        "desegnos-preview://localhost"
    };
    format!("{prefix}/{session_id}/{page_id}/index.html")
}

impl PreviewRegistry {
    pub fn register(
        &mut self,
        project: &Project,
        candidate: Option<&GenerationCandidate>,
        page_id: Uuid,
        run_scripts: bool,
    ) -> Result<PreviewDescriptor, String> {
        project.validate().map_err(|error| error.to_string())?;
        if !project.pages.iter().any(|page| page.id == page_id) {
            return Err("The preview page no longer exists.".into());
        }
        if project.website.is_some() && candidate.is_none() {
            return self.register_website(project, page_id, run_scripts);
        }
        let session_id = Uuid::new_v4();
        let routes: BTreeMap<_, _> = project
            .pages
            .iter()
            .map(|page| {
                let path = page.path.trim_end_matches('/').to_lowercase();
                (
                    if path.is_empty() { "/".into() } else { path },
                    page.id.to_string(),
                )
            })
            .collect();
        let routes = attribute(&serde_json::to_string(&routes).map_err(|error| error.to_string())?);
        let mut preview_content = None;
        let mut resources = BTreeMap::new();
        let mut byte_len = 0_usize;
        for page in &project.pages {
            let content = candidate
                .and_then(|candidate| {
                    candidate
                        .pages
                        .iter()
                        .find(|generated| generated.page_id == page.id)
                        .map(|generated| &generated.content)
                })
                .or(page.content.as_ref());
            let resolved = content
                .map(|content| project.resolve_site_content(content))
                .transpose()
                .map_err(|error| error.to_string())?;
            if page.id == page_id {
                preview_content = resolved.clone();
            }
            let (html, css, javascript) = resolved
                .as_ref()
                .map(|content| {
                    (
                        content.html.as_str(),
                        content.css.as_str(),
                        content.javascript.as_str(),
                    )
                })
                .unwrap_or(("", "", ""));
            let bridge_script = if run_scripts {
                format!(
                    "<script src=\"./bridge.js\" crossorigin=\"anonymous\" data-session=\"{session_id}\" data-page=\"{}\" data-path=\"{}\" data-routes=\"{routes}\"></script>",
                    page.id,
                    attribute(&page.path)
                )
            } else {
                String::new()
            };
            let user_script = if run_scripts && !javascript.trim().is_empty() {
                "<script src=\"./site.js\" crossorigin=\"anonymous\"></script>"
            } else {
                ""
            };
            let region_javascript =
                desegnos_core::region_script_runtime(html).map_err(|error| error.to_string())?;
            let region_script = if run_scripts && !region_javascript.is_empty() {
                "<script src=\"./regions.js\" crossorigin=\"anonymous\"></script>"
            } else {
                ""
            };
            let document = format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"./site.css\" crossorigin=\"anonymous\"></head><body>{html}{bridge_script}{user_script}{region_script}</body></html>",
                attribute(&page.name)
            );
            let mut files = vec![
                (
                    "index.html",
                    "text/html; charset=utf-8",
                    document.into_bytes(),
                ),
                (
                    "site.css",
                    "text/css; charset=utf-8",
                    css.as_bytes().to_vec(),
                ),
            ];
            if run_scripts {
                files.push((
                    "bridge.js",
                    "text/javascript; charset=utf-8",
                    include_bytes!("preview_bridge.js").to_vec(),
                ));
                files.push((
                    "regions.js",
                    "text/javascript; charset=utf-8",
                    region_javascript.into_bytes(),
                ));
                files.push((
                    "site.js",
                    "text/javascript; charset=utf-8",
                    javascript.as_bytes().to_vec(),
                ));
            }
            for (name, content_type, bytes) in files {
                byte_len = byte_len
                    .checked_add(bytes.len())
                    .ok_or("The preview package is too large.")?;
                if byte_len > MAX_PREVIEW_TEXT_BYTES {
                    return Err(format!(
                        "This page's code is too large to preview ({} MB).",
                        byte_len.div_ceil(1024 * 1024)
                    ));
                }
                resources.insert(
                    (page.id, name),
                    Resource {
                        content_type,
                        body: PreviewFile::Inline(bytes.into()),
                    },
                );
            }
        }
        while self.sessions.len() >= MAX_PREVIEW_SESSIONS {
            let Some(expired) = self.order.pop_front() else {
                break;
            };
            self.sessions.remove(&expired);
        }
        self.sessions.insert(
            session_id,
            PreviewSession {
                resources,
                site: None,
                extras: BTreeMap::new(),
                bridges: BTreeMap::new(),
                scripts_enabled: run_scripts,
            },
        );
        self.order.push_back(session_id);
        Ok(PreviewDescriptor {
            website: false,
            content: preview_content,
            url: resource_url(session_id, page_id),
            session_id,
            source_sha256: candidate
                .and_then(|candidate| {
                    candidate
                        .pages
                        .iter()
                        .find(|page| page.page_id == page_id)
                        .map(|page| &page.content)
                })
                .or_else(|| {
                    project
                        .pages
                        .iter()
                        .find(|page| page.id == page_id)
                        .and_then(|page| page.content.as_ref())
                })
                .map(desegnos_core::SiteContent::sha256)
                .transpose()
                .map_err(|error| error.to_string())?,
        })
    }

    fn register_website(
        &mut self,
        project: &Project,
        page_id: Uuid,
        run_scripts: bool,
    ) -> Result<PreviewDescriptor, String> {
        let session_id = Uuid::new_v4();
        let page = project.page(page_id).map_err(|e| e.to_string())?;
        let path = project
            .website
            .as_ref()
            .expect("website")
            .page_files
            .get(&page_id)
            .ok_or("This page has no imported file.")?
            .clone();
        // Static and live frames of the same revision share one rendered package.
        let key = (project.id, project.revision, run_scripts);
        let package = match self.packages.get(&key).and_then(Weak::upgrade) {
            Some(package) => package,
            None => {
                let files = project
                    .website_preview_package(run_scripts, MAX_PREVIEW_TEXT_BYTES)
                    .map_err(|e| e.to_string())?
                    .expect("website");
                let package = Arc::new(SitePackage {
                    files: files
                        .into_iter()
                        .map(|(path, body)| {
                            let content_type = site_mime(&path);
                            (path, Resource { content_type, body })
                        })
                        .collect(),
                });
                // Sessions hold the strong references; unused packages disappear here.
                self.packages
                    .retain(|_, package| package.strong_count() > 0);
                self.packages.insert(key, Arc::downgrade(&package));
                package
            }
        };
        let bridge_name = format!("desegnos-{session_id}.js");
        let mut extras = BTreeMap::new();
        extras.insert(
            bridge_name.clone(),
            Resource {
                content_type: "text/javascript; charset=utf-8",
                body: PreviewFile::Inline(include_bytes!("preview_bridge.js").to_vec().into()),
            },
        );
        // The bridge binds a page to this session, so it is injected when serving, not stored.
        let mut bridges = BTreeMap::new();
        if run_scripts {
            for (id, file) in &project.website.as_ref().expect("website").page_files {
                if package.files.contains_key(file) {
                    let path = project
                        .pages
                        .iter()
                        .find(|page| page.id == *id)
                        .map_or("/", |page| page.path.as_str());
                    bridges.insert(
                        file.clone(),
                        format!(
                            "<script src=\"/{bridge_name}\" crossorigin=\"anonymous\" data-file-mode=\"true\" data-session=\"{session_id}\" data-page=\"{id}\" data-path=\"{}\" data-routes=\"{{}}\"></script>",
                            attribute(path)
                        ),
                    );
                }
            }
            // Framed site files are served by this session too, so they carry the same bridge
            // as the page that framed them: scripts run and links stay inside the preview.
            for (path, resource) in &package.files {
                if resource.content_type.starts_with("text/html") {
                    bridges.entry(path.clone()).or_insert_with(|| {
                        format!(
                            "<script src=\"/{bridge_name}\" crossorigin=\"anonymous\" data-file-mode=\"true\" data-session=\"{session_id}\" data-page=\"{page_id}\" data-path=\"/{}\" data-routes=\"{{}}\"></script>",
                            attribute(path)
                        )
                    });
                }
            }
        }
        while self.sessions.len() >= MAX_PREVIEW_SESSIONS {
            let Some(expired) = self.order.pop_front() else {
                break;
            };
            self.sessions.remove(&expired);
        }
        self.sessions.insert(
            session_id,
            PreviewSession {
                resources: BTreeMap::new(),
                site: Some(package),
                extras,
                bridges,
                scripts_enabled: run_scripts,
            },
        );
        self.order.push_back(session_id);
        let prefix = if cfg!(target_os = "windows") {
            format!("http://{session_id}.desegnos-preview.localhost")
        } else {
            format!("desegnos-preview://{session_id}.localhost")
        };
        Ok(PreviewDescriptor {
            url: format!("{prefix}/{path}"),
            session_id,
            website: true,
            source_sha256: page
                .content
                .as_ref()
                .map(desegnos_core::SiteContent::sha256)
                .transpose()
                .map_err(|e| e.to_string())?,
            content: if run_scripts {
                None
            } else {
                project
                    .website_canvas_content(page)
                    .map_err(|e| e.to_string())?
            },
        })
    }

    pub fn clear(&mut self) {
        self.sessions.clear();
        self.order.clear();
        self.packages.clear();
    }

    pub fn release(&mut self, session_id: Uuid) {
        self.sessions.remove(&session_id);
        self.order.retain(|id| *id != session_id);
    }

    pub fn scripts_enabled(&self) -> bool {
        self.sessions
            .values()
            .any(|session| session.scripts_enabled)
    }

    pub fn serve(&self, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
        let denied = || {
            response(
                StatusCode::NOT_FOUND,
                "text/plain; charset=utf-8",
                b"Preview resource is unavailable.".to_vec(),
            )
        };
        if !matches!(request.method().as_str(), "GET" | "HEAD") || !request.body().is_empty() {
            return denied();
        }
        let host = request.uri().host().unwrap_or_default();
        if let Some(id) = host
            .strip_suffix(".localhost")
            .and_then(|h| h.strip_suffix(".desegnos-preview").or(Some(h)))
            .and_then(|h| Uuid::parse_str(h).ok())
        {
            let Some(session) = self.sessions.get(&id) else {
                return denied();
            };
            let Some(decoded) = decode_site_path(request.uri().path()) else {
                return denied();
            };
            let path = decoded.trim_start_matches('/');
            // A file is served at its own path exactly as stored.
            if !path.is_empty()
                && let Some(resource) = session.file(path)
            {
                return site_response(
                    id,
                    session,
                    resource,
                    session.bridges.get(path).map(String::as_str),
                    request,
                    StatusCode::OK,
                );
            }
            let directory = path.trim_end_matches('/');
            let mut candidates = Vec::new();
            if directory.is_empty() {
                candidates.extend(["index.html".to_owned(), "index.htm".to_owned()]);
            } else {
                candidates.extend([
                    format!("{directory}/index.html"),
                    format!("{directory}/index.htm"),
                    format!("{directory}.html"),
                    format!("{directory}.htm"),
                ]);
            }
            let resolved = candidates
                .into_iter()
                .find_map(|candidate| session.file(&candidate).map(|file| (candidate, file)));
            if let Some((name, resource)) = resolved {
                // A directory route loads its index at the address that keeps the page's
                // relative links correct, the same as an ordinary web server.
                if !path.is_empty() && !path.ends_with('/') && is_index_name(&name) {
                    return redirect(request);
                }
                return site_response(
                    id,
                    session,
                    resource,
                    session.bridges.get(&name).map(String::as_str),
                    request,
                    StatusCode::OK,
                );
            }
            // A site's own 404 page answers missing paths, with the same bridge and policy
            // as any other page the session serves.
            let fallback = session
                .file("404.html")
                .map(|resource| ("404.html".to_owned(), resource))
                .or_else(|| {
                    session.site.as_ref().and_then(|package| {
                        package
                            .files
                            .iter()
                            .find(|(name, _)| name.eq_ignore_ascii_case("404.html"))
                            .map(|(name, resource)| (name.clone(), resource))
                    })
                });
            let Some((name, resource)) = fallback else {
                return denied();
            };
            return site_response(
                id,
                session,
                resource,
                session.bridges.get(&name).map(String::as_str),
                request,
                StatusCode::NOT_FOUND,
            );
        }
        let path: Vec<_> = request.uri().path().split('/').collect();
        if path.len() != 4 || !path[0].is_empty() {
            return denied();
        }
        let (Ok(session), Ok(page)) = (Uuid::parse_str(path[1]), Uuid::parse_str(path[2])) else {
            return denied();
        };
        if session.to_string() != path[1] || page.to_string() != path[2] {
            return denied();
        }
        // A GET form lands on the page document with a query; assets stay path-only.
        if request.uri().query().is_some() && path[3] != "index.html" {
            return denied();
        }
        let Some(package) = self.sessions.get(&session) else {
            return denied();
        };
        let Some(resource) = package.resources.get(&(page, path[3])) else {
            return denied();
        };
        let Some(len) = resource.body.decoded_len() else {
            return denied();
        };
        let head = request.method() == "HEAD";
        match requested_range(request.headers().get("range"), len) {
            RequestedRange::Unsatisfiable => {
                let mut result = source_response(
                    session,
                    package.scripts_enabled,
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    resource.content_type,
                    vec![],
                );
                result.headers_mut().insert(
                    "content-range",
                    format!("bytes */{len}")
                        .parse()
                        .expect("A bounded resource length is a valid header"),
                );
                result
            }
            RequestedRange::Satisfiable(start, end) => {
                let mut result = source_response(
                    session,
                    package.scripts_enabled,
                    StatusCode::PARTIAL_CONTENT,
                    resource.content_type,
                    if head {
                        vec![]
                    } else {
                        resource.body.read_range(start, end).unwrap_or_default()
                    },
                );
                result.headers_mut().insert(
                    "content-range",
                    format!("bytes {start}-{end}/{len}")
                        .parse()
                        .expect("Bounded resource offsets are valid headers"),
                );
                result.headers_mut().insert(
                    "content-length",
                    (end - start + 1)
                        .to_string()
                        .parse()
                        .expect("A bounded resource length is a valid header"),
                );
                result
            }
            RequestedRange::None => source_response(
                session,
                package.scripts_enabled,
                StatusCode::OK,
                resource.content_type,
                if head || len == 0 {
                    vec![]
                } else {
                    resource.body.read_range(0, len - 1).unwrap_or_default()
                },
            ),
        }
    }
}

impl PreviewSession {
    /// Resolve a site path against the shared package and this session's own bridge.
    fn file(&self, path: &str) -> Option<&Resource> {
        self.extras.get(path).or_else(|| {
            self.site
                .as_ref()
                .and_then(|package| package.files.get(path))
        })
    }
}

/// Serve one site resource, decoding only the requested byte range.
fn site_response(
    session_id: Uuid,
    session: &PreviewSession,
    resource: &Resource,
    bridge: Option<&str>,
    request: &Request<Vec<u8>>,
    status: StatusCode,
) -> Response<Vec<u8>> {
    let script = if session.scripts_enabled {
        "'unsafe-inline' desegnos-preview: http: https:"
    } else {
        "'none'"
    };
    // GET forms navigate within this session, whose pages can render the result. There
    // is no server for a POST, and scripts cannot send a form anywhere else.
    let forms = format!("desegnos-preview: http://{session_id}.desegnos-preview.localhost");
    let policy = format!(
        "default-src 'none'; script-src {script}; script-src-attr {script}; style-src 'unsafe-inline' desegnos-preview: http: https: data:; img-src desegnos-preview: http: https: data:; font-src desegnos-preview: http: https: data:; connect-src {EMBED_SOURCES}; frame-src {EMBED_SOURCES}; child-src {EMBED_SOURCES}; object-src 'none'; base-uri desegnos-preview: http: https:; form-action {forms}; media-src desegnos-preview: http: https: data:; worker-src 'none'"
    );
    let decorate = |mut result: Response<Vec<u8>>| {
        result.headers_mut().insert(
            "content-security-policy",
            policy.parse().expect("fixed website policy"),
        );
        result
    };
    let Some(len) = resource.body.decoded_len() else {
        return decorate(response(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"Preview resource is unavailable.".to_vec(),
        ));
    };
    let head = request.method() == "HEAD";
    match requested_range(request.headers().get("range"), len) {
        RequestedRange::Unsatisfiable => {
            let mut result = response(
                StatusCode::RANGE_NOT_SATISFIABLE,
                resource.content_type,
                vec![],
            );
            result.headers_mut().insert(
                "content-range",
                format!("bytes */{len}")
                    .parse()
                    .expect("A bounded resource length is a valid header"),
            );
            decorate(result)
        }
        RequestedRange::Satisfiable(start, end) => {
            let mut result = response(
                StatusCode::PARTIAL_CONTENT,
                resource.content_type,
                if head {
                    vec![]
                } else {
                    resource.body.read_range(start, end).unwrap_or_default()
                },
            );
            result.headers_mut().insert(
                "content-range",
                format!("bytes {start}-{end}/{len}")
                    .parse()
                    .expect("Bounded resource offsets are valid headers"),
            );
            result.headers_mut().insert(
                "content-length",
                (end - start + 1)
                    .to_string()
                    .parse()
                    .expect("A bounded resource length is a valid header"),
            );
            decorate(result)
        }
        RequestedRange::None => {
            let mut body = if head || len == 0 {
                vec![]
            } else {
                resource.body.read_range(0, len - 1).unwrap_or_default()
            };
            if let Some(bridge) = bridge.filter(|_| !head) {
                body = with_bridge(body, bridge);
            }
            let mut result = response(status, resource.content_type, body);
            if head {
                result.headers_mut().insert(
                    "content-length",
                    len.to_string()
                        .parse()
                        .expect("A bounded resource length is a valid header"),
                );
            }
            decorate(result)
        }
    }
}

/// A directory route keeps its trailing slash so relative links resolve inside it.
fn redirect(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let mut location = format!("{}/", request.uri().path());
    if let Some(query) = request.uri().query() {
        location.push('?');
        location.push_str(query);
    }
    let Ok(location) = HeaderValue::from_str(&location) else {
        return response(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"Preview resource is unavailable.".to_vec(),
        );
    };
    Response::builder()
        .status(StatusCode::PERMANENT_REDIRECT)
        .header("location", location)
        .header("cache-control", "no-store")
        .header("referrer-policy", "no-referrer")
        .body(vec![])
        .expect("Preview redirect headers are fixed and valid")
}

fn is_index_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["index.html", "index.htm"].iter().any(|index| {
        name == *index || name.ends_with(&format!("/{index}"))
    })
}

/// Session-bound bridge markup is added only to the served page, never to shared bytes.
fn with_bridge(bytes: Vec<u8>, bridge: &str) -> Vec<u8> {
    match String::from_utf8(bytes) {
        Ok(mut html) => {
            if let Some(position) = html.to_ascii_lowercase().rfind("</body>") {
                html.insert_str(position, bridge);
            } else {
                html.push_str(bridge);
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

fn requested_range(value: Option<&HeaderValue>, len: usize) -> RequestedRange {
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

fn response(status: StatusCode, content_type: &'static str, bytes: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .header("content-security-policy", preview_policy())
        .header("x-content-type-options", "nosniff")
        .header("cache-control", "no-store")
        .header("accept-ranges", "bytes")
        .header("access-control-allow-origin", "*")
        .header("referrer-policy", "no-referrer")
        .body(bytes)
        .expect("Preview response headers are fixed and valid")
}

fn source_response(
    session_id: Uuid,
    run_scripts: bool,
    status: StatusCode,
    content_type: &'static str,
    bytes: Vec<u8>,
) -> Response<Vec<u8>> {
    let origins = format!(
        "desegnos-preview://localhost/{session_id}/ http://desegnos-preview.localhost/{session_id}/ https://desegnos-preview.localhost/{session_id}/"
    );
    let policy = preview_policy()
        .replace(
            "style-src 'unsafe-inline'",
            &format!("style-src 'unsafe-inline' {origins}"),
        )
        // A GET form is a navigation to this session's page document.
        .replace("form-action 'none'", &format!("form-action {origins}"));
    let policy = if run_scripts {
        policy.replace("script-src 'none'", &format!("script-src {origins}"))
    } else {
        policy
    };
    let mut result = response(status, content_type, bytes);
    result.headers_mut().insert(
        "content-security-policy",
        policy
            .parse()
            .expect("Random UUID preview paths form a valid policy"),
    );
    result
}

fn decode_site_path(path: &str) -> Option<String> {
    let mut decoded = Vec::new();
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            ((bytes.next()? as char).to_digit(16)? * 16 + (bytes.next()? as char).to_digit(16)?)
                as u8
        } else {
            byte
        });
    }
    let value = String::from_utf8(decoded).ok()?;
    (!value.contains(['\\', '\0']) && !value.split('/').any(|p| p == ".." || p == "."))
        .then_some(value)
}

fn site_mime(path: &str) -> &'static str {
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
    use desegnos_core::{Appearance, SiteContent, Viewport};

    #[test]
    fn imported_preview_preserves_file_layout_queries_and_script_isolation() {
        let files = BTreeMap::from([
            ("index.html".into(), b"<!doctype html><html><head><link rel='stylesheet' href='/css/site.css?v=1'></head><body><h1>Home</h1><a href='about.html'>About</a><script type='module' src='main.js'></script></body></html>".to_vec()),
            ("about.html".into(), b"<!doctype html><h1>About</h1>".to_vec()),
            ("css/site.css".into(), b"h1{color:red}".to_vec()),
            ("main.js".into(), b"import './module.js';".to_vec()),
            ("module.js".into(), b"document.body.dataset.loaded='yes';".to_vec()),
        ]);
        let project = desegnos_core::import_website_files("Website", &files).unwrap();
        let mut registry = PreviewRegistry::default();
        let live = registry
            .register(&project, None, project.home_page_id, true)
            .unwrap();
        let document = registry.serve(&Request::get(&live.url).body(vec![]).unwrap());
        assert_eq!(document.status(), StatusCode::OK);
        let html = String::from_utf8(document.body().clone()).unwrap();
        assert!(html.contains("href=\"about.html\""));
        assert!(html.contains("src=\"main.js\""));
        assert!(
            document.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("script-src 'unsafe-inline'")
        );
        let origin = live.url.trim_end_matches("/index.html");
        for path in ["css/site.css?v=1", "about.html", "module.js"] {
            assert_eq!(
                registry
                    .serve(
                        &Request::get(format!("{origin}/{path}"))
                            .body(vec![])
                            .unwrap()
                    )
                    .status(),
                StatusCode::OK
            );
        }
        assert_eq!(
            registry
                .serve(
                    &Request::get(format!("{origin}/%2e%2e/private"))
                        .body(vec![])
                        .unwrap()
                )
                .status(),
            StatusCode::NOT_FOUND
        );
        let canvas = registry
            .register(&project, None, project.home_page_id, false)
            .unwrap();
        assert!(canvas.content.unwrap().html.contains("h1{color:red}"));
        let response = registry.serve(&Request::get(canvas.url).body(vec![]).unwrap());
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("script-src 'none'")
        );
        registry.release(live.session_id);
        assert_eq!(
            registry
                .serve(&Request::get(live.url).body(vec![]).unwrap())
                .status(),
            StatusCode::NOT_FOUND
        );
    }

    /// A site page that frames its own files keeps them on the same session: the framed
    /// document is served with the bridge and the scripting policy of a top-level page.
    #[test]
    fn framed_site_files_are_served_by_their_session_with_the_bridge() {
        let files = BTreeMap::from([
            (
                "index.html".into(),
                b"<!doctype html><html><head><title>Home</title></head><body><iframe src=\"components/llama-wrap-preview.html\"></iframe><iframe src=\"play/solopong/index.html\"></iframe><iframe src=\"https://player.example.com/embed\"></iframe></body></html>".to_vec(),
            ),
            (
                "play/solopong/index.html".into(),
                b"<!doctype html><html><head><title>Solo Pong</title></head><body><canvas></canvas></body></html>".to_vec(),
            ),
            (
                "components/llama-wrap-preview.html".into(),
                b"<!doctype html><div class=\"llama\">Wrap</div>".to_vec(),
            ),
        ]);
        let project = desegnos_core::import_website_files("Embeds", &files).unwrap();
        // The wrap file is a fragment, so it stays a site file rather than becoming a page.
        assert_eq!(
            project.website.as_ref().unwrap().page_files.len(),
            files.len() - 1
        );
        let mut registry = PreviewRegistry::default();
        let live = registry
            .register(&project, None, project.home_page_id, true)
            .unwrap();
        let origin = live.url.trim_end_matches("/index.html").to_owned();
        let get = |path: &str| {
            registry.serve(
                &Request::get(format!("{origin}/{path}"))
                    .body(vec![])
                    .unwrap(),
            )
        };
        let home = get("index.html");
        assert_eq!(home.status(), StatusCode::OK);
        let csp = home.headers()["content-security-policy"].to_str().unwrap();
        for directive in ["frame-src", "child-src"] {
            assert!(
                csp.contains(&format!("{directive} {EMBED_SOURCES}")),
                "{directive}: {csp}"
            );
        }
        assert!(csp.contains(&format!("connect-src {EMBED_SOURCES}")));
        for path in [
            "play/solopong/index.html",
            "components/llama-wrap-preview.html",
        ] {
            let framed = get(path);
            assert_eq!(framed.status(), StatusCode::OK, "{path}");
            assert_eq!(framed.headers()["content-type"], "text/html; charset=utf-8");
            let html = String::from_utf8(framed.body().clone()).unwrap();
            assert!(
                html.contains(&format!("src=\"/desegnos-{}.js\"", live.session_id)),
                "{path} is served without the bridge"
            );
            let framed_csp = framed.headers()["content-security-policy"]
                .to_str()
                .unwrap();
            assert!(framed_csp.contains("script-src 'unsafe-inline'"));
            assert!(framed_csp.contains(&format!("frame-src {EMBED_SOURCES}")));
        }
        for path in [
            "%2e%2e/Cargo.toml",
            "..%2fCargo.toml",
            "components/%2e%2e/%2e%2e/Cargo.toml",
        ] {
            assert_eq!(get(path).status(), StatusCode::NOT_FOUND, "{path}");
        }
        // A paused session serves the same files with scripting removed.
        let paused = registry
            .register(&project, None, project.home_page_id, false)
            .unwrap();
        let origin = paused.url.trim_end_matches("/index.html");
        let framed = registry.serve(
            &Request::get(format!("{origin}/components/llama-wrap-preview.html"))
                .body(vec![])
                .unwrap(),
        );
        assert_eq!(framed.status(), StatusCode::OK);
        assert!(
            !String::from_utf8_lossy(framed.body())
                .contains(&format!("desegnos-{}.js", paused.session_id))
        );
        let csp = framed.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(csp.contains("script-src 'none'"));
        assert!(csp.contains(&format!("frame-src {EMBED_SOURCES}")));
    }

    /// Every page a session serves answers its own address with the bridge and the
    /// scripting policy, including directory routes, their slash form, and the site's
    /// own 404 page for paths that do not exist.
    #[test]
    fn site_routes_redirect_to_directories_and_serve_bridge_and_404_pages_directly() {
        let files = BTreeMap::from([
            (
                "index.html".into(),
                b"<!doctype html><html><body><a href=\"/projects/papan\">Papan</a></body></html>".to_vec(),
            ),
            (
                "projects/papan/index.html".into(),
                b"<!doctype html><html><body><h1>Papan</h1></body></html>".to_vec(),
            ),
            (
                "404.html".into(),
                b"<!doctype html><html><body><h1>Page not found</h1></body></html>".to_vec(),
            ),
            ("css/site.css".into(), b"h1{color:red}".to_vec()),
        ]);
        let project = desegnos_core::import_website_files("Routes", &files).unwrap();
        let mut registry = PreviewRegistry::default();
        let live = registry
            .register(&project, None, project.home_page_id, true)
            .unwrap();
        let origin = live.url.trim_end_matches("/index.html").to_owned();
        let bridge = format!("desegnos-{}.js", live.session_id);
        let get = |path: &str| {
            registry.serve(
                &Request::get(format!("{origin}/{path}"))
                    .body(vec![])
                    .unwrap(),
            )
        };
        // A directory route keeps its trailing slash so relative links stay inside it.
        let moved = get("projects/papan");
        assert_eq!(moved.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(moved.headers()["location"], "/projects/papan/");
        let moved = get("projects/papan?ref=home");
        assert_eq!(moved.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(moved.headers()["location"], "/projects/papan/?ref=home");
        for path in [
            "projects/papan/",
            "projects/papan/index.html",
            "projects/papan/?ref=home",
        ] {
            let page = get(path);
            assert_eq!(page.status(), StatusCode::OK, "{path}");
            assert_eq!(page.headers()["content-type"], "text/html; charset=utf-8");
            let html = String::from_utf8(page.body().clone()).unwrap();
            assert!(html.contains(">Papan</h1>"), "{path}: {html}");
            assert!(html.contains(&format!("src=\"/{bridge}\"")), "{path}");
            let csp = page.headers()["content-security-policy"]
                .to_str()
                .unwrap();
            assert!(csp.contains("script-src 'unsafe-inline'"), "{path}");
            assert!(
                csp.contains(&format!(
                    "form-action desegnos-preview: http://{}.desegnos-preview.localhost",
                    live.session_id
                )),
                "{path}: {csp}"
            );
        }
        // The site's own 404 page answers a missing path, with the same bridge and policy.
        let missing = get("projects/unknown/");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(missing.headers()["content-type"], "text/html; charset=utf-8");
        let html = String::from_utf8(missing.body().clone()).unwrap();
        assert!(html.contains("Page not found"));
        assert!(html.contains(&format!("src=\"/{bridge}\"")));
        assert!(
            missing.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("script-src 'unsafe-inline'")
        );
        // The 404 page itself is still an ordinary file of the session.
        let direct = get("404.html");
        assert_eq!(direct.status(), StatusCode::OK);
        assert!(String::from_utf8_lossy(direct.body()).contains("Page not found"));
        assert!(String::from_utf8_lossy(direct.body()).contains(&format!("src=\"/{bridge}\"")));
        // A rejected traversal never reaches the site's 404 page.
        let traversal = get("%2e%2e/Cargo.toml");
        assert_eq!(traversal.status(), StatusCode::NOT_FOUND);
        assert_eq!(traversal.body(), b"Preview resource is unavailable.");
    }

    fn project() -> Project {
        let mut project = Project::new(
            "Preview checks".into(),
            Viewport {
                width: 1440,
                height: 900,
            },
            Appearance::Light,
        )
        .unwrap();
        project.pages[0].content = Some(SiteContent {
            html: "<button id=\"hello\">Hello</button>".into(),
            css: "button{color:navy}".into(),
            javascript: "document.querySelector('button').textContent='Working';".into(),
        });
        project
    }

    #[test]
    fn region_scripts_are_separate_resources_after_page_js_and_absent_when_paused() {
        let mut project = project();
        let content = project.pages[0].content.as_mut().unwrap();
        content.html = format!(
            "<section><button>Sort</button><script type=\"application/x-desegnos-region\" data-desegnos-script=\"{}\">root.querySelector('button').textContent='Sorted';</script></section>",
            Uuid::new_v4()
        );
        let original_js = content.javascript.clone();
        let mut registry = PreviewRegistry::default();
        let live = registry
            .register(&project, None, project.home_page_id, true)
            .unwrap();
        let document = registry.serve(&Request::get(&live.url).body(vec![]).unwrap());
        let html = std::str::from_utf8(document.body()).unwrap();
        assert!(html.find("./site.js").unwrap() < html.find("./regions.js").unwrap());
        let script = registry.serve(
            &Request::get(live.url.replace("index.html", "regions.js"))
                .body(vec![])
                .unwrap(),
        );
        let runtime = std::str::from_utf8(script.body()).unwrap();
        assert_eq!(script.status(), StatusCode::OK);
        assert!(
            runtime.contains("root.querySelector('button').textContent='Sorted';")
                && runtime.contains("catch (error)")
        );
        let script = registry.serve(
            &Request::get(live.url.replace("index.html", "site.js"))
                .body(vec![])
                .unwrap(),
        );
        assert_eq!(script.body(), original_js.as_bytes());
        let paused = registry
            .register(&project, None, project.home_page_id, false)
            .unwrap();
        let document = registry.serve(&Request::get(&paused.url).body(vec![]).unwrap());
        let html = std::str::from_utf8(document.body()).unwrap();
        assert!(!html.contains("./regions.js"));
        assert!(!html.contains("./site.js"));
        let script = registry.serve(
            &Request::get(paused.url.replace("index.html", "regions.js"))
                .body(vec![])
                .unwrap(),
        );
        assert_eq!(script.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn serves_only_frozen_registered_sources_with_deny_by_default_policy() {
        let mut registry = PreviewRegistry::default();
        let mut project = project();
        let page_id = project.home_page_id;
        let descriptor = registry.register(&project, None, page_id, false).unwrap();
        project.pages[0].content.as_mut().unwrap().html = "Changed later".into();
        let document = registry.serve(&Request::get(&descriptor.url).body(vec![]).unwrap());
        assert_eq!(document.status(), StatusCode::OK);
        assert!(String::from_utf8_lossy(document.body()).contains("Hello"));
        assert!(!String::from_utf8_lossy(document.body()).contains("Changed later"));
        assert!(
            !String::from_utf8_lossy(document.body())
                .contains("src=\"./site.js\" crossorigin=\"anonymous\"")
        );
        let csp = document.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(!csp.contains("form-action 'none'"));
        assert!(csp.contains(&format!(
            "form-action desegnos-preview://localhost/{}/",
            descriptor.session_id
        )));
        assert!(!csp.contains("unsafe-eval"));
        // A live or paused page may still frame its own files and an https embed; the
        // policy fails closed for everything else.
        assert!(csp.contains(&format!("frame-src {EMBED_SOURCES}")));
        assert!(csp.contains(&format!("child-src {EMBED_SOURCES}")));
        assert!(csp.contains(&format!("connect-src {EMBED_SOURCES}")));
        assert!(!csp.contains("frame-src 'none'") && !csp.contains("connect-src 'none'"));
        // A GET form may land back on this page document with its query.
        assert_eq!(
            registry
                .serve(
                    &Request::get(format!("{}?name=Ada", descriptor.url))
                        .body(vec![])
                        .unwrap()
                )
                .status(),
            StatusCode::OK
        );
        for suffix in ["site.js", "../../Cargo.toml", "%2e%2e", "site.css?token=x"] {
            let url = descriptor.url.replace("index.html", suffix);
            assert_eq!(
                registry
                    .serve(&Request::get(url).body(vec![]).unwrap())
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
    }

    #[test]
    fn script_execution_requires_a_separate_explicit_package_and_resources_are_bounded() {
        let project = project();
        let mut registry = PreviewRegistry::default();
        let descriptor = registry
            .register(&project, None, project.home_page_id, true)
            .unwrap();
        assert!(registry.scripts_enabled());
        let url = descriptor.url.replace("index.html", "site.js");
        let request = Request::get(&url)
            .header("range", "bytes=0-7")
            .body(vec![])
            .unwrap();
        let partial = registry.serve(&request);
        assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(partial.body(), b"document");
        let paused = registry
            .register(&project, None, project.home_page_id, false)
            .unwrap();
        let policy = registry
            .serve(&Request::get(&paused.url).body(vec![]).unwrap())
            .headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(policy.contains(&paused.session_id.to_string()));
        assert!(!policy.contains(&descriptor.session_id.to_string()));
        assert_eq!(
            registry
                .serve(
                    &Request::get(&url)
                        .header("range", "bytes=0-999999999999")
                        .body(vec![])
                        .unwrap()
                )
                .status(),
            StatusCode::RANGE_NOT_SATISFIABLE
        );
        for _ in 0..MAX_PREVIEW_SESSIONS {
            registry
                .register(&project, None, project.home_page_id, false)
                .unwrap();
        }
        assert_eq!(
            registry
                .serve(&Request::get(&url).body(vec![]).unwrap())
                .status(),
            StatusCode::NOT_FOUND
        );
        registry.clear();
        assert!(!registry.scripts_enabled());
    }

    #[test]
    fn large_media_previews_by_reference_and_supports_range_requests() {
        let clip = 65 * 1024 * 1024;
        let media: Vec<u8> = (0..clip).map(|index| (index % 251) as u8).collect();
        let files = BTreeMap::from([
            (
                "index.html".into(),
                b"<!doctype html><html><body><video src=\"portfolio/clip.mp4\" controls></video></body></html>".to_vec(),
            ),
            ("portfolio/clip.mp4".into(), media),
        ]);
        let project = desegnos_core::import_website_files("Portfolio", &files).unwrap();
        let mut registry = PreviewRegistry::default();
        let live = registry
            .register(&project, None, project.home_page_id, true)
            .unwrap();
        let document = registry.serve(&Request::get(&live.url).body(vec![]).unwrap());
        assert_eq!(document.status(), StatusCode::OK);
        assert!(String::from_utf8_lossy(document.body()).contains("portfolio/clip.mp4"));
        let origin = live.url.trim_end_matches("/index.html");
        let url = format!("{origin}/portfolio/clip.mp4");
        let head = registry.serve(
            &Request::builder()
                .method("HEAD")
                .uri(&url)
                .body(vec![])
                .unwrap(),
        );
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(head.headers()["content-type"], "video/mp4");
        assert_eq!(head.headers()["accept-ranges"], "bytes");
        assert_eq!(head.headers()["content-length"], clip.to_string().as_str());
        let expected = |start: usize, end: usize| -> Vec<u8> {
            (start..=end).map(|index| (index % 251) as u8).collect()
        };
        for (range, start, end) in [
            ("bytes=1000-1999", 1000, 1999),
            ("bytes=999-2001", 999, 2001),
            ("bytes=0-0", 0, 0),
        ] {
            let partial = registry.serve(
                &Request::get(&url)
                    .header("range", range)
                    .body(vec![])
                    .unwrap(),
            );
            assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(
                partial.headers()["content-range"],
                format!("bytes {start}-{end}/{clip}").as_str()
            );
            assert_eq!(
                partial.headers()["content-length"],
                (end - start + 1).to_string().as_str()
            );
            assert_eq!(partial.body(), &expected(start, end));
        }
        let suffix = registry.serve(
            &Request::get(&url)
                .header("range", "bytes=-8")
                .body(vec![])
                .unwrap(),
        );
        assert_eq!(suffix.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(suffix.body(), &expected(clip - 8, clip - 1));
        let open = registry.serve(
            &Request::get(&url)
                .header("range", "bytes=100-")
                .body(vec![])
                .unwrap(),
        );
        assert_eq!(open.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(open.body().len(), clip - 100);
        let beyond = registry.serve(
            &Request::get(&url)
                .header("range", "bytes=0-999999999999")
                .body(vec![])
                .unwrap(),
        );
        assert_eq!(beyond.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            beyond.headers()["content-range"],
            format!("bytes */{clip}").as_str()
        );
    }

    #[test]
    fn text_over_the_limit_names_the_offending_file() {
        let oversized = vec![b'a'; MAX_PREVIEW_TEXT_BYTES + 1];
        let files = BTreeMap::from([
            (
                "index.html".into(),
                b"<!doctype html><html><body>Portfolio</body></html>".to_vec(),
            ),
            ("portfolio/styles.css".into(), oversized),
        ]);
        let project = desegnos_core::import_website_files("Portfolio", &files).unwrap();
        let mut registry = PreviewRegistry::default();
        let Err(error) = registry.register(&project, None, project.home_page_id, false) else {
            panic!("an oversized text payload must not register a preview");
        };
        assert_eq!(
            error,
            "portfolio/styles.css is too large to preview (17 MB)."
        );
    }

    #[test]
    fn sessions_share_site_bytes_instead_of_duplicating_them() {
        let files = BTreeMap::from([
            (
                "index.html".into(),
                b"<!doctype html><html><body><video src=\"media/clip.mp4\"></video></body></html>"
                    .to_vec(),
            ),
            ("media/clip.mp4".into(), vec![7_u8; 2 * 1024 * 1024]),
        ]);
        let project = desegnos_core::import_website_files("Portfolio", &files).unwrap();
        let mut registry = PreviewRegistry::default();
        let first = registry
            .register(&project, None, project.home_page_id, false)
            .unwrap();
        let second = registry
            .register(&project, None, project.home_page_id, false)
            .unwrap();
        let live = registry
            .register(&project, None, project.home_page_id, true)
            .unwrap();
        let package = |id: Uuid| registry.sessions[&id].site.clone().unwrap();
        let (a, b, c) = (
            package(first.session_id),
            package(second.session_id),
            package(live.session_id),
        );
        assert!(Arc::ptr_eq(&a, &b));
        assert!(Arc::strong_count(&a) >= 2);
        assert!(!Arc::ptr_eq(&a, &c));
        let media = |package: &SitePackage| match &package.files["media/clip.mp4"].body {
            PreviewFile::Encoded(bytes) => bytes.clone(),
            _ => panic!("large media must stay referenced, not copied into a session"),
        };
        assert!(Arc::ptr_eq(&media(&a), &media(&c)));
    }
}
