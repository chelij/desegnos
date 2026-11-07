//! Native provider boundary. A completed, bounded JSON candidate is the only
//! output; server search commentary and citations never become candidate text.
use crate::request::{RequestContent, RequestPart};
use crate::{
    ApiKey, AuthenticationMethod, CancellationToken, CostEstimate, ErrorKind, IdentityImage,
    MAX_OUTPUT_BYTES, MAX_OUTPUT_TOKENS, MAX_REQUEST_BYTES, ModelInfo, OpenAiClient, ProviderError,
    ProviderProgress, ProviderResponse, ProviderSettings, ReasoningEffort, Result,
    SubmissionOutcome, TokenUsage,
    sse::{MAX_STREAM_BYTES, SseDecoder},
};
use desegnos_core::{GenerationCapabilities, GenerationMode, RequirementsSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, future::Future, pin::Pin, time::Duration};

pub const DEFAULT_LOCAL_ENDPOINT: &str = "http://127.0.0.1:11434/v1";
const DEEPSEEK_ENDPOINT: &str = "https://api.deepseek.com";
const DEEPSEEK_CONTEXT_TOKENS: u64 = 1_000_000;

pub fn local_endpoint(settings: &ProviderSettings) -> Result<String> {
    let mut url = reqwest::Url::parse(
        settings
            .endpoint
            .as_deref()
            .unwrap_or(DEFAULT_LOCAL_ENDPOINT),
    )
    .map_err(|_| ProviderError::before(ErrorKind::InvalidSettings))?;
    if url.host_str() == Some("localhost") {
        url.set_host(Some("127.0.0.1"))
            .map_err(|_| ProviderError::before(ErrorKind::InvalidSettings))?;
    }
    let local = url
        .host_str()
        .unwrap_or_default()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback());
    if !local
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().trim_end_matches('/') != "/v1"
    {
        return Err(ProviderError::before(ErrorKind::InvalidSettings));
    }
    Ok(url.as_str().trim_end_matches('/').into())
}

#[derive(ts_rs::TS, Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    #[default]
    Openai,
    Anthropic,
    Google,
    Openrouter,
    Deepseek,
    Local,
}
impl ProviderId {
    fn default_model(self) -> &'static str {
        match self {
            Self::Openai => crate::MODEL_ID,
            Self::Anthropic => "claude-sonnet-5-5",
            Self::Google => "gemini-3.8-flash",
            Self::Openrouter => "openai/gpt-6.1-sol",
            Self::Deepseek => "deepseek-flash",
            Self::Local => "local-model",
        }
    }
    pub fn service(self) -> &'static str {
        match self {
            Self::Openai => "io.desegnos.openai",
            Self::Anthropic => "io.desegnos.anthropic",
            Self::Google => "io.desegnos.google",
            Self::Openrouter => "io.desegnos.openrouter",
            Self::Deepseek => "io.desegnos.deepseek",
            Self::Local => "io.desegnos.local",
        }
    }
    pub fn capabilities(self) -> GenerationCapabilities {
        GenerationCapabilities {
            web_search: matches!(self, Self::Openai | Self::Anthropic | Self::Google),
            image_downloads: GenerationCapabilities::default().image_downloads,
            image_generation: (self == Self::Openai).then(Default::default),
        }
    }
    pub fn capabilities_for(
        self,
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
        let (model, reasoning_effort, max_output_tokens, endpoint) = match self {
            Self::Openai => (
                self.default_model(),
                ReasoningEffort::Medium,
                crate::DEFAULT_MAX_OUTPUT_TOKENS,
                None,
            ),
            Self::Anthropic => (
                self.default_model(),
                ReasoningEffort::Medium,
                crate::DEFAULT_MAX_OUTPUT_TOKENS,
                None,
            ),
            Self::Google => (
                self.default_model(),
                ReasoningEffort::Medium,
                crate::DEFAULT_MAX_OUTPUT_TOKENS,
                None,
            ),
            Self::Openrouter => (
                self.default_model(),
                ReasoningEffort::Medium,
                crate::DEFAULT_MAX_OUTPUT_TOKENS,
                None,
            ),
            // DeepSeek thinking is always on and counts inside max_tokens.
            Self::Deepseek => (
                self.default_model(),
                ReasoningEffort::High,
                crate::MAX_OUTPUT_TOKENS,
                None,
            ),
            Self::Local => (
                self.default_model(),
                ReasoningEffort::Medium,
                crate::DEFAULT_MAX_OUTPUT_TOKENS,
                Some(DEFAULT_LOCAL_ENDPOINT),
            ),
        };
        ProviderSettings {
            model: model.into(),
            reasoning_effort,
            max_output_tokens,
            endpoint: endpoint.map(str::to_owned),
            ..Default::default()
        }
    }
    pub fn validate(self, settings: &ProviderSettings, auth: AuthenticationMethod) -> Result<()> {
        if self == Self::Openai {
            return settings.validate_for_auth(auth);
        }
        if auth != AuthenticationMethod::ApiKey || !crate::settings::valid_model_id(&settings.model)
        {
            return Err(ProviderError::before(ErrorKind::InvalidSettings));
        }
        if self == Self::Local {
            local_endpoint(settings)?;
        }
        if self == Self::Deepseek && settings.endpoint.is_some() {
            return Err(ProviderError::before(ErrorKind::InvalidSettings));
        }
        settings.validate_limits(auth)
    }
    pub fn catalogue(self) -> Vec<ModelInfo> {
        if matches!(self, Self::Openrouter | Self::Local) {
            return vec![];
        }
        if self == Self::Openai {
            return crate::model_catalogue();
        }
        if self == Self::Deepseek {
            // Peak rates are used as the ceilings; discovery publishes neither price nor limit.
            // Desegnos never requests more than its own API-key output ceiling.
            return [
                (self.default_model(), "DeepSeek V4.1 Flash", 0.30, 1.20),
                ("deepseek-v4-pro", "DeepSeek V4 Pro", 1.32, 3.96),
            ]
            .into_iter()
            .map(|(id, name, input, output)| ModelInfo {
                id: id.into(),
                name: name.into(),
                input_usd_per_million: input,
                output_usd_per_million: output,
                verified_at: "2026-11-06".into(),
                source_url: "https://api-docs.deepseek.com/quick_start/pricing".into(),
                reasoning_efforts: vec![
                    ReasoningEffort::Low,
                    ReasoningEffort::High,
                    ReasoningEffort::Other("max".into()),
                ],
                context_window_tokens: DEEPSEEK_CONTEXT_TOKENS,
                max_output_tokens: MAX_OUTPUT_TOKENS,
                long_context_input_usd_per_million: input,
                long_context_output_usd_per_million: output,
                pricing_required: false,
                default_pricing: None,
            })
            .collect();
        }
        let (name, input, output, ceiling_input, ceiling_output, source, context) = match self {
            Self::Anthropic => (
                "Claude Sonnet 5.5",
                2.0,
                10.0,
                2.0,
                10.0,
                "https://platform.claude.com/docs/en/about-claude/pricing",
                1_000_000,
            ),
            _ => (
                "Gemini 3.8 Flash",
                0.75,
                3.75,
                1.5,
                7.5,
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
            long_context_input_usd_per_million: ceiling_input,
            long_context_output_usd_per_million: ceiling_output,
            pricing_required: false,
            default_pricing: None,
        }]
    }
}
#[derive(ts_rs::TS, Serialize)]
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
        (ProviderId::Deepseek, "DeepSeek"),
        (ProviderId::Local, "Local"),
    ]
    .into_iter()
    .map(|(id, name)| ProviderDescriptor {
        id,
        name,
        available: true,
        sign_in_supported: id == ProviderId::Openai,
        capabilities: id.capabilities(),
    })
    .collect()
}

/// A local server that rejected the anonymous key needs one entered, not a generic auth error.
pub fn models_need_key(
    provider: ProviderId,
    local_authentication: bool,
    error: &ProviderError,
) -> bool {
    provider == ProviderId::Local
        && !local_authentication
        && error.kind == ErrorKind::Authentication
}

