mod across_pages;
mod area_geometry;
mod attachments;
mod code_format;
mod document;
mod document_context;
mod dossier;
mod editing;
mod element_fix;
mod export;
mod generation;
mod generation_draft;
mod generation_recovery;
mod generation_scope;
mod identity;
mod identity_pages;
mod image_generation;
mod images;
mod layers;
mod model;
mod picture_scaling;
mod project_assets;
mod project_format;
mod provenance;
mod region_scripts;
mod request_history;
mod requirements;
mod session;
mod source;
mod stable_targets;
mod stale_merge;
mod text_merge;
mod website;
mod website_import;
mod website_merge;
pub use website_merge::{WebsiteImportPage, WebsiteImportReview};

pub use across_pages::*;
pub use attachments::*;
pub use code_format::{compact_site_content, format_site_content};
pub use document::*;
pub use document_context::*;
pub use dossier::*;
pub use editing::*;
pub use element_fix::{element_fix_schema, result_schema};
pub use export::*;
pub use generation::*;
pub use generation_draft::*;
pub use generation_recovery::*;
pub use identity::*;
pub use identity_pages::*;
pub use image_generation::*;
pub use images::*;
pub use layers::*;
pub use model::*;
pub use picture_scaling::{
    AI_PREVIEW_BYTES, AI_PREVIEW_EDGE, AI_PREVIEW_TOKEN_RESERVE, ai_preview, scaled_for_display,
};
pub use project_assets::*;
pub use project_format::*;
pub use provenance::*;
pub use region_scripts::region_script_runtime;
pub use request_history::*;
pub use requirements::*;
pub use session::*;
pub use source::*;
pub use stable_targets::*;
pub use stale_merge::*;
pub use text_merge::*;
pub use website::*;
pub use website_import::{
    ImportedWebsite, LivePage, PREVIEW_INLINE_BYTES, PreviewFile, import_website_files,
};

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
