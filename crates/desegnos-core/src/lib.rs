mod web_codegen;
mod geometry;
mod text_layout;
mod element_style;
mod adapt;
mod attachments;
mod dossier;
mod elements;
mod export;
mod generation;
mod identity;
mod identity_pages;
mod image_generation;
mod json_parse;
mod model;
mod picture_scaling;
mod pointers;
mod project_assets;
mod provenance;
mod render;
mod request_history;
mod requirements;
mod session;
mod website;
mod website_import;

pub use adapt::adapt_size;
pub use attachments::*;
pub use dossier::*;
pub use elements::*;
pub use export::*;
pub use generation::*;
pub use identity::*;
pub use identity_pages::*;
pub use image_generation::*;
pub use json_parse::*;
pub use model::*;
pub use picture_scaling::{
    AI_PREVIEW_BYTES, AI_PREVIEW_EDGE, AI_PREVIEW_TOKEN_RESERVE, ai_preview, scaled_for_display,
};
pub use pointers::*;
pub use project_assets::*;
pub use provenance::*;
pub use render::*;
pub use request_history::*;
pub use requirements::*;
pub use session::*;
pub use website::*;
pub use website_import::*;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Error {}

pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn required_nullable<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(deserializer)
}

pub(crate) fn ensure(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error(message.into()))
    }
}
