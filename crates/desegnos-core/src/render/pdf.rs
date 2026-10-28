//! A minimal PDF writer for slide decks: vector paths, strokes, clipped pictures and shadows.
//!
//! Every scene is one page, drawn in canvas order. Glyphs are already outlines, so the PDF
//! embeds no font program and needs no font installed. Pictures are decoded to raw RGB and
//! deflated, so reading the PDF needs no image codec.
//!
//! Opacity is multiplied down the tree and carried by graphics states, including a fill's
//! own alpha. Blurred shadows use tiny-skia silhouettes and Gaussian alpha masks (sigma =
//! blur / 2), padded by three sigma and embedded as RGB image XObjects with soft masks.
//! Masks use two pixels per PDF point unless the four-megapixel limit requires downsampling.
//! Identical shadows share an XObject across pages; sharp shadows remain vector paths.
use super::{
    Clip, Group, Item, Path, Picture, Point, Rgba, Rotate, Scene, Segment, Shadow, ShadowShape,
    Shape,
};
use flate2::{Compression, write::ZlibEncoder};
use std::fmt::Write as _;
use std::io::{Cursor, Write as _};

/// PDF points per canvas pixel (96 dpi pixels to 72 dpi points).
const SCALE: f64 = 0.75;
/// Maximum raster area per shadow, bounding allocation and embedded image size.
const MAX_SHADOW_PIXELS: u32 = 4_000_000;

pub(super) fn deck(scenes: &[Scene]) -> Vec<u8> {
    // Content comes first: it is what discovers the pictures and the opacity states the
    // resources dictionary has to hold.
    let mut alphas = Alphas::default();
    let mut pictures: Vec<(String, Vec<u8>)> = Vec::new();
    let mut shadows = Vec::new();
    let mut streams: Vec<String> = Vec::new();
    for scene in scenes {
        let mut page = Page {
            scene,
            stream: String::new(),
            opacity: 1.0,
            current: 1.0,
            alphas: &mut alphas,
            pictures: &mut pictures,
            shadows: &mut shadows,
        };
        page.items(&scene.items);
        streams.push(page.stream);
    }

    // 1 catalog, 2 pages, 3 resources, then one page and one content object per scene.
    let first_page = 4;
    let first_picture = first_page + scenes.len() * 2;
    let first_shadow = first_picture + pictures.len();
    let first_state = first_shadow + shadows.len() * 2;

    let mut writer = Writer::default();
    writer.dict(1, "<< /Type /Catalog /Pages 2 0 R >>".into());
    let kids: Vec<String> = (0..scenes.len())
        .map(|index| format!("{} 0 R", first_page + index * 2))
        .collect();
    writer.dict(
        2,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            scenes.len()
        ),
    );
    let mut xobjects: Vec<String> = (0..pictures.len())
        .map(|index| format!("/Im{index} {} 0 R", first_picture + index))
        .collect();
    xobjects.extend(
        (0..shadows.len()).map(|index| format!("/Sh{index} {} 0 R", first_shadow + index * 2)),
    );
    let states: Vec<String> = (0..alphas.values.len())
        .map(|index| format!("/GS{index} {} 0 R", first_state + index))
        .collect();
    writer.dict(
        3,
        format!(
            "<< /XObject << {} >> /ExtGState << {} >> >>",
            xobjects.join(" "),
            states.join(" ")
        ),
    );
    for (index, stream) in streams.into_iter().enumerate() {
        let page_id = first_page + index * 2;
        writer.dict(
            page_id,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.3} {:.3}] /Resources 3 0 R /Contents {} 0 R >>",
                scenes[index].width * SCALE,
                scenes[index].height * SCALE,
                page_id + 1
            ),
        );
        writer.stream(page_id + 1, stream.into_bytes(), false);
    }
    for (index, (mime, bytes)) in pictures.iter().enumerate() {
        let image = image_stream(mime, bytes);
        writer.stream(first_picture + index, image, true);
    }
    for (index, shadow) in shadows.iter().enumerate() {
        let id = first_shadow + index * 2;
        let rgb = [shadow.fill.r, shadow.fill.g, shadow.fill.b]
            .repeat(shadow.width as usize * shadow.height as usize);
        writer.stream(
            id,
            raster_stream(
                shadow.width,
                shadow.height,
                "DeviceRGB",
                &rgb,
                &format!(" /SMask {} 0 R", id + 1),
            ),
            true,
        );
        writer.stream(
            id + 1,
            raster_stream(shadow.width, shadow.height, "DeviceGray", &shadow.mask, ""),
            true,
        );
    }
    for (index, alpha) in alphas.values.iter().enumerate() {
        writer.dict(
            first_state + index,
            format!("<< /Type /ExtGState /ca {alpha:.3} /CA {alpha:.3} >>"),
        );
    }
    writer.finish()
}

