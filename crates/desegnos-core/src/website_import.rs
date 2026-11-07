//! Turn rendered website pages into canvas elements.
//!
//! The browser capture is the only source of truth about layout and computed styles; this file
//! is a pure conversion from that compact tree in the browser to the project's element model.
//! It never fetches, writes or runs page code. Images arrive as picture ids resolved by the
//! native layer through the ordinary project picture store.
use crate::{
    BoundaryKind, Element, ElementKind, ElementLayout, ElementMeta, ElementOverride, ElementShadow,
    ElementStroke, ElementStyle, ElementTextStyle, InputType, OutputType, Page, PageSize, Pointer,
    PointerSource, PointerTarget, Project, Rect, ReferenceItem, ReferenceKind, Result, SizingMode,
    StackAlign, StackDirection, StackJustify, TextAlign, ensure,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use uuid::Uuid;

/// A page rendered at one width. `url` is the absolute local-static-server URL the browser
/// loaded, so image sources are already absolute and can be mapped back to snapshot files.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebCapture {
    pub url: String,
    /// The site name shown to the user; page paths are built from it instead of the loopback host.
    #[serde(default)]
    pub site: String,
    /// The original site route (`/about`), not the loopback server path that carries a token.
    #[serde(default)]
    pub route: String,
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub nodes: Vec<WebNode>,
    #[serde(default)]
    pub scripts: Vec<WebScript>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebScript {
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebNode {
    pub tag: String,
    pub rect: Rect,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub href: Option<String>,
    #[serde(default)]
    pub input_type: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub alt: Option<String>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub attributes: WebAttributes,
    pub styles: WebStyles,
    #[serde(default)]
    pub children: Vec<WebNode>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebAttributes {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub class_name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub aria_expanded: Option<String>,
    #[serde(default)]
    pub aria_controls: Option<String>,
    #[serde(default)]
    pub data_toggle: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

fn one() -> f64 {
    1.0
}
fn font_size() -> f64 {
    16.0
}
fn font_weight() -> f64 {
    400.0
}
fn line_height() -> f64 {
    1.4
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebStyles {
    #[serde(default)]
    pub background: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub border_color: String,
    #[serde(default)]
    pub border_width: f64,
    #[serde(default)]
    pub radius: f64,
    #[serde(default = "one")]
    pub opacity: f64,
    #[serde(default)]
    pub shadow_x: f64,
    #[serde(default)]
    pub shadow_y: f64,
    #[serde(default)]
    pub shadow_blur: f64,
    #[serde(default)]
    pub shadow_color: String,
    #[serde(default)]
    pub font_family: String,
    #[serde(default = "font_size")]
    pub font_size: f64,
    #[serde(default = "font_weight")]
    pub font_weight: f64,
    #[serde(default)]
    pub italic: bool,
    #[serde(default = "line_height")]
    pub line_height: f64,
    #[serde(default)]
    pub letter_spacing: f64,
    #[serde(default)]
    pub align: String,
    #[serde(default)]
    pub display: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub row_gap: f64,
    #[serde(default)]
    pub column_gap: f64,
    #[serde(default)]
    pub padding: f64,
    #[serde(default)]
    pub columns: u32,
    #[serde(default)]
    pub align_items: String,
    #[serde(default)]
    pub justify_content: String,
    #[serde(default)]
    pub wrap: bool,
}

impl Default for WebStyles {
    fn default() -> Self {
        Self {
            background: String::new(),
            color: String::new(),
            border_color: String::new(),
            border_width: 0.0,
            radius: 0.0,
            opacity: 1.0,
            shadow_x: 0.0,
            shadow_y: 0.0,
            shadow_blur: 0.0,
            shadow_color: String::new(),
            font_family: String::new(),
            font_size: 16.0,
            font_weight: 400.0,
            italic: false,
            line_height: 1.4,
            letter_spacing: 0.0,
            align: String::new(),
            display: String::new(),
            direction: String::new(),
            row_gap: 0.0,
            column_gap: 0.0,
            padding: 0.0,
            columns: 1,
            align_items: String::new(),
            justify_content: String::new(),
            wrap: false,
        }
    }
}

/// Everything one import adds to a project. The caller applies it as a single undo step.
#[derive(Debug, Clone, Default)]
pub struct ImportedWebsite {
    pub pages: Vec<Page>,
    pub reference: Vec<ReferenceItem>,
    pub pointers: Vec<Pointer>,
    pub warnings: Vec<String>,
}

/// The unique image, video-poster and background sources the native layer must resolve into
/// project pictures before conversion.
pub fn web_capture_image_sources(captures: &[WebCapture]) -> Vec<String> {
    let mut sources = BTreeSet::new();
    for capture in captures {
        collect_sources(&capture.nodes, &mut sources);
    }
    sources.into_iter().collect()
}

fn collect_sources(nodes: &[WebNode], sources: &mut BTreeSet<String>) {
    for node in nodes {
        let tag = node.tag.trim().to_ascii_lowercase();
        if tag == "img"
            && let Some(src) = node.src.as_deref().filter(|value| !value.trim().is_empty())
        {
            sources.insert(src.to_owned());
        }
        if tag == "video"
            && let Some(poster) = node
                .poster
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        {
            sources.insert(poster.to_owned());
        }
        collect_sources(&node.children, sources);
    }
}

/// A page plan: the desktop capture is the base, the phone capture becomes linked-size overrides.
struct PagePlan<'a> {
    url: String,
    desktop: &'a WebCapture,
    phone: Option<&'a WebCapture>,
    path: String,
    name: String,
    page_id: Uuid,
    size_id: Uuid,
    phone_size_id: Uuid,
}

/// A stable identifier from a string. Importing the same page twice therefore gives the same
/// layer ids, which lets the phone pass target the desktop pass's layers.
fn stable_uuid(seed: &str) -> Uuid {
    let hash = crate::digest(seed.as_bytes());
    let mut bytes = [0_u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16).unwrap_or(0);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn normalize_url(value: &str) -> String {
    let mut value = value.trim().to_owned();
    if let Some(index) = value.find(['#', '?']) {
        value.truncate(index);
    }
    // A page is the same whether a link names its file or its route: `about.html`, `about`
    // and `about/index.html` all lead to the captured `about`.
    for suffix in ["/index.html", "/index.htm"] {
        if value.to_ascii_lowercase().ends_with(suffix) {
            value.truncate(value.len() - suffix.len() + 1);
        }
    }
    let file = value.rfind('/').map_or(0, |slash| slash + 1);
    for extension in [".html", ".htm"] {
        if value.len() > file + extension.len() && value.to_ascii_lowercase().ends_with(extension) {
            value.truncate(value.len() - extension.len());
        }
    }
    while value.len() > 1 && value.ends_with('/') {
        value.pop();
    }
    value
}

fn internal_href(
    href: &str,
    page_url: &str,
    page_paths: &BTreeMap<String, String>,
    own_hosts: &BTreeSet<String>,
) -> Option<String> {
    let target = normalize_url(href);
    let page = normalize_url(page_url);
    if target.is_empty() || target == page {
        return None;
    }
    if let Some(path) = page_paths.get(&target) {
        return Some(path.clone());
    }
    // A page served by the loopback server of a folder or ZIP import keeps its imported path
    // after the random first segment. Old sites link to their own pages with absolute URLs on
    // their original domain and with different letter case, so match those paths without case.
    let page_path = served_path(&page)?;
    let linked = url::Url::parse(page_url).ok()?.join(href).ok()?;
    // Another domain counts only when the site links to several of its own pages there, so
    // a link to https://social.example/ never lands on the imported home page.
    let linked_path = served_path(linked.as_str()).or_else(|| {
        (matches!(linked.scheme(), "http" | "https")
            && linked
                .host_str()
                .is_some_and(|host| own_hosts.contains(&host.to_ascii_lowercase())))
        .then(|| normalize_url(linked.path()))
    })?;
    if linked_path.eq_ignore_ascii_case(&page_path) {
        return None;
    }
    page_paths
        .iter()
        .find(|(url, _)| {
            served_path(url).is_some_and(|path| path.eq_ignore_ascii_case(&linked_path))
        })
        .map(|(_, path)| path.clone())
}

/// Hosts the site uses for its own pages: absolute links there reach an imported page by its
/// .htm or .html file name, or at least two different imported pages other than the home page.
fn own_hosts(captures: &[WebCapture], page_paths: &BTreeMap<String, String>) -> BTreeSet<String> {
    fn hrefs<'a>(nodes: &'a [WebNode], out: &mut Vec<&'a str>) {
        for node in nodes {
            if let Some(href) = node.href.as_deref() {
                out.push(href);
            }
            hrefs(&node.children, out);
        }
    }
    let pages: BTreeSet<String> = page_paths
        .keys()
        .filter_map(|url| served_path(url))
        .filter(|path| path != "/")
        .map(|path| path.to_ascii_lowercase())
        .collect();
    let mut matches: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut by_file = BTreeSet::new();
    for capture in captures {
        let mut links = Vec::new();
        hrefs(&capture.nodes, &mut links);
        for link in links {
            let Ok(url) = url::Url::parse(link) else {
                continue;
            };
            let (Some(host), true) = (url.host_str(), matches!(url.scheme(), "http" | "https"))
            else {
                continue;
            };
            let path = normalize_url(url.path()).to_ascii_lowercase();
            if served_path(url.as_str()).is_none() && pages.contains(&path) {
                let file = url.path().to_ascii_lowercase();
                if file.ends_with(".htm") || file.ends_with(".html") {
                    by_file.insert(host.to_ascii_lowercase());
                }
                matches
                    .entry(host.to_ascii_lowercase())
                    .or_default()
                    .insert(path);
            }
        }
    }
    matches
        .into_iter()
        .filter(|(host, paths)| paths.len() >= 2 || by_file.contains(host))
        .map(|(host, _)| host)
        .collect()
}

/// The path a loopback URL serves, without the first segment that is a serving token.
fn served_path(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if !(host.eq_ignore_ascii_case("127.0.0.1") || host.eq_ignore_ascii_case("localhost")) {
        return None;
    }
    let path = parsed.path().strip_prefix('/')?;
    let suffix = path.find('/').map_or("/", |slash| &path[slash..]);
    Some(normalize_url(suffix))
}

fn remote_href(href: &str) -> Option<String> {
    let href = href.trim();
    if href.len() > 2048 {
        return None;
    }
    let parsed = url::Url::parse(href).ok()?;
    (matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none())
    .then(|| href.to_owned())
}

fn parse_colour(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() || value == "transparent" {
        return None;
    }
    if let Some(hex) = value.strip_prefix('#') {
        let valid =
            matches!(hex.len(), 3 | 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
        return valid.then(|| format!("#{hex}"));
    }
    let start = value.find('(')?;
    if !value.starts_with("rgb") || !value.ends_with(')') {
        return None;
    }
    let parts: Vec<f64> = value[start + 1..value.len() - 1]
        .split(|character: char| character == ',' || character.is_whitespace() || character == '/')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let part = part.trim();
            if let Some(percent) = part.strip_suffix('%') {
                percent.parse::<f64>().ok().map(|value| value * 2.55)
            } else {
                part.parse::<f64>().ok()
            }
        })
        .collect();
    if parts.len() < 3 {
        return None;
    }
    let channel = |value: f64| value.clamp(0.0, 255.0).round() as u8;
    let alpha = parts.get(3).copied().unwrap_or(1.0);
    if alpha <= 0.001 {
        return None;
    }
    let (r, g, b) = (channel(parts[0]), channel(parts[1]), channel(parts[2]));
    if alpha >= 0.999 {
        Some(format!("#{r:02x}{g:02x}{b:02x}"))
    } else {
        Some(format!(
            "#{r:02x}{g:02x}{b:02x}{:02x}",
            (alpha * 255.0).round() as u8
        ))
    }
}

fn text_align(value: &str) -> TextAlign {
    match value.trim().to_ascii_lowercase().as_str() {
        "center" => TextAlign::Center,
        "right" | "end" => TextAlign::Right,
        _ => TextAlign::Left,
    }
}

fn font_family(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "system-ui".into()
    } else {
        value.chars().take(240).collect()
    }
}

fn text_style(styles: &WebStyles) -> ElementTextStyle {
    ElementTextStyle {
        font_family: font_family(&styles.font_family),
        size: styles.font_size.clamp(1.0, 400.0),
        weight: styles.font_weight.clamp(100.0, 900.0).round() as u16,
        italic: styles.italic,
        line_height: styles.line_height.clamp(0.5, 4.0),
        letter_spacing: styles.letter_spacing.clamp(-20.0, 100.0),
        color: parse_colour(&styles.color).unwrap_or_else(|| "#1a1a1a".into()),
        align: text_align(&styles.align),
    }
}

fn element_style(styles: &WebStyles, text: bool) -> ElementStyle {
    let radius = styles.radius.clamp(0.0, 4096.0);
    ElementStyle {
        flip_x: false,
        flip_y: false,
        sizing: Default::default(),
        padding: None,
        references: Default::default(),
        fill: if text {
            None
        } else {
            parse_colour(&styles.background)
        },
        stroke: if text {
            ElementStroke {
                color: None,
                width: 0.0,
            }
        } else {
            ElementStroke {
                color: parse_colour(&styles.border_color).filter(|_| styles.border_width >= 0.5),
                width: styles.border_width.clamp(0.0, 128.0),
            }
        },
        radius,
        opacity: styles.opacity.clamp(0.0, 1.0),
        shadow: ElementShadow {
            x: styles.shadow_x.clamp(-512.0, 512.0),
            y: styles.shadow_y.clamp(-512.0, 512.0),
            blur: styles.shadow_blur.clamp(0.0, 512.0),
            color: parse_colour(&styles.shadow_color),
        },
        text_style: text_style(styles),
    }
}

fn layout_for(styles: &WebStyles) -> ElementLayout {
    let display = styles.display.trim().to_ascii_lowercase();
    if display.contains("flex") {
        let direction = if styles.direction.trim().eq_ignore_ascii_case("row") {
            StackDirection::Row
        } else if styles.direction.trim().eq_ignore_ascii_case("column") {
            StackDirection::Column
        } else {
            StackDirection::Row
        };
        let gap = if direction == StackDirection::Row {
            styles.column_gap.max(styles.row_gap)
        } else {
            styles.row_gap.max(styles.column_gap)
        };
        let align = match styles.align_items.trim().to_ascii_lowercase().as_str() {
            "center" => StackAlign::Center,
            "end" | "flex-end" => StackAlign::End,
            "stretch" => StackAlign::Stretch,
            _ => StackAlign::Start,
        };
        let justify = match styles.justify_content.trim().to_ascii_lowercase().as_str() {
            "center" => StackJustify::Center,
            "end" | "flex-end" | "right" => StackJustify::End,
            "space-between" | "space-around" | "space-evenly" => StackJustify::SpaceBetween,
            _ => StackJustify::Start,
        };
        return ElementLayout::Stack {
            direction,
            gap: gap.clamp(0.0, 4096.0),
            padding: styles.padding.clamp(0.0, 4096.0),
            align,
            justify,
            wrap: styles.wrap,
        };
    }
    if display.contains("grid") {
        return ElementLayout::Grid {
            columns: styles.columns.clamp(1, 64),
            gap: styles.row_gap.max(styles.column_gap).clamp(0.0, 4096.0),
            padding: styles.padding.clamp(0.0, 4096.0),
        };
    }
    ElementLayout::Free
}

fn visual(styles: &WebStyles) -> bool {
    parse_colour(&styles.background).is_some()
        || (styles.border_width >= 0.5 && parse_colour(&styles.border_color).is_some())
        || styles.shadow_blur > 0.0 && parse_colour(&styles.shadow_color).is_some()
        || styles.radius > 0.0
        || styles.opacity < 0.999
}

fn rect_local(rect: Rect, origin: (f64, f64)) -> Rect {
    Rect {
        x: rect.x - origin.0,
        y: rect.y - origin.1,
        width: rect.width,
        height: rect.height,
    }
}

fn is_text_tag(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "span"
            | "strong"
            | "em"
            | "b"
            | "i"
            | "u"
            | "small"
            | "li"
            | "blockquote"
            | "figcaption"
            | "label"
            | "legend"
            | "th"
            | "td"
            | "dt"
            | "dd"
            | "cite"
            | "q"
            | "code"
            | "pre"
            | "summary"
            | "address"
    )
}

fn is_skipped(tag: &str) -> bool {
    matches!(
        tag,
        "script"
            | "style"
            | "link"
            | "meta"
            | "head"
            | "title"
            | "noscript"
            | "template"
            | "br"
            | "option"
            | "optgroup"
            | "col"
            | "colgroup"
            | "source"
            | "track"
            | "wbr"
    )
}

fn input_type(value: Option<&str>) -> InputType {
    match value.unwrap_or("text").trim().to_ascii_lowercase().as_str() {
        "email" => InputType::Email,
        "password" => InputType::Password,
        "number" => InputType::Number,
        "search" => InputType::Search,
        "tel" => InputType::Tel,
        "url" => InputType::Url,
        "date" => InputType::Date,
        "time" => InputType::Time,
        "color" => InputType::Color,
        "range" => InputType::Range,
        "file" => InputType::File,
        _ => InputType::Text,
    }
}

fn clip_text(value: &str) -> String {
    value
        .split('\0')
        .next()
        .unwrap_or_default()
        .chars()
        .take(crate::MAX_PROMPT_CHARS)
        .collect()
}

fn trimmed_text(value: Option<&str>) -> Option<String> {
    let value = value?;
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!value.is_empty()).then_some(value)
}

