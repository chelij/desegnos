//! Bounded Image API fulfilment, gated by frozen account capabilities.
use crate::{
    ApiKey, CancellationToken, ErrorKind, OpenAiClient, ProviderError, Result, SubmissionOutcome,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use desegnos_core::limits::MAX_DOWNLOADED_IMAGE_BYTES;
use image::{ImageFormat, ImageReader, Limits};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Cursor};

const MAX_IMAGE_REPLY: usize = 12 * 1024 * 1024;

pub(crate) const IMAGE_MODEL: &str = "gpt-image-2.5-flare";
const IMAGE_EDGE: u32 = 1024;

fn image_body(prompt: &str) -> Value {
    json!({"model": IMAGE_MODEL, "prompt": prompt,
        "size": format!("{IMAGE_EDGE}x{IMAGE_EDGE}"), "n": 1, "quality": "low", "output_format": "png",
        "background": "opaque"})
}

fn invalid_image() -> ProviderError {
    ProviderError::new(ErrorKind::Malformed, SubmissionOutcome::Unknown)
}

fn generated_png(response: &Value) -> Result<String> {
    let data = response["data"].as_array().ok_or_else(invalid_image)?;
    if data.len() != 1 {
        return Err(invalid_image());
    }
    let encoded = data[0]["b64_json"].as_str().ok_or_else(invalid_image)?;
    if encoded.len() > MAX_IMAGE_REPLY {
        return Err(invalid_image());
    }
    let bytes = STANDARD.decode(encoded).map_err(|_| invalid_image())?;
    if image::guess_format(&bytes).map_err(|_| invalid_image())? != ImageFormat::Png {
        return Err(invalid_image());
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(IMAGE_EDGE);
    limits.max_image_height = Some(IMAGE_EDGE);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let mut picture = reader.decode().map_err(|_| invalid_image())?;
    if picture.width() != IMAGE_EDGE || picture.height() != IMAGE_EDGE {
        return Err(invalid_image());
    }
    loop {
        let mut png = Cursor::new(Vec::new());
        picture
            .write_to(&mut png, ImageFormat::Png)
            .map_err(|_| invalid_image())?;
        if png.get_ref().len() <= MAX_DOWNLOADED_IMAGE_BYTES {
            return Ok(format!(
                "data:image/png;base64,{}",
                STANDARD.encode(png.get_ref())
            ));
        }
        if picture.width() <= 128 {
            return Err(invalid_image());
        }
        let edge = picture.width() * 3 / 4;
        picture = picture.resize(edge, edge, image::imageops::FilterType::Lanczos3);
    }
}

impl OpenAiClient {
    /// Generate one bounded PNG per requested picture, keyed by the caller's image key.
    pub async fn generate_requested_images(
        &self,
        requests: &[(String, String)],
        key: &ApiKey,
        cancellation: &CancellationToken,
    ) -> BTreeMap<String, Result<String>> {
        let mut generated = BTreeMap::new();
        for (key_name, prompt) in requests {
            let result = async {
                if cancellation.is_cancelled() { return Err(ProviderError::before(ErrorKind::Cancelled)); }
                let bytes = serde_json::to_vec(&image_body(prompt)).map_err(|_| ProviderError::before(ErrorKind::InvalidRequirements))?;
                let request_builder = self.image_api_request("images/generations", key, "application/json", bytes)?;
                let deadline = tokio::time::Instant::now() + crate::net::IMAGE_DEADLINE;
                let mut response = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Err(ProviderError::before(ErrorKind::Cancelled)),
                    _ = tokio::time::sleep_until(deadline) => return Err(ProviderError::new(ErrorKind::Deadline, SubmissionOutcome::Unknown)),
                    result = request_builder.send() => result.map_err(|_| ProviderError::new(ErrorKind::Transport, SubmissionOutcome::Unknown))?,
                };
                if !response.status().is_success() {
                    return Err(crate::providers::http_error(response.status().as_u16()));
                }
                let bytes = crate::net::bounded_body(&mut response, MAX_IMAGE_REPLY, cancellation, deadline).await?;
                let value = serde_json::from_slice(&bytes).map_err(|_| invalid_image())?;
                generated_png(&value)
            }.await;
            generated.insert(key_name.to_lowercase(), result);
        }
        generated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_reply_requires_completed_bounded_png_and_preserves_alpha() {
        assert!(generated_png(&json!({"status":"failed"})).is_err());
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
        assert!(generated_png(&invalid).is_err());
    }
}
