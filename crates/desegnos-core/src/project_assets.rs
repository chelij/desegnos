//! Project image bytes stay in Desegnos; elements and reference items reference them by id.
use crate::{Project, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const PROJECT_ASSET_SCHEME: &str = "desegnos-asset:";

pub fn project_asset_reference(id: Uuid) -> String {
    format!("{PROJECT_ASSET_SCHEME}{id}")
}

pub(crate) fn inline_image_id(uri: &str) -> Uuid {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(uri.as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_custom_bytes(bytes).into_uuid()
}

impl Project {
    /// Drop pictures nothing on the canvas uses any more. Idempotent, so session setup and
    /// every edit can call it.
    pub fn store_inline_images(&mut self) -> Result<()> {
        let mut pictures = self.pictures.clone();
        let used = referenced_ids(&self.pages, &self.reference);
        pictures.retain(|id, _| used.contains(id));
        validate_pictures(&pictures)?;
        self.pictures = pictures;
        Ok(())
    }

    /// Runs on every edit, so it only checks that each reference is known. Picture bytes are
    /// verified when they enter the store and when a project is opened.
    pub(crate) fn validate_picture_store(&self) -> Result<()> {
        let identity = self.imported_identity.as_ref();
        let mut referenced = referenced_ids(&self.pages, &self.reference);
        for definition in self.linked.values() {
            collect_element_pictures(std::slice::from_ref(&definition.master), &mut referenced);
        }
        for id in referenced {
            let known = self.pictures.contains_key(&id)
                || identity.is_some_and(|identity| {
                    identity.assets.iter().any(|asset| asset.id == id)
                        || identity.photos.iter().any(|photo| photo.id == id)
                });
            ensure(
                known,
                "An image in the project refers to a picture that is not stored.",
            )?;
        }
        Ok(())
    }

    /// A stored picture's media type and bytes, for the direct renderer and the exporter.
    pub fn picture_bytes(&self, id: Uuid) -> Option<(String, Vec<u8>)> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let uri = self.picture_uri(id)?;
        let (header, encoded) = uri.strip_prefix("data:")?.split_once(";base64,")?;
        let mime = header.to_owned();
        STANDARD.decode(encoded).ok().map(|bytes| (mime, bytes))
    }

    /// Pictures used by the given boundaries, with their stable reference.
    pub(crate) fn generation_picture_catalogue(&self, page_ids: &[Uuid]) -> serde_json::Value {
        let mut ids = BTreeSet::new();
        for page in self.pages.iter().filter(|page| page_ids.contains(&page.id)) {
            let elements = crate::resolve_elements(self, &page.elements);
            collect_element_pictures(&elements, &mut ids);
        }
        serde_json::json!(
            ids.into_iter()
                .map(|id| serde_json::json!({"url": project_asset_reference(id)}))
                .collect::<Vec<_>>()
        )
    }

    pub fn generation_asset_catalogue(&self) -> Option<serde_json::Value> {
        self.imported_identity.as_ref().map(|identity| serde_json::json!({
            "sourceName":identity.source_name,"sourceSha256":identity.source_sha256,"summary":identity.summary,
            "assets":identity.assets.iter().map(|asset| serde_json::json!({
                "id":asset.id,"name":asset.name,"mime":asset.mime,"sha256":asset.sha256,"url":project_asset_reference(asset.id)
            })).collect::<Vec<_>>(),
            "photos":identity.photos.iter().map(|photo| serde_json::json!({
                "name":photo.name,"width":photo.width,"height":photo.height,"description":photo.description,"url":project_asset_reference(photo.id)
            })).collect::<Vec<_>>(),
            "content":identity.content
        }))
    }

    /// The imported brand catalogue for the boundaries this request touches.
    pub fn generation_page_catalogue(&self, _page_ids: &[Uuid]) -> Option<serde_json::Value> {
        self.generation_asset_catalogue()
    }
}

fn referenced_ids(pages: &[crate::Page], reference: &[crate::ReferenceItem]) -> BTreeSet<Uuid> {
    let mut ids = BTreeSet::new();
    for item in reference {
        if let crate::ReferenceKind::Image { picture, .. } = &item.kind {
            ids.insert(*picture);
        }
    }
    for page in pages {
        collect_element_pictures(&page.elements, &mut ids);
    }
    ids
}

fn collect_element_pictures(elements: &[crate::Element], ids: &mut BTreeSet<Uuid>) {
    for element in elements {
        if let Some(picture) = element.image {
            ids.insert(picture);
        }
        collect_element_pictures(&element.children, ids);
    }
}

impl Project {
    /// Validate and optionally scale a picture before it enters the project store. Returns the
    /// id and the bytes that were actually stored, so the caller can build the picture map.
    pub fn prepare_picture(uri: &str, display_width: u32) -> Result<(Uuid, String)> {
        let scaled = crate::scaled_for_display(uri, display_width).unwrap_or_else(|| uri.to_owned());
        let id = store_picture_uri(&scaled)?;
        Ok((id, scaled))
    }
}

/// Validate picture bytes once and return their content-addressed identifier.
pub(crate) fn store_picture_uri(uri: &str) -> Result<Uuid> {
    ensure(
        valid_picture(uri),
        "Drop a PNG, JPEG, WebP or GIF picture.",
    )?;
    Ok(inline_image_id(uri))
}

fn valid_picture(uri: &str) -> bool {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let Some((header, encoded)) = uri.split_once(',') else {
        return false;
    };
    header.starts_with("data:image/")
        && header.ends_with(";base64")
        && STANDARD
            .decode(encoded)
            .is_ok_and(|bytes| !bytes.is_empty() && STANDARD.encode(bytes) == encoded)
}

fn validate_pictures(pictures: &BTreeMap<Uuid, std::sync::Arc<str>>) -> Result<()> {
    for (id, uri) in pictures {
        ensure(
            valid_picture(uri) && *id == inline_image_id(uri),
            "A project picture failed its integrity check.",
        )?;
    }
    Ok(())
}
