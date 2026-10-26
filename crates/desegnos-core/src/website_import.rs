//! Turn a bounded website snapshot into editable pages, using the normal picture store and IDs.
use crate::{Appearance, Error, Page, Project, Result, SiteContent, Viewport, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use cssparser::{Parser, ParserInput, Token};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};
use url::Url;
use uuid::Uuid;

struct Importer<'a> {
    keep_scripts: bool,
    /// Canvas documents load site files from the preview session, so they keep their paths.
    inline_assets: bool,
    files: &'a BTreeMap<String, Vec<u8>>,
    routes: BTreeMap<String, String>,
    pictures: BTreeMap<String, (Uuid, std::sync::Arc<str>)>,
}

fn image_mime(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "svg" => Some("image/svg+xml"),
        "ico" => Some("image/x-icon"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

fn text(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| {
        Error("Website source must be UTF-8. The originals were left unchanged.".into())
    })
}

impl Importer<'_> {
    fn local(&self, value: &str, source: &str) -> Option<(String, String)> {
        let base = Url::parse(source)
            .or_else(|_| Url::parse(&format!("https://desegnos.invalid/{source}")))
            .ok()?;
        let url = base.join(value.trim()).ok()?;
        if url.origin() != Url::parse("https://desegnos.invalid/").ok()?.origin() {
            return None;
        }
        let path = url.path().strip_prefix('/')?;
        // URL paths are percent encoded; decode without accepting encoded traversal/separators.
        let mut bytes = Vec::new();
        let mut chars = path.bytes();
        while let Some(b) = chars.next() {
            bytes.push(if b == b'%' {
                let a = (chars.next()? as char).to_digit(16)?;
                let b = (chars.next()? as char).to_digit(16)?;
                (a * 16 + b) as u8
            } else {
                b
            });
        }
        let path = String::from_utf8(bytes).ok()?;
        if path
            .split('/')
            .any(|p| p == ".." || p == "." || p.starts_with('.'))
            || path.contains('\\')
        {
            return None;
        }
        let suffix = format!(
            "{}{}",
            url.query().map(|s| format!("?{s}")).unwrap_or_default(),
            url.fragment().map(|s| format!("#{s}")).unwrap_or_default()
        );
        Some((path, suffix))
    }

    fn resource(&self, value: &str, source: &str, stack: &mut BTreeSet<String>) -> Result<String> {
        let Some((path, suffix)) = self.local(value, source) else {
            return Ok(Url::parse(source)
                .ok()
                .filter(|u| u.host_str() != Some("desegnos.invalid"))
                .and_then(|u| u.join(value).ok())
                .map_or_else(|| value.into(), |u| u.to_string()));
        };
        let Some(bytes) = self.files.get(&path) else {
            return Ok(value.into());
        };
        let fragment = suffix.find('#').map(|i| &suffix[i..]).unwrap_or("");
        if let Some((id, _)) = self.pictures.get(&path) {
            return Ok(format!("{}{fragment}", crate::project_asset_reference(*id)));
        }
        let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let mime = match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            "avif" => "image/avif",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "css" => "text/css",
            _ => return Ok(value.into()),
        };
        if !self.inline_assets {
            return Ok(format!("/{path}{suffix}"));
        }
        let transformed;
        let bytes = if extension == "css" {
            ensure(
                stack.len() < 16 && stack.insert(path.clone()),
                "A stylesheet import is circular or nested too deeply.",
            )?;
            transformed = self.css(text(bytes)?, &path, stack)?.into_bytes();
            stack.remove(&path);
            &transformed
        } else {
            bytes
        };
        // Queries do not apply to embedded bytes; SVG fragments still do.
        Ok(format!(
            "data:{mime};base64,{}{fragment}",
            STANDARD.encode(bytes)
        ))
    }

    fn css(&self, source: &str, file: &str, stack: &mut BTreeSet<String>) -> Result<String> {
        let mut input = ParserInput::new(source);
        let mut parser = Parser::new(&mut input);
        let mut edits = Vec::new();
        self.css_tokens(&mut parser, file, stack, &mut edits, false)?;
        let mut output = source.to_owned();
        for (start, end, value) in edits.into_iter().rev() {
            output.replace_range(start..end, &value);
        }
        Ok(output)
    }

    fn css_tokens(
        &self,
        parser: &mut Parser<'_, '_>,
        file: &str,
        stack: &mut BTreeSet<String>,
        edits: &mut Vec<(usize, usize, String)>,
        images: bool,
    ) -> Result<()> {
        let mut import = false;
        loop {
            let start = parser.position().byte_index();
            let token = match parser.next_including_whitespace_and_comments() {
                Ok(t) => t.clone(),
                Err(_) => break,
            };
            let quote = |s: &str| {
                let mut out = String::new();
                cssparser::serialize_string(s, &mut out).expect("String write");
                out
            };
            match token {
                Token::UnquotedUrl(value) => {
                    let value = self.resource(&value, file, stack)?;
                    edits.push((
                        start,
                        parser.position().byte_index(),
                        format!("url({})", quote(&value)),
                    ));
                    import = false;
                }
                Token::QuotedString(value) if import || images => {
                    edits.push((
                        start,
                        parser.position().byte_index(),
                        quote(&self.resource(&value, file, stack)?),
                    ));
                    import = false;
                }
                Token::AtKeyword(name) if name.eq_ignore_ascii_case("import") => {
                    let url = parser
                        .expect_url_or_string()
                        .map_err(|_| Error("A stylesheet import URL is malformed.".into()))?
                        .to_string();
                    let tail_start = parser.position();
                    let mut tail_end = tail_start;
                    while !parser.is_exhausted() {
                        tail_end = parser.position();
                        if matches!(
                            parser.next_including_whitespace_and_comments(),
                            Ok(Token::Semicolon)
                        ) {
                            break;
                        }
                        tail_end = parser.position();
                    }
                    if let Some((path, _)) = self.local(&url, file)
                        && let Some(bytes) = self.files.get(&path)
                    {
                        ensure(
                            stack.len() < 16 && stack.insert(path.clone()),
                            "A stylesheet import is circular or nested too deeply.",
                        )?;
                        let css = self.css(text(bytes)?, &path, stack)?;
                        stack.remove(&path);
                        let media = parser.slice(tail_start..tail_end).trim();
                        // Leave advanced import qualifiers to the browser rather than changing their meaning.
                        if !media.starts_with("layer") && !media.starts_with("supports") {
                            let css = if media.is_empty() {
                                css
                            } else {
                                format!("@media {media} {{\n{css}\n}}")
                            };
                            edits.push((start, parser.position().byte_index(), css));
                        }
                    }
                    import = false;
                }
                Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                    let value = parser
                        .parse_nested_block(|inner| {
                            inner
                                .expect_string_cloned()
                                .map_err(Into::<cssparser::ParseError<'_, ()>>::into)
                        })
                        .map_err(|_| Error("An imported CSS URL is malformed.".into()))?;
                    edits.push((
                        start,
                        parser.position().byte_index(),
                        format!("url({})", quote(&self.resource(&value, file, stack)?)),
                    ));
                    import = false;
                }
                Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock => {
                    let nested_images = matches!(&token, Token::Function(name) if name.eq_ignore_ascii_case("image-set") || name.eq_ignore_ascii_case("-webkit-image-set"));
                    let result: std::result::Result<(), cssparser::ParseError<'_, ()>> = parser
                        .parse_nested_block(|inner| {
                            self.css_tokens(inner, file, stack, edits, nested_images)
                                .map_err(|_| inner.new_custom_error(()))
                        });
                    result.map_err(|_| Error("An imported CSS resource is invalid.".into()))?;
                    import = false;
                }
                Token::WhiteSpace(_) | Token::Comment(_) => {}
                _ => import = false,
            }
        }
        Ok(())
    }

    fn page(&self, source: &str) -> Result<(String, SiteContent)> {
        let doc = crate::editing::parse_document(text(&self.files[source])?, true)?;
        let page_base =
            Url::parse(&format!("https://desegnos.invalid/{source}")).expect("site base");
        let base = doc
            .select("base[href]")
            .nodes()
            .first()
            .and_then(|n| n.attr("href"))
            .and_then(|href| page_base.join(&href).ok())
            .unwrap_or(page_base);
        let source = base.as_str();
        if !self.keep_scripts {
            for node in doc.select("base").nodes() {
                node.remove_from_parent();
            }
        }
        let title = doc
            .select_single("title")
            .text()
            .trim()
            .chars()
            .take(120)
            .collect::<String>();
        let mut javascript = String::new();
        for node in doc.select("link[href]").nodes() {
            if !node.attr("rel").is_some_and(|r| {
                r.split_whitespace()
                    .any(|s| s.eq_ignore_ascii_case("stylesheet"))
            }) {
                continue;
            }
            if let Some((path, _)) = self.local(&node.attr("href").unwrap_or_default(), source)
                && let Some(bytes) = self.files.get(&path)
            {
                let css = self.css(text(bytes)?, &path, &mut BTreeSet::from([path.clone()]))?;
                node.rename("style");
                node.remove_attrs(&["href", "rel", "integrity", "crossorigin"]);
                node.set_text(css.replace("</style", "<\\/style"));
            }
        }
        for node in doc.select("script[src]").nodes() {
            if self.keep_scripts {
                if self
                    .local(&node.attr("src").unwrap_or_default(), source)
                    .is_some()
                {
                    node.set_attr("crossorigin", "anonymous");
                }
                continue;
            }
            if let Some((path, _)) = self.local(&node.attr("src").unwrap_or_default(), source)
                && let Some(bytes) = self.files.get(&path)
            {
                let script = text(bytes)?;
                if node.has_attr("defer")
                    && node
                        .attr("type")
                        .is_none_or(|t| !t.eq_ignore_ascii_case("module"))
                {
                    javascript.push_str(script);
                    javascript.push_str("\n;\n");
                    node.remove_from_parent();
                } else {
                    node.remove_attrs(&["src", "integrity", "crossorigin", "async", "defer"]);
                    node.set_text(script.replace("</script", "<\\/script"));
                }
            }
        }
        for node in doc.select("*").nodes() {
            // Imported editor identities are rebuilt with the normal stable-target allocator.
            // Keep the stable IDs of the original editable source.
            for attr in ["src", "href", "poster", "xlink:href", "data"] {
                if let Some(value) = node.attr(attr) {
                    if node
                        .node_name()
                        .is_some_and(|n| matches!(n.as_ref(), "a" | "area" | "iframe"))
                        && (attr == "href" || attr == "src")
                    {
                        if self.keep_scripts {
                            continue;
                        }
                        if let Some((path, suffix)) = self.local(&value, source) {
                            let route = self.routes.get(&path).or_else(|| {
                                self.routes.get(&format!(
                                    "{}index.html",
                                    if path.ends_with('/') || path.is_empty() {
                                        path.clone()
                                    } else {
                                        format!("{path}/")
                                    }
                                ))
                            });
                            if let Some(route) = route {
                                node.set_attr(attr, &format!("{route}{suffix}"));
                            }
                        }
                    } else {
                        node.set_attr(attr, &self.resource(&value, source, &mut BTreeSet::new())?);
                    }
                }
            }
            if let Some(value) = node.attr("style") {
                node.set_attr("style", &self.css(&value, source, &mut BTreeSet::new())?);
            }
            if let Some(value) = node.attr("srcset") {
                // Native file references cannot contain unescaped commas or whitespace.
                let items = value
                    .split(',')
                    .map(|item| {
                        let mut parts = item.split_whitespace();
                        let url = self.resource(
                            parts.next().unwrap_or(""),
                            source,
                            &mut BTreeSet::new(),
                        )?;
                        Ok(format!("{} {}", url, parts.collect::<Vec<_>>().join(" "))
                            .trim_end()
                            .to_owned())
                    })
                    .collect::<Result<Vec<_>>>()?;
                if !value.trim_start().starts_with("data:") {
                    node.set_attr("srcset", &items.join(", "));
                }
            }
            if node
                .node_name()
                .is_some_and(|name| name.as_ref() == "style")
            {
                node.set_text(self.css(&node.text(), source, &mut BTreeSet::new())?);
            }
        }
        crate::stable_targets::assign_document_ids(&doc)?;
        let content = SiteContent {
            html: format!("<!doctype html>\n{}", doc.html_root().html()),
            css: String::new(),
            javascript,
        };
        Ok((title, content))
    }
}

