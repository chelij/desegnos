//! Explicit native image requests. HTML cannot grant credentials, URLs or filesystem access.
use crate::{GenerationCandidate, Project, RequirementsSnapshot, Result, editing, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageGenerationRequest {
    pub prompt: String,
    pub source: Option<String>,
    pub mask: Option<String>,
    pub size: String,
    pub transparent: bool,
}

impl ImageGenerationRequest {
    pub fn mask_image(
        &self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
    ) -> Result<Option<String>> {
        let mut mask = self.clone();
        mask.source = self.mask.clone();
        mask.reference_image(snapshot, project)
    }

    /// References are resolved exclusively from frozen request attachments and project assets.
    pub fn reference_image(
        &self,
        snapshot: &RequirementsSnapshot,
        project: &Project,
    ) -> Result<Option<String>> {
        let Some(source) = &self.source else {
            return Ok(None);
        };
        let id = source
            .strip_prefix(crate::PROJECT_ASSET_SCHEME)
            .unwrap_or(source);
        ensure(
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "Image edits need an attachment ID or project asset reference.",
        )?;
        if source.starts_with(crate::PROJECT_ASSET_SCHEME) {
            let content = crate::SiteContent {
                html: format!("<img src=\"{source}\">"),
                css: String::new(),
                javascript: String::new(),
            };
            let resolved = project.resolve_site_content(&content)?;
            let doc = editing::parse(&resolved.html)?;
            return Ok(doc
                .select("img")
                .nodes()
                .first()
                .and_then(|n| n.attr("src"))
                .map(|v| v.to_string()));
        }
        let targets = snapshot.target_ids()?;
        let targets = &targets;
        let image = project
            .pages
            .iter()
            .flat_map(|page| {
                page.regions
                    .iter()
                    .filter(move |region| targets.contains(&(page.id, region.id)))
            })
            .flat_map(|region| &region.attachments)
            .find(|attachment| {
                attachment.id == *source && attachment.kind == crate::AttachmentKind::Image
            })
            .and_then(|attachment| attachment.data_uri.as_deref());
        ensure(
            image.is_some(),
            "An image edit references an unknown request attachment.",
        )?;
        Ok(image.map(str::to_owned))
    }
}

fn read(node: dom_query::NodeRef<'_>) -> Result<ImageGenerationRequest> {
    let prompt = node
        .attr("data-desegnos-generate")
        .unwrap_or_default()
        .to_string();
    ensure(
        !prompt.trim().is_empty() && prompt.chars().count() <= 4096 && !prompt.contains('\0'),
        "Image generation needs a prompt of at most 4096 characters.",
    )?;
    let source = node
        .attr("data-desegnos-source")
        .map(|value| value.to_string());
    if let Some(source) = &source {
        // Asset IDs and attachment IDs only; never interpolate arbitrary text into HTML.
        let id = source
            .strip_prefix(crate::PROJECT_ASSET_SCHEME)
            .unwrap_or(source);
        ensure(
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "Image edits need an attachment ID or project asset reference.",
        )?;
    }
    let mask = node
        .attr("data-desegnos-mask")
        .map(|value| value.to_string());
    ensure(
        mask.is_none() || source.is_some(),
        "An image mask requires a source image.",
    )?;
    if let Some(mask) = &mask {
        let id = mask
            .strip_prefix(crate::PROJECT_ASSET_SCHEME)
            .unwrap_or(mask);
        ensure(
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "Image masks need an attachment ID or project asset reference.",
        )?;
    }
    let size = node
        .attr("data-desegnos-size")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "1024x1024".into());
    ensure(size == "1024x1024", "Unsupported image generation size.")?;
    let background = node
        .attr("data-desegnos-background")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "opaque".into());
    ensure(
        matches!(background.as_str(), "opaque" | "transparent"),
        "Unsupported image background.",
    )?;
    ensure(
        node.attr("data-desegnos-download").is_none()
            && node.attr("data-desegnos-attachment").is_none(),
        "An image cannot combine native image request markers.",
    )?;
    Ok(ImageGenerationRequest {
        prompt,
        source,
        mask,
        size,
        transparent: background == "transparent",
    })
}

impl GenerationCandidate {
    pub fn image_generation_requests(&self) -> Result<Vec<ImageGenerationRequest>> {
        let mut requests = BTreeSet::new();
        let mut placements = 0;
        for page in &self.pages {
            let doc = editing::parse(&page.content.html)?;
            for node in doc.select("[data-desegnos-generate]").nodes() {
                ensure(
                    node.node_name().as_deref() == Some("img"),
                    "Only img elements can request image generation.",
                )?;
                placements += 1;
                ensure(placements <= 64, "Too many generated image placements.")?;
                requests.insert(read(*node)?);
            }
        }
        Ok(requests.into_iter().collect())
    }

    /// Missing results (including requests beyond the paid count) become local visible slots.
    pub fn embed_generated_images(
        &mut self,
        images: &BTreeMap<ImageGenerationRequest, String>,
    ) -> Result<usize> {
        ensure(images.len() <= 2, "Too many generated images.")?;
        let placeholder = format!("data:image/svg+xml;base64,{}", STANDARD.encode(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><rect width="640" height="360" fill="#e5e7eb"/><text x="320" y="180" fill="#374151" text-anchor="middle" font-family="sans-serif" font-size="24">Image generation failed</text></svg>"##
        ));
        let mut missing = 0;
        for page in &mut self.pages {
            let doc = editing::parse(&page.content.html)?;
            for node in doc.select("img[data-desegnos-generate]").nodes() {
                let request = read(*node)?;
                node.remove_attr("data-desegnos-generate");
                node.remove_attr("data-desegnos-source");
                node.remove_attr("data-desegnos-mask");
                node.remove_attr("data-desegnos-size");
                node.remove_attr("data-desegnos-background");
                node.remove_attr("srcset");
                node.remove_attr("sizes");
                node.set_attr("data-desegnos-generated-prompt", &request.prompt);
                node.set_attr("data-desegnos-generated-job", &self.job_id.to_string());
                let image = images.get(&request).filter(|uri| {
                    uri.strip_prefix("data:image/png;base64,")
                        .is_some_and(|encoded| {
                            encoded.len() <= crate::MAX_DOWNLOADED_IMAGE_BYTES.div_ceil(3) * 4
                                && STANDARD.decode(encoded).is_ok_and(|bytes| {
                                    bytes.len() <= crate::MAX_DOWNLOADED_IMAGE_BYTES
                                        && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                                        && STANDARD.encode(bytes) == encoded
                                })
                        })
                });
                if let Some(image) = image {
                    node.set_attr("src", image);
                } else {
                    node.set_attr("src", &placeholder);
                    node.set_attr("data-desegnos-placeholder", "image");
                    node.set_attr(
                        "alt",
                        "Image generation failed. Try again or replace this image.",
                    );
                    missing += 1;
                }
            }
            page.content.html = editing::serialized(&doc)?;
            page.content.validate()?;
        }
        Ok(missing)
    }
}
