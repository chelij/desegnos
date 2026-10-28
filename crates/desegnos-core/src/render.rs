//! Direct rendering from the resolved element tree: SVG, PNG and a slide PDF deck.
//!
//! Websites and app screens are built by the AI export job, but slides and images are written
//! here without any network or browser. One scene of paths, pictures, text outlines and shadows
//! is built once and written three ways: a standalone SVG, a PNG rasterised from that SVG, and
//! vector paths in a PDF. Text becomes glyph outlines from the bundled Archivo, or from a system
//! face through fontdb, so exported files never depend on a font being installed.
//!
//! # The rendering contract
//!
//! `src/projectCanvas/elements.tsx` draws the same element tree on the canvas, so both sides
//! follow one contract. A mismatch is a bug on whichever side disagrees, and neither side may
//! invent a look the other does not show. The canvas is the reference for interaction; this file
//! is the reference for the numbers.
//!
//! **Box.** An element's rect is its border box in page pixels, measured from the top-left of the
//! boundary. A fill covers that box and a stroke is drawn inside it, the way a CSS border is: the
//! outline runs on the centre line of the border and the outer half is clipped away. A group
//! draws nothing of its own; its rect is only where its children sit.
//!
//! **Layout.** Children are laid out by the parent's layout, in document order, inside the
//! parent's content box (the border box inset by the drawn border width):
//!
//! - free: a child sits at its own rect offset from the content box.
//! - stack: children keep their own width and height and are packed along the row or column from
//!   the content box inset by `padding`, `gap` apart, wrapped onto a new line only when `wrap` is
//!   set, then justified along the line and aligned across it. `stretch` behaves like `start`
//!   because children always carry an explicit size, as they do in the canvas.
//! - grid: children fill `columns` equal columns of the content box minus `padding`; each row is
//!   as tall as its tallest child, never stretched to the container height.
//!
//! Children that do not fit overflow the parent. An element with no explicit width or height and
//! text of its own takes the size that text needs, matching the canvas's `max-content` width and
//! automatic height.
//!
//! **Typography.** Text is laid out like a run of CSS inline boxes: the family's ascent and
//! descent fill a line box of `line-height`, with the leftover split evenly above and below, so
//! the first baseline sits at `(lineHeight - (ascent + descent)) / 2 + ascent` below the top of
//! the box. `font-family` is read as a CSS list: each name in turn, then a generic keyword, and
//! `fontdb` resolves both; `sans-serif`, `system-ui` and `ui-sans-serif` mean the system sans
//! face, `serif` and `ui-serif` the system serif, `monospace` and `ui-monospace` the system
//! monospace, `cursive` and `fantasy` their system faces. Archivo is bundled, so the name
//! `Archivo` always resolves to the file that ships with the app, and an unresolvable family
//! falls back to Archivo instead of to an arbitrary installed font. Size, weight, italic, line
//! height, letter spacing and alignment are applied as the canvas applies them: weight selects
//! the closest face and sets the `wght` variation axis; italic selects an italic face or shears
//! an upright fallback,
//! letter spacing follows every character including the last, and alignment positions each line
//! inside the text box. Text wraps at the content box width, except for the single-line controls
//! the canvas gives `white-space: nowrap` (button, link, select, checkbox, radio). Buttons and
//! checkbox/radio labels are centred horizontally, links, inputs and text areas follow the
//! element's alignment, and a select's label is always at the start, as flexbox lays them out.
//!
//! **Transforms.** `rotation` in degrees turns the element clockwise about the centre of its
//! rect, and everything the element draws turns with it, children included; nested rotations
//! compose. The origin is the rect's centre, which is also where CSS puts `transform-origin`.
//!
//! **Clipping.** Every element clips its own content to its rounded box, except boxes and groups,
//! which let children overflow: that is the canvas's `overflow` rule. The boundary itself clips
//! to its size. A picture fills its box like `object-fit: cover`, so it is centred and cropped,
//! never stretched.
//!
//! **Effects.** `opacity` multiplies down the tree: an element's own paint, its shadow and its
//! children are composited together and then faded as one, as CSS does. A shadow is the
//! element's silhouette, offset by `x` and `y`, filled with its colour, softened by its blur and
//! shaped like the element (a rounded box, or an ellipse). Blur is a CSS blur radius: the
//! silhouettes are Gaussian blurred with a standard deviation of `blur / 2` in PNG and SVG
//! `feGaussianBlur`. PDF has no blur primitive, so the PDF writer stands in for one with six
//! nested silhouettes growing outwards, each at a fixed alpha, whose overlap adds back up to the
//! shadow's colour: a soft ramp rather than a true Gaussian.
mod pdf;

use crate::{
    Element, ElementKind, ElementLayout, ElementStyle, ElementTextStyle, Error, Page, Project,
    Rect, Result, StackAlign, StackJustify, TextAlign,
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
            return Some(Self {
                a: 0.0,
                ..Self::BLACK
            });
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
        let radius = clamp_radius(rect, radius);
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
                Point { x: x + width, y },
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
                Point {
                    x: cx + rx,
                    y: cy + oy,
                },
                Point {
                    x: cx + ox,
                    y: cy + ry,
                },
                Point { x: cx, y: cy + ry },
            )
            .cubic_to(
                Point {
                    x: cx - ox,
                    y: cy + ry,
                },
                Point {
                    x: cx - rx,
                    y: cy + oy,
                },
                Point { x: cx - rx, y: cy },
            )
            .cubic_to(
                Point {
                    x: cx - rx,
                    y: cy - oy,
                },
                Point {
                    x: cx - ox,
                    y: cy - ry,
                },
                Point { x: cx, y: cy - ry },
            )
            .cubic_to(
                Point {
                    x: cx + ox,
                    y: cy - ry,
                },
                Point {
                    x: cx + rx,
                    y: cy - oy,
                },
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

/// A rounded rectangle that content is cut to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clip {
    pub rect: Rect,
    pub radius: f64,
}

/// A rotation in canvas degrees about a point, which is the centre of a layer's rect.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rotate {
    pub degrees: f64,
    pub cx: f64,
    pub cy: f64,
}

/// The silhouette a shadow is cast by.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ShadowShape {
    Rect { rect: Rect, radius: f64 },
    Ellipse(Rect),
}