fn descendant_text(nodes: &[WebNode]) -> Option<String> {
    for node in nodes {
        if let Some(text) = trimmed_text(node.text.as_deref()) {
            return Some(text);
        }
        if let Some(text) = descendant_text(&node.children) {
            return Some(text);
        }
    }
    None
}

struct Converter<'a> {
    page: &'a PagePlan<'a>,
    picture_ids: &'a BTreeMap<String, Uuid>,
    page_paths: &'a BTreeMap<String, String>,
    own_hosts: &'a BTreeSet<String>,
    element_count: usize,
    warnings: Vec<String>,
}

impl Converter<'_> {
    fn element_id(&self, path: &[usize], tag: &str) -> Uuid {
        stable_uuid(&format!(
            "website:{}:{}:{}",
            normalize_url(&self.page.url),
            tag,
            path.iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(".")
        ))
    }

    fn map_link(&self, href: Option<&str>) -> Option<String> {
        let href = href?.trim();
        if href.is_empty() {
            return None;
        }
        if let Some(path) = internal_href(href, &self.page.url, self.page_paths, self.own_hosts) {
            return Some(path);
        }
        // A local server address that is not an imported page would export as localhost.
        if let (Ok(page), Ok(target)) = (url::Url::parse(&self.page.url), url::Url::parse(href))
            && page.origin() == target.origin()
        {
            return None;
        }
        remote_href(href)
    }

    fn picture(&self, source: Option<&str>) -> Option<Uuid> {
        self.picture_ids.get(source?.trim()).copied()
    }

    fn node_style(&self, node: &WebNode, text: bool) -> ElementStyle {
        let mut style = element_style(&node.styles, text);
        let limit = node.rect.width.min(node.rect.height) / 2.0;
        if limit.is_finite() {
            style.radius = style.radius.min(limit.max(0.0));
        }
        style
    }

    /// One DOM node becomes zero or more elements positioned relative to `origin`.
    fn elements(
        &self,
        node: &WebNode,
        origin: (f64, f64),
        path: &mut Vec<usize>,
        depth: usize,
    ) -> Vec<Element> {
        if depth > 28 || self.element_count >= crate::MAX_ELEMENTS {
            return vec![];
        }
        let tag = node.tag.trim().to_ascii_lowercase();
        if is_skipped(&tag) || node.styles.opacity <= 0.001 {
            return vec![];
        }
        if !node.rect.width.is_finite()
            || !node.rect.height.is_finite()
            || node.rect.width < 1.0
            || node.rect.height < 1.0
        {
            return vec![];
        }
        let local = rect_local(node.rect, origin);
        let text = trimmed_text(node.text.as_deref());
        let mut children = Vec::new();
        for (index, child) in node.children.iter().enumerate() {
            path.push(index);
            children.extend(self.elements(child, (node.rect.x, node.rect.y), path, depth + 1));
            path.pop();
        }

        if tag == "img" {
            let Some(image) = self.picture(node.src.as_deref()) else {
                return vec![];
            };
            return [Element {
                id: self.element_id(path, &tag),
                kind: ElementKind::Image,
                name: None,
                link: None,
                rect: local,
                rotation: 0.0,
                visible: true,
                locked: false,
                style: self.node_style(node, false),
                text: clip_text(node.alt.as_deref().unwrap_or_default()),
                image: Some(image),
                url: None,
                layout: ElementLayout::Free,
                overrides: BTreeMap::new(),
                children: vec![],
                meta: ElementMeta::default(),
            }]
            .into();
        }
        if tag == "video" {
            if let Some(url) = self.map_link(node.src.as_deref()) {
                return [self.base_element(
                    &tag,
                    path,
                    local,
                    node,
                    ElementKind::Video,
                    &text,
                    Some(url),
                    children,
                )]
                .into();
            }
            let Some(image) = self.picture(node.poster.as_deref()) else {
                return vec![];
            };
            let mut element = self.base_element(
                &tag,
                path,
                local,
                node,
                ElementKind::Image,
                &text,
                None,
                vec![],
            );
            element.image = Some(image);
            return [element].into();
        }
        if tag == "audio" {
            let Some(url) = self.map_link(node.src.as_deref()) else {
                return vec![];
            };
            return [self.base_element(
                &tag,
                path,
                local,
                node,
                ElementKind::Audio,
                &text,
                Some(url),
                children,
            )]
            .into();
        }
        if tag == "iframe" || tag == "embed" || tag == "object" {
            let Some(url) = self.map_link(node.src.as_deref()) else {
                return vec![];
            };
            return [self.base_element(
                &tag,
                path,
                local,
                node,
                ElementKind::Embed,
                &text,
                Some(url),
                children,
            )]
            .into();
        }
        if tag == "input" {
            let kind = node
                .input_type
                .as_deref()
                .unwrap_or("text")
                .trim()
                .to_ascii_lowercase();
            if kind == "hidden" {
                return vec![];
            }
            let value = if matches!(kind.as_str(), "checkbox" | "radio") {
                trimmed_text(node.alt.as_deref())
                    .or_else(|| trimmed_text(node.attributes.name.as_deref()))
            } else {
                trimmed_text(node.value.as_deref())
                    .or_else(|| trimmed_text(node.placeholder.as_deref()))
            };
            let element_kind = match kind.as_str() {
                "checkbox" => ElementKind::Checkbox,
                "radio" => ElementKind::Radio,
                "submit" | "button" | "reset" | "image" => ElementKind::Button,
                _ => ElementKind::Input {
                    input_type: input_type(Some(&kind)),
                },
            };
            return [self.base_element(
                &tag,
                path,
                local,
                node,
                element_kind,
                &value,
                None,
                vec![],
            )]
            .into();
        }
        if tag == "textarea" {
            let value = trimmed_text(node.value.as_deref())
                .or_else(|| trimmed_text(node.placeholder.as_deref()));
            return [self.base_element(
                &tag,
                path,
                local,
                node,
                ElementKind::Textarea,
                &value,
                None,
                vec![],
            )]
            .into();
        }
        if tag == "select" {
            let element = self.base_element(
                &tag,
                path,
                local,
                node,
                ElementKind::Select {
                    options: node.options.clone(),
                },
                &text,
                None,
                vec![],
            );
            return [element].into();
        }
        if tag == "a" {
            let url = self.map_link(node.href.as_deref());
            let inline_children = node
                .children
                .iter()
                .all(|child| is_text_tag(&child.tag.trim().to_ascii_lowercase()));
            let label = text.clone().or_else(|| {
                inline_children
                    .then(|| descendant_text(&node.children))
                    .flatten()
            });
            // An anchor around a card keeps its content as a linked box.
            if let Some(label) = label.filter(|_| children.is_empty() || inline_children) {
                if url.is_some() {
                    return [self.base_element(
                        &tag,
                        path,
                        local,
                        node,
                        ElementKind::Link,
                        &Some(label),
                        url,
                        vec![],
                    )]
                    .into();
                }
                return [self.text_element(&tag, path, local, node, &label)].into();
            }
            let element = self.base_element(
                &tag,
                path,
                local,
                node,
                ElementKind::Box,
                &text,
                url,
                children,
            );
            return [element].into();
        }
        if tag == "button" {
            let value = text
                .or_else(|| descendant_text(&node.children))
                .or_else(|| trimmed_text(node.value.as_deref()))
                .or_else(|| trimmed_text(node.alt.as_deref()))
                .unwrap_or_else(|| "Button".into());
            return [self.base_element(
                &tag,
                path,
                local,
                node,
                ElementKind::Button,
                &Some(value),
                None,
                vec![],
            )]
            .into();
        }
        if is_text_tag(&tag)
            && let Some(text) = text.clone()
        {
            if children.is_empty() {
                return [self.text_element(&tag, path, local, node, &text)].into();
            }
            let mut merged = children;
            let mut text_element = self.text_element(
                &tag,
                path,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: local.width,
                    height: local.height,
                },
                node,
                &text,
            );
            text_element.id = stable_uuid(&format!(
                "{}:{}:text",
                self.element_id(path, &tag),
                path.len()
            ));
            merged.insert(0, text_element);
            let layout = layout_for(&node.styles);
            if !visual(&node.styles) && matches!(layout, ElementLayout::Free) {
                return merged
                    .into_iter()
                    .map(|mut child| {
                        child.rect.x += node.rect.x - origin.0;
                        child.rect.y += node.rect.y - origin.1;
                        child
                    })
                    .collect();
            }
            let boxed = self.base_element(
                &tag,
                path,
                local,
                node,
                ElementKind::Box,
                &None,
                None,
                merged,
            );
            return [boxed].into();
        }
        // Without its own text, a cell or list item is a container like any other: its
        // children move into the parent's coordinates and a painted background stays.

        // Every other visible element is a container. Transparent wrappers are merged into
        // their parent; visual boxes and flex/grid containers stay as real layers.
        let layout = layout_for(&node.styles);
        let keep = visual(&node.styles) || !matches!(layout, ElementLayout::Free);
        if !keep {
            if let Some(text) = text {
                let mut merged = children;
                merged.insert(0, self.text_element(&tag, path, local, node, &text));
                return merged;
            }
            return children
                .into_iter()
                .map(|mut child| {
                    child.rect.x += node.rect.x - origin.0;
                    child.rect.y += node.rect.y - origin.1;
                    child
                })
                .collect();
        }
        let element = self.base_element(
            &tag,
            path,
            local,
            node,
            ElementKind::Box,
            &text,
            None,
            children,
        );
        [element].into()
    }

    fn children_elements(
        &self,
        node: &WebNode,
        path: &mut Vec<usize>,
        depth: usize,
        origin: (f64, f64),
    ) -> Vec<Element> {
        let mut elements = Vec::new();
        for (index, child) in node.children.iter().enumerate() {
            path.push(index);
            elements.extend(self.elements(child, origin, path, depth + 1));
            path.pop();
        }
        elements
    }

    #[allow(clippy::too_many_arguments)]
    fn base_element(
        &self,
        tag: &str,
        path: &[usize],
        rect: Rect,
        node: &WebNode,
        kind: ElementKind,
        text: &Option<String>,
        url: Option<String>,
        children: Vec<Element>,
    ) -> Element {
        // A flex or grid link or button keeps its place; only boxes lay out children.
        let layout = if kind.holds_children() {
            layout_for(&node.styles)
        } else {
            ElementLayout::Free
        };
        Element {
            id: self.element_id(path, tag),
            kind,
            name: None,
            link: None,
            rect,
            rotation: 0.0,
            visible: true,
            locked: false,
            style: self.node_style(node, false),
            text: clip_text(text.as_deref().unwrap_or_default()),
            image: None,
            url,
            layout,
            overrides: BTreeMap::new(),
            children,
            meta: ElementMeta::default(),
        }
    }

    fn text_element(
        &self,
        tag: &str,
        path: &[usize],
        rect: Rect,
        node: &WebNode,
        text: &str,
    ) -> Element {
        Element {
            id: self.element_id(path, &format!("{tag}:text")),
            kind: ElementKind::Text,
            name: None,
            link: None,
            rect,
            rotation: 0.0,
            visible: true,
            locked: false,
            style: {
                // A single line that stands alone hugs its text, so a substitute font that runs
                // wider than the site's own never cuts it short. Text sharing its line with a
                // link or other inline child keeps its width, so it does not run into them.
                let mut style = self.node_style(node, true);
                if node.children.is_empty()
                    && rect.height
                        <= node.styles.font_size * node.styles.line_height.clamp(0.5, 4.0) * 1.5
                    && !text.contains('\n')
                {
                    style.sizing.width = SizingMode::Fit;
                }
                style
            },
            text: clip_text(text),
            image: None,
            url: None,
            layout: ElementLayout::Free,
            overrides: BTreeMap::new(),
            children: vec![],
            meta: ElementMeta::default(),
        }
    }
}

