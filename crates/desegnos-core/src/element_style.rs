use crate::{Element, ElementKind, ElementLayout, ElementPadding};
pub(crate) fn padding(element: &Element) -> ElementPadding {
    let fallback = match element.layout {
        ElementLayout::Free => 0.0,
        ElementLayout::Stack { padding, .. } | ElementLayout::Grid { padding, .. } => padding,
    };
    element.style.padding.unwrap_or(ElementPadding {
        top: fallback,
        right: fallback,
        bottom: fallback,
        left: fallback,
    })
}
pub(crate) fn clips_content(kind: &ElementKind) -> bool {
    !matches!(
        kind,
        ElementKind::Box | ElementKind::Group | ElementKind::Line
    )
}
pub(crate) fn draws_border(kind: &ElementKind) -> bool {
    !matches!(kind, ElementKind::Line | ElementKind::Group)
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

pub(crate) fn border_width(element: &Element) -> f64 {
    if draws_border(&element.kind) && element.style.stroke.color.as_deref().is_some_and(paints) {
        element.style.stroke.width.max(0.0)
    } else {
        0.0
    }
}
pub(crate) fn paints(value: &str) -> bool {
    crate::geometry::Rgba::parse(value).is_some_and(|colour| colour.a > 0.0)
}

pub(crate) fn text_of(element: &Element) -> Option<&str> {
    match &element.kind {
        ElementKind::Code { source, .. } => Some(source),
        ElementKind::Text
        | ElementKind::Button
        | ElementKind::Link
        | ElementKind::Input { .. }
        | ElementKind::Textarea
        | ElementKind::Select { .. }
        | ElementKind::Checkbox
        | ElementKind::Radio => Some(&element.text),
        ElementKind::Box | ElementKind::Group | ElementKind::Ellipse
            if !element.text.is_empty() =>
        {
            Some(&element.text)
        }
        _ => None,
    }
}
pub(crate) fn wraps(kind: &ElementKind) -> bool {
    !matches!(
        kind,
        ElementKind::Input { .. }
            | ElementKind::Button
            | ElementKind::Link
            | ElementKind::Select { .. }
            | ElementKind::Checkbox
            | ElementKind::Radio
    )
}
pub(crate) fn control_padding(kind: &ElementKind) -> crate::ElementPadding {
    let horizontal = match kind {
        ElementKind::Textarea | ElementKind::Input { .. } | ElementKind::Select { .. } => 8.0,
        ElementKind::Button | ElementKind::Link => 1.0,
        ElementKind::Checkbox | ElementKind::Radio => 2.0,
        _ => 0.0,
    };
    let vertical = if matches!(kind, ElementKind::Textarea) {
        8.0
    } else {
        0.0
    };
    crate::ElementPadding {
        top: vertical,
        right: horizontal,
        bottom: vertical,
        left: horizontal,
    }
}
pub(crate) fn line_width(width: f64) -> f64 {
    if width > 0.0 { width } else { 1.0 }
}
pub(crate) fn transformed(element: &Element) -> bool {
    (element.rotation % 360.0).abs() > 0.000_001 || element.style.flip_x || element.style.flip_y
}
pub(crate) fn input_type_name(kind: &crate::InputType) -> &'static str {
    use crate::InputType::*;
    match kind {
        Text => "text",
        Email => "email",
        Password => "password",
        Number => "number",
        Search => "search",
        Tel => "tel",
        Url => "url",
        Date => "date",
        Time => "time",
        Color => "color",
        Range => "range",
        File => "file",
    }
}

pub(crate) const MAX_EMBED_DEPTH: usize = 3;
pub(crate) fn generic_keyword(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "sans-serif" | "system-ui" | "ui-sans-serif" | "ui-rounded" | "-apple-system"
        | "blinkmacsystemfont" => Some("sans-serif"),
        "serif" | "ui-serif" => Some("serif"),
        "monospace" | "ui-monospace" => Some("monospace"),
        "cursive" => Some("cursive"),
        "fantasy" => Some("fantasy"),
        "emoji" => Some("emoji"),
        "math" => Some("math"),
        "fangsong" => Some("fangsong"),
        _ => None,
    }
}
pub(crate) fn grid_column_width(width: f64, columns: u32, gap: f64) -> f64 {
    ((width - gap * columns.saturating_sub(1) as f64) / columns.max(1) as f64).max(0.0)
}
