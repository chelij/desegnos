//! Bounded public website snapshots and imported picture downloads.
use crate::public_fetch::{FetchRequest, PublicFetcher};
use reqwest::Url;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub use reqwest::Url as WebUrl;

pub struct WebsiteFetcher {
    fetcher: PublicFetcher,
}
impl Default for WebsiteFetcher {
    fn default() -> Self {
        Self::new()
    }
}
impl WebsiteFetcher {
    pub fn new() -> Self {
        Self {
            fetcher: PublicFetcher::new(),
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_fixture(origin: &Url) -> Result<Self, String> {
        if origin.scheme() != "http"
            || !origin
                .host_str()
                .is_some_and(|host| matches!(host, "127.0.0.1" | "[::1]"))
        {
            return Err("Test website sources require a literal loopback HTTP origin.".into());
        }
        Ok(Self {
            fetcher: PublicFetcher::for_fixture(origin),
        })
    }
}

use desegnos_core::limits::{MAX_IMPORT_PAGES, MAX_WEBSITE_FILE_BYTES};
use desegnos_storage::website_snapshot::{decode_html, decode_url_path, page_file, page_title};
pub use desegnos_storage::{WebsitePage, WebsiteSnapshot};

pub fn website_address(input: &str) -> Result<Url, String> {
    let trimmed = input.trim();
    let candidate = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    let url = Url::parse(&candidate)
        .map_err(|_| "Enter a website address, such as example.com.".to_owned())?;
    if url.scheme() != "https"
        || url.host_str().is_none_or(|host| !host.contains('.'))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Enter a public https:// website address, such as example.com.".into());
    }
    Ok(url)
}

fn website_name(url: &Url) -> String {
    url.host_str()
        .unwrap_or("website")
        .trim_start_matches("www.")
        .to_owned()
}

fn enqueue_pages(
    base: &Url,
    links: &[String],
    seen: &mut BTreeSet<String>,
    queue: &mut VecDeque<Url>,
) {
    let mut candidates = Vec::new();
    for href in links {
        if seen.len() >= MAX_DISCOVERED_PAGES {
            break;
        }
        let Ok(mut url) = base.join(href) else {
            continue;
        };
        url.set_fragment(None);
        if matches!(url.path(), "/index.html" | "/index.htm") {
            url.set_path("/");
        }
        if url.host_str() != base.host_str()
            || url.origin() != base.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
        {
            continue;
        }
        let path = url.path().to_ascii_lowercase();
        let last = path.rsplit('/').next().unwrap_or_default();
        if (last.contains('.') && !last.ends_with(".html") && !last.ends_with(".htm"))
            || path.split('/').any(|part| {
                matches!(
                    part,
                    "logout" | "signout" | "delete" | "checkout" | "admin" | "login" | "account"
                )
            })
        {
            continue;
        }
        if seen.insert(url.to_string()) {
            candidates.push(url);
        }
    }
    candidates.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    queue.extend(candidates);
}

const MAX_SNAPSHOT_PAGE_BYTES: usize = 1024 * 1024;
const MAX_DISCOVERED_PAGES: usize = 256;
const MAX_SNAPSHOT_ASSETS: usize = 240;
const MAX_SNAPSHOT_TOTAL_BYTES: usize = 96 * 1024 * 1024;

const ASSET_MIMES: &[&str] = &[
    "text/css",
    "text/javascript",
    "application/javascript",
    "application/x-javascript",
    "application/json",
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/svg+xml",
    "image/avif",
    "image/x-icon",
    "image/vnd.microsoft.icon",
    "font/woff",
    "font/woff2",
    "font/ttf",
    "font/otf",
    "application/font-woff",
    "application/vnd.ms-fontobject",
    "video/mp4",
    "video/webm",
    "audio/mpeg",
    "audio/ogg",
    "audio/wav",
];

fn snapshot_route(url: &Url) -> String {
    let path = url.path();
    if path == "/index.html" || path == "/index.htm" {
        "/".into()
    } else {
        path.to_owned()
    }
}

fn decoded_path(path: &str) -> Option<String> {
    let value = decode_url_path(path)?;
    if value.is_empty()
        || value.contains(['\\', '\0'])
        || value.chars().any(char::is_control)
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.starts_with('.'))
    {
        return None;
    }
    Some(value)
}

fn asset_file(url: &Url) -> Option<String> {
    let path = url.path().trim_start_matches('/');
    if path.is_empty() || path.ends_with('/') || path.contains('.') && path.ends_with('.') {
        return None;
    }
    decoded_path(path)
}

