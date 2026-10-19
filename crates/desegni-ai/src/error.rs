use crate::{ApiKey, AuthenticationMethod, TokenUsage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    ChatgptNotEligible,
    ChatgptUsageLimit,
    ChatgptUsageUnavailable,
    ChatgptUnsupportedCapability,
    ChatgptRouteNotSupported,
    ChatgptInvalidUser,
    ChatgptPermission,
    ChatgptUserUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubmissionOutcome {
    NotSubmitted,
    Rejected,
    Unknown,
    Received,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalOutput {
    NotReceived,
    Absent,
    Null,
    Array,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseFormat {
    pub content_type: Option<Box<str>>,
    pub body_kind: ResponseBodyKind,
    pub body_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_message: Option<Box<str>>,
    #[serde(default)]
    pub body_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection: Option<ResponseRejection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<Box<ResponseCorrelation>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderError {
    pub kind: ErrorKind,
    pub outcome: SubmissionOutcome,
    pub response_id: Option<String>,
    pub usage: Option<TokenUsage>,
    #[serde(default)]
    pub authentication: AuthenticationMethod,
    #[serde(default)]
    pub http_status: Option<u16>,
    #[serde(default)]
    pub code: Option<Box<str>>,
    #[serde(default)]
    pub param: Option<Box<str>>,
    #[serde(default)]
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

    pub fn message(&self) -> &'static str {
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
                    return "Choose a verified model available to the selected ChatGPT account and a supported reasoning level.";
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
                    return "The ChatGPT stream ended without a completed candidate. Its plan usage outcome may be unknown; it was not retried.";
                }
                ErrorKind::Cancelled => {
                    return "Generation was cancelled. A submitted request may still count toward ChatGPT plan or credit usage; late output will not be applied.";
                }
                ErrorKind::Deadline => {
                    return "The ChatGPT request deadline expired. A submitted request may still count toward plan or credit usage; it was not retried.";
                }
                ErrorKind::Transport => {
                    return "OpenAI could not complete the ChatGPT connection or admission. Keep the account connected and explicitly try later; a submitted request may have an unknown usage outcome.";
                }
                _ => {}
            }
        }
        match self.kind {
            ErrorKind::InvalidSettings => {
                "Choose a supported model, reasoning level, output limit and positive spending allowance."
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
            ErrorKind::OutputLimit => {
                "The provider response exceeded Desegnos's bounded output limit and was rejected."
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
                "The provider deadline expired. A submitted request may still incur a charge; it was not retried."
            }
            ErrorKind::Transport => {
                "The provider connection failed. A submitted request may have an unknown outcome; it was not retried."
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
        f.write_str(self.message())
    }
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
    let lower = value.to_ascii_lowercase();
    if lower.contains("bearer")
        || lower.contains("sk-")
        || lower.contains("access_token")
        || lower.contains("refresh_token")
        || value
            .split_whitespace()
            .any(|word| word.starts_with("eyJ") && word.matches('.').count() >= 2)
    {
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
