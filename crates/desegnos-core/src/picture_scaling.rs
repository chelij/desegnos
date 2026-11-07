//! Picture sizes around generation: a small preview for the AI, and after generation a copy
//! scaled to the size the page shows it at. GIFs are left as they are (they may be animated).
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageDecoder, ImageReader, imageops::FilterType};

pub use crate::limits::AI_PREVIEW_BYTES;
/// A preview this size costs one image tile on every supported provider (~85–350 tokens).
pub use crate::limits::AI_PREVIEW_EDGE;
/// Upper bound reserved per preview when estimating a request.
pub use crate::limits::AI_PREVIEW_TOKEN_RESERVE;
/// Rendered pictures keep twice their CSS size so they stay sharp on high-density screens.
const DISPLAY_DENSITY: u32 = 2;

fn decode(data_uri: &str) -> crate::Result<DynamicImage> {
    let (mime, bytes) = crate::limits::decode_image_data_uri(data_uri)?;
    crate::ensure(
        matches!(mime, "image/png" | "image/jpeg" | "image/webp"),
        "This image format cannot be scaled.",
    )?;
    let mut decoder = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| crate::Error(format!("Cannot read image format: {error}")))?
        .into_decoder()
        .map_err(|error| crate::Error(format!("Cannot decode image: {error}")))?;
    let (width, height) = decoder.dimensions();
    crate::ensure(
        width > 0
            && height > 0
            && u64::from(width) * u64::from(height) <= crate::limits::MAX_IMAGE_PIXELS,
        "The image exceeds the pixel limit.",
    )?;
    // Browsers show camera pictures upright, so re-encoded copies must too.
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = DynamicImage::from_decoder(decoder)
        .map_err(|error| crate::Error(format!("Cannot decode image: {error}")))?;
    picture.apply_orientation(orientation);
    Ok(picture)
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

/// Share of pixels with visible transparency (alpha below 250).
fn transparent_share(picture: &DynamicImage) -> f64 {
    if !picture.color().has_alpha() {
        return 0.0;
    }
    let pixels = picture.to_rgba8();
    let count = pixels.pixels().filter(|pixel| pixel[3] < 250).count();
    count as f64 / (u64::from(picture.width()) * u64::from(picture.height())) as f64
}

/// Describe the stored image, rather than its downscaled preview.
pub(crate) fn image_metadata(data_uri: &str) -> serde_json::Value {
    let mut metadata = serde_json::json!({});
    if let Some((mime, _)) = data_uri
        .strip_prefix("data:")
        .and_then(|s| s.split_once(";base64,"))
    {
        metadata["storedFormat"] = serde_json::json!(mime);
    }
    if let Ok(picture) = decode(data_uri) {
        let share = transparent_share(&picture);
        if share > 0.005 {
            metadata["transparent"] = serde_json::json!(true);
            metadata["transparentPercent"] = serde_json::json!((share * 100.0).round() as u8);
        }
    }
    metadata
}

/// A bounded preview for the AI. Meaningfully transparent pictures retain alpha as PNG;
/// opaque pictures use JPEG. Undecodable pictures (GIF) travel by name only.
pub fn ai_preview(data_uri: &str) -> Option<String> {
    let picture = decode(data_uri).ok()?;
    preview(picture)
}

fn preview(picture: DynamicImage) -> Option<String> {
    let mut picture = if transparent_share(&picture) > 0.005 {
        picture
    } else {
        flattened(&picture)
    };
    loop {
        let edge = picture.width().max(picture.height());
        if edge > AI_PREVIEW_EDGE {
            let next = (edge * 3 / 4).max(AI_PREVIEW_EDGE);
            picture = picture.resize(next, next, FilterType::Triangle);
            continue;
        }
        let (bytes, mime) = encode(&picture, 75)?;
        if bytes.len() <= AI_PREVIEW_BYTES {
            return Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)));
        }
        // PNG cannot trade quality for size; keep shrinking until it fits.
        let next = (edge * 3 / 4).max(1);
        picture = picture.resize(next, next, FilterType::Triangle);
    }
}

