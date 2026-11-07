pub mod chatgpt;
mod credentials;
mod error;
mod identity;
mod image_access;
mod image_generation;
mod images;
mod model_discovery;
mod providers;
mod request;
mod retry;
mod schema_wire;
mod settings;
mod sse;
mod transport;
mod website;

pub use chatgpt::*;
pub use credentials::*;
pub use error::*;
pub use identity::*;
pub use image_access::*;
pub use providers::*;
pub use settings::*;
pub use transport::*;
pub use website::*;

pub type Result<T> = std::result::Result<T, ProviderError>;

#[cfg(test)]
mod tests;

pub use error::scrub_diagnostic_reply;