pub fn import_website_files(name: &str, files: &BTreeMap<String, Vec<u8>>) -> Result<Project> {
    let pages: Vec<_> = files
        .keys()
        .filter(|p| {
            (p.to_lowercase().ends_with(".html") || p.to_lowercase().ends_with(".htm"))
                && crate::identity_source_page(p, std::str::from_utf8(&files[*p]).unwrap_or(""))
                    .is_some()
        })
        .filter(|p| {
            files[*p].len() <= crate::MAX_SITE_SOURCE_BYTES
                && std::str::from_utf8(&files[*p]).is_ok()
        })
        .take(100)
        .cloned()
        .collect();
    import_website_pages(name, files, pages)
}

pub(crate) fn import_website_pages(
    name: &str,
    files: &BTreeMap<String, Vec<u8>>,
    pages: Vec<String>,
) -> Result<Project> {
    ensure(
        !pages.is_empty() && pages.len() <= 100,
        "Choose a website folder containing between 1 and 100 HTML pages.",
    )?;
    let home = pages
        .iter()
        .find(|p| p.eq_ignore_ascii_case("index.html") || p.eq_ignore_ascii_case("index.htm"))
        .unwrap_or(&pages[0]);
    let mut routes = BTreeMap::from([(home.clone(), "/".to_string())]);
    let mut used = BTreeSet::from(["/".to_string()]);
    for (index, source) in pages.iter().enumerate().filter(|(_, p)| *p != home) {
        let mut route = crate::source_page_path(source);
        if crate::model::validate_path(&route).is_err() {
            route = format!("/page-{}", index + 1);
        }
        let base = route.clone();
        let mut suffix = 2;
        while !used.insert(route.to_lowercase()) {
            route = format!("{}-{suffix}", base.trim_end_matches('/'));
            suffix += 1;
        }
        routes.insert(source.clone(), route);
    }
    // Encode each picture once, rather than expanding the same bytes into every page.
    let pictures = files
        .iter()
        .filter_map(|(path, bytes)| {
            let mime = image_mime(path)?;
            if bytes.is_empty() {
                return None;
            }
            let uri = format!("data:{mime};base64,{}", STANDARD.encode(bytes));
            Some((
                path.clone(),
                (
                    crate::project_assets::inline_image_id(&uri),
                    std::sync::Arc::from(uri),
                ),
            ))
        })
        .collect();
    let importer = Importer {
        keep_scripts: false,
        inline_assets: true,
        files,
        routes,
        pictures,
    };
    let viewport = Viewport {
        width: 1440,
        height: 900,
    };
    let mut project = Project::new(
        name.chars().take(120).collect(),
        viewport,
        Appearance::Light,
    )?;
    project.pages.clear();
    let mut page_files = BTreeMap::new();
    for source in &pages {
        let original = text(&files[source])?;
        let title = dom_query::Document::from(original)
            .select_single("title")
            .text()
            .trim()
            .chars()
            .take(120)
            .collect::<String>();
        let content = SiteContent {
            html: crate::source::assign_editor_ids(&crate::source::strip_editor_ids(original))?,
            css: String::new(),
            javascript: String::new(),
        };
        let id = Uuid::new_v4();
        if source == home {
            project.home_page_id = id;
        }
        page_files.insert(id, source.clone());
        project.pages.push(Page {
            id,
            name: if title.is_empty() {
                source.chars().take(120).collect()
            } else {
                title
            },
            path: importer.routes[source].clone(),
            included: true,
            viewport,
            appearance: Appearance::Light,
            regions: vec![],
            content: Some(content),
        });
    }
    project.pictures = importer.pictures.values().cloned().collect();
    project.website = Some(ImportedWebsite {
        page_files,
        files: files
            .iter()
            .filter(|(p, _)| !pages.contains(p))
            .map(|(p, bytes)| {
                let value = importer
                    .pictures
                    .get(p)
                    .map(|(id, _)| crate::project_asset_reference(*id))
                    .unwrap_or_else(|| STANDARD.encode(bytes));
                (p.clone(), value.into())
            })
            .collect(),
        framework: None,
        skipped: files
            .keys()
            .filter(|p| (p.ends_with(".html") || p.ends_with(".htm")) && !pages.contains(p))
            .take(256)
            .cloned()
            .collect(),
    });
    project.store_inline_images()?;
    project.validate()?;
    Ok(project)
}

