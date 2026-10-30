//! Links worth reading from a fetched home page for brand import: stylesheets and logo images.
use dom_query::Document;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebsiteLinks {
    pub stylesheets: Vec<String>,
    pub logos: Vec<String>,
    pub pages: Vec<String>,
    pub photos: Vec<String>,
}

const MAX_STYLESHEETS: usize = 4;
const MAX_LOGOS: usize = 6;

/// Logo-like images first (by name, alt text, class or id), then icons and the social preview image.
pub fn website_links(html: &str) -> WebsiteLinks {
    let doc = Document::from(html);
    let mut links = WebsiteLinks::default();
    let push = |list: &mut Vec<String>, value: Option<&str>, limit: usize| {
        if let Some(value) = value
            .map(str::trim)
            .filter(|value| !value.is_empty() && !value.starts_with("data:"))
            && list.len() < limit
            && !list.iter().any(|existing| existing == value)
        {
            list.push(value.to_owned());
        }
    };
    for node in doc.select("link[href]").nodes() {
        let rel = node
            .attr("rel")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if rel.split_whitespace().any(|part| part == "stylesheet") {
            push(
                &mut links.stylesheets,
                node.attr("href").as_deref(),
                MAX_STYLESHEETS,
            );
        }
    }
    for node in doc.select("img[src]").nodes() {
        let hint = ["src", "alt", "class", "id"]
            .iter()
            .filter_map(|name| node.attr(name).map(|value| value.to_ascii_lowercase()))
            .collect::<Vec<_>>()
            .join(" ");
        if !hint.contains("logo") && !hint.contains("brand") {
            push(&mut links.photos, node.attr("src").as_deref(), 32);
        }
        if hint.contains("logo") || hint.contains("brand") {
            push(&mut links.logos, node.attr("src").as_deref(), MAX_LOGOS);
        }
    }
    for node in doc.select("link[href]").nodes() {
        let rel = node
            .attr("rel")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if rel.contains("apple-touch-icon") || rel.split_whitespace().any(|part| part == "icon") {
            push(&mut links.logos, node.attr("href").as_deref(), MAX_LOGOS);
        }
    }
    for node in doc
        .select("meta[property='og:image'], meta[name='og:image']")
        .nodes()
    {
        push(&mut links.logos, node.attr("content").as_deref(), MAX_LOGOS);
    }
    for node in doc.select("a[href]").nodes() {
        push(&mut links.pages, node.attr("href").as_deref(), 128);
    }
    links
}

/// Every asset a rendered page needs, in document order: stylesheets, pictures, scripts,
/// media, frames and inline-style URLs. The website importer downloads these so the page can
/// be rendered from the local static server with scripts disabled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebsiteAssets {
    pub stylesheets: Vec<String>,
    pub images: Vec<String>,
    pub scripts: Vec<String>,
    pub media: Vec<String>,
    pub frames: Vec<String>,
    pub inline_styles: Vec<String>,
}

