//! Which documents a Flexible request may change beyond its framed document.
//!
//! A framed request names one document. A request about "these sub-pages", or one that names a
//! page, needs the neighbouring documents too; otherwise the AI cannot answer it and reports the
//! source as missing. This module selects those documents from the project alone, in priority
//! order and within a bounded budget, so the requirements snapshot stays reproducible.
use crate::{DocumentTarget, Page, Project, Region, Result, SiteContent};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Source bytes one request may receive for all of its editable documents.
pub const MAX_DOCUMENT_CONTEXT_BYTES: usize = 400 * 1024;
/// A runaway page tree must not turn one request into a whole-site dump.
pub const MAX_DOCUMENT_CONTEXT_DOCUMENTS: usize = 24;

/// Why a document belongs to this request. It is sent with each included document.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum DocumentReason {
    /// The request is drawn over an iframe that shows this document.
    Framed,
    /// The request has at least one area on this page.
    Targeted,
    /// The user chose this page or page group on the request.
    Selected,
    /// A framed document lives in the same directory as this one and the request covers the set.
    SharedDirectory,
    /// The request names this document's title or path.
    NamedInPrompt,
}

/// One document the AI may change for this request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IncludedDocument {
    pub document: DocumentTarget,
    /// The page route or the imported site path a person recognises.
    pub path: String,
    pub title: String,
    pub reason: DocumentReason,
}

/// A site stylesheet included as read-only reference for the included site documents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharedStylesheet {
    pub path: String,
    pub source: String,
}

/// The editable-document set for one request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentContext {
    pub documents: Vec<IncludedDocument>,
    pub stylesheets: Vec<SharedStylesheet>,
}

/// The documents of one sequential request when a selection exceeds the source budget.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentBatch {
    pub documents: Vec<DocumentTarget>,
    pub bytes: usize,
}

/// One sequential part of a request whose editable documents exceed the source budget.
/// Framed documents are repeated in every part; only the first part may change them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentPart {
    pub documents: Vec<IncludedDocument>,
    pub stylesheets: Vec<SharedStylesheet>,
    pub bytes: usize,
}

/// How one request's editable documents are split, plus the complete set used only for
/// validation and application of the merged result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentPlan {
    pub parts: Vec<DocumentPart>,
    pub merged: DocumentContext,
}

/// Directory prefixes shared by page routes and site paths.
#[derive(Debug, Default)]
struct DirectoryPatterns {
    routes: BTreeSet<String>,
    paths: BTreeSet<String>,
}

/// Words that turn a request about one document into a request about its neighbours.
const SHARED_SCOPE_WORDS: [&str; 7] = ["all", "every", "each", "both", "other", "these", "those"];

fn words(value: &str) -> Vec<&str> {
    value
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|word| !word.is_empty())
        .collect()
}

/// Whether a request talks about a set of pages rather than one page.
pub fn mentions_shared_scope(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    if lower.contains("sub-page") || lower.contains("subpage") || lower.contains("sub page") {
        return true;
    }
    words(&lower)
        .iter()
        .any(|word| SHARED_SCOPE_WORDS.contains(word))
}

fn directory_of(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(directory, _)| directory)
        .filter(|directory| !directory.is_empty())
}

fn parent_of(directory: &str) -> Option<&str> {
    directory
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
}

impl DirectoryPatterns {
    fn add_route(&mut self, route: &str) {
        let route = route.trim_end_matches('/');
        if route.is_empty() {
            return;
        }
        self.routes.insert(format!("{route}/"));
        if let Some((parent, _)) = route.rsplit_once('/')
            && !parent.is_empty()
        {
            self.routes.insert(format!("{parent}/"));
        }
        self.paths
            .insert(format!("{}/", route.trim_start_matches('/')));
    }

    fn add_path(&mut self, path: &str) {
        let Some(directory) = directory_of(path) else {
            return;
        };
        self.paths.insert(format!("{directory}/"));
        self.routes.insert(format!("/{directory}/"));
        if let Some(parent) = parent_of(directory) {
            self.paths.insert(format!("{parent}/"));
            self.routes.insert(format!("/{parent}/"));
        }
    }
}

impl Project {
    /// The page route or imported site path of a document.
    pub fn document_path(&self, document: &DocumentTarget) -> String {
        match document {
            DocumentTarget::Page { page_id } => self
                .page(*page_id)
                .map(|page| page.path.clone())
                .unwrap_or_default(),
            DocumentTarget::SiteFile { path } => path.clone(),
        }
    }