/// The opacity states one deck needs, rounded so the same product reuses one state.
#[derive(Default)]
struct Alphas {
    values: Vec<f64>,
}

impl Alphas {
    fn round(value: f64) -> f64 {
        (value.clamp(0.0, 1.0) * 100.0).round() / 100.0
    }

    fn index(&mut self, value: f64) -> usize {
        let value = Self::round(value);
        if let Some(index) = self
            .values
            .iter()
            .position(|stored| (stored - value).abs() < 0.000_1)
        {
            return index;
        }
        self.values.push(value);
        self.values.len() - 1
    }
}

/// One page's content stream, with everything it needs to keep its own state.
struct Page<'a> {
    scene: &'a Scene,
    stream: String,
    /// The opacity the tree has multiplied down to here.
    opacity: f64,
    /// The alpha the graphics state currently paints with, which `q`/`Q` save and restore.
    current: f64,
    alphas: &'a mut Alphas,
    pictures: &'a mut Vec<(String, Vec<u8>)>,
    shadows: &'a mut Vec<ShadowImage>,
}

impl Page<'_> {
    fn items(&mut self, items: &[Item]) {
        for item in items {
            match item {
                Item::Shape(shape) => self.shape(shape),
                Item::Picture(picture) => self.picture(picture),
                Item::Shadow(shadow) => self.shadow(shadow),
                Item::Group(group) => self.group(group),
            }
        }
    }

    fn shape(&mut self, shape: &Shape) {
        let scene = self.scene;
        let Some(path) = scene.paths.get(shape.path_ref) else {
            return;
        };
        if let Some(fill) = shape.fill {
            self.paint_alpha(fill.a);
            set_fill(&mut self.stream, fill);
            write_path(&mut self.stream, scene, path);
            self.stream.push_str("f\n");
        }
        if let Some((stroke, width)) = shape.stroke {
            self.paint_alpha(stroke.a);
            set_stroke(&mut self.stream, stroke);
            let _ = write!(self.stream, "{:.3} w ", width * SCALE);
            write_path(&mut self.stream, scene, path);
            self.stream.push_str("S\n");
        }
    }

    fn picture(&mut self, picture: &Picture) {
        let index = self.pictures.len();
        self.pictures
            .push((picture.mime.clone(), picture.bytes.clone()));
        // `object-fit: cover`: scale to fill the box, centre it and let the clip crop it.
        let (intrinsic_width, intrinsic_height) =
            picture_dimensions(&picture.bytes).unwrap_or((picture.width, picture.height));
        let scale = (picture.width / intrinsic_width).max(picture.height / intrinsic_height);
        let width = intrinsic_width * scale;
        let height = intrinsic_height * scale;
        let x = picture.x - (width - picture.width) / 2.0;
        let y = picture.y - (height - picture.height) / 2.0;
        let _ = write!(
            self.stream,
            "q\n{:.3} {:.3} {:.3} {:.3} re W n\n{:.3} 0 0 {:.3} {:.3} {:.3} cm\n/Im{index} Do\nQ\n",
            picture.x * SCALE,
            pdf_y(self.scene, picture.y + picture.height),
            picture.width * SCALE,
            picture.height * SCALE,
            width * SCALE,
            height * SCALE,
            x * SCALE,
            pdf_y(self.scene, y + height)
        );
    }

    fn shadow(&mut self, shadow: &Shadow) {
        let scene = self.scene;
        let (parent_opacity, parent_current) = (self.opacity, self.current);
        // Keep the inherited transform (including rotation), clipping and group opacity.
        self.stream.push_str("q\n");
        self.paint_alpha(shadow.fill.a);
        if shadow.blur <= 0.0 {
            set_fill(&mut self.stream, shadow.fill);
            write_path(&mut self.stream, scene, &shadow_path(shadow.shape));
            self.stream.push_str("f\n");
        } else {
            let key = ShadowImage::key(shadow);
            let index = self
                .shadows
                .iter()
                .position(|image| image.key == key)
                .or_else(|| {
                    let image = ShadowImage::new(shadow)?;
                    self.shadows.push(image);
                    Some(self.shadows.len() - 1)
                });
            if let Some(index) = index {
                let image = &self.shadows[index];
                let bounds = shadow.shape.bounds();
                let padding = shadow.blur * 1.5;
                let width = f64::from(image.width) / image.scale;
                let height = f64::from(image.height) / image.scale;
                let _ = writeln!(
                    self.stream,
                    "{:.6} 0 0 {:.6} {:.6} {:.6} cm\n/Sh{index} Do",
                    width * SCALE,
                    height * SCALE,
                    (bounds.x - padding) * SCALE,
                    pdf_y(scene, bounds.y - padding + height)
                );
            }
        }
        self.stream.push_str("Q\n");
        self.opacity = parent_opacity;
        self.current = parent_current;
    }

    fn group(&mut self, group: &Group) {
        let scene = self.scene;
        let (parent_opacity, parent_current) = (self.opacity, self.current);
        self.stream.push_str("q\n");
        if let Some(rotate) = group.rotate {
            write_rotation(&mut self.stream, scene, &rotate);
        }
        if let Some(clip) = group.clip {
            let path = clip_path(clip);
            write_path(&mut self.stream, scene, &path);
            self.stream.push_str("W n\n");
        }
        self.opacity = parent_opacity * group.alpha;
        self.items(&group.items);
        self.stream.push_str("Q\n");
        self.opacity = parent_opacity;
        self.current = parent_current;
    }

    /// Selects the opacity one paint operation needs, writing it only when the graphics state
    /// does not already paint with it.
    fn paint_alpha(&mut self, colour_alpha: f64) {
        let value = Alphas::round(self.opacity * colour_alpha);
        if (value - self.current).abs() < 0.005 {
            return;
        }
        let index = self.alphas.index(value);
        let _ = writeln!(self.stream, "/GS{index} gs");
        self.current = value;
    }
}