fn page_plans<'a>(captures: &'a [WebCapture], project: &Project) -> Result<Vec<PagePlan<'a>>> {
    ensure(
        !captures.is_empty(),
        "Render at least one page before importing it.",
    )?;
    ensure(
        captures.len() <= 60,
        "A website import renders at most 30 pages.",
    )?;
    let mut grouped: BTreeMap<String, (&WebCapture, Option<&WebCapture>)> = BTreeMap::new();
    for capture in captures {
        let key = normalize_url(&capture.url);
        let entry = grouped.entry(key).or_insert((capture, None));
        if capture.width >= 700.0 {
            if entry.0.width < 700.0 {
                entry.0 = capture;
            }
        } else if entry.1.is_none() {
            entry.1 = Some(capture);
        }
    }
    let remaining = crate::MAX_PAGES.saturating_sub(project.pages.len());
    ensure(
        remaining > 0,
        "This project already holds the maximum number of boundaries.",
    )?;
    let limit = remaining.min(30);
    let existing: BTreeSet<String> = project
        .pages
        .iter()
        .map(|page| page.path.to_ascii_lowercase())
        .collect();
    let first = grouped
        .keys()
        .next()
        .and_then(|url| url::Url::parse(url).ok());
    let base = captures
        .first()
        .map(|capture| capture.site.trim())
        .filter(|site| !site.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            first
                .as_ref()
                .and_then(|url| url.host_str())
                .map(|host| host.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_else(|| "website".into())
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    let base = if base.is_empty() {
        "website".into()
    } else {
        base
    };
    let mut paths = existing;
    let mut plans = Vec::new();
    for (key, (desktop, phone)) in grouped {
        let parsed = url::Url::parse(&key).ok();
        let raw_path = if desktop.route.is_empty() {
            parsed.as_ref().map(|url| url.path()).unwrap_or("/")
        } else {
            desktop.route.as_str()
        };
        let slug = path_slug(raw_path, &base);
        let mut path = slug.clone();
        let mut suffix = 2;
        while paths.contains(&path.to_ascii_lowercase()) {
            path = format!("{slug}-{suffix}");
            suffix += 1;
        }
        paths.insert(path.to_ascii_lowercase());
        let title = desktop.title.trim();
        let name = if title.is_empty() {
            if raw_path == "/" || raw_path.is_empty() {
                "Imported home".into()
            } else {
                segment_name(raw_path)
            }
        } else {
            title.chars().take(120).collect()
        };
        let page_id = stable_uuid(&format!("website:page:{key}"));
        let size_id = stable_uuid(&format!("website:size:{key}:desktop"));
        let phone_size_id = stable_uuid(&format!("website:size:{key}:phone"));
        plans.push(PagePlan {
            url: key,
            desktop,
            phone,
            path,
            name,
            page_id,
            size_id,
            phone_size_id,
        });
        if plans.len() >= limit {
            break;
        }
    }
    Ok(plans)
}

fn snake(value: &str) -> String {
    let lowered = value.trim().to_ascii_lowercase();
    let mut slug = lowered
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    slug.trim_matches('-').to_owned()
}

fn reserved_segment(segment: &str) -> bool {
    matches!(
        segment,
        "assets" | "src" | "node_modules" | "con" | "prn" | "aux" | "nul"
    ) || (segment.len() == 4
        && (segment.starts_with("com") || segment.starts_with("lpt"))
        && segment.ends_with(['1', '2', '3', '4', '5', '6', '7', '8', '9']))
}

fn path_slug(raw_path: &str, base: &str) -> String {
    let base = snake(base);
    let base = if base.is_empty() {
        "website".to_owned()
    } else if reserved_segment(&base) {
        format!("{base}-site")
    } else {
        base
    };
    let mut segments: Vec<String> = raw_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let without_extension = segment
                .rsplit_once('.')
                .filter(|(_, extension)| {
                    matches!(extension.to_ascii_lowercase().as_str(), "html" | "htm")
                })
                .map_or(segment, |(stem, _)| stem);
            let slug = snake(without_extension);
            if slug.is_empty() || slug == "index" {
                String::new()
            } else {
                let slug: String = slug.chars().take(60).collect();
                if reserved_segment(&slug) {
                    format!("{slug}-page")
                } else {
                    slug
                }
            }
        })
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.last().is_some_and(|segment| segment == "index") {
        segments.pop();
    }
    if segments.is_empty() {
        format!("/{base}")
    } else {
        format!("/{base}/{}", segments.join("/"))
    }
}

