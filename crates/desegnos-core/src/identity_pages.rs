//! Source routes are evidence; they never grant file or network access.
use crate::{identity::SourcePage, identity::SourcePageRef};
use dom_query::Document;
use url::Url;

pub fn source_page_kind(path: &str, title: &str) -> String {
    if path == "/" {
        return "Home".into();
    }
    for words in [path.to_lowercase(), title.to_lowercase()] {
        for (kind, hints) in [
            ("About", &["about", "team"][..]),
            ("Projects", &["project", "portfolio", "work"]),
            ("Tutorials", &["tutorial", "guide", "learn"]),
            ("Blog", &["blog", "article", "post"]),
            ("Contact", &["contact"]),
        ] {
            if hints.iter().any(|hint| words.contains(hint)) {
                return kind.into();
            }
        }
    }
    "Other".into()
}

/// Convert file-system page names or public URL paths to portable website routes.
pub fn source_page_path(value: &str) -> String {
    let parsed = Url::parse(value).ok();
    let value = parsed.as_ref().map_or(value, |url| url.path());
    let value = value
        .split(['?', '#'])
        .next()
        .unwrap_or(value)
        .trim_matches('/');
    let value = [
        "src/pages/",
        "src/content/",
        "pages/",
        "public/",
        "static/",
        "dist/",
        "build/",
        "out/",
        "_site/",
    ]
    .iter()
    .find_map(|prefix| value.strip_prefix(prefix))
    .unwrap_or(value);
    let value = value
        .rsplit_once('.')
        .filter(|(_, ext)| ["html", "htm", "astro", "md", "mdx"].contains(ext))
        .map_or(value, |(stem, _)| stem);
    let mut parts: Vec<String> = value
        .split('/')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let clean: String = part
                .chars()
                .map(|ch| {
                    if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                        ch
                    } else {
                        '-'
                    }
                })
                .collect();
            if ["assets", "src", "node_modules", "con", "prn", "aux", "nul"]
                .contains(&clean.to_lowercase().as_str())
                || clean == "."
                || clean == ".."
            {
                format!("page-{}", clean.replace('.', "-"))
            } else {
                clean.trim_end_matches('.').to_owned()
            }
        })
        .filter(|part| !part.is_empty())
        .collect();
    if parts
        .last()
        .is_some_and(|part| part.eq_ignore_ascii_case("index"))
    {
        parts.pop();
    }
    format!("/{}", parts.join("/")).chars().take(1000).collect()
}

/// A file alone is page evidence only for HTML or a conventional routing directory.
pub fn identity_source_page(file: &str, text: &str) -> Option<SourcePage> {
    let ext = file.rsplit('.').next()?;
    if !["html", "htm", "astro", "md", "mdx"].contains(&ext) {
        return None;
    }
    let routed = file.starts_with("src/pages/") || file.starts_with("pages/");
    if !routed
        && (!["html", "htm"].contains(&ext)
            || file.split('/').any(|part| {
                [
                    "docs",
                    "notes",
                    "anti-slop",
                    "audits",
                    "audit",
                    "components",
                    "layouts",
                ]
                .contains(&part)
            }))
    {
        return None;
    }
    source_page_at_route(file, text, &source_page_path(file))
}

/// Read scalar frontmatter only inside its delimiters, never from body text or code.
pub fn page_frontmatter<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = text.trim_start().lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name == key
        {
            return Some(value.trim().trim_matches(['\'', '"']));
        }
    }
    None
}