fn clip_path(clip: Clip) -> Path {
    Path::rect(clip.rect, clip.radius)
}

/// The same silhouette used by the SVG/PNG renderer, in canvas coordinates.
fn shadow_path(shape: ShadowShape) -> Path {
    match shape {
        ShadowShape::Rect { rect, radius } => Path::rect(rect, radius),
        ShadowShape::Ellipse(rect) => Path::ellipse(rect),
    }
}

struct ShadowImage {
    key: [u64; 9],
    fill: Rgba,
    width: u32,
    height: u32,
    scale: f64,
    mask: Vec<u8>,
}

impl ShadowImage {
    // Position and the current transform do not change the raster silhouette.
    fn key(shadow: &Shadow) -> [u64; 9] {
        let bounds = shadow.shape.bounds();
        let (kind, radius) = match shadow.shape {
            ShadowShape::Rect { radius, .. } => (0, radius),
            ShadowShape::Ellipse(_) => (1, 0.0),
        };
        [
            kind,
            bounds.width.to_bits(),
            bounds.height.to_bits(),
            radius.to_bits(),
            u64::from(shadow.fill.r),
            u64::from(shadow.fill.g),
            u64::from(shadow.fill.b),
            shadow.fill.a.to_bits(),
            shadow.blur.to_bits(),
        ]
    }

