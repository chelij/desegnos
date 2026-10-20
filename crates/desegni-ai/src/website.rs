//! Brand import from a website address: the home page, its stylesheets and logo images.
//! Every request uses the same public-address, size and timeout rules as generation image
//! downloads, carries no credentials, and never runs page code.
use crate::images::fetch_public;
use reqwest::Url;

/// Web address type used by the website fetcher (re-exported so callers need no HTTP dependency).
pub use reqwest::Url as WebUrl;

const MAX_PAGE_BYTES: usize = 256 * 1024;
const MAX_IMAGE_BYTES: usize = 256 * 1024;

#[derive(Debug, Default)]
pub struct WebsiteFiles {
    pub name: String,
    pub files: Vec<(String, Vec<u8>)>,
    pub skipped: Vec<String>,
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
    let (html, _) = fetch_public(
        page_url.as_str(),
        fixture,
        "/",
        "text/html,application/xhtml+xml",
        &["text/html", "application/xhtml+xml"],
        MAX_PAGE_BYTES,
    )
    .await
    .map_err(|reason| format!("That website couldn’t be read: it {reason}."))?;
    let text = String::from_utf8_lossy(&html).into_owned();
    let links = desegni_core::website_links(&text);
    let mut site = WebsiteFiles {
        name: host,
        files: vec![("index.html".into(), text.into_bytes())],
        skipped: vec![],
    };
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
    Ok(site)
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
