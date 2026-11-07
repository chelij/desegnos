pub mod chatgpt;
mod credentials;
mod error;
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
    CorrelationCause, ErrorKind, ProviderError, ResponseBodyKind, ResponseCorrelation,
    ResponseFormat, ResponseRejection, SubmissionOutcome, TerminalOutput, scrub_diagnostic_reply,
};
pub use identity::{
    IDENTITY_IMAGE_TOKEN_RESERVE, IDENTITY_PHOTO_TOKEN_RESERVE, IdentityImage,
    MAX_IDENTITY_IMAGE_URI_BYTES, MAX_IDENTITY_IMAGE_URI_TOTAL_BYTES, MAX_IDENTITY_IMAGES,
    MAX_IDENTITY_PHOTO_URI_BYTES, estimate_identity_for_auth,
};
pub use image_access::{ImageAccess, ImageAvailability, image_access_now, use_image_key};
pub use providers::{
    DEFAULT_LOCAL_ENDPOINT, GenerationRequest, IdentityRequest, ProviderAdapter, ProviderClient,
    ProviderDescriptor, ProviderFuture, ProviderId, deepseek_reasoning_effort,
    estimate_identity_for_provider, local_endpoint, models_error, models_need_key,
    provider_catalogue,
};
pub use settings::{
    AuthenticationMethod, CHATGPT_MAX_OUTPUT_TOKENS, CONTEXT_WINDOW_TOKENS, ChatgptModel,
    CostEstimate, DEFAULT_MAX_OUTPUT_TOKENS, MAX_OUTPUT_BYTES, MAX_OUTPUT_TOKENS,
    MAX_REQUEST_BYTES, MAX_REQUIREMENTS_BYTES, MAX_SCHEMA_BYTES, MODEL_ID, MODEL_MAX_OUTPUT_TOKENS,
    ModelInfo, ModelPricing, ProviderSettings, ReasoningEffort, TokenUsage, VERIFIED_AT,
    WINDOW_BYTES_PER_TOKEN, chatgpt_model_catalogue, estimate_cost, estimate_for_auth,
    model_catalogue,
};
pub use transport::{
    CancellationToken, OpenAiClient, ProviderProgress, ProviderResponse, ProviderRetry,
};
pub use website::{
    WebUrl, WebsiteFetcher, WebsiteFiles, WebsiteSnapshot, WebsiteSnapshotPage, website_address,
};

pub type Result<T> = std::result::Result<T, ProviderError>;

#[cfg(test)]
mod tests;
