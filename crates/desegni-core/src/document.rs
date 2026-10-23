//! Editable documents shown in a page's iframes: a project page or an imported site HTML file.
use crate::{Error, Project, Result, SiteContent, Viewport, digest, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use dom_query::Document;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// A document the editor and the AI may change. Pages carry their project ID; imported site
/// HTML files carry their original site path and are stored in the project's website bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DocumentTarget {
    Page { page_id: Uuid },
    SiteFile { path: String },
}

impl DocumentTarget {
    pub fn page(page_id: Uuid) -> Self {
        Self::Page { page_id }
    }

    /// Stable dependency key, for example `page:<uuid>` or `site:play/index.html`.
    pub fn key(&self) -> String {
        match self {
            Self::Page { page_id } => format!("page:{page_id}"),
            Self::SiteFile { path } => format!("site:{path}"),
        }
    }

    pub fn from_key(key: &str) -> Result<Self> {
        if let Some(id) = key.strip_prefix("page:") {
            return Uuid::parse_str(id)
                .map(Self::page)
                .map_err(|_| Error("Invalid document dependency.".into()));
        }
        if let Some(path) = key.strip_prefix("site:") {
            ensure(!path.is_empty(), "Invalid document dependency.".to_string())?;
            return Ok(Self::SiteFile { path: path.into() });
        }
        Err(Error("Invalid document dependency.".into()))
    }

    pub fn site_path(&self) -> Option<&str> {
        match self {
            Self::SiteFile { path } => Some(path),
            Self::Page { .. } => None,
        }
    }
}

/// One request's box inside a document shown by an iframe on its host page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrameTarget {
    /// The page that contains the iframe.
    pub page_id: Uuid,
    /// The iframe's `data-desegnos-id` on the host page.
    pub iframe_id: String,
    /// The document shown inside the iframe.
    pub document: DocumentTarget,
    /// The request box, mapped into the framed document's CSS pixels.
    pub rect: crate::Rect,
}

impl FrameTarget {
    pub fn validate(&self, project: &Project) -> Result<()> {
        ensure(
            project.pages.iter().any(|page| page.id == self.page_id),
            "The framed request's host page no longer exists.",
        )?;
        ensure(
            crate::stable_targets::valid_id(&self.iframe_id),
            "The framed request needs the iframe's editor ID.",
        )?;
        self.rect
            .validate(project.document_viewport(&self.document)?)?;
        match &self.document {
            DocumentTarget::Page { page_id } => {
                ensure(
                    *page_id != self.page_id,
                    "A framed request must target another document.",
                )?;
                project.page(*page_id)?;
            }
            DocumentTarget::SiteFile { path } => {
                ensure(
                    project.is_editable_site_file(path),
                    "That framed document is not an editable site HTML file.",
                )?;
            }
        }
        Ok(())
    }
}

/// How an iframe `src` resolves on a page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FrameSource {
    Page { page_id: Uuid },
    SiteFile { path: String },
    External { url: String },
    Unresolvable { src: String },
}

/// One iframe on a page and what its `src` points at.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageFrameTarget {
    pub iframe_id: String,
    pub src: String,
    pub source: FrameSource,
    /// The editable document when the source resolves to one; the editor's edit target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentTarget>,
    /// Fingerprint for selecting an element before the document has editor IDs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
}

fn is_html_path(path: &str) -> bool {
    matches!(
        path.rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "html" | "htm"
    )
}

fn looks_like_document(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.contains("<html") || lower.contains("<!doctype")
}

fn push_segment(part: &str, parts: &mut Vec<String>) -> Option<()> {
    match part {
        "" | "." => Some(()),
        ".." => {
            parts.pop()?;
            Some(())
        }
        part if part.contains('\\') || part.contains('\0') => None,
        part => {
            parts.push(part.to_string());
            Some(())
        }
    }
}

