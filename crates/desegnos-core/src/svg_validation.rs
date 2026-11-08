use crate::{Error, Result, validation::ensure};
use quick_xml::{Reader, events::Event};
// Imported SVGs are portable images, not documents with scripts, external resources,
// animation, or HTML. Attribute values are XML-decoded before reference checks.
pub(crate) fn validate_svg(bytes: &[u8]) -> Result<()> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| Error("An SVG identity image must be UTF-8.".into()))?;
    let mut reader = Reader::from_str(source);
    let mut depth = 0_usize;
    let mut elements = 0_usize;
    let mut root_seen = false;
    let mut style: Option<(usize, String)> = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| Error("An SVG identity image contains invalid XML.".into()))?;
        let is_start = matches!(event, Event::Start(_));
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                ensure(style.is_none(), "An SVG style block cannot contain child elements.")?;
                elements += 1;
                ensure(elements <= 20_000 && depth < 128, "An SVG identity image is too complex.")?;
                let name = element.name();
                let name = name.as_ref();
                if depth == 0 {
                    ensure(!root_seen && name == "svg", "An SVG identity image needs one SVG root.")?;
                    root_seen = true;
                }
                ensure(matches!(name,
                    "svg" | "style" | "g" | "defs" | "symbol" | "use" | "title" | "desc" | "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "text" | "tspan" | "textPath" | "clipPath" | "mask" | "linearGradient" | "radialGradient" | "stop" | "pattern" | "marker" | "filter" | "feBlend" | "feColorMatrix" | "feComponentTransfer" | "feComposite" | "feConvolveMatrix" | "feDiffuseLighting" | "feDisplacementMap" | "feDistantLight" | "feDropShadow" | "feFlood" | "feFuncA" | "feFuncB" | "feFuncG" | "feFuncR" | "feGaussianBlur" | "feMerge" | "feMergeNode" | "feMorphology" | "feOffset" | "fePointLight" | "feSpecularLighting" | "feSpotLight" | "feTile" | "feTurbulence" | "view"),
                    "An SVG identity image contains unsupported active or external content.")?;
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|_| Error("An SVG identity image has invalid attributes.".into()))?;
                    let name = attribute.key.as_ref().to_ascii_lowercase();
                    let value = attribute.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|_| Error("An SVG identity image has an invalid attribute value.".into()))?;
                    ensure(!name.starts_with("on") && name != "xml:base", "An SVG identity image cannot contain event handlers or a base URL.")?;
                    if name == "xmlns" || name == "xmlns:xlink" {
                        ensure(value == "http://www.w3.org/2000/svg" || value == "http://www.w3.org/1999/xlink", "An SVG identity image has an unsupported namespace.")?;
                    } else if name == "href" || name == "xlink:href" {
                        ensure(value.starts_with('#') && !value.chars().any(char::is_whitespace), "An SVG identity image cannot reference external resources.")?;
                    } else {
                        validate_svg_paint(&value)?;
                    }
                }
                if is_start { depth += 1; }
                if name == "style" && is_start { style = Some((depth, String::new())); }
            }
            Event::End(_) => {
                if style.as_ref().is_some_and(|(style_depth, _)| *style_depth == depth) {
                    let (_, css) = style.take().ok_or_else(|| Error("Invalid SVG style block.".into()))?;
                    validate_svg_paint(&css)?;
                }
                depth = depth.checked_sub(1).ok_or_else(|| Error("An SVG identity image has an invalid element structure.".into()))?;
            }
            Event::Text(text) => {
                ensure(depth > 0 || text.as_ref().chars().all(char::is_whitespace), "An SVG identity image has content outside its root.")?;
                if let Some((_, css)) = &mut style { css.push_str(text.as_ref()); }
            }
            Event::Decl(declaration) => {
                ensure(!root_seen && declaration.version().is_ok_and(|value| value == "1.0"), "An SVG declaration must use XML 1.0 and precede the root.")?;
                if let Some(encoding) = declaration.encoding() {
                    ensure(encoding.is_ok_and(|value| value.eq_ignore_ascii_case("utf-8")), "An SVG identity image must declare UTF-8 encoding.")?;
                }
            },
            Event::Comment(_) => {},
            Event::CData(text) => {
                ensure(depth > 0, "An SVG identity image has text outside its root.")?;
                if let Some((_, css)) = &mut style { css.push_str(text.as_ref()); }
            },
            Event::GeneralRef(reference) => {
                let value = reference.as_ref();
                let character = reference.resolve_char_ref().map_err(|_| Error("Invalid SVG character reference.".into()))?;
                ensure(depth > 0 && (matches!(value, "amp" | "lt" | "gt" | "quot" | "apos") || character.is_some()), "An SVG identity image cannot contain custom entities.")?;
                if let Some((_, css)) = &mut style {
                    if let Some(character) = character { css.push(character); }
                    else if let Some(value) = quick_xml::escape::resolve_predefined_entity(value) { css.push_str(value); }
                }
            },
            Event::Eof => break,
            _ => return Err(Error("An SVG identity image cannot contain external entities, processing instructions, or document types.".into())),
        }
    }
    ensure(
        root_seen && depth == 0,
        "An SVG identity image has an incomplete root.",
    )
}

fn validate_svg_paint(value: &str) -> Result<()> {
    let lower = value.to_ascii_lowercase();
    // Reject CSS escapes/imports and every non-fragment URL; alternate spellings
    // must not conceal a resource request. The original image bytes stay intact.
    ensure(
        !value.contains('\\')
            && !lower.contains('@')
            && !lower.contains("javascript:")
            && !lower.contains("https:")
            && !lower.contains("http:")
            && !lower.contains("data:")
            && !lower.contains("//"),
        "An SVG identity image cannot reference external resources.",
    )?;
    let mut rest = lower.as_str();
    while let Some((_, after)) = rest.split_once("url(") {
        let (reference, tail) = after.split_once(')').ok_or_else(|| {
            Error("An SVG identity image contains an invalid paint reference.".into())
        })?;
        let reference = reference.trim().trim_matches(['\'', '"']);
        ensure(
            reference.starts_with('#') && !reference.chars().any(char::is_whitespace),
            "An SVG identity image cannot reference external resources.",
        )?;
        rest = tail;
    }
    Ok(())
}
