//! Found-content metadata is private to the editor; visible credits stay public.
use crate::{ElementTarget, Project, Result, SiteContent, editing, ensure};
use serde::Serialize;
use std::net::IpAddr;
use url::{Host, Url};
use uuid::Uuid;

pub const FOUND_CONTENT_ATTRIBUTE: &str = "data-desegnos-found";

pub fn validate_found_source(value: &str) -> Result<()> {
    ensure(
        value.len() <= 4096 && !value.chars().any(char::is_control),
        "The found-content source address is invalid.",
    )?;
    let url = Url::parse(value)
        .map_err(|_| crate::Error("Use a complete public HTTPS source address.".into()))?;
    ensure(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443),
        "Use a public HTTPS source address without credentials or a private port.",
    )?;
    let public = match url.host() {
        Some(Host::Domain(name)) => {
            name.contains('.')
                && !name.ends_with('.')
                && !name.ends_with(".localhost")
                && !name.ends_with(".local")
                && !name.ends_with(".internal")
                && name != "localhost"
        }
        Some(Host::Ipv4(ip)) => public_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => public_ip(IpAddr::V6(ip)),
        None => false,
    };
    ensure(
        public,
        "Found content must link to a public website, not a local address.",
    )
}
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !matches!(a, 0 | 10 | 127 | 224..=255)
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 169 && b == 254)
                && !(a == 172 && (16..=31).contains(&b))
                && !(a == 192 && (b == 168 || b == 0 || (b == 2 && c == 0)))
                && !(a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                && !(a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundContentSource {
    pub selector: String,
    pub url: String,
}
impl SiteContent {
    pub fn found_sources(&self) -> Result<Vec<FoundContentSource>> {
        let doc = editing::parse(&self.html)?;
        let body = editing::body(&doc)?;
        let mut sources = vec![];
        for node in doc.select(&format!("[{FOUND_CONTENT_ATTRIBUTE}]")).nodes() {
            let url = node
                .attr(FOUND_CONTENT_ATTRIBUTE)
                .unwrap_or_default()
                .to_string();
            validate_found_source(&url)?;
            if let Some(selector) = editing::selector(*node, body) {
                sources.push(FoundContentSource { selector, url });
            }
            ensure(
                sources.len() <= 2048,
                "This page has too many found-content source markers.",
            )?;
        }
        Ok(sources)
    }
    /// Count marked replacement slots, excluding nested duplicates. A second
    /// metric counts unmarked labels in generated or imported HTML.
    pub fn placeholder_metrics(&self) -> Result<PlaceholderMetrics> {
        let doc = editing::parse(&self.html)?;
        let mut marked = 0;
        let mut unmarked = 0;
        for node in doc.select("*").nodes() {
            let ancestor_marked = node
                .ancestors_it(None)
                .any(|parent| parent.has_attr("data-desegnos-placeholder"));
            if node.has_attr("data-desegnos-placeholder") && !ancestor_marked {
                marked += 1;
            }
            if !node.has_attr("data-desegnos-placeholder")
                && !ancestor_marked
                && node.children().iter().any(|child| {
                    child.is_text() && child.text().to_ascii_lowercase().contains("placeholder")
                })
            {
                unmarked += 1;
            }
        }
        Ok(PlaceholderMetrics {
            marked_slots: marked,
            unmarked_labels: unmarked,
            total: marked + unmarked,
        })
    }
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlaceholderMetrics {
    pub marked_slots: usize,
    pub unmarked_labels: usize,
    pub total: usize,
}
impl Project {
    pub fn found_source(&self, page_id: Uuid, target: &ElementTarget) -> Result<Option<String>> {
        let content = self
            .page(page_id)?
            .content
            .as_ref()
            .ok_or_else(|| crate::Error("This page has no content yet.".into()))?;
        ensure(
            !target.id.is_empty() || content.sha256()? == target.source_sha256,
            "The selected item changed. Select it again before opening its source.",
        )?;
        let doc = editing::parse(&content.html)?;
        let node = editing::locate(&doc, target)?;
        let source = node
            .attr(FOUND_CONTENT_ATTRIBUTE)
            .map(|value| value.to_string());
        if let Some(source) = &source {
            validate_found_source(source)?;
        }
        Ok(source)
    }
}
