use crate::{Error, Result};
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

pub(crate) fn validate_text(
    value: &str,
    label: &str,
    maximum: usize,
    required: bool,
) -> Result<()> {
    ensure(
        !required || !value.trim().is_empty(),
        format!("{label} is required."),
    )?;
    ensure(
        value.chars().count() <= maximum,
        format!("{label} exceeds {maximum} characters."),
    )?;
    ensure(
        !value.contains('\0'),
        format!("{label} cannot contain null characters."),
    )
}