    /// A short human title: the page name, or the file stem with `index` resolving to its folder.
    pub fn document_title(&self, document: &DocumentTarget) -> String {
        match document {
            DocumentTarget::Page { page_id } => self
                .page(*page_id)
                .map(|page| page.name.clone())
                .unwrap_or_default(),
            DocumentTarget::SiteFile { path } => {
                let file = path.rsplit('/').next().unwrap_or(path);
                let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
                if stem.eq_ignore_ascii_case("index") {
                    directory_of(path)
                        .and_then(|directory| directory.rsplit('/').next())
                        .unwrap_or(stem)
                        .to_string()
                } else {
                    stem.to_string()
                }
            }
        }
    }

    fn include_document(
        &self,
        context: &mut DocumentContext,
        seen: &mut BTreeSet<DocumentTarget>,
        used: &mut usize,
        document: DocumentTarget,
        reason: DocumentReason,
        mandatory: bool,
    ) -> Result<()> {
        if seen.contains(&document) {
            return Ok(());
        }
        let size = self.document_content(&document)?.byte_len();
        if !mandatory
            && (*used + size > MAX_DOCUMENT_CONTEXT_BYTES
                || context.documents.len() >= MAX_DOCUMENT_CONTEXT_DOCUMENTS)
        {
            return Ok(());
        }
        *used += size;
        seen.insert(document.clone());
        context.documents.push(IncludedDocument {
            path: self.document_path(&document),
            title: self.document_title(&document),
            document,
            reason,
        });
        Ok(())
    }

    /// Every document this request may change. Precise requests keep their framed document and
    /// the user's page selection; Flexible requests add the documents the request clearly
    /// concerns, bounded by [`MAX_DOCUMENT_CONTEXT_BYTES`]. Framed and selected documents come
    /// first and are never dropped: a selection that does not fit one request is an error that
    /// says how many pages fit.
    pub fn document_context(
        &self,
        targets: &[(&Page, &Region)],
        flexible: bool,
    ) -> Result<DocumentContext> {
        let mut context = DocumentContext::default();
        let mut seen = BTreeSet::new();
        let mut framed = BTreeSet::new();
        let mut used = 0usize;
        for (_, region) in targets {
            let Some(frame) = &region.frame_target else {
                continue;
            };
            self.include_document(
                &mut context,
                &mut seen,
                &mut used,
                frame.document.clone(),
                DocumentReason::Framed,
                true,
            )?;
            framed.insert(frame.document.clone());
        }
        // The page picker applies in both modes. Its documents are mandatory.
        for document in self.selected_documents(targets) {
            self.include_document(
                &mut context,
                &mut seen,
                &mut used,
                document,
                DocumentReason::Selected,
                true,
            )?;
        }
        // Without a framed document this is an ordinary page request; the automatic
        // neighbour rules need a framed document to start from.
        if flexible && !framed.is_empty() {
            for (document, reason) in self.extra_documents(targets, &framed, &seen) {
                let _ = self.include_document(
                    &mut context,
                    &mut seen,
                    &mut used,
                    document,
                    reason,
                    false,
                );
            }
        }
        if (flexible || !context.documents.is_empty()) && used < MAX_DOCUMENT_CONTEXT_BYTES {
            context.stylesheets = self.shared_stylesheets(&context.documents, used);
        }
        Ok(context)
    }
}

impl DocumentPart {
    fn into_context(self) -> DocumentContext {
        DocumentContext {
            documents: self.documents,
            stylesheets: self.stylesheets,
        }
    }
}

impl Project {
    /// The editable documents of one part of a batched request, rebuilt from the frozen project
    /// so the snapshot stays reproducible.
    pub(crate) fn batched_document_context(
        &self,
        targets: &[(&Page, &Region)],
        flexible: bool,
        batch: &crate::DocumentBatchContext,
    ) -> Result<DocumentContext> {
        let plan = self.document_plan(targets, flexible)?;
        crate::ensure(
            batch.parts > 0 && batch.parts == plan.parts.len(),
            "The batched request's document split no longer matches the project. Generate new requirements.",
        )?;
        match batch.part {
            Some(part) => {
                crate::ensure(
                    (1..=batch.parts).contains(&part),
                    "The batched request's part number is invalid.",
                )?;
                Ok(plan.parts[part - 1].clone().into_context())
            }
            None => Ok(plan.merged),
        }
    }

