use crate::{ApiKey, AuthenticationMethod, TokenUsage};
use serde::{Deserialize, Serialize};

#[derive(ts_rs::TS, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    InvalidSettings,
    InvalidRequirements,
    MissingCredential,
    InvalidCredential,
    VaultUnavailable,
    ContextLimit,
    SpendingLimit,
    InputLimit,
    OutputLimit,
    Repetition,
    Authentication,
    Quota,
    RateLimited,
    ModelUnavailable,
    RequestRejected,
    Redirect,
    Refused,
    Incomplete,
    Truncated,
    Malformed,
    UnexpectedResponseFormat,
    Cancelled,
    Deadline,
    Transport,
    Server,
    ChatgptNotEligible,
    ChatgptUsageLimit,
    ChatgptUsageUnavailable,
    ChatgptUnsupportedCapability,
    ChatgptRouteNotSupported,
    ChatgptInvalidUser,
    ChatgptPermission,
    ChatgptUserUnavailable,
}

impl ErrorKind {
    /// Failures the user fixes in AI settings rather than by retrying.
    pub const NEEDS_AI_SETTINGS: [ErrorKind; 11] = [
        Self::Authentication,
        Self::InvalidCredential,
        Self::MissingCredential,
        Self::Quota,
        Self::ModelUnavailable,
        Self::InvalidSettings,
        Self::VaultUnavailable,
        Self::SpendingLimit,
        Self::ChatgptUsageLimit,
        Self::ChatgptPermission,
        Self::ChatgptInvalidUser,
    ];

    pub fn needs_ai_settings(self) -> bool {
        Self::NEEDS_AI_SETTINGS.contains(&self)
    }
}

#[derive(ts_rs::TS, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubmissionOutcome {
    NotSubmitted,
    Rejected,
    Unknown,
    Received,
}

#[derive(ts_rs::TS, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseBodyKind {
    Unavailable,
    EventStream,
    Empty,
    JsonError,
    JsonDetail,
    JsonResponse,
    JsonOther,
    Html,
    Text,
    Binary,
}

#[derive(ts_rs::TS, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseRejection {
    EventJson,
    EventType,
    EventName,
    ResponseIdentity,
    TextLocation,
    TextIdentity,
    TextOrder,
    TextMismatch,
    OutputShape,
    MessageRole,
    MessageState,
    MessagePhase,
    ContentShape,
    AnswerCount,
    CredentialEcho,
    CandidateJson,
    CandidateShape,
    JobMismatch,
    RequirementsMismatch,
    Usage,
}

impl ResponseRejection {
    fn message(self) -> &'static str {
        match self {
            Self::CandidateJson | Self::CandidateShape => {
                "The AI's final answer was not in Desegnos's required site format. Nothing was applied."
            }
            Self::JobMismatch => {
                "The AI returned a result for a different request. Nothing was applied."
            }
            Self::RequirementsMismatch => {
                "The AI returned a result for different requirements. Nothing was applied."
            }
            Self::AnswerCount => {
                "The AI did not return one clear final answer. Nothing was applied."
            }
            Self::TextMismatch | Self::TextIdentity | Self::TextLocation | Self::TextOrder => {
                "The AI's streamed reply did not match its completed answer. Nothing was applied."
            }
            Self::CredentialEcho => {
                "The AI reply contained a credential and was rejected. Nothing was applied."
            }
            _ => {
                "Desegnos could not verify the AI reply. Open Details for the failed check. Nothing was applied."
            }
        }
    }
}

#[derive(ts_rs::TS, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalOutput {
    NotReceived,
    Absent,
    Null,
    Array,
    Invalid,
}

