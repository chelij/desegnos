//! Editable documents shown in a page's iframes: a project page or an imported site HTML file.
use crate::{Error, Project, Rect, Result, SiteContent, Viewport, digest, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use dom_query::Document;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// A request box meets an iframe when they share at least this much area, so that a small
/// corner clip on a large page stays a page request.
const MIN_FRAME_OVERLAP_AREA: f64 = 256.0;
const MIN_FRAME_OVERLAP_SIDE: f64 = 4.0;

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

/// The shared part of two boxes, or None when they do not overlap.
fn overlap_rect(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
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

    /// The iframe boxes measured on a page, keyed by editor ID. Requests without their own
    /// framed document contribute the measurement taken before they were prepared.
    pub fn measured_frame_rects(&self, page_id: Uuid) -> BTreeMap<String, Rect> {
        let mut boxes = BTreeMap::new();
        if let Ok(page) = self.page(page_id) {
            for region in page.regions.iter().filter(|region| !region.completed) {
                for bound in &region.area_bounds {
                    if !bound.target.id.is_empty() {
                        boxes.entry(bound.target.id.clone()).or_insert(bound.rect);
                    }
                }
            }
        }
        boxes
    }

    /// The frame a request box belongs to before generation. The frame under the box centre
    /// wins when the box covers several; otherwise the largest shared area wins.
    fn frame_target_in(
        &self,
        page_id: Uuid,
        rect: Rect,
        frames: &[PageFrameTarget],
        boxes: &BTreeMap<String, Rect>,
    ) -> Result<Option<FrameTarget>> {
        let centre = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let mut best: Option<(bool, f64, FrameTarget)> = None;
        for frame in frames {
            let Some(document) = &frame.document else {
                continue;
            };
            let Some(host) = boxes.get(&frame.iframe_id).copied() else {
                continue;
            };
            let Some(shared) = overlap_rect(rect, host) else {
                continue;
            };
            if shared.width < MIN_FRAME_OVERLAP_SIDE
                || shared.height < MIN_FRAME_OVERLAP_SIDE
                || shared.width * shared.height < MIN_FRAME_OVERLAP_AREA
            {
                continue;
            }
            let centred = centre.0 >= host.x
                && centre.0 <= host.x + host.width
                && centre.1 >= host.y
                && centre.1 <= host.y + host.height;
            let area = shared.width * shared.height;
            let better = best.as_ref().is_none_or(|(best_centred, best_area, _)| {
                (centred && !best_centred) || (centred == *best_centred && area > *best_area)
            });
            if better {
                best = Some((
                    centred,
                    area,
                    FrameTarget {
                        page_id,
                        iframe_id: frame.iframe_id.clone(),
                        document: document.clone(),
                        rect: self.frame_rect(document, host, rect)?,
                    },
                ));
            }
        }
        Ok(best.map(|(_, _, frame)| frame))
    }

    /// Map a host-page box into the framed document. The measured iframe box is the document's
    /// viewport drawn at its current size, so the ratio of the two sizes is the mapping.
    fn frame_rect(&self, document: &DocumentTarget, host: Rect, rect: Rect) -> Result<Rect> {
        let viewport = self.document_viewport(document)?;
        let scale = |drawn: f64, viewport: u32| {
            let scale = drawn / f64::from(viewport);
            if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            }
        };
        let (scale_x, scale_y) = (
            scale(host.width, viewport.width),
            scale(host.height, viewport.height),
        );
        let mapped = Rect {
            x: (rect.x - host.x) / scale_x,
            y: (rect.y - host.y) / scale_y,
            width: rect.width / scale_x,
            height: rect.height / scale_y,
        };
        ensure(
            [mapped.x, mapped.y, mapped.width, mapped.height]
                .iter()
                .all(|value| value.is_finite()),
            "The framed request could not be mapped into its document.",
        )?;
        Ok(mapped)
    }

    /// Give every pending request drawn over an editable frame its framed document. A request
    /// created before framed editing existed keeps its host-page box and gains the mapping here.
    /// An existing target stays when its iframe still shows the same document.
    pub fn adopt_frame_targets(&self) -> Result<Self> {
        self.validate()?;
        let mut next = self.clone();
        let mut changed = false;
        for index in 0..next.pages.len() {
            let page_id = next.pages[index].id;
            let frames = self.frame_targets(page_id)?;
            let boxes = self.measured_frame_rects(page_id);
            for region in next.pages[index]
                .regions
                .iter_mut()
                .filter(|region| !region.completed)
            {
                if let Some(existing) = &region.frame_target {
                    let shown = frames.iter().any(|frame| {
                        frame.iframe_id == existing.iframe_id
                            && frame.document.as_ref() == Some(&existing.document)
                    });
                    if shown {
                        continue;
                    }
                }
                let resolved = self.frame_target_in(page_id, region.rect, &frames, &boxes)?;
                if region.frame_target == resolved {
                    continue;
                }
                region.frame_target = resolved.clone();
                if resolved.is_some() {
                    region.element = None;
                    region.elements.clear();
                    region.area_bounds.clear();
                    region.shape = crate::RegionShape::Rect;
                    region.allow_target_removal = false;
                    region.mobile_only = false;
                    region.runtime_errors.clear();
                    region.whole_page = false;
                    region.whole_page_disabled = false;
                }
                changed = true;
            }
        }
        if changed {
            next.revision = self
                .revision
                .checked_add(1)
                .ok_or_else(|| Error("Project revision limit reached.".into()))?;
            next.validate()?;
        }
        Ok(next)
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
