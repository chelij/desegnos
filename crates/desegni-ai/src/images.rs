//! Generation image adapter. It receives no credential and never evaluates code.
use crate::{CancellationToken, OpenAiClient, ProviderResponse};
use base64::{Engine, engine::general_purpose::STANDARD};
use desegni_core::{
    DesignCriticism, GenerationCandidate, MAX_DOWNLOADED_IMAGE_BYTES, Project, RequirementsSnapshot,
};
use image::{ImageFormat, ImageReader, Limits};
use reqwest::{Client, Url};
use std::{
    collections::BTreeMap,
    io::Cursor,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

fn public_address(ip: IpAddr) -> bool {
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

fn permitted_url(url: &Url, fixture: Option<&Url>) -> bool {
    let ordinary = url.scheme() == "https" && url.port_or_known_default() == Some(443);
    let fixture = fixture.is_some_and(|origin| {
        url.scheme() == "http"
            && url.origin() == origin.origin()
            && url.path().starts_with("/images/")
    });
    (ordinary || fixture)
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.as_str().len() <= 4096
}

fn embedded_raster(bytes: &[u8], mime: &str) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_DOWNLOADED_IMAGE_BYTES {
        return None;
    }
    let format = image::guess_format(bytes).ok()?;
    if !matches!(
        (format, mime),
        (ImageFormat::Png, "image/png") | (ImageFormat::Jpeg, "image/jpeg")
    ) {
        return None;
    }
    let dimensions = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .ok()?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || dimensions.0 > 4096
        || dimensions.1 > 4096
        || u64::from(dimensions.0) * u64::from(dimensions.1) > 8_000_000
    {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().ok()?;
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

async fn download(source: &str, fixture: Option<&Url>) -> Option<String> {
    let mut url = Url::parse(source).ok()?;
    for _ in 0..=2 {
        if !permitted_url(&url, fixture) {
            return None;
        }
        let host = url.host_str()?.trim_matches(['[', ']']);
        let port = url.port_or_known_default()?;
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .ok()?
            .take(17)
            .collect();
        let fixture_host = fixture.is_some_and(|origin| url.origin() == origin.origin());
        if addresses.is_empty()
            || addresses.len() > 16
            || addresses.iter().any(|address| {
                !public_address(address.ip()) && !(fixture_host && address.ip().is_loopback())
            })
        {
            return None;
        }
        // Resolve once, validate every answer, then pin these exact addresses.
        // Redirects repeat all checks with a fresh unauthenticated client.
        let client = Client::builder()
            .tls_backend_rustls()
            .https_only(!fixture_host)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .tls_sslkeylogfile(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .resolve_to_addrs(host, &addresses)
            .timeout(Duration::from_secs(8))
            .connect_timeout(Duration::from_secs(4))
            .user_agent(concat!("Desegnos/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok()?;
        let mut response = client
            .get(url.clone())
            .header("Accept", "image/png,image/jpeg")
            .send()
            .await
            .ok()?;
        if response.status().is_redirection() {
            url = url
                .join(response.headers().get("location")?.to_str().ok()?)
                .ok()?;
            continue;
        }
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|size| size > MAX_DOWNLOADED_IMAGE_BYTES as u64)
            || response
                .headers()
                .get("content-encoding")
                .is_some_and(|value| value != "identity")
        {
            return None;
        }
        let mime = response
            .headers()
            .get("content-type")?
            .to_str()
            .ok()?
            .split(';')
            .next()?
            .trim()
            .to_ascii_lowercase();
        if !matches!(mime.as_str(), "image/png" | "image/jpeg") {
            return None;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len() + chunk.len() > MAX_DOWNLOADED_IMAGE_BYTES {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        return tokio::task::spawn_blocking(move || embedded_raster(&bytes, &mime))
            .await
            .ok()?;
    }
    None
}

impl OpenAiClient {
    /// Resolve the explicitly declared image capability before candidate
    /// acceptance. Legacy frozen requests never gain network permission.
    pub async fn download_images(
        &self,
        mut response: ProviderResponse,
        snapshot: &RequirementsSnapshot,
        project: &Project,
        cancellation: &CancellationToken,
    ) -> ProviderResponse {
        let format = snapshot.format_version().unwrap_or(0);
        if format < 7 {
            return response;
        }
        let Ok(mut candidate) = GenerationCandidate::from_json(&response.text) else {
            return response;
        };
        if format >= 8 && candidate.resolve_project_assets(project).is_err() {
            return response;
        }
        if candidate.validate(snapshot, project).is_err() {
            return response;
        }
        let Ok(urls) = candidate.image_download_urls() else {
            return response;
        };
        if urls.is_empty() {
            if format >= 8
                && let Ok(text) = serde_json::to_string(&candidate)
            {
                response.text = text;
            }
            return response;
        }
        #[cfg(any(test, feature = "test-support"))]
        let fixture = Url::parse(&self.endpoint)
            .ok()
            .filter(|url| url.scheme() == "http");
        #[cfg(not(any(test, feature = "test-support")))]
        let fixture: Option<Url> = None;
        let mut images = BTreeMap::new();
        let limit = snapshot
            .capabilities
            .as_ref()
            .map_or(4, |capabilities| usize::from(capabilities.image_downloads));
        for url in urls.into_iter().take(limit) {
            let result = tokio::select! {
                _ = cancellation.cancelled() => return response,
                result = tokio::time::timeout(Duration::from_secs(8), download(&url, fixture.as_ref())) => result.ok().flatten(),
            };
            if let Some(image) = result {
                images.insert(url, image);
            }
        }
        if let Ok(missing) = candidate.embed_downloaded_images(&images) {
            if missing > 0 && candidate.criticism.len() < desegni_core::MAX_DESIGN_CRITICISMS {
                candidate.criticism.push(DesignCriticism { region_id: None,
                    message: format!("{missing} online image(s) could not be included."),
                    suggestion: "Clearly labelled image placeholders are shown. Replace them with your own images or try another direct PNG/JPEG link.".into() });
            }
            if candidate.validate(snapshot, project).is_ok()
                && let Ok(text) = serde_json::to_string(&candidate)
            {
                response.text = text;
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    #[test]
    fn raster_decoder_rejects_html_svg_mime_spoofs_and_truncation() {
        assert!(embedded_raster(b"<html>not a photo</html>", "image/png").is_none());
        assert!(
            embedded_raster(
                b"<svg xmlns='http://www.w3.org/2000/svg'/>",
                "image/svg+xml"
            )
            .is_none()
        );
        let mut output = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut output, ImageFormat::Png)
            .unwrap();
        assert!(embedded_raster(output.get_ref(), "image/png").is_some());
        assert!(embedded_raster(output.get_ref(), "image/jpeg").is_none());
        assert!(embedded_raster(&output.get_ref()[..30], "image/png").is_none());
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
                    "/images/spoof" => (
                        200,
                        String::new(),
                        "image/png",
                        b"<html>not a photo</html>".to_vec(),
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
        for path in ["large", "spoof", "private", "loop"] {
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
        assert!(
            download(
                origin.join("/not-an-image-authority").unwrap().as_str(),
                Some(&origin)
            )
            .await
            .is_none()
        );
        let requests = seen.lock().unwrap();
        assert_eq!(
            requests.len(),
            7,
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

#[cfg(test)]
mod capacity_benchmark {
    use super::*;
    use desegni_core::*;
    use serde_json::{Value, json};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    #[tokio::test]
    async fn eight_image_capacity_reduces_native_placeholders_without_expanding_legacy_authority() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let observed = observed.clone();
                tokio::spawn(async move {
                    let mut request = vec![];
                    loop {
                        let mut part = [0; 4096];
                        let n = socket.read(&mut part).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&part[..n]);
                        if request.windows(4).any(|part| part == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
                    assert!(request.starts_with("get /images/"));
                    assert!(
                        !request.contains("authorization:")
                            && !request.contains("cookie:")
                            && !request.contains("referer:")
                    );
                    observed.fetch_add(1, Ordering::Relaxed);
                    let png=STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGOwaQoAIgYIBQAdRgQ5DejjuQAAAABJRU5ErkJggg==").unwrap();
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",png.len()).as_bytes()).await.unwrap();
                    socket.write_all(&png).await.unwrap();
                });
            }
        });
        let fixture: Value = serde_json::from_str(include_str!(
            "../../desegni-core/tests/fixtures/requirements8.json"
        ))
        .unwrap();
        let mut project: Project = serde_json::from_value(fixture["project"].clone()).unwrap();
        project.schema_version = desegni_core::SCHEMA_VERSION;
        let legacy: RequirementsSnapshot =
            serde_json::from_value(fixture["requirements"].clone()).unwrap();
        let nine: Value = serde_json::from_str(include_str!(
            "../../desegni-core/tests/fixtures/requirements9.json"
        ))
        .unwrap();
        let nine: RequirementsSnapshot =
            serde_json::from_value(nine["requirements"].clone()).unwrap();
        let current = project.prepare_requirements().unwrap();
        let client = OpenAiClient::for_loopback(&format!("{origin}/v1")).unwrap();
        let mut measurements = vec![];
        for snapshot in [legacy, nine, current] {
            let before = count.load(Ordering::Relaxed);
            let id = project.pages[0].regions[0].id;
            let candidate=GenerationCandidate {job_id:snapshot.job_id,requirements_sha256:snapshot.sha256.clone(),summary:"Eight requested images.".into(),regions:vec![RegionOutcome{region_id:id,status:RegionStatus::Implemented,explanation:"Placed the requested images.".into()}],pages:vec![GeneratedPage{page_id:project.home_page_id,content:SiteContent{html:format!("<main data-desegnos-region='{id}'>{}</main>",(0..8).map(|index|format!("<img data-desegnos-download src='{origin}/images/photo-{index}.png' alt='Reference image {index}'>")).collect::<String>()),css:String::new(),javascript:String::new()}}],component_requests:vec![],decision_proposals:vec![],criticism:vec![]};
            candidate.validate(&snapshot, &project).unwrap();
            let response = ProviderResponse {
                text: serde_json::to_string(&candidate).unwrap(),
                response_id: "resp_capacity_benchmark".into(),
                usage: None,
            };
            let response = client
                .download_images(response, &snapshot, &project, &CancellationToken::new())
                .await;
            let result = GenerationCandidate::from_json(&response.text).unwrap();
            result.validate(&snapshot, &project).unwrap();
            let metrics = result.pages[0].content.placeholder_metrics().unwrap();
            measurements.push(json!({"requirementsFormat":snapshot.format_version().unwrap(),"downloadedImages":count.load(Ordering::Relaxed)-before,"placeholdersPerPage":[metrics.total]}));
        }
        task.abort();
        assert_eq!(measurements[0]["downloadedImages"], 4);
        assert_eq!(measurements[0]["placeholdersPerPage"], json!([4]));
        assert_eq!(measurements[1]["downloadedImages"], 8);
        assert_eq!(measurements[1]["placeholdersPerPage"], json!([0]));
        assert_eq!(measurements[2]["downloadedImages"], 8);
        assert_eq!(measurements[2]["placeholdersPerPage"], json!([0]));
        println!(
            "Image capacity benchmark: {}",
            json!({"generationRequests":0,"sameImageSet":true,"measurements":measurements})
        );
    }
}
