use crate::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Image,
    Text,
    Link,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestAttachment {
    pub id: String,
    pub name: String,
    pub kind: AttachmentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_uri: Option<std::sync::Arc<str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<std::sync::Arc<str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl RequestAttachment {
    /// The exact picture reference a generated image can use; filenames are not identifiers.
    pub(crate) fn picture_id(&self) -> Option<uuid::Uuid> {
        if self.kind != AttachmentKind::Image {
            return None;
        }
        self.data_uri
            .as_deref()
            .map(crate::project_assets::inline_image_id)
    }

    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.id.is_empty()
                && self.id.len() <= 16
                && self.id.bytes().all(|b| b.is_ascii_alphanumeric()),
            "An attachment has an invalid identifier.",
        )?;
        ensure(
            !self.name.trim().is_empty()
                && self.name.chars().count() <= crate::limits::MAX_NAME_CHARS
                && !self.name.chars().any(char::is_control),
            "Give the attachment a short file name.",
        )?;
        match self.kind {
            AttachmentKind::Image => ensure(
                self.text.is_none()
                    && self.url.is_none()
                    && self.data_uri.as_deref().is_some_and(|uri| {
                        crate::limits::decode_image_data_uri(uri)
                            .is_ok_and(|(mime, _)| crate::limits::IMAGE_MIME_TYPES.contains(&mime))
                    })
                    && self.preview.as_deref().is_none_or(|preview| {
                        (preview.starts_with("data:image/jpeg;base64,")
                            || preview.starts_with("data:image/png;base64,"))
                            && preview.len()
                                <= crate::limits::base64_encoded_len(crate::AI_PREVIEW_BYTES) + 32
                            && crate::limits::decode_image_data_uri(preview).is_ok()
                    }),
                "Attach a PNG, JPEG, WebP or GIF picture.",
            ),
            AttachmentKind::Text => ensure(
                self.data_uri.is_none()
                    && self.preview.is_none()
                    && self.url.is_none()
                    && self
                        .text
                        .as_ref()
                        .is_some_and(|text| !text.trim().is_empty() && !text.contains('\0')),
                "Attach a text file that is not empty.",
            ),
            AttachmentKind::Link => ensure(
                self.data_uri.is_none()
                    && self.preview.is_none()
                    && self.text.is_none()
                    && self
                        .url
                        .as_ref()
                        .is_some_and(|url| crate::limits::valid_http_url(url)),
                "Attach a web address that starts with https://.",
            ),
        }
    }
}
