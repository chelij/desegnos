//! Project image bytes stay in Desegnos; elements and reference items reference them by id.
use crate::{AttachmentKind, Project, Result, ensure};
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
    /// A picture id resolves from the shared project store or an imported identity catalogue;
    /// its bytes stay in the project.
    pub(crate) fn picture_uri(&self, id: Uuid) -> Option<std::sync::Arc<str>> {
        self.pictures.get(&id).cloned().or_else(|| {
            let identity = self.imported_identity.as_ref()?;
            identity
                .assets
                .iter()
                .find(|asset| asset.id == id)
                .map(|asset| std::sync::Arc::from(asset.data_uri.as_str()))
                .or_else(|| {
                    identity
                        .photos
                        .iter()
                        .find(|photo| photo.id == id)
                        .map(|photo| photo.data_uri.clone())
                })
        })
    }

    pub fn store_inline_images(&mut self) -> Result<()> {
        let mut pictures = self.pictures.clone();
        let mut used = referenced_ids(&self.pages, &self.reference);
        for definition in self.linked.values() {
            collect_element_pictures(std::slice::from_ref(&definition.master), &mut used);
        }
        pictures.retain(|id, _| used.contains(id));
        validate_pictures(&pictures)?;
        self.pictures = pictures;
        Ok(())
    }

    /// Runs on every edit, so it only checks that each reference is known. Picture bytes are
    /// verified when they enter the store and when a project is opened.
    pub(crate) fn validate_picture_store(&self) -> Result<()> {
        let mut referenced = referenced_ids(&self.pages, &self.reference);
        for definition in self.linked.values() {
            collect_element_pictures(std::slice::from_ref(&definition.master), &mut referenced);
        }
        for id in referenced {
            let known = self.picture_uri(id).is_some();
            ensure(
                known,
                "An image in the project refers to a picture that is not stored.",
            )?;
        }
        Ok(())
    }

    pub fn picture_bytes(&self, id: Uuid) -> Option<(String, Vec<u8>)> {
        let uri = self.picture_uri(id)?;
        let (mime, bytes) = crate::limits::decode_image_data_uri(&uri).ok()?;
        Some((mime.to_owned(), bytes))
    }
}

fn referenced_ids(pages: &[crate::Page], reference: &[crate::ReferenceItem]) -> BTreeSet<Uuid> {
    let mut ids = BTreeSet::new();
    for item in reference {
        if let crate::ReferenceKind::Element { element, .. } = &item.kind {
            collect_element_pictures(std::slice::from_ref(element.as_ref()), &mut ids);
        }
        if let crate::ReferenceKind::Image { picture, .. } = &item.kind {
            ids.insert(*picture);
        }
    }
    for page in pages {
        collect_element_pictures(&page.elements, &mut ids);
    }
    ids
}

pub(crate) fn collect_element_pictures(elements: &[crate::Element], ids: &mut BTreeSet<Uuid>) {
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
        store_picture_uri(uri)?;
        let scaled =
            crate::scaled_for_display(uri, display_width)?.unwrap_or_else(|| uri.to_owned());
        let id = store_picture_uri(&scaled)?;
        Ok((id, scaled))
    }
}

pub(crate) fn store_picture_uri(uri: &str) -> Result<Uuid> {
    ensure(valid_picture(uri), "Drop a PNG, JPEG, WebP or GIF picture.")?;
    Ok(inline_image_id(uri))
}

fn valid_picture(uri: &str) -> bool {
    crate::limits::decode_image_data_uri(uri).is_ok()
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

impl crate::Project {
    /// Promote the frozen requests' original pictures before validating or applying their
    /// generated elements. The live project is unchanged until the whole reply is accepted.
    pub(crate) fn store_request_pictures(&mut self, request_ids: &[uuid::Uuid]) -> Result<()> {
        for request in self
            .requests
            .iter()
            .filter(|request| request_ids.contains(&request.id))
        {
            for attachment in &request.attachments {
                if attachment.kind == AttachmentKind::Image
                    && let Some(uri) = &attachment.data_uri
                {
                    let id = crate::project_assets::store_picture_uri(uri)?;
                    self.pictures.insert(id, uri.clone());
                }
            }
        }
        Ok(())
    }
}