/// Discovery failures reach the settings view as text; a server demanding a key says so.
pub fn models_error(
    provider: ProviderId,
    local_authentication: bool,
    error: &ProviderError,
) -> String {
    if models_need_key(provider, local_authentication, error) {
        "This server needs an API key. Add one from the account menu.".into()
    } else {
        error.to_string()
    }
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
/// One frozen brand-identity analysis: the source packet, its logo images and the account.
pub struct IdentityRequest<'a> {
    pub authentication: AuthenticationMethod,
    pub snapshot: &'a RequirementsSnapshot,
    pub schema: &'a Value,
    pub settings: &'a ProviderSettings,
    pub key: &'a ApiKey,
    pub cancellation: &'a CancellationToken,
    pub images: &'a [IdentityImage],
}
pub type ProviderFuture<'a> = Pin<Box<dyn Future<Output = Result<ProviderResponse>> + Send + 'a>>;
/// Provider differences stop here. No HTTP or credential details enter the UI.
pub trait ProviderAdapter: Send + Sync {
    fn generate<'a>(
        &'a self,
        request: GenerationRequest<'a>,
        progress: &'a (dyn Fn(ProviderProgress) + Send + Sync),
    ) -> ProviderFuture<'a>;
    fn analyze_identity<'a>(
        &'a self,
        request: IdentityRequest<'a>,
        progress: &'a (dyn Fn(ProviderProgress) + Send + Sync),
    ) -> ProviderFuture<'a>;
    fn estimate(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
        schema: &Value,
        settings: &ProviderSettings,
    ) -> Result<CostEstimate>;
    fn estimate_identity(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        schema: &Value,
        settings: &ProviderSettings,
        images: &[IdentityImage],
    ) -> Result<CostEstimate>;
    fn list_models<'a>(
        &'a self,
        key: &'a ApiKey,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ModelInfo>>> + Send + 'a>>;
    fn test_access<'a>(
        &'a self,
        settings: &'a ProviderSettings,
        key: &'a ApiKey,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
}
impl ProviderAdapter for OpenAiClient {
    fn generate<'a>(
        &'a self,
        r: GenerationRequest<'a>,
        p: &'a (dyn Fn(ProviderProgress) + Send + Sync),
    ) -> ProviderFuture<'a> {
        Box::pin(OpenAiClient::generate(self, r, p))
    }
    fn analyze_identity<'a>(
        &'a self,
        r: IdentityRequest<'a>,
        p: &'a (dyn Fn(ProviderProgress) + Send + Sync),
    ) -> ProviderFuture<'a> {
        Box::pin(OpenAiClient::analyze_identity(self, r, p))
    }
    fn estimate(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
        schema: &Value,
        settings: &ProviderSettings,
    ) -> Result<CostEstimate> {
        crate::estimate_for_auth(auth, snapshot, mode, schema, settings)
    }
    fn estimate_identity(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        schema: &Value,
        settings: &ProviderSettings,
        images: &[IdentityImage],
    ) -> Result<CostEstimate> {
        crate::estimate_identity_for_auth(auth, snapshot, schema, settings, images)
    }
    fn list_models<'a>(
        &'a self,
        key: &'a ApiKey,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ModelInfo>>> + Send + 'a>> {
        Box::pin(self.list_api_models(key, cancel))
    }
    fn test_access<'a>(
        &'a self,
        settings: &'a ProviderSettings,
        key: &'a ApiKey,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(self.test_model_access(settings, key, cancel))
    }
}
#[derive(Clone)]
pub struct ProviderClient {
    pub id: ProviderId,
    adapter: std::sync::Arc<dyn ProviderAdapter>,
}
impl ProviderClient {
    pub fn with_settings(
        id: ProviderId,
        openai: OpenAiClient,
        settings: &ProviderSettings,
    ) -> Result<Self> {
        let adapter: std::sync::Arc<dyn ProviderAdapter> = match id {
            ProviderId::Openai => std::sync::Arc::new(openai),
            ProviderId::Local => {
                std::sync::Arc::new(KeyAdapter::build(id, &local_endpoint(settings)?, false)?)
            }
            _ => std::sync::Arc::new(KeyAdapter::new(id)?),
        };
        Ok(Self { id, adapter })
    }
    pub fn new(id: ProviderId, openai: OpenAiClient) -> Result<Self> {
        Self::with_settings(id, openai, &id.defaults())
    }
    pub async fn list_api_models(
        &self,
        key: &ApiKey,
        cancel: &CancellationToken,
    ) -> Result<Vec<ModelInfo>> {
        self.adapter.list_models(key, cancel).await
    }
    pub fn estimate(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
        schema: &Value,
        settings: &ProviderSettings,
    ) -> Result<CostEstimate> {
        self.adapter
            .estimate(auth, snapshot, mode, schema, settings)
    }
    pub fn estimate_identity(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        schema: &Value,
        settings: &ProviderSettings,
        images: &[IdentityImage],
    ) -> Result<CostEstimate> {
        self.adapter
            .estimate_identity(auth, snapshot, schema, settings, images)
    }
    pub async fn test_model_access(
        &self,
        settings: &ProviderSettings,
        key: &ApiKey,
        cancel: &CancellationToken,
    ) -> Result<()> {
        self.adapter.test_access(settings, key, cancel).await
    }
    pub async fn generate(
        &self,
        request: GenerationRequest<'_>,
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        self.adapter.generate(request, &progress).await
    }
    pub async fn analyze_identity(
        &self,
        request: IdentityRequest<'_>,
        progress: impl Fn(ProviderProgress) + Send + Sync,
    ) -> Result<ProviderResponse> {
        self.adapter.analyze_identity(request, &progress).await
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
    let body = crate::identity::identity_content(snapshot, schema, images)?;
    foreign_estimate(
        provider,
        &body,
        settings,
        crate::identity::vision_reserve(images),
    )
}

fn foreign_estimate(
    id: ProviderId,
    body: &RequestContent<'_>,
    settings: &ProviderSettings,
    reserve: u64,
) -> Result<CostEstimate> {
    let wire = foreign_wire(id, body, settings)?;
    estimate_wire(id, body, settings, reserve, &wire)
}

fn estimate_wire(
    id: ProviderId,
    body: &RequestContent<'_>,
    settings: &ProviderSettings,
    reserve: u64,
    wire: &[u8],
) -> Result<CostEstimate> {
    if wire.len() > MAX_REQUEST_BYTES {
        return Err(ProviderError::before(ErrorKind::InputLimit));
    }
    // Cost keeps the byte-per-token bound, so spending is never underestimated; the context
    // window uses the two-bytes-per-token bound, so a request that fits is not refused.
    let input = wire.len() as u64 + reserve + 65_536;
    let window_input = wire.len() as u64 / crate::WINDOW_BYTES_PER_TOKEN + reserve + 65_536;
    let info = id
        .catalogue()
        .into_iter()
        .find(|model| model.id == settings.model);
    if window_input + u64::from(settings.max_output_tokens)
        > info.as_ref().map_or(crate::CONTEXT_WINDOW_TOKENS, |model| {
            model.context_window_tokens
        })
    {
        return Err(ProviderError::before(ErrorKind::InputLimit));
    }
    let (input_rate, output_rate, verified_at) = if let Some(pricing) = &settings.pricing {
        pricing.validate(&settings.model)?;
        (
            pricing.input_usd_per_million,
            pricing.output_usd_per_million,
            String::new(),
        )
    } else if let Some(info) = info {
        (
            info.long_context_input_usd_per_million,
            info.long_context_output_usd_per_million,
            info.verified_at,
        )
    } else {
        return Err(ProviderError::before(ErrorKind::InvalidSettings));
    };
    let fees = settings.pricing.as_ref().map_or(0.0, |pricing| {
        pricing.request_usd
            + pricing.image_usd
                * body
                    .parts
                    .iter()
                    .filter(|part| matches!(part, RequestPart::Image { .. }))
                    .count() as f64
    });
    let estimate = (((input as f64 * input_rate
        + f64::from(settings.max_output_tokens) * output_rate)
        / 1_000_000.0
        + fees)
        * 100.0)
        .ceil()
        / 100.0;
    if estimate > settings.spending_limit_usd {
        return Err(ProviderError::before(ErrorKind::SpendingLimit));
    }
    Ok(CostEstimate {
        estimate_usd: estimate,
        input_token_upper_bound: input,
        verified_at,
    })
}

#[derive(Clone)]
struct KeyAdapter {
    id: ProviderId,
    client: reqwest::Client,
    endpoint: String,
    deadline: Duration,
}
use crate::net::{DEADLINE, ERROR_BODY_LIMIT, IDLE as PROVIDER_IDLE, PROBE_IDLE};
impl KeyAdapter {
    fn new(id: ProviderId) -> Result<Self> {
        let endpoint = match id {
            ProviderId::Anthropic => "https://api.anthropic.com/v1",
            ProviderId::Google => "https://generativelanguage.googleapis.com/v1beta",
            ProviderId::Openrouter => "https://openrouter.ai/api/v1",
            ProviderId::Deepseek => DEEPSEEK_ENDPOINT,
            ProviderId::Local => DEFAULT_LOCAL_ENDPOINT,
            _ => return Err(ProviderError::before(ErrorKind::InvalidSettings)),
        };
        Self::build(id, endpoint, id != ProviderId::Local)
    }
    fn build(id: ProviderId, endpoint: &str, https: bool) -> Result<Self> {
        // Same limits as the OpenAI transport: silence for 90 s or 20 minutes overall.
        let deadline = DEADLINE;
        let client = crate::net::hardened_client(https)
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
        crate::model_discovery::authorize(self.id, request, key)
    }
    async fn test_access(
        &self,
        s: &ProviderSettings,
        key: &ApiKey,
        cancel: &CancellationToken,
    ) -> Result<()> {
        if matches!(
            self.id,
            ProviderId::Openrouter | ProviderId::Deepseek | ProviderId::Local
        ) {
            let models = crate::model_discovery::list_models(
                self.id,
                &self.client,
                &self.endpoint,
                key,
                cancel,
            )
            .await?;
            return if models.iter().any(|model| model.id == s.model) {
                Ok(())
            } else {
                Err(ProviderError::before(ErrorKind::ModelUnavailable))
            };
        }
        let path = format!("{}/models/{}", self.endpoint, s.model);
        let mut response = crate::retry::send(
            self.authorized(self.client.get(path), key)?,
            cancel,
            crate::retry::RetryPolicy::new(
                tokio::time::Instant::now() + PROVIDER_IDLE,
                PROBE_IDLE,
                ERROR_BODY_LIMIT,
            ),
            &|_| {},
        )
        .await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(read_http_error(
                &mut response,
                key,
                cancel,
                tokio::time::Instant::now() + PROVIDER_IDLE,
            )
            .await)
        }
    }
    async fn submit(
        &self,
        common: RequestContent<'_>,
        reserve: u64,
        s: &ProviderSettings,
        key: &ApiKey,
        cancel: &CancellationToken,
        progress: &(dyn Fn(ProviderProgress) + Send + Sync),
    ) -> Result<ProviderResponse> {
        let body = foreign_wire(self.id, &common, s)?;
        estimate_wire(self.id, &common, s, reserve, &body)?;
        if body.len() > MAX_REQUEST_BYTES
            || key.appears_in(
                std::str::from_utf8(&body)
                    .map_err(|_| ProviderError::before(ErrorKind::InvalidRequirements))?,
            )
        {
            return Err(ProviderError::before(ErrorKind::InputLimit));
        }
        let path = match self.id {
            ProviderId::Anthropic => format!("{}/messages", self.endpoint),
            ProviderId::Openrouter | ProviderId::Deepseek | ProviderId::Local => {
                format!("{}/chat/completions", self.endpoint)
            }
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
        let mut response = crate::retry::send(
            request,
            cancel,
            crate::retry::RetryPolicy::new(deadline, PROVIDER_IDLE, MAX_STREAM_BYTES),
            progress,
        )
        .await?;
        if !response.status().is_success() {
            return Err(read_http_error(&mut response, key, cancel, deadline).await);
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
                stream.accept(&frame.data).map_err(|error| {
                    let value = serde_json::from_str::<Value>(&frame.data).ok();
                    if let Some(value) = value.filter(|value| value.get("error").is_some()) {
                        let code = value["error"]["type"]
                            .as_str()
                            .or_else(|| value["error"]["status"].as_str());
                        error
                            .with_provider_metadata(key, None, code, None, None)
                            .with_response_format(
                                key,
                                Some("text/event-stream"),
                                crate::ResponseBodyKind::JsonError,
                                frame.data.len(),
                                crate::transport::provider_message(&value),
                            )
                            .with_diagnostic_reply(&frame.data, key)
                    } else {
                        error
                    }
                })?;
            }
            if key.appears_in(&stream.text) {
                return Err(stream.error(ErrorKind::Malformed));
            }
            if stream
                .response_id
                .as_ref()
                .is_some_and(|id| key.appears_in(id))
            {
                return Err(stream.error(ErrorKind::Malformed));
            }
            progress(ProviderProgress {
                retry: None,
                received_bytes: bytes,
                response_id: stream.response_id.clone(),
                web_search_calls: stream.searches,
                citations: stream.citations,
                text_delta: std::mem::take(&mut stream.text_delta),
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
            let body = crate::settings::request_content(r.snapshot, r.mode, r.schema)?;
            self.submit(body, 0, r.settings, r.key, r.cancellation, p)
                .await
        })
    }
    fn analyze_identity<'a>(
        &'a self,
        r: IdentityRequest<'a>,
        p: &'a (dyn Fn(ProviderProgress) + Send + Sync),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.id.validate(r.settings, r.authentication)?;
            let body = crate::identity::identity_content(r.snapshot, r.schema, r.images)?;
            self.submit(
                body,
                crate::identity::vision_reserve(r.images),
                r.settings,
                r.key,
                r.cancellation,
                p,
            )
            .await
        })
    }
    fn estimate(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
        schema: &Value,
        settings: &ProviderSettings,
    ) -> Result<CostEstimate> {
        self.id.validate(settings, auth)?;
        let (counted, reserve) = crate::settings::without_previews(snapshot);
        let body = crate::settings::request_content(&counted, mode, schema)?;
        foreign_estimate(self.id, &body, settings, reserve)
    }
    fn estimate_identity(
        &self,
        auth: AuthenticationMethod,
        snapshot: &RequirementsSnapshot,
        schema: &Value,
        settings: &ProviderSettings,
        images: &[IdentityImage],
    ) -> Result<CostEstimate> {
        self.id.validate(settings, auth)?;
        let body = crate::identity::identity_content(snapshot, schema, images)?;
        foreign_estimate(
            self.id,
            &body,
            settings,
            crate::identity::vision_reserve(images),
        )
    }
    fn list_models<'a>(
        &'a self,
        key: &'a ApiKey,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ModelInfo>>> + Send + 'a>> {
        Box::pin(crate::model_discovery::list_models(
            self.id,
            &self.client,
            &self.endpoint,
            key,
            cancel,
        ))
    }
    fn test_access<'a>(
        &'a self,
        settings: &'a ProviderSettings,
        key: &'a ApiKey,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            self.id.validate(settings, AuthenticationMethod::ApiKey)?;
            KeyAdapter::test_access(self, settings, key, cancel).await
        })
    }
}

