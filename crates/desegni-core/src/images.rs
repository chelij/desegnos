//! Portable image substitution. Networking and raster decoding belong to adapters.
use crate::{GenerationCandidate, Result, editing, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_DOWNLOADED_IMAGES: usize = 8;
pub const MAX_DOWNLOADED_IMAGE_BYTES: usize = 256 * 1024;

impl GenerationCandidate {
    /// Only explicitly marked image elements authorize a download. Other URLs,
    /// CSS, scripts, links, and unmarked images never grant network authority.
    pub fn image_download_urls(&self) -> Result<Vec<String>> {
        let mut urls = BTreeSet::new();
        for page in &self.pages {
            if !page
                .content
                .html
                .to_ascii_lowercase()
                .contains("data-desegnos-download")
            {
                continue;
            }
            let doc = editing::parse(&page.content.html)?;
            for node in doc.select("img[data-desegnos-download]").nodes() {
                urls.insert(node.attr("src").unwrap_or_default().to_string());
            }
        }
        Ok(urls.into_iter().take(MAX_DOWNLOADED_IMAGES).collect())
    }

    /// Embed validated bytes before accepting a candidate. Failed downloads,
    /// excess requests and images that exceed the existing source budget remain
    /// honest, visible placeholders. The source URL stays with each image.
    pub fn embed_downloaded_images(&mut self, images: &BTreeMap<String, String>) -> Result<usize> {
        let placeholder = format!("data:image/svg+xml;base64,{}", STANDARD.encode(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360" viewBox="0 0 640 360"><rect width="640" height="360" fill="#e5e7eb"/><rect x="20" y="20" width="600" height="320" rx="12" fill="none" stroke="#6b7280" stroke-dasharray="8 6"/><g fill="#374151" text-anchor="middle" font-family="sans-serif"><text x="320" y="190" font-size="26">Image placeholder</text></g></svg>"##
        ));
        let mut missing = 0;
        let mut references = 0;
        for page in &mut self.pages {
            if !page
                .content
                .html
                .to_ascii_lowercase()
                .contains("data-desegnos-download")
            {
                continue;
            }
            let doc = editing::parse(&page.content.html)?;
            let nodes = doc.select("img[data-desegnos-download]");
            references += nodes.nodes().len();
            ensure(
                references <= 64,
                "A draft can include at most 64 online image placements.",
            )?;
            for node in nodes.nodes() {
                let url = node.attr("src").unwrap_or_default().to_string();
                node.remove_attr("data-desegnos-download");
                node.remove_attr("srcset");
                node.remove_attr("sizes");
                node.set_attr("data-desegnos-image-source", &url);
                node.set_attr("src", &placeholder);
                node.set_attr("data-desegnos-placeholder", "image");
                let alt = node.attr("alt").unwrap_or_default().to_string();
                node.set_attr("alt", &format!("Image placeholder: {alt}"));
                if let Some(data) = images.get(&url) {
                    // This interface accepts embedded raster data only. It can
                    // never introduce executable SVG or a second remote fetch.
                    let encoded = ["data:image/png;base64,", "data:image/jpeg;base64,"]
                        .iter()
                        .find_map(|prefix| data.strip_prefix(prefix));
                    let valid = encoded.is_some_and(|value| {
                        value.len() <= MAX_DOWNLOADED_IMAGE_BYTES.div_ceil(3) * 4
                            && STANDARD.decode(value).is_ok_and(|bytes| {
                                !bytes.is_empty()
                                    && bytes.len() <= MAX_DOWNLOADED_IMAGE_BYTES
                                    && STANDARD.encode(bytes) == value
                            })
                    });
                    if valid {
                        node.set_attr("src", data);
                        let html = editing::serialized(&doc)?;
                        let mut trial = page.content.clone();
                        trial.html = html;
                        if trial.validate().is_ok() {
                            node.remove_attr("data-desegnos-placeholder");
                            node.set_attr("alt", &alt);
                            continue;
                        }
                        node.set_attr("src", &placeholder);
                    }
                }
                missing += 1;
            }
            page.content.html = editing::serialized(&doc)?;
            page.content.validate()?;
        }
        Ok(missing)
    }
}