    fn new(shadow: &Shadow) -> Option<Self> {
        let bounds = shadow.shape.bounds();
        let sigma = shadow.blur / 2.0;
        let padding = sigma * 3.0;
        let width = bounds.width + padding * 2.0;
        let height = bounds.height + padding * 2.0;
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return None;
        }
        // Work in canvas units: one canvas pixel is SCALE PDF points. Reserve room for
        // rounding so even extremely thin masks stay within the pixel budget.
        let limit = f64::from(MAX_SHADOW_PIXELS);
        let mut scale = (2.0 * SCALE)
            .min((limit / (width * height)).sqrt())
            .min(limit / width)
            .min(limit / height);
        let dimensions = |scale: f64| {
            (
                (width * scale).ceil().max(1.0) as u32,
                (height * scale).ceil().max(1.0) as u32,
            )
        };
        let (mut pixel_width, mut pixel_height) = dimensions(scale);
        while u64::from(pixel_width) * u64::from(pixel_height) > u64::from(MAX_SHADOW_PIXELS) {
            scale *= 0.99 * (limit / (f64::from(pixel_width) * f64::from(pixel_height))).sqrt();
            (pixel_width, pixel_height) = dimensions(scale);
        }
        let mut pixmap = tiny_skia::Pixmap::new(pixel_width, pixel_height)?;
        // Build at the origin so translation never changes the cache or f32 precision.
        let local = super::Rect {
            x: padding,
            y: padding,
            ..bounds
        };
        let shape = match shadow.shape {
            ShadowShape::Rect { radius, .. } => ShadowShape::Rect {
                rect: local,
                radius,
            },
            ShadowShape::Ellipse(_) => ShadowShape::Ellipse(local),
        };
        let mut builder = tiny_skia::PathBuilder::new();
        let point = |p: Point| ((p.x * scale) as f32, (p.y * scale) as f32);
        for segment in shadow_path(shape).segments {
            match segment {
                Segment::Move(p) => {
                    let (x, y) = point(p);
                    builder.move_to(x, y);
                }
                Segment::Line(p) => {
                    let (x, y) = point(p);
                    builder.line_to(x, y);
                }
                Segment::Quad(c, p) => {
                    let (cx, cy) = point(c);
                    let (x, y) = point(p);
                    builder.quad_to(cx, cy, x, y);
                }
                Segment::Cubic(a, b, p) => {
                    let (ax, ay) = point(a);
                    let (bx, by) = point(b);
                    let (x, y) = point(p);
                    builder.cubic_to(ax, ay, bx, by, x, y);
                }
                Segment::Close => builder.close(),
            }
        }
        let path = builder.finish()?;
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(255, 255, 255, 255);
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
        let mask = image::GrayImage::from_raw(
            pixel_width,
            pixel_height,
            pixmap.pixels().iter().map(|pixel| pixel.alpha()).collect(),
        )?;
        let mask = image::imageops::blur(&mask, (sigma * scale) as f32).into_raw();
        Some(Self {
            key: Self::key(shadow),
            fill: shadow.fill,
            width: pixel_width,
            height: pixel_height,
            scale,
            mask,
        })
    }
}

fn set_fill(stream: &mut String, colour: Rgba) {
    let (r, g, b) = colour.components();
    let _ = write!(stream, "{r:.3} {g:.3} {b:.3} rg ");
}

fn set_stroke(stream: &mut String, colour: Rgba) {
    let (r, g, b) = colour.components();
    let _ = write!(stream, "{r:.3} {g:.3} {b:.3} RG ");
}