pub fn website_assets(html: &str) -> WebsiteAssets {
    let doc = Document::from(html);
    let mut assets = WebsiteAssets::default();
    let push = |list: &mut Vec<String>, value: Option<&str>| {
        if let Some(value) = value.map(str::trim).filter(|value| {
            !value.is_empty() && !value.starts_with("data:") && !value.starts_with('#')
        }) && !list.iter().any(|existing| existing == value)
        {
            list.push(value.to_owned());
        }
    };
    for node in doc.select("link[href]").nodes() {
        let rel = node
            .attr("rel")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        let as_value = node
            .attr("as")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if rel.split_whitespace().any(|part| part == "stylesheet") {
            push(&mut assets.stylesheets, node.attr("href").as_deref());
        } else if rel.split_whitespace().any(|part| part == "preload") {
            match as_value.as_str() {
                "style" => push(&mut assets.stylesheets, node.attr("href").as_deref()),
                "script" => push(&mut assets.scripts, node.attr("href").as_deref()),
                "image" => push(&mut assets.images, node.attr("href").as_deref()),
                "font" => push(&mut assets.media, node.attr("href").as_deref()),
                _ => {}
            }
        }
    }
    for node in doc.select("img[src], img[srcset]").nodes() {
        push(&mut assets.images, node.attr("src").as_deref());
        for candidate in node.attr("srcset").as_deref().unwrap_or("").split(',') {
            push(&mut assets.images, candidate.split_whitespace().next());
        }
    }
    for node in doc.select("script[src]").nodes() {
        push(&mut assets.scripts, node.attr("src").as_deref());
    }
    for node in doc
        .select("video[src], video[poster], audio[src], source[src], source[srcset]")
        .nodes()
    {
        let tag = node
            .node_name()
            .map(|name| name.to_ascii_lowercase())
            .unwrap_or_default();
        if node.attr("poster").is_some() {
            push(&mut assets.images, node.attr("poster").as_deref());
        }
        if let Some(value) = node.attr("src").as_deref() {
            if tag == "source"
                && node
                    .attr("type")
                    .is_some_and(|value| value.starts_with("image/"))
            {
                push(&mut assets.images, Some(value));
            } else {
                push(&mut assets.media, Some(value));
            }
        }
        for candidate in node.attr("srcset").as_deref().unwrap_or("").split(',') {
            push(&mut assets.images, candidate.split_whitespace().next());
        }
    }
    for node in doc.select("iframe[src], embed[src], object[data]").nodes() {
        push(
            &mut assets.frames,
            node.attr("src").or(node.attr("data")).as_deref(),
        );
    }
    for node in doc.select("style").nodes() {
        for url in css_urls(node.text().as_ref()) {
            push(&mut assets.inline_styles, Some(url.as_str()));
        }
    }
    assets
}

/// The raw `url(...)` targets in a stylesheet, including `@import` strings. Data URIs are kept
/// out; callers resolve them against the stylesheet URL and fetch only same-site ones.
pub fn css_urls(css: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = css;
    while let Some(index) = rest.find("url(") {
        rest = &rest[index + 4..];
        let Some(end) = rest.find(')') else { break };
        let value = rest[..end].trim().trim_matches(['\'', '"']).trim();
        if !value.is_empty()
            && !value.starts_with("data:")
            && !value.starts_with('#')
            && !urls.iter().any(|existing| existing == value)
        {
            urls.push(value.to_owned());
        }
        rest = &rest[end + 1..];
    }
    let mut rest = css;
    while let Some(index) = rest.find("@import") {
        rest = &rest[index + 7..];
        let rest_trimmed = rest.trim_start();
        let Some(quote) = rest_trimmed
            .chars()
            .next()
            .filter(|value| *value == '\'' || *value == '"')
        else {
            continue;
        };
        let quoted = &rest_trimmed[1..];
        let Some(end) = quoted.find(quote) else { break };
        let value = quoted[..end].trim();
        if !value.is_empty() && !urls.iter().any(|existing| existing == value) {
            urls.push(value.to_owned());
        }
        rest = &quoted[end + 1..];
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_stylesheets_logos_icons_and_social_image_in_order() {
        let links = website_links(
            r#"<html><head>
            <link rel="stylesheet" href="/css/site.css"><link rel="preload stylesheet" href="https://cdn.example.com/a.css">
            <link rel="icon" href="/favicon.png"><link rel="apple-touch-icon" href="/touch.png">
            <meta property="og:image" content="https://example.com/share.jpg"></head>
            <body><img src="/img/hero.jpg" alt="Our bakery"><img src="/img/acme-logo.png" alt="Acme"><img class="brand-mark" src="/mark.png"><img src="data:image/png;base64,AAA" alt="logo"></body></html>"#,
        );
        assert_eq!(
            links.stylesheets,
            ["/css/site.css", "https://cdn.example.com/a.css"]
        );
        assert_eq!(
            links.logos,
            [
                "/img/acme-logo.png",
                "/mark.png",
                "/favicon.png",
                "/touch.png",
                "https://example.com/share.jpg"
            ]
        );
    }
}
