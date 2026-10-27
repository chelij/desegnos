//! Bounded Image API fulfilment, gated by frozen account capabilities.
use crate::{ApiKey, CancellationToken, OpenAiClient};
use base64::{Engine, engine::general_purpose::STANDARD};
use desegnos_core::{
    ImageGenerationRequest, MAX_DOWNLOADED_IMAGE_BYTES,
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
#[cfg(test)]
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
    /// Generate one bounded PNG per requested picture, keyed by the caller's image key.
    /// Failures are skipped so the apply step can place a Box placeholder instead.
    pub async fn generate_requested_images(
        &self,
        requests: &[(String, String)],
        key: &ApiKey,
        cancellation: &CancellationToken,
    ) -> BTreeMap<String, String> {
        let mut generated = BTreeMap::new();
        for (key_name, prompt) in requests {
            if cancellation.is_cancelled() {
                break;
            }
            let request = ImageGenerationRequest {
                prompt: prompt.clone(),
                source: None,
                mask: None,
                size: "1024x1024".into(),
                transparent: false,
            };
            let body = image_body(&request, false);
            let Ok(bytes) = serde_json::to_vec(&body) else {
                continue;
            };
            let Ok(request_builder) =
                self.image_api_request("images/generations", key, "application/json", bytes)
            else {
                continue;
            };
            let response = tokio::select! {
                biased;
                _ = cancellation.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(180)) => continue,
                result = request_builder.send() => result,
            };
            let Ok(mut response) = response else {
                continue;
            };
            if !response.status().is_success() {
                continue;
            }
            let Some(value) = crate::image_access::bounded_json(&mut response, MAX_IMAGE_REPLY).await
            else {
                continue;
            };
            if let Some(uri) = generated_png(&value) {
                generated.insert(key_name.to_lowercase(), uri);
            }
        }
        generated
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
