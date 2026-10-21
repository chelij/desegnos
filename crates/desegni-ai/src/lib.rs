#[cfg(all(feature = "test-support", not(debug_assertions)))]
compile_error!("desegni-ai test-support cannot be included in a release build");

pub mod chatgpt;
mod credentials;
mod error;
mod identity;
mod images;
mod providers;
mod settings;
mod sse;
mod transport;
mod website;

pub use chatgpt::*;
pub use credentials::*;
pub use error::*;
pub use identity::*;
pub use providers::*;
pub use settings::*;
pub use transport::*;
pub use website::*;

pub type Result<T> = std::result::Result<T, ProviderError>;

#[cfg(test)]
mod tests;

pub use error::scrub_diagnostic_reply;
