//! Unauthenticated public-web fetching with DNS pinning and bounded bodies.
use reqwest::Url;
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

pub(crate) struct FetchRequest<'a> {
    pub url: &'a str,
    pub mimes: &'a [&'a str],
    pub limit: usize,
    pub host_lock: Option<&'a str>,
}
pub(crate) struct PublicFetcher {
    fixture: Option<Url>,
}
impl PublicFetcher {
    pub fn new(fixture: Option<&Url>) -> Result<Self, String> {
        #[cfg(not(any(test, feature = "test-support")))]
        if fixture.is_some() {
            return Err("Test website sources are unavailable in this build.".into());
        }
        Ok(Self {
            fixture: fixture.cloned(),
        })
    }
    pub fn fixture(&self) -> Option<&Url> {
        self.fixture.as_ref()
    }
    pub async fn fetch(
        &self,
        request: FetchRequest<'_>,
    ) -> Result<(Vec<u8>, String), &'static str> {
        self.document(request)
            .await
            .map(|(bytes, mime, _)| (bytes, mime))
    }
    pub async fn document(
        &self,
        request: FetchRequest<'_>,
    ) -> Result<(Vec<u8>, String, Url), &'static str> {
        let mut url = Url::parse(request.url).map_err(|_| "is not a valid web address")?;
        for _ in 0..=2 {
            if request
                .host_lock
                .is_some_and(|host| url.host_str() != Some(host))
            {
                return Err("redirected outside the source website");
            }
            if !permitted_url(&url, self.fixture.as_ref()) {
                return Err("must be a public https:// address");
            }
            let host = url
                .host_str()
                .ok_or("has no host")?
                .trim_matches(['[', ']']);
            let port = url.port_or_known_default().ok_or("has no port")?;
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| "could not be found")?
                .take(17)
                .collect();
            let fixture_host = self
                .fixture
                .as_ref()
                .is_some_and(|origin| url.origin() == origin.origin());
            if addresses.is_empty()
                || addresses.len() > 16
                || addresses.iter().any(|address| {
                    !public_address(address.ip()) && !(fixture_host && address.ip().is_loopback())
                })
            {
                return Err("points to a private or special network address");
            }
            let client = crate::net::hardened_client(!fixture_host)
                .resolve_to_addrs(host, &addresses)
                .timeout(Duration::from_secs(8))
                .connect_timeout(Duration::from_secs(4))
                .build()
                .map_err(|_| "could not be requested")?;
            let mut response = client
                .get(url.clone())
                .header("Accept", request.mimes.join(","))
                .send()
                .await
                .map_err(|_| "did not respond")?;
            if response.status().is_redirection() {
                url = url
                    .join(
                        response
                            .headers()
                            .get("location")
                            .and_then(|value| value.to_str().ok())
                            .ok_or("redirected without a destination")?,
                    )
                    .map_err(|_| "redirected to an invalid address")?;
                continue;
            }
            if !response.status().is_success() {
                return Err("returned an error page");
            }
            if response
                .headers()
                .get("content-encoding")
                .is_some_and(|value| value != "identity")
            {
                return Err("used an unsupported compressed response");
            }
            let mime = response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .map(|value| value.trim().to_ascii_lowercase())
                .ok_or("has no content type")?;
            if !request.mimes.contains(&mime.as_str()) {
                return Err("is not a supported file type");
            }
            let bytes = crate::net::bounded_body(
                &mut response,
                request.limit,
                &crate::CancellationToken::default(),
                tokio::time::Instant::now() + Duration::from_secs(8),
            )
            .await
            .map_err(|error| match error.kind {
                crate::ErrorKind::OutputLimit => "is too large",
                _ => "stopped responding",
            })?
            .to_vec();
            return Ok((bytes, mime, url));
        }
        Err("redirected too many times")
    }
}
pub(crate) fn permitted_url(url: &Url, fixture: Option<&Url>) -> bool {
    let ordinary = url.scheme() == "https" && url.port_or_known_default() == Some(443);
    let fixture =
        fixture.is_some_and(|origin| url.scheme() == "http" && url.origin() == origin.origin());
    (ordinary || fixture)
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.as_str().len() <= 4096
}

