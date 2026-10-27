//! Explicit native image requests. Task data cannot grant credentials, URLs or file access.
use std::collections::BTreeMap;

pub const MAX_DOWNLOADED_IMAGE_BYTES: usize = 256 * 1024;

/// One image request the AI asked for. Resolving it to bytes belongs to the provider adapter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageGenerationRequest {
    pub prompt: String,
    pub source: Option<String>,
    pub mask: Option<String>,
    pub size: String,
    pub transparent: bool,
}

/// Bytes the adapter produced, keyed by the request that produced them.
pub type GeneratedImages = BTreeMap<ImageGenerationRequest, String>;
