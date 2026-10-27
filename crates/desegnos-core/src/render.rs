//! Direct rendering from the resolved element tree: SVG, PNG and a slide PDF deck.
//!
//! Websites and app screens are built by the AI export job, but slides and images are written
//! here without any network or browser. One scene of paths and pictures is built once and
//! written three ways: a standalone SVG, a PNG rasterised from that SVG, and vector paths in a
//! PDF. Text becomes glyph outlines from bundled Archivo, with system fonts through fontdb as a
//! fallback, so exported files never depend on a font being installed.
mod pdf;

use crate::{
    Element, ElementKind, ElementLayout, ElementStyle, Error, Page, Project, Rect, Result,
    StackAlign, StackJustify, TextAlign,
};
use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use std::collections::BTreeMap;
use ttf_parser::{Face, OutlineBuilder};

/// Bundled Archivo, the brand typeface. SIL OFL; the notice sits beside the file.
const ARCHIVO: &[u8] = include_bytes!("../assets/Archivo-variable.ttf");
/// Cap raster output so a large boundary cannot exhaust memory.
pub const MAX_RENDER_EDGE: u32 = 16_384;
pub const MAX_RENDER_PIXELS: u64 = 64_000_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f64,
}

impl Rgba {
    const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 1.0,
    };

    /// `#rgb`, `#rrggbb`, `#rrggbbaa` or `transparent`; the editor validates the same forms.
    fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("transparent") {
            return Some(Self { a: 0.0, ..Self::BLACK });
        }
        let hex = value.strip_prefix('#')?;
        let digit = |index: usize| -> Option<u8> {
            let byte = *hex.as_bytes().get(index)?;
            (byte as char).to_digit(16).map(|value| value as u8)
        };
        let (r, g, b, a) = match hex.len() {
            3 => (digit(0)? * 17, digit(1)? * 17, digit(2)? * 17, 1.0),
            6 => (
                digit(0)? * 16 + digit(1)?,
                digit(2)? * 16 + digit(3)?,
                digit(4)? * 16 + digit(5)?,
                1.0,
            ),
            8 => (
                digit(0)? * 16 + digit(1)?,
                digit(2)? * 16 + digit(3)?,
                digit(4)? * 16 + digit(5)?,
                f64::from(digit(6)? * 16 + digit(7)?) / 255.0,
            ),
            _ => return None,
        };
        Some(Self { r, g, b, a })
    }

    pub fn faded(self, factor: f64) -> Self {
        Self {
            a: (self.a * factor).clamp(0.0, 1.0),
            ..self
        }
    }

    pub fn svg_colour(self) -> String {
        if self.a >= 1.0 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("rgba({},{},{},{:.3})", self.r, self.g, self.b, self.a)
        }
    }

    pub fn components(self) -> (f64, f64, f64) {
        (
            f64::from(self.r) / 255.0,
            f64::from(self.g) / 255.0,
            f64::from(self.b) / 255.0,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Segment {
    Move(Point),
    Line(Point),
    Quad(Point, Point),
    Cubic(Point, Point, Point),
    Close,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Path {
    pub segments: Vec<Segment>,
}

impl Path {
    fn move_to(&mut self, point: Point) -> &mut Self {
        self.segments.push(Segment::Move(point));
        self
    }

    fn line_to(&mut self, point: Point) -> &mut Self {
        self.segments.push(Segment::Line(point));
        self
    }

    fn quad_to(&mut self, control: Point, point: Point) -> &mut Self {
        self.segments.push(Segment::Quad(control, point));
        self
    }

    fn cubic_to(&mut self, first: Point, second: Point, point: Point) -> &mut Self {
        self.segments.push(Segment::Cubic(first, second, point));
        self
    }

    fn close(&mut self) -> &mut Self {
        self.segments.push(Segment::Close);
        self
    }

    fn svg_data(&self) -> String {
        let mut data = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Move(point) => data.push_str(&format!("M{:.3} {:.3}", point.x, point.y)),
                Segment::Line(point) => data.push_str(&format!("L{:.3} {:.3}", point.x, point.y)),
                Segment::Quad(control, point) => data.push_str(&format!(
                    "Q{:.3} {:.3} {:.3} {:.3}",
                    control.x, control.y, point.x, point.y
                )),
                Segment::Cubic(first, second, point) => data.push_str(&format!(
                    "C{:.3} {:.3} {:.3} {:.3} {:.3} {:.3}",
                    first.x, first.y, second.x, second.y, point.x, point.y
                )),
                Segment::Close => data.push('Z'),
            }
        }
        data
    }

    fn rect(rect: Rect, radius: f64) -> Self {
        let mut path = Path::default();
        let x = rect.x;
        let y = rect.y;
        let width = rect.width.max(0.0);
        let height = rect.height.max(0.0);
        let radius = radius
            .max(0.0)
            .min(width / 2.0)
            .min(height / 2.0)
            .min(40_000.0);
        if radius <= 0.01 || width <= 0.02 || height <= 0.02 {
            path.move_to(Point { x, y })
                .line_to(Point { x: x + width, y })
                .line_to(Point {
                    x: x + width,
                    y: y + height,
                })
                .line_to(Point { x, y: y + height })
                .close();
            return path;
        }
        // Each corner is one quadratic curve through the sharp corner, matching the canvas's
        // CSS corner radius closely enough for exported output.
        path.move_to(Point { x: x + radius, y })
            .line_to(Point {
                x: x + width - radius,
                y,
            })
            .quad_to(
                Point {
                    x: x + width,
                    y,
                },
                Point {
                    x: x + width,
                    y: y + radius,
                },
            )
            .line_to(Point {
                x: x + width,
                y: y + height - radius,
            })
            .quad_to(
                Point {
                    x: x + width,
                    y: y + height,
                },
                Point {
                    x: x + width - radius,
                    y: y + height,
                },
            )
            .line_to(Point {
                x: x + radius,
                y: y + height,
            })
            .quad_to(
                Point { x, y: y + height },
                Point {
                    x,
                    y: y + height - radius,
                },
            )
            .line_to(Point { x, y: y + radius })
            .quad_to(Point { x, y }, Point { x: x + radius, y });
        path.close();
        path
    }

    fn ellipse(rect: Rect) -> Self {
        const KAPPA: f64 = 0.552_284_749_831;
        let mut path = Path::default();
        let cx = rect.x + rect.width / 2.0;
        let cy = rect.y + rect.height / 2.0;
        let rx = rect.width / 2.0;
        let ry = rect.height / 2.0;
        let ox = rx * KAPPA;
        let oy = ry * KAPPA;
        path.move_to(Point { x: cx + rx, y: cy })
            .cubic_to(
                Point { x: cx + rx, y: cy + oy },
                Point { x: cx + ox, y: cy + ry },
                Point { x: cx, y: cy + ry },
            )
            .cubic_to(
                Point { x: cx - ox, y: cy + ry },
                Point { x: cx - rx, y: cy + oy },
                Point { x: cx - rx, y: cy },
            )
            .cubic_to(
                Point { x: cx - rx, y: cy - oy },
                Point { x: cx - ox, y: cy - ry },
                Point { x: cx, y: cy - ry },
            )
            .cubic_to(
                Point { x: cx + ox, y: cy - ry },
                Point { x: cx + rx, y: cy - oy },
                Point { x: cx + rx, y: cy },
            )
            .close();
        path
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Shape {
    pub path_ref: usize,
    pub fill: Option<Rgba>,
    pub stroke: Option<(Rgba, f64)>,
}

#[derive(Debug, Clone)]
pub(crate) struct Picture {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) enum Item {
    Shape(Shape),
    Picture(Picture),
    Group {
        clip: Option<Rect>,
        alpha: f64,
        items: Vec<Item>,
    },
}

/// One rendered boundary: paths are shared by every shape that uses them.
#[derive(Debug, Clone)]
pub struct Scene {
    pub width: f64,
    pub height: f64,
    pub background: Rgba,
    pub(crate) paths: Vec<Path>,
    pub(crate) items: Vec<Item>,
}

impl Scene {
    fn push_path(&mut self, path: Path) -> usize {
        self.paths.push(path);
        self.paths.len() - 1
    }
}

/// Everything a boundary needs to render, already resolved for one size.
pub struct RenderPage<'a> {
    pub page: &'a Page,
    pub width: f64,
    pub height: f64,
    pub elements: Vec<Element>,
}

impl<'a> RenderPage<'a> {
    pub fn new(project: &Project, page: &'a Page, size_id: uuid::Uuid) -> Result<Self> {
        let size = page
            .size(size_id)
            .ok_or_else(|| Error("This size no longer exists.".into()))?;
        Ok(Self {
            page,
            width: f64::from(size.width),
            height: f64::from(size.height),
            elements: crate::resolve_for_size(project, page, size_id),
        })
    }
}

/// A safe file stem for an exported boundary.
pub fn file_stem(value: &str) -> String {
    let stem: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    let stem = stem.trim_matches('-').to_owned();
    if stem.is_empty() { "boundary".into() } else { stem }
}

/// True when every element is a box, group, ellipse, line or text: shapes an SVG can hold
/// without rasterising anything.
pub fn is_vector_representable(elements: &[Element]) -> bool {
    elements.iter().all(|element| {
        matches!(
            element.kind,
            ElementKind::Box
                | ElementKind::Group
                | ElementKind::Ellipse
                | ElementKind::Line
                | ElementKind::Text
        ) && is_vector_representable(&element.children)
    })
}

/// A boundary as a standalone SVG document.
pub fn boundary_svg(project: &Project, page: &Page, size_id: uuid::Uuid) -> Result<String> {
    let render = RenderPage::new(project, page, size_id)?;
    let scene = build_scene(project, &render);
    Ok(scene_svg(&scene))
}

/// A boundary as a PNG, rasterised from its SVG so both agree exactly.
pub fn boundary_png(project: &Project, page: &Page, size_id: uuid::Uuid) -> Result<Vec<u8>> {
    let render = RenderPage::new(project, page, size_id)?;
    let scene = build_scene(project, &render);
    rasterize(&scene_svg(&scene), scene.width, scene.height)
}

/// One PDF deck with one page per slide, in the order given.
pub fn slide_pdf(project: &Project, slides: &[RenderPage<'_>]) -> Result<Vec<u8>> {
    if slides.is_empty() {
        return Err(Error("No slide boundaries are included.".into()));
    }
    let mut scenes = Vec::with_capacity(slides.len());
    for slide in slides {
        scenes.push(build_scene(project, slide));
    }
    Ok(pdf::deck(&scenes))
}

fn rasterize(svg: &str, width: f64, height: f64) -> Result<Vec<u8>> {
    let width = width.max(1.0);
    let height = height.max(1.0);
    let edge = width.max(height);
    if edge > f64::from(MAX_RENDER_EDGE) || width * height > MAX_RENDER_PIXELS as f64 {
        return Err(Error(
            "This boundary is too large to render as one picture.".into(),
        ));
    }
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .map_err(|_| Error("This design could not be drawn as a picture.".into()))?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| Error("This design could not be drawn as a picture.".into()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|_| Error("This design could not be written as a PNG.".into()))
}

// ---------------------------------------------------------------------------
// Scene building
// ---------------------------------------------------------------------------

fn build_scene(project: &Project, page: &RenderPage<'_>) -> Scene {
    let background = match page.page.appearance {
        crate::Appearance::Light => Rgba {
            r: 255,
            g: 255,
            b: 255,
            a: 1.0,
        },
        crate::Appearance::Dark => Rgba {
            r: 18,
            g: 18,
            b: 18,
            a: 1.0,
        },
    };
    let mut scene = Scene {
        width: page.width,
        height: page.height,
        background,
        paths: Vec::new(),
        items: Vec::new(),
    };
    let mut fonts = FontSet::new();
    let mut items = Vec::new();
    for element in &page.elements {
        paint_element(
            project,
            &mut scene,
            &mut fonts,
            element,
            Rect {
                x: element.rect.x,
                y: element.rect.y,
                width: element.rect.width,
                height: element.rect.height,
            },
            &mut items,
        );
    }
    scene.items.push(Item::Group {
        clip: Some(Rect {
            x: 0.0,
            y: 0.0,
            width: page.width,
            height: page.height,
        }),
        alpha: 1.0,
        items,
    });
    scene
}

fn paint_element(
    project: &Project,
    scene: &mut Scene,
    fonts: &mut FontSet,
    element: &Element,
    at: Rect,
    out: &mut Vec<Item>,
) {
    // A line carries no height; every other element needs a real box to draw in.
    if !element.visible
        || at.width <= 0.0
        || (at.height <= 0.0 && element.kind != ElementKind::Line)
    {
        return;
    }
    let style = &element.style;
    let opacity = style.opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    match element.kind {
        ElementKind::Group => {}
        ElementKind::Line => paint_line(scene, style, at, opacity, out),
        ElementKind::Ellipse => {
            let path = scene.push_path(Path::ellipse(at));
            out.push(Item::Shape(Shape {
                path_ref: path,
                fill: style.fill.as_deref().and_then(Rgba::parse).map(|colour| colour.faded(opacity)),
                stroke: stroke_of(style).map(|(colour, width)| (colour.faded(opacity), width)),
            }));
        }
        ElementKind::Image => paint_picture(project, element, at, out),
        _ => paint_box(scene, style, at, opacity, out),
    }
    let mut content: Vec<Item> = Vec::new();
    if text_of(element).is_some_and(|text| !text.trim().is_empty()) {
        paint_text(scene, fonts, element, at, opacity, &mut content);
    }
    paint_children(project, scene, fonts, element, at, &mut content);
    if !content.is_empty() {
        out.push(Item::Group {
            clip: None,
            alpha: 1.0,
            items: content,
        });
    }
}

fn paint_line(scene: &mut Scene, style: &ElementStyle, at: Rect, opacity: f64, out: &mut Vec<Item>) {
    let middle = at.y + at.height.max(1.0) / 2.0;
    let mut path = Path::default();
    path.move_to(Point {
        x: at.x,
        y: middle,
    })
    .line_to(Point {
        x: at.x + at.width.max(1.0),
        y: middle,
    });
    let path_ref = scene.push_path(path);
    let colour = style
        .stroke
        .color
        .as_deref()
        .and_then(Rgba::parse)
        .or_else(|| Rgba::parse(&style.text_style.color))
        .unwrap_or(Rgba::BLACK)
        .faded(opacity);
    out.push(Item::Shape(Shape {
        path_ref,
        fill: None,
        stroke: Some((colour, style.stroke.width.max(1.0))),
    }));
}

fn paint_picture(project: &Project, element: &Element, at: Rect, out: &mut Vec<Item>) {
    let Some((mime, bytes)) = element
        .image
        .and_then(|id| project.picture_bytes(id))
    else {
        return;
    };
    out.push(Item::Picture(Picture {
        x: at.x,
        y: at.y,
        width: at.width,
        height: at.height,
        mime,
        bytes,
    }));
}

fn stroke_of(style: &ElementStyle) -> Option<(Rgba, f64)> {
    style
        .stroke
        .color
        .as_deref()
        .and_then(Rgba::parse)
        .filter(|colour| colour.a > 0.0 && style.stroke.width > 0.0)
        .map(|colour| (colour, style.stroke.width))
}

fn paint_box(
    scene: &mut Scene,
    style: &ElementStyle,
    at: Rect,
    opacity: f64,
    out: &mut Vec<Item>,
) {
    if let Some(colour) = style.shadow.color.as_deref().and_then(Rgba::parse)
        && colour.a > 0.0
    {
        let shadow = Rect {
            x: at.x + style.shadow.x,
            y: at.y + style.shadow.y,
            width: at.width,
            height: at.height,
        };
        let path = scene.push_path(Path::rect(shadow, style.radius));
        out.push(Item::Shape(Shape {
            path_ref: path,
            fill: Some(colour.faded(opacity * 0.18)),
            stroke: None,
        }));
    }
    let fill = style
        .fill
        .as_deref()
        .and_then(Rgba::parse)
        .map(|colour| colour.faded(opacity));
    let stroke = stroke_of(style).map(|(colour, width)| (colour.faded(opacity), width));
    if fill.is_some_and(|colour| colour.a > 0.0) || stroke.is_some() {
        let path = scene.push_path(Path::rect(at, style.radius));
        out.push(Item::Shape(Shape {
            path_ref: path,
            fill,
            stroke,
        }));
    }
}

fn paint_children(
    project: &Project,
    scene: &mut Scene,
    fonts: &mut FontSet,
    element: &Element,
    at: Rect,
    out: &mut Vec<Item>,
) {
    if element.children.is_empty() {
        return;
    }
    match &element.layout {
        ElementLayout::Free => {
            for child in &element.children {
                paint_element(
                    project,
                    scene,
                    fonts,
                    child,
                    Rect {
                        x: at.x + child.rect.x,
                        y: at.y + child.rect.y,
                        width: child.rect.width,
                        height: child.rect.height,
                    },
                    out,
                );
            }
        }
        ElementLayout::Stack {
            direction,
            gap,
            padding,
            align,
            justify,
            wrap,
        } => {
            let row = matches!(direction, crate::StackDirection::Row);
            let padding = padding.max(0.0);
            let gap = gap.max(0.0);
            let inner_x = at.x + padding;
            let inner_y = at.y + padding;
            let main_extent = (if row { at.width } else { at.height } - padding * 2.0).max(0.0);
            let children = &element.children;
            let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
            let mut used = 0.0;
            for (index, child) in children.iter().enumerate() {
                let main = if row {
                    child.rect.width
                } else {
                    child.rect.height
                };
                let line = lines.last_mut().expect("one line exists");
                if line.is_empty() {
                    used = main;
                    line.push(index);
                } else if *wrap && used + gap + main > main_extent {
                    lines.push(vec![index]);
                    used = main;
                } else {
                    used += gap + main;
                    line.push(index);
                }
            }
            let line_main: Vec<f64> = lines
                .iter()
                .map(|line| {
                    line.iter()
                        .enumerate()
                        .map(|(position, index)| {
                            let child = &children[*index];
                            let main = if row {
                                child.rect.width
                            } else {
                                child.rect.height
                            };
                            if position == 0 { main } else { gap + main }
                        })
                        .sum()
                })
                .collect();
            let mut cross = 0.0;
            for (line_index, line) in lines.iter().enumerate() {
                let line_cross = line
                    .iter()
                    .map(|index| {
                        let child = &children[*index];
                        if row {
                            child.rect.height
                        } else {
                            child.rect.width
                        }
                    })
                    .fold(0.0_f64, f64::max);
                let start = match justify {
                    StackJustify::Start | StackJustify::SpaceBetween => 0.0,
                    StackJustify::Center => (main_extent - line_main[line_index]) / 2.0,
                    StackJustify::End => main_extent - line_main[line_index],
                };
                let between = if matches!(justify, StackJustify::SpaceBetween) && line.len() > 1 {
                    (main_extent - line_main[line_index]) / (line.len() - 1) as f64
                } else {
                    0.0
                };
                let mut main = start;
                for (position, index) in line.iter().enumerate() {
                    let child = &children[*index];
                    let (child_main, child_cross) = if row {
                        (child.rect.width, child.rect.height)
                    } else {
                        (child.rect.height, child.rect.width)
                    };
                    let cross_offset = match align {
                        StackAlign::Start | StackAlign::Stretch => 0.0,
                        StackAlign::Center => (line_cross - child_cross) / 2.0,
                        StackAlign::End => line_cross - child_cross,
                    };
                    let (x, y) = if row {
                        (inner_x + main, inner_y + cross + cross_offset)
                    } else {
                        (inner_x + cross + cross_offset, inner_y + main)
                    };
                    paint_element(
                        project,
                        scene,
                        fonts,
                        child,
                        Rect {
                            x,
                            y,
                            width: child.rect.width,
                            height: child.rect.height,
                        },
                        out,
                    );
                    main += child_main + gap + between;
                    let _ = position;
                }
                cross += line_cross + gap;
                let _ = line_index;
            }
        }
        ElementLayout::Grid {
            columns,
            gap,
            padding,
        } => {
            let padding = padding.max(0.0);
            let gap = gap.max(0.0);
            let columns = (*columns).max(1) as usize;
            let inner_width = (at.width - padding * 2.0).max(0.0);
            let inner_height = (at.height - padding * 2.0).max(0.0);
            let cell_width = ((inner_width - gap * columns.saturating_sub(1) as f64)
                / columns as f64)
                .max(0.0);
            let rows = element.children.len().div_ceil(columns);
            let cell_height = if rows == 0 {
                0.0
            } else {
                ((inner_height - gap * rows.saturating_sub(1) as f64) / rows as f64).max(0.0)
            };
            let cell_width = if cell_width <= 0.0 { 1.0 } else { cell_width };
            for (index, child) in element.children.iter().enumerate() {
                let x = at.x + padding + (index % columns) as f64 * (cell_width + gap);
                let y = at.y + padding + (index / columns) as f64 * (cell_height + gap);
                paint_element(
                    project,
                    scene,
                    fonts,
                    child,
                    Rect {
                        x,
                        y,
                        width: child.rect.width,
                        height: child.rect.height,
                    },
                    out,
                );
            }
        }
    }
}

/// The text a visual element draws, if it draws any.
fn text_of(element: &Element) -> Option<&str> {
    match element.kind {
        ElementKind::Text
        | ElementKind::Button
        | ElementKind::Link
        | ElementKind::Input { .. }
        | ElementKind::Textarea
        | ElementKind::Select { .. }
        | ElementKind::Checkbox
        | ElementKind::Radio => Some(element.text.as_str()),
        _ => None,
    }
}

/// Where a text-bearing element's first line starts, matching how the canvas draws it.
fn text_box(element: &Element, at: Rect) -> (Rect, TextAlign) {
    let style = &element.style.text_style;
    let padding = match element.kind {
        ElementKind::Button => 12.0,
        ElementKind::Input { .. } | ElementKind::Select { .. } | ElementKind::Textarea => 8.0,
        ElementKind::Link | ElementKind::Checkbox | ElementKind::Radio => 1.0,
        _ => 0.0,
    };
    let line_height = (style.size * style.line_height).max(style.size);
    let top = match element.kind {
        ElementKind::Text | ElementKind::Textarea => at.y + padding * 0.0,
        _ => at.y + ((at.height - line_height) / 2.0).max(0.0),
    };
    (
        Rect {
            x: at.x + padding,
            y: top,
            width: (at.width - padding * 2.0).max(1.0),
            height: at.height,
        },
        style.align,
    )
}

fn paint_text(
    scene: &mut Scene,
    fonts: &mut FontSet,
    element: &Element,
    at: Rect,
    opacity: f64,
    out: &mut Vec<Item>,
) {
    let style = &element.style.text_style;
    let Some(text) = text_of(element) else {
        return;
    };
    let colour = Rgba::parse(&style.color)
        .unwrap_or(Rgba::BLACK)
        .faded(opacity);
    let (boxed, align) = text_box(element, at);
    let single_line = matches!(
        element.kind,
        ElementKind::Button
            | ElementKind::Link
            | ElementKind::Input { .. }
            | ElementKind::Select { .. }
            | ElementKind::Checkbox
            | ElementKind::Radio
    );
    let lines = wrap_text(fonts, text, style, boxed.width, !single_line);
    let line_height = (style.size * style.line_height).max(style.size);
    for (index, line) in lines.iter().enumerate() {
        let width = line_width(fonts, line, style);
        let offset = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (boxed.width - width) / 2.0,
            TextAlign::Right => boxed.width - width,
        };
        let baseline = boxed.y + style.size * 0.82 + index as f64 * line_height;
        let mut pen = boxed.x + offset;
        for character in line.chars() {
            let (advance, outline) = fonts.glyph(
                character,
                style.weight,
                style.italic,
                pen,
                baseline,
                style.size,
            );
            if let Some(path) = outline {
                let path_ref = scene.push_path(path);
                out.push(Item::Shape(Shape {
                    path_ref,
                    fill: Some(colour),
                    stroke: None,
                }));
            }
            pen += advance + style.letter_spacing;
        }
    }
}

fn line_width(fonts: &mut FontSet, line: &str, style: &crate::ElementTextStyle) -> f64 {
    line.chars()
        .map(|character| {
            fonts.advance(character, style.weight, style.size) + style.letter_spacing
        })
        .sum()
}

/// Greedy word wrapping with long words broken up, matching the canvas's `pre-wrap` text.
fn wrap_text(
    fonts: &mut FontSet,
    text: &str,
    style: &crate::ElementTextStyle,
    width: f64,
    wrap: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if !wrap || width <= 1.0 {
            lines.push(paragraph.to_owned());
            continue;
        }
        let mut line = String::new();
        let mut line_len = 0.0;
        for word in paragraph.split(' ') {
            let mut word = word.to_owned();
            let mut word_len = line_width(fonts, &word, style);
            let space = fonts.advance(' ', style.weight, style.size);
            if line_len > 0.0 && line_len + space + word_len > width {
                lines.push(std::mem::take(&mut line));
                line_len = 0.0;
            }
            while word_len > width && word.chars().count() > 1 {
                let mut fitting = String::new();
                let mut fitting_len = 0.0;
                let mut rest = String::new();
                let mut overflowed = false;
                for character in word.chars() {
                    if overflowed {
                        rest.push(character);
                        continue;
                    }
                    let advance = fonts.advance(character, style.weight, style.size);
                    if !fitting.is_empty() && fitting_len + advance > width {
                        overflowed = true;
                        rest.push(character);
                    } else {
                        fitting.push(character);
                        fitting_len += advance;
                    }
                }
                if rest.is_empty() {
                    word = fitting;
                    word_len = fitting_len;
                    break;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    line_len = 0.0;
                }
                lines.push(fitting);
                word = rest;
                word_len = line_width(fonts, &word, style);
            }
            if line.is_empty() {
                line = word;
                line_len = word_len;
            } else {
                line.push(' ');
                line.push_str(&word);
                line_len += space + word_len;
            }
        }
        lines.push(line);
    }
    lines
}

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

struct FaceData {
    bytes: Vec<u8>,
    index: u32,
}

struct FontSet {
    faces: Vec<FaceData>,
    database: Option<Database>,
    fallbacks: BTreeMap<char, usize>,
}

impl FontSet {
    fn new() -> Self {
        Self {
            faces: vec![FaceData {
                bytes: ARCHIVO.to_vec(),
                index: 0,
            }],
            database: None,
            fallbacks: BTreeMap::new(),
        }
    }

    /// The bundled face for characters it covers, otherwise a system face that has the glyph.
    fn face_for(&mut self, character: char) -> usize {
        if let Some(index) = self.fallbacks.get(&character) {
            return *index;
        }
        let covered = Face::parse(&self.faces[0].bytes, self.faces[0].index)
            .is_ok_and(|face| face.glyph_index(character).is_some());
        if covered {
            return 0;
        }
        if self.database.is_none() {
            let mut database = Database::new();
            database.load_system_fonts();
            self.database = Some(database);
        }
        let database = self.database.as_ref().expect("the database was just loaded");
        for family in [
            Family::SansSerif,
            Family::Serif,
            Family::Monospace,
            Family::Cursive,
            Family::Fantasy,
        ] {
            let query = Query {
                families: &[family],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
            };
            let Some(id) = database.query(&query) else {
                continue;
            };
            let mut found = None;
            database.with_face_data(id, |bytes, index| {
                if Face::parse(bytes, index).is_ok_and(|face| face.glyph_index(character).is_some())
                {
                    found = Some((bytes.to_vec(), index));
                }
            });
            if let Some((bytes, index)) = found {
                self.faces.push(FaceData { bytes, index });
                let face_index = self.faces.len() - 1;
                self.fallbacks.insert(character, face_index);
                return face_index;
            }
        }
        0
    }

    fn advance(&mut self, character: char, weight: u16, size: f64) -> f64 {
        let face_index = self.face_for(character);
        let data = &self.faces[face_index];
        let Ok(mut face) = Face::parse(&data.bytes, data.index) else {
            return size * 0.5;
        };
        let _ = face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), f32::from(weight));
        let units = f64::from(face.units_per_em().max(1));
        let advance = face
            .glyph_index(character)
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .map_or(units / 2.0, f64::from);
        advance / units * size
    }

    fn glyph(
        &mut self,
        character: char,
        weight: u16,
        italic: bool,
        pen: f64,
        baseline: f64,
        size: f64,
    ) -> (f64, Option<Path>) {
        let face_index = self.face_for(character);
        let data = &self.faces[face_index];
        let Ok(mut face) = Face::parse(&data.bytes, data.index) else {
            return (size * 0.5, None);
        };
        let _ = face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), f32::from(weight));
        let units = f64::from(face.units_per_em().max(1));
        let Some(glyph) = face.glyph_index(character) else {
            return (size * 0.5, None);
        };
        let advance = face
            .glyph_hor_advance(glyph)
            .map_or(units / 2.0, f64::from)
            / units
            * size;
        let mut builder = PathBuilder {
            path: Path::default(),
            pen,
            baseline,
            scale: size / units,
            italic,
        };
        let outline = face.outline_glyph(glyph, &mut builder).is_some();
        (advance, if outline { Some(builder.path) } else { None })
    }
}