#[derive(ts_rs::TS, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CorrelationCause {
    InvalidIndex,
    MissingIdentity,
    ItemAbsent,
    ItemDuplicate,
    IdentityChanged,
    ContentPartAbsent,
    UnfinishedItem,
    ItemChanged,
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseCorrelation {
    pub terminal_output: TerminalOutput,
    pub terminal_items: u32,
    pub terminal_text_parts: u32,
    pub streamed_parts: u32,
    pub finalized_items: u32,
    pub empty_added_items: u32,
    pub cause: CorrelationCause,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_index: Option<u64>,
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseFormat {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_reply: Option<Box<str>>,
    pub content_type: Option<Box<str>>,
    pub body_kind: ResponseBodyKind,
    pub body_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_message: Option<Box<str>>,
    pub body_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection: Option<ResponseRejection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<Box<ResponseCorrelation>>,
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderError {
    pub kind: ErrorKind,
    pub outcome: SubmissionOutcome,
    pub response_id: Option<String>,
    pub usage: Option<TokenUsage>,
    pub authentication: AuthenticationMethod,
    pub http_status: Option<u16>,
    pub code: Option<Box<str>>,
    pub param: Option<Box<str>>,
    pub request_id: Option<Box<str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Box<ResponseFormat>>,
}

impl ProviderError {
    pub fn new(kind: ErrorKind, outcome: SubmissionOutcome) -> Self {
        Self {
            kind,
            outcome,
            response_id: None,
            usage: None,
            authentication: AuthenticationMethod::ApiKey,
            http_status: None,
            code: None,
            param: None,
            request_id: None,
            response_format: None,
        }
    }

    pub(crate) fn with_diagnostic_reply(mut self, text: &str, key: &ApiKey) -> Self {
        if let Some(format) = &mut self.response_format {
            format.diagnostic_reply = Some(if key.appears_in(text) {
                "Reply body omitted because it contained a credential.".into()
            } else {
                scrub_diagnostic_reply(text, Some(key)).into_boxed_str()
            });
        }
        self
    }

    pub(crate) fn before(kind: ErrorKind) -> Self {
        Self::new(kind, SubmissionOutcome::NotSubmitted)
    }

    pub fn with_authentication(mut self, authentication: AuthenticationMethod) -> Self {
        self.authentication = authentication;
        self
    }

    pub(crate) fn with_provider_metadata(
        mut self,
        key: &ApiKey,
        http_status: Option<u16>,
        code: Option<&str>,
        param: Option<&str>,
        request_id: Option<&str>,
    ) -> Self {
        self.http_status = http_status;
        self.code = safe_identifier(key, code, 128, false);
        self.param = safe_identifier(key, param, 256, true);
        self.request_id = safe_identifier(key, request_id, 128, false);
        self
    }

    pub(crate) fn with_response(
        mut self,
        response_id: Option<String>,
        usage: Option<TokenUsage>,
    ) -> Self {
        self.response_id = response_id;
        self.usage = usage;
        self
    }

    pub(crate) fn with_response_format(
        mut self,
        key: &ApiKey,
        content_type: Option<&str>,
        body_kind: ResponseBodyKind,
        body_bytes: usize,
        provider_message: Option<&str>,
    ) -> Self {
        let content_type = content_type
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 128
                    && !key.appears_in(value)
                    && value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric()
                            || matches!(byte, b'/' | b'.' | b'+' | b'-' | b'_')
                    })
            })
            .map(|value| value.to_ascii_lowercase().into_boxed_str());
        self.response_format = Some(Box::new(ResponseFormat {
            diagnostic_reply: None,
            content_type,
            body_kind,
            body_bytes,
            provider_message: safe_provider_message(key, provider_message),
            body_truncated: false,
            limit_bytes: None,
            rejection: None,
            correlation: None,
        }));
        self
    }

    /// The message shown for this failure. Request rejections quote the provider's own bounded
    /// explanation so a rejected reply format is not blamed on the user's request.
    pub fn message(&self) -> String {
        if self.http_status.is_some_and(|status| status >= 400) {
            if self.http_status.is_some_and(|status| status >= 500) {
                return "The provider had a server problem. Try again in a moment. Nothing was changed.".into();
            }
            if self.kind == ErrorKind::Deadline {
                return "The provider rejected the request after a timeout. Nothing was changed. Try again.".into();
            }
            // Keep the diagnostic reason, but do not claim an uncertain generation outcome.
            return format!(
                "{} Nothing was changed.",
                if self.kind == ErrorKind::RequestRejected {
                    self.rejected_message()
                } else {
                    self.default_message().to_owned()
                }
            );
        }
        if self.kind == ErrorKind::RequestRejected {
            return self.rejected_message();
        }
        self.default_message().to_owned()
    }

    fn rejected_message(&self) -> String {
        // OpenAI reports a reply schema it cannot accept through this code and parameter.
        // Nothing the user asked for can avoid it; Desegnos built the schema.
        let schema_problem = self.code.as_deref() == Some("invalid_json_schema")
            || self
                .param
                .as_deref()
                .is_some_and(|param| param.starts_with("text.format"));
        if schema_problem {
            return "Desegnos sent the AI service a reply format it doesn't accept. This is a Desegnos bug, not your request.".into();
        }
        match self
            .response_format
            .as_ref()
            .and_then(|format| format.provider_message.as_deref())
        {
            Some(message) => {
                format!("The AI service rejected this request: {}", bounded_message(message))
            }
            None => {
                "The AI service rejected this request. Review its settings and requirements before explicitly retrying."
                    .into()
            }
        }
    }

    fn default_message(&self) -> &'static str {
        if self.kind == ErrorKind::Malformed
            && let Some(reason) = self
                .response_format
                .as_ref()
                .and_then(|format| format.rejection)
        {
            return reason.message();
        }
        if self.authentication == AuthenticationMethod::Chatgpt {
            match self.kind {
                ErrorKind::InvalidSettings => {
                    return "Choose a model available to the selected ChatGPT account and a supported reasoning level.";
                }
                ErrorKind::InvalidCredential => {
                    return "The selected ChatGPT credential is invalid. Reconnect the account to obtain a valid session.";
                }
                ErrorKind::Authentication => {
                    return "OpenAI did not accept the selected ChatGPT account or its plan permission. Review the account and reconnect if its authorization was revoked.";
                }
                ErrorKind::Quota | ErrorKind::ChatgptUsageLimit => {
                    return "OpenAI reports a limit for this ChatGPT request. Open Manage usage in ChatGPT before explicitly trying again; Desegnos will not switch to API billing.";
                }
                ErrorKind::ModelUnavailable => {
                    return "The selected ChatGPT account cannot use this model. Refresh its available models and choose a supported model.";
                }
                ErrorKind::Incomplete => {
                    return "OpenAI stopped before completing the candidate. Review the requirements and plan usage before explicitly trying again.";
                }
                ErrorKind::Truncated => {
                    return "ChatGPT closed the connection before the reply finished. Try again; if it keeps happening, make the request smaller or more specific.";
                }
                ErrorKind::Cancelled => {
                    return "Generation was cancelled. A submitted request may still count toward ChatGPT plan or credit usage; late output will not be applied.";
                }
                ErrorKind::Deadline => {
                    return "ChatGPT stopped responding for 90 seconds or ran past 20 minutes. The request may still count toward plan or credit usage.";
                }
                ErrorKind::Transport => {
                    return "OpenAI could not complete the ChatGPT connection or admission. Keep the account connected and explicitly try later; a submitted request may have an unknown usage outcome.";
                }
                _ => {}
            }
        }
        match self.kind {
            ErrorKind::InvalidSettings => {
                "Choose an available model, supported reasoning and output settings, valid price ceilings, and a positive budget."
            }
            ErrorKind::InvalidRequirements => {
                "The frozen requirements or output schema failed validation. Prepare a new generation request."
            }
            ErrorKind::MissingCredential => {
                "Add an OpenAI API key in AI settings before sending this request."
            }
            ErrorKind::InvalidCredential => {
                "Enter a valid OpenAI API key without spaces or control characters."
            }
            ErrorKind::VaultUnavailable => {
                "System credential storage is unavailable. Unlock it or explicitly choose session-only memory storage."
            }
            ErrorKind::ContextLimit => {
                "The conservative complete-request estimate exceeds this model's context capacity. Reduce the request before sending."
            }
            ErrorKind::SpendingLimit => {
                "The conservative job estimate exceeds the chosen allowance. Reduce the request or explicitly increase the allowance."
            }
            ErrorKind::InputLimit => {
                "The complete request exceeds Desegnos's input limit. No content was truncated or sent."
            }
            ErrorKind::Repetition => {
                "The AI got stuck repeating the same changes, so Desegnos stopped it. Try again, or reword the request."
            }
            ErrorKind::OutputLimit => {
                "The AI kept writing past Desegnos's output limit, so it was stopped. Try again, or make the request smaller or more specific."
            }
            ErrorKind::Authentication => {
                "The AI service rejected this key or its permissions. Replace the key or review its API access."
            }
            ErrorKind::Quota => {
                "The AI service reports insufficient quota. Review the API account's billing and usage before explicitly retrying."
            }
            ErrorKind::RateLimited => {
                "The AI service rate-limited this request. Wait before explicitly retrying."
            }
            ErrorKind::ModelUnavailable => {
                "This key cannot access the selected model. Review model access in your AI account."
            }
            ErrorKind::RequestRejected => {
                "The AI service rejected this request. Review its settings and requirements before explicitly retrying."
            }
            ErrorKind::Redirect => {
                "The provider attempted an HTTP redirect. Desegnos refused it and did not forward the key."
            }
            ErrorKind::Refused => {
                "The provider refused this request. No generated candidate is available."
            }
            ErrorKind::Incomplete => {
                "The provider stopped before completing the candidate. Review the output limit and requirements before explicitly retrying."
            }
            ErrorKind::Truncated => {
                "The response stream ended without a complete candidate. Its billing outcome may be unknown; it was not retried."
            }
            ErrorKind::Malformed => {
                "The provider returned an invalid or inconsistent structured response. It was rejected."
            }
            ErrorKind::UnexpectedResponseFormat => {
                "The AI service did not open a generation stream. Nothing was applied. Open Details for response information. Usage may be uncertain; nothing was retried."
            }
            ErrorKind::Cancelled => {
                "Generation was cancelled. A submitted request may still incur a charge; late output will not be applied."
            }
            ErrorKind::Deadline => {
                "The provider stopped responding for 90 seconds or ran past 20 minutes. The request may still incur a charge."
            }
            ErrorKind::Server => "The provider had a server problem. Try again in a moment.",
            ErrorKind::Transport => {
                "The provider connection failed. A submitted request may have an unknown billing outcome."
            }
            ErrorKind::ChatgptNotEligible => {
                "The selected ChatGPT user or workspace is not eligible for this request. Review its plan and workspace policy; signing in repeatedly will not change eligibility."
            }
            ErrorKind::ChatgptUsageLimit => {
                "OpenAI reports a ChatGPT usage limit. Open Manage usage in ChatGPT before explicitly trying again."
            }
            ErrorKind::ChatgptUsageUnavailable => {
                "OpenAI cannot check ChatGPT usage right now. Keep the account connected and explicitly try later."
            }
            ErrorKind::ChatgptUnsupportedCapability => {
                "This ChatGPT request contains an unsupported capability. Review the reported request parameter and supported model before preparing another request."
            }
            ErrorKind::ChatgptRouteNotSupported => {
                "OpenAI rejected this ChatGPT endpoint or HTTP method. Update Desegnos or report the request ID; reconnecting will not repair the route."
            }
            ErrorKind::ChatgptInvalidUser => {
                "OpenAI could not validate the selected ChatGPT subscriber. Review the account and reconnect if its session was revoked."
            }
            ErrorKind::ChatgptPermission => {
                "OpenAI did not authorize this ChatGPT permission context. Review the selected account, granted plan permission and application registration."
            }
            ErrorKind::ChatgptUserUnavailable => {
                "OpenAI cannot retrieve ChatGPT user or workspace information right now. Keep the account connected and explicitly try later."
            }
        }
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// Keep a provider explanation to a short, readable line in the UI.
fn bounded_message(message: &str) -> String {
    const LIMIT: usize = 400;
    if message.chars().count() <= LIMIT {
        return message.to_owned();
    }
    let mut bounded: String = message.chars().take(LIMIT).collect();
    bounded.push('…');
    bounded
}

