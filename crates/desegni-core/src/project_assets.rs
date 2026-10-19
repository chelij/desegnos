//! Project image bytes stay in Desegnos; the agent returns stable references.
use crate::{Error, GenerationCandidate, Project, Result, SiteContent, ensure};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const PROJECT_ASSET_SCHEME: &str = "desegnos-asset:";

pub fn project_asset_reference(id: Uuid) -> String {
    format!("{PROJECT_ASSET_SCHEME}{id}")
}

impl Project {
    pub fn generation_asset_catalogue(&self) -> Option<serde_json::Value> {
        self.imported_identity.as_ref().map(|identity| serde_json::json!({
            "sourceName":identity.source_name,"sourceSha256":identity.source_sha256,"summary":identity.summary,
            "assets":identity.assets.iter().map(|asset| serde_json::json!({
                "id":asset.id,"name":asset.name,"mime":asset.mime,"sha256":asset.sha256,"url":project_asset_reference(asset.id)
            })).collect::<Vec<_>>()
        }))
    }

    /// Presentation of known image URLs changes; dependency hashes continue to
    /// identify the exact accepted source and original asset bytes.
    pub fn generation_source_with_asset_references(&self, source: &SiteContent) -> SiteContent {
        let mut content = source.clone();
        if let Some(identity) = &self.imported_identity {
            for asset in &identity.assets {
                for source in [&mut content.html, &mut content.css, &mut content.javascript] {
                    *source = source.replace(&asset.data_uri, &project_asset_reference(asset.id));
                }
            }
        }
        content
    }
}

impl GenerationCandidate {
    pub(crate) fn validate_project_images(&self) -> Result<()> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        for page in &self.pages {
            ensure(
                ![
                    &page.content.html,
                    &page.content.css,
                    &page.content.javascript,
                ]
                .iter()
                .any(|source| source.contains(PROJECT_ASSET_SCHEME)),
                "A project image was not resolved. Your page is unchanged.",
            )?;
            let document = crate::editing::parse(&page.content.html)?;
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
        let assets: BTreeMap<_, _> = project
            .imported_identity
            .iter()
            .flat_map(|identity| &identity.assets)
            .map(|asset| (asset.id, asset))
            .collect();
        let mut pages = self.pages.clone();
        let mut count = 0;
        for page in &mut pages {
            for source in [
                &mut page.content.html,
                &mut page.content.css,
                &mut page.content.javascript,
            ] {
                if !source.contains(PROJECT_ASSET_SCHEME) {
                    continue;
                }
                let mut output = String::with_capacity(source.len());
                let mut rest = source.as_str();
                while let Some(index) = rest.find(PROJECT_ASSET_SCHEME) {
                    output.push_str(&rest[..index]);
                    rest = &rest[index + PROJECT_ASSET_SCHEME.len()..];
                    let id = rest.get(..36).and_then(|value| Uuid::parse_str(value).ok())
                        .ok_or_else(|| Error("The AI returned an invalid project image reference. Your page is unchanged.".into()))?;
                    let asset = assets.get(&id).ok_or_else(|| Error("The AI referred to an image that is not in this project. Your page is unchanged.".into()))?;
                    asset.validate()?;
                    output.push_str(&asset.data_uri);
                    rest = &rest[36..];
                    count += 1;
                    ensure(
                        output.len() <= crate::MAX_SITE_SOURCE_BYTES,
                        "The page exceeds its image/source budget.",
                    )?;
                }
                output.push_str(rest);
                *source = output;
            }
            page.content.validate()?;
        }
        let mut resolved = self.clone();
        resolved.pages = pages;
        resolved.validate_bounds()?;
        self.pages = resolved.pages;
        Ok(count)
    }
}