    /// The documents of one sequential request when a selection exceeds the source budget.
    pub fn document_plan(
        &self,
        targets: &[(&Page, &Region)],
        flexible: bool,
    ) -> Result<DocumentPlan> {
        // Freeze the target order so prepare and validation compute the same partition.
        let mut ordered_targets = targets.to_vec();
        ordered_targets.sort_by_key(|(page, region)| {
            (
                self.pages.iter().position(|item| item.id == page.id),
                region.z_index,
                page.regions.iter().position(|item| item.id == region.id),
            )
        });
        let targets = ordered_targets.as_slice();
        let mut seen = BTreeSet::new();
        let mut framed = Vec::new();
        let mut framed_set = BTreeSet::new();
        for (_, region) in targets {
            let Some(frame) = &region.frame_target else {
                continue;
            };
            if !seen.insert(frame.document.clone()) {
                continue;
            }
            framed_set.insert(frame.document.clone());
            framed.push(IncludedDocument {
                path: self.document_path(&frame.document),
                title: self.document_title(&frame.document),
                document: frame.document.clone(),
                reason: DocumentReason::Framed,
            });
        }
        let mut editable = Vec::new();
        // A page with a request belongs to the request even when the page picker leaves it out.
        for (page, _) in targets {
            if page.content.is_none() {
                continue;
            }
            let document = DocumentTarget::page(page.id);
            if !seen.insert(document.clone()) {
                continue;
            }
            editable.push(IncludedDocument {
                path: page.path.clone(),
                title: page.name.clone(),
                document,
                reason: DocumentReason::Targeted,
            });
        }
        for document in self.selected_documents(targets) {
            if !seen.insert(document.clone()) {
                continue;
            }
            editable.push(IncludedDocument {
                path: self.document_path(&document),
                title: self.document_title(&document),
                document,
                reason: DocumentReason::Selected,
            });
        }
        if flexible && !framed_set.is_empty() {
            for (document, reason) in self.extra_documents(targets, &framed_set, &seen) {
                if seen.insert(document.clone()) {
                    editable.push(IncludedDocument {
                        path: self.document_path(&document),
                        title: self.document_title(&document),
                        document,
                        reason,
                    });
                }
            }
        }
        let size = |document: &DocumentTarget| -> Result<usize> {
            Ok(self.document_content(document)?.byte_len())
        };
        let framed_bytes = framed.iter().try_fold(0usize, |sum, item| {
            Ok::<_, crate::Error>(sum + size(&item.document)?)
        })?;
        let budget = if framed_bytes >= MAX_DOCUMENT_CONTEXT_BYTES {
            MAX_DOCUMENT_CONTEXT_BYTES
        } else {
            MAX_DOCUMENT_CONTEXT_BYTES - framed_bytes
        };
        let cap = MAX_DOCUMENT_CONTEXT_DOCUMENTS
            .saturating_sub(framed.len())
            .max(1);
        let slices = self.partition_documents(&editable, budget, cap)?;
        let all_documents: Vec<IncludedDocument> = framed
            .iter()
            .cloned()
            .chain(editable.iter().cloned())
            .collect();
        let candidates = self.stylesheet_candidates(&all_documents);
        let all_bytes = all_documents.iter().try_fold(0usize, |sum, item| {
            Ok::<_, crate::Error>(sum + size(&item.document)?)
        })?;
        let merged = DocumentContext {
            documents: all_documents,
            stylesheets: within_budget(candidates.clone(), all_bytes),
        };
        let mut parts = Vec::new();
        for slice in slices {
            let bytes = framed_bytes
                + slice.iter().try_fold(0usize, |sum, item| {
                    Ok::<_, crate::Error>(sum + size(&item.document)?)
                })?;
            let mut documents = framed.clone();
            documents.extend(slice);
            parts.push(DocumentPart {
                documents,
                stylesheets: within_budget(candidates.clone(), bytes),
                bytes,
            });
        }
        Ok(DocumentPlan { parts, merged })
    }

    /// Greedy sequential partition: a document larger than the whole budget still gets its own
    /// part instead of making the request impossible.
    fn partition_documents(
        &self,
        documents: &[IncludedDocument],
        budget: usize,
        cap: usize,
    ) -> Result<Vec<Vec<IncludedDocument>>> {
        let mut parts: Vec<Vec<IncludedDocument>> = Vec::new();
        let mut current: Vec<IncludedDocument> = Vec::new();
        let mut bytes = 0usize;
        for document in documents {
            let size = self.document_content(&document.document)?.byte_len();
            let full = !current.is_empty()
                && (bytes + size > budget || current.len() >= cap || size > budget);
            if full {
                parts.push(std::mem::take(&mut current));
                bytes = 0;
            }
            bytes += size;
            current.push(document.clone());
        }
        if !current.is_empty() || parts.is_empty() {
            parts.push(current);
        }
        Ok(parts)
    }

