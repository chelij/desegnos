use crate::{Error, Project, Result, digest, ensure, model::validate_text};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub use crate::limits::MAX_IDENTITY_ASSET_BYTES;
pub use crate::limits::MAX_IDENTITY_ASSETS;
pub use crate::limits::MAX_PHOTO_DESCRIPTION_CHARS;
/// The user's own pictures from an imported folder (portfolio photos, products, team).
pub use crate::limits::MAX_PROJECT_PHOTO_BYTES;

/// Existing work and pages found in imported sources (projects, tutorials, articles), so
/// generation knows what exists and where to link.
pub use crate::limits::MAX_IMPORTED_ITEMS;

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourcePageRef {
    pub path: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourcePage {
    pub path: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub kind: String,
    pub photo_count: usize,
    pub item_count: usize,
}
impl SourcePage {
    pub fn reference(&self) -> SourcePageRef {
        SourcePageRef {
            path: self.path.clone(),
            title: self.title.clone(),
            url: self.url.clone(),
        }
    }
}

/// The content reviewed for an actual Desegnos page. Photo bytes remain shared in identity.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedPageContext {
    pub page_id: Uuid,
    pub source: SourcePage,
    pub photo_ids: Vec<Uuid>,
    pub content: Vec<ImportedItem>,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedItem {
    pub kind: String,
    pub title: String,
    /// A public https address or a site path such as /projects/robot; empty when unknown.
    pub url: String,
    pub summary: String,
    #[serde(default)]
    pub source_pages: Vec<SourcePageRef>,
}

impl ImportedItem {
    pub fn validate(&self) -> Result<()> {
        ensure(
            matches!(
                self.kind.as_str(),
                "project" | "tutorial" | "article" | "page" | "product" | "other"
            ),
            "An imported item has an unknown kind.",
        )?;
        validate_text(&self.title, "Imported item title", 160, true)?;
        for page in &self.source_pages {
            crate::validate_source_page_ref(page)?;
        }
        validate_text(&self.summary, "Imported item summary", 300, false)?;
        validate_text(&self.url, "Imported item link", 2048, false)?;
        ensure(
            self.url.is_empty()
                || self.url.starts_with("https://")
                || self.url.starts_with("http://")
                || (self.url.starts_with('/') && !self.url.starts_with("//")),
            "An imported item link must be a web address or a site path.",
        )
    }
}

/// A photo kept with the project. Bytes are shared, so undo history does not copy them.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectPhoto {
    pub id: Uuid,
    pub name: String,
    pub data_uri: std::sync::Arc<str>,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub description: String,
    #[serde(default)]
    pub source_pages: Vec<SourcePageRef>,
}