fn localize_same_site(text: &str, origin: &Url) -> String {
    let prefix = origin.as_str().trim_end_matches('/');
    let text = text.replace(prefix, "");
    match origin.host_str() {
        Some(host) => text.replace(&format!("//{host}"), ""),
        None => text,
    }
}

impl WebsiteFetcher {
    pub async fn fetch_website_snapshot(&self, input: &str) -> Result<WebsiteSnapshot, String> {
        let fetcher = &self.fetcher;
        let mut first = match fetcher.fixture() {
            Some(origin) => origin
                .join(input)
                .map_err(|_| "Enter a website address.".to_owned())?,
            None => website_address(input)?,
        };
        if first.path() == "/index.html" || first.path() == "/index.htm" {
            first.set_path("/");
        }
        let source_host = first
            .host_str()
            .ok_or("The source website has no host.")?
            .to_owned();
        let host = website_name(&first);
        let mut snapshot = WebsiteSnapshot {
            name: host,
            ..WebsiteSnapshot::default()
        };
        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut pages_seen = BTreeSet::new();
        let mut page_queue = VecDeque::new();
        page_queue.push_back(first.clone());
        pages_seen.insert(first.to_string());
        let mut requests = 0;
        let mut total = 0usize;
        let mut assets: VecDeque<Url> = VecDeque::new();
        let mut asset_seen = BTreeSet::new();
        while requests < MAX_IMPORT_PAGES {
            let Some(url) = page_queue.pop_front() else {
                break;
            };
            requests += 1;
            let (bytes, _mime, final_url) = fetcher
                .document(FetchRequest {
                    url: url.as_str(),
                    mimes: &["text/html", "application/xhtml+xml"],
                    limit: MAX_SNAPSHOT_PAGE_BYTES,
                    host_lock: Some(&source_host),
                })
                .await
                .map_err(|reason| format!("That website couldn’t be read: it {reason}."))?;
            let origin = Url::parse(&final_url.origin().ascii_serialization())
                .map_err(|_| "The source website has an invalid origin.")?;
            total += bytes.len();
            if total > MAX_SNAPSHOT_TOTAL_BYTES {
                return Err("The website is larger than the import limit.".into());
            }
            let text = localize_same_site(&decode_html(&bytes), &origin);
            let route = snapshot_route(&url);
            let file = page_file(&route);
            files.insert(file.clone(), text.clone().into_bytes());
            let title = page_title(&file, &text);
            snapshot.pages.push(WebsitePage { route, title, file });
            let links = desegnos_core::website_links(&text);
            enqueue_pages(&url, &links.pages, &mut pages_seen, &mut page_queue);
            let found = desegnos_core::website_assets(&text);
            for href in found
                .stylesheets
                .iter()
                .chain(found.images.iter())
                .chain(found.scripts.iter())
                .chain(found.media.iter())
                .chain(found.frames.iter())
                .chain(found.inline_styles.iter())
            {
                let Ok(asset) = url.join(href) else { continue };
                if asset.host_str() != first.host_str()
                    || asset.origin() != first.origin()
                    || asset.scheme() != first.scheme()
                {
                    continue;
                }
                if asset_seen.insert(asset.to_string()) {
                    assets.push_back(asset);
                }
            }
            if page_queue.is_empty() || requests >= MAX_IMPORT_PAGES {
                break;
            }
        }
        if !page_queue.is_empty() {
            snapshot.skipped.push(format!(
                "Only the first {MAX_IMPORT_PAGES} same-site pages are imported"
            ));
        }
        let mut downloaded = 0;
        while let Some(url) = assets.pop_front() {
            if downloaded >= MAX_SNAPSHOT_ASSETS {
                snapshot.skipped.push(format!(
                    "Only the first {MAX_SNAPSHOT_ASSETS} page assets are imported"
                ));
                break;
            }
            let Some(file) = asset_file(&url) else {
                continue;
            };
            if files.contains_key(&file) {
                continue;
            }
            let extension = file.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            match fetcher
                .document(FetchRequest {
                    url: url.as_str(),
                    mimes: ASSET_MIMES,
                    limit: MAX_WEBSITE_FILE_BYTES,
                    host_lock: None,
                })
                .await
            {
                Ok((bytes, _mime, _)) => {
                    downloaded += 1;
                    total += bytes.len();
                    if total > MAX_SNAPSHOT_TOTAL_BYTES {
                        snapshot
                            .skipped
                            .push("Website assets reached the import size limit".into());
                        break;
                    }
                    let stored = if extension == "css" {
                        localize_same_site(&String::from_utf8_lossy(&bytes), &first).into_bytes()
                    } else {
                        bytes
                    };
                    if extension == "css" {
                        for href in desegnos_core::css_urls(&String::from_utf8_lossy(&stored)) {
                            let Ok(asset) = url.join(&href) else { continue };
                            if asset.host_str() == first.host_str()
                                && asset.origin() == first.origin()
                                && asset_seen.insert(asset.to_string())
                            {
                                assets.push_back(asset);
                            }
                        }
                    }
                    files.insert(file, stored);
                }
                Err(reason) => snapshot
                    .skipped
                    .push(format!("{url}: asset skipped, it {reason}")),
            }
        }
        snapshot.files = files.into_iter().collect();
        snapshot.pages.sort_by(|first, second| {
            (first.route != "/", &first.route).cmp(&(second.route != "/", &second.route))
        });
        snapshot
            .pages
            .dedup_by(|first, second| first.route == second.route);
        Ok(snapshot)
    }
}

