mod dossier;
mod editing;
mod export;
mod generation;
mod generation_scope;
mod identity;
mod images;
mod layers;
mod migration;
mod model;
mod project_assets;
mod provenance;
mod requirements;
mod session;
mod source;
mod stable_targets;

pub use dossier::*;
pub use editing::*;
pub use export::*;
pub use generation::*;
pub use identity::*;
pub use images::*;
pub use layers::*;
pub use migration::*;
pub use model::*;
pub use project_assets::*;
pub use provenance::*;
pub use requirements::*;
pub use session::*;
pub use source::*;
pub use stable_targets::*;

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
