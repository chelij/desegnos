//! Bounded Image API fulfilment, gated by frozen account capabilities.
use crate::{
    ApiKey, AuthenticationMethod, CancellationToken, OpenAiClient, ProviderResponse,
    ProviderSettings,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use desegni_core::{
    GenerationCandidate, ImageGenerationRequest, MAX_DOWNLOADED_IMAGE_BYTES, Project,
    RequirementsSnapshot,
};
use image::{ImageFormat, ImageReader, Limits};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Cursor, time::Duration};

const MAX_IMAGE_REPLY: usize = 12 * 1024 * 1024;

/// Generation and editing defaults; update model names here when the API changes.
pub const IMAGE_MODELS: [&str; 2] = ["gpt-image-2.5-flare", "gpt-image-2.5-sunburst"];

fn image_body(request: &ImageGenerationRequest, edit: bool) -> Value {
    json!({"model": IMAGE_MODELS[usize::from(edit)], "prompt": request.prompt,
        "size": request.size, "n": 1, "quality": "low", "output_format": "png",
        "background": if request.transparent { "transparent" } else { "opaque" }})
}

// Only fixed field names and native-resolved data URIs enter this multipart body.
fn edit_body(
    request: &ImageGenerationRequest,
    reference: &str,
    mask: Option<&str>,
) -> Option<(String, Vec<u8>)> {
    let boundary = format!("desegnos-{}", uuid::Uuid::new_v4());
    let mut bytes = Vec::new();
    for (name, value) in image_body(request, true).as_object()? {
        let value = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        bytes.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    for (name, uri) in [("image", Some(reference)), ("mask", mask)] {
        let Some(uri) = uri else { continue };
        let (header, encoded) = uri.split_once(',')?;
        let (mime, extension) = match header {
            "data:image/png;base64" => ("image/png", "png"),
            "data:image/jpeg;base64" if name == "image" => ("image/jpeg", "jpg"),
            "data:image/webp;base64" if name == "image" => ("image/webp", "webp"),
            _ => return None,
        };
        if encoded.len() > crate::MAX_REQUEST_BYTES {
            return None;
        }
        let data = STANDARD.decode(encoded).ok()?;
        bytes.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{name}.{extension}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes());
        bytes.extend_from_slice(&data);
        bytes.extend_from_slice(b"\r\n");
        if bytes.len() > crate::MAX_REQUEST_BYTES {
            return None;
        }
    }
    bytes.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (bytes.len() <= crate::MAX_REQUEST_BYTES)
        .then(|| (format!("multipart/form-data; boundary={boundary}"), bytes))
}