/// A clockwise canvas rotation about its centre, as PDF's counter-clockwise `cm` matrix.
fn write_rotation(stream: &mut String, scene: &Scene, rotate: &Rotate) {
    let (sin, cos) = rotate.degrees.to_radians().sin_cos();
    let cx = rotate.cx * SCALE;
    let cy = pdf_y(scene, rotate.cy);
    let e = cx - cos * cx - sin * cy;
    let f = cy + sin * cx - cos * cy;
    let _ = writeln!(
        stream,
        "{cos:.6} {:.6} {sin:.6} {cos:.6} {e:.6} {f:.6} cm",
        -sin
    );
}

fn write_path(stream: &mut String, scene: &Scene, path: &Path) {
    for segment in &path.segments {
        match segment {
            Segment::Move(point) => {
                let _ = write!(
                    stream,
                    "{:.3} {:.3} m ",
                    point.x * SCALE,
                    pdf_y(scene, point.y)
                );
            }
            Segment::Line(point) => {
                let _ = write!(
                    stream,
                    "{:.3} {:.3} l ",
                    point.x * SCALE,
                    pdf_y(scene, point.y)
                );
            }
            // PDF has no quadratic curves; each is raised to an equivalent cubic.
            Segment::Quad(control, point) => {
                let start = previous_point(path, segment);
                let start = start.unwrap_or(*control);
                let c1 = (
                    start.x + 2.0 / 3.0 * (control.x - start.x),
                    start.y + 2.0 / 3.0 * (control.y - start.y),
                );
                let c2 = (
                    point.x + 2.0 / 3.0 * (control.x - point.x),
                    point.y + 2.0 / 3.0 * (control.y - point.y),
                );
                let _ = write!(
                    stream,
                    "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c ",
                    c1.0 * SCALE,
                    pdf_y(scene, c1.1),
                    c2.0 * SCALE,
                    pdf_y(scene, c2.1),
                    point.x * SCALE,
                    pdf_y(scene, point.y)
                );
            }
            Segment::Cubic(first, second, point) => {
                let _ = write!(
                    stream,
                    "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c ",
                    first.x * SCALE,
                    pdf_y(scene, first.y),
                    second.x * SCALE,
                    pdf_y(scene, second.y),
                    point.x * SCALE,
                    pdf_y(scene, point.y)
                );
            }
            Segment::Close => stream.push_str("h "),
        }
    }
}

/// The point a segment starts from, used to raise a quadratic curve to a cubic.
fn previous_point(path: &Path, segment: &Segment) -> Option<Point> {
    let index = path
        .segments
        .iter()
        .position(|item| std::ptr::eq(item, segment))?;
    path.segments[..index]
        .iter()
        .rev()
        .find_map(|item| match item {
            Segment::Move(point) | Segment::Line(point) => Some(*point),
            Segment::Quad(_, point) | Segment::Cubic(_, _, point) => Some(*point),
            Segment::Close => None,
        })
}

fn pdf_y(scene: &Scene, y: f64) -> f64 {
    (scene.height - y) * SCALE
}

/// A picture's pixel size, read from its header; `None` when the bytes cannot be decoded.
fn picture_dimensions(bytes: &[u8]) -> Option<(f64, f64)> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    (width > 0 && height > 0).then_some((f64::from(width), f64::from(height)))
}

/// A picture as a PDF image XObject: raw RGB, deflated.
fn image_stream(mime: &str, bytes: &[u8]) -> Vec<u8> {
    let decoded = image::load_from_memory(bytes).ok();
    let (width, height, rgb) = match decoded {
        Some(picture) => {
            let picture = picture.to_rgb8();
            (picture.width(), picture.height(), picture.into_raw())
        }
        None => (1, 1, vec![0_u8, 0, 0]),
    };
    let _ = mime;
    raster_stream(width, height, "DeviceRGB", &rgb, "")
}

