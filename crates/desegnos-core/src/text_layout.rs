pub(crate) const BUNDLED_FONT_ID: &str = "archivo";
use crate::element_style::border_width;
use crate::element_style::{control_padding, padding as effective_padding, text_of, wraps};
use crate::geometry::{Path, Point};
use crate::{
    elements::Element, elements::ElementKind, elements::ElementLayout, elements::ElementTextStyle,
    elements::TextAlign, model::Project, model::Rect,
};
use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use std::collections::BTreeMap;
use ttf_parser::{Face, OutlineBuilder};
const ARCHIVO: &[u8] = include_bytes!("../assets/Archivo-variable.ttf");
pub(crate) fn sized_in(fonts: &mut FontSet, element: &Element, parent: Rect) -> Rect {
    let mut rect = element.rect;
    if element.style.sizing.width == crate::elements::SizingMode::Fill {
        rect.width = (parent.width - rect.x).max(0.0);
    }
    if element.style.sizing.height == crate::elements::SizingMode::Fill {
        rect.height = (parent.height - rect.y).max(0.0);
    }
    resolve_auto_size(fonts, element, rect, (parent.width - rect.x).max(0.0))
}

pub(crate) fn offset(content: Rect, rect: Rect) -> Rect {
    Rect {
        x: content.x + rect.x,
        y: content.y + rect.y,
        width: rect.width,
        height: rect.height,
    }
}

/// Space offered to children, including borders, layout padding and grid column widths.
pub(crate) fn child_content_rect(element: &Element, at: Rect) -> Rect {
    let content = inset(at, border_width(element));
    match element.layout {
        ElementLayout::Free => content,
        ElementLayout::Stack { .. } => inset_sides(content, effective_padding(element)),
        ElementLayout::Grid {
            columns,
            gap,
            padding: _,
        } => {
            let mut inner = inset_sides(content, effective_padding(element));
            inner.width = crate::element_style::grid_column_width(inner.width, columns, gap);
            inner
        }
    }
}

pub(crate) fn inset_sides(rect: Rect, padding: crate::elements::ElementPadding) -> Rect {
    let left = padding.left.max(0.0);
    let right = padding.right.max(0.0);
    let top = padding.top.max(0.0);
    let bottom = padding.bottom.max(0.0);
    Rect {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    }
}

pub(crate) fn inset(rect: Rect, amount: f64) -> Rect {
    inset_sides(
        rect,
        crate::elements::ElementPadding {
            top: amount,
            right: amount,
            bottom: amount,
            left: amount,
        },
    )
}

pub(crate) fn inflate(rect: Rect, amount: f64) -> Rect {
    let amount = amount.max(0.0);
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: rect.width + amount * 2.0,
        height: rect.height + amount * 2.0,
    }
}

pub(crate) fn line_height(style: &ElementTextStyle) -> f64 {
    (style.size * style.line_height).max(style.size)
}

fn natural_text_width(fonts: &mut FontSet, element: &Element) -> f64 {
    let padding = control_padding(&element.kind);
    text_of(element)
        .unwrap_or_default()
        .split('\n')
        .map(|line| line_width(fonts, line, &element.style.text_style))
        .fold(0.0, f64::max)
        .ceil()
        + 1.0
        + padding.left
        + padding.right
        + border_width(element) * 2.0
}
fn text_height(fonts: &mut FontSet, element: &Element, width: f64) -> f64 {
    let padding = control_padding(&element.kind);
    let content = text_box(
        element,
        inset(
            Rect {
                x: 0.0,
                y: 0.0,
                width,
                height: 1.0,
            },
            border_width(element),
        ),
    )
    .0;
    let lines = wrap_text(
        fonts,
        text_of(element).unwrap_or_default(),
        &element.style.text_style,
        content.width,
        wraps(&element.kind),
    );
    (lines.len().max(1) as f64 * line_height(&element.style.text_style)).ceil()
        + 1.0
        + padding.top
        + padding.bottom
        + border_width(element) * 2.0
}

