//! Shared cost rules; transport-specific wire and context reserves are explicit inputs.
use crate::{CostEstimate, ErrorKind, ModelInfo, ProviderError, ProviderSettings, Result};

pub(crate) const WIRE_FRAMING_TOKENS: u64 = 4096;
pub(crate) const IMAGE_GENERATION_USD: f64 = 0.50;
pub(crate) const CACHE_WRITE_FACTOR: f64 = 1.25;

pub(crate) struct EstimateInput<'a> {
    pub wire_bytes: usize,
    pub visual_tokens: u64,
    pub framing_tokens: u64,
    pub window_bytes_per_token: u64,
    pub context_window: u64,
    pub output_reserve: u32,
    pub images: usize,
    pub included: bool,
    pub cache_write_factor: f64,
    pub verified_at: &'a str,
}

pub(crate) fn estimate(
    input: EstimateInput<'_>,
    settings: &ProviderSettings,
    model: Option<&ModelInfo>,
) -> Result<CostEstimate> {
    let reserve = input
        .visual_tokens
        .checked_add(input.framing_tokens)
        .ok_or_else(|| ProviderError::before(ErrorKind::ContextLimit))?;
    let upper = (input.wire_bytes as u64)
        .checked_add(reserve)
        .ok_or_else(|| ProviderError::before(ErrorKind::ContextLimit))?;
    let window = (input.wire_bytes as u64 / input.window_bytes_per_token)
        .checked_add(reserve)
        .and_then(|count| count.checked_add(u64::from(input.output_reserve)))
        .ok_or_else(|| ProviderError::before(ErrorKind::ContextLimit))?;
    if window > input.context_window {
        return Err(ProviderError::before(ErrorKind::ContextLimit));
    }
    if input.included {
        return Ok(CostEstimate {
            estimate_usd: 0.0,
            input_token_upper_bound: upper,
            verified_at: input.verified_at.into(),
        });
    }
    let (input_rate, output_rate, fees) = if let Some(pricing) = &settings.pricing {
        pricing.validate(&settings.model)?;
        (
            pricing.input_usd_per_million,
            pricing.output_usd_per_million,
            pricing.request_usd + input.images as f64 * pricing.image_usd,
        )
    } else if let Some(model) = model {
        (
            model.long_context_input_usd_per_million * input.cache_write_factor,
            model.long_context_output_usd_per_million,
            0.0,
        )
    } else {
        return Err(ProviderError::before(ErrorKind::InvalidSettings));
    };
    let estimate = ((upper as f64 * input_rate
        + f64::from(settings.max_output_tokens) * output_rate)
        / 1_000_000.0
        + fees)
        .mul_add(100.0, 0.0)
        .ceil()
        / 100.0;
    if !settings.spending_limit_usd.is_finite()
        || settings.spending_limit_usd <= 0.0
        || estimate > settings.spending_limit_usd
    {
        return Err(ProviderError::before(ErrorKind::SpendingLimit));
    }
    Ok(CostEstimate {
        estimate_usd: estimate,
        input_token_upper_bound: upper,
        verified_at: input.verified_at.into(),
    })
}
