use crate::{Error, Result, validation::ensure};
use base64::{Engine, engine::general_purpose::STANDARD};

pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_HTTP_URL_BYTES: usize = 2048;
pub const MAX_IMAGE_PIXELS: u64 = 256_000_000;
pub const GRID_PIXEL_MIN: f64 = 1.0;
pub const GRID_PIXEL_MAX: f64 = 512.0;
pub const GRID_PERCENT_MIN: f64 = 0.25;
pub const GRID_PERCENT_MAX: f64 = 100.0;
pub const MAX_FONT_WEIGHT: u16 = 1000;
pub const MAX_STROKE_WIDTH: f64 = 256.0;
pub const MAX_RADIUS: f64 = 40_000.0;
pub const MAX_SHADOW_OFFSET: f64 = 40_000.0;
pub const MAX_SHADOW_BLUR: f64 = 4096.0;
pub const MAX_LAYOUT_SPACING: f64 = 4096.0;
pub const MAX_GRID_COLUMNS: u32 = 64;
pub const TEXT_SIZE_RANGE: (f64, f64) = (1.0, 512.0);
pub const LINE_HEIGHT_MAX: f64 = 10.0;
pub const MAX_LETTER_SPACING: f64 = 100.0;
pub const MAX_FONT_FAMILY_CHARS: usize = 200;
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

pub fn split_image_data_uri(value: &str) -> Result<(&str, &str)> {
    let (mime, encoded) = value
        .strip_prefix("data:")
        .and_then(|value| value.split_once(";base64,"))
        .ok_or_else(|| Error("An image must use an embedded base64 data URI.".into()))?;
    ensure(
        IMAGE_MIME_TYPES.contains(&mime) || mime == "image/svg+xml",
        "Use a PNG, JPEG, WebP, GIF or SVG image.",
    )?;
    Ok((mime, encoded))
}

pub fn decode_image_data_uri(value: &str) -> Result<(&str, Vec<u8>)> {
    let (mime, encoded) = split_image_data_uri(value)?;
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
            crate::svg_validation::validate_svg(&bytes)?;
            true
        }
        _ => false,
    };
    ensure(matches, "An image's contents do not match its media type.")?;
    Ok((mime, bytes))
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
/// Upper bound reserved per preview when estimating a request.
pub const AI_PREVIEW_TOKEN_RESERVE: u64 = 512;
/// One provider image tile (~85–350 tokens).
pub const AI_PREVIEW_EDGE: u32 = 512;
pub const MAX_REQUEST_IMAGES: usize = 8;

pub const MAX_REQUIREMENTS_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_RENDER_EDGE: u32 = 16_384;
pub const MAX_RENDER_PIXELS: u64 = 64_000_000;

pub const MAX_IMPORT_PAGES: usize = 30;
pub const MAX_IMPORT_CAPTURES: usize = MAX_IMPORT_PAGES * 2;
pub const MAX_WEBSITE_FILE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IDENTITY_SOURCE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_REQUEST_FILE_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_PICTURE_WIDTH: u32 = 1440;
pub const MAX_LOCAL_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;

pub const MAX_IDENTITY_RECORD_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_CLIPBOARD_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_EXPORT_CHECK_BYTES: usize = 8 * 1024 * 1024;

pub const MAX_EMBED_DEPTH: usize = 3;

pub const MAX_EXPORT_FILE_BYTES: usize = 4 * 1024 * 1024;

pub const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;

pub const MAX_EXPORT_FILES: usize = 2_000;

pub const MAX_GENERATION_BYTES: usize = 8 * 1024 * 1024;

pub const MAX_GENERATION_REQUESTS: usize = 512;

pub const MAX_GENERATED_IMAGES: usize = 8;

pub const MAX_POINTER_CLASSIFICATIONS: usize = 1000;

pub const MAX_DECISION_PROPOSALS: usize = 128;

pub const MAX_DESIGN_CRITICISMS: usize = 512;

pub const MAX_APPLIED_JOBS: usize = 10_000;

pub const MAX_EXPLANATION_CHARS: usize = 4096;

pub const MAX_DOWNLOADED_IMAGE_BYTES: usize = 256 * 1024;

pub const REQUEST_HISTORY_CHARS: usize = 3000;

pub const MAX_RECORDS_PER_LAYER: usize = 5;

pub const MAX_UNDO_STEPS: usize = 200;

pub const MAX_STYLESHEETS: usize = 4;

pub const MAX_LOGOS: usize = 6;

pub const MAX_PHOTOS: usize = 32;

pub const MAX_WEBSITE_LINKS: usize = 128;

pub const MAX_REQUEST_ATTACHMENTS: usize = 32;
pub const MAX_IMPORTED_PAGE_CONTEXTS: usize = 10_000;

pub fn image_data_uri(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

pub const IMAGE_GENERATION_SIZE: &str = "1024x1024";

pub const MAX_PROJECT_STYLES: usize = 1000;

/// Bounds allocation and embedded image size for PDF shadow masks.
pub const MAX_PDF_SHADOW_PIXELS: u32 = 4_000_000;
