use crate::{Appearance, Error, Project, Result, digest, ensure, model::validate_text};
use base64::{Engine, engine::general_purpose::STANDARD};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const MAX_IDENTITY_ASSETS: usize = 4;
pub const MAX_IDENTITY_ASSET_BYTES: usize = 512 * 1024;
/// The user's own pictures from an imported folder (portfolio photos, products, team).
pub const MAX_PROJECT_PHOTO_BYTES: usize = 200 * 1024;
pub const MAX_PHOTO_DESCRIPTION_CHARS: usize = 200;

/// Existing work and pages found in imported sources (projects, tutorials, articles), so
/// generation knows what exists and where to link.
pub const MAX_IMPORTED_ITEMS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourcePageRef {
    pub path: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourcePage {
    pub path: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub photo_count: usize,
    #[serde(default)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedPageContext {
    pub page_id: Uuid,
    pub source: SourcePage,
    pub photo_ids: Vec<Uuid>,
    pub content: Vec<ImportedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedItem {
    pub kind: String,
    pub title: String,
    /// A public https address or a site path such as /projects/robot; empty when unknown.
    pub url: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectPhoto {
    pub id: Uuid,
    pub name: String,
    pub data_uri: std::sync::Arc<str>,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
            valid_digest(&self.sha256),
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
            !encoded.is_empty() && encoded.len() <= MAX_PROJECT_PHOTO_BYTES.div_ceil(3) * 4,
            "A project photo exceeds its size limit.",
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityAsset {
    pub id: Uuid,
    pub name: String,
    pub mime: String,
    pub data_uri: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedIdentity {
    pub source_name: String,
    pub source_sha256: String,
    pub summary: String,
    pub assets: Vec<IdentityAsset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub photos: Vec<ProjectPhoto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ImportedItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<ImportedPageContext>,
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl IdentityAsset {
    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.id.is_nil(),
            "An identity asset needs a valid identifier.",
        )?;
        validate_text(&self.name, "Identity asset name", 120, true)?;
        ensure(
            valid_digest(&self.sha256),
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
            !encoded.is_empty() && encoded.len() <= MAX_IDENTITY_ASSET_BYTES.div_ceil(3) * 4,
            "An identity image exceeds the 512 KiB limit.",
        )?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| Error("An identity image contains invalid base64 data.".into()))?;
        ensure(
            !bytes.is_empty()
                && bytes.len() <= MAX_IDENTITY_ASSET_BYTES
                && STANDARD.encode(&bytes) == encoded,
            "An identity image must use canonical base64 and fit within 512 KiB.",
        )?;
        ensure(
            digest(&bytes) == self.sha256,
            "An identity image failed its integrity check.",
        )?;
        let signature_matches = match self.mime.as_str() {
            "image/png" => {
                bytes.len() >= 33
                    && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                    && &bytes[12..16] == b"IHDR"
            }
            "image/jpeg" => {
                bytes.len() >= 4
                    && bytes.starts_with(b"\xff\xd8\xff")
                    && bytes.ends_with(b"\xff\xd9")
            }
            "image/webp" => {
                bytes.len() >= 20
                    && bytes.starts_with(b"RIFF")
                    && &bytes[8..12] == b"WEBP"
                    && matches!(&bytes[12..16], b"VP8 " | b"VP8L" | b"VP8X")
                    && u32::from_le_bytes(
                        bytes[4..8]
                            .try_into()
                            .map_err(|_| Error("Invalid WebP image.".into()))?,
                    ) as usize
                        == bytes.len() - 8
            }
            "image/gif" => {
                bytes.len() >= 14
                    && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
                    && bytes.ends_with(b";")
            }
            "image/svg+xml" => {
                validate_svg(&bytes)?;
                true
            }
            _ => false,
        };
        ensure(
            signature_matches,
            "An identity image's bytes do not match its declared format.",
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
            valid_digest(&self.source_sha256),
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
        next.revision = revision
            .checked_add(1)
            .ok_or_else(|| crate::Error("Project revision limit reached.".into()))?;
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
        let page_contexts = next.import_source_pages(&context)?;
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
                for page in page_contexts.clone() {
                    if let Some(old) = existing.pages.iter_mut().find(|old| {
                        old.page_id == page.page_id && old.source.path == page.source.path
                    }) {
                        for id in page.photo_ids {
                            if !old.photo_ids.contains(&id) {
                                old.photo_ids.push(id);
                            }
                        }
                        for item in page.content {
                            old.content.retain(|existing| {
                                !(existing.title.eq_ignore_ascii_case(&item.title)
                                    && existing.url == item.url)
                            });
                            if old.content.len() == MAX_IMPORTED_ITEMS {
                                old.content.remove(0);
                            }
                            old.content.push(item);
                        }
                    } else {
                        existing.pages.push(page);
                    }
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
                pages: page_contexts,
            },
        };
        next.imported_identity = Some(identity);
        next.validate()?;
        Ok(next)
    }

    pub fn import_source_pages(
        &mut self,
        context: &ImportedContext,
    ) -> Result<Vec<ImportedPageContext>> {
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for source in &context.pages {
            crate::validate_source_page_ref(&source.reference())?;
            ensure(
                seen.insert(&source.path),
                "Choose each source page only once.",
            )?;
            let matching = self.pages.iter().position(|page| page.path == source.path);
            let index = if let Some(index) =
                matching.filter(|_| context.attach_matching_pages || source.path == "/")
            {
                index
            } else {
                let mut path = source.path.clone();
                let mut suffix = 2;
                while self.pages.iter().any(|page| page.path == path) {
                    path = format!("{}-{suffix}", source.path);
                    suffix += 1;
                }
                self.pages.push(crate::Page {
                    id: Uuid::new_v4(),
                    name: source.title.chars().take(120).collect(),
                    path,
                    included: true,
                    viewport: self.viewport,
                    appearance: self.appearance,
                    regions: vec![],
                    content: None,
                });
                self.pages.len() - 1
            };
            if self.pages[index].id == self.home_page_id && self.revision == 0 {
                self.pages[index].name = source.title.chars().take(120).collect();
            }
            result.push(ImportedPageContext {
                page_id: self.pages[index].id,
                source: source.clone(),
                photo_ids: context
                    .photos
                    .iter()
                    .filter(|photo| {
                        photo
                            .source_pages
                            .iter()
                            .any(|page| page.path == source.path)
                    })
                    .map(|photo| photo.id)
                    .collect(),
                content: context
                    .content
                    .iter()
                    .filter(|item| {
                        item.source_pages
                            .iter()
                            .any(|page| page.path == source.path)
                    })
                    .cloned()
                    .collect(),
            });
        }
        Ok(result)
    }

    pub fn set_initial_identity(&self, identity: ImportedIdentity) -> Result<Self> {
        self.validate()?;
        identity.validate()?;
        ensure(
            self.revision == 0
                && self.imported_identity.is_none()
                && self.pages.len() == 1
                && self.pages[0].content.is_none()
                && self.pages[0].regions.is_empty()
                && self.latest_generation.is_none()
                && self.applied_job_ids.is_empty()
                && self.appearance == Appearance::Light
                && self.pages[0].appearance == Appearance::Light,
            "Import identity into a new blank light project before editing begins.",
        )?;
        let mut next = self.clone();
        next.imported_identity = Some(identity);
        next.validate()?;
        Ok(next)
    }
}

// Imported SVGs are portable images, not documents with scripts, external resources,
// animation, or HTML. Attribute values are XML-decoded before reference checks.
fn validate_svg(bytes: &[u8]) -> Result<()> {
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
    match mime {
        "image/png" if bytes.get(8..12) == Some(&13_u32.to_be_bytes()) => Some((
            u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?),
            u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?),
        )),
        "image/gif" => Some((
            u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?) as u32,
            u16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?) as u32,
        )),
        "image/jpeg" => {
            let mut cursor = 2_usize;
            while cursor < bytes.len() {
                if *bytes.get(cursor)? != 0xff {
                    return None;
                }
                while *bytes.get(cursor)? == 0xff {
                    cursor += 1;
                }
                let marker = *bytes.get(cursor)?;
                cursor += 1;
                if marker == 0xda || marker == 0xd9 || marker == 0x00 {
                    return None;
                }
                if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
                    continue;
                }
                let length =
                    u16::from_be_bytes(bytes.get(cursor..cursor + 2)?.try_into().ok()?) as usize;
                let end = cursor.checked_add(length)?;
                if length < 2 || end > bytes.len() {
                    return None;
                }
                if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                    if length < 8 {
                        return None;
                    }
                    let components = *bytes.get(cursor + 7)? as usize;
                    if !(1..=4).contains(&components) || length != 8 + 3 * components {
                        return None;
                    }
                    return Some((
                        u16::from_be_bytes(bytes.get(cursor + 5..cursor + 7)?.try_into().ok()?)
                            as u32,
                        u16::from_be_bytes(bytes.get(cursor + 3..cursor + 5)?.try_into().ok()?)
                            as u32,
                    ));
                }
                cursor = end;
            }
            None
        }
        "image/webp" => {
            let length = u32::from_le_bytes(bytes.get(16..20)?.try_into().ok()?) as usize;
            if length.checked_add(20)? > bytes.len() {
                return None;
            }
            match bytes.get(12..16)? {
                b"VP8X" if length == 10 => Some((
                    1 + little_24(bytes.get(24..27)?),
                    1 + little_24(bytes.get(27..30)?),
                )),
                b"VP8L" if length >= 5 && bytes.get(20) == Some(&0x2f) => {
                    let bits = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
                    Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
                }
                b"VP8 " if length >= 10 && bytes.get(23..26) == Some(&[0x9d, 0x01, 0x2a]) => {
                    Some((
                        (u16::from_le_bytes(bytes.get(26..28)?.try_into().ok()?) & 0x3fff) as u32,
                        (u16::from_le_bytes(bytes.get(28..30)?.try_into().ok()?) & 0x3fff) as u32,
                    ))
                }
                _ => None,
            }
        }
        _ => None,
    }
}
fn little_24(bytes: &[u8]) -> u32 {
    bytes[0] as u32 | (bytes[1] as u32) << 8 | (bytes[2] as u32) << 16
}
