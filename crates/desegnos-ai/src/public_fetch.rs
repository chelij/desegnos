//! Unauthenticated public-web fetching with DNS pinning and bounded bodies.
use reqwest::Url;
use std::net::{IpAddr, SocketAddr};

const MAX_RESOLVED_ADDRESSES: usize = 16;

pub(crate) struct FetchRequest<'a> {
    pub url: &'a str,
    pub mimes: &'a [&'a str],
    pub limit: usize,
    pub host_lock: Option<&'a str>,
}
/// Why a public fetch failed. `Display` completes the sentence "The source ...".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FetchError {
    InvalidAddress,
    OutsideSource,
    NotPublic,
    NoHost,
    NoPort,
    NotFound,
    PrivateNetwork,
    CouldNotRequest,
    NoResponse,
    NoRedirectTarget,
    BadRedirect,
    ErrorPage,
    Compressed,
    NoContentType,
    UnsupportedType,
    TooLarge,
    Stalled,
    TooManyRedirects,
}
impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidAddress => "is not a valid web address",
            Self::OutsideSource => "redirected outside the source website",
            Self::NotPublic => "must be a public https:// address",
            Self::NoHost => "has no host",
            Self::NoPort => "has no port",
            Self::NotFound => "could not be found",
            Self::PrivateNetwork => "points to a private or special network address",
            Self::CouldNotRequest => "could not be requested",
            Self::NoResponse => "did not respond",
            Self::NoRedirectTarget => "redirected without a destination",
            Self::BadRedirect => "redirected to an invalid address",
            Self::ErrorPage => "returned an error page",
            Self::Compressed => "used an unsupported compressed response",
            Self::NoContentType => "has no content type",
            Self::UnsupportedType => "is not a supported file type",
            Self::TooLarge => "is too large",
            Self::Stalled => "stopped responding",
            Self::TooManyRedirects => "redirected too many times",
        })
    }
}

pub(crate) struct PublicFetcher {
    #[cfg(any(test, feature = "test-support"))]
    fixture: Option<Url>,
}
impl PublicFetcher {
    pub fn new() -> Self {
        Self {
            #[cfg(any(test, feature = "test-support"))]
            fixture: None,
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_fixture(origin: &Url) -> Self {
        Self {
            fixture: Some(origin.clone()),
        }
    }
    pub fn fixture(&self) -> Option<&Url> {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.fixture.as_ref()
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            None
        }
    }
    /// Only a test fixture's own origin may use loopback and plain http.
    fn loopback(&self, url: &Url) -> bool {
        self.fixture()
            .is_some_and(|origin| url.origin() == origin.origin())
    }
    pub async fn document(
        &self,
        request: FetchRequest<'_>,
    ) -> Result<(Vec<u8>, String, Url), FetchError> {
        let mut url = Url::parse(request.url).map_err(|_| FetchError::InvalidAddress)?;
        for _ in 0..=2 {
            if request
                .host_lock
                .is_some_and(|host| url.host_str() != Some(host))
            {
                return Err(FetchError::OutsideSource);
            }
            let loopback = self.loopback(&url);
            if !permitted_url(&url, loopback) {
                return Err(FetchError::NotPublic);
            }
            let host = url
                .host_str()
                .ok_or(FetchError::NoHost)?
                .trim_matches(['[', ']']);
            let port = url.port_or_known_default().ok_or(FetchError::NoPort)?;
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| FetchError::NotFound)?
                .take(MAX_RESOLVED_ADDRESSES + 1)
                .collect();
            if addresses.is_empty()
                || addresses.len() > MAX_RESOLVED_ADDRESSES
                || addresses.iter().any(|address| {
                    !public_address(address.ip()) && !(loopback && address.ip().is_loopback())
                })
            {
                return Err(FetchError::PrivateNetwork);
            }
            let client = crate::net::hardened_client(!loopback)
                .resolve_to_addrs(host, &addresses)
                .timeout(crate::net::PUBLIC_FETCH_TIMEOUT)
                .connect_timeout(crate::net::PUBLIC_CONNECT_TIMEOUT)
                .build()
                .map_err(|_| FetchError::CouldNotRequest)?;
            let mut response = client
                .get(url.clone())
                .header("Accept", request.mimes.join(","))
                .send()
                .await
                .map_err(|_| FetchError::NoResponse)?;
            if response.status().is_redirection() {
                url = url
                    .join(
                        response
                            .headers()
                            .get("location")
                            .and_then(|value| value.to_str().ok())
                            .ok_or(FetchError::NoRedirectTarget)?,
                    )
                    .map_err(|_| FetchError::BadRedirect)?;
                continue;
            }
            if !response.status().is_success() {
                return Err(FetchError::ErrorPage);
            }
            if response
                .headers()
                .get("content-encoding")
                .is_some_and(|value| value != "identity")
            {
                return Err(FetchError::Compressed);
            }
            let mime = response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .map(|value| value.trim().to_ascii_lowercase())
                .ok_or(FetchError::NoContentType)?;
            if !request.mimes.contains(&mime.as_str()) {
                return Err(FetchError::UnsupportedType);
            }
            let bytes = crate::net::bounded_body(
                &mut response,
                request.limit,
                &crate::CancellationToken::default(),
                tokio::time::Instant::now() + crate::net::PUBLIC_FETCH_TIMEOUT,
            )
            .await
            .map_err(|error| match error.kind {
                crate::ErrorKind::OutputLimit => FetchError::TooLarge,
                _ => FetchError::Stalled,
            })?
            .to_vec();
            return Ok((bytes, mime, url));
        }
        Err(FetchError::TooManyRedirects)
    }
}
pub(crate) fn permitted_url(url: &Url, loopback: bool) -> bool {
    let ordinary = url.scheme() == "https" && url.port_or_known_default() == Some(443);
    (ordinary || loopback && url.scheme() == "http")
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
    use desegnos_core::limits::MAX_DOWNLOADED_IMAGE_BYTES;
    use image::ImageFormat;
    use std::io::Cursor;
    #[cfg(test)]
    async fn download(source: &str, fixture: Option<&Url>) -> Option<String> {
        let (bytes, mime, _) = fixture
            .map_or_else(PublicFetcher::new, PublicFetcher::for_fixture)
            .document(FetchRequest {
                url: source,
                mimes: &["image/png", "image/jpeg"],
                limit: MAX_DOWNLOADED_IMAGE_BYTES,
                host_lock: None,
            })
            .await
            .ok()?;
        Some(desegnos_core::limits::image_data_uri(&mime, &bytes))
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
            assert!(!permitted_url(&Url::parse(value).unwrap(), false));
        }
        assert!(permitted_url(
            &Url::parse("https://upload.wikimedia.org/photo.png").unwrap(),
            false
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