fn segment_name(raw_path: &str) -> String {
    raw_path
        .trim_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .rsplit_once('.')
                .map_or(value, |(stem, _)| stem)
                .replace(['-', '_'], " ")
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Page".into())
}

/// Convert rendered pages into boundaries, layers, script notes and behaviour pointers.
pub fn import_website(
    captures: &[WebCapture],
    picture_ids: &BTreeMap<String, Uuid>,
    project: &Project,
) -> Result<ImportedWebsite> {
    let plans = page_plans(captures, project)?;
    let page_paths: BTreeMap<String, String> = plans
        .iter()
        .map(|plan| (normalize_url(&plan.url), plan.path.clone()))
        .collect();
    let own_hosts = own_hosts(captures, &page_paths);
    let mut right = project
        .pages
        .iter()
        .flat_map(|page| page.sizes.iter().map(|size| size.x + size.width as f64))
        .chain(
            project
                .reference
                .iter()
                .map(|item| item.rect.x + item.rect.width),
        )
        .fold(0.0_f64, f64::max);
    if right <= 0.0 {
        right = 0.0;
    }
    let top = project
        .pages
        .iter()
        .flat_map(|page| page.sizes.iter().map(|size| size.y))
        .fold(0.0_f64, f64::min);
    right += 160.0;
    let mut imported = ImportedWebsite::default();
    let mut element_total = project
        .pages
        .iter()
        .map(|page| count_elements(&page.elements))
        .sum::<usize>();
    let pointer_room = crate::MAX_POINTERS.saturating_sub(project.pointers.len());
    let mut pointer_total = 0usize;
    let mut reference_room = crate::MAX_REFERENCE_ITEMS.saturating_sub(project.reference.len());
    let mut script_ranges = Vec::new();
    for plan in &plans {
        let page_x = right;
        let page_y = top;
        let desktop_height = plan.desktop.height.clamp(1.0, 100_000.0).round() as u32;
        let converter = Converter {
            page: plan,
            picture_ids,
            page_paths: &page_paths,
            own_hosts: &own_hosts,
            element_count: 0,
            warnings: Vec::new(),
        };
        let mut base_path = Vec::new();
        let mut base_elements = converter.children_elements(
            &WebNode {
                tag: "body".into(),
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: plan.desktop.width,
                    height: desktop_height as f64,
                },
                text: None,
                src: None,
                poster: None,
                href: None,
                input_type: None,
                placeholder: None,
                value: None,
                alt: None,
                options: vec![],
                attributes: WebAttributes::default(),
                styles: WebStyles::default(),
                children: plan.desktop.nodes.clone(),
            },
            &mut base_path,
            0,
            (0.0, 0.0),
        );
        let mut phone_size = Vec::new();
        if let Some(phone) = plan.phone {
            let phone_height = phone.height.clamp(1.0, 100_000.0).round() as u32;
            let phone_converter = Converter {
                page: plan,
                picture_ids,
                page_paths: &page_paths,
                own_hosts: &own_hosts,
                element_count: 0,
                warnings: Vec::new(),
            };
            let mut phone_path = Vec::new();
            let phone_elements = phone_converter.children_elements(
                &WebNode {
                    tag: "body".into(),
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: phone.width,
                        height: phone_height as f64,
                    },
                    text: None,
                    src: None,
                    poster: None,
                    href: None,
                    input_type: None,
                    placeholder: None,
                    value: None,
                    alt: None,
                    options: vec![],
                    attributes: WebAttributes::default(),
                    styles: WebStyles::default(),
                    children: phone.nodes.clone(),
                },
                &mut phone_path,
                0,
                (0.0, 0.0),
            );
            let mut overrides = BTreeMap::new();
            collect_overrides(&phone_elements, &mut overrides);
            apply_overrides(&mut base_elements, &overrides, plan.phone_size_id);
            phone_size.push(PageSize {
                id: plan.phone_size_id,
                label: "Phone".into(),
                width: 390,
                height: phone_height,
                kind: BoundaryKind::Page,
                x: page_x + 1440.0 + 160.0,
                y: page_y,
            });
            imported.warnings.extend(converter.warnings.iter().cloned());
            imported
                .warnings
                .extend(phone_converter.warnings.iter().cloned());
        }
        let mut sizes = vec![PageSize {
            id: plan.size_id,
            label: "Desktop".into(),
            width: 1440,
            height: desktop_height,
            kind: BoundaryKind::Page,
            x: page_x,
            y: page_y,
        }];
        sizes.extend(phone_size);
        let width = if plan.phone.is_some() {
            1440.0 + 160.0 + 390.0
        } else {
            1440.0
        };
        right = page_x + width + 160.0;

        let page_elements = count_elements(&base_elements);
        if element_total + page_elements > crate::MAX_ELEMENTS {
            imported.warnings.push(
                "The remaining pages were left out to stay inside the project element limit."
                    .into(),
            );
            break;
        }
        element_total += page_elements;

        // Scripts are reference notes below the boundary, not layers inside it.
        let script_start = imported.reference.len();
        let mut note_x = page_x;
        let mut note_y = page_y + f64::from(desktop_height) + 24.0;
        for (index, script) in plan
            .desktop
            .scripts
            .iter()
            .take(12.min(reference_room))
            .enumerate()
        {
            let label = script
                .src
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    script
                        .text
                        .as_deref()
                        .and_then(|text| text.lines().map(str::trim).find(|line| !line.is_empty()))
                        .map(str::to_owned)
                });
            let Some(label) = label else { continue };
            let text = format!("Script: {}", label.chars().take(220).collect::<String>());
            if reference_room == 0 {
                break;
            }
            reference_room -= 1;
            imported.reference.push(ReferenceItem {
                id: stable_uuid(&format!("website:script:{}:{index}", plan.url)),
                rect: Rect {
                    x: note_x,
                    y: note_y,
                    width: 280.0,
                    height: 96.0,
                },
                kind: ReferenceKind::Note { text },
            });
            note_x += 300.0;
            if index % 3 == 2 {
                note_x = page_x;
                note_y += 110.0;
            }
        }

        let pointers = behaviour_pointers(plan.desktop, plan);
        script_ranges.push(script_start..imported.reference.len());
        let page = Page {
            id: plan.page_id,
            name: plan.name.clone(),
            path: plan.path.clone(),
            included: true,
            output: OutputType::Webpage,
            sizes,
            background: "#ffffff".into(),
            elements: base_elements,
        };
        for pointer in pointers {
            if pointer_total >= pointer_room {
                imported.warnings.push(
                    "Some recognisable behaviours were left out at the project pointer limit."
                        .into(),
                );
                break;
            }
            pointer_total += 1;
            imported.pointers.push(pointer);
        }
        imported.pages.push(page);
    }
    layout_sitemap(&mut imported, &plans, &script_ranges, project);
    Ok(imported)
}

