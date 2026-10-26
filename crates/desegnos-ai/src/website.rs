//! Brand import from a website address: the home page, its stylesheets and logo images.
//! Every request uses the same public-address, size and timeout rules as generation image
//! downloads, carries no credentials, and never runs page code.
use crate::images::{fetch_public, fetch_public_document};
use reqwest::Url;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Web address type used by the website fetcher (re-exported so callers need no HTTP dependency).
pub use reqwest::Url as WebUrl;

const MAX_PAGE_BYTES: usize = 256 * 1024;
const MAX_IMAGE_BYTES: usize = 256 * 1024;

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

pub async fn fetch_website(input: &str, fixture: Option<&Url>) -> Result<WebsiteFiles, String> {
    #[cfg(not(any(test, feature = "test-support")))]
    if fixture.is_some() {
        return Err("Test website sources are unavailable in this build.".into());
    }
    let page_url = match fixture {
        Some(origin) => origin
            .join(input)
            .map_err(|_| "Enter a website address.".to_owned())?,
        None => website_address(input)?,
    };
    let host = page_url
        .host_str()
        .unwrap_or("website")
        .trim_start_matches("www.")
        .to_owned();
    let (html, _, page_url) = fetch_public_document(
        page_url.as_str(),
        fixture,
        "/",
        "text/html,application/xhtml+xml",
        &["text/html", "application/xhtml+xml"],
        MAX_PAGE_BYTES,
        None,
    )
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
    enqueue_pages(&page_url, &links.pages, &mut seen, &mut queue);
    let mut page_bytes = site.files[0].1.len();
    let mut requests = 1;
    while requests < 20 {
        let Some(url) = queue.pop_front() else {
            break;
        };
        requests += 1;
        match fetch_public_document(
            url.as_str(),
            fixture,
            "/",
            "text/html,application/xhtml+xml",
            &["text/html", "application/xhtml+xml"],
            MAX_PAGE_BYTES,
            page_url.host_str(),
        )
        .await
        {
            Ok((bytes, _, url)) => {
                if page_bytes + bytes.len() > 2 * 1024 * 1024 {
                    site.skipped
                        .push("Linked pages reached the 2 MiB text limit".into());
                    break;
                }
                page_bytes += bytes.len();
                let text = String::from_utf8_lossy(&bytes);
                let next = desegnos_core::website_links(&text);
                enqueue_pages(&url, &next.pages, &mut seen, &mut queue);
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
        match fetch_public(
            url.as_str(),
            fixture,
            "/",
            "text/css",
            &["text/css"],
            MAX_PAGE_BYTES,
        )
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
        match fetch_public(
            url.as_str(),
            fixture,
            "/",
            "image/png,image/jpeg",
            &["image/png", "image/jpeg"],
            MAX_IMAGE_BYTES,
        )
        .await
        {
            Ok((bytes, mime)) => {
                let extension = if mime == "image/png" { "png" } else { "jpg" };
                // Keep "logo" in the name so the importer's logo selection recognises it.
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
    for (address, pages) in photos.into_iter().take(32) {
        match fetch_public(
            &address,
            fixture,
            "/",
            "image/png,image/jpeg,image/webp",
            &["image/png", "image/jpeg", "image/webp"],
            2 * 1024 * 1024,
        )
        .await
        {
            Ok((bytes, mime)) if photo_bytes + bytes.len() <= 8 * 1024 * 1024 => {
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

fn enqueue_pages(
    base: &Url,
    links: &[String],
    seen: &mut BTreeSet<String>,
    queue: &mut VecDeque<Url>,
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
            candidates.push((!work, url));
        }
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.as_str().cmp(b.1.as_str())));
    queue.extend(candidates.into_iter().map(|(_, url)| url));
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
        let site = fetch_website("/", Some(&origin)).await.unwrap();
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
        let error = fetch_website("https://127.0.0.1/", None).await.unwrap_err();
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
        let site = fetch_website("/", Some(&origin)).await.unwrap();
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
        let site = fetch_website("/", Some(&origin)).await.unwrap();
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
