//! Pure, deterministic website rendering. Tree order is paint order; CSS order is ID order.
use super::*;
use crate::{
    ElementKind, ElementLayout, StackAlign, StackDirection, StackJustify, TextAlign, resolve_font,
    standalone_face,
};

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn css_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\a ")
            .replace('\r', "\\d ")
    )
}

fn font_family(value: &str) -> String {
    value
        .split(',')
        .map(|family| {
            let family = family.trim().trim_matches(['\'', '"']);
            match family.to_ascii_lowercase().as_str() {
                "serif" | "sans-serif" | "monospace" | "system-ui" | "ui-serif"
                | "ui-sans-serif" | "ui-monospace" | "cursive" | "fantasy" | "emoji" | "math"
                | "fangsong" => family.to_owned(),
                _ => css_string(family),
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The CSS family list for a face the browser already has: the requested names, ending in a
/// generic family so a host without them still picks a sensible face.
fn fallback_family_list(family: &str) -> String {
    let list = font_family(family);
    if ends_with_generic(&list) {
        list
    } else {
        format!("{list},{}", generic_fallback(family))
    }
}

/// The generic family a font list falls back to when nothing in it is installed. Serif and
/// monospace names keep their look; everything else ends as the browser's sans-serif.
pub(crate) fn generic_fallback(family: &str) -> &'static str {
    let lower = family.to_ascii_lowercase();
    let without_sans = lower
        .replace("sans-serif", "")
        .replace("sans serif", "")
        .replace("sansserif", "");
    let hints = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    if hints(&["mono", "courier", "consolas", "menlo"]) {
        "monospace"
    } else if hints(&["times", "georgia", "garamond", "baskerville", "roman"])
        || without_sans.contains("serif")
    {
        "serif"
    } else {
        "sans-serif"
    }
}

/// Whether a formatted font list already ends in a concrete generic family.
fn ends_with_generic(list: &str) -> bool {
    list.split(',')
        .next_back()
        .map(|last| {
            matches!(
                last.trim().trim_matches(['\'', '"']).to_ascii_lowercase().as_str(),
                "serif"
                    | "sans-serif"
                    | "monospace"
                    | "cursive"
                    | "fantasy"
                    | "emoji"
                    | "math"
                    | "fangsong"
            )
        })
        .unwrap_or(false)
}

/// The CSS family name an embedded face publishes. It never matches an installed family, so the
/// browser shows the same bytes the canvas and PNG renderer draw.
fn embedded_family(id: &str) -> String {
    if id == "archivo" {
        "Desegnos Archivo".into()
    } else {
        let short: String = id.chars().take(16).collect();
        format!("Desegnos Face {short}")
    }
}

/// One face shipped beside the export, with every weight and style the stylesheet declares for it.
struct EmbeddedFace {
    family: String,
    asset: String,
    bytes: Vec<u8>,
    styles: BTreeSet<(u16, bool)>,
}

/// The font lists exported CSS uses, resolved exactly as the canvas resolves them. A bundled or
/// missing family travels as an asset; an installed family keeps its name.
#[derive(Default)]
struct Fonts {
    families: BTreeMap<(String, u16, bool), String>,
    faces: BTreeMap<String, EmbeddedFace>,
}

impl Fonts {
    fn family(&mut self, family: &str, weight: u16, italic: bool) -> String {
        let key = (family.to_owned(), weight, italic);
        if let Some(value) = self.families.get(&key) {
            return value.clone();
        }
        let resolved = resolve_font(family, weight, italic);
        let value = if resolved.bundled || resolved.fell_back {
            let face = self
                .faces
                .entry(resolved.id.clone())
                .or_insert_with(|| EmbeddedFace {
                    family: embedded_family(&resolved.id),
                    asset: format!("assets/fonts/{}.ttf", resolved.id),
                    bytes: standalone_face(&resolved.bytes, resolved.index)
                        .unwrap_or_else(|| resolved.bytes.as_ref().clone()),
                    styles: BTreeSet::new(),
                });
            face.styles.insert((weight, italic));
            format!("{},{}", css_string(&face.family), generic_fallback(family))
        } else {
            fallback_family_list(family)
        };
        self.families.insert(key, value.clone());
        value
    }

    /// `@font-face` rules for every embedded face, in face then weight and style order.
    fn css(&self) -> String {
        let mut css = String::new();
        for face in self.faces.values() {
            for (weight, italic) in &face.styles {
                css.push_str(&format!(
                    "@font-face{{font-family:{};src:url({});font-weight:{weight};font-style:{};font-display:swap;}}\n",
                    css_string(&face.family),
                    css_string(&face.asset),
                    if *italic { "italic" } else { "normal" }
                ));
            }
        }
        css
    }
}

fn length(value: f64) -> String {
    format!("calc({value} * var(--desegnos-unit))")
}

/// A stack or grid content inset: the four sides when the layer names them, else the layout's
/// all-sides value that older projects and the direct renderer carry. The flag reports an inset
/// that paints nothing, so the shared reset rule can carry it instead.
fn padding_css(element: &Element) -> (String, bool) {
    match &element.style.padding {
        Some(padding) => (
            format!(
                "{} {} {} {}",
                length(padding.top),
                length(padding.right),
                length(padding.bottom),
                length(padding.left)
            ),
            [padding.top, padding.right, padding.bottom, padding.left]
                .iter()
                .all(|side| *side == 0.0),
        ),
        None => {
            let base = match &element.layout {
                ElementLayout::Stack { padding, .. } | ElementLayout::Grid { padding, .. } => {
                    *padding
                }
                ElementLayout::Free => 0.0,
            };
            (length(base), base == 0.0)
        }
    }
}
fn class(id: Uuid) -> String {
    format!("d-{}", id.simple())
}

fn safe_url(value: Option<&str>) -> String {
    let value = value.unwrap_or_default().trim();
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || (!value.contains(':') && !value.starts_with("//") && !value.contains(['\\', '\0']))
    {
        escape(value)
    } else {
        String::new()
    }
}

/// The address a hyperlink renders: a project boundary becomes its exported file, and an
/// external address stays as it was typed.
fn link_href(element: &Element, plan: &ExportPlan) -> String {
    let target = element.url.as_deref().and_then(|url| {
        plan.pages
            .iter()
            .find(|page| page.path == url && !page.file.is_empty())
    });
    target
        .map(|page| escape(&page.file))
        .unwrap_or_else(|| safe_url(element.url.as_deref()))
}

/// The HTML tag a layer renders as. A link target on a text, box or button turns that layer into
/// a real anchor.
fn tag_name(element: &Element) -> &'static str {
    use ElementKind::*;
    let hyperlink =
        element.url.is_some() && matches!(element.kind, Box | Text | Button | Link);
    match &element.kind {
        Box if hyperlink => "a",
        Text if hyperlink => "a",
        Button if hyperlink => "a",
        Box | Group | Ellipse | Line => "div",
        Text => "div",
        Image => "img",
        Button => "button",
        Link => "a",
        Input { .. } | Checkbox | Radio => "input",
        Textarea => "textarea",
        Select { .. } => "select",
        Video => "video",
        Audio => "audio",
        Embed => "iframe",
        Code { .. } => "pre",
    }
}

fn markup(
    element: &Element,
    plan: &ExportPlan,
    markers: &BTreeMap<Uuid, Vec<Uuid>>,
    out: &mut String,
) {
    use ElementKind::*;
    if matches!(element.kind, Checkbox | Radio) {
        let input_type = if matches!(element.kind, Checkbox) {
            "checkbox"
        } else {
            "radio"
        };
        out.push_str(&format!("<label class=\"{}\"{}><input type=\"{input_type}\" data-desegnos-id=\"{}\" aria-label=\"{}\">{}</label>\n", class(element.id), pointer_attribute(element.id, markers), element.id, escape(element.name.as_deref().unwrap_or(&element.text)), escape(&element.text)));
        return;
    }
    let tag = tag_name(element);
    out.push_str(&format!(
        "<{tag} class=\"{}\" data-desegnos-id=\"{}\"",
        class(element.id),
        element.id
    ));
    out.push_str(&pointer_attribute(element.id, markers));
    if tag == "a" {
        out.push_str(&format!(" href=\"{}\"", link_href(element, plan)));
    }
    let label = escape(
        element
            .name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(&element.text),
    );
    match &element.kind {
        Image => {
            let path = element
                .image
                .and_then(|id| plan.assets.iter().find(|asset| asset.picture_id == id));
            out.push_str(&format!(
                " src=\"{}\" alt=\"{label}\"",
                path.map(|asset| escape(&asset.path)).unwrap_or_default()
            ));
        }
        Button if tag != "a" => out.push_str(" type=\"button\""),
        Link => {}
        Input { input_type } => {
            let input = serde_json::to_value(input_type).unwrap_or_default();
            out.push_str(&format!(
                " type=\"{}\" placeholder=\"{}\" aria-label=\"{label}\"",
                input.as_str().unwrap_or("text"),
                escape(&element.text)
            ));
        }
        Checkbox | Radio => out.push_str(&format!(
            " type=\"{}\" aria-label=\"{label}\"",
            if matches!(element.kind, Checkbox) {
                "checkbox"
            } else {
                "radio"
            }
        )),
        Video | Audio => out.push_str(&format!(
            " controls src=\"{}\"",
            safe_url(element.url.as_deref())
        )),
        Embed => out.push_str(&format!(
            " sandbox=\"\" title=\"{label}\" src=\"{}\"",
            safe_url(element.url.as_deref())
        )),
        Textarea | Select { .. } => out.push_str(&format!(" aria-label=\"{label}\"")),
        _ => {}
    }
    out.push('>');
    if matches!(element.kind, Image | Input { .. } | Checkbox | Radio) {
        out.push('\n');
        return;
    }
    match &element.kind {
        Select { options } => {
            for option in options {
                out.push_str(&format!(
                    "<option{}>{}</option>",
                    if option == &element.text {
                        " selected"
                    } else {
                        ""
                    },
                    escape(option)
                ));
            }
        }
        Line => {
            let width = element.rect.width.max(1.0);
            let height = element.rect.height.max(1.0);
            let colour = escape(
                element
                    .style
                    .stroke
                    .color
                    .as_deref()
                    .unwrap_or(&element.style.text_style.color),
            );
            out.push_str(&format!("<svg width=\"100%\" height=\"100%\" viewBox=\"0 0 {width} {height}\" preserveAspectRatio=\"none\" aria-hidden=\"true\" style=\"display:block;overflow:visible\"><line x1=\"0\" y1=\"{}\" x2=\"{width}\" y2=\"{}\" stroke=\"{colour}\" stroke-width=\"{}\"/></svg>", height / 2.0, height / 2.0, element.style.stroke.width.max(1.0)));
        }
        Code { language, source } => out.push_str(&format!(
            "<code data-language=\"{}\">{}</code>",
            escape(language),
            escape(source)
        )),
        _ => out.push_str(&escape(&element.text)),
    }
    for child in &element.children {
        markup(child, plan, markers, out);
    }
    out.push_str(&format!("</{tag}>\n"));
}

/// One CSS declaration. `reset` marks a value that leaves the layer's default rendering in
/// place: the common rule already sets it, so a size restates it only to undo a base size.
struct Declarations {
    entries: Vec<(String, String, bool)>,
}

impl Declarations {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn set(&mut self, property: impl Into<String>, value: impl Into<String>) {
        self.entries.push((property.into(), value.into(), false));
    }

    fn reset(&mut self, property: impl Into<String>, value: impl Into<String>) {
        self.entries.push((property.into(), value.into(), true));
    }

    /// The last value of every property, in declaration order.
    fn effective(&self) -> Vec<(&str, &str, bool)> {
        let mut out: Vec<(&str, &str, bool)> = Vec::new();
        let mut slots: BTreeMap<&str, usize> = BTreeMap::new();
        for (property, value, reset) in &self.entries {
            let entry = (property.as_str(), value.as_str(), *reset);
            match slots.get(property.as_str()) {
                Some(slot) => out[*slot] = entry,
                None => {
                    slots.insert(property.as_str(), out.len());
                    out.push(entry);
                }
            }
        }
        out
    }

    /// The declaration block of the base size, without what the common rule already sets.
    fn block(&self) -> String {
        let mut css = String::new();
        for (property, value, reset) in self.effective() {
            if !reset {
                css.push_str(&format!("{property}:{value};"));
            }
        }
        css
    }

    /// Only what this size changes, restating a base value when the layer returns to its default.
    fn diff(&self, base: &Declarations) -> String {
        let mut base_values: BTreeMap<&str, &str> = BTreeMap::new();
        for (property, value, reset) in base.effective() {
            if !reset {
                base_values.insert(property, value);
            }
        }
        let mut css = String::new();
        for (property, value, reset) in self.effective() {
            let changed = match base_values.get(property) {
                Some(base) => *base != value,
                None => !reset,
            };
            if changed {
                css.push_str(&format!("{property}:{value};"));
            }
        }
        css
    }
}

/// Whether a colour paints anything; a missing, transparent or zero-alpha colour draws nothing.
fn paints(colour: Option<&str>) -> bool {
    colour
        .and_then(crate::Rgba::parse)
        .is_some_and(|colour| colour.a > 0.0)
}

/// Whether a layer paints its own text; only then do its font and colour declarations matter.
fn renders_text(element: &Element) -> bool {
    match &element.kind {
        // The canvas paints these kinds with the layer's text style, and an empty string still
        // reserves the line box, so an edit never has to rewrite the stylesheet.
        ElementKind::Text
        | ElementKind::Button
        | ElementKind::Link
        | ElementKind::Input { .. }
        | ElementKind::Textarea
        | ElementKind::Select { .. }
        | ElementKind::Checkbox
        | ElementKind::Radio
        | ElementKind::Code { .. } => true,
        // A box, group or ellipse only paints the text it carries.
        ElementKind::Box | ElementKind::Group | ElementKind::Ellipse => !element.text.is_empty(),
        ElementKind::Image
        | ElementKind::Line
        | ElementKind::Video
        | ElementKind::Audio
        | ElementKind::Embed => false,
    }
}

/// Everything one layer sets, including the values the common rule shares.
fn declarations(element: &Element, parent: &ElementLayout, fonts: &mut Fonts) -> Declarations {
    let defaults = crate::ElementTextStyle::default();
    let free = parent.is_free();
    let rect = element.rect;
    let style = &element.style;
    let text = &style.text_style;
    let line = matches!(element.kind, ElementKind::Line);
    let group = matches!(element.kind, ElementKind::Group);
    let control = matches!(element.kind, ElementKind::Checkbox | ElementKind::Radio);
    // A button or link is a text control on the canvas: a centred row rather than a block.
    let text_control = matches!(element.kind, ElementKind::Button | ElementKind::Link);
    let display = match &element.layout {
        ElementLayout::Free if control || text_control => "flex",
        ElementLayout::Free => "block",
        ElementLayout::Stack { .. } => "flex",
        ElementLayout::Grid { .. } => "grid",
    };
    let width = if line {
        length(rect.width.max(1.0))
    } else if rect.width > 0.0 {
        length(rect.width)
    } else if matches!(
        element.kind,
        ElementKind::Text | ElementKind::Button | ElementKind::Link
    ) {
        "max-content".into()
    } else {
        "auto".into()
    };
    let height = if line {
        length(rect.height.max(1.0))
    } else if rect.height > 0.0 {
        length(rect.height)
    } else {
        "auto".into()
    };
    let mut css = Declarations::new();
    // The common rule carries the resets, so a layer states only what differs from them.
    if !element.visible {
        css.set("display", "none");
    } else if display == "block" && matches!(tag_name(element), "div" | "pre") {
        css.reset("display", display);
    } else {
        css.set("display", display);
    }
    if free {
        css.set("position", "absolute");
        css.set("left", length(rect.x));
        css.set("top", length(rect.y));
    } else {
        css.reset("position", "relative");
        css.reset("left", length(0.0));
        css.reset("top", length(0.0));
    }
    css.set("width", width);
    css.set("height", height);
    let sizing = &style.sizing;
    let main_fill = match parent {
        ElementLayout::Stack {
            direction: StackDirection::Row,
            ..
        } => sizing.width == crate::SizingMode::Fill,
        ElementLayout::Stack { .. } => sizing.height == crate::SizingMode::Fill,
        _ => false,
    };
    for (axis, mode, offset, min, max) in [
        (
            "width",
            sizing.width,
            rect.x,
            sizing.min_width,
            sizing.max_width,
        ),
        (
            "height",
            sizing.height,
            rect.y,
            sizing.min_height,
            sizing.max_height,
        ),
    ] {
        match mode {
            crate::SizingMode::Fixed => {}
            crate::SizingMode::Fit => css.set(
                axis,
                if axis == "width" { "max-content" } else { "auto" },
            ),
            crate::SizingMode::Fill => {
                let main_axis = matches!(
                    parent,
                    ElementLayout::Stack {
                        direction: StackDirection::Row,
                        ..
                    }
                ) == (axis == "width");
                let value = if free {
                    format!("calc(100% - {})", length(offset))
                } else if main_fill && main_axis {
                    "0px".into()
                } else {
                    "100%".into()
                };
                css.set(axis, value);
                css.set(format!("min-{axis}"), "0");
            }
        }
        if let Some(value) = min {
            css.set(format!("min-{axis}"), length(value));
        }
        if let Some(value) = max {
            css.set(format!("max-{axis}"), length(value));
        }
    }
    if main_fill {
        css.set("flex", "1 1 0");
    } else {
        // `flex-shrink:0` in the common rule is everything `flex:0 0 auto` sets.
        css.reset("flex", "0 0 auto");
    }
    match style.fill.as_deref().filter(|value| paints(Some(value))) {
        Some(fill) => css.set("background", fill),
        None => css.reset("background", "transparent"),
    }
    if !line && !group && style.stroke.width > 0.0 && paints(style.stroke.color.as_deref()) {
        css.set(
            "border",
            format!(
                "{} solid {}",
                length(style.stroke.width),
                style.stroke.color.as_deref().unwrap_or("transparent")
            ),
        );
    } else {
        css.reset("border", "0 solid transparent");
    }
    if matches!(element.kind, ElementKind::Ellipse) {
        css.set("border-radius", "50%");
    } else if style.radius != 0.0 {
        css.set("border-radius", length(style.radius));
    } else {
        css.reset("border-radius", length(0.0));
    }
    if style.opacity == 1.0 {
        css.reset("opacity", "1");
    } else {
        css.set("opacity", format!("{}", style.opacity));
    }
    let flipped = style.flip_x || style.flip_y;
    if (element.rotation % 360.0).abs() > 0.000_001 || flipped {
        let mut transform = format!("rotate({}deg)", element.rotation);
        if flipped {
            transform.push_str(&format!(
                " scale({},{})",
                if style.flip_x { -1 } else { 1 },
                if style.flip_y { -1 } else { 1 }
            ));
        }
        css.set("transform", transform);
    } else {
        css.reset("transform", "none");
    }
    if !line && !group && paints(style.shadow.color.as_deref()) {
        css.set(
            "box-shadow",
            format!(
                "{} {} {} {}",
                length(style.shadow.x),
                length(style.shadow.y),
                length(style.shadow.blur),
                style.shadow.color.as_deref().unwrap_or("transparent")
            ),
        );
    } else {
        css.reset("box-shadow", "none");
    }
    if renders_text(element) {
        // Every text property is stated so no layer inherits a neighbour's or parent's value;
        // one that matches the common rule is a reset instead.
        css.set(
            "font-family",
            fonts.family(&text.font_family, text.weight, text.italic),
        );
        if text.size != defaults.size {
            css.set("font-size", length(text.size));
        } else {
            css.reset("font-size", length(defaults.size));
        }
        if text.weight != defaults.weight {
            css.set("font-weight", format!("{}", text.weight));
        } else {
            css.reset("font-weight", format!("{}", defaults.weight));
        }
        if text.italic {
            css.set("font-style", "italic");
        } else {
            css.reset("font-style", "normal");
        }
        if text.line_height != defaults.line_height {
            css.set("line-height", format!("{}", text.line_height));
        } else {
            css.reset("line-height", format!("{}", defaults.line_height));
        }
        if text.letter_spacing != defaults.letter_spacing {
            css.set("letter-spacing", length(text.letter_spacing));
        } else {
            css.reset("letter-spacing", length(defaults.letter_spacing));
        }
        if text.align != defaults.align {
            css.set(
                "text-align",
                match text.align {
                    TextAlign::Left => "left",
                    TextAlign::Center => "center",
                    TextAlign::Right => "right",
                },
            );
        } else {
            css.reset("text-align", "left");
        }
        if text.color.eq_ignore_ascii_case(&defaults.color) {
            css.reset("color", defaults.color.clone());
        } else {
            css.set("color", text.color.clone());
        }
    }
    // `object-fit` only affects replaced content; text, boxes and shapes ignore it.
    if matches!(element.kind, ElementKind::Image | ElementKind::Video) {
        css.set("object-fit", "cover");
    }
    if matches!(element.kind, ElementKind::Link) || element.url.is_some() {
        css.set("text-decoration", "none");
        css.set("cursor", "pointer");
    }
    if control {
        css.set("align-items", "center");
        css.set("gap", "0.4em");
    }
    if let Some(id) = style.references.fill {
        css.set("background", format!("var(--colour-{id})"));
    }
    if let Some(id) = style.references.stroke {
        css.set("border-color", format!("var(--colour-{id})"));
    }
    if let Some(id) = style.references.text_colour {
        css.set("color", format!("var(--colour-{id})"));
    }
    if let Some(id) = style.references.text {
        for property in [
            "font-family",
            "font-size",
            "font-weight",
            "line-height",
            "letter-spacing",
            "font-style",
        ] {
            css.set(property, format!("var(--text-{id}-{property})"));
        }
    }
    match &element.layout {
        ElementLayout::Free => {}
        ElementLayout::Stack {
            direction,
            gap,
            align,
            justify,
            wrap,
            ..
        } => {
            match direction {
                StackDirection::Row => css.reset("flex-direction", "row"),
                StackDirection::Column => css.set("flex-direction", "column"),
            }
            if *gap == 0.0 {
                css.reset("gap", length(0.0));
            } else {
                css.set("gap", length(*gap));
            }
            let (padding, clear) = padding_css(element);
            if clear {
                css.reset("padding", padding);
            } else {
                css.set("padding", padding);
            }
            css.set(
                "align-items",
                match align {
                    StackAlign::Start => "flex-start",
                    StackAlign::Center => "center",
                    StackAlign::End => "flex-end",
                    StackAlign::Stretch => "stretch",
                },
            );
            match justify {
                StackJustify::Start => css.reset("justify-content", "flex-start"),
                StackJustify::Center => css.set("justify-content", "center"),
                StackJustify::End => css.set("justify-content", "flex-end"),
                StackJustify::SpaceBetween => css.set("justify-content", "space-between"),
            }
            if *wrap {
                css.set("flex-wrap", "wrap");
            } else {
                css.reset("flex-wrap", "nowrap");
            }
        }
        ElementLayout::Grid { columns, gap, .. } => {
            css.set(
                "grid-template-columns",
                format!("repeat({columns},minmax(0,1fr))"),
            );
            css.reset("grid-auto-rows", "auto");
            if *gap == 0.0 {
                css.reset("gap", length(0.0));
            } else {
                css.set("gap", length(*gap));
            }
            let (padding, clear) = padding_css(element);
            if clear {
                css.reset("padding", padding);
            } else {
                css.set("padding", padding);
            }
        }
    }
    // The canvas centres a button's label on both axes and a link's label vertically, with its
    // horizontal position from text-align; the PNG renderer paints the same boxes.
    if matches!(element.kind, ElementKind::Button) {
        css.set("align-items", "center");
        css.set("justify-content", "center");
    } else if matches!(element.kind, ElementKind::Link) {
        css.set("align-items", "center");
        css.set(
            "justify-content",
            match text.align {
                TextAlign::Left => "flex-start",
                TextAlign::Center => "center",
                TextAlign::Right => "flex-end",
            },
        );
    }
    css
}

fn rules(
    elements: &[Element],
    parent: &ElementLayout,
    fonts: &mut Fonts,
    rules: &mut BTreeMap<Uuid, Declarations>,
) {
    for element in elements {
        rules.insert(element.id, declarations(element, parent, fonts));
        self::rules(&element.children, &element.layout, fonts, rules);
    }
}

/// The page shell's declarations for one size.
fn page_declarations(
    size: &crate::PageSize,
    elements: &[Element],
    page: &crate::Page,
) -> Declarations {
    let height = if matches!(size.kind, crate::BoundaryKind::Page) {
        elements
            .iter()
            .filter(|element| element.visible)
            .fold(size.height as f64, |height, element| {
                height.max(element.rect.y + element.rect.height)
            })
    } else {
        size.height as f64
    };
    let mut css = Declarations::new();
    css.set("--desegnos-unit", format!("calc(100cqw / {})", size.width));
    css.set("height", length(height));
    if matches!(size.kind, crate::BoundaryKind::Screen) {
        css.set("overflow", "auto");
    } else {
        css.reset("overflow", "visible");
    }
    css.set("background", page.background.clone());
    css.set(
        "color",
        if page.color_scheme() == "dark" {
            "#ffffff"
        } else {
            "#111111"
        },
    );
    css
}

fn pointer_attribute(id: Uuid, markers: &BTreeMap<Uuid, Vec<Uuid>>) -> String {
    markers
        .get(&id)
        .map(|ids| {
            format!(
                " data-desegnos-pointer=\"{}\"",
                ids.iter()
                    .map(Uuid::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .unwrap_or_default()
}

fn color_scheme<'a>(page: &crate::Page, ai: Option<&'a str>) -> &'a str {
    match ai {
        Some(scheme @ ("light" | "dark")) => scheme,
        _ => page.color_scheme(),
    }
}

/// Request every family a subtree paints text with.
fn collect_fonts(element: &Element, fonts: &mut Fonts) {
    let text = &element.style.text_style;
    fonts.family(&text.font_family, text.weight, text.italic);
    for child in &element.children {
        collect_fonts(child, fonts);
    }
}

pub(super) fn render(
    project: &Project,
    plan: &ExportPlan,
    behaviours: &BTreeMap<Uuid, ExportBehaviour>,
    schemes: &[ExportReplyPage],
    files: &mut BTreeMap<String, Vec<u8>>,
) {
    let mut markers: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
    for id in behaviours.keys() {
        if let Some(pointer) = project.pointers.iter().find(|pointer| pointer.id == *id) {
            let source = match pointer.from {
                PointerSource::Element { element_id, .. } => element_id,
                PointerSource::Boundary { page_id, .. } => page_id,
            };
            let target = match pointer.to {
                PointerTarget::Element { element_id, .. } => Some(element_id),
                PointerTarget::Boundary { page_id, .. } => Some(page_id),
                _ => None,
            };
            for endpoint in std::iter::once(source).chain(target) {
                let ids = markers.entry(endpoint).or_default();
                if !ids.contains(id) {
                    ids.push(*id);
                }
            }
        }
    }
    for planned in plan.pages.iter().filter(|page| page.output.needs_ai()) {
        let page = project
            .pages
            .iter()
            .find(|page| page.id == planned.page_id)
            .expect("planned page");
        let scheme = color_scheme(page, schemes.iter().find(|entry| entry.page_id == page.id).map(|entry| entry.color_scheme.as_str()));
        let mut sizes: Vec<_> = page.sizes.iter().collect();
        sizes.sort_by_key(|size| (size.width, size.id));
        let mut fonts = Fonts::default();
        for size in &sizes {
            for element in crate::resolve_for_size(project, page, size.id) {
                collect_fonts(&element, &mut fonts);
            }
        }
        for text in &project.styles.text {
            fonts.family(&text.font_family, text.weight, text.italic);
        }
        let mut html = String::new();
        for element in crate::resolve_for_size(project, page, page.base_size_id()) {
            markup(&element, plan, &markers, &mut html);
        }
        let mut css = format!(
            "html,body{{margin:0;padding:0;}}\n.desegnos-viewport{{container-type:inline-size;width:100%;max-width:{}px;margin:0 auto;}}\n.desegnos-page{{position:relative;width:100%;isolation:isolate;}}\n",
            sizes.last().map_or(1, |size| size.width)
        );
        let default_text = crate::ElementTextStyle::default();
        let default_family = fonts.family(
            &default_text.font_family,
            default_text.weight,
            default_text.italic,
        );
        css.push_str(&fonts.css());
        css.push_str(&format!(":root{{color-scheme:{scheme};"));
        for colour in &project.styles.colours { css.push_str(&format!("--colour-{}:{};", colour.id, colour.value)); }
        for text in &project.styles.text {
            let family = fonts.family(&text.font_family, text.weight, text.italic);
            css.push_str(&format!("--text-{}-font-family:{};--text-{}-font-size:{}px;--text-{}-font-weight:{};--text-{}-line-height:{};--text-{}-letter-spacing:{}px;--text-{}-font-style:{};", text.id, family, text.id, text.size, text.id, text.weight, text.id, text.line_height, text.id, text.letter_spacing, text.id, if text.italic { "italic" } else { "normal" }));
        }
        css.push_str("}\n");
        // Every layer's rule carries the `d-` class, so these resets apply once instead of in every
        // rule. The text resets also stop a layer from inheriting a parent's font or colour.
        css.push_str(&format!(
            "[class^=\"d-\"]{{box-sizing:border-box;margin:0;padding:0;min-width:0;flex-shrink:0;white-space:pre-wrap;overflow-wrap:break-word;position:relative;transform-origin:center;color:{};font-family:{};font-size:{};font-weight:{};font-style:normal;line-height:{};letter-spacing:{};text-align:left;}}\n",
            default_text.color,
            default_family,
            length(default_text.size),
            default_text.weight,
            default_text.line_height,
            length(default_text.letter_spacing)
        ));
        let mut base_page: Option<Declarations> = None;
        let mut base_rules: Option<BTreeMap<Uuid, Declarations>> = None;
        for (index, size) in sizes.iter().enumerate() {
            if index > 0 {
                css.push_str(&format!("@media (min-width:{}px){{\n", size.width));
            }
            let elements = crate::resolve_for_size(project, page, size.id);
            css.push_str(&format!("/* size:{} */\n", size.id));
            let page_css = page_declarations(size, &elements, page);
            let mut sorted = BTreeMap::new();
            rules(&elements, &ElementLayout::Free, &mut fonts, &mut sorted);
            match (&base_page, &base_rules) {
                (Some(base_page), Some(base_rules)) => {
                    // A size restates only the declarations that differ from the base size.
                    let declarations = page_css.diff(base_page);
                    if !declarations.is_empty() {
                        css.push_str(&format!(".desegnos-page{{{declarations}}}\n"));
                    }
                    for (id, declarations) in &sorted {
                        let block = match base_rules.get(id) {
                            Some(base) => declarations.diff(base),
                            None => declarations.block(),
                        };
                        if !block.is_empty() {
                            css.push_str(&format!(".{}{{{block}}}\n", class(*id)));
                        }
                    }
                }
                _ => {
                    css.push_str(&format!(".desegnos-page{{{}}}\n", page_css.block()));
                    for (id, declarations) in &sorted {
                        let block = declarations.block();
                        if !block.is_empty() {
                            css.push_str(&format!(".{}{{{block}}}\n", class(*id)));
                        }
                    }
                }
            }
            if index == 0 {
                base_page = Some(page_css);
                base_rules = Some(sorted);
            }
            if index > 0 {
                css.push_str("}\n");
            }
        }
        let stem = planned.file.trim_end_matches(".html");
        // An empty behaviour file would be a request to nothing, so neither it nor its tag ships.
        let behaviour_script = if behaviours.is_empty() {
            ""
        } else {
            "<script src=\"behaviours.js\" defer></script>"
        };
        let document = format!(
            "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"color-scheme\" content=\"{scheme}\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"{stem}.css\">{behaviour_script}</head><body><div class=\"desegnos-viewport\"><main class=\"desegnos-page\" data-desegnos-id=\"{}\"{}>\n{html}</main></div></body></html>\n",
            escape(&page.name),
            page.id,
            pointer_attribute(page.id, &markers)
        );
        files.insert(planned.file.clone(), document.into_bytes());
        files.insert(format!("{stem}.css"), css.into_bytes());
        for face in fonts.faces.values() {
            files.insert(face.asset.clone(), face.bytes.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Appearance, BoundaryKind, ElementOverride, ElementStroke, InputType, PageSize, PointerKind,
        Rect, Viewport, empty_element,
    };

    fn project() -> Project {
        Project::new(
            "Export".into(),
            Viewport {
                width: 800,
                height: 600,
            },
            Appearance::Light,
        )
        .unwrap()
    }

    fn element(kind: ElementKind) -> Element {
        empty_element(
            kind,
            Rect {
                x: 12.0,
                y: 24.0,
                width: 160.0,
                height: 40.0,
            },
        )
    }

    /// The stylesheet a direct export writes for a project.
    fn exported_css(project: &Project) -> String {
        let bundle = direct_export_bundle(project, &[]).unwrap();
        String::from_utf8(bundle.files["index.css"].clone()).unwrap()
    }

    /// The stylesheet the export wrote before per-element rules were compacted, in bytes: 1.67 MB
    /// for the dense page below. Rebuild it from the commit before the compaction with
    /// `git show <commit>:crates/desegnos-core/src/web_codegen.rs` if the dense page changes.
    const DENSE_CSS_BEFORE: usize = 1_678_979;

    /// A page of many plain layers with a size-specific position for every tenth layer, over
    /// three widths, matching the pages that made the old stylesheet 1.67 MB.
    fn dense_project(count: usize) -> Project {
        let mut project = project();
        let tablet = Uuid::new_v4();
        let phone = Uuid::new_v4();
        project.pages[0].sizes.push(PageSize {
            id: tablet,
            label: "Tablet".into(),
            width: 600,
            height: 900,
            kind: BoundaryKind::Screen,
            x: 0.0,
            y: 0.0,
        });
        project.pages[0].sizes.push(PageSize {
            id: phone,
            label: "Phone".into(),
            width: 390,
            height: 844,
            kind: BoundaryKind::Screen,
            x: 0.0,
            y: 0.0,
        });
        let mut elements = Vec::new();
        for index in 0..count {
            let kind = if index % 3 == 0 {
                ElementKind::Text
            } else {
                ElementKind::Box
            };
            let mut layer = element(kind);
            layer.rect = Rect {
                x: (index % 20) as f64 * 40.0,
                y: (index / 20) as f64 * 25.0,
                width: 160.0,
                height: 40.0,
            };
            if matches!(layer.kind, ElementKind::Text) {
                layer.text = format!("Layer {index}");
            }
            if index % 4 == 0 {
                layer.style.fill = Some("#eeeeee".into());
                layer.style.radius = 4.0;
            }
            if index % 7 == 0 {
                layer.style.stroke = ElementStroke {
                    color: Some("#cccccc".into()),
                    width: 1.0,
                };
            }
            if index % 11 == 0 {
                layer.style.shadow.color = Some("#00000033".into());
                layer.style.opacity = 0.9;
            }
            if index % 10 == 0 {
                for (id, width) in [(tablet, 120.0), (phone, 90.0)] {
                    layer.overrides.insert(
                        id,
                        ElementOverride {
                            rect: Some(Rect {
                                x: layer.rect.x,
                                y: layer.rect.y,
                                width,
                                height: layer.rect.height,
                            }),
                            ..Default::default()
                        },
                    );
                }
            }
            elements.push(layer);
        }
        project.pages[0].elements = elements;
        project
    }

    #[test]
    fn css_for_many_plain_layers_is_a_fraction_of_the_uncompacted_size() {
        let css = exported_css(&dense_project(640));
        println!("dense stylesheet: {} bytes", css.len());
        assert!(
            css.len() * 4 <= DENSE_CSS_BEFORE,
            "{} bytes, expected at least four times smaller than {DENSE_CSS_BEFORE}",
            css.len()
        );
    }

    #[test]
    fn a_size_override_emits_only_the_declarations_it_changes() {
        let mut project = project();
        let tablet = Uuid::new_v4();
        project.pages[0].sizes.push(PageSize {
            id: tablet,
            label: "Tablet".into(),
            width: 600,
            height: 900,
            kind: BoundaryKind::Screen,
            x: 0.0,
            y: 0.0,
        });
        project.pages[0].sizes.push(PageSize {
            id: Uuid::new_v4(),
            label: "Phone".into(),
            width: 390,
            height: 844,
            kind: BoundaryKind::Screen,
            x: 0.0,
            y: 0.0,
        });
        let mut changed = element(ElementKind::Box);
        // Only the width differs at tablet; every other declaration matches the base size.
        changed.overrides.insert(
            tablet,
            ElementOverride {
                rect: Some(Rect {
                    x: 12.0,
                    y: 24.0,
                    width: 100.0,
                    height: 40.0,
                }),
                ..Default::default()
            },
        );
        let unchanged = element(ElementKind::Box);
        project.pages[0].elements = vec![changed.clone(), unchanged.clone()];
        let css = exported_css(&project);
        assert!(css.contains(&format!(
            ".{}{{position:absolute;left:{};top:{};width:{};height:{};",
            class(changed.id),
            length(12.0),
            length(24.0),
            length(160.0),
            length(40.0)
        )));
        let start = css.find("@media (min-width:600px){").expect("tablet query");
        let media = &css[start..];
        let media = &media[..media.find("\n}\n").expect("tablet query end")];
        let rule = rule_of(media, changed.id);
        assert!(
            rule.contains(&format!("width:{};", length(100.0))),
            "{rule}"
        );
        // Unchanged declarations are inherited from the base rule instead of repeated.
        for property in ["height:", "left:", "top:", "display:", "background:"] {
            assert!(!rule.contains(property), "{property} repeated in {rule}");
        }
        assert!(
            !media.contains(&format!(".{}{{", class(unchanged.id))),
            "a layer with no change still wrote a rule"
        );
    }

    #[test]
    fn a_plain_layer_states_only_what_it_paints() {
        let mut project = project();
        let plain = element(ElementKind::Box);
        let mut text = element(ElementKind::Text);
        text.text = "Hi".into();
        let image = element(ElementKind::Image);
        project.pages[0].elements = vec![plain.clone(), text.clone(), image.clone()];
        let css = exported_css(&project);
        let rule = rule_of(&css, plain.id);
        for property in [
            "font-family",
            "font-size",
            "font-weight",
            "line-height",
            "letter-spacing",
            "text-align",
            "color",
            "object-fit",
            "box-shadow",
            "border",
            "transform",
            "opacity",
            "background",
        ] {
            assert!(!rule.contains(property), "{property} in {rule}");
        }
        assert!(!rule_of(&css, text.id).contains("object-fit"));
        assert!(rule_of(&css, image.id).contains("object-fit:cover;"));
        // The resets live in one shared rule instead of every layer's rule.
        for reset in [
            "box-sizing:border-box",
            "white-space:pre-wrap",
            "overflow-wrap:break-word",
            "transform-origin:center",
        ] {
            assert_eq!(css.matches(reset).count(), 1, "{reset}");
        }
        lightningcss::stylesheet::StyleSheet::parse(&css, Default::default()).unwrap();
    }

    /// One element's declaration block from the exported stylesheet.
    fn rule_of(css: &str, id: Uuid) -> String {
        let start = css
            .find(&format!(".{}{{", class(id)))
            .unwrap_or_else(|| panic!("no rule for {id}"));
        let rest = &css[start..];
        rest[..rest.find('}').unwrap() + 1].to_owned()
    }

    /// Every font list that styles the page ends in a concrete generic family.
    fn assert_generic_font_fallbacks(css: &str) {
        let mut checked = 0;
        for line in css
            .lines()
            .filter(|line| line.starts_with('.') || line.contains("--text-"))
        {
            let Some((_, rest)) = line.split_once("font-family:") else {
                continue;
            };
            let value = rest.split(';').next().unwrap_or_default();
            checked += 1;
            assert!(
                ["sans-serif", "serif", "monospace", "cursive", "fantasy"]
                    .iter()
                    .any(|generic| value.ends_with(generic)),
                "no generic fallback in {value}"
            );
        }
        assert!(checked > 0, "no font lists were checked");
    }

    #[test]
    fn missing_and_bundled_families_ship_one_embedded_face_with_a_generic_fallback() {
        let mut project = project();
        let missing = resolve_font("Desegnos nonexistent font 987654", 400, false);
        assert!(missing.fell_back);
        let mut regular = element(ElementKind::Text);
        regular.text = "Hello".into();
        regular.style.text_style.font_family = "Desegnos nonexistent font 987654".into();
        let mut bold = regular.clone();
        bold.id = Uuid::new_v4();
        bold.style.text_style.weight = 700;
        let mut italic = regular.clone();
        italic.id = Uuid::new_v4();
        italic.style.text_style.italic = true;
        let mut bundled = regular.clone();
        bundled.id = Uuid::new_v4();
        bundled.style.text_style.font_family = "Archivo".into();
        project.pages[0].elements = vec![regular.clone(), bold.clone(), italic.clone(), bundled.clone()];
        let bundle = direct_export_bundle(&project, &[]).unwrap();
        let css = std::str::from_utf8(&bundle.files["index.css"]).unwrap();
        let font_assets: Vec<_> = bundle
            .files
            .keys()
            .filter(|path| path.starts_with("assets/fonts/"))
            .collect();
        assert_eq!(font_assets, ["assets/fonts/archivo.ttf"]);
        assert_eq!(
            bundle.files["assets/fonts/archivo.ttf"].as_slice(),
            missing.bytes.as_slice()
        );
        assert!(css.contains("@font-face{font-family:\"Desegnos Archivo\";src:url(\"assets/fonts/archivo.ttf\");font-weight:400;font-style:normal;font-display:swap;}"));
        assert!(css.contains("@font-face{font-family:\"Desegnos Archivo\";src:url(\"assets/fonts/archivo.ttf\");font-weight:400;font-style:italic;font-display:swap;}"));
        assert!(css.contains("@font-face{font-family:\"Desegnos Archivo\";src:url(\"assets/fonts/archivo.ttf\");font-weight:700;font-style:normal;font-display:swap;}"));
        assert_eq!(css.matches("@font-face{").count(), 3);
        for element in [&regular, &bold, &italic, &bundled] {
            assert!(
                rule_of(css, element.id).contains("font-family:\"Desegnos Archivo\",sans-serif;"),
                "{} did not use the embedded face",
                element.id
            );
        }
        assert_generic_font_fallbacks(css);
        lightningcss::stylesheet::StyleSheet::parse(css, Default::default()).unwrap();
    }

    #[test]
    fn installed_families_keep_their_name_and_ship_no_face() {
        let family = "system-ui";
        let resolved = resolve_font(family, 400, false);
        if resolved.bundled || resolved.fell_back {
            // No installed face on this host; the missing-face test covers that path.
            return;
        }
        let mut project = project();
        let mut text = element(ElementKind::Text);
        text.text = "Hello".into();
        text.style.text_style.font_family = family.into();
        project.pages[0].elements = vec![text.clone()];
        let bundle = direct_export_bundle(&project, &[]).unwrap();
        let css = std::str::from_utf8(&bundle.files["index.css"]).unwrap();
        assert!(!bundle.files.keys().any(|path| path.starts_with("assets/fonts/")));
        assert!(!css.contains("@font-face"));
        assert!(rule_of(css, text.id).contains("font-family:system-ui,sans-serif;"));
    }

    #[test]
    fn font_lists_end_with_the_generic_that_suits_the_family() {
        assert_eq!(generic_fallback("Inter"), "sans-serif");
        assert_eq!(generic_fallback("Source Sans 3"), "sans-serif");
        assert_eq!(generic_fallback("Georgia, Times New Roman"), "serif");
        assert_eq!(generic_fallback("JetBrains Mono"), "monospace");
        assert_eq!(fallback_family_list("Inter"), "\"Inter\",sans-serif");
        assert_eq!(
            fallback_family_list("Georgia, Times New Roman"),
            "\"Georgia\",\"Times New Roman\",serif"
        );
        assert_eq!(fallback_family_list("monospace"), "monospace");
        assert_eq!(fallback_family_list("system-ui"), "system-ui,sans-serif");
    }

    #[test]
    fn project_text_styles_publish_the_embedded_face_too() {
        let mut project = project();
        let style = crate::TextStyle {
            id: Uuid::new_v4(),
            name: "Display".into(),
            font_family: "Desegnos nonexistent font 987654".into(),
            size: 32.0,
            weight: 700,
            line_height: 1.2,
            letter_spacing: 0.0,
            italic: true,
        };
        project.styles.text.push(style.clone());
        let bundle = direct_export_bundle(&project, &[]).unwrap();
        let css = std::str::from_utf8(&bundle.files["index.css"]).unwrap();
        assert!(css.contains(&format!(
            "--text-{}-font-family:\"Desegnos Archivo\",sans-serif;",
            style.id
        )));
        assert!(bundle.files.contains_key("assets/fonts/archivo.ttf"));
        assert_generic_font_fallbacks(css);
    }

    #[test]
    fn buttons_and_links_centre_their_labels_like_the_canvas() {
        let mut project = project();
        let mut button = element(ElementKind::Button);
        button.text = "Go".into();
        let mut link = element(ElementKind::Link);
        link.text = "Read".into();
        link.style.text_style.align = TextAlign::Right;
        let mut left_link = element(ElementKind::Link);
        left_link.text = "Home".into();
        project.pages[0].elements = vec![button.clone(), link.clone(), left_link.clone()];
        let bundle = direct_export_bundle(&project, &[]).unwrap();
        let css = std::str::from_utf8(&bundle.files["index.css"]).unwrap();
        for id in [button.id, link.id, left_link.id] {
            let rule = rule_of(css, id);
            assert!(rule.contains("display:flex;"), "{rule}");
            assert!(!rule.contains("display:block;"), "{rule}");
            assert!(rule.contains("align-items:center;"), "{rule}");
        }
        assert!(rule_of(css, button.id).contains("justify-content:center;"));
        assert!(rule_of(css, link.id).contains("justify-content:flex-end;"));
        assert!(rule_of(css, left_link.id).contains("justify-content:flex-start;"));
    }

    #[test]
    fn an_export_without_behaviours_writes_no_script_and_no_behaviour_file() {
        let mut project = project();
        let mut text = element(ElementKind::Text);
        text.text = "Hello".into();
        project.pages[0].elements = vec![text];
        let bundle = direct_export_bundle(&project, &[]).unwrap();
        assert!(!bundle.files.contains_key("behaviours.js"));
        let html = std::str::from_utf8(&bundle.files["index.html"]).unwrap();
        assert!(!html.contains("behaviours.js"), "{html}");
        assert!(!html.contains("<script"), "{html}");
    }

    #[test]
    fn standalone_face_passes_plain_fonts_through_and_rewrites_a_collection() {
        let font = resolve_font("Archivo", 400, false);
        assert_eq!(
            crate::standalone_face(&font.bytes, font.index).unwrap(),
            font.bytes.as_slice()
        );
        let mut collection = Vec::new();
        collection.extend_from_slice(b"ttcf");
        collection.extend_from_slice(&1u32.to_be_bytes());
        collection.extend_from_slice(&1u32.to_be_bytes());
        collection.extend_from_slice(&16u32.to_be_bytes());
        collection.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        collection.extend_from_slice(&1u16.to_be_bytes());
        collection.extend_from_slice(&[0u8; 6]);
        collection.extend_from_slice(b"cmap");
        collection.extend_from_slice(&0u32.to_be_bytes());
        collection.extend_from_slice(&28u32.to_be_bytes());
        collection.extend_from_slice(&4u32.to_be_bytes());
        collection.extend_from_slice(b"data");
        let face = crate::standalone_face(&collection, 0).unwrap();
        assert_eq!(&face[..4], &0x0001_0000u32.to_be_bytes());
        // The one table now starts after the rewritten directory it was copied into.
        assert_eq!(&face[20..24], &56u32.to_be_bytes());
        assert_eq!(&face[face.len() - collection.len()..], collection.as_slice());
        assert!(crate::standalone_face(&collection[..12], 0).is_none());
    }

    #[test]
    fn deterministic_color_scheme_and_background_follow_luminance() {
        let mut project = project();
        for (background, scheme) in [("#ffffff", "light"), ("#f7f7f5", "light"), ("#e9e9e9", "light"), ("#1f1f1f", "dark"), ("#121212", "dark"), ("#000000", "dark"), ("#fff", "light")] {
            project.pages[0].background = background.into();
            let bundle = direct_export_bundle(&project, &[]).unwrap();
            let css = std::str::from_utf8(&bundle.files["index.css"]).unwrap();
            let html = std::str::from_utf8(&bundle.files["index.html"]).unwrap();
            assert!(css.contains(&format!(":root{{color-scheme:{scheme};}}")));
            assert!(css.contains(&format!("background:{background};")));
            assert!(html.contains(&format!("<meta name=\"color-scheme\" content=\"{scheme}\">")));
        }
    }

    #[test]
    fn parsed_ai_color_scheme_overrides_luminance_and_invalid_values_fall_back() {
        let mut project = project();
        for background in ["#ffffff", "#121212"] {
            project.pages[0].background = background.into();
            let plan = plan_export(&project, &[], Uuid::new_v4()).unwrap();
            assert!(plan.markdown.contains("background and content colours"));
            for (field, expected) in [("light", "light"), ("dark", "dark"), ("invalid", project.pages[0].color_scheme())] {
                let json = serde_json::json!({
                    "jobId": plan.job_id, "requirementsSha256": plan.sha256, "summary": "Checked colours",
                    "pages": [{"pageId": project.pages[0].id, "colorScheme": field}],
                    "shared": {"css": "", "js": ""}, "pointers": [],
                });
                let reply = parse_export_reply(&plan, &json.to_string()).unwrap();
                let bundle = ai_export_bundle(&project, &plan, &reply).unwrap();
                let css = std::str::from_utf8(&bundle.files["index.css"]).unwrap();
                let html = std::str::from_utf8(&bundle.files["index.html"]).unwrap();
                assert!(css.contains(&format!("color-scheme:{expected};")));
                assert!(html.contains(&format!("name=\"color-scheme\" content=\"{expected}\"")));
            }
        }
    }

    #[test]
    fn sizing_rules_emit_fill_flex_limits_and_style_variables() {
        let mut child = element(ElementKind::Box);
        child.style.sizing.width = crate::SizingMode::Fill;
        child.style.sizing.height = crate::SizingMode::Fit;
        child.style.sizing.min_width = Some(20.0);
        let colour = Uuid::new_v4(); child.style.references.fill = Some(colour);
        let layout = ElementLayout::Stack { direction: StackDirection::Row, gap: 8.0, padding: 0.0, align: StackAlign::Start, justify: StackJustify::Start, wrap: false };
        let mut fonts = Fonts::default();
        let mut output = BTreeMap::new(); rules(&[child.clone()], &layout, &mut fonts, &mut output);
        let css = output[&child.id].block();
        assert!(css.contains("flex:1 1 0;"));
        assert!(css.contains("height:auto;"));
        assert!(css.contains("min-width:calc(20 * var(--desegnos-unit));"));
        assert!(css.contains(&format!("background:var(--colour-{colour});")));
        output.clear(); rules(&[child.clone()], &ElementLayout::Free, &mut fonts, &mut output);
        assert!(output[&child.id].block().contains("width:calc(100% -"));
    }

    #[test]
    fn stack_and_grid_padding_uses_each_named_side() {
        let mut child = element(ElementKind::Box);
        child.layout = ElementLayout::Stack {
            direction: StackDirection::Row,
            gap: 8.0,
            padding: 0.0,
            align: StackAlign::Start,
            justify: StackJustify::Start,
            wrap: false,
        };
        child.style.padding = Some(crate::ElementPadding {
            top: 4.0,
            right: 8.0,
            bottom: 12.0,
            left: 16.0,
        });
        let mut fonts = Fonts::default();
        let mut output = BTreeMap::new();
        rules(&[child.clone()], &ElementLayout::Free, &mut fonts, &mut output);
        let css = output[&child.id].block();
        assert!(css.contains(&format!(
            "padding:{} {} {} {};",
            length(4.0),
            length(8.0),
            length(12.0),
            length(16.0)
        )));
        // Without named sides the layout's all-sides value still wins, so old files are unchanged.
        child.style.padding = None;
        child.layout = ElementLayout::Grid {
            columns: 2,
            gap: 4.0,
            padding: 6.0,
        };
        output.clear();
        rules(&[child.clone()], &ElementLayout::Free, &mut fonts, &mut output);
        assert!(output[&child.id].block().contains(&format!("padding:{};", length(6.0))));
    }

    #[test]
    fn codegen_covers_kinds_layouts_and_size_overrides_deterministically() {
        let mut project = project();
        let kinds = vec![
            ElementKind::Box,
            ElementKind::Group,
            ElementKind::Text,
            ElementKind::Ellipse,
            ElementKind::Line,
            ElementKind::Image,
            ElementKind::Button,
            ElementKind::Link,
            ElementKind::Input {
                input_type: InputType::Email,
            },
            ElementKind::Textarea,
            ElementKind::Select {
                options: vec!["A & B".into()],
            },
            ElementKind::Checkbox,
            ElementKind::Radio,
            ElementKind::Video,
            ElementKind::Audio,
            ElementKind::Embed,
            ElementKind::Code {
                language: "html".into(),
                source: "<script>example</script>".into(),
            },
        ];
        let mut elements: Vec<_> = kinds.into_iter().map(element).collect();
        for e in &mut elements {
            e.text = "<Hello & world>".into();
        }
        elements[0].layout = ElementLayout::Stack {
            direction: StackDirection::Row,
            gap: 8.0,
            padding: 12.0,
            align: StackAlign::Center,
            justify: StackJustify::SpaceBetween,
            wrap: true,
        };
        elements[0].children.push(element(ElementKind::Button));
        elements[1].layout = ElementLayout::Grid {
            columns: 2,
            gap: 4.0,
            padding: 6.0,
        };
        elements[1].children.push(element(ElementKind::Text));
        elements[2].rect.width = 0.0;
        elements[2].rect.height = 0.0;
        elements[7].url = Some("/".into());
        elements[2].rotation = 15.0;
        elements[2].style.opacity = 0.5;
        elements[2].style.shadow.color = Some("#123456".into());
        let phone = Uuid::new_v4();
        elements[2].overrides.insert(
            phone,
            ElementOverride {
                rect: Some(Rect {
                    x: 8.0,
                    y: 16.0,
                    width: 100.0,
                    height: 30.0,
                }),
                ..Default::default()
            },
        );
        elements[3].overrides.insert(
            phone,
            ElementOverride {
                hidden: true,
                ..Default::default()
            },
        );
        project.pages[0].elements = elements;
        project.pages[0].sizes.push(PageSize {
            id: phone,
            label: "Phone".into(),
            width: 390,
            height: 844,
            kind: BoundaryKind::Screen,
            x: 1000.0,
            y: 0.0,
        });
        let one = direct_export_bundle(&project, &[]).unwrap();
        let two = direct_export_bundle(&project, &[]).unwrap();
        assert_eq!(one.files, two.files);
        assert_eq!(
            plan_export(&project, &[], Uuid::new_v4())
                .unwrap()
                .request_count,
            0
        );
        let html = std::str::from_utf8(&one.files["index.html"]).unwrap();
        let css = std::str::from_utf8(&one.files["index.css"]).unwrap();
        assert!(html.contains("&lt;Hello &amp; world&gt;"));
        assert!(html.contains("type=\"email\""));
        let resolved = resolve_font("system-ui", 400, false);
        let expected = if resolved.bundled {
            "font-family:\"Desegnos Archivo\",sans-serif;"
        } else {
            "font-family:system-ui,sans-serif;"
        };
        assert!(css.contains(expected), "missing {expected}");
        assert_generic_font_fallbacks(css);
        assert!(css.contains("width:max-content;height:auto;"));
        assert!(html.contains("href=\"index.html\""));
        assert!(html.contains("<line x1=\"0\" y1=\"20\""));
        assert!(html.contains("<option>A &amp; B</option>"));
        assert!(html.contains("&lt;script&gt;example&lt;/script&gt;"));
        for rule in [
            "position:absolute",
            "position:relative",
            "display:flex",
            "display:grid",
            "grid-template-columns:repeat(2",
            "@media (min-width:800px)",
            "100cqw / 390",
            "rotate(15deg)",
            "opacity:0.5",
            "display:none",
            "overflow:auto",
        ] {
            assert!(css.contains(rule), "missing {rule}");
        }
        lightningcss::stylesheet::StyleSheet::parse(css, Default::default()).unwrap();
    }

    #[test]
    fn behaviour_reuse_survives_text_edits_and_invalidates_only_changed_intent() {
        let mut project = project();
        let source = element(ElementKind::Button);
        let source_id = source.id;
        let target = element(ElementKind::Box);
        let target_id = target.id;
        project.pages[0].elements = vec![source, target];
        let page_id = project.pages[0].id;
        for comment in ["Toggle target", "Focus target"] {
            project.pointers.push(Pointer {
                id: Uuid::new_v4(),
                kind: PointerKind::Change,
                from: PointerSource::Element {
                    page_id,
                    element_id: source_id,
                    size_id: None,
                },
                to: PointerTarget::Element {
                    page_id,
                    element_id: target_id,
                    size_id: None,
                },
                comment: comment.into(),
                wiring: None,
                play: None,
                play_comment: String::new(),
            });
        }
        let plan = plan_export(&project, &[], Uuid::new_v4()).unwrap();
        let reply = ExportReply { job_id: plan.job_id, requirements_sha256: plan.sha256.clone(), summary: "Wired".into(), pages: vec![],
            shared: ExportReplyShared { css: String::new(), js: plan.pointers.iter().map(|p| format!("/* desegnos-pointer:{} */\nconst hook = document.body; console.log('a  b', hook);\n/* /desegnos-pointer:{} */\n", p.id, p.id)).collect() },
            pointers: plan.pointers.iter().map(|p| ExportReplyPointer { pointer_id: p.id, implemented: true }).collect() };
        let initial = ai_export_bundle(&project, &plan, &reply).unwrap();
        assert_eq!(initial.pointers.len(), 2);
        assert!(
            std::str::from_utf8(&initial.files["index.html"])
                .unwrap()
                .contains("<script src=\"behaviours.js\" defer></script>")
        );
        assert!(initial.files.contains_key("behaviours.js"));
        let mut html_edit = initial.files.clone();
        let html = std::str::from_utf8(&html_edit["index.html"])
            .unwrap()
            .replace("type=\"button\"", "type=\"reset\"");
        html_edit.insert("index.html".into(), html.into_bytes());
        for id in initial.pointers.keys() {
            assert_ne!(
                pointer_fingerprints(&initial.files)[id],
                pointer_fingerprints(&html_edit)[id]
            );
        }

        project.last_export = Some(ExportRecord {
            id: Uuid::new_v4(),
            project_id: project.id,
            version: 1,
            destination: "/tmp/export".into(),
            exported_at: "now".into(),
            pages: initial.pages.clone(),
            pointers: initial.pointers.clone(),
            behaviours: initial.behaviours.clone(),
        });
        let record = project.last_export.as_ref().unwrap();
        let mut another_run = record.clone();
        another_run.id = Uuid::new_v4();
        another_run.version += 1;
        another_run.destination = "/tmp/elsewhere".into();
        another_run.exported_at = "later".into();
        assert_eq!(export_manifest(record), export_manifest(&another_run));
        let encoded = serde_json::to_vec(&project).unwrap();
        project = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            plan_export(&project, &[], Uuid::new_v4())
                .unwrap()
                .request_count,
            0
        );
        project.pages[0].elements[0].text = "New label".into();
        let reused = direct_export_bundle(&project, &[]).unwrap();
        assert_ne!(initial.files["index.html"], reused.files["index.html"]);
        assert_eq!(initial.files["index.css"], reused.files["index.css"]);
        assert_eq!(
            initial.files["behaviours.js"],
            reused.files["behaviours.js"]
        );
        assert!(
            std::str::from_utf8(&reused.files["behaviours.js"])
                .unwrap()
                .contains("'a  b'")
        );
        assert_eq!(
            plan_export(&project, &[], Uuid::new_v4())
                .unwrap()
                .pointers
                .len(),
            0
        );
        project.pointers[0].comment = "Hide target".into();
        let changed = plan_export(&project, &[], Uuid::new_v4()).unwrap();
        assert_eq!(changed.pointers.len(), 1);
        assert_eq!(changed.pointers[0].id, project.pointers[0].id);
        project.pages[0].elements[1]
            .children
            .push(element(ElementKind::Text));
        assert_eq!(
            plan_export(&project, &[], Uuid::new_v4())
                .unwrap()
                .pointers
                .len(),
            2
        );
        project.pointers[0].kind = PointerKind::Reference;
        assert_eq!(
            plan_export(&project, &[], Uuid::new_v4())
                .unwrap()
                .pointers
                .len(),
            1
        );
        let mut edited = reused.files.clone();
        edited.insert("behaviours.js".into(), b"// hand edit".to_vec());
        assert_ne!(
            pointer_fingerprints(&reused.files),
            pointer_fingerprints(&edited)
        );
    }
}