    /// The documents a request's page picker covers, in project order. Pages without code are
    /// skipped: there is nothing to change yet.
    pub fn selected_documents(&self, targets: &[(&Page, &Region)]) -> Vec<DocumentTarget> {
        let selections: Vec<&crate::PageSelection> = targets
            .iter()
            .flat_map(|(_, region)| region.pages.iter())
            .collect();
        if selections.is_empty() {
            return vec![];
        }
        let mut seen = BTreeSet::new();
        let mut documents = Vec::new();
        for page in &self.pages {
            if page.content.is_none() {
                continue;
            }
            if selections
                .iter()
                .any(|selection| selection.covers(&page.path, Some(page.id)))
            {
                let document = DocumentTarget::page(page.id);
                if seen.insert(document.clone()) {
                    documents.push(document);
                }
            }
        }
        for path in self.site_document_paths() {
            if selections
                .iter()
                .any(|selection| selection.covers(&path, None))
            {
                let document = DocumentTarget::SiteFile { path };
                if seen.insert(document.clone()) {
                    documents.push(document);
                }
            }
        }
        documents
    }

    /// Split selected documents into sequential batches that each fit the source budget. A single
    /// document larger than the whole budget gets its own batch.
    pub fn document_batches(
        &self,
        documents: &[DocumentTarget],
        budget: usize,
    ) -> Result<Vec<DocumentBatch>> {
        crate::ensure(
            budget > 0,
            "This request has no room for another page. Choose fewer pages.",
        )?;
        let mut batches: Vec<DocumentBatch> = Vec::new();
        for document in documents {
            let size = self.document_content(document)?.byte_len();
            let full = batches.last().is_some_and(|batch| {
                batch.bytes + size > budget
                    || batch.documents.len() >= MAX_DOCUMENT_CONTEXT_DOCUMENTS
                    || size > budget
            });
            if batches.is_empty() || full {
                batches.push(DocumentBatch::default());
            }
            let batch = batches.last_mut().expect("a document batch");
            batch.bytes += size;
            batch.documents.push(document.clone());
        }
        Ok(batches)
    }

    /// Documents added by a Flexible request beyond its framed and chosen documents: the framed
    /// document's directory when the request covers a set, then documents the prompt names.
    fn extra_documents(
        &self,
        targets: &[(&Page, &Region)],
        framed: &BTreeSet<DocumentTarget>,
        seen: &BTreeSet<DocumentTarget>,
    ) -> Vec<(DocumentTarget, DocumentReason)> {
        let prompts: Vec<String> = targets
            .iter()
            .map(|(_, region)| region.prompt.to_lowercase())
            .collect();
        let host_pages: BTreeSet<Uuid> = targets.iter().map(|(page, _)| page.id).collect();
        let candidates: Vec<DocumentTarget> = self
            .pages
            .iter()
            .filter(|page| !host_pages.contains(&page.id) && page.content.is_some())
            .map(|page| DocumentTarget::page(page.id))
            .chain(
                self.site_document_paths()
                    .into_iter()
                    .map(|path| DocumentTarget::SiteFile { path }),
            )
            .filter(|document| !seen.contains(document))
            .collect();
        let mut found = Vec::new();
        let mut local_seen = seen.clone();
        // 1. Documents sharing the framed document's directory, when the request covers a set.
        if prompts.iter().any(|prompt| mentions_shared_scope(prompt)) {
            let patterns = self.framed_patterns(framed);
            for document in &candidates {
                if local_seen.insert(document.clone())
                    && patterns.contains(document, &self.document_path(document))
                {
                    found.push((document.clone(), DocumentReason::SharedDirectory));
                }
            }
        }
        // 2. Documents the prompt names by title, path or route.
        for document in &candidates {
            if local_seen.contains(document) {
                continue;
            }
            let path = self.document_path(document).to_lowercase();
            let title = self.document_title(document).to_lowercase();
            if prompts
                .iter()
                .any(|prompt| names_document(prompt, &path, &title))
            {
                local_seen.insert(document.clone());
                found.push((document.clone(), DocumentReason::NamedInPrompt));
            }
        }
        found
    }

