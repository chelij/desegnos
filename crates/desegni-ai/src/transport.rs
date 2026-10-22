use crate::{
    ApiKey, AuthenticationMethod, ChatgptModel, ErrorKind, IdentityImage, MODEL_MAX_OUTPUT_TOKENS,
    ProviderError, ProviderSettings, ResponseBodyKind, Result, SubmissionOutcome, TokenUsage,
    identity::prepare_identity_body,
    settings::prepare_body,
    sse::{
        MAX_EVENT_BYTES, MAX_STREAM_BYTES, ResponseStream, SseDecoder, error_code,
        event_stream_prefix,
    },
};
use desegni_core::{GenerationMode, RequirementsSnapshot};
use reqwest::{
    Client, Response, StatusCode,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::{sync::watch, time::Instant};

const ENDPOINT: &str = "https://api.openai.com/v1";
const DEADLINE: Duration = Duration::from_secs(300);
const ERROR_BODY_LIMIT: usize = 65_536;
const MODELS_BODY_LIMIT: usize = 1024 * 1024;
const MAX_MODELS: usize = 2048;

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
    pub received_bytes: usize,
    pub response_id: Option<String>,
    pub web_search_calls: usize,
    pub citations: usize,
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
    deadline: Duration,
}

impl OpenAiClient {
    pub fn new() -> Result<Self> {
        Self::build(ENDPOINT.into(), true, DEADLINE)
    }

