//! Project image bytes stay in Desegnos; the agent returns stable references.
use crate::{Error, GenerationCandidate, Project, Result, SiteContent, ensure};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const PROJECT_ASSET_SCHEME: &str = "desegnos-asset:";

pub fn project_asset_reference(id: Uuid) -> String {
    format!("{PROJECT_ASSET_SCHEME}{id}")
}

pub(crate) fn inline_images(source: &str) -> impl Iterator<Item = &str> {
    source
        .match_indices("data:image/")
        .filter_map(move |(start, _)| {
            let rest = &source[start..];
            let end = rest
                .find(|c: char| matches!(c, '"' | '\'' | ')' | '<' | '>') || c.is_whitespace())
                .unwrap_or(rest.len());
            let uri = &rest[..end];
            let (header, _) = uri.split_once(',')?;
            (header.ends_with(";base64")).then_some(uri)
        })
}

pub(crate) fn inline_image_id(uri: &str) -> Uuid {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(uri.as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_custom_bytes(bytes).into_uuid()
}

impl Project {
    /// Pull inline pictures into a shared, content-addressed store before history/recovery.
    /// Placed pictures stay available even when a later import replaces the source catalogue.
    /// Repeated execution is idempotent.
    pub fn store_inline_images(&mut self) -> Result<()> {
        let mut pictures = self.pictures.clone();
        let mut pages = self.pages.clone();
        for page in &mut pages {
            if let Some(content) = &mut page.content {
                let sources = [&content.html, &content.css, &content.javascript];
                let needs_store = sources.iter().any(|source| {
                    source.contains("data:image/")
                        || source
                            .match_indices(PROJECT_ASSET_SCHEME)
                            .any(|(start, _)| {
                                let start = start + PROJECT_ASSET_SCHEME.len();
                                source
                                    .get(start..start + 36)
                                    .and_then(|id| Uuid::parse_str(id).ok())
                                    .is_none_or(|id| !pictures.contains_key(&id))
                            })
                });
                if !needs_store {
                    continue;
                }
                let mut resolved = self.resolve_site_content(content)?;
                for source in [
                    &mut resolved.html,
                    &mut resolved.css,
                    &mut resolved.javascript,
                ] {
                    // A broken picture (e.g. truncated Base64 in older pages) stays inline as
                    // it was; it must not make the whole project unloadable.
                    let uris: Vec<_> = inline_images(source)
                        .filter(|uri| valid_picture(uri))
                        .map(str::to_owned)
                        .collect();
                    for uri in uris {
                        let id = inline_image_id(&uri);
                        pictures
                            .entry(id)
                            .or_insert_with(|| std::sync::Arc::from(uri.as_str()));
                        *source = source.replace(&uri, &project_asset_reference(id));
                    }
                }
                *content = resolved;
            }
        }
        // Pictures no page uses any more are dropped; Undo keeps its own copy of the store.
        let mut used = referenced_ids(&pages);
        if let Some(site) = &self.website {
            used.extend(site.files.values().filter_map(|v| {
                v.strip_prefix(PROJECT_ASSET_SCHEME)
                    .and_then(|id| Uuid::parse_str(id).ok())
            }));
        }
        pictures.retain(|id, _| used.contains(id));
        validate_pictures(&pictures)?;
        self.pictures = pictures;
        self.pages = pages;
        Ok(())
    }

    /// Runs on every edit, so it only checks that each reference is known. Picture bytes are
    /// verified when they enter the store and when a project is opened.
    pub(crate) fn validate_picture_store(&self) -> Result<()> {
        let identity = self.imported_identity.as_ref();
        let mut inline = None;
        for page in &self.pages {
            let Some(content) = &page.content else {
                continue;
            };
            for source in [&content.html, &content.css, &content.javascript] {
                for (start, _) in source.match_indices(PROJECT_ASSET_SCHEME) {
                    let start = start + PROJECT_ASSET_SCHEME.len();
                    let id = source
                        .get(start..start + 36)
                        .and_then(|value| Uuid::parse_str(value).ok())
                        .ok_or_else(|| Error("An image reference is invalid.".into()))?;
                    let known = self.pictures.contains_key(&id)
                        || identity.is_some_and(|identity| {
                            identity.assets.iter().any(|asset| asset.id == id)
                                || identity.photos.iter().any(|photo| photo.id == id)
                        })
                        || inline
                            .get_or_insert_with(|| self.inline_page_images())
                            .contains_key(&id);
                    ensure(
                        known,
                        "An image reference is not in this project. Your page is unchanged.",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Full integrity check of the stored picture bytes.
    pub(crate) fn verify_picture_bytes(&self) -> Result<()> {
        validate_pictures(&self.pictures)
    }

    /// Resolve at the rendering/export boundary; accepted source and its hashes stay small.
    pub fn resolve_site_content(&self, content: &SiteContent) -> Result<SiteContent> {
        let mut result = content.clone();
        let mut inline = None;
        for source in [&mut result.html, &mut result.css, &mut result.javascript] {
            let mut output = String::new();
            let mut rest = source.as_str();
            while let Some(index) = rest.find(PROJECT_ASSET_SCHEME) {
                output.push_str(&rest[..index]);
                rest = &rest[index + PROJECT_ASSET_SCHEME.len()..];
                let id = rest
                    .get(..36)
                    .and_then(|value| Uuid::parse_str(value).ok())
                    .ok_or_else(|| Error("An image reference is invalid.".into()))?;
                let identity = self.imported_identity.as_ref();
                let uri = self
                    .pictures
                    .get(&id)
                    .map(|uri| &**uri)
                    .or_else(|| {
                        identity?
                            .assets
                            .iter()
                            .find(|asset| asset.id == id)
                            .map(|asset| asset.data_uri.as_str())
                    })
                    .or_else(|| {
                        identity?
                            .photos
                            .iter()
                            .find(|photo| photo.id == id)
                            .map(|photo| &*photo.data_uri)
                    })
                    .or_else(|| {
                        inline
                            .get_or_insert_with(|| self.inline_page_images())
                            .get(&id)
                            .copied()
                    })
                    .ok_or_else(|| {
                        Error(
                            "An image reference is not in this project. Your page is unchanged."
                                .into(),
                        )
                    })?;
                output.push_str(uri);
                rest = &rest[36..];
            }
            output.push_str(rest);
            *source = output;
        }
        Ok(result)
    }

    /// Images actually referenced by the affected pages, including inline and stored pictures.
    pub(crate) fn generation_picture_catalogue(&self, page_ids: &[Uuid]) -> serde_json::Value {
        let mut urls = BTreeSet::new();
        for page in self.pages.iter().filter(|page| page_ids.contains(&page.id)) {
            if let Some(content) = &page.content {
                let content = self.all_asset_references(content);
                for source in [&content.html, &content.css, &content.javascript] {
                    for (start, _) in source.match_indices(PROJECT_ASSET_SCHEME) {
                        let start = start + PROJECT_ASSET_SCHEME.len();
                        if let Some(id) = source
                            .get(start..start + 36)
                            .and_then(|id| Uuid::parse_str(id).ok())
                        {
                            urls.insert(project_asset_reference(id));
                        }
                    }
                }
            }
        }
        serde_json::json!(
            urls.into_iter()
                .map(|url| serde_json::json!({"url":url}))
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

    /// Format 16: page assignments are kept out of the global picture/work catalogue.
    pub fn generation_page_catalogue(&self, page_ids: &[Uuid]) -> Option<serde_json::Value> {
        self.imported_identity.as_ref().map(|identity| {
            let mut catalogue = self.generation_asset_catalogue().expect("identity catalogue");
            catalogue["photos"] = serde_json::json!(identity.photos.iter().filter(|photo| !identity.pages.iter().any(|page| page.photo_ids.contains(&photo.id))).map(|photo| serde_json::json!({"name":photo.name,"width":photo.width,"height":photo.height,"description":photo.description,"url":project_asset_reference(photo.id)})).collect::<Vec<_>>());
            catalogue["content"] = serde_json::json!(identity.content.iter().filter(|item| !identity.pages.iter().any(|page| page.content.contains(item))).collect::<Vec<_>>());
            catalogue["pages"] = serde_json::json!(self.pages.iter().filter(|page| page_ids.contains(&page.id)).filter_map(|page| {
                let contexts: Vec<_> = identity.pages.iter().filter(|context| context.page_id == page.id).collect();
                if contexts.is_empty() { return None; }
                Some(serde_json::json!({"pageId":page.id,"name":page.name,"path":page.path,
                    "sources":contexts.iter().map(|context| &context.source).collect::<Vec<_>>(),
                    "photos":identity.photos.iter().filter(|photo| contexts.iter().any(|context| context.photo_ids.contains(&photo.id))).map(|photo| serde_json::json!({"name":photo.name,"width":photo.width,"height":photo.height,"description":photo.description,"url":project_asset_reference(photo.id)})).collect::<Vec<_>>(),
                    "content":contexts.iter().flat_map(|context| &context.content).collect::<Vec<_>>()
                }))
            }).collect::<Vec<_>>());
            catalogue
        })
    }

    /// Presentation of known image URLs changes; dependency hashes continue to
    /// identify the exact accepted source and original asset bytes.
    pub fn generation_source_with_asset_references(&self, source: &SiteContent) -> SiteContent {
        self.asset_references(source, false)
    }

    pub fn all_asset_references(&self, source: &SiteContent) -> SiteContent {
        self.asset_references(source, true)
    }

    fn asset_references(&self, source: &SiteContent, all: bool) -> SiteContent {
        let mut content = source.clone();
        if let Some(identity) = &self.imported_identity {
            for asset in &identity.assets {
                for source in [&mut content.html, &mut content.css, &mut content.javascript] {
                    *source = source.replace(&asset.data_uri, &project_asset_reference(asset.id));
                }
            }
            for photo in &identity.photos {
                for source in [&mut content.html, &mut content.css, &mut content.javascript] {
                    *source = source.replace(&*photo.data_uri, &project_asset_reference(photo.id));
                }
            }
        }
        for source in [&mut content.html, &mut content.css, &mut content.javascript] {
            let uris: Vec<String> = inline_images(source)
                .filter(|uri| all || uri.len() >= 1024)
                .map(str::to_owned)
                .collect();
            for uri in uris {
                *source = source.replace(&uri, &project_asset_reference(inline_image_id(&uri)));
            }
        }
        content
    }

    /// Pictures already inside this project's pages, by the reference they are sent as.
    fn inline_page_images(&self) -> BTreeMap<Uuid, &str> {
        self.pages
            .iter()
            .filter_map(|page| page.content.as_ref())
            .flat_map(|content| [&content.html, &content.css, &content.javascript])
            .flat_map(|source| inline_images(source))
            .map(|uri| (inline_image_id(uri), uri))
            .collect()
    }
}

impl GenerationCandidate {
    pub(crate) fn validate_project_images(
        &self,
        project: &Project,
        references: bool,
    ) -> Result<()> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        for page in &self.pages {
            ensure(
                references
                    || ![
                        &page.content.html,
                        &page.content.css,
                        &page.content.javascript,
                    ]
                    .iter()
                    .any(|source| source.contains(PROJECT_ASSET_SCHEME)),
                "A project image was not resolved. Your page is unchanged.",
            )?;
            let content = project.resolve_site_content(&page.content)?;
            let document = crate::editing::parse(&content.html)?;
            for node in document.select("img[src]").nodes() {
                let Some(src) = node.attr("src") else {
                    continue;
                };
                if src.starts_with("data:image/")
                    && let Some((header, encoded)) = src.split_once(',')
                    && header.ends_with(";base64")
                {
                    ensure(
                        STANDARD
                            .decode(encoded)
                            .is_ok_and(|bytes| STANDARD.encode(bytes) == encoded),
                        "The AI returned damaged image data. Your page is unchanged.",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Resolve only IDs already frozen with the project, atomically and without
    /// network or filesystem access. Unknown IDs never become guessed files.
    pub fn resolve_project_assets(&mut self, project: &Project) -> Result<usize> {
        let mut pages = self.pages.clone();
        let mut count = 0;
        for page in &mut pages {
            count += [
                &page.content.html,
                &page.content.css,
                &page.content.javascript,
            ]
            .iter()
            .map(|source| source.matches(PROJECT_ASSET_SCHEME).count())
            .sum::<usize>();
            page.content = project.resolve_site_content(&page.content)?;
        }
        let mut resolved = self.clone();
        resolved.pages = pages;
        resolved.validate_bounds()?;
        self.pages = resolved.pages;
        Ok(count)
    }
}

fn referenced_ids(pages: &[crate::Page]) -> BTreeSet<Uuid> {
    pages
        .iter()
        .filter_map(|page| page.content.as_ref())
        .flat_map(|content| [&content.html, &content.css, &content.javascript])
        .flat_map(|source| {
            source
                .match_indices(PROJECT_ASSET_SCHEME)
                .filter_map(|(start, _)| {
                    let start = start + PROJECT_ASSET_SCHEME.len();
                    Uuid::parse_str(source.get(start..start + 36)?).ok()
                })
        })
        .collect()
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
