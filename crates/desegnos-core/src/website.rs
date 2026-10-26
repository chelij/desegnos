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
