//! Native provider boundary. A completed, bounded JSON candidate is the only
//! output; server search commentary and citations never become candidate text.
use crate::{
    ApiKey, AuthenticationMethod, CancellationToken, CostEstimate, ErrorKind, IdentityImage,
    MAX_OUTPUT_BYTES, MAX_OUTPUT_TOKENS, MAX_REQUEST_BYTES, ModelInfo, OpenAiClient, ProviderError,
    ProviderProgress, ProviderResponse, ProviderSettings, ReasoningEffort, Result,
    SubmissionOutcome, TokenUsage,
    sse::{MAX_STREAM_BYTES, SseDecoder},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use desegni_core::{GenerationCapabilities, GenerationMode, RequirementsSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, future::Future, pin::Pin, time::Duration};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    #[default]
    Openai,
    Anthropic,
    Google,
    Openrouter,
    Local,
}
impl ProviderId {
    pub fn service(self) -> &'static str {
        match self {
            Self::Openai => "io.desegni.openai",
            Self::Anthropic => "io.desegnos.anthropic",
            Self::Google => "io.desegnos.google",
            Self::Openrouter => "io.desegnos.openrouter",
            Self::Local => "io.desegnos.local",
        }
    }
    pub fn available(self) -> bool {
        matches!(self, Self::Openai | Self::Anthropic | Self::Google)
    }
    pub fn capabilities(self) -> GenerationCapabilities {
        GenerationCapabilities {
            web_search: self.available(),
            image_downloads: if self.available() { 8 } else { 0 },
            image_generation: (self == Self::Openai).then(Default::default),
        }
    }
    pub fn capabilities_for(
        self,
        _auth: AuthenticationMethod,
        settings: &ProviderSettings,
        access: Option<&crate::ImageAccess>,
    ) -> GenerationCapabilities {
        let mut capabilities = self.capabilities();
        if !settings.image_generation
            || !access.is_some_and(|access| {
                access.available == crate::ImageAvailability::Yes
                    && !access.expired(crate::image_access_now())
            })
        {
            capabilities.image_generation = None;
        }
        capabilities
    }
    pub fn defaults(self) -> ProviderSettings {
        ProviderSettings {
            model: match self {
                Self::Anthropic => "claude-sonnet-5-5",
                Self::Google => "gemini-3.8-flash",
                _ => crate::MODEL_ID,
            }
            .into(),
            ..Default::default()
        }
    }
    pub fn validate(self, settings: &ProviderSettings, auth: AuthenticationMethod) -> Result<()> {
        if self == Self::Openai {
            return settings.validate_for_auth(auth);
        }
        if !self.available()
            || auth != AuthenticationMethod::ApiKey
            || settings.model != self.defaults().model
        {
            return Err(ProviderError::before(ErrorKind::InvalidSettings));
        }
        let checked = ProviderSettings {
            model: crate::MODEL_ID.into(),
            ..settings.clone()
        };
        checked.validate()
    }
    pub fn catalogue(self) -> Vec<ModelInfo> {
        if self == Self::Openai {
            return crate::model_catalogue();
        }
        if !self.available() {
            return vec![];
        }
        let (name, input, output, source, context) = match self {
            Self::Anthropic => (
                "Claude Sonnet 5.5",
                2.0,
                10.0,
                "https://platform.claude.com/docs/en/about-claude/pricing",
                1_000_000,
            ),
            // Catalogue rates verified on the date below; estimates use the announced higher ceiling.
            _ => (
                "Gemini 3.8 Flash",
                0.75,
                3.75,
                "https://ai.google.dev/gemini-api/docs/pricing",
                1_048_576,
            ),
        };
        vec![ModelInfo {
            id: self.defaults().model,
            name: name.into(),
            input_usd_per_million: input,
            output_usd_per_million: output,
            verified_at: "2026-10-19".into(),
            source_url: source.into(),
            reasoning_efforts: vec![
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            context_window_tokens: context,
            max_output_tokens: MAX_OUTPUT_TOKENS,
            long_context_input_usd_per_million: input,
            long_context_output_usd_per_million: output,
        }]
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub name: &'static str,
    pub available: bool,
    pub sign_in_supported: bool,
    pub capabilities: GenerationCapabilities,
}
pub fn provider_catalogue() -> Vec<ProviderDescriptor> {
    [
        (ProviderId::Openai, "OpenAI"),
        (ProviderId::Anthropic, "Anthropic"),
        (ProviderId::Google, "Google"),
        (ProviderId::Openrouter, "OpenRouter"),
        (ProviderId::Local, "Local"),
    ]
    .into_iter()
    .map(|(id, name)| ProviderDescriptor {
        id,
        name,
        available: id.available(),
        sign_in_supported: id == ProviderId::Openai,
        capabilities: id.capabilities(),
    })
    .collect()
}

pub struct GenerationRequest<'a> {
    pub authentication: AuthenticationMethod,
    pub snapshot: &'a RequirementsSnapshot,
    pub mode: GenerationMode,
    pub schema: &'a Value,
    pub settings: &'a ProviderSettings,
    pub key: &'a ApiKey,
    pub cancellation: &'a CancellationToken,
}
pub type ProviderFuture<'a> = Pin<Box<dyn Future<Output = Result<ProviderResponse>> + Send + 'a>>;
/// Provider differences stop here. No HTTP or credential details enter the UI.
pub trait ProviderAdapter: Send + Sync {
    fn generate<'a>(
        &'a self,
        request: GenerationRequest<'a>,
        progress: &'a (dyn Fn(ProviderProgress) + Send + Sync),
    ) -> ProviderFuture<'a>;
}
impl ProviderAdapter for OpenAiClient {
    fn generate<'a>(
        &'a self,
        r: GenerationRequest<'a>,
        p: &'a (dyn Fn(ProviderProgress) + Send + Sync),
    ) -> ProviderFuture<'a> {
        Box::pin(self.generate_for_auth(
            r.authentication,
            r.snapshot,
            r.mode,
            r.schema,
            r.settings,
            r.key,
            r.cancellation,
            p,
        ))
    }
}