/// A copy no wider than `DISPLAY_DENSITY` times the CSS width it is shown at. None when the
/// original is already that small, cannot be decoded, or the copy would not be smaller.
pub fn scaled_for_display(data_uri: &str, display_width: u32) -> crate::Result<Option<String>> {
    let (mime, _) = crate::limits::decode_image_data_uri(data_uri)?;
    if matches!(mime, "image/gif" | "image/svg+xml") {
        return Ok(None);
    }
    let target = display_width.max(1).saturating_mul(DISPLAY_DENSITY);
    let picture = decode(data_uri)?;
    if picture.width() <= target {
        return Ok(None);
    }
    let height = (u64::from(picture.height()) * u64::from(target) / u64::from(picture.width()))
        .max(1) as u32;
    let scaled = picture.resize_exact(target, height, FilterType::Lanczos3);
    let (bytes, mime) = encode(&scaled, 85)
        .ok_or_else(|| crate::Error("Cannot encode the resized image.".into()))?;
    let uri = format!("data:{mime};base64,{}", STANDARD.encode(bytes));
    Ok((uri.len() < data_uri.len()).then_some(uri))
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
    fn transparent_png_and_webp_previews_preserve_alpha_within_budget() {
        let mut pixels = image::RgbaImage::new(1024, 512);
        let mut seed = 7_u32;
        for (x, _, pixel) in pixels.enumerate_pixels_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *pixel = image::Rgba([
                seed as u8,
                (seed >> 8) as u8,
                (seed >> 16) as u8,
                if x < 512 { 0 } else { 128 },
            ]);
        }
        for (format, mime) in [
            (image::ImageFormat::Png, "image/png"),
            (image::ImageFormat::WebP, "image/webp"),
        ] {
            let mut bytes = Vec::new();
            DynamicImage::ImageRgba8(pixels.clone())
                .write_to(&mut std::io::Cursor::new(&mut bytes), format)
                .unwrap();
            let uri = format!("data:{mime};base64,{}", STANDARD.encode(bytes));
            let preview = ai_preview(&uri).unwrap();
            assert!(preview.starts_with("data:image/png;base64,"));
            let (width, height, bytes) = size(&preview);
            assert!(width <= AI_PREVIEW_EDGE && height <= AI_PREVIEW_EDGE);
            assert!(
                width < AI_PREVIEW_EDGE,
                "noisy PNG must shrink further to fit"
            );
            assert!(bytes <= AI_PREVIEW_BYTES);
            let decoded = decode(&preview).unwrap().to_rgba8();
            assert_eq!(decoded.get_pixel(0, 0)[3], 0);
            assert_eq!(decoded.get_pixel(width - 1, 0)[3], 128);
            assert_eq!(
                image_metadata(&uri),
                serde_json::json!({
                    "storedFormat": mime, "transparent": true, "transparentPercent": 100
                })
            );
        }
    }

    #[test]
    fn transparency_requires_more_than_half_a_percent_below_alpha_250() {
        let mut pixels = image::RgbaImage::from_pixel(100, 100, image::Rgba([10, 20, 30, 250]));
        for pixel in pixels.pixels_mut().take(50) {
            pixel[3] = 0;
        }
        let picture = DynamicImage::ImageRgba8(pixels.clone());
        assert_eq!(transparent_share(&picture), 0.005);
        assert!(
            preview(picture)
                .unwrap()
                .starts_with("data:image/jpeg;base64,")
        );
        pixels.get_pixel_mut(50, 0)[3] = 249;
        assert!(
            preview(DynamicImage::ImageRgba8(pixels))
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
    }

    #[test]
    fn display_copies_keep_twice_the_shown_width_and_never_grow() {
        let original = png(2400, 1200);
        let scaled = scaled_for_display(&original, 300).unwrap().unwrap();
        assert_eq!(size(&scaled).0, 600);
        assert_eq!(size(&scaled).1, 300);
        assert!(scaled_for_display(&original, 1600).unwrap().is_none());
    }
}