struct PathBuilder {
    path: Path,
    pen: f64,
    baseline: f64,
    scale: f64,
    italic: bool,
}

impl PathBuilder {
    fn map(&self, x: f32, y: f32) -> Point {
        let x = f64::from(x) * self.scale;
        let y = f64::from(y) * self.scale;
        let screen_y = self.baseline - y;
        let shear = if self.italic { 0.2 * y } else { 0.0 };
        Point {
            x: self.pen + x + shear,
            y: screen_y,
        }
    }
}

impl OutlineBuilder for PathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let point = self.map(x, y);
        self.path.move_to(point);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let point = self.map(x, y);
        self.path.line_to(point);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let control = self.map(x1, y1);
        let point = self.map(x, y);
        self.path.quad_to(control, point);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let first = self.map(x1, y1);
        let second = self.map(x2, y2);
        let point = self.map(x, y);
        self.path.cubic_to(first, second, point);
    }

    fn close(&mut self) {
        self.path.close();
    }
}

// ---------------------------------------------------------------------------
// SVG
// ---------------------------------------------------------------------------

fn scene_svg(scene: &Scene) -> String {
    let mut clips = Vec::new();
    collect_clips(&scene.items, &mut clips);
    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\" viewBox=\"0 0 {:.0} {:.0}\">",
        scene.width, scene.height, scene.width, scene.height
    ));
    if !clips.is_empty() {
        svg.push_str("<defs>");
        for (index, rect) in clips.iter().enumerate() {
            svg.push_str(&format!(
                "<clipPath id=\"clip{index}\" clipPathUnits=\"userSpaceOnUse\"><rect x=\"{:.3}\" y=\"{:.3}\" width=\"{:.3}\" height=\"{:.3}\"/></clipPath>",
                rect.x, rect.y, rect.width, rect.height
            ));
        }
        svg.push_str("</defs>");
    }
    svg.push_str(&format!(
        "<rect width=\"100%\" height=\"100%\" fill=\"{}\"/>",
        scene.background.svg_colour()
    ));
    let mut next = 0_usize;
    write_items(&mut svg, scene, &scene.items, &mut next);
    svg.push_str("</svg>");
    svg
}

