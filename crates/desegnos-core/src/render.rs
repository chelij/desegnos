//! Direct SVG, PNG and PDF scene painting. The canvas uses CSS border boxes, inside strokes,
//! centred line boxes and cover-cropped images; exported scenes follow those conventions.
//! Text becomes glyph outlines so exported documents need no installed font.
mod pdf;
use crate::element_style::{padding as effective_padding, text_of, wraps};

pub use crate::geometry::Rgba;
use crate::geometry::*;
use crate::text_layout::*;
pub use crate::text_layout::{ResolvedFont, font_families, resolve_font, standalone_face};
use crate::{
    Element, ElementKind, ElementLayout, ElementStyle, ElementTextStyle, Error, Page, Project,
    Rect, Result, StackAlign, StackJustify, TextAlign,
};

pub use crate::limits::{MAX_RENDER_EDGE, MAX_RENDER_PIXELS};

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

/// Rotation and reflection about the centre of a layer's rect, then a translation: content
/// scaled into another layer's box needs both.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LayerTransform {
    pub sx: f64,
    pub sy: f64,
    pub degrees: f64,
    pub cx: f64,
    pub cy: f64,
    pub tx: f64,
    pub ty: f64,
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
    pub transform: Option<LayerTransform>,
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

pub fn slide_pdf(project: &Project, slides: &[RenderPage<'_>]) -> Result<Vec<u8>> {
    if slides.is_empty() {
        return Err(Error("No slide boundaries are included.".into()));
    }
    let mut scenes = Vec::with_capacity(slides.len());
    for slide in slides {
        scenes.push(build_scene(project, slide));
    }
    pdf::deck(&scenes)
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
    let stack = [page.page.id];
    for element in &page.elements {
        let rect = sized_in(
            &mut fonts,
            element,
            Rect {
                x: 0.0,
                y: 0.0,
                width: page.width,
                height: page.height,
            },
        );
        paint_element(
            project, &mut scene, &mut fonts, element, rect, &mut items, &stack,
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
        transform: None,
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
    stack: &[uuid::Uuid],
) {
    if !element.visible {
        return;
    }
    let style = &element.style;
    let opacity = style.opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let at = resolve_auto_size(fonts, element, at, at.width);
    // A line carries no height; every other element needs a real box to draw in.
    if at.width <= 0.0 || (at.height <= 0.0 && element.kind != ElementKind::Line) {
        return;
    }
    let radius = layer_radius(element);
    let stroke = stroke_of(style);
    // The canvas lays children out in the content box, inside the element's border.
    let content = inset(at, crate::element_style::border_width(element));
    let mut items: Vec<Item> = Vec::new();
    if let Some(shadow) = shadow_of(style, &element.kind, at, radius) {
        items.push(shadow);
    }
    let mut own: Vec<Item> = Vec::new();
    match element.kind {
        // A group paints its fill like the exporter does, but never its border or shadow.
        ElementKind::Group => {
            if let Some(fill) = style
                .fill
                .as_deref()
                .and_then(Rgba::parse)
                .filter(|colour| colour.a > 0.0)
            {
                own.push(Item::Shape(Shape {
                    path_ref: scene.push_path(Path::rect(at, radius)),
                    fill: Some(fill),
                    stroke: None,
                }));
            }
        }
        ElementKind::Line => paint_line(scene, style, at, &mut own),
        ElementKind::Ellipse => paint_ellipse(scene, style, at, &mut own),
        ElementKind::Image => paint_picture(project, element, at, &mut own),
        _ => paint_surface(scene, style, at, radius, &mut own),
    }
    if !own.is_empty() {
        // A border is drawn inside the border box, so the outer half of a stroke is cut away;
        // a picture is cut to the rounded box as `object-fit: cover` plus a radius does.
        let clip = ((stroke.is_some() && element.kind != ElementKind::Line)
            || element.kind == ElementKind::Image)
            .then_some(Clip { rect: at, radius });
        items.push(Item::Group(Group {
            clip,
            transform: None,
            alpha: 1.0,
            items: own,
        }));
    }
    let mut content_items: Vec<Item> = Vec::new();
    paint_text(scene, fonts, element, content, &mut content_items);
    paint_children(
        project,
        scene,
        fonts,
        element,
        content,
        &mut content_items,
        stack,
    );
    if element.kind == ElementKind::Embed {
        paint_embedded_page(
            project,
            scene,
            fonts,
            element,
            content,
            stack,
            &mut content_items,
        );
    }
    if !content_items.is_empty() {
        // Boxes, groups and lines let content overflow, matching the canvas.
        let clip =
            crate::element_style::clips_content(&element.kind).then_some(Clip { rect: at, radius });
        items.push(Item::Group(Group {
            clip,
            transform: None,
            alpha: 1.0,
            items: content_items,
        }));
    }
    if items.is_empty() {
        return;
    }
    let degrees = element.rotation % 360.0;
    let transform = crate::element_style::transformed(element).then_some(LayerTransform {
        sx: if element.style.flip_x { -1.0 } else { 1.0 },
        sy: if element.style.flip_y { -1.0 } else { 1.0 },
        degrees,
        cx: at.x + at.width / 2.0,
        cy: at.y + at.height / 2.0,
        tx: 0.0,
        ty: 0.0,
    });
    out.push(Item::Group(Group {
        clip: None,
        transform,
        alpha: opacity,
        items,
    }));
}

/// The page an Embed frames, drawn live inside its box: the linked size that best fits the
/// width, scaled to it and clipped to it. A page never frames itself, and nesting stops after
/// two frames, matching the canvas.
fn paint_embedded_page(
    project: &Project,
    scene: &mut Scene,
    fonts: &mut FontSet,
    element: &Element,
    at: Rect,
    stack: &[uuid::Uuid],
    out: &mut Vec<Item>,
) {
    let Some(page) = element
        .url
        .as_deref()
        .and_then(|url| project.pages.iter().find(|page| page.path == url))
    else {
        return;
    };
    if stack.len() >= crate::element_style::MAX_EMBED_DEPTH || stack.contains(&page.id) {
        return;
    }
    let size = page
        .sizes
        .iter()
        .filter(|size| f64::from(size.width) <= at.width)
        .max_by_key(|size| size.width)
        .or_else(|| page.sizes.iter().min_by_key(|size| size.width));
    let Some(size) = size else { return };
    let scale = at.width / f64::from(size.width).max(1.0);
    if !scale.is_finite() || scale <= 0.0 {
        return;
    }
    let page_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: f64::from(size.width),
        height: f64::from(size.height),
    };
    let mut items: Vec<Item> = Vec::new();
    if let Some(background) = Rgba::parse(&page.background)
        && background.a > 0.0
    {
        let path_ref = scene.push_path(Path::rect(page_rect, 0.0));
        items.push(Item::Shape(Shape {
            path_ref,
            fill: Some(background),
            stroke: None,
        }));
    }
    let mut next = stack.to_vec();
    next.push(page.id);
    for child in crate::resolve_for_size(project, page, size.id) {
        let rect = sized_in(fonts, &child, page_rect);
        paint_element(project, scene, fonts, &child, rect, &mut items, &next);
    }
    out.push(Item::Group(Group {
        clip: None,
        transform: Some(LayerTransform {
            sx: scale,
            sy: scale,
            degrees: 0.0,
            cx: 0.0,
            cy: 0.0,
            tx: at.x,
            ty: at.y,
        }),
        alpha: 1.0,
        items,
    }));
}

fn layer_radius(element: &Element) -> f64 {
    if element.kind == ElementKind::Ellipse {
        0.0
    } else {
        element.style.radius.max(0.0)
    }
}

fn shadow_of(style: &ElementStyle, kind: &ElementKind, at: Rect, radius: f64) -> Option<Item> {
    if !crate::element_style::draws_border(kind) {
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
        stroke: Some((colour, crate::element_style::line_width(style.stroke.width))),
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

pub(crate) fn stroke_of(style: &ElementStyle) -> Option<(Rgba, f64)> {
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

fn paint_children(
    project: &Project,
    scene: &mut Scene,
    fonts: &mut FontSet,
    element: &Element,
    content: Rect,
    out: &mut Vec<Item>,
    stack: &[uuid::Uuid],
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
                paint_element(
                    project,
                    scene,
                    fonts,
                    child,
                    offset(content, rect),
                    out,
                    stack,
                );
            }
        }
        ElementLayout::Stack {
            direction,
            gap,
            padding: _,
            align,
            justify,
            wrap,
        } => {
            let row = matches!(direction, crate::StackDirection::Row);
            let inner = inset_sides(content, effective_padding(element));
            let extent = if row { inner.width } else { inner.height };
            let cross_extent = if row { inner.height } else { inner.width };
            let mut rects: Vec<_> = children
                .iter()
                .map(|child| resolve_auto_size(fonts, child, child.rect, inner.width))
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
                    paint_element(project, scene, fonts, child, rect, out, stack);
                    cursor += main(rect) + gap + between;
                }
                cross += line_cross + gap;
            }
        }
        ElementLayout::Grid {
            columns,
            gap,
            padding: _,
        } => {
            let inner = inset_sides(content, effective_padding(element));
            let columns = *columns as usize;
            let width = crate::element_style::grid_column_width(inner.width, columns as u32, *gap);
            let natural: Vec<f64> = children
                .chunks(columns)
                .map(|row| {
                    row.iter()
                        .filter(|child| child.style.sizing.height != crate::SizingMode::Fill)
                        .map(|child| resolve_auto_size(fonts, child, child.rect, width).height)
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
                    .map(|child| resolve_auto_size(fonts, child, child.rect, width))
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
                    paint_element(project, scene, fonts, child, rect, out, stack);
                }
                y += height + gap;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

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
    let colour = Rgba::parse(&style.color).unwrap_or_else(|| {
        Rgba::parse(&ElementTextStyle::default().color).expect("default text colour")
    });
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
        let decoration_y = match style.decoration {
            Some(crate::TextDecoration::Underline) => Some(baseline + style.size * 0.1),
            Some(crate::TextDecoration::LineThrough) => Some(baseline - style.size * 0.3),
            _ => None,
        };
        if let Some(y) = decoration_y {
            let path_ref = scene.push_path(Path::rect(
                Rect {
                    x: boxed.x + offset,
                    y,
                    width,
                    height: (style.size / 16.0).max(1.0),
                },
                0.0,
            ));
            out.push(Item::Shape(Shape {
                path_ref,
                fill: Some(colour),
                stroke: None,
            }));
        }
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
                if let Some(transform) = group.transform {
                    let reflection = if transform.sx == 1.0 && transform.sy == 1.0 {
                        String::new()
                    } else {
                        format!(
                            " translate({:.3} {:.3}) scale({} {}) translate({:.3} {:.3})",
                            transform.cx,
                            transform.cy,
                            transform.sx,
                            transform.sy,
                            -transform.cx,
                            -transform.cy
                        )
                    };
                    let shift = if transform.tx == 0.0 && transform.ty == 0.0 {
                        String::new()
                    } else {
                        format!("translate({:.3} {:.3}) ", transform.tx, transform.ty)
                    };
                    attributes.push_str(&format!(
                        " transform=\"{shift}rotate({:.4} {:.3} {:.3}){}\"",
                        transform.degrees, transform.cx, transform.cy, reflection
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
    use fontdb::{Family, Query, Stretch, Style, Weight};
    use ttf_parser::Face;
    const ARCHIVO: &[u8] = include_bytes!("../assets/Archivo-variable.ttf");

    #[test]
    fn boundary_background_is_painted_in_svg_png_and_pdf() {
        let mut project = Project::new(
            "Background".into(),
            crate::Viewport {
                width: 16,
                height: 16,
            },
            crate::Appearance::Dark,
        )
        .unwrap();
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

    #[test]
    fn an_embed_draws_the_framed_page_and_never_the_page_it_sits_on() {
        let project = Project::new(
            "Frames".into(),
            crate::Viewport {
                width: 400,
                height: 300,
            },
            crate::Appearance::Light,
        )
        .unwrap();
        let mut project = project
            .apply(
                project.revision,
                crate::Command::AddCanvasPage {
                    name: "About".into(),
                    path: "/about".into(),
                    size: crate::PageSize {
                        id: uuid::Uuid::new_v4(),
                        label: "Desktop".into(),
                        width: 800,
                        height: 600,
                        kind: crate::BoundaryKind::Page,
                        x: 0.0,
                        y: 1000.0,
                    },
                    moves: vec![],
                    reference: vec![],
                },
            )
            .unwrap();
        let mut framed = crate::empty_element(ElementKind::Text, rect(20.0, 20.0, 200.0, 40.0));
        framed.text = "Framed".into();
        project.pages[1].elements = vec![framed];
        let mut embed = crate::empty_element(ElementKind::Embed, rect(10.0, 10.0, 200.0, 100.0));
        embed.url = Some("/about".into());
        project.pages[0].elements = vec![embed.clone()];
        let page = &project.pages[0];
        let svg = boundary_svg(&project, page, page.base_size_id()).unwrap();
        // The framed page's text arrives as glyph paths, scaled from its size into the box.
        assert!(svg.matches("<path").count() > 1, "{svg}");
        assert!(svg.contains("scale(0.25 0.25)"), "{svg}");
        embed.url = Some(project.pages[0].path.clone());
        project.pages[0].elements = vec![embed];
        let page = &project.pages[0];
        let svg = boundary_svg(&project, page, page.base_size_id()).unwrap();
        assert!(!svg.contains("scale("), "{svg}");
        assert!(svg.matches("<path").count() <= 1, "{svg}");
    }

    #[test]
    fn boundary_png_zero_height_line_preserves_six_pixel_stroke() {
        let mut project = Project::new(
            "Line".into(),
            crate::Viewport {
                width: 40,
                height: 40,
            },
            crate::Appearance::Light,
        )
        .unwrap();
        let mut line = crate::empty_element(ElementKind::Line, rect(5.0, 20.0, 30.0, 0.0));
        line.style.stroke.width = 6.0;
        line.style.stroke.color = Some("#000000".into());
        project.pages[0].elements.push(line);
        let page = &project.pages[0];
        let png = boundary_png(&project, page, page.base_size_id()).unwrap();
        let pixels = image::load_from_memory(&png).unwrap().to_rgba8();
        let painted: Vec<_> = (0..pixels.height())
            .filter(|y| pixels.get_pixel(20, *y)[0] < 200)
            .collect();
        assert!(
            (6..=7).contains(&painted.len()),
            "painted rows: {painted:?}"
        );
        assert!(painted.first().unwrap() < &20 && painted.last().unwrap() > &20);
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

    fn group(transform: Option<LayerTransform>, alpha: f64, items: Vec<Item>) -> Item {
        Item::Group(Group {
            clip: None,
            transform,
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
            Some(LayerTransform {
                sx: 1.0,
                sy: 1.0,
                degrees: 45.0,
                cx: 50.0,
                cy: 25.0,
                tx: 0.0,
                ty: 0.0,
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
    fn reflection_moves_a_child_about_the_layer_centre_in_svg_and_png() {
        let mut reflected = scene();
        let child = boxed(&mut reflected, rect(10.0, 10.0, 20.0, 20.0));
        reflected.items.push(group(
            Some(LayerTransform {
                sx: -1.0,
                sy: 1.0,
                degrees: 0.0,
                cx: 50.0,
                cy: 25.0,
                tx: 0.0,
                ty: 0.0,
            }),
            1.0,
            vec![child],
        ));
        let mut expected = scene();
        let child = boxed(&mut expected, rect(70.0, 10.0, 20.0, 20.0));
        expected.items.push(child);
        let svg = scene_svg(&reflected);
        assert!(svg.contains("scale(-1 1)"), "{svg}");
        assert_eq!(
            rasterize(&svg, reflected.width, reflected.height).unwrap(),
            rasterize(&scene_svg(&expected), expected.width, expected.height).unwrap()
        );
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
            transform: Some(LayerTransform {
                sx: 1.0,
                sy: 1.0,
                degrees: 30.0,
                cx: 50.0,
                cy: 40.0,
                tx: 0.0,
                ty: 0.0,
            }),
            alpha: 0.5,
            items: vec![child],
        }));
        let pdf = pdf::deck(&[page]).unwrap();
        assert!(pdf.starts_with(b"%PDF-1."));
        assert!(pdf.windows(6).any(|window| window == b"%%EOF\n"));
        let content = String::from_utf8_lossy(&pdf);
        assert!(content.contains("/ExtGState"), "opacity states are listed");
        assert!(content.contains(" cm\n"), "the rotation is a matrix");
        assert!(
            content.contains("/Subtype /Image"),
            "the blurred shadow is embedded"
        );
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
    fn a_missing_serif_family_keeps_a_serif_look() {
        let generic = resolve_font("serif", 400, false);
        let missing = resolve_font("Desegnos missing Times 987654", 400, false);
        if !generic.bundled {
            assert_eq!(missing.id, generic.id);
        }
        let sans = resolve_font("Desegnos missing font 987654", 400, false);
        assert!(sans.bundled && sans.fell_back);
    }

    #[test]
    fn generic_families_resolve_to_faces_that_draw_latin_text() {
        // A symbol font maps the letter 'a' to its Greek alpha glyph.
        for family in [
            "serif",
            "sans-serif",
            "monospace",
            "cursive",
            "fantasy",
            "Georgia, serif",
        ] {
            let resolved = resolve_font(family, 400, false);
            let face = Face::parse(&resolved.bytes, resolved.index).unwrap();
            let glyph = face
                .glyph_index('a')
                .unwrap_or_else(|| panic!("{family} has no glyph for 'a'"));
            if let Some(name) = face.glyph_name(glyph) {
                assert_eq!(name, "a", "{family} draws 'a' with the glyph {name}");
            }
        }
    }

    #[test]
    fn resolver_matches_fontdb_style_selection() {
        // Compare against fontdb directly when this host has a generic sans-serif.
        let database = system_fonts();
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

    #[test]
    fn text_controls_use_stored_alignment_and_decoration() {
        for kind in [
            ElementKind::Text,
            ElementKind::Button,
            ElementKind::Link,
            ElementKind::Input {
                input_type: crate::InputType::Text,
            },
            ElementKind::Textarea,
            ElementKind::Select { options: vec![] },
            ElementKind::Checkbox,
            ElementKind::Radio,
        ] {
            let mut element = text_element("Archivo");
            element.kind = kind;
            for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
                element.style.text_style.align = align;
                assert_eq!(text_box(&element, element.rect).1, align);
            }
            let mut fonts = FontSet::new();
            let mut target = scene();
            let mut plain = Vec::new();
            paint_text(&mut target, &mut fonts, &element, element.rect, &mut plain);
            for decoration in [
                crate::TextDecoration::Underline,
                crate::TextDecoration::LineThrough,
            ] {
                element.style.text_style.decoration = Some(decoration);
                let mut decorated = Vec::new();
                paint_text(
                    &mut target,
                    &mut fonts,
                    &element,
                    element.rect,
                    &mut decorated,
                );
                let (boxed, _) = text_box(&element, element.rect);
                let lines = wrap_text(
                    &mut fonts,
                    &element.text,
                    &element.style.text_style,
                    boxed.width,
                    wraps(&element.kind),
                )
                .len();
                assert_eq!(decorated.len(), plain.len() + lines);
            }
        }
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
    fn fit_text_wraps_at_boundary_and_syncs_the_painted_size() {
        let mut project = Project::new(
            "Phone".into(),
            crate::Viewport {
                width: 390,
                height: 844,
            },
            crate::Appearance::Dark,
        )
        .unwrap();
        let mut heading = text_element("Archivo");
        heading.rect = rect(104.0, 0.0, 800.0, 60.0);
        heading.text = "A heading that wraps inside the phone boundary".into();
        heading.style.text_style.size = 48.0;
        heading.style.sizing.width = crate::SizingMode::Fit;
        heading.style.sizing.height = crate::SizingMode::Fit;
        project.pages[0].elements = vec![heading];
        assert!(sync_fit_rects(&mut project));
        let heading = &project.pages[0].elements[0];
        assert_eq!(heading.rect.width, 286.0);
        let mut fonts = FontSet::new();
        let lines = wrap_text(
            &mut fonts,
            &heading.text,
            &heading.style.text_style,
            286.0,
            true,
        );
        assert!(lines.len() > 1);
        assert_eq!(
            heading.rect.height,
            // Hug height rounds the lines up to a whole pixel and keeps one pixel of slack.
            (lines.len() as f64 * line_height(&heading.style.text_style)).ceil() + 1.0
        );
        let painted = sized_in(&mut fonts, heading, rect(0.0, 0.0, 390.0, 844.0));
        assert_eq!(painted, heading.rect);
        assert_eq!(
            resolve_auto_size(&mut fonts, heading, painted, painted.width),
            painted
        );
        let page = &project.pages[0];
        let svg = boundary_svg(&project, page, page.base_size_id()).unwrap();
        let mut fixed_project = project.clone();
        fixed_project.pages[0].elements[0].style.sizing.width = crate::SizingMode::Fixed;
        fixed_project.pages[0].elements[0].style.sizing.height = crate::SizingMode::Fixed;
        let page = &fixed_project.pages[0];
        assert_eq!(
            svg,
            boundary_svg(&fixed_project, page, page.base_size_id()).unwrap()
        );
        let before = project.pages[0].elements[0].rect;
        sync_fit_rects(&mut project);
        assert_eq!(project.pages[0].elements[0].rect, before);

        // Free boxes subtract the child offset; stacks use their padded content width.
        let mut parent = crate::empty_element(ElementKind::Box, rect(0.0, 0.0, 240.0, 600.0));
        parent.children = project.pages[0].elements.clone();
        project.pages[0].elements = vec![parent];
        sync_fit_rects(&mut project);
        assert_eq!(project.pages[0].elements[0].children[0].rect.width, 136.0);
        project.pages[0].elements[0].layout = ElementLayout::Stack {
            direction: crate::StackDirection::Column,
            gap: 0.0,
            padding: 20.0,
            align: StackAlign::Start,
            justify: StackJustify::Start,
            wrap: false,
        };
        sync_fit_rects(&mut project);
        assert_eq!(project.pages[0].elements[0].children[0].rect.width, 200.0);

        project.pages[0].elements[0].style.sizing.width = crate::SizingMode::Fit;
        project.pages[0].elements[0].style.sizing.height = crate::SizingMode::Fit;
        project.pages[0].elements[0].style.sizing.max_width = Some(160.0);
        sync_fit_rects(&mut project);
        let parent = &project.pages[0].elements[0];
        assert_eq!(parent.rect.width, 160.0);
        assert_eq!(parent.children[0].rect.width, 120.0);
        assert_eq!(parent.rect.height, parent.children[0].rect.height + 40.0);

        for kind in [
            ElementKind::Button,
            ElementKind::Link,
            ElementKind::Input {
                input_type: crate::InputType::Text,
            },
        ] {
            let mut control = project.pages[0].elements[0].children[0].clone();
            control.kind = kind;
            let measured = sized_in(&mut fonts, &control, rect(0.0, 0.0, 390.0, 844.0));
            assert_eq!(measured.width, 286.0);
            assert_eq!(
                measured.height,
                line_height(&control.style.text_style).ceil() + 1.0
            );
        }
        let mut fixed = project.pages[0].elements[0].children[0].clone();
        fixed.style.sizing.width = crate::SizingMode::Fixed;
        fixed.rect.width = 800.0;
        assert_eq!(
            sized_in(&mut fonts, &fixed, rect(0.0, 0.0, 390.0, 844.0)).width,
            800.0
        );
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
        let hugged = resolve_auto_size(&mut fonts, &parent, parent.rect, f64::INFINITY);
        assert_eq!((hugged.width, hugged.height), (60.0, 40.0));
        parent.layout = ElementLayout::Stack {
            direction: crate::StackDirection::Row,
            gap: 8.0,
            padding: 4.0,
            align: StackAlign::Start,
            justify: StackJustify::Start,
            wrap: false,
        };
        let hugged = resolve_auto_size(&mut fonts, &parent, parent.rect, f64::INFINITY);
        assert_eq!((hugged.width, hugged.height), (48.0, 38.0));
    }

    /// Every axis-aligned box painted by a scene, in paint order, read from its outline.
    fn painted_boxes(scene: &Scene, items: &[Item]) -> Vec<Rect> {
        let mut boxes = Vec::new();
        for item in items {
            match item {
                Item::Shape(Shape { path_ref, .. }) => {
                    let point = |index: usize| match scene.paths[*path_ref].segments[index] {
                        Segment::Move(point) | Segment::Line(point) => point,
                        _ => panic!("a box outline starts with two corners"),
                    };
                    let (origin, across, down) = (point(0), point(1), point(2));
                    boxes.push(rect(
                        origin.x,
                        origin.y,
                        across.x - origin.x,
                        down.y - origin.y,
                    ));
                }
                Item::Group(group) => boxes.extend(painted_boxes(scene, &group.items)),
                _ => {}
            }
        }
        boxes
    }

    #[test]
    fn uneven_padding_insets_children_and_hug_on_each_named_side() {
        let mut fonts = FontSet::new();
        let mut parent = crate::empty_element(ElementKind::Box, rect(0.0, 0.0, 200.0, 200.0));
        parent.style.sizing.width = crate::SizingMode::Fit;
        parent.style.sizing.height = crate::SizingMode::Fit;
        parent.style.padding = Some(crate::ElementPadding {
            top: 8.0,
            right: 16.0,
            bottom: 4.0,
            left: 24.0,
        });
        parent.layout = ElementLayout::Stack {
            direction: crate::StackDirection::Column,
            gap: 6.0,
            padding: 2.0,
            align: StackAlign::Start,
            justify: StackJustify::Start,
            wrap: false,
        };
        let mut first = crate::empty_element(ElementKind::Box, rect(0.0, 0.0, 40.0, 20.0));
        let mut second = crate::empty_element(ElementKind::Box, rect(0.0, 0.0, 30.0, 10.0));
        for child in [&mut first, &mut second] {
            child.style.fill = Some("#000000".into());
        }
        parent.children = vec![first, second];
        let hugged = resolve_auto_size(&mut fonts, &parent, parent.rect, f64::INFINITY);
        assert_eq!(
            (hugged.width, hugged.height),
            (80.0, 48.0),
            "40 wide + 24 left + 16 right, 20 + 6 + 10 tall + 8 top + 4 bottom"
        );

        let project = Project::new(
            "Padding".into(),
            crate::Viewport {
                width: 200,
                height: 200,
            },
            crate::Appearance::Dark,
        )
        .unwrap();
        let mut target = scene();
        let mut items = Vec::new();
        paint_children(
            &project,
            &mut target,
            &mut fonts,
            &parent,
            parent.rect,
            &mut items,
            &[],
        );
        assert_eq!(
            painted_boxes(&target, &items),
            vec![rect(24.0, 8.0, 40.0, 20.0), rect(24.0, 34.0, 30.0, 10.0)]
        );

        parent.style.padding = None;
        let uniform = resolve_auto_size(&mut fonts, &parent, parent.rect, f64::INFINITY);
        assert_eq!(
            (uniform.width, uniform.height),
            (44.0, 40.0),
            "the layout's all-sides padding remains the fallback"
        );
    }

    #[test]
    fn a_text_layer_without_a_size_takes_the_size_of_its_text() {
        let mut fonts = FontSet::new();
        let mut element = text_element("Archivo");
        element.rect = rect(0.0, 0.0, 0.0, 0.0);
        let at = resolve_auto_size(&mut fonts, &element, element.rect, f64::INFINITY);
        assert!(at.width > 1.0, "{at:?}");
        assert!(at.height > element.style.text_style.size, "{at:?}");
    }
}
