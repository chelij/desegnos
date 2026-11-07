//! Brand import from a website address: the home page, its stylesheets and logo images.
//! Every request uses the same public-address, size and timeout rules as generation image
//! downloads, carries no credentials, and never runs page code.
use crate::public_fetch::{FetchRequest, PublicFetcher};
use reqwest::Url;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Web address type used by the website fetcher (re-exported so callers need no HTTP dependency).
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
            fetcher: PublicFetcher::new(None).expect("public fetching needs no fixture"),
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
            fetcher: PublicFetcher::new(Some(origin))?,
        })
    }
}

const MAX_PAGE_BYTES: usize = 256 * 1024;
use desegnos_core::MAX_DOWNLOADED_IMAGE_BYTES as MAX_IMAGE_BYTES;
const MAX_BRAND_PAGES: usize = 20;
const MAX_BRAND_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_PHOTOS: usize = 32;
const MAX_PHOTO_BYTES: usize = 2 * 1024 * 1024;
const MAX_PHOTO_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_WEBSITE_IMAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct WebsiteFiles {
    pub name: String,
    pub files: Vec<(String, Vec<u8>)>,
    pub skipped: Vec<String>,
    pub photo_pages: BTreeMap<String, Vec<desegnos_core::SourcePageRef>>,
}

/// Accept "example.com" as well as a full https:// address.
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

fn file_name(url: &Url, fallback: &str, extension: &str) -> String {
    let last = url
        .path_segments()
        .and_then(|mut parts| parts.next_back())
        .filter(|part| !part.is_empty())
        .unwrap_or(fallback);
    let stem: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .take(80)
        .collect();
    let stem = stem
        .rsplit_once('.')
        .map_or(stem.as_str(), |(stem, _)| stem)
        .to_owned();
    format!("{stem}.{extension}")
}