fn failure(kind: ErrorKind) -> ProviderError {
    ProviderError::new(kind, SubmissionOutcome::Unknown)
}
pub(crate) fn http_error(status: u16) -> ProviderError {
    let kind = crate::net::status_kind(status);
    let mut error = ProviderError::new(kind, SubmissionOutcome::Rejected);
    error.http_status = Some(status);
    error
}

async fn read_http_error(
    response: &mut crate::retry::RetryResponse,
    key: &ApiKey,
    cancel: &CancellationToken,
    deadline: tokio::time::Instant,
) -> ProviderError {
    let status = response.status().as_u16();
    let request_id = response
        .headers()
        .get("request-id")
        .or_else(|| response.headers().get("x-request-id"))
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = match crate::net::bounded_body(response, ERROR_BODY_LIMIT, cancel, deadline).await {
        Ok(body) => body,
        Err(_) => {
            return http_error(status).with_provider_metadata(
                key,
                Some(status),
                None,
                None,
                request_id.as_deref(),
            );
        }
    };
    foreign_http_error(
        status,
        &body,
        key,
        content_type.as_deref(),
        request_id.as_deref(),
    )
}

pub(crate) fn foreign_http_error(
    status: u16,
    body: &[u8],
    key: &ApiKey,
    content_type: Option<&str>,
    request_id: Option<&str>,
) -> ProviderError {
    let value = serde_json::from_slice::<Value>(body).ok();
    let error = value.as_ref().and_then(|v| v.get("error"));
    // Anthropic uses error.type, while Google uses a numeric error.code and string status.
    let code = error
        .and_then(|v| {
            v["code"]
                .as_str()
                .or_else(|| v["type"].as_str())
                .or_else(|| v["status"].as_str())
        })
        .or_else(|| value.as_ref().and_then(|v| v["code"].as_str()));
    http_error(status)
        .with_provider_metadata(key, Some(status), code, None, request_id)
        .with_response_format(
            key,
            content_type,
            crate::transport::response_body_kind(body, value.as_ref()),
            body.len(),
            value.as_ref().and_then(crate::transport::provider_message),
        )
        .with_diagnostic_reply(&String::from_utf8_lossy(body), key)
}