/// Keep the stored page order intact; only canvas positions follow the site's navigation.
fn layout_sitemap(
    imported: &mut ImportedWebsite,
    plans: &[PagePlan<'_>],
    script_ranges: &[std::ops::Range<usize>],
    project: &Project,
) {
    fn links(elements: &[Element], paths: &BTreeMap<&str, usize>, out: &mut Vec<usize>) {
        for element in elements {
            if matches!(
                element.kind,
                ElementKind::Link | ElementKind::Button | ElementKind::Box
            ) && let Some(target) = element.url.as_deref().and_then(|url| paths.get(url))
            {
                out.push(*target);
            }
            links(&element.children, paths, out);
        }
    }

    if imported.pages.is_empty() {
        return;
    }
    let routes: Vec<String> = plans
        .iter()
        .take(imported.pages.len())
        .map(|plan| {
            if plan.desktop.route.is_empty() {
                url::Url::parse(&plan.url)
                    .map(|url| normalize_url(url.path()))
                    .unwrap_or_else(|_| "/".into())
            } else {
                normalize_url(&plan.desktop.route)
            }
        })
        .collect();
    let sections: Vec<String> = routes
        .iter()
        .map(|route| {
            let segments: Vec<_> = route
                .split('/')
                .filter(|segment| !segment.is_empty())
                .collect();
            if segments.len() > 1 {
                segments[0].to_owned()
            } else {
                String::new()
            }
        })
        .collect();
    let labelled = sections.iter().collect::<BTreeSet<_>>().len() > 1;
    let paths = imported
        .pages
        .iter()
        .enumerate()
        .map(|(index, page)| (page.path.as_str(), index))
        .collect();
    let mut edges = Vec::new();
    for page in &imported.pages {
        let mut targets = Vec::new();
        links(&page.elements, &paths, &mut targets);
        edges.push(targets);
    }
    let home = routes.iter().position(|route| route == "/").unwrap_or(0);
    let mut depths = vec![usize::MAX; imported.pages.len()];
    let mut order = vec![usize::MAX; imported.pages.len()];
    let mut queue = VecDeque::from([home]);
    depths[home] = 0;
    order[home] = 0;
    let mut next_order = 1;
    while let Some(source) = queue.pop_front() {
        for &target in &edges[source] {
            if depths[target] == usize::MAX {
                depths[target] = depths[source] + 1;
                order[target] = next_order;
                next_order += 1;
                queue.push_back(target);
            }
        }
    }
    let mut rows: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, depth) in depths.into_iter().enumerate() {
        rows.entry(depth).or_default().push(index);
    }
    let left = project
        .pages
        .iter()
        .flat_map(|page| &page.sizes)
        .map(|size| size.x + f64::from(size.width))
        .chain(
            project
                .reference
                .iter()
                .map(|item| item.rect.x + item.rect.width),
        )
        .fold(0.0_f64, f64::max)
        + 160.0;
    let mut top = project
        .pages
        .iter()
        .flat_map(|page| &page.sizes)
        .map(|size| size.y)
        .fold(0.0_f64, f64::min);
    // A long level wraps after a few pages, so it reads as a block rather than a strip.
    const PAGES_PER_LINE: usize = 6;
    let label_room = if labelled { 120.0 } else { 0.0 };
    for (depth, mut row) in rows {
        row.sort_by_key(|&index| (&sections[index], order[index], index));
        let mut page_y = top + label_room;
        let mut x = left;
        let mut bottom = page_y;
        let mut previous_section = None;
        for (position, index) in row.into_iter().enumerate() {
            if position > 0 && position % PAGES_PER_LINE == 0 {
                x = left;
                page_y = bottom + 240.0 + label_room;
                previous_section = None;
            }
            let section = &sections[index];
            if previous_section != Some(section) {
                if previous_section.is_some() {
                    x += 240.0;
                }
                if labelled
                    && project.reference.len() + imported.reference.len()
                        < crate::MAX_REFERENCE_ITEMS
                {
                    let label = if section.is_empty() {
                        "Top level".to_owned()
                    } else {
                        let name = section.replace(['-', '_'], " ");
                        let mut chars = name.chars();
                        chars.next().unwrap().to_uppercase().collect::<String>() + chars.as_str()
                    };
                    imported.reference.push(ReferenceItem {
                        id: stable_uuid(&format!(
                            "website:section:{}:{depth}:{}:{section}",
                            plans[home].url,
                            position / PAGES_PER_LINE
                        )),
                        rect: Rect {
                            x,
                            y: page_y - label_room,
                            width: 280.0,
                            height: 80.0,
                        },
                        kind: ReferenceKind::Note { text: label },
                    });
                }
                previous_section = Some(section);
            }
            let page = &mut imported.pages[index];
            let dx = x - page.sizes[0].x;
            let dy = page_y - page.sizes[0].y;
            let mut right = x;
            let mut page_bottom = page_y;
            for size in &mut page.sizes {
                size.x += dx;
                size.y += dy;
                right = right.max(size.x + f64::from(size.width));
                page_bottom = page_bottom.max(size.y + f64::from(size.height));
            }
            // Script notes clear both sizes, including a phone taller than the desktop.
            let notes = &mut imported.reference[script_ranges[index].clone()];
            if let Some(first) = notes.first() {
                let note_dy = page_bottom + 24.0 - first.rect.y;
                for note in notes {
                    note.rect.x += dx;
                    note.rect.y += note_dy;
                    bottom = bottom.max(note.rect.y + note.rect.height);
                }
            }
            bottom = bottom.max(page_bottom);
            x = right + 160.0;
        }
        top = bottom + 240.0;
    }
}