fn collect_clips(items: &[Item], clips: &mut Vec<Rect>) {
    for item in items {
        if let Item::Group {
            clip: Some(rect),
            items,
            ..
        } = item
        {
            clips.push(*rect);
            collect_clips(items, clips);
        } else if let Item::Group { items, .. } = item {
            collect_clips(items, clips);
        }
    }
}

fn write_items(svg: &mut String, scene: &Scene, items: &[Item], next: &mut usize) {
    for item in items {
        match item {
            Item::Shape(shape) => {
                let Some(path) = scene.paths.get(shape.path_ref) else {
                    continue;
                };
                let data = path.svg_data();
                if data.is_empty() {
                    continue;
                }
                let mut attributes = String::new();
                match shape.fill {
                    Some(fill) => attributes.push_str(&format!(" fill=\"{}\"", fill.svg_colour())),
                    None => attributes.push_str(" fill=\"none\""),
                }
                if let Some((stroke, width)) = shape.stroke {
                    attributes.push_str(&format!(
                        " stroke=\"{}\" stroke-width=\"{:.3}\"",
                        stroke.svg_colour(),
                        width
                    ));
                }
                svg.push_str(&format!("<path d=\"{data}\"{attributes}/>"));
            }
            Item::Picture(picture) => {
                use base64::{Engine, engine::general_purpose::STANDARD};
                svg.push_str(&format!(
                    "<image x=\"{:.3}\" y=\"{:.3}\" width=\"{:.3}\" height=\"{:.3}\" preserveAspectRatio=\"none\" href=\"data:{};base64,{}\"/>",
                    picture.x,
                    picture.y,
                    picture.width,
                    picture.height,
                    picture.mime,
                    STANDARD.encode(&picture.bytes)
                ));
            }
            Item::Group {
                clip,
                alpha,
                items,
            } => {
                let mut attributes = String::new();
                if *alpha < 0.999 {
                    attributes.push_str(&format!(" opacity=\"{alpha:.3}\""));
                }
                if clip.is_some() {
                    attributes.push_str(&format!(" clip-path=\"url(#clip{})\"", *next));
                    *next += 1;
                }
                svg.push_str(&format!("<g{attributes}>"));
                write_items(svg, scene, items, next);
                svg.push_str("</g>");
            }
        }
    }
}
