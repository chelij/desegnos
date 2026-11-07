//! Explicit native image requests. Task data cannot grant credentials, URLs or file access.

pub const MAX_DOWNLOADED_IMAGE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageGenerationRequest {
    pub prompt: String,
    pub source: Option<String>,
    pub mask: Option<String>,
    pub size: String,
    pub transparent: bool,
}