/// Byte threshold above which a binary preview file stays a reference, not a copy.
pub const PREVIEW_INLINE_BYTES: usize = 1024 * 1024;
/// Preview-only route for pictures that have no file of their own in the imported site.
const PREVIEW_ASSET_DIRECTORY: &str = "__desegnos_assets__";

/// One preview resource. Inline bytes are copied into a session; encoded and picture values
/// point at bytes the project already shares, and a rendered page builds its bytes on first use.
#[derive(Clone)]
pub enum PreviewFile {
    Inline(Arc<[u8]>),
    Encoded(Arc<str>),
    Picture(Arc<str>),
    /// A live page that renders the first time it is served, not when the package is built.
    Rendered(Arc<LivePage>),
}

/// One page of a live package, rendered against the package's shared inline sources on demand.
pub struct LivePage {
    source: String,
    render: Arc<BTreeMap<String, Vec<u8>>>,
    text_limit: usize,
    rendered: OnceLock<std::result::Result<Arc<[u8]>, String>>,
}

impl LivePage {
    pub fn bytes(&self) -> Result<Arc<[u8]>> {
        match self
            .rendered
            .get_or_init(|| self.render().map_err(|error| error.0))
        {
            Ok(bytes) => Ok(bytes.clone()),
            Err(error) => Err(Error(error.clone())),
        }
    }