/// Resolve a URL path against a base file path, without accepting traversal outside the site.
pub(crate) fn join_site_path(base: &str, value: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut relative_parts = value.split('/');
    if !value.starts_with('/') {
        let directory = base.rsplit_once('/').map_or("", |(directory, _)| directory);
        for part in directory.split('/') {
            push_segment(part, &mut parts)?;
        }
    } else {
        relative_parts.next();
    }
    for part in relative_parts {
        push_segment(part, &mut parts)?;
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

impl Project {
    /// True when this site path is an HTML file the editor may change (not a page file).
    pub fn is_editable_site_file(&self, path: &str) -> bool {
        let Some(site) = &self.website else {
            return false;
        };
        is_html_path(path)
            && site.files.contains_key(path)
            && !site.page_files.values().any(|source| source == path)
    }

    pub fn site_document_paths(&self) -> Vec<String> {
        let mut paths: Vec<_> = self
            .website
            .as_ref()
            .map(|site| {
                site.files
                    .keys()
                    .filter(|path| self.is_editable_site_file(path))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        paths.sort();
        paths
    }

    /// The stored bytes of one site file, decoded from the project's portable representation.
    pub fn site_file_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let site = self
            .website
            .as_ref()
            .ok_or_else(|| Error("This project has no imported website.".into()))?;
        let encoded = site
            .files
            .get(path)
            .ok_or_else(|| Error("That website file no longer exists.".into()))?;
        ensure(
            !encoded.starts_with(crate::PROJECT_ASSET_SCHEME),
            "That website file is a stored picture, not an editable document.",
        )?;
        let resolved = self.resolve_site_content(&SiteContent {
            html: encoded.to_string(),
            css: String::new(),
            javascript: String::new(),
        })?;
        let value = resolved
            .html
            .strip_prefix("data:")
            .and_then(|value| value.split_once(";base64,"))
            .map_or(resolved.html.as_str(), |(_, value)| value);
        STANDARD
            .decode(value)
            .map_err(|_| Error("Invalid imported website resource.".into()))
    }

    pub fn site_document_html(&self, path: &str) -> Result<String> {
        ensure(
            self.is_editable_site_file(path),
            "That framed document is not an editable site HTML file.",
        )?;
        let bytes = self.site_file_bytes(path)?;
        String::from_utf8(bytes)
            .map_err(|_| Error("That website document is not UTF-8 and cannot be edited.".into()))
    }

    pub(crate) fn set_site_document_html(&mut self, path: &str, html: &str) -> Result<()> {
        ensure(
            self.is_editable_site_file(path),
            "That framed document is not an editable site HTML file.",
        )?;
        let site = self.website.as_mut().expect("editable site file");
        site.files
            .insert(path.into(), STANDARD.encode(html.as_bytes()).into());
        Ok(())
    }

    /// The current source of a document. Site files have no separate CSS or JavaScript fields.
    pub fn document_content(&self, target: &DocumentTarget) -> Result<SiteContent> {
        match target {
            DocumentTarget::Page { page_id } => self
                .page(*page_id)?
                .content
                .clone()
                .ok_or_else(|| Error("That page has no code yet.".into())),
            DocumentTarget::SiteFile { path } => Ok(SiteContent {
                html: self.site_document_html(path)?,
                css: String::new(),
                javascript: String::new(),
            }),
        }
    }

    pub fn document_html(&self, target: &DocumentTarget) -> Result<String> {
        Ok(self.document_content(target)?.html)
    }

    pub fn document_viewport(&self, target: &DocumentTarget) -> Result<Viewport> {
        match target {
            DocumentTarget::Page { page_id } => Ok(self.page(*page_id)?.viewport),
            DocumentTarget::SiteFile { .. } => Ok(self.viewport),
        }
    }

    /// Fingerprint used by element targets and requirements dependencies.
    pub fn document_hash(&self, target: &DocumentTarget) -> Result<String> {
        match target {
            DocumentTarget::Page { page_id } => self
                .page(*page_id)?
                .content
                .as_ref()
                .ok_or_else(|| Error("That page has no code yet.".into()))?
                .sha256(),
            DocumentTarget::SiteFile { path } => {
                Ok(digest(self.site_document_html(path)?.as_bytes()))
            }
        }
    }

    /// Parse a document the same way editing does: pages keep their page form; site HTML files
    /// keep a complete authored document when they have one.
    pub(crate) fn parse_document_target(
        &self,
        target: &DocumentTarget,
        html: &str,
    ) -> Result<Document> {
        match target {
            DocumentTarget::Page { .. } => self.parse_page_html(html),
            DocumentTarget::SiteFile { .. } => {
                crate::editing::parse_document(html, looks_like_document(html))
            }
        }
    }

    pub(crate) fn document_parse_full(&self, target: &DocumentTarget, html: &str) -> bool {
        match target {
            DocumentTarget::Page { .. } => self.website.is_some(),
            DocumentTarget::SiteFile { .. } => looks_like_document(html),
        }
    }

    fn page_for_file(&self, path: &str) -> Option<Uuid> {
        if let Some(site) = &self.website {
            for (id, source) in &site.page_files {
                if source == path {
                    return Some(*id);
                }
            }
        }
        self.pages
            .iter()
            .find(|page| crate::export::output_path(&page.path) == path)
            .map(|page| page.id)
    }

    fn page_for_route(&self, route: &str) -> Option<Uuid> {
        let route = if route == "/" {
            route.to_string()
        } else {
            route.trim_end_matches('/').to_string()
        };
        self.pages
            .iter()
            .find(|page| page.path == route)
            .map(|page| page.id)
    }

    /// Resolve one iframe `src` relative to the page that contains it.
    pub fn resolve_frame_source(&self, page: &crate::Page, src: &str) -> FrameSource {
        let raw = src.trim();
        let lower = raw.to_ascii_lowercase();
        if raw.is_empty() {
            return FrameSource::Unresolvable { src: raw.into() };
        }
        if lower.starts_with("//")
            || ["http:", "https:", "mailto:", "tel:", "data:", "ftp:"]
                .iter()
                .any(|scheme| lower.starts_with(scheme))
        {
            return FrameSource::External { url: raw.into() };
        }
        if lower.starts_with("javascript:") || lower.starts_with("blob:") {
            return FrameSource::Unresolvable { src: raw.into() };
        }
        let path = raw.split(['?', '#']).next().unwrap_or("").trim();
        if path.is_empty() {
            return FrameSource::Unresolvable { src: raw.into() };
        }
        let mut bases = Vec::new();
        if let Some(source) = self
            .website
            .as_ref()
            .and_then(|site| site.page_files.get(&page.id))
        {
            bases.push(source.clone());
        }
        bases.push(crate::export::output_path(&page.path));
        let mut candidates = BTreeSet::new();
        for base in &bases {
            if let Some(candidate) = join_site_path(base, path) {
                candidates.insert(candidate);
            }
        }
        if path.starts_with('/') {
            let mut trimmed = path.trim_start_matches('/').to_string();
            if trimmed.is_empty() {
                trimmed = "index.html".into();
            }
            candidates.insert(trimmed);
            if let Some(id) = self.page_for_route(&format!("/{path}")) {
                return FrameSource::Page { page_id: id };
            }
            if let Some(id) = self.page_for_route(path) {
                return FrameSource::Page { page_id: id };
            }
        }
        for candidate in &candidates {
            if let Some(id) = self.page_for_file(candidate) {
                return FrameSource::Page { page_id: id };
            }
            if self.is_editable_site_file(candidate) {
                return FrameSource::SiteFile {
                    path: candidate.clone(),
                };
            }
        }
        FrameSource::Unresolvable { src: raw.into() }
    }

    /// Every iframe on a page, with its editor ID and resolved source.
    pub fn frame_targets(&self, page_id: Uuid) -> Result<Vec<PageFrameTarget>> {
        let page = self.page(page_id)?;
        let Some(content) = &page.content else {
            return Ok(vec![]);
        };
        let doc = self.parse_page_html(&content.html)?;
        Ok(doc
            .select("iframe")
            .nodes()
            .iter()
            .map(|node| {
                let src = node
                    .attr("src")
                    .map(|value| value.to_string())
                    .unwrap_or_default();
                let source = self.resolve_frame_source(page, &src);
                let document = match &source {
                    FrameSource::Page { page_id } => Some(DocumentTarget::page(*page_id)),
                    FrameSource::SiteFile { path } => {
                        Some(DocumentTarget::SiteFile { path: path.clone() })
                    }
                    FrameSource::External { .. } | FrameSource::Unresolvable { .. } => None,
                };
                let source_sha256 = document
                    .as_ref()
                    .and_then(|document| self.document_hash(document).ok());
                PageFrameTarget {
                    iframe_id: node
                        .attr(crate::ELEMENT_ID_ATTRIBUTE)
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    source,
                    document,
                    source_sha256,
                    src,
                }
            })
            .collect())
    }
}
