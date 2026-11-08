use crate::{ErrorKind, ProviderError, Result};
use serde::{Deserialize, Serialize};

pub const VERIFIED_AT: &str = "2026-10-31";
pub const MODEL_ID: &str = "gpt-6.1-sol";
/// Ceiling on a reply's billed output for every provider; discovered models are clamped to it.
pub const MAX_OUTPUT_TOKENS: u32 = 128_000;
/// Provider defaults stay at 32,768 so a first estimate stays inside the default budget.
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_768;
/// ChatGPT plans ignore `max_output_tokens`, so only their reply text keeps this allowance.
pub const CHATGPT_MAX_OUTPUT_TOKENS: u32 = DEFAULT_MAX_OUTPUT_TOKENS;
pub use desegnos_core::limits::MAX_GENERATION_BYTES as MAX_OUTPUT_BYTES;
pub use desegnos_core::limits::MAX_REQUIREMENTS_BYTES;
pub const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
pub const CONTEXT_WINDOW_TOKENS: u64 = 1_050_000;
/// A context window is metered in tokens, and a wire byte is not a token: JSON, schema text and
/// markup carry two or more bytes per token. Counting every byte as a token keeps a cost estimate
/// safe, but as a window bound it refuses requests a large model accepts: a generated page with
/// several hundred layers is about half a megabyte of JSON, far inside a million tokens. Budget
/// the window on two bytes per token instead, still an over-count, and let the provider reject a
/// request that is really too long.
pub const WINDOW_BYTES_PER_TOKEN: u64 = 2;
pub const MAX_SCHEMA_BYTES: usize = 256 * 1024;

#[derive(ts_rs::TS, Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthenticationMethod {
    #[default]
    ApiKey,
    Chatgpt,
}

#[derive(ts_rs::TS, Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatgptModel {
    pub id: String,
    pub name: String,
    #[serde(default = "default_reasoning_efforts")]
    pub reasoning_efforts: Vec<ReasoningEffort>,
    #[serde(default)]
    pub context_window_tokens: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[ts(type = "string")]
#[serde(try_from = "String", into = "String")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
    Other(String),
}

impl ReasoningEffort {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Other(value) => value,
        }
    }
}

impl TryFrom<String> for ReasoningEffort {
    type Error = &'static str;
    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err("Invalid reasoning effort");
        }
        Ok(match value.as_str() {
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            _ => Self::Other(value),
        })
    }
}

impl From<ReasoningEffort> for String {
    fn from(value: ReasoningEffort) -> Self {
        value.as_str().into()
    }
}

pub(crate) fn default_reasoning_efforts() -> Vec<ReasoningEffort> {
    vec![
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
    ]
}

pub(crate) fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && !matches!(id, "." | "..")
        && id.len() <= 128
        && id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
        && id
            .split('/')
            .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}

impl ChatgptModel {
    pub fn validate_settings(&self, settings: &ProviderSettings) -> Result<()> {
        settings.validate_for_auth(AuthenticationMethod::Chatgpt)?;
        if settings.model != self.id || !self.reasoning_efforts.contains(&settings.reasoning_effort)
        {
            return Err(ProviderError::before(ErrorKind::InvalidSettings)
                .with_authentication(AuthenticationMethod::Chatgpt));
        }
        Ok(())
    }

    /// Account-advertised bounds supplement the transport's application limits.
    /// Missing output metadata is not an advertised output allowance.
    pub fn validate_input(&self, input_tokens: u64) -> Result<()> {
        let context = self
            .context_window_tokens
            .unwrap_or(CONTEXT_WINDOW_TOKENS)
            .min(CONTEXT_WINDOW_TOKENS);
        let output = self
            .max_output_tokens
            .map_or(0, |limit| limit.min(MAX_OUTPUT_TOKENS));
        if input_tokens.saturating_add(u64::from(output)) > context {
            return Err(ProviderError::before(ErrorKind::ContextLimit)
                .with_authentication(AuthenticationMethod::Chatgpt));
        }
        Ok(())
    }
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderSettings {
    pub model: String,
    pub max_output_tokens: u32,
    pub reasoning_effort: ReasoningEffort,
    pub spending_limit_usd: f64,
    pub image_generation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<ModelPricing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub local_authentication: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(ts_rs::TS, Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelPricing {
    pub model: String,
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
    #[serde(default)]
    pub request_usd: f64,
    #[serde(default)]
    pub image_usd: f64,
}

impl ModelPricing {
    pub fn validate(&self, model: &str) -> Result<()> {
        if self.model != model
            || [
                self.input_usd_per_million,
                self.output_usd_per_million,
                self.request_usd,
                self.image_usd,
            ]
            .iter()
            .any(|rate| !rate.is_finite() || *rate < 0.0 || *rate > 1_000_000.0)
        {
            return Err(ProviderError::before(ErrorKind::InvalidSettings));
        }
        Ok(())
    }
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            model: MODEL_ID.into(),
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
            reasoning_effort: ReasoningEffort::Medium,
            spending_limit_usd: 2.0,
            image_generation: true,
            pricing: None,
            endpoint: None,
            local_authentication: false,
        }
    }
}

impl ProviderSettings {
    /// The cost of a finished request at the configured pricing; none when unpriced.
    pub fn usage_cost_usd(&self, usage: TokenUsage) -> Option<f64> {
        let pricing = self.pricing.as_ref()?;
        pricing.validate(&self.model).ok()?;
        Some(
            (usage.input_tokens as f64 * pricing.input_usd_per_million
                + usage.output_tokens as f64 * pricing.output_usd_per_million)
                / 1_000_000.0
                + pricing.request_usd,
        )
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_for_auth(AuthenticationMethod::ApiKey)
    }

