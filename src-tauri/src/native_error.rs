use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(ts_rs::TS, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeErrorKind {
    Command,
    AiSettings,
    SaveLocation,
    Clipboard,
    Browser,
}

impl NativeErrorKind {
    /// The `map_err` argument that turns a command's error into a typed `NativeFailure`.
    pub fn failure<E: Serialize>(self) -> impl Fn(E) -> NativeFailure {
        move |error| NativeFailure::from_serializable(error, self)
    }
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeFailure {
    pub kind: NativeErrorKind,
    pub message: String,
    #[ts(skip)]
    #[serde(flatten)]
    details: Map<String, Value>,
}

impl NativeFailure {
    pub fn from_serializable(error: impl Serialize, kind: NativeErrorKind) -> Self {
        let value = serde_json::to_value(error).unwrap_or(Value::Null);
        let (message, mut details) = match value {
            Value::String(message) => (message, Map::new()),
            Value::Object(mut fields) => {
                let message = fields
                    .remove("message")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "The operation could not finish. Try again.".into());
                (message, fields)
            }
            _ => (
                "The operation could not finish. Try again.".into(),
                Map::new(),
            ),
        };
        let kind = details
            .get("kind")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or(kind);
        // Preserve recovery codes, provider errors and browser URLs alongside the native kind.
        details.remove("kind");
        Self {
            kind,
            message,
            details,
        }
    }
}

impl std::fmt::Display for NativeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<NativeFailure> for String {
    fn from(error: NativeFailure) -> Self {
        error.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_failures_keep_kind_and_recovery_or_provider_details() {
        let original = serde_json::json!({
            "kind": "aiSettings", "message": "Wording can change freely.",
            "code": "pendingRecovery", "projectId": "project", "url": "http://localhost:1234",
            "providerError": { "kind": "authentication" }
        });
        let failure = NativeFailure::from_serializable(original.clone(), NativeErrorKind::Command);
        assert_eq!(serde_json::to_value(failure).unwrap(), original);
    }

    #[test]
    fn command_context_types_string_errors_without_reading_their_message() {
        for message in [
            "permission denied",
            "The wording no longer mentions permissions.",
        ] {
            let failure = NativeFailure::from_serializable(message, NativeErrorKind::SaveLocation);
            let value = serde_json::to_value(failure).unwrap();
            assert_eq!(value["kind"], "saveLocation");
            assert_eq!(value["message"], message);
        }
    }
}