    fn build(endpoint: String, https_only: bool, deadline: Duration) -> Result<Self> {
        let client = Client::builder()
            .tls_backend_rustls()
            .https_only(https_only)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .referer(false)
            .connection_verbose(false)
            .tls_sslkeylogfile(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .timeout(deadline)
            .connect_timeout(Duration::from_secs(20))
            .user_agent(concat!("Desegnos/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| ProviderError::before(ErrorKind::Transport))?;
        Ok(Self {
            client,
            endpoint,
            deadline,
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
        Self::build(endpoint.trim_end_matches('/').into(), false, DEADLINE)
    }

    #[cfg(test)]
    pub(crate) fn with_test_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
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

    #[allow(clippy::too_many_arguments)]
    pub async fn generate(
        &self,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
        schema: &Value,
        settings: &ProviderSettings,
        key: &ApiKey,
        cancellation: &CancellationToken,
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        self.generate_for_auth(
            AuthenticationMethod::ApiKey,
            snapshot,
            mode,
            schema,
            settings,
            key,
            cancellation,
            progress,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn generate_for_auth(
        &self,
        authentication: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
        schema: &Value,
        settings: &ProviderSettings,
        key: &ApiKey,
        cancellation: &CancellationToken,
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        self.generate_inner(
            authentication,
            snapshot,
            mode,
            schema,
            settings,
            key,
            cancellation,
            progress,
        )
        .await
        .map_err(|error| error.with_authentication(authentication))
    }

    #[allow(clippy::too_many_arguments)]
    async fn generate_inner(
        &self,
        authentication: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
        schema: &Value,
        settings: &ProviderSettings,
        key: &ApiKey,
        cancellation: &CancellationToken,
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        let body = prepare_body(authentication, snapshot, mode, schema, settings)?;
        self.submit_for_auth(
            authentication,
            snapshot,
            settings,
            key,
            cancellation,
            body,
            progress,
        )
        .await
    }

    /// Analyze reusable identity from a frozen source packet without generating website code.
    ///
    /// The caller validates the identity-specific candidate after the shared transport verifies
    /// the completed stream and its exact job ID and requirements digest.
    #[allow(clippy::too_many_arguments)]
    pub async fn analyze_identity_for_auth(
        &self,
        authentication: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        schema: &Value,
        settings: &ProviderSettings,
        key: &ApiKey,
        cancellation: &CancellationToken,
        images: &[IdentityImage],
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        let result = async {
            let body = prepare_identity_body(authentication, snapshot, schema, settings, images)?;
            self.submit_for_auth(
                authentication,
                snapshot,
                settings,
                key,
                cancellation,
                body,
                progress,
            )
            .await
        }
        .await;
        result.map_err(|error| error.with_authentication(authentication))
    }

    #[allow(clippy::too_many_arguments)]
    async fn submit_for_auth(
        &self,
        authentication: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        settings: &ProviderSettings,
        key: &ApiKey,
        cancellation: &CancellationToken,
        body: Vec<u8>,
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        if cancellation.is_cancelled() {
            return Err(ProviderError::before(ErrorKind::Cancelled));
        }
        let deadline = Instant::now() + self.deadline;
        let request = self.responses_request(key, body)?;
        progress(ProviderProgress {
            received_bytes: 0,
            response_id: None,
            web_search_calls: 0,
            citations: 0,
        });
        if cancellation.is_cancelled() {
            return Err(ProviderError::before(ErrorKind::Cancelled));
        }
        let mut response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(ProviderError::new(ErrorKind::Cancelled, SubmissionOutcome::Unknown)),
            _ = tokio::time::sleep_until(deadline) => return Err(ProviderError::new(ErrorKind::Deadline, SubmissionOutcome::Unknown)),
            result = request.send() => result.map_err(|error| ProviderError::new(
                if error.is_timeout() { ErrorKind::Deadline } else { ErrorKind::Transport }, SubmissionOutcome::Unknown))?,
        };
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
            snapshot.job_id.to_string(),
            &snapshot.sha256,
            match authentication {
                AuthenticationMethod::ApiKey => settings.max_output_tokens,
                AuthenticationMethod::Chatgpt => MODEL_MAX_OUTPUT_TOKENS,
            },
            response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok()),
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
            let bytes = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(stream.error(ErrorKind::Cancelled, SubmissionOutcome::Unknown)),
                _ = tokio::time::sleep_until(deadline) => return Err(stream.error(ErrorKind::Deadline, SubmissionOutcome::Unknown)),
                result = response.chunk() => result.map_err(|error| stream.error(
                    if error.is_timeout() { ErrorKind::Deadline } else { ErrorKind::Transport }, SubmissionOutcome::Unknown))?,
            };
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
                            received_bytes: received,
                            response_id: None,
                            web_search_calls: 0,
                            citations: 0,
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
                        received_bytes: received,
                        response_id: stream.response_id.clone(),
                        web_search_calls: stream.web_search_calls(),
                        citations: stream.citations(),
                    });
                    return Ok(complete);
                }
            }
            progress(ProviderProgress {
                received_bytes: received,
                response_id: stream.response_id.clone(),
                web_search_calls: stream.web_search_calls(),
                citations: stream.citations(),
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
        if cancellation.is_cancelled() {
            return Err(ProviderError::before(ErrorKind::Cancelled));
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        let request = self
            .client
            .get(format!("{}/models/{}", self.endpoint, settings.model))
            .header(AUTHORIZATION, authorization(key)?)
            .header(ACCEPT, "application/json");
        let mut response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(ProviderError::before(ErrorKind::Cancelled)),
            _ = tokio::time::sleep_until(deadline) => return Err(ProviderError::before(ErrorKind::Deadline)),
            result = request.send() => result.map_err(|_| ProviderError::before(ErrorKind::Transport))?,
        };
        check_status(
            &mut response,
            AuthenticationMethod::ApiKey,
            key,
            cancellation,
            deadline,
        )
        .await?;
        let bytes = bounded_body(&mut response, ERROR_BODY_LIMIT, cancellation, deadline).await?;
        let result: Value = serde_json::from_slice(&bytes)
            .map_err(|_| ProviderError::before(ErrorKind::Malformed))?;
        if result.get("id").and_then(Value::as_str) != Some(&settings.model)
            || result.get("object").and_then(Value::as_str) != Some("model")
        {
            return Err(ProviderError::before(ErrorKind::ModelUnavailable));
        }
        Ok(())
    }

    pub async fn list_chatgpt_models(
        &self,
        key: &ApiKey,
        cancellation: &CancellationToken,
    ) -> Result<Vec<ChatgptModel>> {
        self.list_chatgpt_models_inner(key, cancellation)
            .await
            .map_err(|error| error.with_authentication(AuthenticationMethod::Chatgpt))
    }

    async fn list_chatgpt_models_inner(
        &self,
        key: &ApiKey,
        cancellation: &CancellationToken,
    ) -> Result<Vec<ChatgptModel>> {
        if cancellation.is_cancelled() {
            return Err(ProviderError::before(ErrorKind::Cancelled));
        }
        let deadline = Instant::now() + self.deadline.min(Duration::from_secs(30));
        let request = self
            .client
            .get(format!("{}/models", self.endpoint))
            .header(AUTHORIZATION, authorization(key)?)
            .header(ACCEPT, "application/json");
        let mut response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(ProviderError::before(ErrorKind::Cancelled)),
            _ = tokio::time::sleep_until(deadline) => return Err(ProviderError::before(ErrorKind::Deadline)),
            result = request.send() => result.map_err(|error| ProviderError::before(
                if error.is_timeout() { ErrorKind::Deadline } else { ErrorKind::Transport }))?,
        };
        check_status(
            &mut response,
            AuthenticationMethod::Chatgpt,
            key,
            cancellation,
            deadline,
        )
        .await?;
        let bytes = bounded_body(&mut response, MODELS_BODY_LIMIT, cancellation, deadline).await?;
        let catalogue: ChatgptCatalogue = serde_json::from_slice(&bytes)
            .map_err(|_| ProviderError::before(ErrorKind::Malformed))?;
        if catalogue.models.len() > MAX_MODELS {
            return Err(ProviderError::before(ErrorKind::OutputLimit));
        }
        let mut models = Vec::new();
        let mut slugs = std::collections::HashSet::new();
        for model in catalogue.models {
            if model.slug.is_empty()
                || model.slug.len() > 128
                || !model
                    .slug
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
                || model.display_name.trim().is_empty()
                || model.display_name.len() > 256
                || model.display_name.chars().any(char::is_control)
                || model.visibility.is_empty()
                || model.visibility.len() > 32
                || !model
                    .visibility
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
                || key.appears_in(&model.slug)
                || key.appears_in(&model.display_name)
                || key.appears_in(&model.visibility)
                || !slugs.insert(model.slug.clone())
            {
                return Err(ProviderError::before(ErrorKind::Malformed));
            }
            if model.visibility == "list" {
                models.push(ChatgptModel {
                    id: model.slug,
                    name: model.display_name,
                });
            }
        }
        if cancellation.is_cancelled() {
            return Err(ProviderError::before(ErrorKind::Cancelled));
        }
        if Instant::now() >= deadline {
            return Err(ProviderError::before(ErrorKind::Deadline));
        }
        Ok(models)
    }
}

#[derive(Deserialize)]
struct ChatgptCatalogue {
    models: Vec<ChatgptCatalogueModel>,
}

#[derive(Deserialize)]
struct ChatgptCatalogueModel {
    slug: String,
    display_name: String,
    visibility: String,
}

fn authorization(key: &ApiKey) -> Result<HeaderValue> {
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
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut received = body.len();
    let mut truncated = body.len() > ERROR_BODY_LIMIT;
    body.truncate(ERROR_BODY_LIMIT);
    progress(ProviderProgress {
        received_bytes: received,
        response_id: None,
        web_search_calls: 0,
        citations: 0,
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
            received_bytes: received,
            response_id: None,
            web_search_calls: 0,
            citations: 0,
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
        .and_then(|value| value.pointer("/error/code"))
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

fn provider_message(value: &Value) -> Option<&str> {
    value
        .pointer("/error/message")
        .or_else(|| value.get("detail"))
        .and_then(Value::as_str)
}

fn response_body_kind(body: &[u8], json: Option<&Value>) -> ResponseBodyKind {
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

async fn check_status(
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
                    response
                        .headers()
                        .get("x-request-id")
                        .and_then(|value| value.to_str().ok()),
                ),
        );
    }
    if status == StatusCode::OK {
        return Ok(());
    }
    let outcome = if status.is_server_error() || status == StatusCode::REQUEST_TIMEOUT {
        SubmissionOutcome::Unknown
    } else {
        SubmissionOutcome::Rejected
    };
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = bounded_body(response, ERROR_BODY_LIMIT, cancellation, deadline)
        .await
        .map_err(|error| {
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
        .and_then(|value| value.pointer("/error/code"))
        .and_then(Value::as_str);
    let classified = error_code(authentication, code);
    let kind = if authentication == AuthenticationMethod::Chatgpt
        && classified != ErrorKind::RequestRejected
    {
        classified
    } else {
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorKind::Authentication,
            StatusCode::NOT_FOUND => ErrorKind::ModelUnavailable,
            StatusCode::TOO_MANY_REQUESTS => match classified {
                ErrorKind::Quota => ErrorKind::Quota,
                _ => ErrorKind::RateLimited,
            },
            _ if status.is_server_error() => ErrorKind::Transport,
            _ => classified,
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

async fn bounded_body(
    response: &mut Response,
    limit: usize,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(ProviderError::new(
            ErrorKind::OutputLimit,
            SubmissionOutcome::Unknown,
        ));
    }
    let mut body = vec![];
    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(ProviderError::new(ErrorKind::Cancelled, SubmissionOutcome::Unknown)),
            _ = tokio::time::sleep_until(deadline) => return Err(ProviderError::new(ErrorKind::Deadline, SubmissionOutcome::Unknown)),
            result = response.chunk() => result.map_err(|_| ProviderError::new(ErrorKind::Transport, SubmissionOutcome::Unknown))?,
        };
        let Some(chunk) = chunk else {
            return Ok(body);
        };
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(ProviderError::new(
                ErrorKind::OutputLimit,
                SubmissionOutcome::Unknown,
            ));
        }
        body.extend_from_slice(&chunk);
    }
}
