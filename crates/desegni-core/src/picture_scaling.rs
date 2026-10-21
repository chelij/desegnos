//! Picture sizes around generation: a small preview for the AI, and after generation a copy
//! scaled to the size the page shows it at. GIFs are left as they are (they may be animated).
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageDecoder, ImageReader, imageops::FilterType};

/// A preview this size costs one image tile on every supported provider (~85–350 tokens).
pub const AI_PREVIEW_EDGE: u32 = 512;
pub const AI_PREVIEW_BYTES: usize = 48 * 1024;
/// Upper bound reserved per preview when estimating a request.
pub const AI_PREVIEW_TOKEN_RESERVE: u64 = 512;
const MIN_PREVIEW_EDGE: u32 = 96;
/// Rendered pictures keep twice their CSS size so they stay sharp on high-density screens.
const DISPLAY_DENSITY: u32 = 2;

fn decode(data_uri: &str) -> Option<DynamicImage> {
    let (mime, encoded) = data_uri.strip_prefix("data:")?.split_once(";base64,")?;
    if !matches!(mime, "image/png" | "image/jpeg" | "image/webp") {
        return None;
    }
    let bytes = STANDARD.decode(encoded).ok()?;
    let mut decoder = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 256_000_000 {
        return None;
    }
    // Browsers show camera pictures upright, so re-encoded copies must too.
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = DynamicImage::from_decoder(decoder).ok()?;
    picture.apply_orientation(orientation);
    Some(picture)
}

fn encode(picture: &DynamicImage, jpeg_quality: u8) -> Option<(Vec<u8>, &'static str)> {
    let mut bytes = Vec::new();
    if picture.color().has_alpha() {
        picture
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .ok()?;
        Some((bytes, "image/png"))
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, jpeg_quality)
            .encode_image(&picture.to_rgb8())
            .ok()?;
        Some((bytes, "image/jpeg"))
    }
}

fn flattened(picture: &DynamicImage) -> DynamicImage {
    if !picture.color().has_alpha() {
        return DynamicImage::ImageRgb8(picture.to_rgb8());
    }
    let mut flat = image::RgbaImage::from_pixel(
        picture.width(),
        picture.height(),
        image::Rgba([255, 255, 255, 255]),
    );
    image::imageops::overlay(&mut flat, &picture.to_rgba8(), 0, 0);
    DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(flat).to_rgb8())
}

/// The JPEG the AI sees: shrunk by a quarter at a time until it is at most
/// `AI_PREVIEW_EDGE` pixels on its longest side and `AI_PREVIEW_BYTES` in size.
/// None for pictures that cannot be decoded here (GIF); those travel by name only.
pub fn ai_preview(data_uri: &str) -> Option<String> {
    let mut picture = flattened(&decode(data_uri)?);
    loop {
        let edge = picture.width().max(picture.height());
        if edge > AI_PREVIEW_EDGE {
            let next = (edge * 3 / 4).max(AI_PREVIEW_EDGE);
            picture = picture.resize(next, next, FilterType::Triangle);
            continue;
        }
        let (bytes, _) = encode(&picture, 75)?;
        if bytes.len() <= AI_PREVIEW_BYTES || edge <= MIN_PREVIEW_EDGE {
            return Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)));
        }
        let next = (edge * 3 / 4).max(MIN_PREVIEW_EDGE);
        picture = picture.resize(next, next, FilterType::Triangle);
    }
}

/// A copy no wider than `DISPLAY_DENSITY` times the CSS width it is shown at. None when the
/// original is already that small, cannot be decoded, or the copy would not be smaller.
pub fn scaled_for_display(data_uri: &str, display_width: u32) -> Option<String> {
    let target = display_width.max(1).saturating_mul(DISPLAY_DENSITY);
    let picture = decode(data_uri)?;
    if picture.width() <= target {
        return None;
    }
    let height = (u64::from(picture.height()) * u64::from(target) / u64::from(picture.width()))
        .max(1) as u32;
    let scaled = picture.resize_exact(target, height, FilterType::Lanczos3);
    let (bytes, mime) = encode(&scaled, 85)?;
    let uri = format!("data:{mime};base64,{}", STANDARD.encode(bytes));
    (uri.len() < data_uri.len()).then_some(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> String {
        let mut picture = image::RgbImage::new(width, height);
        let mut seed = 7_u32;
        for pixel in picture.pixels_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *pixel = image::Rgb([seed as u8, (seed >> 8) as u8, (seed >> 16) as u8]);
        }
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(picture)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        format!("data:image/png;base64,{}", STANDARD.encode(bytes))
    }

    fn size(uri: &str) -> (u32, u32, usize) {
        let bytes = STANDARD.decode(uri.split_once(',').unwrap().1).unwrap();
        let picture = image::load_from_memory(&bytes).unwrap();
        (picture.width(), picture.height(), bytes.len())
    }

    #[test]
    fn previews_shrink_step_by_step_within_the_token_budget() {
        let preview = ai_preview(&png(3000, 1500)).unwrap();
        assert!(preview.starts_with("data:image/jpeg;base64,"));
        let (width, height, bytes) = size(&preview);
        assert!(width <= AI_PREVIEW_EDGE && height <= AI_PREVIEW_EDGE);
        assert!(bytes <= AI_PREVIEW_BYTES);
        assert_eq!(width, 2 * height);
        assert!(ai_preview("data:image/gif;base64,R0lGODlhAQABAAAAACw=").is_none());
    }

    #[test]
    fn display_copies_keep_twice_the_shown_width_and_never_grow() {
        let original = png(2400, 1200);
        let scaled = scaled_for_display(&original, 300).unwrap();
        assert_eq!(size(&scaled).0, 600);
        assert_eq!(size(&scaled).1, 300);
        assert!(scaled_for_display(&original, 1600).is_none());
    }
}
