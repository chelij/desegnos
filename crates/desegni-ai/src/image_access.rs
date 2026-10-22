//! Account-specific discovery. Declaring a tool with choice `none` never generates an image.
use crate::{ApiKey, AuthenticationMethod, OpenAiClient, ProviderSettings, sse::SseDecoder};
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

pub(crate) fn image_models_offered(value: &Value, auth: AuthenticationMethod) -> bool {
    value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models.iter().any(|model| {
                model
                    .get("id")
                    .or_else(|| model.get("slug"))
                    .and_then(Value::as_str)
                    .is_some_and(|id| id.starts_with("gpt-image"))
                    && (auth == AuthenticationMethod::ApiKey || model["visibility"] == "list")
            })
        })
}

/// Only explicit tool/parameter/permission rejections disable images. Auth failures need reconnect.
pub(crate) fn image_tool_rejected(status: u16, body: &Value) -> bool {
    if !(400..500).contains(&status) || matches!(status, 401 | 408 | 409 | 425 | 429) {
        return false;
    }
    let error = body.get("error").unwrap_or(body);
    // SSE errors carry a code without an HTTP status. Transient codes must still win.
    let code = error["code"].as_str().unwrap_or_default();
    if [
        "rate_limit",
        "quota",
        "usage_limit",
        "server_error",
        "internal_error",
        "invalid_api_key",
        "authentication",
        "token_expired",
    ]
    .iter()
    .any(|part| code.contains(part))
    {
        return false;
    }
    let description = ["code", "type", "param", "message"]
        .iter()
        .filter_map(|key| error[*key].as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    description.contains("image_generation")
        || description.contains("image generation")
        || description.contains("unsupported_parameter")
        || description.contains("unsupported parameter")
        || description.contains("permission")
        || description.contains("not authorized")
        || description.contains("unsupported_tool")
        || description.contains("access_denied")
}

pub(crate) fn image_access_error(status: u16, body: &Value) -> ImageAccess {
    if image_tool_rejected(status, body) {
        ImageAccess::new(ImageAvailability::No, "image_tool_rejected")
    } else {
        ImageAccess::new(
            ImageAvailability::Unknown,
            match status {
                401 => "authentication_required",
                429 => "rate_limited",
                500..=599 => "server_error",
                _ => "unconfirmed_error",
            },
        )
    }
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
        tokio::time::timeout(
            Duration::from_secs(25),
            self.probe_images(auth, settings, key),
        )
        .await
        .unwrap_or_else(|_| ImageAccess::new(ImageAvailability::Unknown, "network_or_timeout"))
    }
    async fn probe_images(
        &self,
        auth: AuthenticationMethod,
        settings: &ProviderSettings,
        key: &ApiKey,
    ) -> ImageAccess {
        // Model visibility is supporting evidence, never proof of Responses tool access.
        let offered = match self
            .client
            .get(format!("{}/models", self.endpoint))
            .bearer_auth(key.expose())
            .send()
            .await
        {
            Ok(mut response) if response.status().is_success() => {
                bounded_json(&mut response, 1024 * 1024)
                    .await
                    .map(|value| image_models_offered(&value, auth))
            }
            _ => None,
        };
        let mut access = self.probe_image_tool(settings, key).await;
        access.reason.push_str(match offered {
            Some(true) => ";image_model_listed",
            Some(false) => ";image_model_not_listed",
            None => ";models_unknown",
        });
        access
    }
    async fn probe_image_tool(&self, settings: &ProviderSettings, key: &ApiKey) -> ImageAccess {
        let body = json!({"model":settings.model,"input":"ok","tools":[{"type":"image_generation"}],
            "tool_choice":"none","store":false,"stream":true,"max_output_tokens":16});
        let Ok(mut response) = self
            .client
            .post(format!("{}/responses", self.endpoint))
            .bearer_auth(key.expose())
            .json(&body)
            .send()
            .await
        else {
            return ImageAccess::new(ImageAvailability::Unknown, "network_or_timeout");
        };
        if !response.status().is_success() {
            let status = response.status().as_u16();
            return image_access_error(
                status,
                &bounded_json(&mut response, 65_536)
                    .await
                    .unwrap_or(Value::Null),
            );
        }
        if response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/json"))
        {
            let value = bounded_json(&mut response, 65_536)
                .await
                .unwrap_or(Value::Null);
            return if value["status"] == "completed" && value["error"].is_null() {
                ImageAccess::new(ImageAvailability::Yes, "tool_accepted")
            } else {
                image_access_error(400, &value)
            };
        }
        let mut decoder = SseDecoder::default();
        let mut bytes = 0;
        while let Ok(Some(chunk)) = response.chunk().await {
            bytes += chunk.len();
            if bytes > 65_536 {
                break;
            }
            let Ok(frames) = decoder.push(&chunk) else {
                break;
            };
            for frame in frames {
                let Ok(value) = serde_json::from_str::<Value>(&frame.data) else {
                    continue;
                };
                match value["type"].as_str() {
                    Some("error" | "response.failed") => {
                        let error = value.get("response").unwrap_or(&value);
                        return image_access_error(400, error);
                    }
                    Some(
                        "response.created"
                        | "response.in_progress"
                        | "response.completed"
                        | "response.output_text.delta"
                        | "response.incomplete",
                    ) => {
                        // Dropping the response cancels the stream immediately upon acceptance.
                        return ImageAccess::new(ImageAvailability::Yes, "tool_accepted");
                    }
                    _ => {}
                }
            }
        }
        ImageAccess::new(ImageAvailability::Unknown, "unconfirmed_response")
    }
}