fn count_elements(elements: &[Element]) -> usize {
    elements
        .iter()
        .map(|element| 1 + count_elements(&element.children))
        .sum()
}

fn collect_overrides(elements: &[Element], overrides: &mut BTreeMap<Uuid, ElementOverride>) {
    for element in elements {
        overrides.insert(
            element.id,
            ElementOverride {
                rect: Some(element.rect),
                style: Some(element.style.clone()),
                layout: element
                    .kind
                    .holds_children()
                    .then(|| element.layout.clone()),
                hidden: false,
                dismissed: false,
                automatic: false,
            },
        );
        collect_overrides(&element.children, overrides);
    }
}

fn apply_overrides(
    elements: &mut [Element],
    overrides: &BTreeMap<Uuid, ElementOverride>,
    phone_size: Uuid,
) {
    for element in elements {
        if let Some(over) = overrides.get(&element.id) {
            element.overrides.insert(phone_size, over.clone());
        }
        apply_overrides(&mut element.children, overrides, phone_size);
    }
}

fn behaviour_pointers(capture: &WebCapture, plan: &PagePlan<'_>) -> Vec<Pointer> {
    let mut pointers = Vec::new();
    let mut path = Vec::new();
    walk_behaviours(&capture.nodes, &mut path, plan, &mut pointers);
    pointers
}