    pub fn validate_for_auth(&self, authentication: AuthenticationMethod) -> Result<()> {
        if !valid_model_id(&self.model) {
            return Err(ProviderError::before(ErrorKind::InvalidSettings)
                .with_authentication(authentication));
        }
        self.validate_limits(authentication)
    }

    pub(crate) fn validate_limits(&self, authentication: AuthenticationMethod) -> Result<()> {
        if authentication == AuthenticationMethod::ApiKey
            && let Some(pricing) = &self.pricing
        {
            pricing.validate(&self.model)?;
        }
        if authentication == AuthenticationMethod::ApiKey
            && (self.max_output_tokens == 0
                || self.max_output_tokens > MAX_OUTPUT_TOKENS
                || !self.spending_limit_usd.is_finite()
                || self.spending_limit_usd <= 0.0
                || self.spending_limit_usd > 10_000.0)
        {
            return Err(ProviderError::before(ErrorKind::InvalidSettings)
                .with_authentication(authentication));
        }
        Ok(())
    }
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
    pub verified_at: String,
    pub source_url: String,
    pub reasoning_efforts: Vec<ReasoningEffort>,
    pub context_window_tokens: u64,
    pub max_output_tokens: u32,
    pub long_context_input_usd_per_million: f64,
    pub long_context_output_usd_per_million: f64,
    #[serde(default)]
    pub pricing_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_pricing: Option<ModelPricing>,
}

impl ModelInfo {
    pub fn validate_api_settings(&self, settings: &ProviderSettings) -> Result<()> {
        settings.validate_for_auth(AuthenticationMethod::ApiKey)?;
        if self.id != settings.model
            || !self.reasoning_efforts.contains(&settings.reasoning_effort)
            || settings.max_output_tokens > self.max_output_tokens
            || ((self.pricing_required || self.default_pricing.is_some())
                && settings.pricing.is_none())
        {
            return Err(ProviderError::before(ErrorKind::InvalidSettings));
        }
        if let (Some(quote), Some(pricing)) = (&self.default_pricing, &settings.pricing)
            && (pricing.input_usd_per_million < quote.input_usd_per_million
                || pricing.output_usd_per_million < quote.output_usd_per_million
                || pricing.request_usd < quote.request_usd
                || pricing.image_usd < quote.image_usd)
        {
            return Err(ProviderError::before(ErrorKind::InvalidSettings));
        }
        Ok(())
    }

    pub fn validate_api_input(&self, settings: &ProviderSettings, input_tokens: u64) -> Result<()> {
        self.validate_api_settings(settings)?;
        if input_tokens.saturating_add(u64::from(settings.max_output_tokens))
            > self.context_window_tokens
        {
            return Err(ProviderError::before(ErrorKind::ContextLimit));
        }
        Ok(())
    }
}

pub fn model_catalogue() -> Vec<ModelInfo> {
    [
        (MODEL_ID, "GPT-6.1 Sol", 2.0, 10.0),
        ("gpt-6-astra", "GPT-6 Astra", 10.0, 50.0),
        ("gpt-6.1-terra", "GPT-6.1 Terra", 1.0, 5.0),
        ("gpt-6-luna", "GPT-6 Luna", 0.1, 0.5),
    ]
    .into_iter()
    .map(|(id, name, input, output)| ModelInfo {
        id: id.into(),
        name: name.into(),
        input_usd_per_million: input,
        output_usd_per_million: output,
        verified_at: VERIFIED_AT.into(),
        source_url: format!("https://developers.openai.com/api/docs/models/{id}"),
        reasoning_efforts: vec![
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
        ],
        context_window_tokens: CONTEXT_WINDOW_TOKENS,
        max_output_tokens: MAX_OUTPUT_TOKENS,
        long_context_input_usd_per_million: input * 2.0,
        long_context_output_usd_per_million: output * 1.5,
        pricing_required: false,
        default_pricing: None,
    })
    .collect()
}

/// The authenticated account catalogue is authoritative for ChatGPT selection.
/// API prices are unused for plan requests and must not be guessed for new models.
pub fn chatgpt_model_catalogue(available: &[ChatgptModel]) -> Vec<ModelInfo> {
    available
        .iter()
        .map(|model| ModelInfo {
            id: model.id.clone(),
            name: model.name.clone(),
            reasoning_efforts: model.reasoning_efforts.clone(),
            context_window_tokens: model.context_window_tokens.unwrap_or(CONTEXT_WINDOW_TOKENS),
            max_output_tokens: model.max_output_tokens.unwrap_or(MAX_OUTPUT_TOKENS),
            input_usd_per_million: 0.0,
            output_usd_per_million: 0.0,
            long_context_input_usd_per_million: 0.0,
            long_context_output_usd_per_million: 0.0,
            verified_at: String::new(),
            source_url: String::new(),
            pricing_required: false,
            default_pricing: None,
        })
        .collect()
}

#[derive(ts_rs::TS, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostEstimate {
    pub estimate_usd: f64,
    pub input_token_upper_bound: u64,
    pub verified_at: String,
}