pub fn source_page_at_route(file: &str, text: &str, route: &str) -> Option<SourcePage> {
    let lower = file.to_lowercase();
    if lower
        .split('/')
        .any(|part| part.starts_with('.') || part == "node_modules")
        || page_frontmatter(text, "draft").is_some_and(|v| {
            v.split('#')
                .next()
                .unwrap_or(v)
                .trim()
                .eq_ignore_ascii_case("true")
        })
    {
        return None;
    }
    let stem = lower.rsplit('/').next()?.split('.').next()?;
    if [
        "readme",
        "claude",
        "agents",
        "changelog",
        "license",
        "contributing",
        "design",
        "notes",
        "404",
        "500",
        "error",
        "_error",
        "+error",
        "not-found",
    ]
    .contains(&stem)
    {
        return None;
    }
    let url = text
        .strip_prefix("<!-- Source URL: ")
        .and_then(|rest| rest.split_once(" -->"))
        .and_then(|(url, _)| Url::parse(url).ok());
    let path = source_page_path(url.as_ref().map_or(route, |url| url.as_str()));
    if path
        .split('/')
        .any(|part| ["404", "500", "error", "_error", "+error", "not-found"].contains(&part))
    {
        return None;
    }
    // Insert separation before parsing so entities are decoded and inline/br/block
    // boundaries cannot concatenate words. Inspect only the first title or h1.
    let body = text
        .trim_start()
        .strip_prefix("---")
        .and_then(|rest| rest.split_once("\n---").map(|(_, body)| body))
        .unwrap_or(text);
    let spaced = body.replace('<', " <").replace('>', "> ");
    let doc = Document::from(spaced.as_str());
    let title = page_frontmatter(text, "title")
        .map(str::to_owned)
        .or_else(|| {
            ["title", "h1"].iter().find_map(|selector| {
                let value = doc.select(selector).first().text().to_string();
                (!value.trim().is_empty() && !value.contains(['{', '}'])).then_some(value)
            })
        })
        .or_else(|| {
            text.lines()
                .find_map(|line| line.strip_prefix("# ").map(str::to_owned))
        })
        .unwrap_or_else(|| readable_route_title(&path));
    let mut title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    for punctuation in ['.', ',', '!', '?', ';', ':'] {
        title = title.replace(&format!(" {punctuation}"), &punctuation.to_string());
    }
    let title = [" | ", " — ", " – "]
        .iter()
        .filter_map(|sep| title.split_once(sep).map(|(title, _)| title))
        .min_by_key(|title| title.len())
        .unwrap_or(&title)
        .chars()
        .take(240)
        .collect::<String>();
    Some(SourcePage {
        kind: source_page_kind(&path, &title),
        path,
        title,
        url: url.map(Into::into),
        photo_count: 0,
        item_count: 0,
    })
}

fn readable_route_title(path: &str) -> String {
    if path == "/" {
        return "Home".into();
    }
    path.rsplit('/')
        .next()
        .unwrap_or("Page")
        .split(['-', '_'])
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// HTML, Markdown and Astro imports all contain literal resource references. Resolve
/// these relative to their page, including ../, root paths and public/ assets.
pub fn page_references_image(file: &str, text: &str, image: &str) -> bool {
    let crawl = text
        .strip_prefix("<!-- Source URL: ")
        .and_then(|rest| rest.split_once(" -->"))
        .and_then(|(url, _)| Url::parse(url).ok());
    let base = crawl.unwrap_or_else(|| {
        Url::parse(&format!("https://source.invalid/{file}")).expect("local base")
    });
    let target = image
        .strip_prefix("public/")
        .or_else(|| image.strip_prefix("static/"))
        .unwrap_or(image);
    let doc = Document::from(text);
    let mut references: Vec<String> = doc
        .select("img[src]")
        .nodes()
        .iter()
        .filter_map(|node| node.attr("src").map(|value| value.to_string()))
        .collect();
    references.extend(
        text.split([
            '\'', '"', '(', ')', '<', '>', ' ', '\n', '\r', '\t', '`', ',',
        ])
        .map(str::to_owned),
    );
    references.iter().any(|token| {
        base.join(token).is_ok_and(|url| {
            url.host_str() == base.host_str()
                && [image, target].contains(&url.path().trim_start_matches('/'))
        })
    })
}

pub fn validate_source_page_ref(page: &SourcePageRef) -> crate::Result<()> {
    crate::validation::validate_text(&page.title, "Source page title", 240, true)?;
    crate::validation::ensure(
        page.path == source_page_path(&page.path),
        "A source page has an invalid path.",
    )?;
    crate::validation::ensure(
        page.url
            .as_ref()
            .is_none_or(|url| crate::limits::valid_http_url(url)),
        "A source page has an invalid URL.",
    )
}