/// A deflated image stream; a shadow's RGB image references its grayscale soft mask.
fn raster_stream(
    width: u32,
    height: u32,
    colour_space: &str,
    bytes: &[u8],
    extra: &str,
) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bytes)
        .expect("compress image into memory");
    let compressed = encoder.finish().expect("finish image compression");
    let mut stream = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /{colour_space} /BitsPerComponent 8 /Filter /FlateDecode{extra} /Length {} >>\nstream\n",
        compressed.len()
    ).into_bytes();
    stream.extend_from_slice(&compressed);
    stream.extend_from_slice(b"\nendstream");
    stream
}

#[derive(Default)]
struct Writer {
    objects: std::collections::BTreeMap<usize, Vec<u8>>,
}

impl Writer {
    fn dict(&mut self, id: usize, body: String) {
        let mut bytes = format!("{id} 0 obj\n").into_bytes();
        bytes.extend_from_slice(body.as_bytes());
        bytes.extend_from_slice(b"\nendobj\n");
        self.objects.insert(id, bytes);
    }

    /// `body` is the complete stream, including its dictionary and `endstream` marker.
    fn stream(&mut self, id: usize, body: Vec<u8>, complete: bool) {
        let mut bytes = format!("{id} 0 obj\n").into_bytes();
        if complete {
            bytes.extend_from_slice(&body);
        } else {
            let _ = write!(bytes, "<< /Length {} >>\nstream\n", body.len());
            let mut body = body;
            bytes.append(&mut body);
            bytes.extend_from_slice(b"\nendstream");
        }
        bytes.extend_from_slice(b"\nendobj\n");
        self.objects.insert(id, bytes);
    }