/// Hug text takes its natural width up to the available container space, then wraps before
/// measuring automatic height. Fixed widths keep their editing intent.
pub(crate) fn resolve_auto_size(
    fonts: &mut FontSet,
    element: &Element,
    mut at: Rect,
    available_width: f64,
) -> Rect {
    let sizing = &element.style.sizing;
    let fit_width = sizing.width == crate::elements::SizingMode::Fit;
    let fit_height = sizing.height == crate::elements::SizingMode::Fit;
    if text_of(element).is_some() {
        if at.width <= 0.0 || fit_width {
            at.width = natural_text_width(fonts, element);
        }
        at.width = sizing.constrain(at.width, at.height).0;
        if fit_width {
            at.width = at.width.min(available_width.max(0.0));
        }
        if at.height <= 0.0 || fit_height {
            at.height = text_height(fonts, element, at.width);
        }
    } else if fit_width || fit_height {
        // A Hug parent has no definite width yet: use its containing space rather than its
        // previous stored width, so edits can both grow and shrink the intrinsic content.
        let offered = if fit_width {
            Rect {
                width: sizing
                    .max_width
                    .map_or(available_width, |max| max.min(available_width)),
                ..at
            }
        } else {
            at
        };
        let inner = child_content_rect(element, offered);
        let children: Vec<_> = element
            .children
            .iter()
            .filter(|c| c.visible)
            .map(|c| {
                let offset = if element.layout.is_free() {
                    c.rect.x
                } else {
                    0.0
                };
                resolve_auto_size(fonts, c, c.rect, (inner.width - offset).max(0.0))
            })
            .collect();
        let (width, height) = match &element.layout {
            ElementLayout::Free => (
                children.iter().map(|r| r.x + r.width).fold(0.0, f64::max),
                children.iter().map(|r| r.y + r.height).fold(0.0, f64::max),
            ),
            ElementLayout::Stack { direction, gap, .. } => {
                let padding = effective_padding(element);
                let sides = padding.left + padding.right;
                let ends = padding.top + padding.bottom;
                let gaps = gap * children.len().saturating_sub(1) as f64;
                if *direction == crate::elements::StackDirection::Row {
                    (
                        children.iter().map(|r| r.width).sum::<f64>() + gaps + sides,
                        children.iter().map(|r| r.height).fold(0.0, f64::max) + ends,
                    )
                } else {
                    (
                        children.iter().map(|r| r.width).fold(0.0, f64::max) + sides,
                        children.iter().map(|r| r.height).sum::<f64>() + gaps + ends,
                    )
                }
            }
            ElementLayout::Grid {
                columns,
                gap,
                padding: _,
            } => {
                let padding = effective_padding(element);
                let columns = *columns as usize;
                (
                    children.iter().map(|r| r.width).fold(0.0, f64::max) * columns as f64
                        + gap * columns.saturating_sub(1) as f64
                        + padding.left
                        + padding.right,
                    children
                        .chunks(columns)
                        .map(|row| row.iter().map(|r| r.height).fold(0.0, f64::max))
                        .sum::<f64>()
                        + gap * children.len().div_ceil(columns).saturating_sub(1) as f64
                        + padding.top
                        + padding.bottom,
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
    if fit_width && text_of(element).is_some() {
        at.width = at.width.min(available_width.max(0.0));
    }
    at
}

/// Persist Hug dimensions in the same transaction as content, typography and layout changes.
/// Resolve shared styles and linked sizes before measuring, just as rendering does. Only Hug
/// axes are written; fixed dimensions and every layer's position remain editing intent.
/// Returns whether measurement ran, requiring validation of the resulting geometry.
pub(crate) fn sync_fit_rects(project: &mut Project) -> bool {
    fn has_fit(elements: &[Element]) -> bool {
        elements.iter().any(|element| {
            let fits = |style: &crate::elements::ElementStyle| {
                style.sizing.width == crate::elements::SizingMode::Fit
                    || style.sizing.height == crate::elements::SizingMode::Fit
            };
            fits(&element.style)
                || element
                    .overrides
                    .values()
                    .any(|over| over.style.as_ref().is_some_and(fits))
                || has_fit(&element.children)
        })
    }

    // Fixed-only projects cannot change here. Avoid resolving/cloning every size and loading
    // font resources on ordinary moves, inserts and unrelated project edits.
    if !project.pages.iter().any(|page| has_fit(&page.elements))
        && !project
            .linked
            .values()
            .any(|definition| has_fit(std::slice::from_ref(&definition.master)))
    {
        return false;
    }
    fn measure_tree(fonts: &mut FontSet, elements: &mut [Element], parent: Rect, flow: bool) {
        for element in elements {
            let available = (parent.width - if flow { 0.0 } else { element.rect.x }).max(0.0);
            let mut rect = if flow {
                resolve_auto_size(fonts, element, element.rect, available)
            } else {
                sized_in(fonts, element, parent)
            };
            let inner = child_content_rect(element, rect);
            measure_tree(
                fonts,
                &mut element.children,
                inner,
                !element.layout.is_free(),
            );
            // Children may have grown taller after wrapping; remeasure Hug parents afterwards.
            rect = resolve_auto_size(fonts, element, rect, available);
            if element.style.sizing.width == crate::elements::SizingMode::Fit {
                element.rect.width = rect.width;
            }
            if element.style.sizing.height == crate::elements::SizingMode::Fit {
                element.rect.height = rect.height;
            }
        }
    }

    fn write_tree(stored: &mut [Element], measured: &[Element], size_id: Option<uuid::Uuid>) {
        for (element, measured) in stored.iter_mut().zip(measured) {
            let fit_width = measured.style.sizing.width == crate::elements::SizingMode::Fit;
            let fit_height = measured.style.sizing.height == crate::elements::SizingMode::Fit;
            if fit_width || fit_height {
                let mut rect = size_id
                    .and_then(|id| element.overrides.get(&id).and_then(|over| over.rect))
                    .unwrap_or(element.rect);
                let previous = rect;
                if fit_width {
                    rect.width = measured.rect.width;
                }
                if fit_height {
                    rect.height = measured.rect.height;
                }
                if rect != previous {
                    if let Some(id) = size_id {
                        element.overrides.entry(id).or_default().rect = Some(rect);
                    } else {
                        element.rect = rect;
                    }
                }
            }
            if element.link.is_none() {
                write_tree(&mut element.children, &measured.children, size_id);
            }
        }
    }

    thread_local! {
        static FONTS: std::cell::RefCell<FontSet> = std::cell::RefCell::new(FontSet::new());
    }
    FONTS.with_borrow_mut(|fonts| {
        let definitions: Vec<_> = project
            .linked
            .iter()
            .map(|(id, definition)| {
                let mut elements = crate::elements::resolve_elements(
                    project,
                    std::slice::from_ref(&definition.master),
                );
                crate::elements::resolve_shared_styles(project, &mut elements);
                measure_tree(
                    fonts,
                    &mut elements,
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: f64::INFINITY,
                        height: f64::INFINITY,
                    },
                    false,
                );
                (*id, elements)
            })
            .collect();
        for (id, elements) in definitions {
            if let Some(definition) = project.linked.get_mut(&id) {
                write_tree(
                    std::slice::from_mut(&mut definition.master),
                    &elements,
                    None,
                );
            }
        }
        for index in 0..project.pages.len() {
            let page = &project.pages[index];
            let mut base = crate::elements::resolve_elements(project, &page.elements);
            crate::elements::resolve_shared_styles(project, &mut base);
            measure_tree(
                fonts,
                &mut base,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: page.base_size().width as f64,
                    height: page.base_size().height as f64,
                },
                false,
            );
            let sizes: Vec<_> = page
                .sizes
                .iter()
                .map(|size| {
                    let mut elements = crate::elements::resolve_for_size(project, page, size.id);
                    measure_tree(
                        fonts,
                        &mut elements,
                        Rect {
                            x: 0.0,
                            y: 0.0,
                            width: size.width as f64,
                            height: size.height as f64,
                        },
                        false,
                    );
                    (size.id, elements)
                })
                .collect();
            let stored = &mut project.pages[index].elements;
            write_tree(stored, &base, None);
            for (id, elements) in sizes {
                write_tree(stored, &elements, Some(id));
            }
        }
    });
    true
}

/// Where a text-bearing element's first line box starts, and how its lines align.
pub(crate) fn text_box(element: &Element, content: Rect) -> (Rect, TextAlign) {
    let style = &element.style.text_style;
    let padding = control_padding(&element.kind);
    let align = style.align;
    let height = line_height(style);
    let top = match element.kind {
        ElementKind::Text | ElementKind::Code { .. } => content.y,
        ElementKind::Textarea => content.y + padding.top,
        _ => content.y + ((content.height - height) / 2.0).max(0.0),
    };
    (
        Rect {
            x: content.x + padding.left,
            y: top,
            width: (content.width - padding.left - padding.right).max(1.0),
            height: content.height,
        },
        align,
    )
}

pub(crate) fn line_width(fonts: &mut FontSet, line: &str, style: &ElementTextStyle) -> f64 {
    let run = Run::of(style);
    line.chars()
        .map(|character| fonts.advance(run, character) + style.letter_spacing)
        .sum()
}

/// Greedy word wrapping with long words broken up, matching the canvas's `pre-wrap` text.
pub(crate) fn wrap_text(
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

/// Family names the editor can offer: the bundled face first, then installed families.
/// Generic CSS families resolve through `resolve_font` and are not repeated here.
pub fn font_families() -> Vec<String> {
    use std::sync::{Mutex, OnceLock};
    static FAMILIES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let families = FAMILIES.get_or_init(|| {
        let mut database = Database::new();
        database.load_system_fonts();
        let mut names: Vec<String> = database
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .filter(|name| {
                let name = name.trim();
                !name.is_empty() && !name.eq_ignore_ascii_case("Archivo")
            })
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names.dedup_by(|first, second| first.eq_ignore_ascii_case(second));
        let mut result = vec!["Archivo".to_string()];
        result.extend(names);
        Mutex::new(result)
    });
    families
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
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
        .get_or_init(|| Mutex::new((system_fonts(), BTreeMap::new())))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A missing serif or monospace family keeps its kind through the installed generic face,
    // the same kind the web export names as its fallback.
    let inferred = crate::element_style::generic_fallback(family);
    let candidates = family
        .split(',')
        .chain((inferred != "sans-serif").then_some(inferred));
    for requested in candidates {
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

/// Installed fonts with the CSS generic families pointed at real installed faces. fontdb
/// defaults to Windows names such as Times New Roman; where those are missing it can land on
/// a symbol font that draws Latin letters as Greek. A generic family with no installed match
/// resolves to nothing, so the bundled Archivo stands in.
pub(crate) fn system_fonts() -> Database {
    let mut database = Database::new();
    database.load_system_fonts();
    let installed = |name: &str| {
        database.faces().any(|face| {
            face.families
                .iter()
                .any(|(family, _)| family.eq_ignore_ascii_case(name))
        })
    };
    let pick = |candidates: &[&str]| {
        candidates
            .iter()
            .find(|name| installed(name))
            .map_or("Desegnos no generic font", |name| *name)
            .to_owned()
    };
    let sans = pick(&[
        "Arial",
        "Helvetica",
        "Liberation Sans",
        "Arimo",
        "DejaVu Sans",
        "Noto Sans",
        "Nimbus Sans",
        "Cantarell",
        "Roboto",
        "Ubuntu",
    ]);
    let serif = pick(&[
        "Times New Roman",
        "Times",
        "Liberation Serif",
        "Tinos",
        "DejaVu Serif",
        "Noto Serif",
        "Nimbus Roman",
        "Georgia",
    ]);
    let mono = pick(&[
        "Courier New",
        "Menlo",
        "Consolas",
        "Liberation Mono",
        "Cousine",
        "DejaVu Sans Mono",
        "Noto Sans Mono",
        "Nimbus Mono PS",
        "Ubuntu Mono",
    ]);
    database.set_sans_serif_family(sans.clone());
    database.set_serif_family(serif);
    database.set_monospace_family(mono);
    database.set_cursive_family(sans.clone());
    database.set_fantasy_family(sans);
    database
}

fn bundled_font(fell_back: bool) -> ResolvedFont {
    ResolvedFont {
        id: BUNDLED_FONT_ID.into(),
        bytes: std::sync::Arc::new(ARCHIVO.to_vec()),
        index: 0,
        bundled: true,
        fell_back,
        italic: false,
    }
}

/// A standalone SFNT copy of one face from a TrueType/OpenType collection. A file that is not a
/// collection is already standalone. Browsers load only a collection's first face, so an export
/// needs the selected member's table directory rewritten to point at its own tables.
pub fn standalone_face(bytes: &[u8], index: u32) -> Option<Vec<u8>> {
    if bytes.get(..4)? != b"ttcf" {
        return Some(bytes.to_vec());
    }
    let read_u32 = |at| {
        Some(u32::from_be_bytes(
            bytes
                .get(at..at + 4)?
                .try_into()
                .expect("four-byte font table entry"),
        ))
    };
    let offset = read_u32(12 + index as usize * 4)? as usize;
    let count = u16::from_be_bytes(
        bytes
            .get(offset + 4..offset + 6)?
            .try_into()
            .expect("two-byte font table count"),
    ) as usize;
    let header_len = 12 + count * 16;
    let mut result = bytes.get(offset..offset + header_len)?.to_vec();
    for table in 0..count {
        let at = 12 + table * 16 + 8;
        let original = read_u32(offset + at)?;
        result[at..at + 4].copy_from_slice(&original.checked_add(header_len as u32)?.to_be_bytes());
    }
    result.extend_from_slice(bytes);
    Some(result)
}

struct FaceData {
    bytes: Vec<u8>,
    index: u32,
}

/// One text style's font choice: everything about a run of glyphs that does not change per
/// character.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Run<'a> {
    family: &'a str,
    weight: u16,
    italic: bool,
    size: f64,
}

impl<'a> Run<'a> {
    pub(crate) fn of(style: &'a ElementTextStyle) -> Self {
        Self {
            family: &style.font_family,
            weight: style.weight,
            italic: style.italic,
            size: style.size,
        }
    }
}

/// A CSS generic family keyword as the face fontdb resolves it to.
pub(crate) fn generic_family(name: &str) -> Family<'_> {
    match crate::element_style::generic_keyword(name) {
        Some("sans-serif") => Family::SansSerif,
        Some("serif") => Family::Serif,
        Some("monospace") => Family::Monospace,
        Some("cursive") => Family::Cursive,
        Some("fantasy") => Family::Fantasy,
        _ => Family::Name(name),
    }
}

pub(crate) struct FontSet {
    faces: Vec<FaceData>,
    database: Option<Database>,
    /// The face each `font-family` value resolves to, in request order.
    families: BTreeMap<(String, u16, bool), usize>,
    resolved: BTreeMap<String, usize>,
    /// The face that covers a character the primary face does not.
    fallbacks: BTreeMap<(usize, char), usize>,
}

impl FontSet {
    pub(crate) fn new() -> Self {
        Self {
            faces: vec![FaceData {
                bytes: ARCHIVO.to_vec(),
                index: 0,
            }],
            database: None,
            families: BTreeMap::new(),
            resolved: BTreeMap::from([(BUNDLED_FONT_ID.into(), 0)]),
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
    pub(crate) fn primary(&mut self, family: &str, weight: u16, italic: bool) -> usize {
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
            let found = database
                .with_face_data(id, |bytes, index| {
                    let candidate = FaceData {
                        bytes: bytes.to_vec(),
                        index,
                    };
                    covers(&candidate, character).then_some(candidate)
                })
                .flatten();
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
    pub(crate) fn line_metrics(&mut self, run: Run<'_>) -> (f64, f64) {
        let primary = self.primary(run.family, run.weight, run.italic);
        let data = &self.faces[primary];
        let face = weighted_face(data, run.weight);
        let units = f64::from(face.units_per_em().max(1));
        let ascent = f64::from(face.ascender()) / units * run.size;
        let descent = f64::from(face.descender()) / units * run.size;
        (ascent, (ascent - descent).max(run.size * 0.5))
    }

    pub(crate) fn advance(&mut self, run: Run<'_>, character: char) -> f64 {
        let primary = self.primary(run.family, run.weight, run.italic);
        let face_index = self.face_for(character, primary);
        let data = &self.faces[face_index];
        let face = weighted_face(data, run.weight);
        let units = f64::from(face.units_per_em().max(1));
        let advance = face
            .glyph_index(character)
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .map_or(units / 2.0, f64::from);
        advance / units * run.size
    }

    pub(crate) fn glyph(
        &mut self,
        run: Run<'_>,
        character: char,
        pen: f64,
        baseline: f64,
    ) -> (f64, Option<Path>) {
        let primary = self.primary(run.family, run.weight, run.italic);
        let face_index = self.face_for(character, primary);
        let data = &self.faces[face_index];
        let face = weighted_face(data, run.weight);
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

fn weighted_face(data: &FaceData, weight: u16) -> Face<'_> {
    let mut face = Face::parse(&data.bytes, data.index)
        .unwrap_or_else(|_| Face::parse(ARCHIVO, 0).expect("bundled Archivo face"));
    let tag = ttf_parser::Tag::from_bytes(b"wght");
    // Static faces use the weight selected by fontdb and have no variation axis.
    if face
        .variation_axes()
        .into_iter()
        .any(|axis| axis.tag == tag)
        && face.set_variation(tag, f32::from(weight)).is_none()
    {
        face = Face::parse(ARCHIVO, 0).expect("bundled Archivo face");
        face.set_variation(tag, f32::from(weight))
            .expect("bundled Archivo weight axis");
    }
    face
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
/// Uses exactly the same font advances and word wrapping as canvas fit-text.
pub(crate) struct ResponsiveTextMeasure(FontSet, crate::model::ProjectStyles, f64);
impl ResponsiveTextMeasure {
    pub(crate) fn new(styles: crate::model::ProjectStyles) -> Self {
        Self(FontSet::new(), styles, f64::INFINITY)
    }
    /// The width of the size being adapted; display text never grows past a share of it.
    pub(crate) fn set_viewport(&mut self, width: f64) {
        self.2 = width;
    }
    pub(crate) fn viewport(&self) -> f64 {
        self.2
    }
    pub(crate) fn resolve_style(
        &self,
        base: &crate::elements::ElementStyle,
    ) -> crate::elements::ElementStyle {
        let mut style = base.clone();
        self.1.resolve(&mut style);
        style
    }
    pub(crate) fn height(
        &mut self,
        element: &Element,
        style: &crate::elements::ElementStyle,
        width: f64,
    ) -> f64 {
        let mut measured = element.clone();
        measured.style = style.clone();
        text_height(&mut self.0, &measured, width)
    }
    /// The width the text needs on one line at this style, padding included.
    pub(crate) fn natural_width(
        &mut self,
        element: &Element,
        style: &crate::elements::ElementStyle,
    ) -> f64 {
        let mut measured = element.clone();
        measured.style = style.clone();
        natural_text_width(&mut self.0, &measured)
    }
}