impl WebsiteFetcher {
    pub async fn fetch_website(&self, input: &str) -> Result<WebsiteFiles, String> {
        let fetcher = &self.fetcher;
        let page_url = match fetcher.fixture() {
            Some(origin) => origin
                .join(input)
                .map_err(|_| "Enter a website address.".to_owned())?,
            None => website_address(input)?,
        };
        let host = website_name(&page_url);
        let (html, _, page_url) = fetcher
            .document(FetchRequest {
                url: page_url.as_str(),
                mimes: &["text/html", "application/xhtml+xml"],
                limit: MAX_PAGE_BYTES,
                host_lock: None,
            })
            .await
            .map_err(|reason| format!("That website couldn’t be read: it {reason}."))?;
        let text = String::from_utf8_lossy(&html).into_owned();
        let links = desegnos_core::website_links(&text);
        let mut site = WebsiteFiles {
            name: host,
            files: vec![(
                "index.html".into(),
                format!("<!-- Source URL: {} -->\n{}", page_url, text).into_bytes(),
            )],
            skipped: vec![],
            photo_pages: BTreeMap::new(),
        };
        // Freeze bounded public page context before styles/images. Every redirect repeats
        // the downloader's address checks and must stay on the original host.
        let mut seen = BTreeSet::from([page_url.to_string()]);
        let mut queue = VecDeque::new();
        enqueue_pages(&page_url, &links.pages, &mut seen, &mut queue, true);
        let mut page_bytes = site.files[0].1.len();
        let mut requests = 1;
        while requests < MAX_BRAND_PAGES {
            let Some(url) = queue.pop_front() else {
                break;
            };
            requests += 1;
            match fetcher
                .document(FetchRequest {
                    url: url.as_str(),
                    mimes: &["text/html", "application/xhtml+xml"],
                    limit: MAX_PAGE_BYTES,
                    host_lock: page_url.host_str(),
                })
                .await
            {
                Ok((bytes, _, url)) => {
                    if page_bytes + bytes.len() > MAX_BRAND_TEXT_BYTES {
                        site.skipped
                            .push("Linked pages reached the 2 MiB text limit".into());
                        break;
                    }
                    page_bytes += bytes.len();
                    let text = String::from_utf8_lossy(&bytes);
                    let next = desegnos_core::website_links(&text);
                    enqueue_pages(&url, &next.pages, &mut seen, &mut queue, true);
                    let source = format!("<!-- Source URL: {} -->\n{}", url, text);
                    site.files.push((
                        format!("pages/{:02}-{}", requests, file_name(&url, "page", "html")),
                        source.into_bytes(),
                    ));
                }
                Err(reason) => site
                    .skipped
                    .push(format!("{url}: page skipped, it {reason}")),
            }
        }
        if !queue.is_empty() {
            site.skipped
                .push("Only the first 20 page requests are included".into());
        }
        for (index, href) in links.stylesheets.iter().enumerate() {
            let Ok(url) = page_url.join(href) else {
                site.skipped.push(format!("{href}: not a valid address"));
                continue;
            };
            match fetcher
                .fetch(FetchRequest {
                    url: url.as_str(),
                    mimes: &["text/css"],
                    limit: MAX_PAGE_BYTES,
                    host_lock: None,
                })
                .await
            {
                Ok((bytes, _)) => site.files.push((
                    format!(
                        "styles/{}",
                        file_name(&url, &format!("style-{}", index + 1), "css")
                    ),
                    bytes,
                )),
                Err(reason) => site
                    .skipped
                    .push(format!("{}: stylesheet skipped, it {reason}", url.as_str())),
            }
        }
        for href in &links.logos {
            let Ok(url) = page_url.join(href) else {
                site.skipped.push(format!("{href}: not a valid address"));
                continue;
            };
            match fetcher
                .fetch(FetchRequest {
                    url: url.as_str(),
                    mimes: &["image/png", "image/jpeg"],
                    limit: MAX_IMAGE_BYTES,
                    host_lock: None,
                })
                .await
            {
                Ok((bytes, mime)) => {
                    let extension = if mime == "image/png" { "png" } else { "jpg" };
                    let name = file_name(&url, "logo", extension);
                    let name = if name.to_ascii_lowercase().contains("logo") {
                        name
                    } else {
                        format!("logo-{name}")
                    };
                    site.files.push((format!("images/{name}"), bytes));
                }
                Err(reason) => site
                    .skipped
                    .push(format!("{}: image skipped, it {reason}", url.as_str())),
            }
        }
        // Keep explicit page attribution as CDN, query and extensionless image URLs do
        // not have a unique local filename. Download names are stable per original URL.
        let mut photos: BTreeMap<String, Vec<desegnos_core::SourcePageRef>> = BTreeMap::new();
        for (name, bytes) in &site.files {
            if !name.ends_with(".html") {
                continue;
            }
            let text = String::from_utf8_lossy(bytes);
            let Some(page) = desegnos_core::identity_source_page(name, &text) else {
                continue;
            };
            let Some(base) = page.url.as_deref().and_then(|url| Url::parse(url).ok()) else {
                continue;
            };
            for href in desegnos_core::website_links(&text).photos {
                if let Ok(mut url) = base.join(&href) {
                    url.set_fragment(None);
                    let pages = photos.entry(url.to_string()).or_default();
                    if !pages.iter().any(|source| source.path == page.path) {
                        pages.push(page.reference());
                    }
                }
            }
        }
        let mut photo_bytes = 0;
        for (address, pages) in photos.into_iter().take(MAX_PHOTOS) {
            match fetcher
                .fetch(FetchRequest {
                    url: &address,
                    mimes: &["image/png", "image/jpeg", "image/webp"],
                    limit: MAX_PHOTO_BYTES,
                    host_lock: None,
                })
                .await
            {
                Ok((bytes, mime)) if photo_bytes + bytes.len() <= MAX_PHOTO_TOTAL_BYTES => {
                    photo_bytes += bytes.len();
                    let extension = match mime.as_str() {
                        "image/png" => "png",
                        "image/webp" => "webp",
                        _ => "jpg",
                    };
                    let name = format!(
                        "photos/{}.{}",
                        desegnos_core::digest(address.as_bytes()),
                        extension
                    );
                    site.photo_pages.insert(name.clone(), pages);
                    site.files.push((name, bytes));
                }
                Ok(_) => site
                    .skipped
                    .push(format!("{address}: photo download budget is full")),
                Err(reason) => site
                    .skipped
                    .push(format!("{address}: photo skipped, it {reason}")),
            }
        }
        Ok(site)
    }
}

fn enqueue_pages(
    base: &Url,
    links: &[String],
    seen: &mut BTreeSet<String>,
    queue: &mut VecDeque<Url>,
    prioritize_work: bool,
) {
    let mut candidates = Vec::new();
    for href in links {
        if seen.len() >= 256 {
            break;
        }
        let Ok(mut url) = base.join(href) else {
            continue;
        };
        url.set_fragment(None);
        if !prioritize_work && matches!(url.path(), "/index.html" | "/index.htm") {
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
            let work = [
                "project",
                "tutorial",
                "blog",
                "article",
                "portfolio",
                "work",
                "case-study",
            ]
            .iter()
            .any(|part| path.contains(part));
            candidates.push((prioritize_work && !work, url));
        }
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.as_str().cmp(b.1.as_str())));
    queue.extend(candidates.into_iter().map(|(_, url)| url));
}

