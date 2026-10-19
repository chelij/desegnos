//! Compile accepted source to portable files without evaluating authored code.
use crate::{Error, Project, Result, digest, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use cssparser::{Parser, ParserInput, Token};
use dom_query::Document;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsiteExport {
    pub files: BTreeMap<String, Vec<u8>>,
    pub page_count: usize,
    pub warnings: Vec<String>,
}

struct Compiler<'a> {
    project: &'a Project,
    files: BTreeMap<String, Vec<u8>>,
    warnings: BTreeSet<String>,
    routes: BTreeMap<String, String>,
    css_files: BTreeMap<String, String>,
}

/// All included pages are compiled from this exact validated revision. Private
/// notes, identity references, prompts, history and editor code are unreachable.
pub fn compile_website(project: &Project) -> Result<WebsiteExport> {
    project.validate()?;
    ensure(
        project
            .pages
            .iter()
            .any(|p| p.included && p.content.is_some()),
        "There is no website content to export. Add content to an included page first.",
    )?;
    let routes = project
        .pages
        .iter()
        .filter(|p| p.included)
        .map(|p| (p.path.clone(), output_path(&p.path)))
        .collect();
    let mut compiler = Compiler {
        project,
        files: BTreeMap::new(),
        warnings: BTreeSet::new(),
        routes,
        css_files: BTreeMap::new(),
    };
    for page in project.pages.iter().filter(|p| p.included) {
        let path = output_path(&page.path);
        let doc = Document::from(page.content.as_ref().map(|c| c.html.as_str()).unwrap_or(""));
        let mut pending = vec![(doc.root(), 0)];
        let mut count = 0;
        while let Some((node, depth)) = pending.pop() {
            count += 1;
            ensure(
                count <= 30_000 && depth <= 256,
                format!("{} is too complex to export safely.", page.name),
            )?;
            pending.extend(node.children().into_iter().map(|c| (c, depth + 1)));
        }
        ensure(
            doc.select("base[href]").nodes().is_empty(),
            format!(
                "{} uses a base URL. Remove its <base href> before portable export; it changes every local destination.",
                page.name
            ),
        )?;
        let head = doc.select_single("head").nodes()[0];
        for charset in doc.select("meta[charset]").nodes() {
            charset.set_attr("charset", "utf-8");
        }
        let body = doc.select_single("body").nodes()[0];
        if doc.select("meta[charset]").nodes().is_empty() {
            head.prepend_html("<meta charset=\"utf-8\">");
        }
        if doc.select("meta[name=viewport]").nodes().is_empty() {
            head.append_html(
                "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
            );
        }
        if doc.select("title").nodes().is_empty() {
            head.append_html("<title></title>");
            doc.select_single("title").nodes()[0].set_text(page.name.as_str());
        }
        for node in doc.select("*").nodes() {
            let tag = node.node_name().unwrap_or_default().to_string();
            for attribute in node.attrs() {
                let name = attribute.name.local.to_string();
                let value = attribute.value.to_string();
                if name == crate::FOUND_CONTENT_ATTRIBUTE || name == crate::ELEMENT_ID_ATTRIBUTE {
                    node.remove_attr(&name);
                } else if name == "style" {
                    node.set_attr(&name, &compiler.css(&value, &path, &page.path)?);
                } else if name == "srcset" || name == "imagesrcset" {
                    node.set_attr(&name, &compiler.srcset(&value, &path, &page.path)?);
                } else if [
                    "src",
                    "poster",
                    "href",
                    "xlink:href",
                    "action",
                    "formaction",
                    "data",
                ]
                .contains(&name.as_str())
                {
                    let navigation = ((tag == "a" || tag == "area") && name == "href")
                        || (tag == "iframe" && name == "src");
                    node.set_attr(&name, &compiler.url(&value, &path, &page.path, navigation)?);
                }
            }
            if tag == "style" {
                node.set_text(compiler.css(&node.text(), &path, &page.path)?);
            }
            if (tag == "script" && node.attr("type").is_none_or(|x| !x.contains("json")))
                || node.attrs().iter().any(|a| a.name.local.starts_with("on"))
            {
                compiler.warn(&page.path, "Authored inline scripts and event handlers are preserved. Their dynamic destinations and runtime behavior need manual verification.");
            }
            if node.attr("srcdoc").is_some() {
                compiler.warn(&page.path, "An embedded HTML document is preserved unchanged. Its resources and navigation require manual verification.");
            }
            if tag == "script"
                && node
                    .attr("type")
                    .is_some_and(|x| x.eq_ignore_ascii_case("module"))
            {
                compiler.warn(&page.path, "Module scripts can be blocked when opened from disk. Test this page on a static host as well.");
            }
            if tag == "form" {
                compiler.warn(&page.path, "Forms retain their authored configuration. Submission endpoints and server behavior were not tested.");
            }
            if tag == "meta"
                && node
                    .attr("http-equiv")
                    .is_some_and(|x| x.eq_ignore_ascii_case("content-type"))
            {
                node.set_attr("content", "text/html; charset=utf-8");
            }
            if tag == "meta"
                && node
                    .attr("http-equiv")
                    .is_some_and(|x| x.eq_ignore_ascii_case("content-security-policy"))
            {
                compiler.warn(&page.path, "An authored Content Security Policy is preserved. Verify it allows the exported styles, scripts and resources.");
            }
            if tag == "meta"
                && node
                    .attr("http-equiv")
                    .is_some_and(|x| x.eq_ignore_ascii_case("refresh"))
            {
                compiler.warn(&page.path, "A meta refresh is present. Its destination is preserved and needs manual verification.");
            }
        }
        if let Some(content) = &page.content {
            if !content.css.trim().is_empty() {
                // Different page styles never share a cascade. Identical styles
                // share one file, and relative CSS resources resolve from that file.
                let rewritten = compiler.css(&content.css, "styles.css", &page.path)?;
                let hash = digest(rewritten.as_bytes());
                let css_path = if let Some(existing) = compiler.css_files.get(&hash) {
                    existing.clone()
                } else {
                    let name = if compiler.css_files.is_empty() {
                        "styles.css".into()
                    } else {
                        format!("assets/styles-{hash}.css")
                    };
                    // Asset URLs are based at export root; nested CSS needs ../.
                    let css = if name.starts_with("assets/") {
                        compiler.css(&content.css, &name, &page.path)?
                    } else {
                        rewritten
                    };
                    compiler.insert(name.clone(), css.into_bytes())?;
                    compiler.css_files.insert(hash, name.clone());
                    name
                };
                head.append_html(format!(
                    "<link rel=\"stylesheet\" href=\"{}\">",
                    relative(&path, &css_path)
                ));
            }
            if !content.javascript.trim().is_empty() {
                let script = format!("assets/page-{}.js", digest(content.javascript.as_bytes()));
                compiler.insert(script.clone(), content.javascript.as_bytes().to_vec())?;
                body.append_html(format!(
                    "<script defer src=\"{}\"></script>",
                    relative(&path, &script)
                ));
                compiler.warn(&page.path, "Custom JavaScript is included unchanged. Network requests, dynamic navigation and browser-specific behavior need manual verification.");
            }
        } else {
            compiler.warn(&page.path, "This included page is blank.");
        }
        compiler.insert(
            path,
            format!("<!doctype html>\n{}", doc.html_root().html()).into_bytes(),
        )?;
    }
    compiler.warn("Website", "This is a local website export, not a publication or accessibility certification. Public URL metadata, hosting configuration and external services require review.");
    let warnings: Vec<_> = std::mem::take(&mut compiler.warnings).into_iter().collect();
    let readme = format!(
        "{} — website export\n\nOpen index.html in your browser. Every included page has its own HTML file. Upload this entire folder to a static host to share the site. No Desegnos runtime is required.\n\nExported project revision: {}\nPages: {}\n\nReview before sharing\n{}\n\nOnly authored website content is exported. Private project notes, prompts, identity reference files, credentials and editor history are excluded. Authored website code and any resources you placed in it remain your responsibility.\n",
        project.name,
        project.revision,
        compiler.routes.len(),
        warnings
            .iter()
            .map(|w| format!("- {w}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    compiler.insert("README.txt".into(), readme.into_bytes())?;
    Ok(WebsiteExport {
        files: compiler.files,
        page_count: compiler.routes.len(),
        warnings,
    })
}

fn output_path(route: &str) -> String {
    if route == "/" {
        "index.html".into()
    } else {
        format!("{}/index.html", &route[1..])
    }
}
fn encode_path(path: &str) -> String {
    path.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn relative(from: &str, to: &str) -> String {
    let from: Vec<_> = from.split('/').collect();
    let to: Vec<_> = to.split('/').collect();
    let common = from[..from.len() - 1]
        .iter()
        .zip(&to)
        .take_while(|(a, b)| a == b)
        .count();
    encode_path(&format!(
        "{}{}",
        "../".repeat(from.len() - 1 - common),
        to[common..].join("/")
    ))
}
fn decode_percent(source: &str) -> Result<Vec<u8>> {
    let bytes = source.as_bytes();
    let mut output = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            ensure(
                i + 2 < bytes.len(),
                "An embedded resource has incomplete percent encoding.",
            )?;
            output.push(
                std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|s| u8::from_str_radix(s, 16).ok())
                    .ok_or(())
                    .map_err(|_| {
                        Error("An embedded resource has invalid percent encoding.".into())
                    })?,
            );
            i += 3;
        } else {
            output.push(bytes[i]);
            i += 1;
        }
    }
    Ok(output)
}
impl Compiler<'_> {
    fn warn(&mut self, page: &str, message: &str) {
        self.warnings.insert(format!("{page}: {message}"));
    }
    fn insert(&mut self, path: String, bytes: Vec<u8>) -> Result<()> {
        ensure(
            !self.files.keys().any(|p| {
                p.eq_ignore_ascii_case(&path) && (p != &path || self.files[p] != bytes)
                    || p.to_lowercase()
                        .starts_with(&format!("{}/", path.to_lowercase()))
                    || path
                        .to_lowercase()
                        .starts_with(&format!("{}/", p.to_lowercase()))
            }),
            format!("Export paths collide at {path}. Rename that page path."),
        )?;
        self.files.insert(path, bytes);
        ensure(
            self.files.len() <= 10_000
                && self.files.values().map(Vec::len).sum::<usize>() <= 128 * 1024 * 1024,
            "This export exceeds the 128 MiB or 10,000 file limit.",
        )
    }
    fn url(&mut self, value: &str, file: &str, route: &str, navigation: bool) -> Result<String> {
        let value = value.trim();
        if value.is_empty() || value.starts_with('#') {
            return Ok(value.into());
        }
        if value
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case("data:"))
        {
            let Some((header, payload)) = value[5..].split_once(',') else {
                return Err(Error(
                    "An embedded resource is missing its data separator.".into(),
                ));
            };
            let (payload, fragment) = payload
                .find('#')
                .map(|index| (&payload[..index], &payload[index..]))
                .unwrap_or((payload, ""));
            let mime = header.split(';').next().unwrap_or("").to_ascii_lowercase();
            let extension = match mime.as_str() {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/webp" => "webp",
                "image/gif" => "gif",
                "image/svg+xml" => "svg",
                "image/avif" => "avif",
                "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
                _ => {
                    if mime.starts_with("image/") {
                        self.warn(route, "An embedded image format is preserved as data. Verify that target browsers support it.");
                    }
                    return Ok(value.into());
                }
            };
            if extension == "svg" {
                self.warn(route, "Embedded SVG images are exported unchanged. Their external references, fonts and embedded behavior need manual verification.");
            }
            let bytes = if header.split(';').any(|s| s.eq_ignore_ascii_case("base64")) {
                STANDARD
                    .decode(decode_percent(payload)?)
                    .map_err(|_| Error("An embedded image has invalid base64 data.".into()))?
            } else {
                decode_percent(payload)?
            };
            ensure(!bytes.is_empty(), "An embedded image is empty.")?;
            let asset = format!("assets/{}.{}", digest(&bytes), extension);
            self.insert(asset.clone(), bytes)?;
            return Ok(format!("{}{fragment}", relative(file, &asset)));
        }
        if value.starts_with("//")
            || value
                .split(['/', '?', '#'])
                .next()
                .is_some_and(|s| s.contains(':'))
        {
            if value.starts_with("http:") || value.starts_with("https:") || value.starts_with("//")
            {
                self.warn(route, "External URLs are preserved. They require network access and were not downloaded or tested.");
            } else if !value.starts_with("mailto:") && !value.starts_with("tel:") {
                self.warn(
                    route,
                    "An authored URL scheme is preserved and requires manual verification.",
                );
            }
            return Ok(value.into());
        }
        let cut = value.find(['?', '#']).unwrap_or(value.len());
        let raw_path = String::from_utf8(decode_percent(&value[..cut])?)
            .map_err(|_| Error("A local URL is not valid UTF-8.".into()))?;
        if raw_path.is_empty() {
            return Ok(value.into());
        }
        let base = if raw_path.starts_with('/') {
            String::new()
        } else {
            route
                .rsplit_once('/')
                .map(|x| x.0)
                .unwrap_or("")
                .to_string()
        };
        let joined = format!("{base}/{raw_path}");
        let mut parts = Vec::new();
        for part in joined.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                _ => parts.push(part),
            }
        }
        let mut resolved = format!("/{}", parts.join("/"));
        if resolved.ends_with("/index.html") {
            resolved.truncate(resolved.len() - 11);
            if resolved.is_empty() {
                resolved.push('/');
            }
        }
        if navigation && let Some(target) = self.routes.get(&resolved) {
            return Ok(format!("{}{}", relative(file, target), &value[cut..]));
        }
        if navigation
            && self
                .project
                .pages
                .iter()
                .any(|p| p.path == resolved && !p.included)
        {
            self.warn(route, "A link points to a page excluded from this export.");
        } else {
            self.warn(route, "A local URL does not resolve to an exported resource. Its authored value is preserved; supply or configure that resource before sharing.");
        }
        Ok(value.into())
    }
    fn srcset(&mut self, source: &str, file: &str, route: &str) -> Result<String> {
        // HTML's candidate tokenizer: data URLs contain commas; a candidate URL
        // ends at whitespace, and trailing commas delimit URL-only candidates.
        let mut remaining = source;
        let mut output = Vec::new();
        while !remaining
            .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',')
            .is_empty()
        {
            remaining = remaining.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
            let end = remaining
                .find(|c: char| c.is_ascii_whitespace())
                .unwrap_or(remaining.len());
            let token = &remaining[..end];
            remaining = &remaining[end..];
            let comma = token.ends_with(',');
            let url = token.trim_end_matches(',');
            let descriptor = if comma {
                ""
            } else {
                let end = remaining.find(',').unwrap_or(remaining.len());
                let d = remaining[..end].trim();
                remaining = &remaining[end..];
                d
            };
            let rewritten = self.url(url, file, route, false)?;
            output.push(if descriptor.is_empty() {
                rewritten
            } else {
                format!("{rewritten} {descriptor}")
            });
        }
        Ok(output.join(", "))
    }
    fn css(&mut self, source: &str, file: &str, route: &str) -> Result<String> {
        let mut input = ParserInput::new(source);
        let mut parser = Parser::new(&mut input);
        let mut edits = Vec::new();
        self.css_tokens(&mut parser, file, route, &mut edits, false)?;
        let mut output = source.to_string();
        for (start, end, value) in edits.into_iter().rev() {
            output.replace_range(start..end, &value);
        }
        Ok(output)
    }
    fn css_tokens(
        &mut self,
        parser: &mut Parser<'_, '_>,
        file: &str,
        route: &str,
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
            match token {
                Token::UnquotedUrl(url) => {
                    let url = self.url(&url, file, route, false)?;
                    edits.push((
                        start,
                        parser.position().byte_index(),
                        format!("url({})", css_quote(&url)),
                    ));
                    import = false;
                }
                Token::QuotedString(url) if import || images => {
                    let url = self.url(&url, file, route, false)?;
                    edits.push((start, parser.position().byte_index(), css_quote(&url)));
                    import = false;
                }
                Token::AtKeyword(name) if name.eq_ignore_ascii_case("import") => import = true,
                Token::Function(name) => {
                    if name.eq_ignore_ascii_case("url") {
                        let value = parser
                            .parse_nested_block(|inner| {
                                inner
                                    .expect_string_cloned()
                                    .map_err(Into::<cssparser::ParseError<'_, ()>>::into)
                            })
                            .map_err(|_| Error("A CSS URL is malformed.".into()))?;
                        let value = self.url(&value, file, route, false)?;
                        edits.push((
                            start,
                            parser.position().byte_index(),
                            format!("url({})", css_quote(&value)),
                        ));
                    } else {
                        let nested_images = name.eq_ignore_ascii_case("image-set")
                            || name.eq_ignore_ascii_case("-webkit-image-set");
                        let result: std::result::Result<(), cssparser::ParseError<'_, ()>> = parser
                            .parse_nested_block(|inner| {
                                self.css_tokens(inner, file, route, edits, nested_images)
                                    .map_err(|_| inner.new_custom_error(()))
                            });
                        result.map_err(|_| {
                            Error("A nested CSS resource could not be exported.".into())
                        })?;
                    }
                    import = false;
                }
                Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                    let result: std::result::Result<(), cssparser::ParseError<'_, ()>> = parser
                        .parse_nested_block(|inner| {
                            self.css_tokens(inner, file, route, edits, false)
                                .map_err(|_| inner.new_custom_error(()))
                        });
                    result.map_err(|_| {
                        Error("A nested CSS resource could not be exported.".into())
                    })?;
                }
                Token::BadUrl(_) => return Err(Error("A CSS URL is malformed.".into())),
                Token::WhiteSpace(_) | Token::Comment(_) => {}
                _ => import = false,
            }
        }
        Ok(())
    }
}
fn css_quote(value: &str) -> String {
    let mut output = String::new();
    cssparser::serialize_string(value, &mut output).expect("Writing to a String cannot fail");
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Appearance, Page, SiteContent, Viewport};
    use uuid::Uuid;
    fn project() -> Project {
        let mut project = Project::new(
            "Export test".into(),
            Viewport {
                width: 1440,
                height: 900,
            },
            Appearance::Light,
        )
        .unwrap();
        project.pages[0].content = Some(content("<h1>Home</h1>", "body { color: red }", ""));
        project.brief.purpose = "PRIVATE_BRIEF".into();
        project
    }
    fn content(html: &str, css: &str, javascript: &str) -> SiteContent {
        SiteContent {
            html: html.into(),
            css: css.into(),
            javascript: javascript.into(),
        }
    }
    fn page(project: &mut Project, path: &str, source: SiteContent, included: bool) {
        project.pages.push(Page {
            id: Uuid::new_v4(),
            name: path.into(),
            path: path.into(),
            included,
            viewport: project.viewport,
            appearance: project.appearance,
            regions: vec![],
            content: Some(source),
        });
    }
    fn text(export: &WebsiteExport, path: &str) -> String {
        String::from_utf8(export.files[path].clone()).unwrap()
    }
    #[test]
    fn website_export_preserves_real_documents_and_private_material_is_unreachable() {
        let mut project = project();
        project.pages[0].content = Some(content(
            "<!doctype html><html lang='id'><head><title>Actual title</title><meta name='description' content='Authored description'><style>h1 {font-size:28px}</style></head><body class='site'><h1>Real content</h1><script>window.inline = 3</script></body></html>",
            "body {color:red}",
            "document.querySelector('h1').addEventListener('click', () => document.title='clicked');",
        ));
        let result = compile_website(&project).unwrap();
        let html = text(&result, "index.html");
        for expected in [
            "lang=\"id\"",
            "Actual title",
            "Authored description",
            "font-size:28px",
            "class=\"site\"",
            "Real content",
            "window.inline = 3",
            "defer=\"\" src=\"assets/page-",
        ] {
            assert!(html.contains(expected), "{expected}: {html}");
        }
        assert!(text(&result, "styles.css").contains("color:red"));
        assert!(
            result.files.iter().any(|(p, b)| p.ends_with(".js")
                && String::from_utf8_lossy(b).contains("addEventListener"))
        );
        assert!(
            !result
                .files
                .values()
                .any(|b| String::from_utf8_lossy(b).contains("PRIVATE_BRIEF"))
        );
        assert!(!result.files.contains_key("PROJECT.md"));
    }
    #[test]
    fn website_export_rewrites_nested_routes_with_query_fragments_and_unicode() {
        let mut project = project();
        project.pages[0].content = Some(content(
            "<a href='/team/people?sort=name#lead'>Team</a><a href='hello%20world'>Hello</a>",
            "",
            "",
        ));
        page(
            &mut project,
            "/team/people",
            content(
                "<a href='/'>Home</a><a href='../hello%20world#title'>Hello</a><a href='#local'>Local</a>",
                "",
                "",
            ),
            true,
        );
        page(
            &mut project,
            "/hello world",
            content("<p>Hello</p>", "", ""),
            true,
        );
        let result = compile_website(&project).unwrap();
        assert!(text(&result, "index.html").contains("team/people/index.html?sort=name#lead"));
        assert!(text(&result, "index.html").contains("hello%20world/index.html"));
        let nested = text(&result, "team/people/index.html");
        assert!(nested.contains("../../index.html"));
        assert!(nested.contains("../../hello%20world/index.html#title"));
        assert!(nested.contains("href=\"#local\""));
    }
    #[test]
    fn website_export_omits_excluded_pages_and_reports_missing_dependencies() {
        let mut project = project();
        project.pages[0].content = Some(content(
            "<a href='/draft'>Draft</a><img src='/missing.png'><img src='https://example.com/photo.png'><form action='https://example.com/post'></form>",
            "@import 'https://example.com/font.css';",
            "fetch('/api')",
        ));
        page(
            &mut project,
            "/draft",
            content("<h1>SECRET_DRAFT</h1>", "", ""),
            false,
        );
        let result = compile_website(&project).unwrap();
        assert_eq!(result.page_count, 1);
        assert!(!result.files.contains_key("draft/index.html"));
        for expected in [
            "excluded",
            "local URL",
            "External URLs",
            "Forms",
            "Custom JavaScript",
        ] {
            assert!(
                result.warnings.iter().any(|w| w.contains(expected)),
                "{expected}"
            );
        }
        assert!(
            !result
                .files
                .values()
                .any(|b| String::from_utf8_lossy(b).contains("SECRET_DRAFT"))
        );
    }
    #[test]
    fn website_export_extracts_actual_images_from_html_css_srcset_and_image_set() {
        let mut project = project();
        let data = "data:image/png;base64,aGVsbG8=";
        project.pages[0].content = Some(content(
            &format!(
                "<img src='{data}' srcset='{data} 1x, data:image/png;base64,d29ybGQ= 2x'><div style=\"background:url('{data}')\"></div>"
            ),
            &format!(
                "/* url(ignored.png) */ .a {{background-image:image-set(url('{data}') 1x, \"data:image/png;base64,d29ybGQ=\" 2x)}}"
            ),
            "",
        ));
        let result = compile_website(&project).unwrap();
        let hello = format!("assets/{}.png", digest(b"hello"));
        let world = format!("assets/{}.png", digest(b"world"));
        assert_eq!(result.files[&hello], b"hello");
        assert_eq!(result.files[&world], b"world");
        assert_eq!(
            result.files.keys().filter(|p| p.ends_with(".png")).count(),
            2
        );
        let html = text(&result, "index.html");
        assert!(html.contains(&hello));
        assert!(html.contains(&format!("{hello} 1x, {world} 2x")));
        let css = text(&result, "styles.css");
        assert!(css.contains(&hello));
        assert!(css.contains(&world));
        assert!(css.contains("/* url(ignored.png) */"));
    }
    #[test]
    fn website_export_preserves_css_cascade_isolation_and_reuses_identical_sources() {
        let mut project = project();
        page(
            &mut project,
            "/same",
            content("<p>Same</p>", "body { color: red }", ""),
            true,
        );
        page(
            &mut project,
            "/blue",
            content("<p>Blue</p>", "body { color: blue }", ""),
            true,
        );
        let result = compile_website(&project).unwrap();
        assert!(text(&result, "same/index.html").contains("../styles.css"));
        let doc = Document::from(text(&result, "blue/index.html"));
        let href = doc
            .select_single("link[rel=stylesheet]")
            .attr("href")
            .unwrap()
            .to_string();
        assert!(href.starts_with("../assets/styles-"));
        assert!(!text(&result, "styles.css").contains("blue"));
        assert!(text(&result, href.trim_start_matches("../")).contains("blue"));
    }
    #[test]
    fn website_export_nested_styles_resolve_assets_from_their_actual_location() {
        let mut project = project();
        page(
            &mut project,
            "/deep/nested",
            content(
                "<p>Nested</p>",
                "p {background:url(data:image/svg+xml,%3Csvg%20xmlns=%22http://www.w3.org/2000/svg%22%3E%3C/svg%3E)}",
                "",
            ),
            true,
        );
        let result = compile_website(&project).unwrap();
        let css = result
            .files
            .iter()
            .find(|(p, _)| p.starts_with("assets/styles-"))
            .unwrap();
        let css = String::from_utf8_lossy(css.1);
        assert!(!css.contains("url(\"assets/"));
        assert!(css.contains(".svg\")"));
    }
    #[test]
    fn website_export_honors_authored_empty_pages_without_fabricating_content() {
        let mut project = project();
        let mut blank = project.pages[0].clone();
        blank.id = Uuid::new_v4();
        blank.path = "/blank".into();
        blank.content = None;
        project.pages.push(blank);
        let result = compile_website(&project).unwrap();
        let html = text(&result, "blank/index.html");
        assert!(html.contains("<body></body>"));
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("/blank: This included page is blank"))
        );
        project.pages[0].content = None;
        assert!(
            compile_website(&project)
                .unwrap_err()
                .0
                .contains("no website content")
        );
    }
    #[test]
    fn website_export_rejects_collisions_base_urls_and_invalid_image_data() {
        for route in ["/index.html", "/styles.css", "/README.txt"] {
            let mut project = project();
            page(
                &mut project,
                route,
                content("<p>Collision</p>", "", ""),
                true,
            );
            assert!(
                compile_website(&project).unwrap_err().0.contains("collide"),
                "{route}"
            );
        }
        for html in [
            "<base href='https://example.com/'><p>Content</p>",
            "<img src='data:image/png;base64,!!!!'>",
            "<img src='data:image/png,%G0'>",
            "<img src='data:image/png,%€'>",
        ] {
            let mut project = project();
            project.pages[0].content = Some(content(html, "", ""));
            assert!(compile_website(&project).is_err());
        }
    }
    #[test]
    fn website_export_keeps_svg_fragments_and_emits_consistent_utf8_metadata() {
        let mut project = project();
        project.pages[0].content = Some(content(
            "<meta charset='iso-8859-1'><meta http-equiv='Content-Type' content='text/html; charset=latin1'><p>Héllo</p><img src='data:image/svg+xml,%3Csvg%3E%3C/svg%3E#icon'>",
            "",
            "",
        ));
        let result = compile_website(&project).unwrap();
        let html = text(&result, "index.html");
        assert!(html.contains("charset=\"utf-8\""));
        assert!(html.contains("content=\"text/html; charset=utf-8\""));
        assert!(html.contains("Héllo"));
        let path = format!("assets/{}.svg", digest(b"<svg></svg>"));
        assert_eq!(result.files[&path], b"<svg></svg>");
        assert!(html.contains(&format!("{path}#icon")));
        assert!(result.warnings.iter().any(|w| w.contains("SVG images")));
    }
    #[test]
    fn website_export_is_deterministic_without_evaluating_authored_scripts() {
        let mut project = project();
        project.pages[0].content = Some(content(
            "<script>throw new Error('must never execute')</script><p>Live</p>",
            "p {color:red}",
            "process.exit(1)",
        ));
        assert_eq!(
            compile_website(&project).unwrap(),
            compile_website(&project).unwrap()
        );
    }
    #[test]
    fn website_export_parses_escaped_css_urls_without_rewriting_strings_or_comments() {
        let mut project = project();
        project.pages[0].content = Some(content(
            "<p>Hi</p>",
            r#"p { background: u\72l(\64 ata:image/png;base64,aGk=); content: 'url(no.png)' } /*url(no.png)*/"#,
            "",
        ));
        let result = compile_website(&project).unwrap();
        let css = text(&result, "styles.css");
        assert!(css.contains(&format!("assets/{}.png", digest(b"hi"))));
        assert!(css.contains("'url(no.png)'"));
        assert!(css.contains("/*url(no.png)*/"));
    }
}