/// Keeps the existing OpenAI session/model-discovery client independent of key adapters.
#[derive(Clone)]
pub struct ProviderClient {
    pub id: ProviderId,
    openai: OpenAiClient,
    remote: Option<KeyAdapter>,
}
impl ProviderClient {
    pub fn new(id: ProviderId, openai: OpenAiClient) -> Result<Self> {
        if !id.available() {
            return Err(ProviderError::before(ErrorKind::InvalidSettings));
        }
        Ok(Self {
            id,
            openai,
            remote: if id == ProviderId::Openai {
                None
            } else {
                Some(KeyAdapter::new(id)?)
            },
        })
    }
    pub fn estimate(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
        schema: &Value,
        settings: &ProviderSettings,
    ) -> Result<CostEstimate> {
        if self.id == ProviderId::Openai {
            return crate::estimate_for_auth(auth, snapshot, mode, schema, settings);
        }
        self.id.validate(settings, auth)?;
        let (counted, reserve) = crate::settings::without_previews(snapshot);
        let body = crate::settings::request_body(
            AuthenticationMethod::ApiKey,
            &counted,
            mode,
            schema,
            &shadow(settings),
        )?;
        foreign_estimate(self.id, &body, settings, reserve)
    }
    pub fn estimate_identity(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        schema: &Value,
        settings: &ProviderSettings,
        images: &[IdentityImage],
    ) -> Result<CostEstimate> {
        estimate_identity_for_provider(self.id, auth, snapshot, schema, settings, images)
    }
    pub async fn test_model_access(
        &self,
        settings: &ProviderSettings,
        key: &ApiKey,
        cancel: &CancellationToken,
    ) -> Result<()> {
        if let Some(remote) = &self.remote {
            self.id.validate(settings, AuthenticationMethod::ApiKey)?;
            remote.test_access(settings, key, cancel).await
        } else {
            self.openai.test_model_access(settings, key, cancel).await
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_for_auth(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
        schema: &Value,
        settings: &ProviderSettings,
        key: &ApiKey,
        cancel: &CancellationToken,
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        let r = GenerationRequest {
            authentication: auth,
            snapshot,
            mode,
            schema,
            settings,
            key,
            cancellation: cancel,
        };
        if let Some(remote) = &self.remote {
            remote.generate(r, &progress).await
        } else {
            ProviderAdapter::generate(&self.openai, r, &progress).await
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn analyze_identity_for_auth(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        schema: &Value,
        settings: &ProviderSettings,
        key: &ApiKey,
        cancel: &CancellationToken,
        images: &[IdentityImage],
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        if let Some(remote) = &self.remote {
            self.estimate_identity(auth, snapshot, schema, settings, images)?;
            let common =
                crate::identity::identity_body(auth, snapshot, schema, &shadow(settings), images)?;
            remote
                .submit(common, settings, key, cancel, &progress)
                .await
        } else {
            self.openai
                .analyze_identity_for_auth(
                    auth, snapshot, schema, settings, key, cancel, images, progress,
                )
                .await
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_images(
        &self,
        response: ProviderResponse,
        auth: AuthenticationMethod,
        settings: &ProviderSettings,
        snapshot: &RequirementsSnapshot,
        project: &desegni_core::Project,
        key: &ApiKey,
        cancel: &CancellationToken,
    ) -> ProviderResponse {
        if self.id != ProviderId::Openai {
            return response;
        }
        self.openai
            .generate_images(response, auth, settings, snapshot, project, key, cancel)
            .await
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_images_with_status(
        &self,
        response: ProviderResponse,
        auth: AuthenticationMethod,
        settings: &ProviderSettings,
        snapshot: &RequirementsSnapshot,
        project: &desegni_core::Project,
        key: &ApiKey,
        cancel: &CancellationToken,
    ) -> (ProviderResponse, bool) {
        if self.id != ProviderId::Openai {
            return (response, false);
        }
        self.openai
            .generate_images_with_status(response, auth, settings, snapshot, project, key, cancel)
            .await
    }
    pub async fn download_images(
        &self,
        response: ProviderResponse,
        snapshot: &RequirementsSnapshot,
        project: &desegni_core::Project,
        cancel: &CancellationToken,
    ) -> ProviderResponse {
        self.openai
            .download_images(response, snapshot, project, cancel)
            .await
    }
}

/// The same complete request check used before sending, without credentials or network access.
pub fn estimate_identity_for_provider(
    provider: ProviderId,
    auth: AuthenticationMethod,
    snapshot: &RequirementsSnapshot,
    schema: &Value,
    settings: &ProviderSettings,
    images: &[IdentityImage],
) -> Result<CostEstimate> {
    if provider == ProviderId::Openai {
        return crate::estimate_identity_for_auth(auth, snapshot, schema, settings, images);
    }
    provider.validate(settings, auth)?;
    let body = crate::identity::identity_body(auth, snapshot, schema, &shadow(settings), images)?;
    foreign_estimate(
        provider,
        &body,
        settings,
        crate::identity::vision_reserve(images),
    )
}

fn shadow(s: &ProviderSettings) -> ProviderSettings {
    ProviderSettings {
        model: crate::MODEL_ID.into(),
        ..s.clone()
    }
}
fn foreign_estimate(
    id: ProviderId,
    body: &[u8],
    settings: &ProviderSettings,
    reserve: u64,
) -> Result<CostEstimate> {
    // Byte-per-token bound includes schema, attachments and per-image vision reserve.
    let wire = serde_json::to_vec(&foreign_body(id, body, settings)?)
        .map_err(|_| ProviderError::before(ErrorKind::InvalidRequirements))?;
    if wire.len() > MAX_REQUEST_BYTES {
        return Err(ProviderError::before(ErrorKind::InputLimit));
    }
    let input = wire.len() as u64 + reserve + 65_536;
    let info = id.catalogue().remove(0);
    if input + u64::from(settings.max_output_tokens) > info.context_window_tokens {
        return Err(ProviderError::before(ErrorKind::InputLimit));
    }
    let input_rate = if id == ProviderId::Google {
        1.5
    } else {
        info.input_usd_per_million
    };
    let output_rate = if id == ProviderId::Google {
        7.5
    } else {
        info.output_usd_per_million
    };
    let estimate = ((input as f64 * input_rate
        + f64::from(settings.max_output_tokens) * output_rate)
        / 1_000_000.0
        * 100.0)
        .ceil()
        / 100.0;
    if estimate > settings.spending_limit_usd {
        return Err(ProviderError::before(ErrorKind::SpendingLimit));
    }
    Ok(CostEstimate {
        estimate_usd: estimate,
        input_token_upper_bound: input,
        verified_at: info.verified_at,
    })
}

#[derive(Clone)]
struct KeyAdapter {
    id: ProviderId,
    client: reqwest::Client,
    endpoint: String,
    deadline: Duration,
}
const PROVIDER_IDLE: Duration = Duration::from_secs(90);
impl KeyAdapter {
    fn new(id: ProviderId) -> Result<Self> {
        let endpoint = match id {
            ProviderId::Anthropic => "https://api.anthropic.com/v1",
            ProviderId::Google => "https://generativelanguage.googleapis.com/v1beta",
            _ => return Err(ProviderError::before(ErrorKind::InvalidSettings)),
        };
        Self::build(id, endpoint, true)
    }
    fn build(id: ProviderId, endpoint: &str, https: bool) -> Result<Self> {
        // Same limits as the OpenAI transport: silence for 90 s or 20 minutes overall.
        let deadline = Duration::from_secs(20 * 60);
        let client = reqwest::Client::builder()
            .tls_backend_rustls()
            .https_only(https)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .referer(false)
            .tls_sslkeylogfile(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .timeout(deadline)
            .read_timeout(PROVIDER_IDLE)
            .connect_timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| ProviderError::before(ErrorKind::Transport))?;
        Ok(Self {
            id,
            client,
            endpoint: endpoint.into(),
            deadline,
        })
    }
    fn authorized(
        &self,
        request: reqwest::RequestBuilder,
        key: &ApiKey,
    ) -> Result<reqwest::RequestBuilder> {
        let mut header = reqwest::header::HeaderValue::from_str(key.expose())
            .map_err(|_| ProviderError::before(ErrorKind::InvalidCredential))?;
        header.set_sensitive(true);
        Ok(match self.id {
            ProviderId::Anthropic => request
                .header("x-api-key", header)
                .header("anthropic-version", "2023-06-01"),
            _ => request.header("x-goog-api-key", header),
        })
    }
    async fn test_access(
        &self,
        s: &ProviderSettings,
        key: &ApiKey,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let path = match self.id {
            ProviderId::Anthropic => format!("{}/models/{}", self.endpoint, s.model),
            _ => format!("{}/models/{}", self.endpoint, s.model),
        };
        let response = tokio::select! {biased;_ = cancel.cancelled()=>return Err(ProviderError::before(ErrorKind::Cancelled)), r=self.authorized(self.client.get(path),key)?.send()=>r.map_err(|_|ProviderError::before(ErrorKind::Transport))?};
        if response.status().is_success() {
            Ok(())
        } else {
            Err(http_error(response.status().as_u16()))
        }
    }
    async fn submit(
        &self,
        common: Vec<u8>,
        s: &ProviderSettings,
        key: &ApiKey,
        cancel: &CancellationToken,
        progress: &(dyn Fn(ProviderProgress) + Send + Sync),
    ) -> Result<ProviderResponse> {
        let body = foreign_body(self.id, &common, s)?;
        let body = serde_json::to_vec(&body)
            .map_err(|_| ProviderError::before(ErrorKind::InvalidRequirements))?;
        if body.len() > MAX_REQUEST_BYTES
            || key.appears_in(std::str::from_utf8(&body).unwrap_or_default())
        {
            return Err(ProviderError::before(ErrorKind::InputLimit));
        }
        let path = match self.id {
            ProviderId::Anthropic => format!("{}/messages", self.endpoint),
            _ => format!(
                "{}/models/{}:streamGenerateContent?alt=sse",
                self.endpoint, s.model
            ),
        };
        let deadline = tokio::time::Instant::now() + self.deadline;
        let request = self.authorized(
            self.client
                .post(path)
                .header("content-type", "application/json")
                .header("accept", "text/event-stream")
                .body(body),
            key,
        )?;
        let mut response = tokio::select! {biased;_ = cancel.cancelled()=>return Err(ProviderError::before(ErrorKind::Cancelled)), _=tokio::time::sleep_until(deadline.min(tokio::time::Instant::now()+PROVIDER_IDLE))=>return Err(failure(ErrorKind::Deadline)),r=request.send()=>r.map_err(|_|failure(ErrorKind::Transport))?};
        if !response.status().is_success() {
            return Err(http_error(response.status().as_u16()));
        }
        if !response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
            })
        {
            return Err(failure(ErrorKind::UnexpectedResponseFormat));
        }
        let mut decoder = SseDecoder::default();
        let mut stream = ForeignStream::new(self.id);
        let mut bytes = 0usize;
        loop {
            let chunk = tokio::select! {biased;_=cancel.cancelled()=>return Err(stream.error(ErrorKind::Cancelled)),_=tokio::time::sleep_until(deadline.min(tokio::time::Instant::now()+PROVIDER_IDLE))=>return Err(stream.error(ErrorKind::Deadline)),r=response.chunk()=>r.map_err(|_|stream.error(ErrorKind::Transport))?};
            let Some(chunk) = chunk else {
                break;
            };
            bytes = bytes
                .checked_add(chunk.len())
                .ok_or_else(|| stream.error(ErrorKind::OutputLimit))?;
            if bytes > MAX_STREAM_BYTES {
                return Err(stream.error(ErrorKind::OutputLimit));
            }
            for frame in decoder.push(&chunk)? {
                if key.appears_in(&frame.data) {
                    return Err(stream.error(ErrorKind::Malformed));
                }
                stream.accept(&frame.data)?;
            }
            if key.appears_in(&stream.text) {
                return Err(stream.error(ErrorKind::Malformed));
            }
            progress(ProviderProgress {
                received_bytes: bytes,
                response_id: stream.response_id.clone(),
                web_search_calls: stream.searches,
                citations: stream.citations,
            });
        }
        decoder.finish()?;
        if cancel.is_cancelled() {
            return Err(stream.error(ErrorKind::Cancelled));
        }
        stream.finish()
    }
}
impl ProviderAdapter for KeyAdapter {
    fn generate<'a>(
        &'a self,
        r: GenerationRequest<'a>,
        p: &'a (dyn Fn(ProviderProgress) + Send + Sync),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.id.validate(r.settings, r.authentication)?;
            let body = crate::settings::request_body(
                r.authentication,
                r.snapshot,
                r.mode,
                r.schema,
                &shadow(r.settings),
            )?;
            foreign_estimate(self.id, &body, r.settings, 0)?;
            self.submit(body, r.settings, r.key, r.cancellation, p)
                .await
        })
    }
}
fn failure(kind: ErrorKind) -> ProviderError {
    ProviderError::new(kind, SubmissionOutcome::Unknown)
}
fn http_error(status: u16) -> ProviderError {
    let kind = match status {
        401 | 403 => ErrorKind::Authentication,
        404 => ErrorKind::ModelUnavailable,
        429 => ErrorKind::RateLimited,
        300..=399 => ErrorKind::Redirect,
        _ => ErrorKind::Transport,
    };
    let mut error = ProviderError::new(
        kind,
        if status >= 500 {
            SubmissionOutcome::Unknown
        } else {
            SubmissionOutcome::Rejected
        },
    );
    error.http_status = Some(status);
    error
}
fn foreign_body(id: ProviderId, bytes: &[u8], s: &ProviderSettings) -> Result<Value> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|_| ProviderError::before(ErrorKind::InvalidRequirements))?;
    let schema = &v["text"]["format"]["schema"];
    let content = v["input"][0]["content"]
        .as_array()
        .ok_or_else(|| ProviderError::before(ErrorKind::InvalidRequirements))?;
    let mut parts = vec![];
    for c in content {
        match c["type"].as_str() {
            Some("input_text") => parts.push(json!({"text":c["text"]})),
            Some("input_file") => {
                let data = c["file_data"]
                    .as_str()
                    .and_then(|v| v.split_once(','))
                    .ok_or_else(|| ProviderError::before(ErrorKind::InvalidRequirements))?
                    .1;
                let text = String::from_utf8(
                    STANDARD
                        .decode(data)
                        .map_err(|_| ProviderError::before(ErrorKind::InvalidRequirements))?,
                )
                .map_err(|_| ProviderError::before(ErrorKind::InvalidRequirements))?;
                parts.push(json!({"text":text}));
            }
            Some("input_image") => {
                let (mime, data) = c["image_url"]
                    .as_str()
                    .and_then(|v| v.strip_prefix("data:"))
                    .and_then(|v| v.split_once(";base64,"))
                    .ok_or_else(|| ProviderError::before(ErrorKind::InvalidRequirements))?;
                parts.push(json!({"inlineData":{"mimeType":mime,"data":data}}));
            }
            _ => return Err(ProviderError::before(ErrorKind::InvalidRequirements)),
        }
    }
    let search = v["tools"].as_array().is_some_and(|v| !v.is_empty());
    if id == ProviderId::Google {
        let mut body = json!({"systemInstruction":{"parts":[{"text":v["instructions"]}]},"contents":[{"role":"user","parts":parts}],"generationConfig":{"maxOutputTokens":s.max_output_tokens,"thinkingConfig":{"thinkingLevel":format!("{:?}",s.reasoning_effort).to_ascii_uppercase()},"responseFormat":{"text":{"mimeType":"APPLICATION_JSON","schema":schema}}}});
        if search {
            body["tools"] = json!([{"googleSearch":{}}]);
        }
        Ok(body)
    } else {
        let content:Vec<_>=parts.into_iter().map(|p|if p.get("text").is_some(){json!({"type":"text","text":p["text"]})}else{json!({"type":"image","source":{"type":"base64","media_type":p["inlineData"]["mimeType"],"data":p["inlineData"]["data"]}})}).collect();
        let mut tools = vec![
            json!({"name":"submit_result","description":"Return the complete requested structured result exactly once. Do not execute this tool; its input is the final answer.","input_schema":schema,"strict":true}),
        ];
        if search {
            tools.push(json!({"type":"web_search_20250305","name":"web_search","max_uses":5}));
        }
        Ok(
            json!({"model":s.model,"max_tokens":s.max_output_tokens,"stream":true,"thinking":{"type":"between_tools"},"output_config":{"effort":s.reasoning_effort},"system":format!("{}\nAfter any research, submit the final candidate using submit_result exactly once. Its input must match the schema. Do not call any other client tool.",v["instructions"].as_str().unwrap_or_default()),"messages":[{"role":"user","content":content}],"tools":tools,"tool_choice":{"type":"auto","disable_parallel_tool_use":true}}),
        )
    }
}