    fn render(&self) -> Result<Arc<[u8]>> {
        let importer = Importer {
            keep_scripts: true,
            inline_assets: false,
            files: &self.render,
            routes: BTreeMap::new(),
            pictures: BTreeMap::new(),
        };
        let (_, content) = importer.page(&self.source)?;
        let bytes = content.html.into_bytes();
        ensure(
            bytes.len() <= self.text_limit,
            preview_too_large(&self.source, bytes.len()),
        )?;
        Ok(bytes.into())
    }

    #[cfg(test)]
    fn is_rendered(&self) -> bool {
        self.rendered.get().is_some()
    }
}

impl PreviewFile {
    /// Byte length of an inline file; referenced files have no session copy.
    pub fn inline_len(&self) -> Option<usize> {
        match self {
            Self::Inline(bytes) => Some(bytes.len()),
            Self::Rendered(page) => page.bytes().ok().map(|bytes| bytes.len()),
            Self::Encoded(_) | Self::Picture(_) => None,
        }
    }

    /// Decoded length of the full resource, without decoding its bytes.
    pub fn decoded_len(&self) -> Option<usize> {
        match self {
            Self::Inline(bytes) => Some(bytes.len()),
            Self::Rendered(page) => page.bytes().ok().map(|bytes| bytes.len()),
            Self::Encoded(encoded) => base64_decoded_len(encoded),
            Self::Picture(uri) => base64_decoded_len(base64_body(uri)),
        }
    }

    /// Bytes for an inclusive range, decoding only the base64 groups that cover it.
    pub fn read_range(&self, start: usize, end: usize) -> Result<Vec<u8>> {
        let body = match self {
            Self::Inline(bytes) => return inline_range(bytes, start, end),
            Self::Rendered(page) => {
                let bytes = page.bytes()?;
                return inline_range(&bytes, start, end);
            }
            Self::Encoded(encoded) => encoded,
            Self::Picture(uri) => base64_body(uri),
        };
        decode_base64_range(body, start, end)
    }
}

fn inline_range(bytes: &[u8], start: usize, end: usize) -> Result<Vec<u8>> {
    bytes
        .get(start..=end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Error("The preview resource is unavailable.".into()))
}

fn base64_body(value: &str) -> &str {
    value.split_once(";base64,").map_or(value, |(_, body)| body)
}

fn base64_decoded_len(value: &str) -> Option<usize> {
    let body = value.trim_end();
    if !body.len().is_multiple_of(4) {
        return None;
    }
    let padding = body
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count()
        .min(2);
    (body.len() / 4 * 3).checked_sub(padding)
}

fn decode_base64_range(body: &str, start: usize, end: usize) -> Result<Vec<u8>> {
    let invalid = || Error("The preview resource is unavailable.".into());
    let decoded_len = base64_decoded_len(body).ok_or_else(invalid)?;
    if start > end || end >= decoded_len {
        return Err(invalid());
    }
    // Base64 stores three bytes per four characters, so one group covers a whole range.
    let first = start / 3;
    let last = end / 3;
    let chars = &body.as_bytes()[first * 4..((last + 1) * 4).min(body.len())];
    let decoded = STANDARD.decode(chars).map_err(|_| invalid())?;
    let offset = start - first * 3;
    decoded
        .get(offset..offset + end - start + 1)
        .map(<[u8]>::to_vec)
        .ok_or_else(invalid)
}

fn preview_text_path(path: &str) -> bool {
    matches!(
        path.rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "html" | "htm" | "css" | "js" | "mjs"
    )
}

fn preview_too_large(path: &str, bytes: usize) -> String {
    format!(
        "{path} is too large to preview ({} MB).",
        bytes.div_ceil(1024 * 1024)
    )
}

fn check_preview_text(files: &BTreeMap<String, PreviewFile>, text_limit: usize) -> Result<()> {
    let mut text_bytes = 0usize;
    let mut largest: Option<(String, usize)> = None;
    for (path, file) in files {
        let Some(len) = file.inline_len().filter(|_| preview_text_path(path)) else {
            continue;
        };
        text_bytes = text_bytes.saturating_add(len);
        if largest.as_ref().is_none_or(|(_, size)| *size < len) {
            largest = Some((path.clone(), len));
        }
    }
    if text_bytes > text_limit {
        let (path, len) = largest.expect("text bytes are attributed to a file");
        return Err(Error(preview_too_large(&path, len)));
    }
    Ok(())
}