impl ShadowShape {
    fn bounds(self) -> Rect {
        match self {
            Self::Rect { rect, .. } | Self::Ellipse(rect) => rect,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Shadow {
    pub shape: ShadowShape,
    pub fill: Rgba,
    /// A CSS blur radius: the silhouette is blurred with a standard deviation of `blur / 2`.
    pub blur: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub clip: Option<Clip>,
    pub rotate: Option<Rotate>,
    pub alpha: f64,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub(crate) enum Item {
    Shape(Shape),
    Picture(Picture),
    Shadow(Shadow),
    Group(Group),
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
    if stem.is_empty() {
        "boundary".into()
    } else {
        stem
    }
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
    let background = Rgba::parse(&page.page.background).expect("validated boundary background");
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
        let rect = sized_in(&mut fonts, element, Rect { x: 0.0, y: 0.0, width: page.width, height: page.height });
        paint_element(
            project,
            &mut scene,
            &mut fonts,
            element,
            rect,
            &mut items,
        );
    }
    scene.items.push(Item::Group(Group {
        clip: Some(Clip {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: page.width,
                height: page.height,
            },
            radius: 0.0,
        }),
        rotate: None,
        alpha: 1.0,
        items,
    }));
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
    if !element.visible {
        return;
    }
    let style = &element.style;
    let opacity = style.opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let at = resolve_auto_size(fonts, element, at);
    // A line carries no height; every other element needs a real box to draw in.
    if at.width <= 0.0 || (at.height <= 0.0 && element.kind != ElementKind::Line) {
        return;
    }
    let radius = layer_radius(element);
    let stroke = stroke_of(style);
    // The canvas lays children out in the content box, inside the element's border.
    let content = inset(at, stroke.map_or(0.0, |(_, width)| width));
    let mut items: Vec<Item> = Vec::new();
    if let Some(shadow) = shadow_of(style, &element.kind, at, radius) {
        items.push(shadow);
    }
    let mut own: Vec<Item> = Vec::new();
    match element.kind {
        ElementKind::Group => {}
        ElementKind::Line => paint_line(scene, style, at, &mut own),
        ElementKind::Ellipse => paint_ellipse(scene, style, at, &mut own),
        ElementKind::Image => paint_picture(project, element, at, &mut own),
        _ => paint_surface(scene, style, at, radius, &mut own),
    }
    if !own.is_empty() {
        // A border is drawn inside the border box, so the outer half of a stroke is cut away;
        // a picture is cut to the rounded box as `object-fit: cover` plus a radius does.
        let clip = (stroke.is_some() || element.kind == ElementKind::Image)
            .then_some(Clip { rect: at, radius });
        items.push(Item::Group(Group {
            clip,
            rotate: None,
            alpha: 1.0,
            items: own,
        }));
    }
    let mut content_items: Vec<Item> = Vec::new();
    paint_text(scene, fonts, element, content, &mut content_items);
    paint_children(project, scene, fonts, element, content, &mut content_items);
    if !content_items.is_empty() {
        // Boxes and groups let children overflow; every other layer clips its content.
        let clip = clips_content(&element.kind).then_some(Clip { rect: at, radius });
        items.push(Item::Group(Group {
            clip,
            rotate: None,
            alpha: 1.0,
            items: content_items,
        }));
    }
    if items.is_empty() {
        return;
    }
    let degrees = element.rotation % 360.0;
    let rotate = (degrees.is_finite() && degrees.abs() > 0.000_001).then_some(Rotate {
        degrees,
        cx: at.x + at.width / 2.0,
        cy: at.y + at.height / 2.0,
    });
    out.push(Item::Group(Group {
        clip: None,
        rotate,
        alpha: opacity,
        items,
    }));
}

/// The corner radius this layer is painted and clipped with.
fn layer_radius(element: &Element) -> f64 {
    if element.kind == ElementKind::Ellipse {
        0.0
    } else {
        element.style.radius.max(0.0)
    }
}

/// Whether an element's own content is cut to its box, matching the canvas's `overflow` rule.
fn clips_content(kind: &ElementKind) -> bool {
    !matches!(kind, ElementKind::Box | ElementKind::Group)
}

fn shadow_of(style: &ElementStyle, kind: &ElementKind, at: Rect, radius: f64) -> Option<Item> {
    if matches!(kind, ElementKind::Line | ElementKind::Group) {
        return None;
    }
    let colour = style.shadow.color.as_deref().and_then(Rgba::parse)?;
    if colour.a <= 0.0 {
        return None;
    }
    let rect = Rect {
        x: at.x + style.shadow.x,
        y: at.y + style.shadow.y,
        width: at.width,
        height: at.height,
    };
    let shape = if matches!(kind, ElementKind::Ellipse) {
        ShadowShape::Ellipse(rect)
    } else {
        ShadowShape::Rect { rect, radius }
    };
    Some(Item::Shadow(Shadow {
        shape,
        fill: colour,
        blur: style.shadow.blur.max(0.0),
    }))
}

fn paint_line(scene: &mut Scene, style: &ElementStyle, at: Rect, out: &mut Vec<Item>) {
    let middle = at.y + at.height.max(1.0) / 2.0;
    let mut path = Path::default();
    path.move_to(Point { x: at.x, y: middle }).line_to(Point {
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
        .unwrap_or(Rgba::BLACK);
    out.push(Item::Shape(Shape {
        path_ref,
        fill: None,
        stroke: Some((colour, style.stroke.width.max(1.0))),
    }));
}

fn paint_picture(project: &Project, element: &Element, at: Rect, out: &mut Vec<Item>) {
    let Some((mime, bytes)) = element.image.and_then(|id| project.picture_bytes(id)) else {
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

fn paint_surface(
    scene: &mut Scene,
    style: &ElementStyle,
    at: Rect,
    radius: f64,
    out: &mut Vec<Item>,
) {
    let fill = style.fill.as_deref().and_then(Rgba::parse);
    let stroke = stroke_of(style);
    if fill.is_some_and(|colour| colour.a > 0.0) || stroke.is_some() {
        let path_ref = scene.push_path(Path::rect(at, radius));
        out.push(Item::Shape(Shape {
            path_ref,
            fill,
            stroke,
        }));
    }
}

fn paint_ellipse(scene: &mut Scene, style: &ElementStyle, at: Rect, out: &mut Vec<Item>) {
    let fill = style.fill.as_deref().and_then(Rgba::parse);
    let stroke = stroke_of(style);
    if fill.is_some_and(|colour| colour.a > 0.0) || stroke.is_some() {
        let path_ref = scene.push_path(Path::ellipse(at));
        out.push(Item::Shape(Shape {
            path_ref,
            fill,
            stroke,
        }));
    }
}

fn sized_in(fonts: &mut FontSet, element: &Element, parent: Rect) -> Rect {
    let mut rect = element.rect;
    if element.style.sizing.width == crate::SizingMode::Fill {
        rect.width = (parent.width - rect.x).max(0.0);
    }
    if element.style.sizing.height == crate::SizingMode::Fill {
        rect.height = (parent.height - rect.y).max(0.0);
    }
    resolve_auto_size(fonts, element, rect)
}

fn paint_children(
    project: &Project,
    scene: &mut Scene,
    fonts: &mut FontSet,
    element: &Element,
    content: Rect,
    out: &mut Vec<Item>,
) {
    let children: Vec<_> = element
        .children
        .iter()
        .filter(|child| child.visible)
        .collect();
    if children.is_empty() {
        return;
    }
    match &element.layout {
        ElementLayout::Free => {
            for child in children {
                let rect = sized_in(fonts, child, content);
                paint_element(project, scene, fonts, child, offset(content, rect), out);
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
            let inner = inset(content, *padding);
            let extent = if row { inner.width } else { inner.height };
            let cross_extent = if row { inner.height } else { inner.width };
            let mut rects: Vec<_> = children
                .iter()
                .map(|child| resolve_auto_size(fonts, child, child.rect))
                .collect();
            let fill = |index: usize| if row { children[index].style.sizing.width } else { children[index].style.sizing.height } == crate::SizingMode::Fill;
            let main = |rect: Rect| if row { rect.width } else { rect.height };
            let mut lines = vec![vec![]];
            let mut used = 0.0;
            for (i, rect) in rects.iter().enumerate() {
                let size = if fill(i) { 0.0 } else { main(*rect) };
                let line = lines.last_mut().expect("one line");
                if *wrap && !line.is_empty() && used + gap + size > extent {
                    lines.push(vec![i]);
                    used = size;
                } else {
                    used += if line.is_empty() { size } else { gap + size };
                    line.push(i);
                }
            }
            let mut cross = 0.0;
            for line in lines {
                let gaps = gap * line.len().saturating_sub(1) as f64;
                let fixed: f64 = line
                    .iter()
                    .filter(|i| !fill(**i))
                    .map(|i| main(rects[*i]))
                    .sum();
                let mut remaining = (extent - gaps - fixed).max(0.0);
                let mut active: Vec<_> = line.iter().copied().filter(|i| fill(*i)).collect();
                // Freeze constrained children and redistribute the remaining free space.
                while !active.is_empty() {
                    let share = remaining / active.len() as f64;
                    let mut frozen = Vec::new();
                    for &i in &active {
                        let sizing = &children[i].style.sizing;
                        let (w, h) = sizing.constrain(
                            if row { share } else { rects[i].width },
                            if row { rects[i].height } else { share },
                        );
                        let value = if row { w } else { h };
                        if row {
                            rects[i].width = value;
                        } else {
                            rects[i].height = value;
                        }
                        if (value - share).abs() > 0.001 {
                            frozen.push(i);
                            remaining = (remaining - value).max(0.0);
                        }
                    }
                    if frozen.is_empty() {
                        break;
                    }
                    active.retain(|i| !frozen.contains(i));
                }
                let total: f64 = line.iter().map(|i| main(rects[*i])).sum::<f64>() + gaps;
                let extra = (extent - total).max(0.0);
                let mut cursor = match justify {
                    StackJustify::Center => extra / 2.0,
                    StackJustify::End => extra,
                    _ => 0.0,
                };
                let between = if *justify == StackJustify::SpaceBetween && line.len() > 1 {
                    extra / (line.len() - 1) as f64
                } else {
                    0.0
                };
                let line_cross = if !wrap {
                    cross_extent
                } else {
                    line.iter()
                        .map(|i| {
                            if row {
                                rects[*i].height
                            } else {
                                rects[*i].width
                            }
                        })
                        .fold(0.0_f64, f64::max)
                };
                for i in line {
                    let child = children[i];
                    let mut rect = rects[i];
                    let cross_fill = if row {
                        child.style.sizing.height
                    } else {
                        child.style.sizing.width
                    } == crate::SizingMode::Fill;
                    if cross_fill {
                        if row {
                            rect.height = line_cross;
                        } else {
                            rect.width = line_cross;
                        }
                    }
                    (rect.width, rect.height) =
                        child.style.sizing.constrain(rect.width, rect.height);
                    let cross_size = if row { rect.height } else { rect.width };
                    let cross_offset = match align {
                        StackAlign::Center => (line_cross - cross_size) / 2.0,
                        StackAlign::End => line_cross - cross_size,
                        _ => 0.0,
                    };
                    rect.x = inner.x + if row { cursor } else { cross + cross_offset };
                    rect.y = inner.y + if row { cross + cross_offset } else { cursor };
                    paint_element(project, scene, fonts, child, rect, out);
                    cursor += main(rect) + gap + between;
                }
                cross += line_cross + gap;
            }
        }
        ElementLayout::Grid {
            columns,
            gap,
            padding,
        } => {
            let inner = inset(content, *padding);
            let columns = *columns as usize;
            let width =
                ((inner.width - gap * columns.saturating_sub(1) as f64) / columns as f64).max(0.0);
            let natural: Vec<f64> = children
                .chunks(columns)
                .map(|row| {
                    row.iter()
                        .filter(|child| child.style.sizing.height != crate::SizingMode::Fill)
                        .map(|child| resolve_auto_size(fonts, child, child.rect).height)
                        .fold(0.0_f64, f64::max)
                })
                .collect();
            let extra = ((inner.height
                - natural.iter().sum::<f64>()
                - gap * natural.len().saturating_sub(1) as f64)
                / natural.len() as f64)
                .max(0.0);
            let mut y = inner.y;
            for (row_index, row) in children.chunks(columns).enumerate() {
                let rects: Vec<_> = row
                    .iter()
                    .map(|child| resolve_auto_size(fonts, child, child.rect))
                    .collect();
                let height = natural[row_index] + extra;
                for (column, (child, mut rect)) in row.iter().zip(rects).enumerate() {
                    if child.style.sizing.width == crate::SizingMode::Fill {
                        rect.width = width;
                    }
                    if child.style.sizing.height == crate::SizingMode::Fill {
                        rect.height = height;
                    }
                    (rect.width, rect.height) =
                        child.style.sizing.constrain(rect.width, rect.height);
                    rect.x = inner.x + column as f64 * (width + gap);
                    rect.y = y;
                    paint_element(project, scene, fonts, child, rect, out);
                }
                y += height + gap;
            }
        }
    }
}

/// A rect placed at its own offset inside the parent's content box.
fn offset(content: Rect, rect: Rect) -> Rect {
    Rect {
        x: content.x + rect.x,
        y: content.y + rect.y,
        width: rect.width,
        height: rect.height,
    }
}

/// A rect shrunk by the same amount on every side.
fn inset(rect: Rect, amount: f64) -> Rect {
    let amount = amount.max(0.0);
    Rect {
        x: rect.x + amount,
        y: rect.y + amount,
        width: (rect.width - amount * 2.0).max(0.0),
        height: (rect.height - amount * 2.0).max(0.0),
    }
}

/// A rect grown by the same amount on every side.
fn inflate(rect: Rect, amount: f64) -> Rect {
    let amount = amount.max(0.0);
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: rect.width + amount * 2.0,
        height: rect.height + amount * 2.0,
    }
}

fn clamp_radius(rect: Rect, radius: f64) -> f64 {
    radius
        .max(0.0)
        .min(rect.width.max(0.0) / 2.0)
        .min(rect.height.max(0.0) / 2.0)
        .min(40_000.0)
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

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

/// Whether a text line may wrap; the canvas gives single-line controls `white-space: nowrap`.
fn wraps(kind: &ElementKind) -> bool {
    !matches!(
        kind,
        ElementKind::Button
            | ElementKind::Link
            | ElementKind::Select { .. }
            | ElementKind::Checkbox
            | ElementKind::Radio
    )
}

/// The height of one line box.
fn line_height(style: &ElementTextStyle) -> f64 {
    (style.size * style.line_height).max(style.size)
}

/// An element with no explicit size takes the size its own text needs, like the canvas's
/// `max-content` width and automatic height.
fn resolve_auto_size(fonts: &mut FontSet, element: &Element, mut at: Rect) -> Rect {
    let sizing = &element.style.sizing;
    let fit_width = sizing.width == crate::SizingMode::Fit;
    let fit_height = sizing.height == crate::SizingMode::Fit;
    if let Some(text) = text_of(element) {
        let style = &element.style.text_style;
        if at.width <= 0.0 || fit_width {
            at.width = text
                .split('\n')
                .map(|line| line_width(fonts, line, style))
                .fold(0.0, f64::max)
                .max(1.0);
        }
        at.width = sizing.constrain(at.width, at.height).0;
        if at.height <= 0.0 || fit_height {
            at.height = (wrap_text(fonts, text, style, at.width, wraps(&element.kind))
                .len()
                .max(1) as f64
                * line_height(style))
            .max(1.0);
        }
    } else if fit_width || fit_height {
        let children: Vec<_> = element
            .children
            .iter()
            .filter(|c| c.visible)
            .map(|c| resolve_auto_size(fonts, c, c.rect))
            .collect();
        let (width, height) = match &element.layout {
            ElementLayout::Free => (
                children.iter().map(|r| r.x + r.width).fold(0.0, f64::max),
                children.iter().map(|r| r.y + r.height).fold(0.0, f64::max),
            ),
            ElementLayout::Stack {
                direction,
                gap,
                padding,
                ..
            } => {
                let gaps = gap * children.len().saturating_sub(1) as f64;
                if *direction == crate::StackDirection::Row {
                    (
                        children.iter().map(|r| r.width).sum::<f64>() + gaps + padding * 2.0,
                        children.iter().map(|r| r.height).fold(0.0, f64::max) + padding * 2.0,
                    )
                } else {
                    (
                        children.iter().map(|r| r.width).fold(0.0, f64::max) + padding * 2.0,
                        children.iter().map(|r| r.height).sum::<f64>() + gaps + padding * 2.0,
                    )
                }
            }
            ElementLayout::Grid {
                columns,
                gap,
                padding,
            } => {
                let columns = *columns as usize;
                (
                    children.iter().map(|r| r.width).fold(0.0, f64::max) * columns as f64
                        + gap * columns.saturating_sub(1) as f64
                        + padding * 2.0,
                    children
                        .chunks(columns)
                        .map(|row| row.iter().map(|r| r.height).fold(0.0, f64::max))
                        .sum::<f64>()
                        + gap * children.len().div_ceil(columns).saturating_sub(1) as f64
                        + padding * 2.0,
                )
            }
        };
        let border = element
            .style
            .stroke
            .color
            .as_ref()
            .map_or(0.0, |_| element.style.stroke.width * 2.0);
        if fit_width {
            at.width = width + border;
        }
        if fit_height {
            at.height = height + border;
        }
    }
    (at.width, at.height) = sizing.constrain(at.width, at.height);
    at
}

/// Where a text-bearing element's first line box starts, and how its lines align.
fn text_box(element: &Element, content: Rect) -> (Rect, TextAlign) {
    let style = &element.style.text_style;
    let (padding, align) = match element.kind {
        ElementKind::Text => (0.0, style.align),
        ElementKind::Textarea => (8.0, style.align),
        ElementKind::Button | ElementKind::Checkbox | ElementKind::Radio => {
            (1.0, TextAlign::Center)
        }
        ElementKind::Link => (1.0, style.align),
        ElementKind::Input { .. } => (8.0, style.align),
        // A select lays its label out at the start and its arrow at the end.
        ElementKind::Select { .. } => (8.0, TextAlign::Left),
        _ => (0.0, style.align),
    };
    let height = line_height(style);
    let top = match element.kind {
        ElementKind::Text => content.y,
        ElementKind::Textarea => content.y + 8.0,
        _ => content.y + ((content.height - height) / 2.0).max(0.0),
    };
    (
        Rect {
            x: content.x + padding,
            y: top,
            width: (content.width - padding * 2.0).max(1.0),
            height: content.height,
        },
        align,
    )
}

fn paint_text(
    scene: &mut Scene,
    fonts: &mut FontSet,
    element: &Element,
    content: Rect,
    out: &mut Vec<Item>,
) {
    let Some(text) = text_of(element) else {
        return;
    };
    if text.trim().is_empty() {
        return;
    }
    let style = &element.style.text_style;
    let colour = Rgba::parse(&style.color).unwrap_or(Rgba::BLACK);
    let run = Run::of(style);
    let (boxed, align) = text_box(element, content);
    let lines = wrap_text(fonts, text, style, boxed.width, wraps(&element.kind));
    let height = line_height(style);
    let (ascent, font_height) = fonts.line_metrics(run);
    // CSS centres the font's ascent and descent in the line box and splits the rest of the
    // line height evenly above and below it.
    let baseline_offset = (height - font_height) / 2.0 + ascent;
    for (index, line) in lines.iter().enumerate() {
        let width = line_width(fonts, line, style);
        let offset = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (boxed.width - width) / 2.0,
            TextAlign::Right => boxed.width - width,
        };
        let baseline = boxed.y + baseline_offset + index as f64 * height;
        let mut pen = boxed.x + offset;
        for character in line.chars() {
            let (advance, outline) = fonts.glyph(run, character, pen, baseline);
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

fn line_width(fonts: &mut FontSet, line: &str, style: &ElementTextStyle) -> f64 {
    let run = Run::of(style);
    line.chars()
        .map(|character| fonts.advance(run, character) + style.letter_spacing)
        .sum()
}

/// Greedy word wrapping with long words broken up, matching the canvas's `pre-wrap` text.
fn wrap_text(
    fonts: &mut FontSet,
    text: &str,
    style: &ElementTextStyle,
    width: f64,
    wrap: bool,
) -> Vec<String> {
    let run = Run::of(style);
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
            let space = fonts.advance(run, ' ');
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
                    let advance = fonts.advance(run, character);
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

/// The exact primary face used by exports and the canvas. The ID identifies file contents
/// and collection index, so aliases and styles selecting one face share a browser load.
#[derive(Clone)]
pub struct ResolvedFont {
    pub id: String,
    pub bytes: std::sync::Arc<Vec<u8>>,
    pub index: u32,
    pub bundled: bool,
    pub fell_back: bool,
    pub italic: bool,
}

/// Resolve a CSS family list with the same database and fallback policy as the renderer.
pub fn resolve_font(family: &str, weight: u16, italic: bool) -> ResolvedFont {
    use std::sync::{Mutex, OnceLock};
    type Requests = BTreeMap<(String, u16, bool), ResolvedFont>;
    static REQUESTS: OnceLock<Mutex<Requests>> = OnceLock::new();
    let mut requests = REQUESTS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    requests
        .entry((family.to_owned(), weight, italic))
        .or_insert_with(|| resolve_font_uncached(family, weight, italic))
        .clone()
}

fn resolve_font_uncached(family: &str, weight: u16, italic: bool) -> ResolvedFont {
    use sha2::{Digest, Sha256};
    use std::sync::{Mutex, OnceLock};
    static DATABASE: OnceLock<Mutex<(Database, BTreeMap<fontdb::ID, ResolvedFont>)>> =
        OnceLock::new();
    let mut database = DATABASE
        .get_or_init(|| {
            let mut database = Database::new();
            database.load_system_fonts();
            Mutex::new((database, BTreeMap::new()))
        })
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for requested in family.split(',') {
        let name = requested.trim().trim_matches(['"', '\'']).trim();
        if name.eq_ignore_ascii_case("Archivo") {
            return bundled_font(false);
        }
        if name.is_empty() {
            continue;
        }
        let query = Query {
            families: &[generic_family(name)],
            weight: Weight(weight),
            stretch: Stretch::Normal,
            style: if italic { Style::Italic } else { Style::Normal },
        };
        let Some(id) = database.0.query(&query) else {
            continue;
        };
        if let Some(face) = database.1.get(&id) {
            return face.clone();
        }
        if let Some(face) = database.0.with_face_data(id, |bytes, index| {
            let mut hash = Sha256::new();
            hash.update(bytes);
            hash.update(index.to_be_bytes());
            ResolvedFont {
                id: format!("{:x}", hash.finalize()),
                bytes: std::sync::Arc::new(bytes.to_vec()),
                index,
                bundled: false,
                fell_back: false,
                italic: Face::parse(bytes, index).is_ok_and(|face| face.is_italic()),
            }
        }) {
            database.1.insert(id, face.clone());
            return face;
        }
    }
    bundled_font(true)
}

fn bundled_font(fell_back: bool) -> ResolvedFont {
    ResolvedFont {
        id: "archivo".into(),
        bytes: std::sync::Arc::new(ARCHIVO.to_vec()),
        index: 0,
        bundled: true,
        fell_back,
        italic: false,
    }
}

struct FaceData {
    bytes: Vec<u8>,
    index: u32,
}

/// One text style's font choice: everything about a run of glyphs that does not change per
/// character.
#[derive(Debug, Clone, Copy)]
struct Run<'a> {
    family: &'a str,
    weight: u16,
    italic: bool,
    size: f64,
}

impl<'a> Run<'a> {
    fn of(style: &'a ElementTextStyle) -> Self {
        Self {
            family: &style.font_family,
            weight: style.weight,
            italic: style.italic,
            size: style.size,
        }
    }
}

/// A CSS generic family keyword as the face fontdb resolves it to.
fn generic_family(name: &str) -> Family<'_> {
    match name.to_ascii_lowercase().as_str() {
        "sans-serif" | "system-ui" | "ui-sans-serif" | "ui-rounded" | "-apple-system"
        | "blinkmacsystemfont" => Family::SansSerif,
        "serif" | "ui-serif" => Family::Serif,
        "monospace" | "ui-monospace" => Family::Monospace,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        _ => Family::Name(name),
    }
}

struct FontSet {
    faces: Vec<FaceData>,
    database: Option<Database>,
    /// The face each `font-family` value resolves to, in request order.
    families: BTreeMap<(String, u16, bool), usize>,
    resolved: BTreeMap<String, usize>,
    /// The face that covers a character the primary face does not.
    fallbacks: BTreeMap<(usize, char), usize>,
}

impl FontSet {
    fn new() -> Self {
        Self {
            faces: vec![FaceData {
                bytes: ARCHIVO.to_vec(),
                index: 0,
            }],
            database: None,
            families: BTreeMap::new(),
            resolved: BTreeMap::from([("archivo".into(), 0)]),
            fallbacks: BTreeMap::new(),
        }
    }

    fn database(&mut self) -> &Database {
        self.database.get_or_insert_with(|| {
            let mut database = Database::new();
            database.load_system_fonts();
            database
        })
    }

    /// The face a `font-family` value asks for: each comma-separated family in turn, generic
    /// keywords through fontdb, and the bundled Archivo when nothing else is available.
    fn primary(&mut self, family: &str, weight: u16, italic: bool) -> usize {
        let key = (family.trim().to_ascii_lowercase(), weight, italic);
        if let Some(index) = self.families.get(&key) {
            return *index;
        }
        let resolved = resolve_font(family, weight, italic);
        let index = if let Some(index) = self.resolved.get(&resolved.id) {
            *index
        } else {
            self.faces.push(FaceData {
                bytes: resolved.bytes.as_ref().clone(),
                index: resolved.index,
            });
            let index = self.faces.len() - 1;
            self.resolved.insert(resolved.id, index);
            index
        };
        self.families.insert(key, index);
        index
    }

    /// The face that draws one character: the family's own face, else a system face with the
    /// glyph, else the family's face again so the character keeps its place in the line.
    fn face_for(&mut self, character: char, primary: usize) -> usize {
        if let Some(index) = self.fallbacks.get(&(primary, character)) {
            return *index;
        }
        if covers(&self.faces[primary], character) {
            return primary;
        }
        let database = self.database();
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
            let _ = database.with_face_data(id, |bytes, index| {
                let candidate = FaceData {
                    bytes: bytes.to_vec(),
                    index,
                };
                if covers(&candidate, character) {
                    found = Some(candidate);
                }
            });
            if let Some(face) = found {
                self.faces.push(face);
                let index = self.faces.len() - 1;
                self.fallbacks.insert((primary, character), index);
                return index;
            }
        }
        self.fallbacks.insert((primary, character), primary);
        primary
    }

    /// The family's ascent and its ascent plus descent in pixels: how a CSS line box measures it.
    fn line_metrics(&mut self, run: Run<'_>) -> (f64, f64) {
        let primary = self.primary(run.family, run.weight, run.italic);
        let data = &self.faces[primary];
        let Ok(face) = Face::parse(&data.bytes, data.index) else {
            return (run.size * 0.8, run.size);
        };
        let units = f64::from(face.units_per_em().max(1));
        let ascent = f64::from(face.ascender()) / units * run.size;
        let descent = f64::from(face.descender()) / units * run.size;
        (ascent, (ascent - descent).max(run.size * 0.5))
    }

    fn advance(&mut self, run: Run<'_>, character: char) -> f64 {
        let primary = self.primary(run.family, run.weight, run.italic);
        let face_index = self.face_for(character, primary);
        let data = &self.faces[face_index];
        let Ok(mut face) = Face::parse(&data.bytes, data.index) else {
            return run.size * 0.5;
        };
        let _ = face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), f32::from(run.weight));
        let units = f64::from(face.units_per_em().max(1));
        let advance = face
            .glyph_index(character)
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .map_or(units / 2.0, f64::from);
        advance / units * run.size
    }

    fn glyph(
        &mut self,
        run: Run<'_>,
        character: char,
        pen: f64,
        baseline: f64,
    ) -> (f64, Option<Path>) {
        let primary = self.primary(run.family, run.weight, run.italic);
        let face_index = self.face_for(character, primary);
        let data = &self.faces[face_index];
        let Ok(mut face) = Face::parse(&data.bytes, data.index) else {
            return (run.size * 0.5, None);
        };
        let _ = face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), f32::from(run.weight));
        let units = f64::from(face.units_per_em().max(1));
        let Some(glyph) = face.glyph_index(character) else {
            return (run.size * 0.5, None);
        };
        let advance =
            face.glyph_hor_advance(glyph).map_or(units / 2.0, f64::from) / units * run.size;
        let mut builder = PathBuilder {
            path: Path::default(),
            pen,
            baseline,
            scale: run.size / units,
            italic: run.italic && !face.is_italic(),
        };
        let outline = face.outline_glyph(glyph, &mut builder).is_some();
        (advance, if outline { Some(builder.path) } else { None })
    }
}

fn covers(face: &FaceData, character: char) -> bool {
    Face::parse(&face.bytes, face.index).is_ok_and(|face| face.glyph_index(character).is_some())
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

/// A shadow filter's region and its Gaussian standard deviation.
struct Blur {
    region: Rect,
    sigma: f64,
}

struct SvgIds {
    clip: usize,
    blur: usize,
}

fn scene_svg(scene: &Scene) -> String {
    let mut clips = Vec::new();
    collect_clips(&scene.items, &mut clips);
    let mut blurs = Vec::new();
    collect_blurs(&scene.items, &mut blurs);
    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\" viewBox=\"0 0 {:.0} {:.0}\">",
        scene.width, scene.height, scene.width, scene.height
    ));
    if !clips.is_empty() || !blurs.is_empty() {
        svg.push_str("<defs>");
        for (index, clip) in clips.iter().enumerate() {
            svg.push_str(&clip_svg(index, clip));
        }
        for (index, blur) in blurs.iter().enumerate() {
            svg.push_str(&format!(
                "<filter id=\"blur{index}\" filterUnits=\"userSpaceOnUse\" x=\"{:.3}\" y=\"{:.3}\" width=\"{:.3}\" height=\"{:.3}\"><feGaussianBlur stdDeviation=\"{:.3}\"/></filter>",
                blur.region.x,
                blur.region.y,
                blur.region.width.max(0.0),
                blur.region.height.max(0.0),
                blur.sigma
            ));
        }
        svg.push_str("</defs>");
    }
    svg.push_str(&format!(
        "<rect width=\"100%\" height=\"100%\" fill=\"{}\"/>",
        scene.background.svg_colour()
    ));
    let mut ids = SvgIds { clip: 0, blur: 0 };
    write_items(&mut svg, scene, &scene.items, &mut ids);
    svg.push_str("</svg>");
    svg
}

fn collect_clips(items: &[Item], clips: &mut Vec<Clip>) {
    for item in items {
        if let Item::Group(group) = item {
            if let Some(clip) = group.clip {
                clips.push(clip);
            }
            collect_clips(&group.items, clips);
        }
    }
}

fn collect_blurs(items: &[Item], blurs: &mut Vec<Blur>) {
    for item in items {
        match item {
            Item::Shadow(shadow) if shadow.blur > 0.01 => {
                let sigma = shadow.blur / 2.0;
                blurs.push(Blur {
                    region: inflate(shadow.shape.bounds(), sigma * 3.0),
                    sigma,
                });
            }
            Item::Group(group) => collect_blurs(&group.items, blurs),
            _ => {}
        }
    }
}

fn clip_svg(index: usize, clip: &Clip) -> String {
    let rect = clip.rect;
    let radius = clamp_radius(rect, clip.radius);
    let corners = if radius > 0.01 {
        format!(" rx=\"{radius:.3}\" ry=\"{radius:.3}\"")
    } else {
        String::new()
    };
    format!(
        "<clipPath id=\"clip{index}\" clipPathUnits=\"userSpaceOnUse\"><rect x=\"{:.3}\" y=\"{:.3}\" width=\"{:.3}\" height=\"{:.3}\"{corners}/></clipPath>",
        rect.x,
        rect.y,
        rect.width.max(0.0),
        rect.height.max(0.0)
    )
}

fn shadow_svg(shadow: &Shadow, blur: Option<usize>) -> String {
    let filter = blur.map_or_else(String::new, |index| {
        format!(" filter=\"url(#blur{index})\"")
    });
    let fill = shadow.fill.svg_colour();
    match shadow.shape {
        ShadowShape::Rect { rect, radius } => {
            let radius = clamp_radius(rect, radius);
            let corners = if radius > 0.01 {
                format!(" rx=\"{radius:.3}\" ry=\"{radius:.3}\"")
            } else {
                String::new()
            };
            format!(
                "<rect x=\"{:.3}\" y=\"{:.3}\" width=\"{:.3}\" height=\"{:.3}\"{corners} fill=\"{fill}\"{filter}/>",
                rect.x,
                rect.y,
                rect.width.max(0.0),
                rect.height.max(0.0)
            )
        }
        ShadowShape::Ellipse(rect) => format!(
            "<ellipse cx=\"{:.3}\" cy=\"{:.3}\" rx=\"{:.3}\" ry=\"{:.3}\" fill=\"{fill}\"{filter}/>",
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
            rect.width.max(0.0) / 2.0,
            rect.height.max(0.0) / 2.0
        ),
    }
}

fn write_items(svg: &mut String, scene: &Scene, items: &[Item], ids: &mut SvgIds) {
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
                    "<image x=\"{:.3}\" y=\"{:.3}\" width=\"{:.3}\" height=\"{:.3}\" preserveAspectRatio=\"xMidYMid slice\" href=\"data:{};base64,{}\"/>",
                    picture.x,
                    picture.y,
                    picture.width,
                    picture.height,
                    picture.mime,
                    STANDARD.encode(&picture.bytes)
                ));
            }
            Item::Shadow(shadow) => {
                let blurred = shadow.blur > 0.01;
                let blur = blurred.then_some(ids.blur);
                if blurred {
                    ids.blur += 1;
                }
                svg.push_str(&shadow_svg(shadow, blur));
            }
            Item::Group(group) => {
                let mut attributes = String::new();
                if group.alpha < 0.999 {
                    attributes.push_str(&format!(" opacity=\"{:.3}\"", group.alpha));
                }
                if let Some(rotate) = group.rotate {
                    attributes.push_str(&format!(
                        " transform=\"rotate({:.4} {:.3} {:.3})\"",
                        rotate.degrees, rotate.cx, rotate.cy
                    ));
                }
                if group.clip.is_some() {
                    attributes.push_str(&format!(" clip-path=\"url(#clip{})\"", ids.clip));
                    ids.clip += 1;
                }
                svg.push_str(&format!("<g{attributes}>"));
                write_items(svg, scene, &group.items, ids);
                svg.push_str("</g>");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_background_is_painted_in_svg_png_and_pdf() {
        let mut project = Project::new("Background".into(), crate::Viewport { width: 16, height: 16 }, crate::Appearance::Dark).unwrap();
        assert_eq!(project.pages[0].background, "#ffffff");
        project.pages[0].background = "#123456".into();
        let page = &project.pages[0];
        let svg = boundary_svg(&project, page, page.base_size_id()).unwrap();
        assert!(svg.contains("<rect width=\"100%\" height=\"100%\" fill=\"#123456\"/>"));
        let png = boundary_png(&project, page, page.base_size_id()).unwrap();
        let pixels = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(pixels.get_pixel(0, 0).0, [18, 52, 86, 255]);
        let rendered = RenderPage::new(&project, page, page.base_size_id()).unwrap();
        let pdf = slide_pdf(&project, &[rendered]).unwrap();
        let pdf = String::from_utf8_lossy(&pdf);
        assert!(pdf.contains("0.071 0.204 0.337 rg 0 0 12.000 12.000 re f"));
    }

    const INK: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 1.0,
    };

    fn scene() -> Scene {
        Scene {
            width: 200.0,
            height: 100.0,
            background: Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 1.0,
            },
            paths: Vec::new(),
            items: Vec::new(),
        }
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn boxed(scene: &mut Scene, at: Rect) -> Item {
        let path_ref = scene.push_path(Path::rect(at, 0.0));
        Item::Shape(Shape {
            path_ref,
            fill: Some(INK),
            stroke: None,
        })
    }

    fn group(rotate: Option<Rotate>, alpha: f64, items: Vec<Item>) -> Item {
        Item::Group(Group {
            clip: None,
            rotate,
            alpha,
            items,
        })
    }

    #[test]
    fn a_rotated_layer_turns_about_its_centre_with_its_children() {
        let mut upright = scene();
        let plain = boxed(&mut upright, rect(10.0, 10.0, 20.0, 20.0));
        upright.items.push(plain);
        let mut turned = scene();
        let child = boxed(&mut turned, rect(10.0, 10.0, 20.0, 20.0));
        turned.items.push(group(
            Some(Rotate {
                degrees: 45.0,
                cx: 50.0,
                cy: 25.0,
            }),
            1.0,
            vec![child],
        ));
        let turned_svg = scene_svg(&turned);
        assert!(
            turned_svg.contains("transform=\"rotate(45.0000 50.000 25.000)\""),
            "{turned_svg}"
        );
        assert_ne!(scene_svg(&upright), turned_svg);
    }

    #[test]
    fn shadow_blur_is_a_gaussian_of_half_its_css_radius() {
        let hard = Shadow {
            shape: ShadowShape::Rect {
                rect: rect(20.0, 20.0, 60.0, 40.0),
                radius: 4.0,
            },
            fill: INK,
            blur: 0.0,
        };
        let soft = Shadow {
            blur: 20.0,
            ..hard.clone()
        };
        let mut sharp_scene = scene();
        sharp_scene.items.push(Item::Shadow(hard));
        let mut blurred_scene = scene();
        blurred_scene.items.push(Item::Shadow(soft));
        let sharp_svg = scene_svg(&sharp_scene);
        let blurred_svg = scene_svg(&blurred_scene);
        assert!(!sharp_svg.contains("feGaussianBlur"), "{sharp_svg}");
        assert!(
            blurred_svg.contains("<feGaussianBlur stdDeviation=\"10.000\"/>"),
            "{blurred_svg}"
        );
        assert_ne!(sharp_svg, blurred_svg);
    }

    #[test]
    fn a_pdf_embeds_a_blurred_shadow_and_carries_rotation() {
        let mut page = scene();
        let child = boxed(&mut page, rect(20.0, 20.0, 60.0, 40.0));
        page.items.push(Item::Shadow(Shadow {
            shape: ShadowShape::Ellipse(rect(20.0, 20.0, 60.0, 40.0)),
            fill: Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 0.4,
            },
            blur: 20.0,
        }));
        page.items.push(Item::Group(Group {
            clip: Some(Clip {
                rect: rect(20.0, 20.0, 60.0, 40.0),
                radius: 8.0,
            }),
            rotate: Some(Rotate {
                degrees: 30.0,
                cx: 50.0,
                cy: 40.0,
            }),
            alpha: 0.5,
            items: vec![child],
        }));
        let pdf = pdf::deck(&[page]);
        assert!(pdf.starts_with(b"%PDF-1."));
        assert!(pdf.windows(6).any(|window| window == b"%%EOF\n"));
        let content = String::from_utf8_lossy(&pdf);
        assert!(content.contains("/ExtGState"), "opacity states are listed");
        assert!(content.contains(" cm\n"), "the rotation is a matrix");
        assert!(content.contains("/Subtype /Image"), "the blurred shadow is embedded");
        assert!(content.contains("/SMask"), "the blur has a soft alpha mask");
    }

    #[test]
    fn opacity_multiplies_down_the_tree() {
        let mut outer = scene();
        let child = boxed(&mut outer, rect(10.0, 10.0, 20.0, 20.0));
        outer
            .items
            .push(group(None, 0.5, vec![group(None, 0.5, vec![child])]));
        let svg = scene_svg(&outer);
        assert_eq!(svg.matches("opacity=\"0.500\"").count(), 2, "{svg}");
    }

    #[test]
    fn resolver_uses_archivo_only_when_requested_or_missing() {
        let explicit = resolve_font(" 'Archivo' ", 700, true);
        let missing = resolve_font("Desegnos nonexistent font 987654", 700, true);
        assert!(explicit.bundled && !explicit.fell_back);
        assert!(missing.bundled && missing.fell_back);
        assert_eq!(explicit.id, missing.id);
        assert_eq!(explicit.bytes.as_slice(), ARCHIVO);
        let list = resolve_font("Desegnos nonexistent font 987654, Archivo", 400, false);
        assert!(!list.fell_back);
        assert_eq!(list.id, explicit.id);
    }

    #[test]
    fn resolver_matches_fontdb_style_selection() {
        // Compare against fontdb directly when this host has a generic sans-serif.
        let mut database = Database::new();
        database.load_system_fonts();
        let query = Query {
            families: &[Family::SansSerif],
            weight: Weight(700),
            stretch: Stretch::Normal,
            style: Style::Italic,
        };
        if let Some(id) = database.query(&query) {
            let resolved = resolve_font("system-ui", 700, true);
            assert!(!resolved.fell_back);
            database
                .with_face_data(id, |bytes, index| {
                    assert_eq!(resolved.bytes.as_slice(), bytes);
                    assert_eq!(resolved.index, index);
                })
                .unwrap();
            let alias = resolve_font("sans-serif", 700, true);
            assert_eq!(resolved.id, alias.id);
            assert!(std::sync::Arc::ptr_eq(&resolved.bytes, &alias.bytes));
        }
    }

    #[test]
    fn a_generic_family_maps_to_its_system_face() {
        assert!(matches!(generic_family("Sans-Serif"), Family::SansSerif));
        assert!(matches!(generic_family("system-ui"), Family::SansSerif));
        assert!(matches!(generic_family("serif"), Family::Serif));
        assert!(matches!(generic_family("monospace"), Family::Monospace));
        assert!(matches!(generic_family("'Archivo'"), Family::Name(name) if name == "'Archivo'"));

        let mut fonts = FontSet::new();
        assert_eq!(fonts.primary("Archivo", 400, false), 0, "the bundled face");
        assert_eq!(
            fonts.primary("No Such Family, Anywhere", 400, false),
            0,
            "the last resort"
        );
        assert!(fonts.primary("system-ui, sans-serif", 400, false) != 0);
    }

    /// Whether this machine has a monospace and a serif face that differ, which the font test
    /// below needs; a bare container without fonts falls back to the bundled Archivo.
    fn distinct_monospace_and_serif() -> bool {
        let mut fonts = FontSet::new();
        let monospace = fonts.primary("monospace", 400, false);
        let serif = fonts.primary("serif", 400, false);
        monospace != 0 && serif != 0 && monospace != serif
    }

    fn text_element(family: &str) -> Element {
        let mut element = crate::empty_element(ElementKind::Text, rect(0.0, 0.0, 120.0, 40.0));
        element.text = "Handgloves 123".into();
        element.style.text_style.font_family = family.into();
        element
    }

    fn text_svg(family: &str) -> String {
        let element = text_element(family);
        let mut fonts = FontSet::new();
        let mut target = scene();
        let mut items = Vec::new();
        paint_text(&mut target, &mut fonts, &element, element.rect, &mut items);
        target.items.extend(items);
        scene_svg(&target)
    }

    #[test]
    fn the_font_family_changes_the_outlines() {
        if !distinct_monospace_and_serif() {
            return;
        }
        let monospace = text_svg("monospace");
        let serif = text_svg("serif");
        assert!(monospace.contains("<path"), "text is outlines, not a font");
        assert_ne!(monospace, serif);
    }

    #[test]
    fn text_wraps_at_its_width_and_never_in_a_single_line_control() {
        let mut fonts = FontSet::new();
        let element = text_element("Archivo");
        let style = &element.style.text_style;
        assert!(wrap_text(&mut fonts, "one two three four five six", style, 60.0, true).len() > 1);
        assert_eq!(
            wrap_text(
                &mut fonts,
                "one two three four five six",
                style,
                60.0,
                false
            )
            .len(),
            1
        );
        assert!(!wraps(&ElementKind::Button));
        assert!(wraps(&ElementKind::Text));
    }

    #[test]
    fn fill_reaches_parent_edge_and_hug_uses_child_bounds_with_limits() {
        let mut fonts = FontSet::new();
        let mut child = crate::empty_element(ElementKind::Box, rect(20.0, 10.0, 40.0, 30.0));
        child.style.sizing.width = crate::SizingMode::Fill;
        child.style.sizing.height = crate::SizingMode::Fill;
        child.style.sizing.max_width = Some(70.0);
        let filled = sized_in(&mut fonts, &child, rect(0.0, 0.0, 100.0, 90.0));
        assert_eq!((filled.width, filled.height), (70.0, 80.0));
        let mut parent = crate::empty_element(ElementKind::Box, rect(0.0, 0.0, 200.0, 200.0));
        parent.style.sizing.width = crate::SizingMode::Fit;
        parent.style.sizing.height = crate::SizingMode::Fit;
        parent.children.push(child);
        let hugged = resolve_auto_size(&mut fonts, &parent, parent.rect);
        assert_eq!((hugged.width, hugged.height), (60.0, 40.0));
        parent.layout = ElementLayout::Stack { direction: crate::StackDirection::Row, gap: 8.0, padding: 4.0, align: StackAlign::Start, justify: StackJustify::Start, wrap: false };
        let hugged = resolve_auto_size(&mut fonts, &parent, parent.rect);
        assert_eq!((hugged.width, hugged.height), (48.0, 38.0));
    }

    #[test]
    fn a_text_layer_without_a_size_takes_the_size_of_its_text() {
        let mut fonts = FontSet::new();
        let mut element = text_element("Archivo");
        element.rect = rect(0.0, 0.0, 0.0, 0.0);
        let at = resolve_auto_size(&mut fonts, &element, element.rect);
        assert!(at.width > 1.0, "{at:?}");
        assert!(at.height > element.style.text_style.size, "{at:?}");
    }
}
