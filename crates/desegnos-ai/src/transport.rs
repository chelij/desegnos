use crate::net::{DEADLINE, ERROR_BODY_LIMIT, IDLE};
use crate::retry::RetryResponse as Response;
use crate::{
    ApiKey, AuthenticationMethod, ErrorKind, MAX_OUTPUT_TOKENS, ProviderError, ProviderSettings,
    ResponseBodyKind, Result, SubmissionOutcome, TokenUsage,
    identity::prepare_identity_body,
    request::prepare_body,
    sse::{
        MAX_EVENT_BYTES, MAX_STREAM_BYTES, ResponseStream, SseDecoder, error_code,
        event_stream_prefix,
    },
};
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::{sync::watch, time::Instant};

// A long, high-effort generation can stream for well over five minutes. Give up only when
// the provider goes silent for IDLE, or after DEADLINE overall.

#[derive(Clone)]
pub struct CancellationToken(watch::Sender<bool>);

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    pub fn new() -> Self {
        Self(watch::channel(false).0)
    }
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    /// Drop in-flight preparation or I/O as soon as cancellation is requested.
    pub async fn run_until_cancelled<T>(
        &self,
        future: impl std::future::Future<Output = T>,
    ) -> Option<T> {
        tokio::select! {
            biased;
            _ = self.cancelled() => None,
            value = future => Some(value),
        }
    }

    pub(crate) async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        loop {
            if *receiver.borrow_and_update() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProgress {
    pub retry: Option<ProviderRetry>,
    pub received_bytes: usize,
    pub response_id: Option<String>,
    pub web_search_calls: usize,
    pub citations: usize,
    /// Model text that arrived since the previous progress event. It stays in the process
    /// (never serialised) so a caller can show live progress and keep a cut-off reply.
    #[serde(skip)]
    pub text_delta: String,
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRetry {
    pub attempt: u8,
    pub kind: ErrorKind,
}
impl ProviderRetry {
    pub fn message(&self, provider: crate::ProviderId) -> String {
        let name = crate::provider_catalogue()
            .into_iter()
            .find(|item| item.id == provider)
            .map_or("The AI provider", |item| item.name);
        let reason = match self.kind {
            ErrorKind::Server => "had a server problem",
            ErrorKind::RateLimited => "is rate-limiting requests",
            _ => "lost the connection before replying",
        };
        format!("{name} {reason} — retrying ({} of 3)…", self.attempt)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderResponse {
    pub text: String,
    pub response_id: String,
    pub usage: Option<TokenUsage>,
}

impl std::fmt::Debug for ProviderResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderResponse")
            .field("text_bytes", &self.text.len())
            .field("response_id", &self.response_id)
            .field("usage", &self.usage)
            .finish()
    }
}

#[derive(Clone)]
pub struct OpenAiClient {
    pub(crate) client: Client,
    pub(crate) endpoint: String,
    pub(crate) timeouts: Timeouts,
}

#[derive(Clone)]
pub(crate) struct Timeouts {
    pub deadline: Duration,
    pub idle: Duration,
    pub retry_backoff: [Duration; 2],
}

struct PreparedRequest<'a> {
    authentication: AuthenticationMethod,
    job_id: uuid::Uuid,
    requirements_sha256: &'a str,
    settings: &'a ProviderSettings,
    key: &'a ApiKey,
    cancellation: &'a CancellationToken,
    body: Vec<u8>,
}

impl OpenAiClient {
    pub async fn list_api_models(
        &self,
        key: &ApiKey,
        cancellation: &CancellationToken,
    ) -> Result<Vec<crate::ModelInfo>> {
        crate::model_discovery::list_models(
            crate::ProviderId::Openai,
            &self.client,
            &self.endpoint,
            key,
            cancellation,
        )
        .await
    }
    pub fn new() -> Result<Self> {
        Self::build(
            crate::ProviderId::Openai.spec().endpoint.into(),
            true,
            DEADLINE,
        )
    }

    fn build(endpoint: String, https_only: bool, deadline: Duration) -> Result<Self> {
        let client = crate::net::hardened_client(https_only)
            .timeout(deadline)
            .read_timeout(IDLE)
            .connect_timeout(crate::net::CONNECT_TIMEOUT)
            .build()
            .map_err(|_| ProviderError::before(ErrorKind::Transport))?;
        Ok(Self {
            client,
            endpoint,
            timeouts: Timeouts {
                deadline,
                idle: IDLE,
                retry_backoff: crate::retry::DEFAULT_BACKOFF,
            },
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn for_loopback(endpoint: &str) -> Result<Self> {
        let url = reqwest::Url::parse(endpoint)
            .map_err(|_| ProviderError::before(ErrorKind::InvalidSettings))?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
            || url.port().is_none()
            || url.path().trim_end_matches('/') != "/v1"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ProviderError::before(ErrorKind::InvalidSettings));
        }
        let mut client = Self::build(endpoint.trim_end_matches('/').into(), false, DEADLINE)?;
        client.timeouts.retry_backoff = [Duration::from_millis(20), Duration::from_millis(80)];
        Ok(client)
    }

    // Test timeouts must stay well above the loopback `retry_backoff` of 20/80 ms.
    #[cfg(test)]
    pub(crate) fn with_test_deadline(mut self, deadline: Duration) -> Self {
        self.timeouts.deadline = deadline;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_test_idle(mut self, idle: Duration) -> Self {
        self.timeouts.idle = idle;
        self
    }

    pub(crate) fn image_api_request(
        &self,
        path: &str,
        key: &ApiKey,
        content_type: &str,
        body: Vec<u8>,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(self
            .client
            .post(format!("{}/{path}", self.endpoint))
            .header(AUTHORIZATION, authorization(key)?)
            .header(CONTENT_TYPE, content_type)
            .header(ACCEPT, "application/json")
            .body(body))
    }

    /// Shared wire contract for Responses generation and capability probes.
    pub(crate) fn responses_request(
        &self,
        key: &ApiKey,
        body: Vec<u8>,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(self
            .client
            .post(format!("{}/responses", self.endpoint))
            .header(AUTHORIZATION, authorization(key)?)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "text/event-stream")
            .body(body))
    }

    /// Send one frozen generation request and stream its reply.
    pub async fn generate(
        &self,
        request: crate::GenerationRequest<'_>,
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        let crate::GenerationRequest {
            authentication,
            snapshot,
            mode,
            schema,
            settings,
            key,
            cancellation,
        } = request;
        async {
            let body = prepare_body(authentication, snapshot, mode, schema, settings)?;
            self.submit_for_auth(
                PreparedRequest {
                    authentication,
                    job_id: snapshot.job_id,
                    requirements_sha256: &snapshot.sha256,
                    settings,
                    key,
                    cancellation,
                    body,
                },
                progress,
            )
            .await
        }
        .await
        .map_err(|error| error.with_authentication(authentication))
    }

    /// Analyze reusable identity from a frozen source packet without generating website code.
    ///
    /// The caller validates the identity-specific candidate after the shared transport verifies
    /// the completed stream and its exact job ID and requirements digest.
    pub async fn analyze_identity(
        &self,
        request: crate::IdentityRequest<'_>,
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        let crate::IdentityRequest {
            authentication,
            snapshot,
            schema,
            settings,
            key,
            cancellation,
            images,
        } = request;
        let result = async {
            let body = prepare_identity_body(authentication, snapshot, schema, settings, images)?;
            self.submit_for_auth(
                PreparedRequest {
                    authentication,
                    job_id: snapshot.job_id,
                    requirements_sha256: &snapshot.sha256,
                    settings,
                    key,
                    cancellation,
                    body,
                },
                progress,
            )
            .await
        }
        .await;
        result.map_err(|error| error.with_authentication(authentication))
    }

    async fn submit_for_auth(
        &self,
        request: PreparedRequest<'_>,
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        let PreparedRequest {
            authentication,
            job_id,
            requirements_sha256,
            settings,
            key,
            cancellation,
            body,
        } = request;
        if cancellation.is_cancelled() {
            return Err(ProviderError::before(ErrorKind::Cancelled));
        }
        let deadline = Instant::now() + self.timeouts.deadline;
        let request = self.responses_request(key, body)?;
        progress(ProviderProgress {
            retry: None,
            received_bytes: 0,
            response_id: None,
            web_search_calls: 0,
            citations: 0,
            text_delta: String::new(),
        });
        if cancellation.is_cancelled() {
            return Err(ProviderError::before(ErrorKind::Cancelled));
        }
        let mut response = crate::retry::send(
            request,
            cancellation,
            crate::retry::RetryPolicy {
                backoff: self.timeouts.retry_backoff,
                ..crate::retry::RetryPolicy::new(deadline, self.timeouts.idle, MAX_STREAM_BYTES)
            },
            &progress,
        )
        .await?;
        check_status(&mut response, authentication, key, cancellation, deadline).await?;
        let declared_stream = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"));
        let mut decoder = SseDecoder::default();
        let mut stream = ResponseStream::new(
            authentication,
            key,
            job_id.to_string(),
            requirements_sha256,
            match authentication {
                AuthenticationMethod::ApiKey => settings.max_output_tokens,
                AuthenticationMethod::Chatgpt => MAX_OUTPUT_TOKENS,
            },
            crate::net::request_id(response.headers()),
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
        );
        if declared_stream {
            stream.body_kind = ResponseBodyKind::EventStream;
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_STREAM_BYTES as u64)
        {
            return Err(stream.byte_limit_error(MAX_STREAM_BYTES, SubmissionOutcome::Unknown));
        }
        let mut streaming = declared_stream;
        let mut prefix = vec![];
        let mut received = 0_usize;
        loop {
            let bytes =
                crate::net::next_chunk(&mut response, cancellation, deadline, self.timeouts.idle)
                    .await
                    .map_err(|failure| stream.error(failure.kind(), SubmissionOutcome::Unknown))?;
            let Some(bytes) = bytes else {
                if !streaming {
                    return Err(non_stream_error(
                        &mut response,
                        authentication,
                        key,
                        cancellation,
                        deadline,
                        prefix,
                        &progress,
                    )
                    .await);
                }
                return Err(stream.error(ErrorKind::Truncated, SubmissionOutcome::Unknown));
            };
            received = received.checked_add(bytes.len()).ok_or_else(|| {
                stream.byte_limit_error(MAX_STREAM_BYTES, SubmissionOutcome::Unknown)
            })?;
            stream.received_bytes = received;
            if received > MAX_STREAM_BYTES {
                return Err(stream.byte_limit_error(MAX_STREAM_BYTES, SubmissionOutcome::Unknown));
            }
            let bytes = if streaming {
                bytes.as_ref()
            } else {
                prefix.extend_from_slice(&bytes);
                match event_stream_prefix(&prefix) {
                    Some(true) => {
                        streaming = true;
                        stream.body_kind = ResponseBodyKind::EventStream;
                    }
                    Some(false) => {
                        return Err(non_stream_error(
                            &mut response,
                            authentication,
                            key,
                            cancellation,
                            deadline,
                            prefix,
                            &progress,
                        )
                        .await);
                    }
                    None => {
                        progress(ProviderProgress {
                            retry: None,
                            received_bytes: received,
                            response_id: None,
                            web_search_calls: 0,
                            citations: 0,
                            text_delta: String::new(),
                        });
                        continue;
                    }
                }
                prefix.as_slice()
            };
            let frames = decoder.push(bytes).map_err(|error| {
                if error.kind == ErrorKind::OutputLimit {
                    stream.byte_limit_error(MAX_EVENT_BYTES, error.outcome)
                } else {
                    stream.error(error.kind, error.outcome)
                }
            })?;
            prefix.clear();
            for frame in frames {
                if let Some(complete) = stream.accept(frame)? {
                    if cancellation.is_cancelled() {
                        return Err(stream.error(ErrorKind::Cancelled, SubmissionOutcome::Received));
                    }
                    if Instant::now() >= deadline {
                        return Err(stream.error(ErrorKind::Deadline, SubmissionOutcome::Received));
                    }
                    progress(ProviderProgress {
                        retry: None,
                        received_bytes: received,
                        response_id: stream.response_id.clone(),
                        web_search_calls: stream.web_search_calls(),
                        citations: stream.citations(),
                        text_delta: stream.take_text_delta(),
                    });
                    return Ok(complete);
                }
            }
            progress(ProviderProgress {
                retry: None,
                received_bytes: received,
                response_id: stream.response_id.clone(),
                web_search_calls: stream.web_search_calls(),
                citations: stream.citations(),
                text_delta: stream.take_text_delta(),
            });
        }
    }

    pub async fn test_model_access(
        &self,
        settings: &ProviderSettings,
        key: &ApiKey,
        cancellation: &CancellationToken,
    ) -> Result<()> {
        settings.validate()?;
        crate::model_discovery::probe_model(
            crate::ProviderId::Openai,
            &self.client,
            &self.endpoint,
            &settings.model,
            key,
            cancellation,
            crate::retry::RetryPolicy {
                backoff: self.timeouts.retry_backoff,
                ..crate::retry::RetryPolicy::new(
                    Instant::now() + crate::net::IDLE,
                    crate::net::PROBE_IDLE,
                    ERROR_BODY_LIMIT,
                )
            },
        )
        .await
    }
}

pub(crate) fn authorization(key: &ApiKey) -> Result<HeaderValue> {
    if key.is_anonymous() {
        return Err(ProviderError::before(ErrorKind::InvalidCredential));
    }
    let text = zeroize::Zeroizing::new(format!("Bearer {}", key.expose()));
    let mut value = HeaderValue::from_str(&text)
        .map_err(|_| ProviderError::before(ErrorKind::InvalidCredential))?;
    value.set_sensitive(true);
    Ok(value)
}

async fn non_stream_error(
    response: &mut Response,
    authentication: AuthenticationMethod,
    key: &ApiKey,
    cancellation: &CancellationToken,
    deadline: Instant,
    mut body: Vec<u8>,
    progress: &(impl Fn(ProviderProgress) + Send + Sync),
) -> ProviderError {
    let status = response.status().as_u16();
    let request_id = crate::net::request_id(response.headers()).map(str::to_owned);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut received = body.len();
    let mut truncated = body.len() > ERROR_BODY_LIMIT;
    body.truncate(ERROR_BODY_LIMIT);
    progress(ProviderProgress {
        retry: None,
        received_bytes: received,
        response_id: None,
        web_search_calls: 0,
        citations: 0,
        text_delta: String::new(),
    });
    while !truncated {
        let chunk = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(ProviderError::new(ErrorKind::Cancelled, SubmissionOutcome::Unknown)),
            _ = tokio::time::sleep_until(deadline) => Err(ProviderError::new(ErrorKind::Deadline, SubmissionOutcome::Unknown)),
            result = response.chunk() => result.map_err(|error| ProviderError::new(
                if error.is_timeout() { ErrorKind::Deadline } else { ErrorKind::Transport }, SubmissionOutcome::Unknown)),
        };
        let chunk = match chunk {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(error) => {
                let mut error = error
                    .with_authentication(authentication)
                    .with_provider_metadata(key, Some(status), None, None, request_id.as_deref())
                    .with_response_format(
                        key,
                        content_type.as_deref(),
                        ResponseBodyKind::Unavailable,
                        received,
                        None,
                    );
                error
                    .response_format
                    .as_mut()
                    .expect("response format added")
                    .body_truncated = true;
                return error;
            }
        };
        received = received.saturating_add(chunk.len());
        progress(ProviderProgress {
            retry: None,
            received_bytes: received,
            response_id: None,
            web_search_calls: 0,
            citations: 0,
            text_delta: String::new(),
        });
        let retained = chunk.len().min(ERROR_BODY_LIMIT - body.len());
        body.extend_from_slice(&chunk[..retained]);
        truncated = retained < chunk.len();
    }
    let json: Option<Value> = if truncated {
        None
    } else {
        serde_json::from_slice(&body).ok()
    };
    let body_kind = response_body_kind(&body, json.as_ref());
    let code = json
        .as_ref()
        .and_then(|value| value.pointer("/error/code").or_else(|| value.get("code")))
        .and_then(Value::as_str);
    let param = json
        .as_ref()
        .and_then(|value| value.pointer("/error/param"))
        .and_then(Value::as_str);
    let message = json.as_ref().and_then(provider_message);
    let (kind, outcome) = match body_kind {
        ResponseBodyKind::JsonError => (
            error_code(authentication, code),
            SubmissionOutcome::Received,
        ),
        ResponseBodyKind::JsonDetail => (ErrorKind::RequestRejected, SubmissionOutcome::Received),
        _ => (
            ErrorKind::UnexpectedResponseFormat,
            SubmissionOutcome::Unknown,
        ),
    };
    let mut error = ProviderError::new(kind, outcome)
        .with_authentication(authentication)
        .with_provider_metadata(key, Some(status), code, param, request_id.as_deref())
        .with_response_format(key, content_type.as_deref(), body_kind, received, message)
        .with_diagnostic_reply(&String::from_utf8_lossy(&body), key);
    if truncated {
        let format = error
            .response_format
            .as_mut()
            .expect("response format added");
        format.body_truncated = true;
        format.limit_bytes = Some(ERROR_BODY_LIMIT);
    }
    error
}

pub(crate) fn provider_message(value: &Value) -> Option<&str> {
    value
        .pointer("/error/message")
        .or_else(|| value.get("providerMessage"))
        .or_else(|| value.get("detail"))
        .and_then(Value::as_str)
}

pub(crate) fn response_body_kind(body: &[u8], json: Option<&Value>) -> ResponseBodyKind {
    if body.is_empty() {
        ResponseBodyKind::Empty
    } else if let Some(value) = json {
        if value.get("error").is_some_and(Value::is_object) {
            ResponseBodyKind::JsonError
        } else if value.get("detail").is_some_and(Value::is_string) {
            ResponseBodyKind::JsonDetail
        } else if value.get("object").and_then(Value::as_str) == Some("response") {
            ResponseBodyKind::JsonResponse
        } else {
            ResponseBodyKind::JsonOther
        }
    } else if let Ok(text) = std::str::from_utf8(body) {
        let start = text.trim_start().to_ascii_lowercase();
        if start.starts_with("<!doctype html") || start.starts_with("<html") {
            ResponseBodyKind::Html
        } else {
            ResponseBodyKind::Text
        }
    } else {
        ResponseBodyKind::Binary
    }
}

pub(crate) async fn check_status(
    response: &mut Response,
    authentication: AuthenticationMethod,
    key: &ApiKey,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<()> {
    let status = response.status();
    if status.is_redirection() {
        return Err(
            ProviderError::new(ErrorKind::Redirect, SubmissionOutcome::Rejected)
                .with_provider_metadata(
                    key,
                    Some(status.as_u16()),
                    None,
                    None,
                    crate::net::request_id(response.headers()),
                ),
        );
    }
    if status == StatusCode::OK {
        return Ok(());
    }
    let outcome = SubmissionOutcome::Rejected;
    let crate::net::ErrorHead {
        request_id,
        content_type,
        ..
    } = crate::net::ErrorHead::of(response);
    let body = bounded_body(response, ERROR_BODY_LIMIT, cancellation, deadline)
        .await
        .map_err(|mut error| {
            error.outcome = SubmissionOutcome::Rejected;
            if status.is_server_error() {
                error.kind = ErrorKind::Server;
            }
            error.with_provider_metadata(
                key,
                Some(status.as_u16()),
                None,
                None,
                request_id.as_deref(),
            )
        })?;
    let error: Option<Value> = serde_json::from_slice(&body).ok();
    let code = error
        .as_ref()
        .and_then(|value| value.pointer("/error/code").or_else(|| value.get("code")))
        .and_then(Value::as_str);
    let classified = error_code(authentication, code);
    let kind = if authentication == AuthenticationMethod::Chatgpt
        && classified != ErrorKind::RequestRejected
    {
        classified
    } else {
        match (crate::net::status_kind(status.as_u16()), classified) {
            (ErrorKind::RateLimited, ErrorKind::Quota) => ErrorKind::Quota,
            (ErrorKind::RequestRejected, classified) => classified,
            (kind, _) => kind,
        }
    };
    let param = error
        .as_ref()
        .and_then(|value| value.pointer("/error/param"))
        .and_then(Value::as_str);
    Err(ProviderError::new(kind, outcome)
        .with_authentication(authentication)
        .with_provider_metadata(
            key,
            Some(status.as_u16()),
            code,
            param,
            request_id.as_deref(),
        )
        .with_response_format(
            key,
            content_type.as_deref(),
            response_body_kind(&body, error.as_ref()),
            body.len(),
            error.as_ref().and_then(provider_message),
        )
        .with_diagnostic_reply(&String::from_utf8_lossy(&body), key))
}

use crate::net::bounded_body;
