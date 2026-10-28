//! A minimal PDF writer for slide decks: vector paths, strokes, clipped pictures and shadows.
//!
//! Every scene is one page, drawn in canvas order. Glyphs are already outlines, so the PDF
//! embeds no font program and needs no font installed. Pictures are decoded to raw RGB and
//! deflated, so reading the PDF needs no image codec.
//!
//! Two things PDF cannot draw natively are approximated. Opacity is a graphics state constant,
//! so the writer multiplies opacity down the tree itself and switches state only where the
//! product changes; the same mechanism carries a fill's own alpha, which PDF colours do not
//! have. A Gaussian-blurred shadow becomes [`SHADOW_LAYERS`] silhouettes growing outwards with
//! one fixed alpha each, chosen so the overlap in the middle adds up to the shadow's own alpha:
//! a soft ramp in place of the blur that PNG and SVG get.
use super::{
    Clip, Group, Item, Path, Picture, Point, Rgba, Rotate, Scene, Segment, Shadow, ShadowShape,
    Shape,
};
use flate2::{Compression, write::ZlibEncoder};
use std::fmt::Write as _;
use std::io::{Cursor, Write as _};

/// PDF points per canvas pixel (96 dpi pixels to 72 dpi points).
const SCALE: f64 = 0.75;
/// Silhouettes that stand in for one Gaussian-blurred shadow.
const SHADOW_LAYERS: usize = 6;

pub(super) fn deck(scenes: &[Scene]) -> Vec<u8> {
    // Content comes first: it is what discovers the pictures and the opacity states the
    // resources dictionary has to hold.
    let mut alphas = Alphas::default();
    let mut pictures: Vec<(String, Vec<u8>)> = Vec::new();
    let mut streams: Vec<String> = Vec::new();
    for scene in scenes {
        let mut page = Page {
            scene,
            stream: String::new(),
            opacity: 1.0,
            current: 1.0,
            alphas: &mut alphas,
            pictures: &mut pictures,
        };
        page.items(&scene.items);
        streams.push(page.stream);
    }

    // 1 catalog, 2 pages, 3 resources, then one page and one content object per scene.
    let first_page = 4;
    let first_picture = first_page + scenes.len() * 2;
    let first_state = first_picture + pictures.len();

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
    let xobjects: Vec<String> = (0..pictures.len())
        .map(|index| format!("/Im{index} {} 0 R", first_picture + index))
        .collect();
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
        // The silhouette is its own state scope: its layers each want a different opacity, and
        // `Q` puts the page's opacity back for whatever is painted over it.
        self.stream.push_str("q\n");
        set_fill(&mut self.stream, shadow.fill);
        let sigma = shadow.blur / 2.0;
        if sigma <= 0.01 {
            self.paint_alpha(shadow.fill.a);
            let path = shadow_path(shadow.shape, 0.0);
            write_path(&mut self.stream, scene, &path);
            self.stream.push_str("f\n");
        } else {
            // A Gaussian fades out about two standard deviations from the silhouette, so the
            // outermost stand-in grows by that much and the innermost is the silhouette itself.
            let layers = SHADOW_LAYERS;
            let spread = sigma * 2.0;
            let step = spread / (layers - 1) as f64;
            // Overlapping the layers evenly gets the middle back to the shadow's own alpha.
            let layer_alpha = 1.0 - (1.0 - shadow.fill.a.clamp(0.0, 1.0)).powf(1.0 / layers as f64);
            for layer in (0..layers).rev() {
                self.paint_alpha(layer_alpha);
                let path = shadow_path(shadow.shape, layer as f64 * step);
                write_path(&mut self.stream, scene, &path);
                self.stream.push_str("f\n");
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

/// The silhouette a shadow layer draws, grown by `spread` on every side.
fn shadow_path(shape: ShadowShape, spread: f64) -> Path {
    let rect = super::inflate(shape.bounds(), spread);
    match shape {
        ShadowShape::Rect { radius, .. } => Path::rect(rect, radius + spread),
        ShadowShape::Ellipse(_) => Path::ellipse(rect),
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
    let mut compressed = Vec::new();
    {
        let mut encoder = ZlibEncoder::new(&mut compressed, Compression::default());
        let _ = encoder.write_all(&rgb);
        let _ = encoder.finish();
    }
    let mut stream = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
        compressed.len()
    )
    .into_bytes();
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