/// Original file layout and bounded local resources, retained across saves and recovery.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedWebsite {
    pub page_files: BTreeMap<Uuid, String>,
    pub files: BTreeMap<String, Arc<str>>,
    pub framework: Option<String>,
    pub skipped: Vec<String>,
}
/// Page scripts stay inside the document. Appending them after `</html>` produced pages that
/// browsers render but that no HTML parser, including this importer, accepts.
fn insert_page_script(html: &mut String, tag: &str) {
    let lower = html.to_ascii_lowercase();
    let at = lower
        .rfind("</body>")
        .or_else(|| lower.rfind("</html>"))
        .unwrap_or(html.len());
    html.insert_str(at, tag);
}

impl Project {
    pub fn website_files(&self) -> Result<Option<BTreeMap<String, Vec<u8>>>> {
        self.website_source_files(|_, encoded| self.decode_site_file(encoded))
    }

    /// Canvas documents only read HTML, CSS, and scripts as text; other files just need a path.
    fn website_canvas_files(&self) -> Result<Option<BTreeMap<String, Vec<u8>>>> {
        self.website_source_files(|path, encoded| {
            if preview_text_path(path) {
                self.decode_site_file(encoded)
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn decode_site_file(&self, encoded: &str) -> Result<Vec<u8>> {
        let value = self
            .resolve_site_content(&SiteContent {
                html: encoded.to_string(),
                css: String::new(),
                javascript: String::new(),
            })?
            .html;
        let value = value
            .strip_prefix("data:")
            .and_then(|v| v.split_once(";base64,"))
            .map_or(value.as_str(), |(_, v)| v);
        STANDARD
            .decode(value)
            .map_err(|_| Error("Invalid imported website resource.".into()))
    }

    fn website_source_files(
        &self,
        decode: impl Fn(&str, &str) -> Result<Vec<u8>>,
    ) -> Result<Option<BTreeMap<String, Vec<u8>>>> {
        let Some(site) = &self.website else {
            return Ok(None);
        };
        let mut files = BTreeMap::new();
        for (path, encoded) in &site.files {
            files.insert(path.clone(), decode(path, encoded)?);
        }
        for page in self.pages.iter().filter(|p| p.included) {
            if let Some(content) = &page.content {
                let path = site
                    .page_files
                    .get(&page.id)
                    .cloned()
                    .unwrap_or_else(|| crate::export::output_path(&page.path));
                let resolved = self.resolve_site_content(content)?;
                let mut html = resolved.html.clone();
                if !resolved.css.is_empty() {
                    let style = format!(
                        "<style>{}</style>",
                        resolved.css.replace("</style", "<\\/style")
                    );
                    if let Some(at) = html.to_ascii_lowercase().rfind("</head>") {
                        html.insert_str(at, &style);
                    } else {
                        html.insert_str(0, &style);
                    }
                }
                if !resolved.javascript.is_empty() {
                    insert_page_script(
                        &mut html,
                        &format!(
                            "<script>{}</script>",
                            resolved.javascript.replace("</script", "<\\/script")
                        ),
                    );
                }
                let runtime = crate::region_script_runtime(&resolved.html)?;
                if !runtime.is_empty() {
                    let name = format!("desegnos-regions-{}.js", crate::digest(runtime.as_bytes()));
                    // Resolve the resource beside the page, including nested imported routes.
                    let resource = path
                        .rsplit_once('/')
                        .map_or(name.clone(), |(directory, _)| format!("{directory}/{name}"));
                    insert_page_script(
                        &mut html,
                        &format!("<script defer src=\"{name}\"></script>"),
                    );
                    files.insert(resource, runtime.into_bytes());
                }
                files.insert(path, html.into_bytes());
            }
        }
        Ok(Some(files))
    }
    pub fn website_live_files(&self) -> Result<Option<BTreeMap<String, Vec<u8>>>> {
        let Some(mut files) = self.website_files()? else {
            return Ok(None);
        };
        let importer = Importer {
            keep_scripts: true,
            inline_assets: true,
            files: &files,
            routes: BTreeMap::new(),
            pictures: BTreeMap::new(),
        };
        let mut rendered = BTreeMap::new();
        for source in self
            .website
            .as_ref()
            .expect("website")
            .page_files
            .values()
            .filter(|source| files.contains_key(*source))
        {
            let (_, content) = importer.page(source)?;
            rendered.insert(source.clone(), content.html.into_bytes());
        }
        files.extend(rendered);
        Ok(Some(files))
    }
    /// Preview resources keep text and small files as inline bytes, while larger binary files
    /// stay references to bytes the project already holds. No whole-site copy is made.
    pub fn website_preview_files(
        &self,
        run_scripts: bool,
        text_limit: usize,
    ) -> Result<Option<BTreeMap<String, PreviewFile>>> {
        let Some(mut files) = self.website_preview_package(run_scripts, text_limit)? else {
            return Ok(None);
        };
        for file in files.values_mut() {
            let PreviewFile::Rendered(page) = file else {
                continue;
            };
            *file = PreviewFile::Inline(page.bytes()?);
        }
        check_preview_text(&files, text_limit)?;
        Ok(Some(files))
    }

    /// The same package with live pages deferred: each page renders only when it is served.
    pub fn website_preview_package(
        &self,
        run_scripts: bool,
        text_limit: usize,
    ) -> Result<Option<BTreeMap<String, PreviewFile>>> {
        let Some(site) = &self.website else {
            return Ok(None);
        };
        let pictures = self.preview_pictures();
        let mut paths_by_picture = BTreeMap::new();
        let mut files = BTreeMap::new();
        for (path, encoded) in &site.files {
            let reference = encoded
                .strip_prefix(crate::PROJECT_ASSET_SCHEME)
                .and_then(|value| Uuid::parse_str(value).ok());
            if let Some(id) = reference {
                let uri = pictures
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| Error("An imported website picture is missing.".into()))?;
                paths_by_picture.insert(id, path.clone());
                files.insert(path.clone(), PreviewFile::Picture(uri));
            } else if !preview_text_path(path) && encoded.len() > PREVIEW_INLINE_BYTES {
                files.insert(path.clone(), PreviewFile::Encoded(encoded.clone()));
            } else {
                let bytes = STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(|_| Error("Invalid imported website resource.".into()))?;
                files.insert(path.clone(), PreviewFile::Inline(bytes.into()));
            }
        }
        for page in self.pages.iter().filter(|page| page.included) {
            let Some(content) = &page.content else {
                continue;
            };
            let path = site
                .page_files
                .get(&page.id)
                .cloned()
                .unwrap_or_else(|| crate::export::output_path(&page.path));
            let html =
                self.preview_source(&content.html, &pictures, &paths_by_picture, &mut files)?;
            let css =
                self.preview_source(&content.css, &pictures, &paths_by_picture, &mut files)?;
            let javascript = self.preview_source(
                &content.javascript,
                &pictures,
                &paths_by_picture,
                &mut files,
            )?;
            let mut document = html;
            if !css.is_empty() {
                let style = format!("<style>{}</style>", css.replace("</style", "<\\/style"));
                if let Some(at) = document.to_ascii_lowercase().rfind("</head>") {
                    document.insert_str(at, &style);
                } else {
                    document.insert_str(0, &style);
                }
            }
            if !javascript.is_empty() {
                insert_page_script(
                    &mut document,
                    &format!(
                        "<script>{}</script>",
                        javascript.replace("</script", "<\\/script")
                    ),
                );
            }
            let runtime = crate::region_script_runtime(&document)?;
            if !runtime.is_empty() {
                let name = format!("desegnos-regions-{}.js", crate::digest(runtime.as_bytes()));
                let resource = path
                    .rsplit_once('/')
                    .map_or(name.clone(), |(directory, _)| format!("{directory}/{name}"));
                insert_page_script(
                    &mut document,
                    &format!("<script defer src=\"{name}\"></script>"),
                );
                files.insert(resource, PreviewFile::Inline(runtime.into_bytes().into()));
            }
            ensure(
                document.len() <= text_limit,
                preview_too_large(&path, document.len()),
            )?;
            files.insert(path, PreviewFile::Inline(document.into_bytes().into()));
        }
        // A rendered page is measured when it renders, so live packages are sized by their
        // static text only; the eager caller checks the forced result again.
        check_preview_text(&files, text_limit)?;
        if run_scripts {
            // Render scripts against inline sources only; larger files stay referenceable URLs.
            let render: Arc<BTreeMap<String, Vec<u8>>> = Arc::new(
                files
                    .iter()
                    .map(|(path, file)| {
                        (
                            path.clone(),
                            match file {
                                PreviewFile::Inline(bytes) => bytes.to_vec(),
                                // Path presence is enough to resolve referenced assets; their
                                // bytes remain shared in the package and are never inlined.
                                _ => Vec::new(),
                            },
                        )
                    })
                    .collect(),
            );
            for source in site
                .page_files
                .values()
                .filter(|source| render.contains_key(*source))
            {
                files.insert(
                    source.clone(),
                    PreviewFile::Rendered(Arc::new(LivePage {
                        source: source.clone(),
                        render: render.clone(),
                        text_limit,
                        rendered: OnceLock::new(),
                    })),
                );
            }
        }
        Ok(Some(files))
    }
    /// Point image references at preview URLs, keeping the session text free of picture bytes.
    fn preview_source(
        &self,
        source: &str,
        pictures: &BTreeMap<Uuid, Arc<str>>,
        paths_by_picture: &BTreeMap<Uuid, String>,
        files: &mut BTreeMap<String, PreviewFile>,
    ) -> Result<String> {
        let mut output = String::with_capacity(source.len());
        let mut rest = source;
        while let Some(index) = rest.find(crate::PROJECT_ASSET_SCHEME) {
            output.push_str(&rest[..index]);
            rest = &rest[index + crate::PROJECT_ASSET_SCHEME.len()..];
            let id = rest
                .get(..36)
                .and_then(|value| Uuid::parse_str(value).ok())
                .ok_or_else(|| Error("An image reference is invalid.".into()))?;
            let uri = pictures.get(&id).ok_or_else(|| {
                Error("An image reference is not in this project. Your page is unchanged.".into())
            })?;
            let url = if let Some(path) = paths_by_picture.get(&id) {
                format!("/{path}")
            } else {
                let path = format!("{PREVIEW_ASSET_DIRECTORY}/{id}");
                files
                    .entry(path.clone())
                    .or_insert_with(|| PreviewFile::Picture(uri.clone()));
                format!("/{path}")
            };
            output.push_str(&url);
            rest = &rest[36..];
        }
        output.push_str(rest);
        Ok(output)
    }
    fn preview_pictures(&self) -> BTreeMap<Uuid, Arc<str>> {
        let mut pictures = self.pictures.clone();
        if let Some(identity) = &self.imported_identity {
            for asset in &identity.assets {
                pictures
                    .entry(asset.id)
                    .or_insert_with(|| Arc::from(asset.data_uri.as_str()));
            }
            for photo in &identity.photos {
                pictures
                    .entry(photo.id)
                    .or_insert_with(|| photo.data_uri.clone());
            }
        }
        for page in &self.pages {
            let Some(content) = &page.content else {
                continue;
            };
            for source in [&content.html, &content.css, &content.javascript] {
                for uri in crate::project_assets::inline_images(source) {
                    pictures
                        .entry(crate::project_assets::inline_image_id(uri))
                        .or_insert_with(|| Arc::from(uri));
                }
            }
        }
        pictures
    }
    /// Rendering changes are disposable; the editor source and saved resources remain intact.
    pub fn website_canvas_content(&self, page: &Page) -> Result<Option<SiteContent>> {
        let Some(files) = self.website_canvas_files()? else {
            return Ok(None);
        };
        let site = self.website.as_ref().expect("website files");
        let Some(source) = site.page_files.get(&page.id) else {
            return Ok(None);
        };
        let routes = self
            .pages
            .iter()
            .filter_map(|p| {
                site.page_files
                    .get(&p.id)
                    .map(|f| (f.clone(), p.path.clone()))
            })
            .collect();
        let importer = Importer {
            keep_scripts: false,
            inline_assets: false,
            files: &files,
            routes,
            pictures: BTreeMap::new(),
        };
        let (_, mut content) = importer.page(source)?;
        if let Some(authored) = &page.content {
            content.css.clone_from(&authored.css);
        }
        Ok(Some(content))
    }
}

impl ImportedWebsite {
    pub(crate) fn validate(&self, project: &Project) -> Result<()> {
        let valid = |path: &str| {
            !path.is_empty()
                && path.len() <= 4096
                && !path.contains(['\\', ':', '\0'])
                && path.split('/').all(|part| {
                    !part.is_empty() && !part.starts_with('.') && !part.ends_with(['.', ' '])
                })
        };
        ensure(
            self.files.len() <= 10_000 && self.page_files.len() <= 100 && self.skipped.len() <= 512,
            "The imported website exceeds its file limits.",
        )?;
        ensure(
            self.files
                .keys()
                .chain(self.page_files.values())
                .all(|p| valid(p)),
            "An imported website path is invalid.",
        )?;
        let mut bytes = 0usize;
        for value in self.files.values() {
            bytes = bytes.saturating_add(value.len());
            ensure(
                bytes <= 128 * 1024 * 1024,
                "The imported website exceeds its resource limit.",
            )?;
            if let Some(id) = value.strip_prefix(crate::PROJECT_ASSET_SCHEME) {
                ensure(
                    Uuid::parse_str(id)
                        .ok()
                        .is_some_and(|id| project.pictures.contains_key(&id)),
                    "An imported website picture is missing.",
                )?;
            }
        }
        ensure(
            self.framework
                .as_ref()
                .is_none_or(|s| !s.is_empty() && s.len() <= 80),
            "The imported framework name is invalid.",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Project;

    const LIMIT: usize = 16 * 1024 * 1024;

    fn site() -> Project {
        let files = BTreeMap::from([
            ("index.html".into(), b"<!doctype html><html><head><link rel='stylesheet' href='/site.css'></head><body><h1>Home</h1><a href='about.html'>About</a><script type='module' src='main.js'></script></body></html>".to_vec()),
            ("about.html".into(), b"<!doctype html><html><body><h1>About</h1><script>document.body.dataset.ready='yes';</script></body></html>".to_vec()),
            ("contact.html".into(), b"<!doctype html><html><body><h1>Contact</h1></body></html>".to_vec()),
            ("site.css".into(), b"h1{color:red}".to_vec()),
            ("main.js".into(), b"import './module.js'; document.body.dataset.home='yes';".to_vec()),
            ("module.js".into(), b"document.body.dataset.loaded='yes';".to_vec()),
        ]);
        crate::import_website_files("Website", &files).unwrap()
    }

    fn body(file: &PreviewFile) -> Vec<u8> {
        let len = file.decoded_len().expect("inline resource");
        if len == 0 {
            return vec![];
        }
        file.read_range(0, len - 1).expect("range")
    }

    fn inline_files(files: &BTreeMap<String, PreviewFile>) -> BTreeMap<String, Vec<u8>> {
        files
            .iter()
            .map(|(path, file)| {
                (
                    path.clone(),
                    match file {
                        PreviewFile::Inline(bytes) => bytes.to_vec(),
                        _ => Vec::new(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn live_pages_render_on_request_and_match_the_eager_package() {
        let project = site();
        let eager = project.website_preview_files(true, LIMIT).unwrap().unwrap();
        let mut lazy = project
            .website_preview_package(true, LIMIT)
            .unwrap()
            .unwrap();
        let pages: BTreeSet<String> = project
            .website
            .as_ref()
            .unwrap()
            .page_files
            .values()
            .cloned()
            .collect();
        assert!(pages.len() > 1);
        for path in &pages {
            match &lazy[path] {
                PreviewFile::Rendered(page) => {
                    assert!(!page.is_rendered(), "{path} rendered eagerly");
                }
                _ => panic!("{path} is not a live page"),
            }
        }
        for file in lazy.values_mut() {
            let PreviewFile::Rendered(page) = file else {
                continue;
            };
            *file = PreviewFile::Inline(page.bytes().unwrap());
        }
        assert_eq!(eager.len(), lazy.len());
        for (path, file) in &eager {
            assert_eq!(body(file), body(&lazy[path]), "{path}");
        }
        // The old algorithm rendered each page from the static inline sources.
        let static_files = project
            .website_preview_package(false, LIMIT)
            .unwrap()
            .unwrap();
        let render = inline_files(&static_files);
        let importer = Importer {
            keep_scripts: true,
            inline_assets: false,
            files: &render,
            routes: BTreeMap::new(),
            pictures: BTreeMap::new(),
        };
        for source in &pages {
            let (_, content) = importer.page(source).unwrap();
            assert_eq!(content.html.into_bytes(), body(&eager[source]), "{source}");
        }
    }

    fn media_files() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([
            ("index.html".into(), b"<!doctype html><html><head><link rel='stylesheet' href='site.css'></head><body><img src='images/photo.png'><img src='/portrait/pose.webp'><p style=\"background:url('images/tile.png')\"></p></body></html>".to_vec()),
            ("site.css".into(), b"body{background:url(images/bg.png)}".to_vec()),
            ("images/photo.png".into(), b"photo".to_vec()),
            ("images/bg.png".into(), b"bg".to_vec()),
            ("images/tile.png".into(), b"tile".to_vec()),
            ("portrait/pose.webp".into(), b"pose".to_vec()),
        ])
    }

    fn media_site() -> Project {
        crate::import_website_files("Website", &media_files()).unwrap()
    }

    #[test]
    fn canvas_content_references_site_assets_by_path() {
        let project = media_site();
        let page = project.page(project.home_page_id).unwrap();
        let content = project.website_canvas_content(page).unwrap().unwrap();
        // Root-relative sources (`/portrait/...`) resolve from the site root like relative ones.
        for reference in [
            "/images/photo.png",
            "/images/bg.png",
            "/images/tile.png",
            "\"/portrait/pose.webp\"",
        ] {
            assert!(
                content.html.contains(reference),
                "{reference} in {}",
                content.html
            );
        }
        assert!(!content.html.contains("data:image/"), "{}", content.html);
    }

    #[test]
    fn scripted_canvas_package_keeps_assets_referenced_and_source_unchanged() {
        let project = media_site();
        let before = project.clone();
        let package = project
            .website_preview_package(true, LIMIT)
            .unwrap()
            .unwrap();
        let rendered = String::from_utf8(body(&package["index.html"])).unwrap();
        assert!(rendered.contains("/images/photo.png"), "{rendered}");
        assert!(rendered.contains("/images/bg.png"));
        assert!(!rendered.contains("data:image/"));
        assert_eq!(project, before);
    }

    #[test]
    fn canvas_content_does_not_decode_binary_site_files() {
        let mut project = media_site();
        project
            .website
            .as_mut()
            .unwrap()
            .files
            .insert("images/photo.png".into(), "not base64!".into());
        assert!(project.website_files().is_err());
        let page = project.page(project.home_page_id).unwrap();
        let content = project.website_canvas_content(page).unwrap().unwrap();
        assert!(content.html.contains("/images/photo.png"));
    }

    #[test]
    fn inline_imports_still_embed_site_assets() {
        let files = media_files();
        let importer = Importer {
            keep_scripts: true,
            inline_assets: true,
            files: &files,
            routes: BTreeMap::new(),
            pictures: BTreeMap::new(),
        };
        let (_, content) = importer.page("index.html").unwrap();
        assert!(
            content.html.contains("data:image/png;base64,"),
            "{}",
            content.html
        );
        assert!(!content.html.contains("/images/"), "{}", content.html);
    }

    #[test]
    fn render_errors_are_cached_and_read_as_unavailable() {
        let file = PreviewFile::Rendered(Arc::new(LivePage {
            source: "broken.html".into(),
            render: Arc::new(BTreeMap::from([("broken.html".into(), vec![0xff])])),
            text_limit: LIMIT,
            rendered: OnceLock::new(),
        }));
        let PreviewFile::Rendered(page) = &file else {
            unreachable!()
        };
        let error = page.bytes().unwrap_err();
        assert_eq!(page.bytes().unwrap_err(), error);
        assert_eq!(file.inline_len(), None);
        assert_eq!(file.decoded_len(), None);
        assert!(file.read_range(0, 0).is_err());
    }

    fn median(mut values: [f64; 5]) -> f64 {
        values.sort_by(f64::total_cmp);
        values[2]
    }

    /// DESEGNOS_BENCH_JOURNAL=<journal> cargo test --release -p desegnos-core live_preview_bench -- --ignored --nocapture
    #[test]
    #[ignore = "timing"]
    fn live_preview_bench() {
        let Ok(journal) = std::env::var("DESEGNOS_BENCH_JOURNAL") else {
            eprintln!("DESEGNOS_BENCH_JOURNAL is not set");
            return;
        };
        let text = std::fs::read_to_string(journal).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let project: Project = serde_json::from_value(value["project"].clone()).unwrap();
        let pages = project.website.as_ref().unwrap().page_files.len();
        let mut eager = [0.0; 5];
        let mut lazy = [0.0; 5];
        let mut served = [0.0; 5];
        for run in 0..5 {
            let start = std::time::Instant::now();
            project.website_preview_files(true, LIMIT).unwrap();
            eager[run] = start.elapsed().as_secs_f64() * 1000.0;

            let start = std::time::Instant::now();
            let package = project
                .website_preview_package(true, LIMIT)
                .unwrap()
                .unwrap();
            lazy[run] = start.elapsed().as_secs_f64() * 1000.0;

            let page = package
                .values()
                .find_map(|file| match file {
                    PreviewFile::Rendered(page) => Some(page),
                    _ => None,
                })
                .expect("a live page");
            let start = std::time::Instant::now();
            page.bytes().unwrap();
            served[run] = start.elapsed().as_secs_f64() * 1000.0;
        }
        println!(
            "{pages} pages: eager {:.1} ms, lazy package {:.1} ms, lazy + 1 page {:.1} ms",
            median(eager),
            median(lazy),
            median(served)
        );
    }
}