/// One renderable snapshot fetched from a public website. Files keep their original site paths
/// so relative CSS, image and link URLs keep working when the snapshot is served locally.
#[derive(Debug, Default)]
pub struct WebsiteSnapshot {
    pub name: String,
    pub files: Vec<(String, Vec<u8>)>,
    pub pages: Vec<WebsiteSnapshotPage>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsiteSnapshotPage {
    pub route: String,
    pub title: String,
}

const MAX_SNAPSHOT_PAGES: usize = 30;
const MAX_SNAPSHOT_PAGE_BYTES: usize = 1024 * 1024;
const MAX_SNAPSHOT_ASSET_BYTES: usize = 4 * 1024 * 1024;
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

fn page_file(route: &str) -> String {
    let route = route.trim_start_matches('/');
    if route.is_empty() {
        "index.html".into()
    } else if route.ends_with('/') {
        format!("{route}index.html")
    } else if route.ends_with(".html") || route.ends_with(".htm") {
        route.to_owned()
    } else {
        format!("{route}.html")
    }
}

fn snapshot_route(url: &Url) -> String {
    let path = url.path();
    if path == "/index.html" || path == "/index.htm" {
        "/".into()
    } else {
        path.to_owned()
    }
}

fn decoded_path(path: &str) -> Option<String> {
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
        || value.split('/').any(|part| part.is_empty() || part == "." || part == ".." || part.starts_with('.'))
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
    let host = origin.host_str().unwrap_or_default();
    let mut text = text.replace(prefix, "");
    if !host.is_empty() {
        text = text.replace(&format!("//{host}"), "");
    }
    text
}

/// Fetch up to 30 same-site pages and the assets they need, with size and request caps.
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
        let mut assets: VecDeque<(Url, Url)> = VecDeque::new();
        let mut asset_seen = BTreeSet::new();
        while requests < MAX_SNAPSHOT_PAGES {
            let Some(url) = page_queue.pop_front() else {
                break;
            };
            requests += 1;
            let (bytes, _mime, final_url) = fetcher
                .document(FetchRequest {
                    url: url.as_str(),
                    mimes: &["text/html", "application/xhtml+xml"],
                    limit: MAX_SNAPSHOT_PAGE_BYTES,
                    host_lock: Some(first.host_str().unwrap_or_default()),
                })
                .await
                .map_err(|reason| format!("That website couldn’t be read: it {reason}."))?;
            let origin = Url::parse(&format!(
                "{}://{}",
                final_url.scheme(),
                final_url.authority()
            ))
            .ok();
            let Some(origin) = origin else { continue };
            total += bytes.len();
            if total > MAX_SNAPSHOT_TOTAL_BYTES {
                return Err("The website is larger than the import limit.".into());
            }
            let text = localize_same_site(&String::from_utf8_lossy(&bytes), &origin);
            let route = snapshot_route(&url);
            let file = page_file(&route);
            files.insert(file.clone(), text.clone().into_bytes());
            let title = desegnos_core::identity_source_page(&file, &text)
                .map(|page| page.title)
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| {
                    if route == "/" {
                        "Home".into()
                    } else {
                        route
                            .trim_matches('/')
                            .rsplit('/')
                            .next()
                            .unwrap_or("Page")
                            .replace(['-', '_'], " ")
                    }
                });
            snapshot.pages.push(WebsiteSnapshotPage { route, title });
            let links = desegnos_core::website_links(&text);
            enqueue_pages(&url, &links.pages, &mut pages_seen, &mut page_queue, false);
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
                    assets.push_back((asset, url.clone()));
                }
            }
            if page_queue.is_empty() || requests >= MAX_SNAPSHOT_PAGES {
                break;
            }
        }
        if !page_queue.is_empty() {
            snapshot
                .skipped
                .push("Only the first 30 same-site pages are imported".into());
        }
        let mut downloaded = 0;
        while let Some((url, _referrer)) = assets.pop_front() {
            if downloaded >= MAX_SNAPSHOT_ASSETS {
                snapshot
                    .skipped
                    .push("Only the first 240 page assets are imported".into());
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
                .fetch(FetchRequest {
                    url: url.as_str(),
                    mimes: ASSET_MIMES,
                    limit: MAX_SNAPSHOT_ASSET_BYTES,
                    host_lock: None,
                })
                .await
            {
                Ok((bytes, _mime)) => {
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
                                assets.push_back((asset, url.clone()));
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

/// Download one picture referenced by an imported page. Same public-address and size rules as
/// every other Desegnos download; the bytes are capped before they reach the project store.
impl WebsiteFetcher {
    pub async fn fetch_website_image(&self, source: &str) -> Result<(Vec<u8>, String), String> {
        let fetcher = &self.fetcher;
        fetcher
            .fetch(FetchRequest {
                url: source,
                mimes: &[
                    "image/png",
                    "image/jpeg",
                    "image/webp",
                    "image/gif",
                    "image/svg+xml",
                ],
                limit: MAX_WEBSITE_IMAGE_BYTES,
                host_lock: None,
            })
            .await
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
    async fn fetches_page_stylesheet_and_logo_and_reports_skips() {
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
                        "/" => ("text/html", br#"<link rel="stylesheet" href="/site.css"><link rel="icon" href="/favicon.ico"><img src="/brand/acme-logo.png" alt="Acme">"#.to_vec()),
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
        let site = WebsiteFetcher::for_fixture(&origin)
            .unwrap()
            .fetch_website("/")
            .await
            .unwrap();
        let names: Vec<_> = site.files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            ["index.html", "styles/site.css", "images/acme-logo.png"]
        );
        assert!(
            site.skipped
                .iter()
                .any(|reason| reason.contains("favicon.ico"))
        );
        let error = WebsiteFetcher::new()
            .fetch_website("https://127.0.0.1/")
            .await
            .unwrap_err();
        assert!(
            error.contains("couldn’t be read") || error.contains("public"),
            "{error}"
        );
    }
}

#[cfg(test)]
mod crawl_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn same_host_crawl_is_deduplicated_bounded_and_preserves_source_urls() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let requested = Arc::new(Mutex::new(Vec::<String>::new()));
        let observed = requested.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let observed = observed.clone();
                tokio::spawn(async move {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).await.unwrap();
                    let path = String::from_utf8_lossy(&buffer[..count])
                        .split_whitespace()
                        .nth(1)
                        .unwrap()
                        .to_owned();
                    observed.lock().unwrap().push(path.clone());
                    let body = if path == "/" {
                        format!("<a href='/'>Home</a><a href='https://other.example/projects/private'>Outside</a><a href='/logout'>Logout</a><a href='/file.pdf'>PDF</a>{}", (0..30).map(|index| format!("<a href='/projects/{index:02}'>Project {index}</a><a href='/projects/{index:02}#details'>Duplicate</a>")).collect::<String>())
                    } else {
                        "<title>Actual project title</title><p>A detailed project summary.</p><a href='/'>Home</a>".into()
                    };
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(header.as_bytes()).await.unwrap();
                    socket.write_all(body.as_bytes()).await.unwrap();
                });
            }
        });
        let site = WebsiteFetcher::for_fixture(&origin)
            .unwrap()
            .fetch_website("/")
            .await
            .unwrap();
        assert_eq!(site.files.len(), 20);
        let paths = requested.lock().unwrap();
        assert_eq!(paths.len(), 20);
        assert_eq!(paths.iter().collect::<BTreeSet<_>>().len(), 20);
        assert!(
            paths
                .iter()
                .all(|path| path == "/" || path.starts_with("/projects/"))
        );
        assert!(
            String::from_utf8_lossy(&site.files[1].1)
                .contains(&format!("Source URL: {origin}projects/00"))
        );
        assert!(
            site.skipped
                .iter()
                .any(|reason| reason.contains("20 page requests"))
        );
        server.abort();
    }
}

