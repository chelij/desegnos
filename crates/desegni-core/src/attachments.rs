//! Files and links attached to a request ("drop a file on the box"). From requirements format
//! 13 the AI sees a small preview of each picture; the original stays in the project and is
//! inserted after generation, scaled to the size the page shows it at.
use crate::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

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
    pub data_uri: Option<std::sync::Arc<str>>,
    /// The small JPEG the AI sees, made once when the picture is attached (format 13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<std::sync::Arc<str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Checks the type, the file signature and the Base64 alphabet without decoding the whole
/// picture, because this runs on every edit and pictures have no size limit.
fn valid_image(data_uri: &str) -> bool {
    let Some((mime, encoded)) = data_uri
        .strip_prefix("data:")
        .and_then(|value| value.split_once(";base64,"))
    else {
        return false;
    };
    let body = encoded.trim_end_matches('=');
    if !matches!(
        mime,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    ) || encoded.len() % 4 != 0
        || encoded.len() - body.len() > 2
        || !body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
    {
        return false;
    }
    let Ok(bytes) = STANDARD.decode(&encoded[..encoded.len().min(16)]) else {
        return false;
    };
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        _ => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
    }
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
                    && self.data_uri.as_deref().is_some_and(valid_image)
                    && self.preview.as_deref().is_none_or(|preview| {
                        preview.starts_with("data:image/jpeg;base64,")
                            && preview.len() <= crate::AI_PREVIEW_BYTES.div_ceil(3) * 4 + 32
                            && valid_image(preview)
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

/// Swap attached pictures into generated HTML: `<img data-desegnos-attachment="ID">` gets a
/// project picture reference, and the marker is removed. Each picture is scaled to its widest
/// placement (its `width` attribute, else the page width) and only that copy is kept.
/// Unknown identifiers are left as they are.
pub(crate) fn insert_attached_images(
    html: &str,
    attachments: &[&RequestAttachment],
    page_width: u32,
    pictures: &mut BTreeMap<Uuid, Arc<str>>,
) -> Result<Option<String>> {
    if !html.contains(ATTACHMENT_ATTRIBUTE) {
        return Ok(None);
    }
    let doc = crate::editing::parse(html)?;
    let selection = doc.select(&format!("img[{ATTACHMENT_ATTRIBUTE}]"));
    let nodes = selection.nodes();
    let mut widths = BTreeMap::<String, u32>::new();
    for node in nodes {
        let id = node
            .attr(ATTACHMENT_ATTRIBUTE)
            .unwrap_or_default()
            .to_string();
        let width = node
            .attr("width")
            .and_then(|value| value.trim().trim_end_matches("px").parse::<u32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(page_width)
            .min(page_width.max(1));
        let widest = widths.entry(id).or_default();
        *widest = (*widest).max(width);
    }
    let mut sources = BTreeMap::new();
    for (id, width) in widths {
        if let Some(data) = attachments
            .iter()
            .find(|item| item.id == id && item.kind == AttachmentKind::Image)
            .and_then(|item| item.data_uri.as_ref())
        {
            let uri: Arc<str> = crate::scaled_for_display(data, width)
                .map(Arc::from)
                .unwrap_or_else(|| data.clone());
            let picture = crate::project_assets::inline_image_id(&uri);
            pictures.entry(picture).or_insert(uri);
            sources.insert(id, crate::project_asset_reference(picture));
        }
    }
    for node in nodes {
        let id = node
            .attr(ATTACHMENT_ATTRIBUTE)
            .unwrap_or_default()
            .to_string();
        if let Some(source) = sources.get(&id) {
            node.set_attr("src", source);
            node.remove_attr("srcset");
        }
        node.remove_attr(ATTACHMENT_ATTRIBUTE);
    }
    crate::editing::serialized(&doc).map(Some)
}
