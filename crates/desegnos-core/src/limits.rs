use crate::{Error, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};

pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_HTTP_URL_BYTES: usize = 2048;
pub const MAX_IMAGE_PIXELS: u64 = 256_000_000;
pub const GRID_PIXEL_MIN: f64 = 1.0;
pub const GRID_PIXEL_MAX: f64 = 512.0;
pub const GRID_PERCENT_MIN: f64 = 0.25;
pub const GRID_PERCENT_MAX: f64 = 100.0;
pub const MAX_FONT_WEIGHT: u16 = 1000;
pub const DEFAULT_PAGE_BACKGROUND: &str = "#ffffff";
pub const MAX_REQUEST_SHAPE_POINTS: usize = 4096;
pub const IMAGE_MIME_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];

pub const fn base64_encoded_len(bytes: usize) -> usize {
    bytes.div_ceil(3) * 4
}

pub fn valid_http_url(value: &str) -> bool {
    value.len() <= MAX_HTTP_URL_BYTES
        && url::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

pub fn validate_http_url(value: &str) -> Result<()> {
    ensure(
        valid_http_url(value),
        "A web address must start with http:// or https:// and have a host and no credentials.",
    )
}

pub fn is_hex_colour(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 3 | 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn decode_image_data_uri(value: &str) -> Result<(&str, Vec<u8>)> {
    let (mime, encoded) = value
        .strip_prefix("data:")
        .and_then(|value| value.split_once(";base64,"))
        .ok_or_else(|| Error("An image must use an embedded base64 data URI.".into()))?;
    ensure(
        IMAGE_MIME_TYPES.contains(&mime) || mime == "image/svg+xml",
        "Use a PNG, JPEG, WebP, GIF or SVG image.",
    )?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| Error("An image contains invalid base64 data.".into()))?;
    ensure(
        !bytes.is_empty() && STANDARD.encode(&bytes) == encoded,
        "An image must use canonical base64 data.",
    )?;
    let matches = match mime {
        "image/png" => {
            bytes.len() >= 33
                && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                && &bytes[12..16] == b"IHDR"
        }
        "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff") && bytes.ends_with(b"\xff\xd9"),
        "image/webp" => {
            bytes.len() >= 20
                && &bytes[..4] == b"RIFF"
                && &bytes[8..12] == b"WEBP"
                && matches!(&bytes[12..16], b"VP8 " | b"VP8L" | b"VP8X")
                && u32::from_le_bytes(bytes[4..8].try_into().expect("four bytes")) as usize
                    == bytes.len() - 8
        }
        "image/gif" => {
            bytes.len() >= 14
                && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
                && bytes.ends_with(b";")
        }
        "image/svg+xml" => {
            crate::identity::validate_svg(&bytes)?;
            true
        }
        _ => false,
    };
    ensure(matches, "An image's contents do not match its media type.")?;
    Ok((mime, bytes))
}

pub(crate) fn next_revision(revision: u64) -> Result<u64> {
    revision
        .checked_add(1)
        .ok_or_else(|| Error("Project revision limit reached.".into()))
}

pub const MAX_EXPORT_DESTINATION: usize = 4_096;

pub const MAX_PAGES: usize = 100;

pub const MAX_PAGE_SIZES: usize = 8;

pub const MAX_PROMPT_CHARS: usize = 16_384;

pub const MAX_REFERENCE_ITEMS: usize = 1000;

pub const MAX_POINTERS: usize = 1000;

pub const MAX_REQUESTS: usize = 256;

pub const MAX_REQUEST_TARGETS: usize = 64;

pub const MAX_NAMED_VIEWS: usize = 50;

pub const MAX_NAMED_VIEW_NAME_CHARS: usize = 80;

pub const MAX_VIEWPORT_SIDE: u32 = 100_000;

pub const MAX_ELEMENTS: usize = 10_000;

pub const MAX_ELEMENT_DEPTH: usize = 32;

pub const MAX_ELEMENT_CODE_CHARS: usize = 200_000;

pub const MAX_ELEMENT_REQUESTS: usize = 100;

pub const MAX_ELEMENT_CONTEXT: usize = 32;

pub const MAX_ELEMENT_OPTIONS: usize = 100;

pub const MAX_ELEMENT_NAME_CHARS: usize = MAX_NAME_CHARS;

pub const MAX_LINKED_DEFINITIONS: usize = 1_000;

pub const MAX_IDENTITY_ASSETS: usize = 4;

pub const MAX_IDENTITY_ASSET_BYTES: usize = 512 * 1024;

pub const MAX_PROJECT_PHOTO_BYTES: usize = 200 * 1024;

pub const MAX_PHOTO_DESCRIPTION_CHARS: usize = 200;

pub const MAX_IMPORTED_ITEMS: usize = 64;

pub const AI_PREVIEW_BYTES: usize = 48 * 1024;
pub const AI_PREVIEW_TOKEN_RESERVE: u64 = 512;
pub const AI_PREVIEW_EDGE: u32 = 512;