/// Decode with allocation/dimension limits, then keep a bounded PNG with its alpha channel.
fn generated_png(response: &Value) -> Option<String> {
    let data = response["data"].as_array()?;
    if data.len() != 1 {
        return None;
    }
    let encoded = data[0]["b64_json"].as_str()?;
    if encoded.len() > MAX_IMAGE_REPLY {
        return None;
    }
    let bytes = STANDARD.decode(encoded).ok()?;
    if image::guess_format(&bytes).ok()? != ImageFormat::Png {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let mut picture = reader.decode().ok()?;
    if picture.width() != 1024 || picture.height() != 1024 {
        return None;
    }
    loop {
        let mut png = Cursor::new(Vec::new());
        picture.write_to(&mut png, ImageFormat::Png).ok()?;
        if png.get_ref().len() <= MAX_DOWNLOADED_IMAGE_BYTES {
            return Some(format!(
                "data:image/png;base64,{}",
                STANDARD.encode(png.get_ref())
            ));
        }
        if picture.width() <= 128 {
            return None;
        }
        let edge = picture.width() * 3 / 4;
        picture = picture.resize(edge, edge, image::imageops::FilterType::Lanczos3);
    }
}

impl OpenAiClient {
    async fn generate_image(
        &self,
        request: &ImageGenerationRequest,
        reference: Option<&str>,
        mask: Option<&str>,
        key: &ApiKey,
    ) -> std::result::Result<Option<String>, ()> {
        let (path, content_type, bytes) = if let Some(reference) = reference {
            let Some((content_type, bytes)) = edit_body(request, reference, mask) else {
                return Ok(None);
            };
            ("images/edits", content_type, bytes)
        } else {
            let Ok(bytes) = serde_json::to_vec(&image_body(request, false)) else {
                return Ok(None);
            };
            ("images/generations", "application/json".into(), bytes)
        };
        if bytes.len() > crate::MAX_REQUEST_BYTES {
            return Ok(None);
        }
        let Ok(builder) = self.image_api_request(path, key, &content_type, bytes) else {
            return Ok(None);
        };
        let Ok(mut response) = builder.send().await else {
            return Ok(None);
        };
        let status = response.status().as_u16();
        let value = crate::image_access::bounded_json(&mut response, MAX_IMAGE_REPLY)
            .await
            .unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            return if crate::image_access::image_api_rejected(status, &value) {
                Err(())
            } else {
                Ok(None)
            };
        }
        Ok(generated_png(&value))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn generate_images(
        &self,
        response: ProviderResponse,
        auth: AuthenticationMethod,
        settings: &ProviderSettings,
        snapshot: &RequirementsSnapshot,
        project: &Project,
        key: &ApiKey,
        cancellation: &CancellationToken,
    ) -> ProviderResponse {
        self.generate_images_with_status(
            response,
            auth,
            settings,
            snapshot,
            project,
            key,
            cancellation,
        )
        .await
        .0
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn generate_images_with_status(
        &self,
        mut response: ProviderResponse,
        auth: AuthenticationMethod,
        settings: &ProviderSettings,
        snapshot: &RequirementsSnapshot,
        project: &Project,
        key: &ApiKey,
        cancellation: &CancellationToken,
    ) -> (ProviderResponse, bool) {
        if cancellation.is_cancelled()
            || !settings.image_generation
            || snapshot.format_version().unwrap_or(0) < 17
        {
            return (response, false);
        }
        let Some(capability) = snapshot
            .capabilities
            .as_ref()
            .and_then(|c| c.image_generation.as_ref())
        else {
            return (response, false);
        };
        let Ok(mut candidate) = GenerationCandidate::from_reply(&response.text, snapshot, project)
        else {
            return (response, false);
        };
        if candidate.validate(snapshot, project).is_err() {
            return (response, false);
        }
        let Ok(requests) = candidate.image_generation_requests() else {
            return (response, false);
        };
        if requests.is_empty() {
            return (response, false);
        }
        let budget_ok = crate::estimate_for_auth(
            auth,
            snapshot,
            project.generation_mode,
            &desegni_core::result_schema(snapshot),
            settings,
        )
        .is_ok();
        let mut images = BTreeMap::new();
        let mut unavailable = false;
        for request in requests
            .into_iter()
            .take(usize::from(capability.max_images))
        {
            if cancellation.is_cancelled() {
                return (response, false);
            }
            if !budget_ok {
                break;
            }
            let Ok(reference) = request.reference_image(snapshot, project) else {
                continue;
            };
            let Ok(mask) = request.mask_image(snapshot, project) else {
                continue;
            };
            let result = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return (response, false),
                result = tokio::time::timeout(Duration::from_secs(180), self.generate_image(&request, reference.as_deref(), mask.as_deref(), key)) => result.ok(),
            };
            match result {
                Some(Ok(Some(image))) => {
                    images.insert(request, image);
                }
                Some(Err(())) => {
                    unavailable = true;
                    images.clear();
                    break;
                }
                _ => {}
            }
        }
        if cancellation.is_cancelled() {
            return (response, unavailable);
        }
        // If insertion would exceed a source budget, fall back to placeholders for this request.
        let original = candidate.clone();
        if candidate.embed_generated_images(&images).is_err()
            || candidate.validate(snapshot, project).is_err()
        {
            candidate = original;
            if candidate.embed_generated_images(&BTreeMap::new()).is_err() {
                return (response, unavailable);
            }
        }
        if candidate.validate(snapshot, project).is_ok()
            && let Ok(text) = serde_json::to_string(&candidate)
        {
            response.text = text;
        }
        (response, unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_and_multipart_edits_choose_models_and_preserve_reference_and_mask() {
        let request = ImageGenerationRequest {
            prompt: "Cut out this picture".into(),
            source: Some("attachment1".into()),
            mask: Some("mask1".into()),
            size: "1024x1024".into(),
            transparent: true,
        };
        let body = image_body(&request, false);
        assert_eq!(body["model"], IMAGE_MODELS[0]);
        assert_eq!(body["size"], "1024x1024");
        assert_eq!(body["quality"], "low");
        assert_eq!(body["n"], 1);
        assert_eq!(body["background"], "transparent");
        assert_eq!(body["output_format"], "png");
        assert!(body.get("tools").is_none());
        let reference = format!("data:image/png;base64,{}", STANDARD.encode(b"source bytes"));
        let mask = format!(
            "data:image/png;base64,{}",
            STANDARD.encode(b"alpha mask bytes")
        );
        let (content_type, body) = edit_body(&request, &reference, Some(&mask)).unwrap();
        let boundary = content_type
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        let body = String::from_utf8(body).unwrap();
        assert!(body.contains(&format!("name=\"model\"\r\n\r\n{}", IMAGE_MODELS[1])));
        assert!(body.contains("name=\"image\"; filename=\"image.png\"\r\nContent-Type: image/png\r\n\r\nsource bytes\r\n"));
        assert!(body.contains("name=\"mask\"; filename=\"mask.png\"\r\nContent-Type: image/png\r\n\r\nalpha mask bytes\r\n"));
        assert!(body.ends_with(&format!("--{boundary}--\r\n")));
        assert!(edit_body(&request, "https://example.com/image.png", None).is_none());
        assert!(edit_body(&request, &reference, Some("data:image/jpeg;base64,YQ==")).is_none());
    }
    #[test]
    fn image_reply_requires_completed_bounded_png_and_preserves_alpha() {
        assert!(generated_png(&json!({"status":"failed"})).is_none());
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1024, 1024)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        let value = json!({"data":[{"b64_json":STANDARD.encode(png.get_ref())}]});
        let uri = generated_png(&value).unwrap();
        let decoded = STANDARD.decode(uri.split_once(',').unwrap().1).unwrap();
        assert!(decoded.len() <= MAX_DOWNLOADED_IMAGE_BYTES);
        assert!(
            image::load_from_memory(&decoded)
                .unwrap()
                .color()
                .has_alpha()
        );
        let mut invalid = value;
        invalid["data"][0]["b64_json"] = json!(STANDARD.encode(b"<svg/>"));
        assert!(generated_png(&invalid).is_none());
    }
}

#[cfg(test)]
pub(crate) mod adapter_tests {
    use super::*;
    use crate::ProviderId;
    use desegni_core::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    pub(crate) fn fixture() -> (Project, RequirementsSnapshot, ProviderResponse) {
        let value: Value = serde_json::from_str(include_str!(
            "../../desegni-core/tests/fixtures/requirements8.json"
        ))
        .unwrap();
        let mut project: Project = serde_json::from_value(value["project"].clone()).unwrap();
        project.schema_version = SCHEMA_VERSION;
        project.pages[0].content = None;
        let snapshot = project
            .prepare_requirements_with_capabilities(GenerationCapabilities {
                image_generation: Some(ImageGenerationCapability::default()),
                ..Default::default()
            })
            .unwrap();
        let id = project.pages[0].regions[0].id;
        let candidate=GenerationCandidate{
            job_id:snapshot.job_id,requirements_sha256:snapshot.sha256.clone(),summary:"Three requested pictures".into(),
            regions:vec![RegionOutcome{region_id:id,status:RegionStatus::Implemented,explanation:"Native image requests.".into()}],
            pages:vec![GeneratedPage{page_id:project.home_page_id,content:SiteContent {
                html:format!("<section data-desegnos-region='{id}'><p>Keep this copy</p>{}</section>",["A","B","C"].map(|prompt|format!("<img data-desegnos-generate='{prompt}' src='#' alt='Picture {prompt}'>")).join("")),
                css:String::new(),javascript:String::new()
            }}],documents:vec![],component_requests:vec![],decision_proposals:vec![],criticism:vec![]
        };
        candidate.validate(&snapshot, &project).unwrap();
        let response = ProviderResponse {
            text: serde_json::to_string(&candidate).unwrap(),
            response_id: "resp_main".into(),
            usage: None,
        };
        (project, snapshot, response)
    }

    #[tokio::test]
    async fn bounded_native_calls_keep_success_and_failures_without_blocking() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1024, 1024)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        let generated = json!({"data":[{"b64_json":STANDARD.encode(png.get_ref())}]}).to_string();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let (header_end, length) = loop {
                    let mut part = [0; 4096];
                    let n = socket.read(&mut part).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&part[..n]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                        assert!(headers.starts_with("post /v1/images/generations "));
                        assert!(headers.contains("authorization: bearer "));
                        let length: usize = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while request.len() < header_end + length {
                    let mut part = [0; 4096];
                    let n = socket.read(&mut part).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&part[..n]);
                }
                let body: Value =
                    serde_json::from_slice(&request[header_end..header_end + length]).unwrap();
                assert_eq!(body["model"], IMAGE_MODELS[0]);
                assert!(body.get("tools").is_none());
                let ordinal = seen.fetch_add(1, Ordering::SeqCst);
                let (status, body) = if ordinal == 0 {
                    (200, generated.as_str())
                } else {
                    (500, "{}")
                };
                socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
        });
        let client = OpenAiClient::for_loopback(&endpoint).unwrap();
        let key = ApiKey::new("sk-test-image-generation".into()).unwrap();
        let settings = ProviderSettings {
            spending_limit_usd: 10000.,
            ..Default::default()
        };
        let (project, snapshot, response) = fixture();
        let resolved = client
            .generate_images(
                response.clone(),
                AuthenticationMethod::ApiKey,
                &settings,
                &snapshot,
                &project,
                &key,
                &CancellationToken::new(),
            )
            .await;
        assert_eq!(
            count.load(Ordering::SeqCst),
            2,
            "excess image placements cannot authorize extra spending"
        );
        let candidate = GenerationCandidate::from_json(&resolved.text).unwrap();
        candidate.validate(&snapshot, &project).unwrap();
        assert!(candidate.pages[0].content.html.contains("Keep this copy"));
        assert!(candidate.image_generation_requests().unwrap().is_empty());
        assert_eq!(
            candidate.pages[0]
                .content
                .placeholder_metrics()
                .unwrap()
                .marked_slots,
            2
        );
        let applied = project
            .apply_generation(&snapshot, &candidate, &[])
            .unwrap();
        assert_eq!(applied.pending_count(), 0);
        assert!(
            applied.pages[0]
                .content
                .as_ref()
                .unwrap()
                .html
                .contains("desegnos-asset:")
        );
        for (auth, enabled, cancelled) in [
            (AuthenticationMethod::ApiKey, false, false),
            (AuthenticationMethod::ApiKey, true, true),
        ] {
            let cancel = CancellationToken::new();
            if cancelled {
                cancel.cancel();
            }
            let settings = ProviderSettings {
                image_generation: enabled,
                ..settings.clone()
            };
            let skipped = client
                .generate_images(
                    response.clone(),
                    auth,
                    &settings,
                    &snapshot,
                    &project,
                    &key,
                    &cancel,
                )
                .await;
            assert_eq!(skipped, response);
        }
        assert_eq!(count.load(Ordering::SeqCst), 2);
        task.abort();
    }

    #[tokio::test]
    async fn cancellation_interrupts_an_in_flight_image_call() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let cancel = CancellationToken::new();
        let server_cancel = cancel.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut part = [0; 4096];
            assert!(socket.read(&mut part).await.unwrap() > 0);
            observed.fetch_add(1, Ordering::SeqCst);
            server_cancel.cancel();
            // Keep the response pending to prove cancellation, rather than a transport error, wins.
            std::future::pending::<()>().await;
            drop(socket);
        });
        let client = OpenAiClient::for_loopback(&endpoint).unwrap();
        let key = ApiKey::new("sk-test-image-generation".into()).unwrap();
        let settings = ProviderSettings {
            spending_limit_usd: 10000.,
            ..Default::default()
        };
        let (project, snapshot, response) = fixture();
        let original = project.clone();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            client.generate_images(
                response.clone(),
                AuthenticationMethod::ApiKey,
                &settings,
                &snapshot,
                &project,
                &key,
                &cancel,
            ),
        )
        .await
        .unwrap();
        assert!(cancel.is_cancelled());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "Stop prevents subsequent image calls"
        );
        assert_eq!(project, original);
        assert_eq!(
            result, response,
            "late image output cannot become an applicable result"
        );
        task.abort();
    }

    #[test]
    fn capability_and_settings_and_spending_reserve_follow_authentication() {
        let (project, snapshot, _) = fixture();
        let settings = ProviderSettings {
            spending_limit_usd: 10000.,
            ..Default::default()
        };
        let on = ProviderId::Openai.capabilities_for(
            AuthenticationMethod::ApiKey,
            &settings,
            Some(&crate::ImageAccess::new(
                crate::ImageAvailability::Yes,
                "test",
            )),
        );
        assert!(on.image_generation.is_some());
        assert!(
            ProviderId::Openai
                .capabilities_for(AuthenticationMethod::Chatgpt, &settings, None)
                .image_generation
                .is_none()
        );
        assert!(
            ProviderId::Google
                .capabilities_for(
                    AuthenticationMethod::ApiKey,
                    &settings,
                    Some(&crate::ImageAccess::new(
                        crate::ImageAvailability::Yes,
                        "test"
                    ))
                )
                .image_generation
                .is_none()
        );
        let schema = result_schema(&snapshot);
        let enabled = crate::estimate_for_auth(
            AuthenticationMethod::ApiKey,
            &snapshot,
            project.generation_mode,
            &schema,
            &settings,
        )
        .unwrap();
        let plain = project.prepare_requirements().unwrap();
        // Check the image reserve directly; other frozen capabilities change the text token estimate.
        assert_eq!(
            crate::settings::image_generation_reserve(AuthenticationMethod::ApiKey, &snapshot),
            1.
        );
        assert_eq!(
            crate::settings::image_generation_reserve(AuthenticationMethod::Chatgpt, &snapshot),
            1.
        );
        assert_eq!(
            crate::settings::image_generation_reserve(AuthenticationMethod::ApiKey, &plain),
            0.
        );
        let limited = ProviderSettings {
            spending_limit_usd: enabled.estimate_usd - 0.01,
            ..settings.clone()
        };
        assert_eq!(
            crate::estimate_for_auth(
                AuthenticationMethod::ApiKey,
                &snapshot,
                project.generation_mode,
                &schema,
                &limited
            )
            .unwrap_err()
            .kind,
            crate::ErrorKind::SpendingLimit
        );
        let mut legacy = serde_json::to_value(settings).unwrap();
        legacy.as_object_mut().unwrap().remove("imageGeneration");
        assert!(
            serde_json::from_value::<ProviderSettings>(legacy)
                .unwrap()
                .image_generation
        );
    }
}