// Claude strict tools reject recursive schemas and validation bounds. Keep the original
// recursive tool schema in ordinary tool mode; local candidate validation still applies.
// Remove this fallback when Claude supports recursive strict schemas.
fn anthropic_schema(schema: &Value) -> (Value, bool) {
    fn recursive(node: &Value, root: &Value, refs: &mut Vec<String>) -> bool {
        if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
            if refs.iter().any(|r| r == reference) {
                return true;
            }
            if let Some(target) = reference.strip_prefix('#').and_then(|p| root.pointer(p)) {
                refs.push(reference.into());
                let found = recursive(target, root, refs);
                refs.pop();
                if found {
                    return true;
                }
            }
        }
        match node {
            Value::Object(map) => map
                .iter()
                .any(|(key, value)| key != "$defs" && recursive(value, root, refs)),
            Value::Array(values) => values.iter().any(|v| recursive(v, root, refs)),
            _ => false,
        }
    }
    fn simplify(node: &mut Value) {
        let Some(map) = node.as_object_mut() else {
            return;
        };
        let mut limits = Vec::new();
        for key in [
            "minimum",
            "maximum",
            "exclusiveMinimum",
            "exclusiveMaximum",
            "multipleOf",
            "minLength",
            "maxLength",
            "maxItems",
        ] {
            if let Some(value) = map.remove(key) {
                limits.push(format!("{key}: {value}"));
            }
        }
        if map
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|n| n > 1)
        {
            limits.push(format!("minItems: {}", map.remove("minItems").unwrap()));
        }
        if !limits.is_empty() {
            let description = map
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default();
            map.insert(
                "description".into(),
                json!(format!(
                    "{description} Constraints enforced by Desegnos: {}.",
                    limits.join(", ")
                )),
            );
        }
        for key in ["properties", "$defs", "definitions"] {
            if let Some(properties) = map.get_mut(key).and_then(Value::as_object_mut) {
                for value in properties.values_mut() {
                    simplify(value);
                }
            }
        }
        if let Some(items) = map.get_mut("items") {
            simplify(items);
        }
        for key in ["anyOf", "oneOf", "allOf"] {
            if let Some(branches) = map.get_mut(key).and_then(Value::as_array_mut) {
                for branch in branches {
                    simplify(branch);
                }
            }
        }
    }
    if recursive(schema, schema, &mut Vec::new()) {
        return (schema.clone(), false);
    }
    let mut schema = schema.clone();
    simplify(&mut schema);
    (schema, true)
}

fn foreign_wire(
    id: ProviderId,
    request: &RequestContent<'_>,
    settings: &ProviderSettings,
) -> Result<Vec<u8>> {
    crate::schema_wire::serialize_body(&foreign_body(id, request, settings)?)
        .map_err(|_| ProviderError::before(ErrorKind::InvalidRequirements))
}