impl ProjectPhoto {
    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.id.is_nil(),
            "A project photo needs a valid identifier.",
        )?;
        for page in &self.source_pages {
            crate::validate_source_page_ref(page)?;
        }
        validate_text(&self.name, "Photo name", 240, true)?;
        validate_text(
            &self.description,
            "Photo description",
            MAX_PHOTO_DESCRIPTION_CHARS,
            false,
        )?;
        ensure(
            crate::limits::is_sha256_hex(&self.sha256),
            "A project photo has an invalid fingerprint.",
        )?;
        ensure(
            (1..=4096).contains(&self.width) && (1..=4096).contains(&self.height),
            "A project photo has an invalid size.",
        )?;
        let encoded = self
            .data_uri
            .strip_prefix("data:image/jpeg;base64,")
            .ok_or_else(|| Error("A project photo must be an embedded JPEG.".into()))?;
        ensure(
            !encoded.is_empty()
                && encoded.len() <= crate::limits::base64_encoded_len(MAX_PROJECT_PHOTO_BYTES),
            "A project photo exceeds its size limit.",
        )?;
        let (_, bytes) = crate::limits::decode_image_data_uri(&self.data_uri)?;
        ensure(
            bytes.len() <= MAX_PROJECT_PHOTO_BYTES && digest(&bytes) == self.sha256,
            "A project photo failed its integrity check.",
        )?;
        ensure(
            raster_dimensions("image/jpeg", &bytes) == Some((self.width, self.height)),
            "A project photo's dimensions do not match its stored image.",
        )
    }
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityAsset {
    pub id: Uuid,
    pub name: String,
    pub mime: String,
    pub data_uri: String,
    pub sha256: String,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedIdentity {
    pub source_name: String,
    pub source_sha256: String,
    pub summary: String,
    pub assets: Vec<IdentityAsset>,
    #[serde(default)]
    pub photos: Vec<ProjectPhoto>,
    #[serde(default)]
    pub content: Vec<ImportedItem>,
    #[serde(default)]
    pub pages: Vec<ImportedPageContext>,
}

impl IdentityAsset {
    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.id.is_nil(),
            "An identity asset needs a valid identifier.",
        )?;
        validate_text(
            &self.name,
            "Identity asset name",
            crate::limits::MAX_NAME_CHARS,
            true,
        )?;
        ensure(
            crate::limits::is_sha256_hex(&self.sha256),
            "An identity asset has an invalid fingerprint.",
        )?;
        ensure(
            matches!(
                self.mime.as_str(),
                "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "image/svg+xml"
            ),
            "Identity images must be PNG, JPEG, WebP, GIF, or SVG.",
        )?;
        let prefix = format!("data:{};base64,", self.mime);
        let encoded = self.data_uri.strip_prefix(&prefix).ok_or_else(|| {
            Error("An identity image must use a matching embedded base64 data URI.".into())
        })?;
        ensure(
            !encoded.is_empty()
                && encoded.len() <= crate::limits::base64_encoded_len(MAX_IDENTITY_ASSET_BYTES),
            format!(
                "An identity image exceeds the {} byte limit.",
                MAX_IDENTITY_ASSET_BYTES
            ),
        )?;
        let (mime, bytes) = crate::limits::decode_image_data_uri(&self.data_uri)?;
        ensure(
            mime == self.mime && bytes.len() <= MAX_IDENTITY_ASSET_BYTES,
            format!(
                "An identity image must fit within {} bytes.",
                MAX_IDENTITY_ASSET_BYTES
            ),
        )?;
        ensure(
            digest(&bytes) == self.sha256,
            "An identity image failed its integrity check.",
        )?;
        if self.mime != "image/svg+xml" {
            ensure(
                raster_dimensions(&self.mime, &bytes).is_some_and(|(width, height)| {
                    (1..=16_384).contains(&width) && (1..=16_384).contains(&height)
                }),
                "An identity image must have a valid image header and dimensions between 1 and 16,384 pixels per side.",
            )?;
        }
        Ok(())
    }
}

/// Reviewed brand findings and logos to merge into an open project ("Import context").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedContext {
    pub source_name: String,
    pub source_sha256: String,
    pub summary: String,
    pub findings: Vec<(String, String)>,
    pub assets: Vec<IdentityAsset>,
    pub photos: Vec<ProjectPhoto>,
    pub content: Vec<ImportedItem>,
    pub pages: Vec<SourcePage>,
    pub attach_matching_pages: bool,
}

impl ImportedIdentity {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.source_name, "Identity source name", 240, true)?;
        validate_text(
            &self.summary,
            "Imported identity summary",
            crate::MAX_PROMPT_CHARS,
            true,
        )?;
        ensure(
            crate::limits::is_sha256_hex(&self.source_sha256),
            "The imported identity has an invalid source fingerprint.",
        )?;
        ensure(
            self.assets.len() <= MAX_IDENTITY_ASSETS,
            "An imported identity supports at most four images.",
        )?;
        let mut ids = BTreeSet::new();
        for asset in &self.assets {
            asset.validate()?;
            ensure(
                ids.insert(asset.id),
                "Identity asset identifiers must be unique.",
            )?;
        }
        for photo in &self.photos {
            photo.validate()?;
            ensure(
                ids.insert(photo.id),
                "Project photo identifiers must be unique.",
            )?;
        }
        ensure(
            self.content.len() <= MAX_IMPORTED_ITEMS,
            "A project keeps at most 64 imported content items.",
        )?;
        for item in &self.content {
            item.validate()?;
        }
        ensure(
            self.pages.len() <= crate::MAX_PAGES * crate::MAX_PAGES,
            "Too many imported page contexts.",
        )?;
        for page in &self.pages {
            crate::validate_source_page_ref(&page.source.reference())?;
            ensure(
                page.content.len() <= MAX_IMPORTED_ITEMS,
                "Too many work items on an imported page.",
            )?;
            validate_text(&page.source.title, "Source page title", 240, true)?;
            ensure(
                !page.page_id.is_nil()
                    && page
                        .photo_ids
                        .iter()
                        .all(|id| self.photos.iter().any(|photo| photo.id == *id)),
                "An imported page references a missing photo.",
            )?;
            for item in &page.content {
                item.validate()?;
            }
        }
        Ok(())
    }
}