fn walk_behaviours(
    nodes: &[WebNode],
    path: &mut Vec<usize>,
    plan: &PagePlan<'_>,
    pointers: &mut Vec<Pointer>,
) {
    for (index, node) in nodes.iter().enumerate() {
        path.push(index);
        let tag = node.tag.trim().to_ascii_lowercase();
        let class = node.attributes.class_name.to_ascii_lowercase();
        let id = node.attributes.id.to_ascii_lowercase();
        let role = node.attributes.role.to_ascii_lowercase();
        let controls = node.attributes.aria_controls.clone().unwrap_or_default();
        let expanded = node.attributes.aria_expanded.is_some();
        let toggle = node
            .attributes
            .data_toggle
            .clone()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let menuish = class.contains("menu")
            || class.contains("nav")
            || class.contains("hamburger")
            || id.contains("menu")
            || id.contains("nav")
            || toggle.contains("collapse")
            || toggle.contains("menu");
        let tabish = role == "tab" || (class.contains("tab") && (expanded || !controls.is_empty()));
        let accordion = class.contains("accordion") || toggle.contains("collapse");
        let visible =
            node.rect.width >= 1.0 && node.rect.height >= 1.0 && node.styles.opacity > 0.001;
        if visible
            && (tag == "button" || tag == "a" || role == "button")
            && (menuish || tabish || accordion)
        {
            let comment = if tabish {
                "Switches to the tab panel this control names."
            } else if accordion {
                "Expands and collapses this section."
            } else {
                "Toggles the navigation menu."
            };
            let element_id = stable_uuid(&format!(
                "website:{}:{}:{}",
                normalize_url(&plan.url),
                tag,
                path.iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(".")
            ));
            pointers.push(Pointer {
                id: stable_uuid(&format!(
                    "website:pointer:{}:{}",
                    plan.url,
                    path.iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join(".")
                )),
                from: PointerSource::Element {
                    page_id: plan.page_id,
                    element_id,
                    size_id: Some(plan.size_id),
                },
                to: PointerTarget::Boundary {
                    page_id: plan.page_id,
                    size_id: Some(plan.size_id),
                },
                comment: comment.into(),
                kind: crate::PointerKind::Reference,
                wiring: None,
                play: None,
                play_comment: String::new(),
            });
        }
        walk_behaviours(&node.children, path, plan, pointers);
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::{internal_href, normalize_url};
    use std::collections::{BTreeMap, BTreeSet};

    fn own() -> BTreeSet<String> {
        ["www.oldsite.example".to_string()].into()
    }

    fn page_paths() -> BTreeMap<String, String> {
        [
            ("http://127.0.0.1:9/token/", "/site"),
            ("http://127.0.0.1:9/token/pages/p002", "/site/pages/p002"),
            ("http://127.0.0.1:9/token/main.htm", "/site/main"),
        ]
        .into_iter()
        .map(|(url, path)| (normalize_url(url), path.to_owned()))
        .collect()
    }

    #[test]
    fn absolute_links_on_the_original_domain_find_the_imported_page() {
        let pages = page_paths();
        let page = "http://127.0.0.1:9/token/pages/p001";
        assert_eq!(
            internal_href(
                "http://www.oldsite.example/pages/P002.HTM",
                page,
                &pages,
                &own()
            )
            .as_deref(),
            Some("/site/pages/p002")
        );
        assert_eq!(
            internal_href(
                "https://www.oldsite.example/pages/p002.html",
                page,
                &pages,
                &own()
            )
            .as_deref(),
            Some("/site/pages/p002")
        );
        assert_eq!(
            internal_href(
                "http://www.oldsite.example/pages/p999.htm",
                page,
                &pages,
                &own()
            ),
            None
        );
        // A domain the site does not use for its own pages stays external, even at a matching path.
        assert_eq!(
            internal_href("https://social.example/", page, &pages, &own()),
            None
        );
        assert_eq!(
            internal_href("https://social.example/pages/p002", page, &pages, &own()),
            None
        );
    }

    #[test]
    fn page_files_match_with_a_different_letter_case() {
        let pages = page_paths();
        let page = "http://127.0.0.1:9/token/pages/p002";
        assert_eq!(
            internal_href("../MAIN.HTM", page, &pages, &own()).as_deref(),
            Some("/site/main")
        );
        assert_eq!(
            internal_href("http://127.0.0.1:9/token/MAIN.htm", page, &pages, &own()).as_deref(),
            Some("/site/main")
        );
        assert_eq!(
            internal_href(
                "http://www.oldsite.example/MAIN.htm",
                "http://127.0.0.1:9/token/main.htm",
                &pages,
                &own()
            ),
            None
        );
    }

    #[test]
    fn public_url_imports_keep_their_links_external() {
        let pages: BTreeMap<String, String> = [
            ("https://oldsite.example/pages/p001", "/site/pages/p001"),
            ("https://oldsite.example/pages/p002", "/site/pages/p002"),
        ]
        .into_iter()
        .map(|(url, path)| (normalize_url(url), path.to_owned()))
        .collect();
        let page = "https://oldsite.example/pages/p001";
        assert_eq!(
            internal_href(
                "https://www.oldsite.example/pages/P002.HTM",
                page,
                &pages,
                &own()
            ),
            None
        );
        assert_eq!(
            internal_href(
                "https://www.oldsite.example/pages/p002.html",
                page,
                &pages,
                &own()
            ),
            None
        );
    }
}
