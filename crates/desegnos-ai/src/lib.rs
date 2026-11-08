pub mod chatgpt;
mod credentials;
mod error;
mod estimate;
mod identity;
mod image_access;
mod image_generation;
mod model_discovery;
mod net;
mod providers;
mod public_fetch;
mod request;
mod retry;
mod schema_wire;
mod settings;
mod sse;
mod transport;
mod website;

#[cfg(any(test, feature = "test-support"))]
pub use chatgpt::ChatgptTestVault;
pub use chatgpt::{
    ChatgptAccount, ChatgptAuthorization, ChatgptError, ChatgptErrorKind, ChatgptManager,
    ChatgptResult, ChatgptSignIn, ChatgptSignInStatus, ChatgptView,
};
pub use credentials::{
    ApiKey, CredentialInfo, CredentialLocation, CredentialManager, CredentialStorage,
};
pub use error::{
    ErrorKind, ProviderError, ResponseRejection, SubmissionOutcome, scrub_diagnostic_reply,
};

pub use identity::{
    IdentityImage, MAX_IDENTITY_IMAGE_URI_BYTES, MAX_IDENTITY_IMAGE_URI_TOTAL_BYTES,
    MAX_IDENTITY_IMAGES, estimate_identity_for_auth,
};

pub use image_access::{ImageAccess, ImageAvailability, image_access_now, use_image_key};
pub use providers::{
    DEFAULT_LOCAL_ENDPOINT, GenerationRequest, IdentityRequest, ProviderClient, ProviderDescriptor, ProviderId,
    estimate_identity_for_provider, local_endpoint, models_error, models_need_key,
    provider_catalogue,
};

pub use settings::{
    AuthenticationMethod, ChatgptModel, CostEstimate, DEFAULT_MAX_OUTPUT_TOKENS, MAX_OUTPUT_BYTES,
    MAX_OUTPUT_TOKENS, MAX_REQUEST_BYTES, MAX_REQUIREMENTS_BYTES, MAX_SCHEMA_BYTES, MODEL_ID,
    ModelInfo, ModelPricing, ProviderSettings, ReasoningEffort, TokenUsage,
    chatgpt_model_catalogue, estimate_for_auth, model_catalogue,
};

pub use transport::{
    CancellationToken, OpenAiClient, ProviderProgress, ProviderResponse, ProviderRetry,
};
pub use website::{WebUrl, WebsiteFetcher, website_address};

pub type Result<T> = std::result::Result<T, ProviderError>;

#[cfg(test)]
mod tests;

pub(crate) use error::CorrelationCause;

pub(crate) use error::ResponseBodyKind;

pub(crate) use error::ResponseCorrelation;

pub(crate) use error::ResponseFormat;

pub(crate) use error::TerminalOutput;

#[cfg(test)]
use identity::IDENTITY_IMAGE_TOKEN_RESERVE;

#[cfg(test)]
use identity::IDENTITY_PHOTO_TOKEN_RESERVE;

#[cfg(test)]
use providers::deepseek_reasoning_effort;

pub(crate) use settings::CHATGPT_MAX_OUTPUT_TOKENS;

pub(crate) use settings::CONTEXT_WINDOW_TOKENS;

pub(crate) use settings::MODEL_MAX_OUTPUT_TOKENS;

pub(crate) use settings::WINDOW_BYTES_PER_TOKEN;