impl Project {
    /// Merge reviewed context into an existing project as one change. Findings become project
    /// rules (protected rules are never overwritten), brand name and tone fill the brief, and
    /// logos join the project's images up to the usual limit of four.
    pub fn import_context(&self, revision: u64, context: ImportedContext) -> Result<Self> {
        ensure(
            revision == self.revision,
            "The project changed. Import the context again.",
        )?;
        ensure(
            !context.findings.is_empty()
                || !context.assets.is_empty()
                || !context.photos.is_empty()
                || !context.content.is_empty()
                || !context.pages.is_empty(),
            "Choose at least one finding, logo, photo or content item to import.",
        )?;
        let mut next = self.clone();
        next.store_inline_images()?;
        next.revision = crate::limits::next_revision(revision)?;
        for (topic, value) in &context.findings {
            let (topic, value) = (topic.trim(), value.trim());
            let normalized = topic.to_lowercase();
            if normalized == "brand name" {
                next.brief.brand_name = value.into();
            }
            if normalized == "tone" {
                next.brief.tone = value.into();
            }
            match next.decisions.iter_mut().find(|decision| {
                decision.scope == crate::Scope::Project
                    && decision.topic.eq_ignore_ascii_case(topic)
            }) {
                Some(existing) if existing.protected => {}
                Some(existing) => {
                    existing.value = value.into();
                    existing.accepted_at_revision = next.revision;
                }
                None => next.decisions.push(crate::Decision {
                    id: uuid::Uuid::new_v4(),
                    topic: topic.into(),
                    value: value.into(),
                    scope: crate::Scope::Project,
                    protected: false,
                    accepted_at_revision: next.revision,
                }),
            }
        }
        let identity = match next.imported_identity.take() {
            Some(mut existing) => {
                for asset in context.assets {
                    if !existing
                        .assets
                        .iter()
                        .any(|item| item.sha256 == asset.sha256 || item.id == asset.id)
                    {
                        if existing.assets.len() == MAX_IDENTITY_ASSETS {
                            existing.assets.remove(0);
                        }
                        existing.assets.push(asset);
                    }
                }
                for photo in context.photos {
                    if !existing.photos.iter().any(|item| item.id == photo.id) {
                        existing.photos.push(photo);
                    }
                }
                // The same title and link from a newer import replaces the older entry.
                for item in context.content {
                    existing.content.retain(|old| {
                        !(old.title.eq_ignore_ascii_case(&item.title) && old.url == item.url)
                    });
                    if existing.content.len() == MAX_IMPORTED_ITEMS {
                        existing.content.remove(0);
                    }
                    existing.content.push(item);
                }
                existing.source_name = context.source_name;
                existing.source_sha256 = context.source_sha256;
                let summary = format!("{}\n{}", existing.summary, context.summary);
                existing.summary = summary
                    .chars()
                    .rev()
                    .take(crate::MAX_PROMPT_CHARS)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                existing
            }
            None => ImportedIdentity {
                source_name: context.source_name,
                source_sha256: context.source_sha256,
                summary: context.summary,
                assets: context.assets,
                photos: context.photos,
                content: context.content,
                pages: vec![],
            },
        };
        next.imported_identity = Some(identity);
        next.validate()?;
        Ok(next)
    }
}

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

fn raster_dimensions(mime: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    // GIF decoding is not enabled: retain animation and inspect its logical screen size.
    if mime == "image/gif" {
        return Some((
            u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?) as u32,
            u16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?) as u32,
        ));
    }
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}
