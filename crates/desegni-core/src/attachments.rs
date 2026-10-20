//! Files and links attached to a request ("drop a file on the box"). Pictures stay in the
//! project and are inserted after generation, so their bytes are never sent to the AI.
use crate::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

pub const MAX_ATTACHMENTS: usize = 6;
pub const MAX_ATTACHED_IMAGE_BYTES: usize = 1024 * 1024;
pub const MAX_ATTACHED_TEXT_CHARS: usize = 32 * 1024;
pub const ATTACHMENT_ATTRIBUTE: &str = "data-desegnos-attachment";

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
    pub data_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

fn image_bytes(data_uri: &str) -> Option<Vec<u8>> {
    let (mime, encoded) = data_uri.strip_prefix("data:")?.split_once(";base64,")?;
    if !matches!(
        mime,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    ) || encoded.len() > MAX_ATTACHED_IMAGE_BYTES.div_ceil(3) * 4
    {
        return None;
    }
    let bytes = STANDARD.decode(encoded).ok()?;
    let signature = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        _ => bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
    };
    (signature && !bytes.is_empty() && bytes.len() <= MAX_ATTACHED_IMAGE_BYTES).then_some(bytes)
}

impl RequestAttachment {
    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.id.is_empty()
                && self.id.len() <= 16
                && self.id.bytes().all(|b| b.is_ascii_alphanumeric()),
            "An attachment has an invalid identifier.",
        )?;
        ensure(
            !self.name.trim().is_empty()
                && self.name.chars().count() <= 120
                && !self.name.chars().any(char::is_control),
            "Give the attachment a short file name.",
        )?;
        match self.kind {
            AttachmentKind::Image => ensure(
                self.text.is_none()
                    && self.url.is_none()
                    && self.data_uri.as_deref().and_then(image_bytes).is_some(),
                "Attach a PNG, JPEG, WebP or GIF picture of at most 1 MB.",
            ),
            AttachmentKind::Text => ensure(
                self.data_uri.is_none()
                    && self.url.is_none()
                    && self.text.as_ref().is_some_and(|text| {
                        !text.trim().is_empty()
                            && text.chars().count() <= MAX_ATTACHED_TEXT_CHARS
                            && !text.contains('\0')
                    }),
                "Attach a text file of at most 32,000 characters.",
            ),
            AttachmentKind::Link => ensure(
                self.data_uri.is_none()
                    && self.text.is_none()
                    && self.url.as_ref().is_some_and(|url| {
                        url.len() <= 2048
                            && url::Url::parse(url).is_ok_and(|parsed| {
                                matches!(parsed.scheme(), "https" | "http")
                                    && parsed.host_str().is_some()
                                    && parsed.username().is_empty()
                                    && parsed.password().is_none()
                            })
                    }),
                "Attach a web address that starts with https://.",
            ),
        }
    }
}

/// Swap attached pictures into generated HTML: `<img data-desegnos-attachment="ID">` gets the
/// stored picture, and the marker is removed. Unknown identifiers are left as they are.
pub(crate) fn insert_attached_images(
    html: &str,
    attachments: &[&RequestAttachment],
) -> Result<Option<String>> {
    if !html.contains(ATTACHMENT_ATTRIBUTE) {
        return Ok(None);
    }
    let doc = crate::editing::parse(html)?;
    for node in doc.select(&format!("img[{ATTACHMENT_ATTRIBUTE}]")).nodes() {
        let id = node
            .attr(ATTACHMENT_ATTRIBUTE)
            .unwrap_or_default()
            .to_string();
        if let Some(data) = attachments
            .iter()
            .find(|item| item.id == id && item.kind == AttachmentKind::Image)
            .and_then(|item| item.data_uri.as_ref())
        {
            node.set_attr("src", data);
            node.remove_attr("srcset");
        }
        node.remove_attr(ATTACHMENT_ATTRIBUTE);
    }
    crate::editing::serialized(&doc).map(Some)
}