#[derive(Clone, Copy)]
enum WireProtocol {
    Chat {
        schema_in_prompt: bool,
        require_parameters: bool,
    },
    Google,
    Anthropic,
}
impl ProviderId {
    fn wire_protocol(self) -> Result<WireProtocol> {
        Ok(match self {
            Self::Openrouter => WireProtocol::Chat {
                schema_in_prompt: false,
                require_parameters: true,
            },
            Self::Deepseek => WireProtocol::Chat {
                schema_in_prompt: true,
                require_parameters: false,
            },
            Self::Local => WireProtocol::Chat {
                schema_in_prompt: false,
                require_parameters: false,
            },
            Self::Google => WireProtocol::Google,
            Self::Anthropic => WireProtocol::Anthropic,
            Self::Openai => return Err(ProviderError::before(ErrorKind::InvalidSettings)),
        })
    }
}
pub(crate) fn foreign_body(
    id: ProviderId,
    request: &RequestContent<'_>,
    settings: &ProviderSettings,
) -> Result<Value> {
    match id.wire_protocol()? {
        WireProtocol::Chat {
            schema_in_prompt,
            require_parameters,
        } => chat_body(request, settings, schema_in_prompt, require_parameters),
        WireProtocol::Google => google_body(request, settings),
        WireProtocol::Anthropic => anthropic_body(request, settings),
    }
}
fn chat_body(
    request: &RequestContent<'_>,
    s: &ProviderSettings,
    schema_in_prompt: bool,
    require_parameters: bool,
) -> Result<Value> {
    if request.web_search {
        return Err(ProviderError::before(ErrorKind::InvalidRequirements));
    }
    let content = request
        .parts
        .iter()
        .map(|part| {
            Ok(match part {
                RequestPart::Document { text, .. } => json!({"type":"text","text":text}),
                RequestPart::Text(text) => json!({"type":"text","text":text}),
                RequestPart::Image { uri, detail } => {
                    image_data(uri)?;
                    json!({"type":"image_url","image_url":{"url":uri,"detail":detail}})
                }
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // DeepSeek only accepts json_object: local validation enforces the schema in the prompt.
    let system = if schema_in_prompt {
        format!(
            "{}\n\nReply with one JSON object only that conforms to the JSON Schema below. Do not wrap it in prose or a code fence.\n{}",
            request.instructions, request.schema
        )
    } else {
        request.instructions.to_owned()
    };
    let response_format = if schema_in_prompt {
        json!({"type":"json_object"})
    } else {
        json!({"type":"json_schema","json_schema":{"name":request.schema_name,"schema":request.schema,"strict":true}})
    };
    let mut body = json!({"model":s.model,"stream":true,"stream_options":{"include_usage":true},"max_tokens":s.max_output_tokens,
        "messages":[{"role":"system","content":system},{"role":"user","content":content}],"response_format":response_format});
    if s.reasoning_effort.as_str() != "none" {
        body["reasoning_effort"] = json!(if schema_in_prompt {
            deepseek_reasoning_effort(&s.reasoning_effort)
        } else {
            s.reasoning_effort.as_str()
        });
    }
    if require_parameters {
        body["provider"] = json!({"require_parameters":true,"allow_fallbacks":false});
    }
    Ok(body)
}
fn image_data(uri: &str) -> Result<(&str, &str)> {
    uri.strip_prefix("data:")
        .and_then(|uri| uri.split_once(";base64,"))
        .ok_or_else(|| ProviderError::before(ErrorKind::InvalidRequirements))
}
fn content_parts(
    request: &RequestContent<'_>,
    text: impl Fn(&str) -> Value,
    image: impl Fn(&str, &str) -> Value,
) -> Result<Vec<Value>> {
    request
        .parts
        .iter()
        .map(|part| {
            Ok(match part {
                RequestPart::Document { text: value, .. } => text(value),
                RequestPart::Text(value) => text(value),
                RequestPart::Image { uri, .. } => {
                    let (mime, data) = image_data(uri)?;
                    image(mime, data)
                }
            })
        })
        .collect()
}
fn google_body(request: &RequestContent<'_>, s: &ProviderSettings) -> Result<Value> {
    let parts = content_parts(
        request,
        |text| json!({"text":text}),
        |mime, data| json!({"inlineData":{"mimeType":mime,"data":data}}),
    )?;
    let mut body = json!({"systemInstruction":{"parts":[{"text":request.instructions}]},"contents":[{"role":"user","parts":parts}],"generationConfig":{"maxOutputTokens":s.max_output_tokens,"responseFormat":{"text":{"mimeType":"APPLICATION_JSON","schema":request.schema}}}});
    if s.reasoning_effort.as_str() != "none" {
        body["generationConfig"]["thinkingConfig"] =
            json!({"thinkingLevel":s.reasoning_effort.as_str().to_ascii_uppercase()});
    }
    if request.web_search {
        body["tools"] = json!([{"googleSearch":{}}]);
    }
    Ok(body)
}
fn anthropic_body(request: &RequestContent<'_>, s: &ProviderSettings) -> Result<Value> {
    let parts = content_parts(
        request,
        |text| json!({"type":"text","text":text}),
        |mime, data| json!({"type":"image","source":{"type":"base64","media_type":mime,"data":data}}),
    )?;
    let (schema, strict) = anthropic_schema(&request.schema);
    let mut tools = vec![
        json!({"name":"submit_result","description":"Return the complete requested structured result exactly once. Do not execute this tool; its input is the final answer.","input_schema":schema,"strict":strict}),
    ];
    if request.web_search {
        tools.push(json!({"type":"web_search_20250305","name":"web_search","max_uses":5}));
    }
    let mut body = json!({"model":s.model,"max_tokens":s.max_output_tokens,"stream":true,"system":format!("{}\nAfter any research, submit the final candidate using submit_result exactly once. Its input must match the schema. Do not call any other client tool.",request.instructions),"messages":[{"role":"user","content":parts}],"tools":tools,"tool_choice":{"type":"auto","disable_parallel_tool_use":true}});
    if s.reasoning_effort.as_str() != "none" {
        body["output_config"] = json!({"effort":s.reasoning_effort});
        if ProviderId::Anthropic
            .catalogue()
            .iter()
            .any(|model| model.id == s.model)
        {
            body["thinking"] = json!({"type":"between_tools"});
        }
    }
    Ok(body)
}

/// DeepSeek names three effort levels; Desegnos's finer levels map onto the nearest.
pub fn deepseek_reasoning_effort(effort: &ReasoningEffort) -> &'static str {
    match effort.as_str() {
        "max" => "max",
        "medium" | "high" | "xhigh" => "high",
        _ => "low",
    }
}

struct Block {
    kind: String,
    closed: bool,
}
/// How far a foreign stream got, from open until its terminal marker.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StreamState {
    /// Frames still arrive and the answer text may still grow.
    Open,
    /// The provider reported its stop reason; only the terminal marker may follow.
    Stopped,
    /// The terminal marker arrived; nothing may follow.
    Terminal,
}
impl StreamState {
    /// Whether the provider reported its stop reason, so no more text may arrive.
    fn stopped(self) -> bool {
        !matches!(self, Self::Open)
    }
}
struct ForeignStream {
    provider: ProviderId,
    response_id: Option<String>,
    text: String,
    /// Text not yet handed to a progress event.
    text_delta: String,
    usage: Option<TokenUsage>,
    anthropic_tokens: [u64; 4],
    blocks: BTreeMap<u64, Block>,
    result: Option<u64>,
    state: StreamState,
    searches: usize,
    citations: usize,
}
impl ForeignStream {
    fn new(provider: ProviderId) -> Self {
        Self {
            provider,
            response_id: None,
            text: String::new(),
            text_delta: String::new(),
            usage: None,
            anthropic_tokens: [0; 4],
            blocks: BTreeMap::new(),
            result: None,
            state: StreamState::Open,
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
        self.text_delta.push_str(text);
        Ok(())
    }
    fn accept(&mut self, data: &str) -> Result<()> {
        let protocol = self.provider.wire_protocol()?;
        if matches!(protocol, WireProtocol::Chat { .. }) && data.trim() == "[DONE]" {
            if self.state != StreamState::Stopped {
                return Err(self.error(ErrorKind::Truncated));
            }
            self.state = StreamState::Terminal;
            return Ok(());
        }
        let v: Value = serde_json::from_str(data).map_err(|_| self.error(ErrorKind::Malformed))?;
        if v.get("error").is_some() || v["type"] == "error" {
            return Err(self.error(ErrorKind::Transport));
        }
        if self.state == StreamState::Terminal {
            return Err(self.error(ErrorKind::Malformed));
        }
        match protocol {
            WireProtocol::Google => return self.google(v),
            WireProtocol::Chat { .. } => return self.chat(v),
            WireProtocol::Anthropic => {}
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
                    || self.state.stopped()
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
                if self.state.stopped()
                    || self.blocks.values().any(|b| !b.closed)
                    || self.result.is_none()
                    || v["delta"]["stop_reason"] != "tool_use"
                {
                    return Err(self.error(ErrorKind::Incomplete));
                }
                self.usage = Some(anthropic_usage(&v["usage"], &mut self.anthropic_tokens)?);
                self.state = StreamState::Stopped;
            }
            Some("message_stop") => {
                if self.state != StreamState::Stopped {
                    return Err(self.error(ErrorKind::Truncated));
                }
                self.state = StreamState::Terminal;
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
                        if self.state.stopped() {
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
                if self.state.stopped() || reason != "STOP" {
                    return Err(self.error(ErrorKind::Incomplete));
                }
                self.state = StreamState::Stopped;
            }
        } else if v.get("usageMetadata").is_none() {
            return Err(self.error(ErrorKind::Malformed));
        }
        Ok(())
    }
    fn chat(&mut self, value: Value) -> Result<()> {
        if let Some(id) = value["id"].as_str() {
            if id.is_empty()
                || id.len() > 256
                || id.chars().any(char::is_control)
                || self.response_id.as_ref().is_some_and(|prior| prior != id)
            {
                return Err(self.error(ErrorKind::Malformed));
            }
            self.response_id = Some(id.into());
        }
        if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
            let input = usage["prompt_tokens"]
                .as_u64()
                .ok_or_else(|| self.error(ErrorKind::Malformed))?;
            let output = usage["completion_tokens"]
                .as_u64()
                .ok_or_else(|| self.error(ErrorKind::Malformed))?;
            let total = input
                .checked_add(output)
                .ok_or_else(|| self.error(ErrorKind::Malformed))?;
            if usage
                .get("total_tokens")
                .is_some_and(|reported| reported.as_u64() != Some(total))
            {
                return Err(self.error(ErrorKind::Malformed));
            }
            self.usage = Some(TokenUsage {
                input_tokens: input,
                output_tokens: output,
                total_tokens: total,
            });
        }
        let choices = value["choices"]
            .as_array()
            .ok_or_else(|| self.error(ErrorKind::Malformed))?;
        if choices.is_empty() {
            return if value.get("usage").is_some() {
                Ok(())
            } else {
                Err(self.error(ErrorKind::Malformed))
            };
        }
        if choices.len() != 1 || choices[0]["index"] != 0 || self.state.stopped() {
            return Err(self.error(ErrorKind::Malformed));
        }
        let delta = &choices[0]["delta"];
        if delta.get("tool_calls").is_some() || delta.get("function_call").is_some() {
            return Err(self.error(ErrorKind::Malformed));
        }
        if delta
            .get("refusal")
            .is_some_and(|refusal| !refusal.is_null())
        {
            return Err(self.error(ErrorKind::Refused));
        }
        match delta.get("content") {
            Some(Value::String(text)) => self.append(text)?,
            None | Some(Value::Null) => {}
            _ => return Err(self.error(ErrorKind::Malformed)),
        }
        if let Some(reason) = choices[0]["finish_reason"].as_str() {
            if reason != "stop" {
                return Err(self.error(if reason == "content_filter" {
                    ErrorKind::Refused
                } else {
                    ErrorKind::Incomplete
                }));
            }
            self.state = StreamState::Stopped;
        }
        Ok(())
    }

    fn finish(self) -> Result<ProviderResponse> {
        // Google ends its stream with the finish reason; every other provider also sends a
        // separate terminal marker.
        let complete = match self.provider.wire_protocol()? {
            WireProtocol::Google => self.state.stopped(),
            _ => self.state == StreamState::Terminal,
        };
        if !complete {
            return Err(self.error(ErrorKind::Truncated));
        }
        if self.response_id.is_none() {
            return Err(self.error(ErrorKind::Malformed));
        }
        // A blank reply is unparseable output, never a usable answer: DeepSeek may answer with
        // empty content even at finish_reason "stop".
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
    fn chat_chunk(text: &str, reason: Value) -> Value {
        json!({"id":"chat_fixture","choices":[{"index":0,"delta":{"content":text},"finish_reason":reason}]})
    }

    #[test]
    fn local_addresses_are_normalized_and_restricted_to_loopback() {
        assert_eq!(
            local_endpoint(&ProviderId::Local.defaults()).unwrap(),
            DEFAULT_LOCAL_ENDPOINT
        );
        for (address, expected) in [
            ("http://localhost:8080/v1/", "http://127.0.0.1:8080/v1"),
            ("http://[::1]:1234/v1", "http://[::1]:1234/v1"),
        ] {
            assert_eq!(
                local_endpoint(&ProviderSettings {
                    endpoint: Some(address.into()),
                    ..ProviderId::Local.defaults()
                })
                .unwrap(),
                expected
            );
        }
        for address in [
            "http://example.com/v1",
            "http://192.168.1.1/v1",
            "file:///v1",
            "http://user:secret@localhost/v1",
            "http://localhost/v1?key=secret",
            "http://localhost/v1#fragment",
            "http://localhost/v2",
        ] {
            assert!(
                local_endpoint(&ProviderSettings {
                    endpoint: Some(address.into()),
                    ..ProviderId::Local.defaults()
                })
                .is_err(),
                "{address}"
            );
        }
    }

    #[test]
    fn chat_stream_requires_one_complete_json_answer_and_a_terminal_marker() {
        for provider in [
            ProviderId::Openrouter,
            ProviderId::Deepseek,
            ProviderId::Local,
        ] {
            let mut stream = ForeignStream::new(provider);
            let mut first = chat_chunk("{\"html\":", Value::Null);
            first["choices"][0]["delta"]["reasoning"] = json!("private thoughts");
            stream.accept(&first.to_string()).unwrap();
            stream
                .accept(&chat_chunk("\"Création 💡\"}", json!("stop")).to_string())
                .unwrap();
            stream.accept(&json!({"id":"chat_fixture","choices":[],"usage":{"prompt_tokens":42,"completion_tokens":12,"total_tokens":54}}).to_string()).unwrap();
            stream.accept("[DONE]").unwrap();
            let answer = stream.finish().unwrap();
            assert_eq!(answer.text, "{\"html\":\"Création 💡\"}");
            assert_eq!(answer.usage.unwrap().total_tokens, 54);
            let mut stream = ForeignStream::new(provider);
            stream
                .accept(&chat_chunk("{}", json!("stop")).to_string())
                .unwrap();
            assert_eq!(stream.finish().unwrap_err().kind, ErrorKind::Truncated);
            assert!(ForeignStream::new(provider).accept("[DONE]").is_err());
            for event in [
                chat_chunk("{}", json!("length")),
                json!({"id":"chat_fixture","choices":[{"index":0,"delta":{"tool_calls":[]}}]}),
                json!({"id":"chat_fixture","choices":[{"index":0,"delta":{"refusal":"No"}}]}),
                json!({"id":"chat_fixture","choices":[],"usage":{"prompt_tokens":42,"completion_tokens":12,"total_tokens":1}}),
            ] {
                assert!(
                    ForeignStream::new(provider)
                        .accept(&event.to_string())
                        .is_err()
                );
            }
            let mut stream = ForeignStream::new(provider);
            stream
                .accept(&chat_chunk("{}", json!("stop")).to_string())
                .unwrap();
            stream.accept("[DONE]").unwrap();
            assert!(stream.accept("[DONE]").is_err());
        }
    }

    #[test]
    fn deepseek_blank_reply_is_rejected_as_invalid_output() {
        for text in ["", "   ", "\r\n\t"] {
            let mut stream = ForeignStream::new(ProviderId::Deepseek);
            stream
                .accept(&chat_chunk(text, json!("stop")).to_string())
                .unwrap();
            stream.accept("[DONE]").unwrap();
            let error = stream.finish().unwrap_err();
            assert_eq!(error.kind, ErrorKind::Malformed);
            assert_eq!(error.outcome, SubmissionOutcome::Received);
            assert_eq!(
                error.response_format.unwrap().rejection,
                Some(crate::ResponseRejection::CandidateJson)
            );
        }
    }

    #[tokio::test]
    async fn chat_transports_preserve_requests_and_optional_local_authentication() {
        for (provider, key) in [
            (
                ProviderId::Openrouter,
                ApiKey::new("synthetic-chat-key".into()).unwrap(),
            ),
            (
                ProviderId::Deepseek,
                ApiKey::new("synthetic-deepseek-key".into()).unwrap(),
            ),
            (ProviderId::Local, ApiKey::local_anonymous()),
            (
                ProviderId::Local,
                ApiKey::new("synthetic-local-key".into()).unwrap(),
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_owned)
                            })
                            .unwrap()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let data = format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    chat_chunk("{}", json!("stop"))
                );
                socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{data}",data.len()).as_bytes()).await.unwrap();
                bytes
            });
            let snapshot = crate::tests::snapshot(GenerationMode::Strict);
            let schema = desegnos_core::candidate_schema();
            let mut packet =
                crate::settings::request_content(&snapshot, GenerationMode::Strict, &schema)
                    .unwrap();
            packet.web_search = false;
            packet.parts.push(RequestPart::Image {
                uri: "data:image/png;base64,AQ==",
                detail: "low",
            });
            let settings = ProviderSettings {
                model: "vendor/future-model:free".into(),
                reasoning_effort: ReasoningEffort::Other("none".into()),
                pricing: Some(crate::ModelPricing {
                    model: "vendor/future-model:free".into(),
                    ..Default::default()
                }),
                ..provider.defaults()
            };
            let adapter = KeyAdapter::build(provider, &endpoint, false).unwrap();
            let mut sent =
                crate::settings::request_content(&snapshot, GenerationMode::Strict, &schema)
                    .unwrap();
            sent.web_search = false;
            sent.parts.push(RequestPart::Image {
                uri: "data:image/png;base64,AQ==",
                detail: "low",
            });
            let response = adapter
                .submit(sent, 0, &settings, &key, &CancellationToken::new(), &|_| {})
                .await
                .unwrap();
            assert_eq!(response.text, "{}");
            let bytes = server.await.unwrap();
            let request = std::str::from_utf8(&bytes).unwrap();
            assert!(request.starts_with("POST /v1/chat/completions "));
            assert_eq!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer"),
                key.expose() != crate::credentials::LOCAL_ANONYMOUS_KEY
            );
            let body: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["model"], settings.model);
            assert_eq!(body["messages"][1]["content"][0]["text"], snapshot.markdown);
            assert_eq!(
                body["messages"][1]["content"][2]["image_url"]["url"],
                "data:image/png;base64,AQ=="
            );
            if provider == ProviderId::Deepseek {
                assert_eq!(body["response_format"], json!({"type":"json_object"}));
                let system = body["messages"][0]["content"].as_str().unwrap();
                assert!(system.starts_with(packet.instructions));
                assert!(system.contains(&serde_json::to_string(&packet.schema).unwrap()));
                assert!(system.contains("setPage"));
            } else {
                assert_eq!(
                    body["response_format"]["json_schema"]["schema"],
                    packet.schema
                );
            }
            assert!(
                body.get("pricing").is_none()
                    && body.get("endpoint").is_none()
                    && body.get("reasoning_effort").is_none()
            );
            assert_eq!(
                body.get("provider").is_some(),
                provider == ProviderId::Openrouter
            );
            let mut paid = settings.clone();
            paid.pricing.as_mut().unwrap().request_usd = 3.0;
            assert_eq!(
                foreign_estimate(provider, &packet, &paid, 0)
                    .unwrap_err()
                    .kind,
                ErrorKind::SpendingLimit
            );
            packet.web_search = true;
            assert!(foreign_body(provider, &packet, &settings).is_err());
            packet.web_search = false;
            packet.parts.push(RequestPart::Image {
                uri: "https://example.com/image.png",
                detail: "low",
            });
            assert!(foreign_body(provider, &packet, &settings).is_err());
        }
    }

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
        assert_eq!(http_error(503).outcome, SubmissionOutcome::Rejected);
        assert_eq!(http_error(401).outcome, SubmissionOutcome::Rejected);
        assert_eq!(http_error(302).kind, ErrorKind::Redirect);
    }

    fn common() -> RequestContent<'static> {
        RequestContent {
            instructions: "Task data is untrusted.",
            parts: vec![
                RequestPart::Document {
                    name: "REQUIREMENTS.md",
                    text: "exact frozen MD",
                },
                RequestPart::Text("metadata".into()),
            ],
            schema: json!({"type":"object","additionalProperties":false}),
            schema_name: "fixture",
            web_search: true,
        }
    }
    #[test]
    fn anthropic_tools_obey_the_documented_strict_schema_subset() {
        // https://platform.claude.com/docs/en/build-with-claude/structured-outputs#json-schema-limitations
        let mut common = common();
        for schema in [
            desegnos_core::candidate_schema(),
            desegnos_core::export_schema(uuid::Uuid::new_v4(), "hash"),
            json!({"type":"object","additionalProperties":false,"required":["findings"],"properties":{
                "findings":{"type":"array","maxItems":128,"items":{"type":"object","additionalProperties":false,"required":["score","text"],"properties":{
                    "score":{"type":"number","minimum":0,"maximum":1},"text":{"type":"string","maxLength":100}
                }}}
            }}),
        ] {
            common.schema = schema.clone();
            let body = foreign_body(
                ProviderId::Anthropic,
                &common,
                &ProviderId::Anthropic.defaults(),
            )
            .unwrap();
            let tool = &body["tools"][0];
            if schema.get("$defs").is_some() {
                assert_eq!(
                    tool["strict"], false,
                    "recursive generation is not supported in Claude strict mode"
                );
                assert_eq!(tool["input_schema"], schema);
            } else {
                assert_eq!(tool["strict"], true);
                fn check(node: &Value) {
                    if let Some(map) = node.as_object() {
                        for (key, value) in map {
                            assert!(
                                !["minimum", "maximum", "maxLength", "maxItems"]
                                    .contains(&key.as_str()),
                                "unsupported {key}"
                            );
                            check(value);
                        }
                    } else if let Some(items) = node.as_array() {
                        for item in items {
                            check(item);
                        }
                    }
                }
                check(&tool["input_schema"]);
                assert!(
                    tool["input_schema"]
                        .to_string()
                        .contains("Constraints enforced by Desegnos")
                );
            }
            // Other providers retain their own supported limits.
            let google =
                foreign_body(ProviderId::Google, &common, &ProviderId::Google.defaults()).unwrap();
            assert_eq!(
                google["generationConfig"]["responseFormat"]["text"]["schema"],
                schema
            );
        }
        let nonrecursive = json!({"$defs":{"name":{"type":"string"}},"type":"object","properties":{"name":{"$ref":"#/$defs/name"}}});
        assert!(anthropic_schema(&nonrecursive).1);
    }

    #[tokio::test]
    async fn foreign_request_rejections_keep_provider_details_and_scrub_credentials() {
        for (id, body, code) in [
            (
                ProviderId::Anthropic,
                json!({"error":{"type":"invalid_request_error","message":"input_schema: maximum is unsupported"}}),
                "invalid_request_error",
            ),
            (
                ProviderId::Google,
                json!({"error":{"code":400,"status":"INVALID_ARGUMENT","message":"generationConfig: invalid schema"}}),
                "INVALID_ARGUMENT",
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let reply = body.to_string();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = [0; 4096];
                assert!(socket.read(&mut buffer).await.unwrap() > 0);
                socket.write_all(format!("HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\nrequest-id: req_fixture\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}", reply.len()).as_bytes()).await.unwrap();
            });
            let key = ApiKey::new("private-fixture-credential".into()).unwrap();
            let adapter = KeyAdapter::build(id, &endpoint, false).unwrap();
            let error = adapter
                .test_access(&id.defaults(), &key, &CancellationToken::new())
                .await
                .unwrap_err();
            server.await.unwrap();
            assert_eq!(error.kind, ErrorKind::RequestRejected);
            assert_eq!(error.outcome, SubmissionOutcome::Rejected);
            assert_eq!(error.code.as_deref(), Some(code));
            assert_eq!(error.request_id.as_deref(), Some("req_fixture"));
            assert_eq!(
                error.response_format.unwrap().provider_message.as_deref(),
                body["error"]["message"].as_str()
            );
            let echoed = foreign_http_error(
                400,
                br#"{"error":{"message":"private-fixture-credential"}}"#,
                &key,
                Some("application/json"),
                None,
            );
            assert!(
                !serde_json::to_string(&echoed)
                    .unwrap()
                    .contains(key.expose())
            );
        }
        assert_eq!(http_error(408).outcome, SubmissionOutcome::Rejected);
        assert_eq!(http_error(408).kind, ErrorKind::Deadline);
    }

    #[tokio::test]
    async fn foreign_stream_errors_retain_the_actual_provider_message() {
        for id in [ProviderId::Anthropic, ProviderId::Google] {
            let event = json!({"type":"error","error":{"type":"overloaded_error","message":"Provider temporarily overloaded"}});
            let (adapter, server) = fixture(id, vec![event], false).await;
            let key = ApiKey::new("private-fixture-credential".into()).unwrap();
            let error = adapter
                .submit(
                    common(),
                    0,
                    &id.defaults(),
                    &key,
                    &CancellationToken::new(),
                    &|_| {},
                )
                .await
                .unwrap_err();
            server.await.unwrap();
            assert_eq!(error.code.as_deref(), Some("overloaded_error"));
            assert_eq!(
                error.response_format.unwrap().provider_message.as_deref(),
                Some("Provider temporarily overloaded")
            );
        }
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
                id.validate(
                    &ProviderSettings {
                        model: "../invalid-model".into(),
                        ..Default::default()
                    },
                    AuthenticationMethod::ApiKey
                )
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
        assert_eq!(ProviderId::Openai.service(), "io.desegnos.openai");
    }
    #[test]
    fn task_content_preserves_documents_metadata_images_and_search_in_each_wire_format() {
        let snapshot = crate::tests::snapshot(GenerationMode::Strict);
        let schema = desegnos_core::candidate_schema();
        let image_uri = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
        let mut packet =
            crate::settings::request_content(&snapshot, GenerationMode::Strict, &schema).unwrap();
        packet.parts.push(RequestPart::Image {
            uri: image_uri,
            detail: "low",
        });
        for search in [false, true] {
            packet.web_search = search;
            let openai = packet.responses_body();
            let encoded = openai["input"][0]["content"][0]["file_data"]
                .as_str()
                .unwrap()
                .split_once(',')
                .unwrap()
                .1;
            use base64::{Engine, engine::general_purpose::STANDARD};
            assert_eq!(
                STANDARD.decode(encoded).unwrap(),
                snapshot.markdown.as_bytes()
            );
            assert_eq!(openai["input"][0]["content"][2]["image_url"], image_uri);
            for id in [ProviderId::Anthropic, ProviderId::Google] {
                let body = foreign_body(id, &packet, &id.defaults()).unwrap();
                let (document, metadata, image, schema) = if id == ProviderId::Google {
                    let parts = &body["contents"][0]["parts"];
                    assert_eq!(body.get("tools").is_some(), search);
                    (
                        &parts[0]["text"],
                        &parts[1]["text"],
                        &parts[2]["inlineData"]["data"],
                        &body["generationConfig"]["responseFormat"]["text"]["schema"],
                    )
                } else {
                    let parts = &body["messages"][0]["content"];
                    assert_eq!(
                        body["tools"].as_array().unwrap().len(),
                        if search { 2 } else { 1 }
                    );
                    (
                        &parts[0]["text"],
                        &parts[1]["text"],
                        &parts[2]["source"]["data"],
                        &body["tools"][0]["input_schema"],
                    )
                };
                assert_eq!(document, &json!(snapshot.markdown));
                assert_eq!(metadata, &openai["input"][0]["content"][1]["text"]);
                assert_eq!(image, &json!(image_uri.split_once(',').unwrap().1));
                assert_eq!(
                    schema["properties"]["jobId"]["enum"],
                    json!([snapshot.job_id])
                );
                assert_eq!(
                    schema["properties"]["requirementsSha256"]["enum"],
                    json!([snapshot.sha256])
                );
            }
        }
    }

    #[test]
    fn foreign_preflight_rejects_invalid_settings_packets_and_budgets() {
        let snapshot = crate::tests::snapshot(GenerationMode::Strict);
        let schema = desegnos_core::candidate_schema();
        for id in [ProviderId::Anthropic, ProviderId::Google] {
            let client = ProviderClient::new(id, OpenAiClient::new().unwrap()).unwrap();
            let estimate = |packet: &RequirementsSnapshot, mode, settings: &ProviderSettings| {
                client.estimate(
                    AuthenticationMethod::ApiKey,
                    packet,
                    mode,
                    &schema,
                    settings,
                )
            };
            let settings = id.defaults();
            assert!(estimate(&snapshot, GenerationMode::Strict, &settings).is_ok());
            let mut invalid = settings.clone();
            invalid.max_output_tokens = 0;
            assert_eq!(
                estimate(&snapshot, GenerationMode::Strict, &invalid)
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidSettings
            );
            invalid = settings.clone();
            invalid.spending_limit_usd = f64::NAN;
            assert_eq!(
                estimate(&snapshot, GenerationMode::Strict, &invalid)
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidSettings
            );
            invalid = settings.clone();
            invalid.spending_limit_usd = 0.000001;
            assert_eq!(
                estimate(&snapshot, GenerationMode::Strict, &invalid)
                    .unwrap_err()
                    .kind,
                ErrorKind::SpendingLimit
            );
            assert_eq!(
                estimate(&snapshot, GenerationMode::Fuzzy, &settings)
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidRequirements
            );
            let mut changed = snapshot.clone();
            changed.markdown.push('!');
            assert_eq!(
                estimate(&changed, GenerationMode::Strict, &settings)
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidRequirements
            );
            // Identity has its own schema/image checks, not the generation count/mode rules.
            let mut identity = snapshot.clone();
            identity.request_count = 0;
            identity.page_count = 0;
            let image = crate::IdentityImage {
                data_uri: "data:image/png;base64,not-base64".into(),
                photo: false,
            };
            assert!(
                client
                    .estimate_identity(
                        AuthenticationMethod::ApiKey,
                        &identity,
                        &schema,
                        &settings,
                        &[]
                    )
                    .is_ok()
            );
            assert_eq!(
                client
                    .estimate_identity(
                        AuthenticationMethod::ApiKey,
                        &identity,
                        &schema,
                        &settings,
                        &[image]
                    )
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidRequirements
            );
        }
    }

    #[tokio::test]
    async fn new_provider_model_ids_and_explicit_prices_reach_the_request() {
        for id in [ProviderId::Anthropic, ProviderId::Google] {
            let events = if id == ProviderId::Anthropic {
                anthropic("{\"ok\":true}")
            } else {
                google("{\"ok\":true}")
            };
            let (remote, server) = fixture(id, events, false).await;
            let client = ProviderClient {
                id,
                adapter: std::sync::Arc::new(remote),
            };
            let model = format!("future-{}", uuid::Uuid::new_v4());
            let settings = ProviderSettings {
                model: model.clone(),
                reasoning_effort: ReasoningEffort::Other("none".into()),
                max_output_tokens: 2048,
                spending_limit_usd: 10_000.0,
                pricing: Some(crate::ModelPricing {
                    model: model.clone(),
                    input_usd_per_million: 20.0,
                    output_usd_per_million: 80.0,
                    ..Default::default()
                }),
                ..id.defaults()
            };
            let frozen = crate::tests::snapshot(GenerationMode::Strict);
            let response = client
                .generate(
                    crate::GenerationRequest {
                        authentication: AuthenticationMethod::ApiKey,
                        snapshot: &frozen,
                        mode: GenerationMode::Strict,
                        schema: &desegnos_core::candidate_schema(),
                        settings: &settings,
                        key: &ApiKey::new("private-fixture-credential".into()).unwrap(),
                        cancellation: &CancellationToken::new(),
                    },
                    |_| {},
                )
                .await
                .unwrap();
            assert_eq!(response.text, "{\"ok\":true}");
            let request = server.await.unwrap();
            let body: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert!(body.get("pricing").is_none());
            if id == ProviderId::Anthropic {
                assert_eq!(body["model"], model);
                assert_eq!(body["max_tokens"], 2048);
                assert!(body.get("thinking").is_none());
                assert!(body.get("output_config").is_none());
            } else {
                assert!(
                    request
                        .lines()
                        .next()
                        .unwrap()
                        .contains(&format!("/models/{model}:streamGenerateContent"))
                );
                assert_eq!(body["generationConfig"]["maxOutputTokens"], 2048);
                assert!(body["generationConfig"].get("thinkingConfig").is_none());
            }
        }
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
                    0,
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
                    .submit(common(), 0, &id.defaults(), &key, &token, &|_| {})
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
                        0,
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
        stream.state = StreamState::Stopped;
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

#[cfg(test)]
mod http_status_tests {
    use super::*;

    #[test]
    fn every_http_error_status_is_rejected_even_without_a_readable_error_body() {
        let key = ApiKey::new("test-provider-key".to_owned()).unwrap();
        for status in [400, 401, 402, 403, 408, 429, 500, 502, 503, 504, 529] {
            for body in [
                b"".as_slice(),
                b"not JSON",
                br#"{"code":"internal_error"}"#,
                br#"{"error":{"type":"overloaded_error"}}"#,
            ] {
                let error = foreign_http_error(status, body, &key, None, None);
                assert_eq!(error.http_status, Some(status));
                assert_eq!(error.outcome, SubmissionOutcome::Rejected);
                assert!(error.response_id.is_none());
                assert!(error.message().contains("Nothing was changed"));
            }
        }
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use crate::tests::{Reply, Server};
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn deepseek_retries_http_500_before_streaming_and_returns_one_result() {
        let answer = json!({"id":"chat_retry", "choices":[{"index":0,"delta":{"content":"{\"ok\":true}"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}});
        let bytes = format!("data: {answer}\n\ndata: [DONE]\n\n").into_bytes();
        let server = Server::start_sequence(vec![
            Reply::json(500, json!({"code":"internal_error"})),
            Reply::sse(bytes),
        ])
        .await;
        let adapter = KeyAdapter::build(ProviderId::Deepseek, &server.endpoint, false).unwrap();
        let common = RequestContent {
            instructions: "Frozen task",
            parts: vec![RequestPart::Text("Same request".into())],
            schema: json!({"type":"object"}),
            schema_name: "fixture",
            web_search: false,
        };
        let settings = ProviderSettings {
            model: "deepseek-flash".into(),
            ..ProviderSettings::default()
        };
        let response = adapter
            .submit(
                common,
                0,
                &settings,
                &ApiKey::new("test-retry-key".into()).unwrap(),
                &CancellationToken::new(),
                &|_| {},
            )
            .await
            .unwrap();
        assert_eq!(response.text, "{\"ok\":true}");
        assert_eq!(response.usage.unwrap().total_tokens, 30);
        assert_eq!(server.count.load(Ordering::SeqCst), 2);
    }
}