impl WebsiteFetcher {
    pub async fn fetch_website_image(&self, source: &str) -> Result<(Vec<u8>, String), String> {
        let fetcher = &self.fetcher;
        fetcher
            .document(FetchRequest {
                url: source,
                mimes: &[
                    "image/png",
                    "image/jpeg",
                    "image/webp",
                    "image/gif",
                    "image/svg+xml",
                ],
                limit: MAX_WEBSITE_FILE_BYTES,
                host_lock: None,
            })
            .await
            .map(|(bytes, mime, _)| (bytes, mime))
            .map_err(|reason| format!("That picture couldn’t be read: it {reason}."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[test]
    fn addresses_default_to_https_and_reject_other_schemes() {
        assert_eq!(
            website_address("example.com").unwrap().as_str(),
            "https://example.com/"
        );
        assert_eq!(
            website_address(" https://shop.example.com/about ")
                .unwrap()
                .as_str(),
            "https://shop.example.com/about"
        );
        assert!(website_address("http://example.com").is_err());
        assert!(website_address("ftp://example.com").is_err());
        assert!(website_address("localhost").is_err());
        assert!(website_address("https://user:pass@example.com").is_err());
    }
    #[tokio::test]
    async fn fetches_shared_snapshot_stylesheet_and_logo_and_reports_skips() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let png = {
            let mut bytes = Vec::new();
            image::DynamicImage::new_rgb8(4, 4)
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .unwrap();
            bytes
        };
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let png = png.clone();
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 2048];
                    let count = socket.read(&mut buffer).await.unwrap();
                    let path = String::from_utf8_lossy(&buffer[..count])
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_owned();
                    let (kind, body): (&str, Vec<u8>) = match path.as_str() {
                        "/" => ("text/html", br#"<link rel="stylesheet" href="/site.css"><img src="/missing.png" alt=""><img src="/brand/acme-logo.png" alt="Acme">"#.to_vec()),
                        "/site.css" => ("text/css", b":root{--brand:#c0392b}body{font-family:Georgia,serif}".to_vec()),
                        "/brand/acme-logo.png" => ("image/png", png),
                        _ => ("text/plain", b"missing".to_vec()),
                    };
                    let status = if kind == "text/plain" {
                        "404 Not Found"
                    } else {
                        "200 OK"
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(head.as_bytes()).await.unwrap();
                    socket.write_all(&body).await.unwrap();
                });
            }
        });
        let site: desegnos_storage::WebsiteSnapshot = WebsiteFetcher::for_fixture(&origin)
            .unwrap()
            .fetch_website_snapshot("/")
            .await
            .unwrap();
        let names: Vec<_> = site.files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["brand/acme-logo.png", "index.html", "site.css"]);
        assert!(
            site.skipped
                .iter()
                .any(|reason| reason.contains("missing.png"))
        );
        assert_eq!(
            site.pages,
            vec![desegnos_storage::WebsitePage {
                route: "/".into(),
                title: "Home".into(),
                file: "index.html".into(),
            }]
        );
        let error = WebsiteFetcher::new()
            .fetch_website_snapshot("https://127.0.0.1/")
            .await
            .unwrap_err();
        assert!(
            error.contains("couldn’t be read") || error.contains("public"),
            "{error}"
        );
    }
}
