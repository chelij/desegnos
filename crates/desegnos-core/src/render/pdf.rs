//! A minimal PDF writer for slide decks: vector paths, strokes and raster pictures.
//!
//! Every scene is one page, drawn in canvas order. Glyphs are already outlines, so the PDF
//! embeds no font program and needs no font installed. Pictures are decoded to raw RGB and
//! deflated, so reading the PDF needs no image codec.
use super::{Item, Rgba, Scene, Segment};
use flate2::{Compression, write::ZlibEncoder};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;

/// PDF points per canvas pixel (96 dpi pixels to 72 dpi points).
const SCALE: f64 = 0.75;

pub(super) fn deck(scenes: &[Scene]) -> Vec<u8> {
    let mut pictures: Vec<(String, Vec<u8>)> = Vec::new();
    let mut picture_ids: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (scene_index, scene) in scenes.iter().enumerate() {
        let mut ordinal = 0_usize;
        collect_pictures(&scene.items, scene_index, &mut ordinal, &mut pictures, &mut picture_ids);
    }
    let alphas = collect_alphas(scenes);
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
    let states: Vec<String> = (0..alphas.len())
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
    for (index, scene) in scenes.iter().enumerate() {
        let page_id = first_page + index * 2;
        writer.dict(
            page_id,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.3} {:.3}] /Resources 3 0 R /Contents {} 0 R >>",
                scene.width * SCALE,
                scene.height * SCALE,
                page_id + 1
            ),
        );
        let stream = content(scene, index, &alphas, &picture_ids);
        writer.stream(page_id + 1, stream.into_bytes(), false);
    }
    for (index, (mime, bytes)) in pictures.iter().enumerate() {
        let image = image_stream(mime, bytes);
        writer.stream(first_picture + index, image, true);
    }
    for (index, alpha) in alphas.iter().enumerate() {
        writer.dict(
            first_state + index,
            format!("<< /Type /ExtGState /ca {alpha:.3} /CA {alpha:.3} >>"),
        );
    }
    writer.finish()
}

fn collect_pictures(
    items: &[Item],
    scene: usize,
    ordinal: &mut usize,
    pictures: &mut Vec<(String, Vec<u8>)>,
    ids: &mut BTreeMap<(usize, usize), usize>,
) {
    for item in items {
        match item {
            Item::Picture(picture) => {
                ids.insert((scene, *ordinal), pictures.len());
                *ordinal += 1;
                pictures.push((picture.mime.clone(), picture.bytes.clone()));
            }
            Item::Group { items, .. } => collect_pictures(items, scene, ordinal, pictures, ids),
            Item::Shape(_) => {}
        }
    }
}

fn collect_alphas(scenes: &[Scene]) -> Vec<f64> {
    let mut alphas: Vec<f64> = Vec::new();
    fn walk(items: &[Item], alphas: &mut Vec<f64>) {
        for item in items {
            if let Item::Group { alpha, items, .. } = item {
                let rounded = (alpha * 100.0).round() / 100.0;
                if rounded < 0.995 && !alphas.iter().any(|value| (*value - rounded).abs() < 0.001)
                {
                    alphas.push(rounded);
                }
                walk(items, alphas);
            }
        }
    }
    for scene in scenes {
        walk(&scene.items, &mut alphas);
    }
    alphas
}

fn content(
    scene: &Scene,
    scene_index: usize,
    alphas: &[f64],
    picture_ids: &BTreeMap<(usize, usize), usize>,
) -> String {
    let mut stream = String::new();
    let mut picture = 0_usize;
    write_items(
        &mut stream,
        scene,
        &scene.items,
        alphas,
        picture_ids,
        scene_index,
        &mut picture,
    );
    stream
}

#[allow(clippy::too_many_arguments)]
fn write_items(
    stream: &mut String,
    scene: &Scene,
    items: &[Item],
    alphas: &[f64],
    picture_ids: &BTreeMap<(usize, usize), usize>,
    scene_index: usize,
    picture: &mut usize,
) {
    for item in items {
        match item {
            Item::Shape(shape) => {
                let Some(path) = scene.paths.get(shape.path_ref) else {
                    continue;
                };
                if let Some(fill) = shape.fill {
                    set_fill(stream, fill);
                    if let Some((stroke, width)) = shape.stroke {
                        set_stroke(stream, stroke);
                        let _ = write!(stream, "{:.3} w ", width * SCALE);
                        write_path(stream, scene, path);
                        stream.push_str("B\n");
                    } else {
                        write_path(stream, scene, path);
                        stream.push_str("f\n");
                    }
                } else if let Some((stroke, width)) = shape.stroke {
                    set_stroke(stream, stroke);
                    let _ = write!(stream, "{:.3} w ", width * SCALE);
                    write_path(stream, scene, path);
                    stream.push_str("S\n");
                }
            }
            Item::Picture(picture_item) => {
                let ordinal = *picture;
                *picture += 1;
                let Some(index) = picture_ids.get(&(scene_index, ordinal)) else {
                    continue;
                };
                let _ = writeln!(
                    stream,
                    "q {:.3} 0 0 {:.3} {:.3} {:.3} cm /Im{index} Do Q",
                    picture_item.width * SCALE,
                    picture_item.height * SCALE,
                    picture_item.x * SCALE,
                    (scene.height - picture_item.y - picture_item.height) * SCALE
                );
            }
            Item::Group {
                clip,
                alpha,
                items,
            } => {
                stream.push_str("q\n");
                if let Some(rect) = clip {
                    let _ = writeln!(
                        stream,
                        "{:.3} {:.3} {:.3} {:.3} re W n",
                        rect.x * SCALE,
                        (scene.height - rect.y - rect.height) * SCALE,
                        rect.width * SCALE,
                        rect.height * SCALE
                    );
                }
                if *alpha < 0.995 {
                    let rounded = (alpha * 100.0).round() / 100.0;
                    let index = alphas
                        .iter()
                        .position(|value| (*value - rounded).abs() < 0.001)
                        .unwrap_or(0);
                    let _ = writeln!(stream, "/GS{index} gs");
                }
                write_items(stream, scene, items, alphas, picture_ids, scene_index, picture);
                stream.push_str("Q\n");
            }
        }
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

fn write_path(stream: &mut String, scene: &Scene, path: &super::Path) {
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
fn previous_point(path: &super::Path, segment: &Segment) -> Option<super::Point> {
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
    objects: BTreeMap<usize, Vec<u8>>,
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
            let _ = write!(
                bytes,
                "<< /Length {} >>\nstream\n",
                body.len()
            );
            let mut body = body;
            bytes.append(&mut body);
            bytes.extend_from_slice(b"\nendstream");
        }
        bytes.extend_from_slice(b"\nendobj\n");
        self.objects.insert(id, bytes);
    }

    fn finish(mut self) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = BTreeMap::new();
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
                Some(offset) => out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()),
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