struct Block {
    kind: String,
    closed: bool,
}
struct ForeignStream {
    provider: ProviderId,
    response_id: Option<String>,
    text: String,
    usage: Option<TokenUsage>,
    anthropic_tokens: [u64; 4],
    blocks: BTreeMap<u64, Block>,
    result: Option<u64>,
    stopped: bool,
    terminal: bool,
    searches: usize,
    citations: usize,
}
impl ForeignStream {
    fn new(provider: ProviderId) -> Self {
        Self {
            provider,
            response_id: None,
            text: String::new(),
            usage: None,
            anthropic_tokens: [0; 4],
            blocks: BTreeMap::new(),
            result: None,
            stopped: false,
            terminal: false,
            searches: 0,
            citations: 0,
        }
    }
    fn error(&self, kind: ErrorKind) -> ProviderError {
        let mut e = failure(kind);
        e.response_id = self.response_id.clone();
        e.usage = self.usage;
        e
    }
    fn append(&mut self, text: &str) -> Result<()> {
        if self.text.len().saturating_add(text.len()) > MAX_OUTPUT_BYTES {
            return Err(self.error(ErrorKind::OutputLimit));
        }
        self.text.push_str(text);
        Ok(())
    }
    fn accept(&mut self, data: &str) -> Result<()> {
        let v: Value = serde_json::from_str(data).map_err(|_| self.error(ErrorKind::Malformed))?;
        if v.get("error").is_some() || v["type"] == "error" {
            return Err(self.error(ErrorKind::Transport));
        }
        if self.terminal {
            return Err(self.error(ErrorKind::Malformed));
        }
        if self.provider == ProviderId::Google {
            return self.google(v);
        }
        match v["type"].as_str() {
            Some("ping") => {}
            Some("message_start") => {
                if self.response_id.is_some() {
                    return Err(self.error(ErrorKind::Malformed));
                }
                self.response_id = v["message"]["id"]
                    .as_str()
                    .filter(|s| s.len() <= 256)
                    .map(str::to_owned);
                self.usage = Some(anthropic_usage(
                    &v["message"]["usage"],
                    &mut self.anthropic_tokens,
                )?);
            }
            Some("content_block_start") => {
                let i = v["index"]
                    .as_u64()
                    .ok_or_else(|| self.error(ErrorKind::Malformed))?;
                if self.response_id.is_none()
                    || self.blocks.len() >= 4096
                    || self.blocks.contains_key(&i)
                    || self.stopped
                {
                    return Err(self.error(ErrorKind::Malformed));
                }
                let b = &v["content_block"];
                let kind = b["type"]
                    .as_str()
                    .ok_or_else(|| self.error(ErrorKind::Malformed))?;
                if kind == "tool_use" {
                    if self.result.is_some()
                        || b["name"] != "submit_result"
                        || b["input"] != json!({})
                    {
                        return Err(self.error(ErrorKind::Malformed));
                    }
                    self.result = Some(i);
                }
                if kind == "server_tool_use" {
                    if b["name"] != "web_search" {
                        return Err(self.error(ErrorKind::Malformed));
                    }
                    self.searches += 1;
                }
                if !matches!(
                    kind,
                    "text"
                        | "tool_use"
                        | "server_tool_use"
                        | "web_search_tool_result"
                        | "thinking"
                        | "redacted_thinking"
                ) {
                    return Err(self.error(ErrorKind::Malformed));
                }
                self.blocks.insert(
                    i,
                    Block {
                        kind: kind.into(),
                        closed: false,
                    },
                );
            }
            Some("content_block_delta") => {
                let i = v["index"]
                    .as_u64()
                    .ok_or_else(|| self.error(ErrorKind::Malformed))?;
                let b = self
                    .blocks
                    .get(&i)
                    .filter(|b| !b.closed)
                    .ok_or_else(|| self.error(ErrorKind::Malformed))?;
                let d = &v["delta"];
                match d["type"].as_str() {
                    Some("input_json_delta") if b.kind == "tool_use" => self.append(
                        d["partial_json"]
                            .as_str()
                            .ok_or_else(|| self.error(ErrorKind::Malformed))?,
                    )?,
                    Some("input_json_delta") if b.kind == "server_tool_use" => {}
                    Some("text_delta") if b.kind == "text" => {}
                    Some("citations_delta") if b.kind == "text" => self.citations += 1,
                    Some("thinking_delta" | "signature_delta") if b.kind == "thinking" => {}
                    _ => return Err(self.error(ErrorKind::Malformed)),
                }
            }
            Some("content_block_stop") => {
                let i = v["index"]
                    .as_u64()
                    .ok_or_else(|| self.error(ErrorKind::Malformed))?;
                if !self.blocks.get(&i).is_some_and(|b| !b.closed) {
                    return Err(self.error(ErrorKind::Malformed));
                }
                self.blocks.get_mut(&i).unwrap().closed = true;
            }
            Some("message_delta") => {
                if self.stopped
                    || self.blocks.values().any(|b| !b.closed)
                    || self.result.is_none()
                    || v["delta"]["stop_reason"] != "tool_use"
                {
                    return Err(self.error(ErrorKind::Incomplete));
                }
                self.usage = Some(anthropic_usage(&v["usage"], &mut self.anthropic_tokens)?);
                self.stopped = true;
            }
            Some("message_stop") => {
                if !self.stopped {
                    return Err(self.error(ErrorKind::Truncated));
                }
                self.terminal = true;
            }
            _ => return Err(self.error(ErrorKind::Malformed)),
        }
        Ok(())
    }
    fn google(&mut self, v: Value) -> Result<()> {
        if v.get("promptFeedback")
            .is_some_and(|p| p.get("blockReason").is_some())
        {
            return Err(self.error(ErrorKind::Refused));
        }
        if let Some(id) = v["responseId"].as_str() {
            if id.len() > 256 || self.response_id.as_ref().is_some_and(|old| old != id) {
                return Err(self.error(ErrorKind::Malformed));
            }
            self.response_id = Some(id.into());
        }
        if let Some(u) = v.get("usageMetadata") {
            let input = u["promptTokenCount"].as_u64().unwrap_or(0);
            let output = u["candidatesTokenCount"]
                .as_u64()
                .unwrap_or(0)
                .checked_add(u["thoughtsTokenCount"].as_u64().unwrap_or(0))
                .ok_or_else(|| self.error(ErrorKind::Malformed))?;
            let total = u["totalTokenCount"].as_u64().unwrap_or(
                input
                    .checked_add(output)
                    .ok_or_else(|| self.error(ErrorKind::Malformed))?,
            );
            if total
                < input
                    .checked_add(output)
                    .ok_or_else(|| self.error(ErrorKind::Malformed))?
            {
                return Err(self.error(ErrorKind::Malformed));
            }
            self.usage = Some(TokenUsage {
                input_tokens: input,
                output_tokens: output,
                total_tokens: total,
            });
        }
        if let Some(candidates) = v["candidates"].as_array() {
            if candidates.len() != 1 {
                return Err(self.error(ErrorKind::Malformed));
            }
            let c = &candidates[0];
            if c["index"].as_u64().is_some_and(|i| i != 0) {
                return Err(self.error(ErrorKind::Malformed));
            }
            if let Some(parts) = c["content"]["parts"].as_array() {
                for p in parts {
                    if p["thought"] == true {
                        continue;
                    }
                    if let Some(t) = p["text"].as_str() {
                        if self.stopped {
                            return Err(self.error(ErrorKind::Malformed));
                        }
                        self.append(t)?;
                    } else if p.as_object().is_some_and(|part| {
                        part.len() == 1
                            && part.get("thoughtSignature").is_some_and(Value::is_string)
                    }) {
                        // A standalone encrypted signature is metadata for a future turn.
                    } else {
                        return Err(self.error(ErrorKind::Malformed));
                    }
                }
            }
            if let Some(g) = c.get("groundingMetadata") {
                self.searches = self
                    .searches
                    .max(g["webSearchQueries"].as_array().map_or(0, Vec::len));
                self.citations = self
                    .citations
                    .max(g["groundingChunks"].as_array().map_or(0, Vec::len));
            }
            if let Some(reason) = c["finishReason"].as_str() {
                if self.stopped || reason != "STOP" {
                    return Err(self.error(ErrorKind::Incomplete));
                }
                self.stopped = true;
            }
        } else if v.get("usageMetadata").is_none() {
            return Err(self.error(ErrorKind::Malformed));
        }
        Ok(())
    }
    fn finish(self) -> Result<ProviderResponse> {
        if !self.stopped || (self.provider == ProviderId::Anthropic && !self.terminal) {
            return Err(self.error(ErrorKind::Truncated));
        }
        if self.response_id.is_none() || self.text.is_empty() {
            return Err(self.error(ErrorKind::Malformed));
        }
        let rejection = match serde_json::from_str::<Value>(&self.text) {
            Err(_) => Some(crate::ResponseRejection::CandidateJson),
            Ok(value) if !value.is_object() => Some(crate::ResponseRejection::CandidateShape),
            _ => None,
        };
        if let Some(rejection) = rejection {
            // Only a completed final answer is repairable. Wire/terminal errors above remain
            // transport failures; credential echoes were rejected while receiving frames.
            let mut error = self.error(ErrorKind::Malformed);
            error.outcome = SubmissionOutcome::Received;
            error.response_format = Some(Box::new(crate::ResponseFormat {
                diagnostic_reply: Some(
                    crate::scrub_diagnostic_reply(&self.text, None).into_boxed_str(),
                ),
                content_type: Some("text/event-stream".into()),
                body_kind: crate::ResponseBodyKind::Text,
                body_bytes: self.text.len(),
                provider_message: None,
                body_truncated: false,
                limit_bytes: None,
                rejection: Some(rejection),
                correlation: None,
            }));
            return Err(error);
        }
        Ok(ProviderResponse {
            text: self.text,
            response_id: self.response_id.unwrap(),
            usage: self.usage,
        })
    }
}
fn anthropic_usage(v: &Value, counts: &mut [u64; 4]) -> Result<TokenUsage> {
    for (index, name) in [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(value) = v.get(name) {
            counts[index] = value
                .as_u64()
                .ok_or_else(|| failure(ErrorKind::Malformed))?;
        }
    }
    let input = counts[0]
        .checked_add(counts[1])
        .and_then(|v| v.checked_add(counts[2]))
        .ok_or_else(|| failure(ErrorKind::Malformed))?;
    let output = counts[3];
    Ok(TokenUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: input
            .checked_add(output)
            .ok_or_else(|| failure(ErrorKind::Malformed))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    fn anthropic(text: &str) -> Vec<Value> {
        vec![
            json!({"type":"message_start","message":{"id":"msg_test","usage":{"input_tokens":42,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I found a source."}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"citations_delta","citation":{"url":"https://example.org"}}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"server_tool_use","id":"srv1","name":"web_search","input":{}}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"photo\"}"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"web_search_tool_result","tool_use_id":"srv1","content":[]}}),
            json!({"type":"content_block_stop","index":2}),
            json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"tool1","name":"submit_result","input":{}}}),
            json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":text}}),
            json!({"type":"content_block_stop","index":3}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":12}}),
            json!({"type":"message_stop"}),
        ]
    }
    fn google(text: &str) -> Vec<Value> {
        vec![
            json!({"responseId":"response_test","candidates":[{"index":0,"content":{"parts":[{"text":text}]}}]}),
            json!({"responseId":"response_test","candidates":[{"index":0,"finishReason":"STOP","groundingMetadata":{"webSearchQueries":["photo"],"groundingChunks":[{"web":{"uri":"https://example.org"}}]}}],"usageMetadata":{"promptTokenCount":42,"candidatesTokenCount":10,"thoughtsTokenCount":2,"totalTokenCount":54}}),
        ]
    }
    #[test]
    fn protocols_accept_exact_json_and_keep_search_and_citations_separate() {
        for (id, events) in [
            (
                ProviderId::Anthropic,
                anthropic("{\"html\":\"Création 💡\"}"),
            ),
            (ProviderId::Google, google("{\"html\":\"Création 💡\"}")),
        ] {
            let mut stream = ForeignStream::new(id);
            for event in events {
                stream.accept(&event.to_string()).unwrap();
            }
            assert_eq!(stream.searches, 1);
            assert_eq!(stream.citations, 1);
            let response = stream.finish().unwrap();
            assert_eq!(response.text, "{\"html\":\"Création 💡\"}");
            assert_eq!(response.usage.unwrap().total_tokens, 54);
        }
    }
    #[test]
    fn incomplete_duplicate_unexpected_tools_and_non_json_are_rejected() {
        for id in [ProviderId::Anthropic, ProviderId::Google] {
            let mut stream = ForeignStream::new(id);
            for v in if id == ProviderId::Anthropic {
                anthropic("oops")
            } else {
                google("oops")
            } {
                stream.accept(&v.to_string()).unwrap();
            }
            assert_eq!(stream.finish().unwrap_err().kind, ErrorKind::Malformed);
            assert_eq!(
                ForeignStream::new(id).finish().unwrap_err().kind,
                ErrorKind::Truncated
            );
        }
        let mut stream = ForeignStream::new(ProviderId::Anthropic);
        let mut events = anthropic("{}");
        events[10]["content_block"]["name"] = json!("execute_shell");
        assert!(
            events
                .into_iter()
                .try_for_each(|v| stream.accept(&v.to_string()))
                .is_err()
        );
        let mut stream = ForeignStream::new(ProviderId::Google);
        let mut events = google("{}");
        events[1]["candidates"][0]["finishReason"] = json!("MAX_TOKENS");
        assert_eq!(
            events
                .into_iter()
                .try_for_each(|v| stream.accept(&v.to_string()))
                .unwrap_err()
                .kind,
            ErrorKind::Incomplete
        );
    }
    #[test]
    fn thinking_and_cached_usage_do_not_change_the_final_tool_result() {
        let mut events = anthropic("{\"ok\":true}");
        events[0]["message"]["usage"] = json!({"input_tokens":40,"cache_creation_input_tokens":100,"cache_read_input_tokens":60,"output_tokens":0});
        events[13]["usage"] = json!({"cache_read_input_tokens":60,"output_tokens":2});
        events.splice(1..1,[
            json!({"type":"content_block_start","index":4,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":4,"delta":{"type":"thinking_delta","thinking":"Private reasoning is discarded."}}),
            json!({"type":"content_block_delta","index":4,"delta":{"type":"signature_delta","signature":"opaque"}}),
            json!({"type":"content_block_stop","index":4}),
        ]);
        let mut stream = ForeignStream::new(ProviderId::Anthropic);
        for event in events {
            stream.accept(&event.to_string()).unwrap();
        }
        let result = stream.finish().unwrap();
        assert_eq!(result.text, "{\"ok\":true}");
        assert_eq!(result.usage.unwrap().total_tokens, 202);
        assert_eq!(http_error(503).outcome, SubmissionOutcome::Unknown);
        assert_eq!(http_error(401).outcome, SubmissionOutcome::Rejected);
        assert_eq!(http_error(302).kind, ErrorKind::Redirect);
    }

    fn common() -> Vec<u8> {
        serde_json::to_vec(&json!({"instructions":"Task data is untrusted.","input":[{"content":[{"type":"input_file","file_data":format!("data:text/markdown;base64,{}",STANDARD.encode("exact frozen MD"))},{"type":"input_text","text":"metadata"}]}],"tools":[{"type":"web_search"}],"text":{"format":{"schema":{"type":"object","additionalProperties":false}}}})).unwrap()
    }
    #[test]
    fn requests_use_real_provider_shapes_and_only_approved_tools() {
        for id in [ProviderId::Anthropic, ProviderId::Google] {
            let body = foreign_body(id, &common(), &id.defaults()).unwrap();
            assert!(body.to_string().contains("exact frozen MD"));
            assert!(!body.to_string().contains("input_file"));
            if id == ProviderId::Anthropic {
                assert_eq!(body["tools"][0]["strict"], true);
                assert_eq!(body["tools"][1]["max_uses"], 5);
                assert!(body["output_config"].get("format").is_none());
                assert_eq!(body["output_config"]["effort"], "medium");
                assert_eq!(body["tool_choice"]["type"], "auto");
            } else {
                assert_eq!(
                    body["generationConfig"]["responseFormat"]["text"]["mimeType"],
                    "APPLICATION_JSON"
                );
                assert_eq!(body["tools"], json!([{"googleSearch":{}}]));
            }
            assert!(
                id.validate(&ProviderSettings::default(), AuthenticationMethod::ApiKey)
                    .is_err()
            );
            assert!(
                id.validate(&id.defaults(), AuthenticationMethod::Chatgpt)
                    .is_err()
            );
        }
        assert_ne!(
            ProviderId::Anthropic.service(),
            ProviderId::Google.service()
        );
        assert_eq!(ProviderId::Openai.service(), "io.desegni.openai");
    }
    async fn fixture(
        id: ProviderId,
        events: Vec<Value>,
        delay: bool,
    ) -> (KeyAdapter, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![];
            let mut chunk = [0; 2048];
            loop {
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(start) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..start]);
                    let len = header
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|s| s.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= start + 4 + len {
                        break;
                    }
                }
            }
            let request = String::from_utf8(bytes).unwrap();
            let body = events
                .iter()
                .map(|e| format!("data: {e}\n\n"))
                .collect::<String>();
            socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
            if delay {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            // Deliberately split every UTF-8 character and SSE boundary.
            for byte in body.as_bytes() {
                if socket.write_all(&[*byte]).await.is_err() {
                    break;
                }
            }
            request
        });
        (
            KeyAdapter::build(id, &format!("http://127.0.0.1:{port}"), false).unwrap(),
            task,
        )
    }
    #[tokio::test]
    async fn real_http_streams_use_provider_headers_and_support_cancel_without_retry() {
        for id in [ProviderId::Anthropic, ProviderId::Google] {
            let events = if id == ProviderId::Anthropic {
                anthropic("{\"ok\":true}")
            } else {
                google("{\"ok\":true}")
            };
            let (adapter, server) = fixture(id, events.clone(), false).await;
            let key = ApiKey::new("private-fixture-credential".into()).unwrap();
            let result = adapter
                .submit(
                    common(),
                    &id.defaults(),
                    &key,
                    &CancellationToken::new(),
                    &|_| {},
                )
                .await
                .unwrap();
            assert_eq!(result.text, "{\"ok\":true}");
            let request = server.await.unwrap();
            let first = request.lines().next().unwrap();
            assert!(!first.contains(key.expose()));
            assert!(request.contains(if id == ProviderId::Anthropic {
                "x-api-key: private-fixture-credential"
            } else {
                "x-goog-api-key: private-fixture-credential"
            }));
            assert!(!request.contains("authorization:"));
            let (adapter, server) = fixture(id, events, true).await;
            let token = CancellationToken::new();
            let cancellation = token.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                cancellation.cancel();
            });
            assert_eq!(
                adapter
                    .submit(common(), &id.defaults(), &key, &token, &|_| {})
                    .await
                    .unwrap_err()
                    .kind,
                ErrorKind::Cancelled
            );
            server.await.unwrap();
            let (adapter, server) = fixture(
                id,
                if id == ProviderId::Anthropic {
                    anthropic("{\"key\":\"private-fixture-credential\"}")
                } else {
                    google("{\"key\":\"private-fixture-credential\"}")
                },
                false,
            )
            .await;
            assert_eq!(
                adapter
                    .submit(
                        common(),
                        &id.defaults(),
                        &key,
                        &CancellationToken::new(),
                        &|_| {}
                    )
                    .await
                    .unwrap_err()
                    .kind,
                ErrorKind::Malformed
            );
            server.await.unwrap();
        }
    }
}

#[cfg(test)]
mod validation_repair_tests {
    use super::*;

    #[test]
    fn completed_bad_json_is_distinguished_from_an_interrupted_stream() {
        let mut stream = ForeignStream::new(ProviderId::Google);
        stream.response_id = Some("repair-fixture".into());
        stream.text = "{unfinished JSON".into();
        stream.stopped = true;
        let error = stream.finish().unwrap_err();
        assert_eq!(error.outcome, SubmissionOutcome::Received);
        assert_eq!(
            error.response_format.unwrap().rejection,
            Some(crate::ResponseRejection::CandidateJson)
        );
        let mut interrupted = ForeignStream::new(ProviderId::Google);
        interrupted.text = "{unfinished JSON".into();
        let error = interrupted.finish().unwrap_err();
        assert_ne!(error.outcome, SubmissionOutcome::Received);
        assert!(error.response_format.is_none());
    }
}
