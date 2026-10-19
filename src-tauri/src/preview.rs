use desegni_core::{GenerationCandidate, Project};
use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
use tauri::http::{Request, Response, StatusCode};
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
            manager.register_uri_scheme_as_cors_enabled("desegni-preview");
        }
        #[cfg(feature = "native-tests")]
        if let Some(settings) = webview.inner().settings() {
            settings.set_enable_write_console_messages_to_stdout(true);
        }
    })
}

const MAX_PREVIEW_BYTES: usize = 64 * 1024 * 1024;
const MAX_PREVIEW_SESSIONS: usize = 8;
const PREVIEW_CSP: &str = "default-src 'none'; script-src 'none'; script-src-attr 'none'; style-src 'unsafe-inline'; img-src data:; font-src 'none'; connect-src 'none'; frame-src 'none'; child-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; media-src 'none'; worker-src 'none'";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDescriptor {
    pub url: String,
    pub session_id: Uuid,
    pub source_sha256: Option<String>,
}

struct Resource {
    content_type: &'static str,
    bytes: Arc<[u8]>,
}

struct PreviewSession {
    resources: BTreeMap<(Uuid, &'static str), Resource>,
    byte_len: usize,
    scripts_enabled: bool,
}

#[derive(Default)]
pub struct PreviewRegistry {
    sessions: BTreeMap<Uuid, PreviewSession>,
    order: VecDeque<Uuid>,
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
        "http://desegni-preview.localhost"
    } else {
        "desegni-preview://localhost"
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
            if let Some(content) = content {
                content.validate().map_err(|error| error.to_string())?;
            }
            let (html, css, javascript) = content
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
            let document = format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"./site.css\" crossorigin=\"anonymous\"></head><body>{html}{bridge_script}{user_script}</body></html>",
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
                    "site.js",
                    "text/javascript; charset=utf-8",
                    javascript.as_bytes().to_vec(),
                ));
            }
            for (name, content_type, bytes) in files {
                byte_len = byte_len
                    .checked_add(bytes.len())
                    .ok_or("The preview package is too large.")?;
                if byte_len > MAX_PREVIEW_BYTES {
                    return Err("The complete preview exceeds the 64 MiB local limit. Reduce the site's source before previewing.".into());
                }
                resources.insert(
                    (page.id, name),
                    Resource {
                        content_type,
                        bytes: bytes.into(),
                    },
                );
            }
        }
        while self.sessions.len() >= MAX_PREVIEW_SESSIONS
            || self
                .sessions
                .values()
                .map(|session| session.byte_len)
                .sum::<usize>()
                + byte_len
                > MAX_PREVIEW_BYTES
        {
            let Some(expired) = self.order.pop_front() else {
                break;
            };
            self.sessions.remove(&expired);
        }
        self.sessions.insert(
            session_id,
            PreviewSession {
                resources,
                byte_len,
                scripts_enabled: run_scripts,
            },
        );
        self.order.push_back(session_id);
        Ok(PreviewDescriptor {
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
                .map(desegni_core::SiteContent::sha256)
                .transpose()
                .map_err(|error| error.to_string())?,
        })
    }

    pub fn clear(&mut self) {
        self.sessions.clear();
        self.order.clear();
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
        if !matches!(request.method().as_str(), "GET" | "HEAD")
            || request.uri().query().is_some()
            || !request.body().is_empty()
        {
            return denied();
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
        let Some(package) = self.sessions.get(&session) else {
            return denied();
        };
        let Some(resource) = package.resources.get(&(page, path[3])) else {
            return denied();
        };
        if let Some(range) = request.headers().get("range") {
            let range = range
                .to_str()
                .ok()
                .and_then(|range| range.strip_prefix("bytes="))
                .and_then(|range| range.split_once('-'))
                .filter(|(start, end)| !start.is_empty() && !end.contains(','));
            let bounds = range.and_then(|(start, end)| {
                Some((
                    start.parse::<usize>().ok()?,
                    if end.is_empty() {
                        resource.bytes.len().checked_sub(1)?
                    } else {
                        end.parse::<usize>().ok()?
                    },
                ))
            });
            let Some((start, end)) =
                bounds.filter(|(start, end)| start <= end && *end < resource.bytes.len())
            else {
                let mut result = source_response(
                    session,
                    package.scripts_enabled,
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    resource.content_type,
                    vec![],
                );
                result.headers_mut().insert(
                    "content-range",
                    format!("bytes */{}", resource.bytes.len())
                        .parse()
                        .expect("A bounded resource length is a valid header"),
                );
                return result;
            };
            let mut result = source_response(
                session,
                package.scripts_enabled,
                StatusCode::PARTIAL_CONTENT,
                resource.content_type,
                if request.method() == "HEAD" {
                    vec![]
                } else {
                    resource.bytes[start..=end].to_vec()
                },
            );
            result.headers_mut().insert(
                "content-range",
                format!("bytes {start}-{end}/{}", resource.bytes.len())
                    .parse()
                    .expect("Bounded resource offsets are valid headers"),
            );
            return result;
        }
        source_response(
            session,
            package.scripts_enabled,
            StatusCode::OK,
            resource.content_type,
            if request.method() == "HEAD" {
                vec![]
            } else {
                resource.bytes.to_vec()
            },
        )
    }
}

fn response(status: StatusCode, content_type: &'static str, bytes: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .header("content-security-policy", PREVIEW_CSP)
        .header("x-content-type-options", "nosniff")
        .header("cache-control", "no-store")
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
        "desegni-preview://localhost/{session_id}/ http://desegni-preview.localhost/{session_id}/ https://desegni-preview.localhost/{session_id}/"
    );
    let policy = PREVIEW_CSP.replace(
        "style-src 'unsafe-inline'",
        &format!("style-src 'unsafe-inline' {origins}"),
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use desegni_core::{Appearance, SiteContent, Viewport};

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
        assert!(csp.contains("connect-src 'none'"));
        assert!(csp.contains("form-action 'none'"));
        assert!(!csp.contains("unsafe-eval"));
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
}