pub(crate) fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !matches!(a, 0 | 10 | 127 | 224..=255)
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 169 && b == 254)
                && !(a == 172 && (16..=31).contains(&b))
                && !(a == 192 && (b == 168 || b == 0 || (b == 2 && c == 0)))
                && !(a == 192 && b == 88 && c == 99)
                && !(a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                && !(a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            // Permit global unicast only, excluding special-use, transition,
            // benchmark and documentation ranges (including IPv4 mappings).
            s[0] & 0xe000 == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use desegnos_core::limits::MAX_DOWNLOADED_IMAGE_BYTES;
    use image::ImageFormat;
    use std::io::Cursor;
    #[cfg(test)]
    async fn download(source: &str, fixture: Option<&Url>) -> Option<String> {
        let (bytes, mime) = PublicFetcher::new(fixture)
            .unwrap()
            .fetch(FetchRequest {
                url: source,
                mimes: &["image/png", "image/jpeg"],
                limit: MAX_DOWNLOADED_IMAGE_BYTES,
                host_lock: None,
            })
            .await
            .ok()?;
        Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
    }

    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[test]
    fn rejects_private_special_mapped_and_documentation_addresses() {
        for value in [
            "0.0.0.0",
            "10.1.2.3",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.1.1",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "::1",
            "::ffff:8.8.8.8",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2002:0808:0808::1",
            "3fff::1",
        ] {
            assert!(!public_address(value.parse().unwrap()), "{value}");
        }
        for value in ["8.8.8.8", "104.18.1.1", "2606:4700:4700::1111"] {
            assert!(public_address(value.parse().unwrap()));
        }
    }
    #[test]
    fn production_urls_require_public_https_without_credentials_or_unusual_ports() {
        for value in [
            "http://example.com/a.png",
            "https://user:secret@example.com/a.png",
            "https://example.com:8443/a.png",
            "file:///a.png",
            "https://example.com/a.png#x",
        ] {
            assert!(!permitted_url(&Url::parse(value).unwrap(), None));
        }
        assert!(permitted_url(
            &Url::parse("https://upload.wikimedia.org/photo.png").unwrap(),
            None
        ));
    }
    #[tokio::test]
    async fn real_http_download_is_unauthenticated_bounded_and_rechecks_redirects() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = seen.clone();
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut bytes = vec![0; 8192];
                let size = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..size]).to_string();
                let path = request.split_whitespace().nth(1).unwrap().to_string();
                captured.lock().unwrap().push(request);
                let (status, extra, mime, body) = match path.as_str() {
                    "/images/private" => (
                        302,
                        "Location: https://127.0.0.1/private\r\n".to_string(),
                        "text/plain",
                        Vec::new(),
                    ),
                    "/images/loop" => (
                        302,
                        "Location: /images/loop\r\n".to_string(),
                        "text/plain",
                        Vec::new(),
                    ),
                    "/images/large" => (
                        200,
                        String::new(),
                        "image/png",
                        vec![0; MAX_DOWNLOADED_IMAGE_BYTES + 1],
                    ),
                    _ => (200, String::new(), "image/png", png.get_ref().clone()),
                };
                let header = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    body.len()
                );
                if socket.write_all(header.as_bytes()).await.is_ok() {
                    let _ = socket.write_all(&body).await;
                }
            }
        });
        assert!(
            download(origin.join("/images/good").unwrap().as_str(), Some(&origin))
                .await
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        for path in ["large", "private", "loop"] {
            assert!(
                download(
                    origin.join(&format!("/images/{path}")).unwrap().as_str(),
                    Some(&origin)
                )
                .await
                .is_none(),
                "{path}"
            );
        }
        let requests = seen.lock().unwrap();
        assert_eq!(
            requests.len(),
            6,
            "Redirects must be bounded and private destinations must never receive a request"
        );
        for request in requests.iter() {
            let lower = request.to_ascii_lowercase();
            assert!(
                !lower.contains("authorization:")
                    && !lower.contains("cookie:")
                    && !lower.contains("referer:")
            );
        }
        task.abort();
    }
}
