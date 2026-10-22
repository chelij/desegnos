//! Free account-specific discovery using a deliberately invalid Image API size.
use crate::{ApiKey, AuthenticationMethod, OpenAiClient, ProviderSettings};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn image_access_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageAvailability {
    Yes,
    No,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageAccess {
    pub available: ImageAvailability,
    pub checked_at: u64,
    pub reason: String,
}
impl ImageAccess {
    pub fn new(available: ImageAvailability, reason: &str) -> Self {
        Self {
            available,
            checked_at: image_access_now(),
            reason: reason.into(),
        }
    }
    pub fn expired(&self, now: u64) -> bool {
        // Transient failures are retried later without a polling/request loop.
        let ttl = if self.available == ImageAvailability::Unknown {
            300
        } else {
            7 * 24 * 60 * 60
        };
        now < self.checked_at || now.saturating_sub(self.checked_at) >= ttl
    }
}

/// Access denials only; transient failures must never disable a credential.
pub(crate) fn image_api_rejected(status: u16, body: &Value) -> bool {
    if status == 429 || status >= 500 {
        return false;
    }
    if matches!(status, 401 | 403) {
        return true;
    }
    if !(400..500).contains(&status) {
        return false;
    }
    let description = body.to_string().to_ascii_lowercase();
    if description.contains("organization")
        && (description.contains("verif") || description.contains("enrolled"))
    {
        return true;
    }
    [
        "subscription",
        "sharing",
        "not allowed",
        "not authorized",
        "permission denied",
        "organization verification",
        "organization must be verified",
        "verify your organization",
        "organization must be enrolled",
        "unsupported_tool",
        "unsupported tool",
        "tools are unavailable",
        "image_generation is unavailable",
        "image_generation is unsupported",
        "not available for your plan",
    ]
    .iter()
    .any(|part| description.contains(part))
}

/// Main connection always wins when both credentials have confirmed access.
/// Unknown access is never permission for a paid call.
pub fn use_image_key(main: Option<&ImageAccess>, image_key: Option<&ImageAccess>) -> bool {
    let usable = |access: Option<&ImageAccess>| {
        access.is_some_and(|a| {
            a.available == ImageAvailability::Yes && !a.expired(image_access_now())
        })
    };
    !usable(main) && usable(image_key)
}

pub(crate) fn image_access_error(status: u16, body: &Value, key: &ApiKey) -> ImageAccess {
    let available = if image_api_rejected(status, body) {
        ImageAvailability::No
    } else if status == 400 {
        let error = body.get("error").unwrap_or(body);
        let message = error["message"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if error["param"] == "size" || (message.contains("size") && message.contains("16x16")) {
            ImageAvailability::Yes
        } else {
            ImageAvailability::Unknown
        }
    } else {
        ImageAvailability::Unknown
    };
    let error = body.get("error").unwrap_or(body);
    let detail = [
        error["code"].as_str().or_else(|| error["type"].as_str()),
        error["message"].as_str(),
        error["param"].as_str(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(":");
    // Scrub the full value before bounding it, including the exact active credential.
    let scrubbed = crate::scrub_diagnostic_reply(&detail, Some(key));
    let scrubbed = scrubbed
        .split_inclusive(|c: char| !(c.is_ascii_alphanumeric() || "_-.+/~=".contains(c)))
        .map(|word| {
            // Also hide opaque credentials with no recognizable prefix or JWT shape.
            if word.chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 32 {
                "[redacted]".to_owned()
            } else {
                word.to_owned()
            }
        })
        .collect::<String>();
    let detail = scrubbed.split_whitespace().collect::<Vec<_>>().join(" ");
    let organization = body.to_string().to_ascii_lowercase();
    let prefix = if organization.contains("organization")
        && (organization.contains("verif") || organization.contains("enrolled"))
    {
        "API Organization Verification required: "
    } else {
        ""
    };
    let reason = format!("{prefix}http_{status}:{detail}")
        .chars()
        .take(200)
        .collect::<String>();
    ImageAccess::new(available, &reason)
}

pub(crate) async fn bounded_json(response: &mut reqwest::Response, limit: usize) -> Option<Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > limit {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).ok()
}

impl OpenAiClient {
    pub async fn probe_image_access(
        &self,
        auth: AuthenticationMethod,
        settings: &ProviderSettings,
        key: &ApiKey,
    ) -> ImageAccess {
        let access = tokio::time::timeout(
            Duration::from_secs(25),
            self.probe_images(auth, settings, key),
        )
        .await
        .unwrap_or_else(|_| ImageAccess::new(ImageAvailability::Unknown, "network_or_timeout"));
        eprintln!(
            "Image access probe ({auth:?}): {:?}: {}",
            access.available, access.reason
        );
        access
    }
    async fn probe_images(
        &self,
        _auth: AuthenticationMethod,
        _settings: &ProviderSettings,
        key: &ApiKey,
    ) -> ImageAccess {
        // 16x16 is invalid for every supported model. Do not retry with a valid size:
        // validation proves access without ever authorizing image generation.
        let body = json!({"model": crate::image_generation::IMAGE_MODELS[0], "prompt": "Access check",
            "size": "16x16", "n": 1, "quality": "low", "output_format": "png"});
        let request = serde_json::to_vec(&body).ok().and_then(|bytes| {
            self.image_api_request("images/generations", key, "application/json", bytes)
                .ok()
        });
        let Some(request) = request else {
            return ImageAccess::new(ImageAvailability::Unknown, "invalid_probe_request");
        };
        let Ok(mut response) = request.send().await else {
            return ImageAccess::new(ImageAvailability::Unknown, "network_or_timeout");
        };
        let status = response.status().as_u16();
        let value = bounded_json(&mut response, 65_536)
            .await
            .unwrap_or(Value::Null);
        image_access_error(status, &value, key)
    }
}