    fn finish(mut self) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = std::collections::BTreeMap::new();
        let max_id = self.objects.keys().max().copied().unwrap_or(0);
        for id in 0..=max_id {
            let Some(bytes) = self.objects.remove(&id) else {
                continue;
            };
            offsets.insert(id, out.len());
            out.extend_from_slice(&bytes);
        }
        let xref = out.len();
        let count = max_id + 1;
        out.extend_from_slice(format!("xref\n0 {count}\n").as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for id in 1..count {
            match offsets.get(&id) {
                Some(offset) => {
                    out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes())
                }
                None => out.extend_from_slice(b"0000000000 65535 f \n"),
            }
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n")
                .as_bytes(),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::Rect;
    use super::*;

    fn shadow(blur: f64) -> Shadow {
        Shadow {
            shape: ShadowShape::Rect {
                rect: Rect {
                    x: 60.0,
                    y: 60.0,
                    width: 80.0,
                    height: 80.0,
                },
                radius: 0.0,
            },
            fill: Rgba {
                r: 30,
                g: 60,
                b: 90,
                a: 0.4,
            },
            blur,
        }
    }

    fn scene(items: Vec<Item>) -> Scene {
        Scene {
            width: 240.0,
            height: 240.0,
            background: Rgba {
                a: 0.0,
                ..Rgba::BLACK
            },
            paths: Vec::new(),
            items,
        }
    }

    #[test]
    fn blurred_shadows_embed_a_soft_mask_and_reuse_it_under_rotation() {
        let first = shadow(20.0);
        let mut moved = first.clone();
        moved.shape = ShadowShape::Rect {
            rect: Rect {
                x: 90.0,
                y: 30.0,
                ..first.shape.bounds()
            },
            radius: 0.0,
        };
        let page = scene(vec![
            Item::Shadow(first),
            Item::Group(Group {
                clip: None,
                rotate: Some(Rotate {
                    degrees: 30.0,
                    cx: 100.0,
                    cy: 100.0,
                }),
                alpha: 0.5,
                items: vec![Item::Shadow(moved)],
            }),
        ]);
        let pdf = deck(&[page.clone(), page]);
        let content = String::from_utf8_lossy(&pdf);
        assert_eq!(content.matches("/SMask ").count(), 1);
        assert_eq!(content.matches("/Subtype /Image").count(), 2);
        assert_eq!(content.matches("/Sh0 Do").count(), 4);
        assert!(content.contains("/ColorSpace /DeviceGray"));
        assert!(content.contains("/ColorSpace /DeviceRGB"));
        assert!(content.contains("/Filter /FlateDecode"));
        assert!(content.contains("0.866025 -0.500000 0.500000 0.866025"));
        assert!(content.contains("105.000000 0 0 105.000000 22.500000 52.500000 cm\n/Sh0 Do"));
        assert!(
            content.contains("/ca 0.200"),
            "group and shadow alpha multiply"
        );
    }

    #[test]
    fn zero_blur_stays_vector() {
        let pdf = deck(&[scene(vec![Item::Shadow(shadow(0.0))])]);
        let content = String::from_utf8_lossy(&pdf);
        assert!(!content.contains("/SMask"));
        assert!(!content.contains("/Subtype /Image"));
        assert!(content.contains("f\n"));
    }

    #[test]
    fn gaussian_mask_matches_png_alpha() {
        let mut shadow = shadow(20.0);
        shadow.fill.a = 1.0;
        let mask = ShadowImage::new(&shadow).unwrap();
        assert_eq!(mask.scale, 2.0 * SCALE);
        assert_eq!((mask.width, mask.height), (210, 210));
        // Supply the three-sigma filter region explicitly: this isolates the PDF blur
        // from scene_svg's region calculation while using the production PNG rasteriser.
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"240\" height=\"240\">\
             <defs><filter id=\"blur0\" filterUnits=\"userSpaceOnUse\" \
             x=\"30\" y=\"30\" width=\"140\" height=\"140\">\
             <feGaussianBlur stdDeviation=\"10\"/></filter></defs>{}</svg>",
            super::super::shadow_svg(&shadow, Some(0))
        );
        let png = super::super::rasterize(&svg, 240.0, 240.0).unwrap();
        let png = image::load_from_memory(&png).unwrap().to_rgba8();
        // At, one sigma outside, and two sigma outside the left edge. These samples
        // distinguish a Gaussian's tails from the old concentric silhouettes.
        for x in [40, 50, 60, 70, 80] {
            let local_x = ((f64::from(x) + 0.5 - 30.0) * mask.scale) as usize;
            let local_y = ((100.5 - 30.0) * mask.scale) as usize;
            let actual = mask.mask[local_y * mask.width as usize + local_x];
            let expected = png.get_pixel(x, 100)[3];
            assert!(
                actual.abs_diff(expected) <= 5,
                "x={x}: mask {actual}, PNG {expected}"
            );
        }
    }

    #[test]
    fn raster_area_is_capped() {
        let mut shadow = shadow(2.0);
        shadow.shape = ShadowShape::Ellipse(Rect {
            x: 0.0,
            y: 0.0,
            width: 4000.0,
            height: 2000.0,
        });
        let mask = ShadowImage::new(&shadow).unwrap();
        assert!(u64::from(mask.width) * u64::from(mask.height) <= u64::from(MAX_SHADOW_PIXELS));
        assert!(mask.scale < 2.0 * SCALE);
    }

    #[test]
    fn qpdf_accepts_shadow_images() {
        let path =
            std::env::temp_dir().join(format!("desegnos-shadow-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(
            &path,
            deck(&[scene(vec![
                Item::Shadow(shadow(20.0)),
                Item::Shadow(shadow(0.0)),
            ])]),
        )
        .unwrap();
        let result = std::process::Command::new("qpdf")
            .arg("--check")
            .arg(&path)
            .output();
        std::fs::remove_file(&path).unwrap();
        let output = match result {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("qpdf is unavailable; skipping PDF validation");
                return;
            }
            Err(error) => panic!("could not run qpdf: {error}"),
        };
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