impl std::error::Error for ProviderError {}

fn safe_identifier(
    key: &ApiKey,
    value: Option<&str>,
    maximum: usize,
    parameter: bool,
) -> Option<Box<str>> {
    value
        .filter(|value| {
            !value.is_empty()
                && value.len() <= maximum
                && !key.appears_in(value)
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b'_' | b'-')
                        || (parameter && matches!(byte, b'.' | b'[' | b']'))
                })
        })
        .map(Box::from)
}

fn safe_provider_message(key: &ApiKey, value: Option<&str>) -> Option<Box<str>> {
    let value = value.filter(|value| {
        !value.trim().is_empty()
            && value.len() <= 1024
            && !key.appears_in(value)
            && !value.chars().any(|character| {
                (character.is_control() && !character.is_whitespace())
                    || matches!(character, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
    })?;
    if crate::redact::contains_secret(value) {
        return None;
    }
    Some(
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .into_boxed_str(),
    )
}

/// Bounded local diagnostics. Scrub before truncation so a secret crossing the limit cannot leak.
pub fn scrub_diagnostic_reply(text: &str, key: Option<&ApiKey>) -> String {
    const LIMIT: usize = 64 * 1024;
    let text = key.filter(|key| !key.is_anonymous()).map_or_else(
        || text.to_owned(),
        |key| text.replace(key.expose(), "[redacted]"),
    );
    let mut output = String::new();
    let mut redact_next = false;
    let mut rest = text.as_str();
    while !rest.is_empty() {
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || "_-.+/~".contains(c)))
            .unwrap_or(rest.len());
        if end == 0 {
            let ch = rest.chars().next().unwrap();
            if (!ch.is_control() || matches!(ch, '\n' | '\t'))
                && !matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                output.push(ch);
            } else {
                output.push_str("[control]");
            }
            rest = &rest[ch.len_utf8()..];
        } else {
            let word = &rest[..end];
            let secret = redact_next || crate::redact::is_secret_word(word);
            output.push_str(if secret { "[redacted]" } else { word });
            redact_next = crate::redact::SECRET_NAMES.contains(&word.to_ascii_lowercase().as_str());
            rest = &rest[end..];
        }
        if output.len() > LIMIT {
            const MARKER: &str = "\n[reply truncated]";
            while output.len() > LIMIT - MARKER.len() {
                output.pop();
            }
            output.push_str(MARKER);
            break;
        }
    }
    output
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn failed_reply_diagnostics_scrub_credentials_before_utf8_bounding() {
        let text = format!(
            "Bearer private-token api_key=another-secret access_token=third-secret sk-different-secret eyJheader.payload.signature {}",
            "é".repeat(70000)
        );
        let safe = scrub_diagnostic_reply(&text, None);
        for secret in [
            "private-token",
            "another-secret",
            "third-secret",
            "sk-different-secret",
            "eyJheader.payload.signature",
        ] {
            assert!(!safe.contains(secret));
        }
        assert!(safe.len() <= 64 * 1024);
        assert!(safe.ends_with("[reply truncated]"));
    }
}