#[cfg(test)]
mod page_photo_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn crawl_fetches_page_photos_and_retains_each_originating_page() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).await.unwrap();
                    let request = String::from_utf8_lossy(&buffer[..count]);
                    let path = request.split_whitespace().nth(1).unwrap();
                    let (mime, body) = match path {
                        "/" => ("text/html", b"<title>Home</title><a href='/projects'>Projects</a><img src='/photos/home.png'>".to_vec()),
                        "/projects" => ("text/html", b"<title>Our projects</title><img src='/photos/robot.png'>".to_vec()),
                        _ => {
                            let color = if path.contains("home") { [120, 20, 40] } else { [20, 90, 140] };
                            let image = image::RgbImage::from_pixel(320, 240, image::Rgb(color));
                            let mut bytes = std::io::Cursor::new(Vec::new());
                            image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
                            ("image/png", bytes.into_inner())
                        }
                    };
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
                    socket.write_all(&body).await.unwrap();
                });
            }
        });
        let site = WebsiteFetcher::for_fixture(&origin)
            .unwrap()
            .fetch_website("/")
            .await
            .unwrap();
        let page = site
            .files
            .iter()
            .find(|(name, _)| name.starts_with("pages/"))
            .unwrap();
        let source = String::from_utf8_lossy(&page.1);
        let page_ref = desegnos_core::identity_source_page(&page.0, &source).unwrap();
        assert_eq!(page_ref.path, "/projects");
        assert_eq!(page_ref.title, "Our projects");
        assert_eq!(page_ref.url, Some(format!("{origin}projects")));
        assert!(
            site.photo_pages
                .values()
                .any(|pages| pages.iter().any(|page| page.path == "/projects"))
        );
        assert!(desegnos_core::page_references_image(
            &page.0,
            &source,
            "photos/robot.png"
        ));
        assert!(!desegnos_core::page_references_image(
            &page.0,
            &source,
            "photos/home.png"
        ));
        server.abort();
    }
}