    /// Directory patterns of the framed documents, as page routes and site paths.
    fn framed_patterns(&self, framed: &BTreeSet<DocumentTarget>) -> DirectoryPatterns {
        let mut patterns = DirectoryPatterns::default();
        for document in framed {
            match document {
                DocumentTarget::Page { page_id } => {
                    if let Ok(page) = self.page(*page_id)
                        && page.path != "/"
                    {
                        patterns.add_route(&page.path);
                    }
                }
                DocumentTarget::SiteFile { path } => patterns.add_path(path),
            }
        }
        patterns
    }

    /// Site stylesheets linked by the included site documents, within the remaining budget.
    fn shared_stylesheets(
        &self,
        documents: &[IncludedDocument],
        used: usize,
    ) -> Vec<SharedStylesheet> {
        within_budget(self.stylesheet_candidates(documents), used)
    }

    /// Site stylesheets linked by the included site documents, without a budget.
    fn stylesheet_candidates(&self, documents: &[IncludedDocument]) -> Vec<SharedStylesheet> {
        let mut seen = BTreeSet::new();
        let mut stylesheets = Vec::new();
        for document in documents {
            let DocumentTarget::SiteFile { path } = &document.document else {
                continue;
            };
            let Ok(html) = self.site_document_html(path) else {
                continue;
            };
            let Ok(parsed) = crate::editing::parse(&html) else {
                continue;
            };
            for node in parsed.select("link[href]").nodes() {
                let rel = node
                    .attr("rel")
                    .map(|value| value.to_ascii_lowercase())
                    .unwrap_or_default();
                if !rel.split_whitespace().any(|part| part == "stylesheet") {
                    continue;
                }
                let Some(resolved) = node
                    .attr("href")
                    .and_then(|href| crate::document::join_site_path(path, href.as_ref()))
                else {
                    continue;
                };
                if !seen.insert(resolved.clone()) {
                    continue;
                }
                let Ok(source) = self.site_file_text(&resolved) else {
                    continue;
                };
                stylesheets.push(SharedStylesheet {
                    path: resolved,
                    source,
                });
            }
        }
        stylesheets
    }
}

/// Keep the stylesheets that still fit beside `used` bytes of document source.
fn within_budget(candidates: Vec<SharedStylesheet>, used: usize) -> Vec<SharedStylesheet> {
    let mut budget = MAX_DOCUMENT_CONTEXT_BYTES.saturating_sub(used);
    let mut stylesheets = Vec::new();
    for stylesheet in candidates {
        if stylesheet.source.len() > budget {
            continue;
        }
        budget -= stylesheet.source.len();
        stylesheets.push(stylesheet);
    }
    stylesheets
}

impl DirectoryPatterns {
    fn contains(&self, document: &DocumentTarget, location: &str) -> bool {
        match document {
            DocumentTarget::Page { .. } => self.routes.iter().any(|prefix| {
                location.starts_with(prefix) && location != prefix.trim_end_matches('/')
            }),
            DocumentTarget::SiteFile { .. } => {
                self.paths.iter().any(|prefix| location.starts_with(prefix))
            }
        }
    }
}

fn names_document(prompt: &str, path: &str, title: &str) -> bool {
    if !path.is_empty() && prompt.contains(path) {
        return true;
    }
    let ignored = [
        "the", "page", "pages", "site", "home", "index", "this", "that", "with", "and", "for",
    ];
    let prompt_words = words(prompt);
    let title_words: Vec<&str> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= 3)
        .collect();
    prompt_words
        .iter()
        .any(|word| word.len() >= 3 && !ignored.contains(word) && title_words.contains(word))
}

/// Read one imported site file as UTF-8 text, whatever its stored encoding.
impl Project {
    pub fn site_file_text(&self, path: &str) -> Result<String> {
        let site = self
            .website
            .as_ref()
            .ok_or_else(|| crate::Error("This project has no imported website.".into()))?;
        let encoded = site
            .files
            .get(path)
            .ok_or_else(|| crate::Error("That website file no longer exists.".into()))?;
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
        use base64::{Engine, engine::general_purpose::STANDARD};
        let bytes = STANDARD
            .decode(value)
            .map_err(|_| crate::Error("Invalid imported website resource.".into()))?;
        String::from_utf8(bytes).map_err(|_| crate::Error("That website file is not UTF-8.".into()))
    }
}
