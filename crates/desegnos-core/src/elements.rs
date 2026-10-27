//! Visual elements: the things drawn on the canvas. Elements hold visual properties only;
//! code appears at export. Every page boundary owns a back-to-front tree of root elements.
use crate::{
    Error, MAX_PROMPT_CHARS, Page, Project, Rect, RequestAttachment, Result, ensure,
    model::validate_text,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const MAX_ELEMENTS: usize = 10_000;
pub const MAX_ELEMENT_DEPTH: usize = 32;
pub const MAX_ELEMENT_CODE_CHARS: usize = 200_000;
pub const MAX_ELEMENT_REQUESTS: usize = 100;
pub const MAX_ELEMENT_CONTEXT: usize = 32;
pub const MAX_ELEMENT_OPTIONS: usize = 100;
pub const MAX_ELEMENT_NAME_CHARS: usize = 120;
pub const MAX_LINKED_DEFINITIONS: usize = 1_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InputType {
    Text,
    Email,
    Password,
    Number,
    Search,
    Tel,
    Url,
    Date,
    Time,
    Color,
    Range,
    File,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StackDirection {
    Row,
    Column,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StackAlign {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StackJustify {
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// What a visual element is. Kind-specific settings live on the variant, so an input always
/// carries its type, a select its options and a code block its language and source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ElementKind {
    Box,
    /// A layer made of several elements; it has no visuals of its own and its rect is the union.
    Group,
    Text,
    Ellipse,
    Line,
    Image,
    Button,
    Link,
    Input { input_type: InputType },
    Textarea,
    Select { options: Vec<String> },
    Checkbox,
    Radio,
    Video,
    Audio,
    Embed,
    Code { language: String, source: String },
}

impl ElementKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Box => "Box",
            Self::Group => "Group",
            Self::Text => "Text",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
            Self::Image => "Image",
            Self::Button => "Button",
            Self::Link => "Link",
            Self::Input { .. } => "Input",
            Self::Textarea => "Textarea",
            Self::Select { .. } => "Select",
            Self::Checkbox => "Checkbox",
            Self::Radio => "Radio",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Embed => "Embed",
            Self::Code { .. } => "Code",
        }
    }

    pub fn holds_children(&self) -> bool {
        matches!(self, Self::Box | Self::Group)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementStroke {
    /// `None` draws no stroke.
    pub color: Option<String>,
    pub width: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementShadow {
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    /// `None` draws no shadow.
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementTextStyle {
    pub font_family: String,
    pub size: f64,
    pub weight: u16,
    pub italic: bool,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub color: String,
    pub align: TextAlign,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementStyle {
    /// `None` fills nothing.
    pub fill: Option<String>,
    pub stroke: ElementStroke,
    pub radius: f64,
    pub opacity: f64,
    pub shadow: ElementShadow,
    pub text_style: ElementTextStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ElementLayout {
    Free,
    Stack {
        direction: StackDirection,
        gap: f64,
        padding: f64,
        align: StackAlign,
        justify: StackJustify,
        wrap: bool,
    },
    Grid {
        columns: u32,
        gap: f64,
        padding: f64,
    },
}

impl ElementLayout {
    pub fn is_free(&self) -> bool {
        matches!(self, Self::Free)
    }

    pub fn free() -> Self {
        Self::Free
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Free => Ok(()),
            Self::Stack { gap, padding, .. } => {
                ensure(
                    gap.is_finite() && (0.0..=4096.0).contains(gap),
                    "A stack gap must be between 0 and 4,096 pixels.",
                )?;
                ensure(
                    padding.is_finite() && (0.0..=4096.0).contains(padding),
                    "Stack padding must be between 0 and 4,096 pixels.",
                )
            }
            Self::Grid {
                columns,
                gap,
                padding,
            } => {
                ensure(
                    (1..=64).contains(columns),
                    "A grid needs between 1 and 64 columns.",
                )?;
                ensure(
                    gap.is_finite() && (0.0..=4096.0).contains(gap),
                    "A grid gap must be between 0 and 4,096 pixels.",
                )?;
                ensure(
                    padding.is_finite() && (0.0..=4096.0).contains(padding),
                    "Grid padding must be between 0 and 4,096 pixels.",
                )
            }
        }
    }
}

/// A request waiting to be sent for one layer. A frame is a box with one; Ask AI and the
/// request box in Properties put one on any layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingRequest {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<RequestAttachment>,
    pub requested_at: String,
    /// The linked size the request was written in, so the AI can answer for that size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_id: Option<Uuid>,
}

impl PendingRequest {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.prompt, "Request", MAX_PROMPT_CHARS, false)?;
        ensure(
            !self.requested_at.is_empty()
                && self.requested_at.chars().count() <= 64
                && !self.requested_at.chars().any(char::is_control),
            "A request needs a short creation time.",
        )?;
        ensure(
            self.attachments.len() <= MAX_ELEMENT_CONTEXT,
            "A request supports at most 32 attachments.",
        )?;
        for attachment in &self.attachments {
            attachment.validate()?;
        }
        Ok(())
    }
}

/// One past request on a layer. Outcome and job arrive with the AI step; until then a manual
/// Ask AI records the prompt and leaves them empty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestRecord {
    pub id: Uuid,
    pub prompt: String,
    pub attachments: Vec<RequestAttachment>,
    pub created_at: String,
    pub outcome: Option<String>,
    pub job_id: Option<String>,
}

impl RequestRecord {
    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.id.is_nil(),
            "A request record needs a nonempty identifier.",
        )?;
        validate_text(&self.prompt, "Request", MAX_PROMPT_CHARS, true)?;
        ensure(
            !self.created_at.is_empty()
                && self.created_at.chars().count() <= 64
                && !self.created_at.chars().any(char::is_control),
            "A request record needs a short creation time.",
        )?;
        if let Some(outcome) = &self.outcome {
            ensure(
                outcome.chars().count() <= 4096 && !outcome.contains('\0'),
                "A request outcome is limited to 4,096 characters.",
            )?;
        }
        ensure(
            self.job_id
                .as_deref()
                .is_none_or(|id| !id.trim().is_empty() && id.len() <= 64),
            "A request job identifier must be short and nonempty.",
        )?;
        ensure(
            self.attachments.len() <= MAX_ELEMENT_CONTEXT,
            "A request supports at most 32 attachments.",
        )?;
        for attachment in &self.attachments {
            attachment.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementMeta {
    /// Newest last on disk; Properties lists them newest first.
    pub requests: Vec<RequestRecord>,
    pub context: Vec<RequestAttachment>,
    /// A request waiting for Generate. One per layer; applying clears it.
    #[serde(default, deserialize_with = "crate::required_nullable", skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingRequest>,
}

impl ElementMeta {
    fn validate(&self) -> Result<()> {
        ensure(
            self.requests.len() <= MAX_ELEMENT_REQUESTS,
            "A layer keeps at most 100 past requests.",
        )?;
        for record in &self.requests {
            record.validate()?;
        }
        ensure(
            self.context.len() <= MAX_ELEMENT_CONTEXT,
            "A layer supports at most 32 context attachments.",
        )?;
        for attachment in &self.context {
            attachment.validate()?;
        }
        if let Some(pending) = &self.pending {
            pending.validate()?;
        }
        Ok(())
    }
}

/// What one linked size changes about an element. Missing fields inherit the base values, so a
/// size only stores what it overrides. `hidden` removes the element from that size and draws a
/// placeholder where it would go; `dismissed` keeps it hidden without the placeholder.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ElementStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<ElementLayout>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub dismissed: bool,
}

impl ElementOverride {
    pub fn is_empty(&self) -> bool {
        self.rect.is_none()
            && self.style.is_none()
            && self.layout.is_none()
            && !self.hidden
            && !self.dismissed
    }

    fn validate(&self, element: &Element) -> Result<()> {
        if let Some(rect) = &self.rect {
            validate_element_rect(rect)?;
        }
        if let Some(style) = &self.style {
            style.validate()?;
        }
        if let Some(layout) = &self.layout {
            ensure(
                element.kind.holds_children(),
                "Only a box lays out its children.",
            )?;
            layout.validate()?;
        }
        ensure(
            !self.dismissed || self.hidden,
            "A dismissed element stays hidden in that size.",
        )
    }

    fn clear(&mut self, rect: bool, style: bool, layout: bool, visible: bool) {
        if rect {
            self.rect = None;
        }
        if style {
            self.style = None;
        }
        if layout {
            self.layout = None;
        }
        if visible {
            self.hidden = false;
            self.dismissed = false;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Element {
    pub id: Uuid,
    pub kind: ElementKind,
    /// Set to name a layer.
    #[serde(deserialize_with = "crate::required_nullable")]
    pub name: Option<String>,
    /// Set on one copy of a linked element; the definition holds everything but position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Uuid>,
    /// Relative to its parent box; boundary origin for roots.
    pub rect: Rect,
    pub rotation: f64,
    pub visible: bool,
    pub locked: bool,
    pub style: ElementStyle,
    pub text: String,
    /// A picture in the project store.
    pub image: Option<Uuid>,
    /// A link, video, audio or embed target.
    pub url: Option<String>,
    pub layout: ElementLayout,
    /// Per-size changes, keyed by the page size they apply to. Text and kind are never per size.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<Uuid, ElementOverride>,
    pub children: Vec<Element>,
    pub meta: ElementMeta,
}

impl Element {
    pub fn validate(&self, project: &Project) -> Result<()> {
        ensure(
            !self.id.is_nil(),
            "An element needs a nonempty identifier.",
        )?;
        validate_element_rect(&self.rect)?;
        ensure(
            self.rotation.is_finite()
                && self.rotation.abs() <= 360_000.0,
            "An element rotation must be finite.",
        )?;
        if let Some(name) = &self.name {
            validate_text(name, "Layer name", MAX_ELEMENT_NAME_CHARS, false)?;
        }
        for over in self.overrides.values() {
            over.validate(self)?;
        }
        if self.link.is_some() {
            ensure(
                self.link.is_some_and(|id| project.linked.contains_key(&id)),
                "This linked layer is missing its linked element.",
            )?;
            ensure(
                self.children.is_empty(),
                "A linked layer keeps its content in its linked element.",
            )?;
            return Ok(());
        }
        ensure(
            self.text.chars().count() <= MAX_PROMPT_CHARS && !self.text.contains('\0'),
            "Element text is limited to 16,384 characters.",
        )?;
        self.style.validate()?;
        self.layout.validate()?;
        self.meta.validate()?;
        ensure(
            self.kind.holds_children() || self.children.is_empty(),
            "Only a box or a group can hold children.",
        )?;
        match &self.kind {
            ElementKind::Image => {
                let image = self
                    .image
                    .ok_or_else(|| Error("Choose a picture for this image.".into()))?;
                ensure(
                    project.picture_uri(image).is_some(),
                    "An image uses a picture that is not in this project.",
                )?;
            }
            ElementKind::Link => {
                let url = self
                    .url
                    .as_deref()
                    .ok_or_else(|| Error("This link needs an address.".into()))?;
                // A link may point at another boundary in this project, stored as its page path.
                if url.starts_with('/') {
                    crate::model::validate_path(url)?;
                } else {
                    validate_http_url(url)?;
                }
            }
            ElementKind::Video | ElementKind::Audio | ElementKind::Embed => {
                let url = self
                    .url
                    .as_deref()
                    .ok_or_else(|| Error("This element needs a web address.".into()))?;
                validate_http_url(url)?;
            }
            ElementKind::Input { .. }
            | ElementKind::Textarea
            | ElementKind::Select { .. }
            | ElementKind::Checkbox
            | ElementKind::Radio => {
                ensure(self.image.is_none(), "This element cannot show a picture.")?;
            }
            ElementKind::Code { language, source } => {
                validate_text(language, "Code language", 40, false)?;
                ensure(
                    source.chars().count() <= MAX_ELEMENT_CODE_CHARS
                        && !source.contains('\0'),
                    "A code block is limited to 200,000 characters.",
                )?;
            }
            _ => {}
        }
        if !matches!(self.kind, ElementKind::Image) {
            ensure(
                self.image.is_none(),
                "Only an image element uses a picture.",
            )?;
        }
        if !self.kind.holds_children() {
            ensure(
                self.layout.is_free(),
                "Only a box lays out its children.",
            )?;
        }
        if let Some(url) = &self.url {
            validate_http_url(url)?;
        }
        if let ElementKind::Select { options } = &self.kind {
            ensure(
                (1..=MAX_ELEMENT_OPTIONS).contains(&options.len()),
                "A select needs between 1 and 100 options.",
            )?;
            let mut seen = BTreeSet::new();
            for option in options {
                ensure(
                    !option.trim().is_empty()
                        && option.chars().count() <= 120
                        && !option.contains('\0'),
                    "Select options are limited to 120 characters.",
                )?;
                ensure(
                    seen.insert(option.to_lowercase()),
                    "Select options must be unique.",
                )?;
            }
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        if let Some(name) = &self.name
            && !name.trim().is_empty()
        {
            return name.clone();
        }
        match &self.kind {
            ElementKind::Code { language, .. } => {
                format!("{language} code")
            }
            kind => {
                let line = self.text.trim().split('\n').next().unwrap_or("");
                if line.is_empty() {
                    kind.label().into()
                } else {
                    line.chars().take(80).collect()
                }
            }
        }
    }
}

impl ElementStyle {
    fn validate(&self) -> Result<()> {
        if let Some(fill) = &self.fill {
            validate_colour(fill, "Fill")?;
        }
        if let Some(colour) = &self.stroke.color {
            validate_colour(colour, "Stroke")?;
        }
        ensure(
            self.stroke.width.is_finite() && (0.0..=256.0).contains(&self.stroke.width),
            "A stroke width must be between 0 and 256 pixels.",
        )?;
        ensure(
            self.radius.is_finite() && (0.0..=40_000.0).contains(&self.radius),
            "A corner radius must not be negative.",
        )?;
        ensure(
            self.opacity.is_finite() && (0.0..=1.0).contains(&self.opacity),
            "Layer opacity must be between 0 and 1.",
        )?;
        let shadow = &self.shadow;
        ensure(
            [shadow.x, shadow.y, shadow.blur]
                .iter()
                .all(|value| value.is_finite())
                && shadow.blur >= 0.0
                && shadow.blur <= 4096.0
                && shadow.x.abs() <= 40_000.0
                && shadow.y.abs() <= 40_000.0,
            "A shadow must use finite offsets and a blur between 0 and 4,096 pixels.",
        )?;
        if let Some(colour) = &shadow.color {
            validate_colour(colour, "Shadow")?;
        }
        let text = &self.text_style;
        validate_text(&text.font_family, "Font family", 200, false)?;
        ensure(
            text.size.is_finite() && (1.0..=512.0).contains(&text.size),
            "Text size must be between 1 and 512 pixels.",
        )?;
        ensure(
            (1..=1000).contains(&text.weight),
            "Text weight must be between 1 and 1,000.",
        )?;
        ensure(
            text.line_height.is_finite() && (0.0..=10.0).contains(&text.line_height),
            "Line height must be between 0 and 10.",
        )?;
        ensure(
            text.letter_spacing.is_finite() && text.letter_spacing.abs() <= 100.0,
            "Letter spacing must be between -100 and 100 pixels.",
        )?;
        validate_colour(&text.color, "Text colour")?;
        Ok(())
    }
}

fn validate_element_rect(rect: &Rect) -> Result<()> {
    ensure(
        [rect.x, rect.y, rect.width, rect.height]
            .iter()
            .all(|value| value.is_finite()),
        "Element coordinates must be finite.",
    )?;
    ensure(
        rect.width >= 0.0 && rect.height >= 0.0,
        "An element cannot have a negative size.",
    )?;
    ensure(
        (rect.x + rect.width).is_finite() && (rect.y + rect.height).is_finite(),
        "Element endpoints must be finite.",
    )
}

fn validate_http_url(url: &str) -> Result<()> {
    ensure(
        url.len() <= 2048
            && url::Url::parse(url).is_ok_and(|parsed| {
                matches!(parsed.scheme(), "https" | "http")
                    && parsed.host_str().is_some()
                    && parsed.username().is_empty()
                    && parsed.password().is_none()
            }),
        "A web address must start with http:// or https://.",
    )
}

fn validate_colour(value: &str, label: &str) -> Result<()> {
    let hex = value
        .strip_prefix('#')
        .or_else(|| value.eq_ignore_ascii_case("transparent").then_some(""));
    let valid = match hex {
        Some("") => value.eq_ignore_ascii_case("transparent"),
        Some(hex) => {
            matches!(hex.len(), 3 | 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        }
        None => false,
    };
    ensure(valid, format!("{label} must be a #rgb, #rrggbb or #rrggbbaa colour."))
}

/// Validates one page's element forest and records every id in the project set. `sizes` holds
/// the page's size ids so a per-size override can never name a size the page does not have.
pub(crate) fn validate_elements(
    elements: &[Element],
    project: &Project,
    sizes: &BTreeSet<Uuid>,
    ids: &mut BTreeSet<Uuid>,
    count: &mut usize,
) -> Result<()> {
    for element in elements {
        validate_element(element, project, sizes, ids, count, 1)?;
    }
    Ok(())
}

fn validate_element(
    element: &Element,
    project: &Project,
    sizes: &BTreeSet<Uuid>,
    ids: &mut BTreeSet<Uuid>,
    count: &mut usize,
    depth: usize,
) -> Result<()> {
    ensure(
        depth <= MAX_ELEMENT_DEPTH,
        "Elements cannot be nested more than 32 deep.",
    )?;
    *count += 1;
    ensure(
        *count <= MAX_ELEMENTS,
        "A project holds at most 10,000 elements.",
    )?;
    ensure(
        ids.insert(element.id),
        "Element identifiers must be unique.",
    )?;
    ensure(
        element.overrides.keys().all(|size| sizes.contains(size)),
        "A size override names a size this boundary does not have.",
    )?;
    element.validate(project)?;
    for child in &element.children {
        validate_element(child, project, sizes, ids, count, depth + 1)?;
    }
    Ok(())
}

pub(crate) fn find_in(elements: &[Element], id: Uuid) -> Option<&Element> {
    find_element(elements, id)
}

/// One named element every copy shares. The master holds no position: its root sits at the
/// origin and children keep rects relative to their parent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkedDefinition {
    pub id: Uuid,
    pub name: String,
    pub master: Element,
}

impl LinkedDefinition {
    pub(crate) fn validate(&self, project: &Project) -> Result<()> {
        ensure(
            !self.id.is_nil(),
            "A linked element needs a nonempty identifier.",
        )?;
        ensure(
            self.master.link.is_none(),
            "A linked element cannot hold another linked element at its root.",
        )?;
        ensure(
            self.master.rect.x == 0.0 && self.master.rect.y == 0.0,
            "A linked element keeps its position on each copy, not in its definition.",
        )?;
        let mut ids = BTreeSet::new();
        let mut count = 0;
        ensure(
            self.master.overrides.is_empty(),
            "Per-size overrides live on a copy, not on a linked element.",
        )?;
        validate_elements(
            std::slice::from_ref(&self.master),
            project,
            &BTreeSet::new(),
            &mut ids,
            &mut count,
        )
    }
}

fn find_element(elements: &[Element], id: Uuid) -> Option<&Element> {
    for element in elements {
        if element.id == id {
            return Some(element);
        }
        if let Some(found) = find_element(&element.children, id) {
            return Some(found);
        }
    }
    None
}

fn find_element_mut(elements: &mut [Element], id: Uuid) -> Option<&mut Element> {
    for element in elements.iter_mut() {
        if element.id == id {
            return Some(element);
        }
        if let Some(found) = find_element_mut(&mut element.children, id) {
            return Some(found);
        }
    }
    None
}

pub(crate) fn remove_element(elements: &mut Vec<Element>, id: Uuid) -> Option<Element> {
    if let Some(index) = elements.iter().position(|element| element.id == id) {
        return Some(elements.remove(index));
    }
    for element in elements.iter_mut() {
        if let Some(removed) = remove_element(&mut element.children, id) {
            return Some(removed);
        }
    }
    None
}

fn collect_ids(elements: &[Element], ids: &mut BTreeSet<Uuid>) {
    for element in elements {
        ids.insert(element.id);
        collect_ids(&element.children, ids);
    }
}

/// A copy's concrete tree: its own id and position, the definition's content. Children get
/// stable ids derived from the copy, so one path is the same layer in every copy.
pub fn resolve_element(project: &Project, element: &Element) -> Element {
    match element.link.and_then(|id| project.linked.get(&id)) {
        Some(definition) => resolved_instance(element, definition),
        None => {
            let mut resolved = element.clone();
            resolve_nested_instances(project, &mut resolved.children);
            resolved
        }
    }
}

fn resolve_nested_instances(project: &Project, elements: &mut [Element]) {
    for element in elements {
        if element.link.is_some() {
            *element = resolve_element(project, element);
        } else {
            resolve_nested_instances(project, &mut element.children);
        }
    }
}

/// Concrete trees for every root layer of a boundary, for drawing, sending to the AI and export.
pub fn resolve_elements(project: &Project, elements: &[Element]) -> Vec<Element> {
    elements
        .iter()
        .map(|element| resolve_element(project, element))
        .collect()
}

/// The elements a linked size really shows: linked-element resolution first, then that size's
/// overrides. Pure, so drawing, hit-testing and the AI all read the same design.
pub fn resolve_for_size(project: &Project, page: &Page, size_id: Uuid) -> Vec<Element> {
    let mut elements = resolve_elements(project, &page.elements);
    apply_size_overrides(&mut elements, size_id);
    elements
}

fn apply_size_overrides(elements: &mut [Element], size_id: Uuid) {
    for element in elements {
        if let Some(over) = element.overrides.get(&size_id) {
            if let Some(rect) = &over.rect {
                element.rect = *rect;
            }
            if let Some(style) = &over.style {
                element.style = style.clone();
            }
            if let Some(layout) = &over.layout {
                element.layout = layout.clone();
            }
            if over.hidden {
                element.visible = false;
            }
        }
        apply_size_overrides(&mut element.children, size_id);
    }
}

/// The concrete tree a copy defines, with the copy's own id, position, visibility and
/// request history, and the definition's content, size and children.
fn resolved_instance(instance: &Element, definition: &LinkedDefinition) -> Element {
    let mut concrete = definition.master.clone();
    concrete.id = instance.id;
    concrete.rect.x = instance.rect.x;
    concrete.rect.y = instance.rect.y;
    concrete.visible = instance.visible;
    concrete.locked = instance.locked;
    concrete.meta = instance.meta.clone();
    // The copy's own size overrides apply after resolution, exactly like a plain layer's.
    concrete.overrides = instance.overrides.clone();
    concrete.link = None;
    concrete.name = Some(definition.name.clone());
    let mut path = Vec::new();
    assign_nested_ids(instance.id, &mut concrete, &mut path);
    concrete
}

fn assign_nested_ids(root: Uuid, element: &mut Element, path: &mut Vec<usize>) {
    for (index, child) in element.children.iter_mut().enumerate() {
        path.push(index);
        child.id = nested_element_id(root, path);
        child.link = None;
        assign_nested_ids(root, child, path);
        path.pop();
    }
}

/// The id of a layer inside one copy, from its path in the definition.
pub fn nested_element_id(root: Uuid, path: &[usize]) -> Uuid {
    let hex = root.simple().to_string();
    let tail = format!("{:08x}", nested_path_hash(path));
    let body = format!("{}{tail}", &hex[..24]);
    Uuid::parse_str(&format!(
        "{}-{}-{}-{}-{}",
        &body[0..8],
        &body[8..12],
        &body[12..16],
        &body[16..20],
        &body[20..32]
    ))
    .expect("a nested element identifier is a uuid")
}

fn nested_path_hash(path: &[usize]) -> u32 {
    let mut value: u32 = 0x811c_9dc5;
    for step in path {
        for byte in (*step as u64).to_le_bytes() {
            value ^= u32::from(byte);
            value = value.wrapping_mul(0x0100_0193);
        }
    }
    value ^ 0xa5a5_a5a5
}

/// Where a layer lives when it is part of a linked element: the definition and its path there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkLocation {
    pub definition: Uuid,
    pub instance: Uuid,
    pub page_id: Uuid,
    pub path: Vec<usize>,
}

impl LinkLocation {
    pub fn is_copy(&self) -> bool {
        self.path.is_empty()
    }
}

/// Finds a stored layer's definition, or the definition behind one of a copy's nested layers.
pub fn link_location(project: &Project, id: Uuid) -> Option<LinkLocation> {
    for page in &project.pages {
        if let Some(element) = find_element(&page.elements, id) {
            let definition = element.link?;
            return Some(LinkLocation {
                definition,
                instance: element.id,
                page_id: page.id,
                path: Vec::new(),
            });
        }
        if let Some(location) = nested_link_location(project, &page.elements, page.id, id) {
            return Some(location);
        }
    }
    None
}

/// Finds the copy whose definition holds one of its nested layers, anywhere in a boundary.
fn nested_link_location(
    project: &Project,
    elements: &[Element],
    page_id: Uuid,
    id: Uuid,
) -> Option<LinkLocation> {
    for element in elements {
        if let Some(definition) = element.link
            && let Some(master) = project.linked.get(&definition).map(|item| &item.master)
            && let Some(path) = path_to_nested_id(master, element.id, id)
        {
            return Some(LinkLocation {
                definition,
                instance: element.id,
                page_id,
                path,
            });
        }
        if let Some(found) = nested_link_location(project, &element.children, page_id, id) {
            return Some(found);
        }
    }
    None
}

fn path_to_nested_id(master: &Element, root: Uuid, target: Uuid) -> Option<Vec<usize>> {
    fn walk(element: &Element, root: Uuid, target: Uuid, path: &mut Vec<usize>) -> bool {
        for (index, child) in element.children.iter().enumerate() {
            path.push(index);
            if nested_element_id(root, path) == target || walk(child, root, target, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    walk(master, root, target, &mut path).then_some(path)
}

/// Layers that differ only in content can be linked; a different kind or child layout cannot.
fn same_element_structure(first: &Element, second: &Element) -> bool {
    first.kind.label() == second.kind.label()
        && first.children.len() == second.children.len()
        && first
            .children
            .iter()
            .zip(&second.children)
            .all(|(left, right)| same_element_structure(left, right))
}

/// The shared tree for a new definition: no position, no request history, no link of its own.
fn definition_master(content: &Element) -> Element {
    let mut master = content.clone();
    master.id = Uuid::new_v4();
    master.name = None;
    master.link = None;
    master.rect.x = 0.0;
    master.rect.y = 0.0;
    master.overrides.clear();
    master.meta = ElementMeta::default();
    master
}

fn tree_has_link(elements: &[Element], link: Uuid) -> bool {
    elements
        .iter()
        .any(|element| element.link == Some(link) || tree_has_link(&element.children, link))
}

fn collect_resolved_ids(
    project: &Project,
    elements: &[Element],
    ids: &mut BTreeSet<Uuid>,
) {
    for element in elements {
        ids.insert(element.id);
        match element.link.and_then(|id| project.linked.get(&id)) {
            Some(definition) => {
                let mut path = Vec::new();
                collect_nested_ids(element.id, &definition.master, &mut path, ids);
            }
            None => collect_resolved_ids(project, &element.children, ids),
        }
    }
}

fn collect_nested_ids(
    root: Uuid,
    element: &Element,
    path: &mut Vec<usize>,
    ids: &mut BTreeSet<Uuid>,
) {
    for (index, child) in element.children.iter().enumerate() {
        path.push(index);
        ids.insert(nested_element_id(root, path));
        collect_nested_ids(root, child, path, ids);
        path.pop();
    }
}

/// A new copy of a linked element, placed by the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkedPasteInstance {
    pub id: Uuid,
    pub source_id: Uuid,
    pub page_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub index: usize,
    pub rect: Rect,
}

impl Project {
    /// The page and element for an id, wherever the tree holds it.
    pub fn element(&self, id: Uuid) -> Option<(&Page, &Element)> {
        self.pages.iter().find_map(|page| {
            find_element(&page.elements, id).map(|element| (page, element))
        })
    }

    pub fn element_mut(&mut self, id: Uuid) -> Option<&mut Element> {
        for page in &mut self.pages {
            if let Some(element) = find_element_mut(&mut page.elements, id) {
                return Some(element);
            }
        }
        None
    }

    /// A request waiting for Generate, on any layer of any boundary.
    pub fn element_has_pending_request(&self, id: Uuid) -> bool {
        self.element(id)
            .and_then(|(_, element)| element.meta.pending.as_ref())
            .is_some_and(|pending| !pending.prompt.trim().is_empty())
    }

    pub fn pending_request_count(&self) -> usize {
        self.pages
            .iter()
            .map(|page| count_pending(&page.elements))
            .sum()
    }

    pub(crate) fn set_element_request(
        &mut self,
        element_id: Uuid,
        request: Option<PendingRequest>,
    ) -> Result<()> {
        if let Some(request) = &request {
            request.validate()?;
        }
        let element = self
            .element_mut(element_id)
            .ok_or_else(|| Error("This layer no longer exists.".into()))?;
        element.meta.pending = request;
        Ok(())
    }

    /// Element ids in a layer's subtree, including the layer itself and the layers a linked
    /// copy resolves from its definition.
    pub fn element_subtree_ids(&self, element_id: Uuid) -> BTreeSet<Uuid> {
        let mut ids = BTreeSet::new();
        if let Some((_, element)) = self.element(element_id) {
            collect_resolved_ids(self, std::slice::from_ref(element), &mut ids);
        } else if let Some(location) = link_location(self, element_id) {
            let definition = self.linked.get(&location.definition);
            if let Some(definition) = definition {
                let mut node = &definition.master;
                for index in &location.path {
                    match node.children.get(*index) {
                        Some(child) => node = child,
                        None => return ids,
                    }
                }
                ids.insert(element_id);
                let mut path = Vec::new();
                collect_nested_ids(location.instance, node, &mut path, &mut ids);
            }
        }
        ids
    }

    /// The layer a generation step edits: a stored layer, or the definition behind a copy's
    /// nested layer. A copy's position, parent and request history stay with the copy.
    pub(crate) fn editable_element_mut(&mut self, id: Uuid) -> Option<&mut Element> {
        let location = link_location(self, id);
        match location {
            None => self.element_mut(id),
            Some(location) => {
                let definition = self.linked.get_mut(&location.definition)?;
                let mut node = &mut definition.master;
                for index in &location.path {
                    node = node.children.get_mut(*index)?;
                }
                Some(node)
            }
        }
    }

    pub(crate) fn remove_editable_element(&mut self, id: Uuid) -> bool {
        for page in &mut self.pages {
            if remove_element(&mut page.elements, id).is_some() {
                return true;
            }
        }
        let Some(location) = link_location(self, id) else {
            return false;
        };
        let Some((last, parent)) = location.path.split_last() else {
            return false;
        };
        let Some(definition) = self.linked.get_mut(&location.definition) else {
            return false;
        };
        let mut node = &mut definition.master;
        for index in parent {
            match node.children.get_mut(*index) {
                Some(child) => node = child,
                None => return false,
            }
        }
        if *last < node.children.len() {
            node.children.remove(*last);
            return true;
        }
        false
    }

    /// Add elements to a boundary. Adding in one size only (`size_id`) hides the new layer in
    /// every other size, so those sizes draw a placeholder instead of a sudden copy.
    pub(crate) fn insert_elements(
        &mut self,
        page_id: Uuid,
        parent_id: Option<Uuid>,
        index: usize,
        mut elements: Vec<Element>,
        picture_data_uri: Option<String>,
        size_id: Option<Uuid>,
    ) -> Result<()> {
        ensure(!elements.is_empty(), "Add at least one element.")?;
        if let Some(uri) = picture_data_uri {
            let picture = crate::project_assets::store_picture_uri(&uri)?;
            self.pictures
                .insert(picture, std::sync::Arc::from(uri.as_str()));
            for element in &mut elements {
                if matches!(element.kind, ElementKind::Image) && element.image.is_none() {
                    element.image = Some(picture);
                }
            }
        }
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .ok_or_else(|| Error("This page no longer exists.".into()))?;
        let children = match parent_id {
            None => &mut page.elements,
            Some(parent_id) => {
                let parent = find_element_mut(&mut page.elements, parent_id)
                    .ok_or_else(|| Error("This box no longer exists.".into()))?;
                ensure(
                    parent.kind.holds_children(),
                    "Only a box or a group can hold children.",
                )?;
                &mut parent.children
            }
        };
        let at = index.min(children.len());
        let inserted: Vec<Uuid> = elements.iter().map(|element| element.id).collect();
        children.splice(at..at, elements);
        if let Some(size_id) = size_id {
            let page = self
                .pages
                .iter_mut()
                .find(|page| page.id == page_id)
                .ok_or_else(|| Error("This page no longer exists.".into()))?;
            ensure(
                page.size(size_id).is_some(),
                "This size is not part of the boundary.",
            )?;
            let others: Vec<Uuid> = page
                .sizes
                .iter()
                .map(|size| size.id)
                .filter(|id| *id != size_id)
                .collect();
            for id in inserted {
                if let Some(element) = find_element_mut(&mut page.elements, id) {
                    for other in &others {
                        element.overrides.entry(*other).or_default().hidden = true;
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn update_elements(
        &mut self,
        page_id: Uuid,
        patches: Vec<ElementPatch>,
    ) -> Result<()> {
        ensure(!patches.is_empty(), "Choose a layer to change.")?;
        let page = self
            .pages
            .iter()
            .find(|page| page.id == page_id)
            .ok_or_else(|| Error("This page no longer exists.".into()))?;
        let targets: Vec<SizeTarget> = patches
            .iter()
            .map(|patch| size_target_for_patch(self, page, patch))
            .collect::<Result<_>>()?;
        for (mut patch, target) in patches.into_iter().zip(targets) {
            let mut picture = None;
            if let Some(uri) = patch.picture_data_uri.take() {
                let stored = crate::project_assets::store_picture_uri(&uri)?;
                self.pictures
                    .insert(stored, std::sync::Arc::from(uri.as_str()));
                picture = Some(stored);
            }
            if let Some(location) = link_location(self, patch.id) {
                self.apply_linked_patch(location, patch, picture, target)?;
                continue;
            }
            let linking = matches!(&patch.name, Some(Some(name)) if !name.trim().is_empty());
            let element_id = patch.id;
            let page = self
                .pages
                .iter_mut()
                .find(|page| page.id == page_id)
                .ok_or_else(|| Error("This page no longer exists.".into()))?;
            let element = find_element_mut(&mut page.elements, patch.id)
                .ok_or_else(|| Error("This layer no longer exists.".into()))?;
            if let Some(picture) = picture {
                ensure(
                    matches!(element.kind, ElementKind::Image),
                    "Only an image element uses a picture.",
                )?;
                element.image = Some(picture);
            }
            apply_sized_patch(element, patch, target)?;
            if linking {
                self.promote_to_linked(element_id)?;
            }
        }
        Ok(())
    }

    /// A patch on a copy: content changes the definition, position and visibility stay here.
    fn apply_linked_patch(
        &mut self,
        location: LinkLocation,
        mut patch: ElementPatch,
        picture: Option<Uuid>,
        target: SizeTarget,
    ) -> Result<()> {
        // Only a copy's own layer stores a per-size override; a layer inside the shared
        // definition edits every copy, so it keeps its values in the definition.
        let target = if location.path.is_empty() {
            target
        } else {
            SizeTarget::Base
        };
        if let SizeTarget::Size(size_id) = target {
            let (over, visible, dismissed) = sized_override(&mut patch)?;
            let owner = self
                .override_owner_mut(&location)
                .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
            let entry = owner.overrides.entry(size_id).or_default();
            merge_override(entry, &over, visible, dismissed);
            if entry.is_empty() {
                owner.overrides.remove(&size_id);
            }
        } else if target == SizeTarget::AllSizes {
            let cleared = cleared_fields(&patch);
            if let Some(owner) = self.override_owner_mut(&location) {
                for over in owner.overrides.values_mut() {
                    over.clear(cleared.0, cleared.1, cleared.2, cleared.3);
                }
                owner.overrides.retain(|_, over| !over.is_empty());
            }
        }
        if let Some(picture) = picture {
            let node = self.definition_node(&location)?;
            ensure(
                matches!(node.kind, ElementKind::Image),
                "Only an image element uses a picture.",
            )?;
            patch.image = Some(Some(picture));
        }
        if location.path.is_empty() {
            if let Some(name) = patch.name.take() {
                let name = name.map(|name| name.trim().to_string());
                if name.as_deref().is_none_or(str::is_empty) {
                    return self.detach_instance(location.instance);
                }
                let name = name.expect("a nonempty name");
                self.rename_definition(location.definition, &name)?;
                if let Some(element) = self.element_mut(location.instance) {
                    element.name = Some(name);
                }
            }
            if let Some(rect) = patch.rect.take() {
                validate_element_rect(&rect)?;
                let definition = self
                    .linked
                    .get_mut(&location.definition)
                    .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
                definition.master.rect.width = rect.width;
                definition.master.rect.height = rect.height;
                let instance = self
                    .element_mut(location.instance)
                    .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                instance.rect.x = rect.x;
                instance.rect.y = rect.y;
            }
            if let Some(visible) = patch.visible.take()
                && let Some(element) = self.element_mut(location.instance)
            {
                element.visible = visible;
            }
            if let Some(locked) = patch.locked.take()
                && let Some(element) = self.element_mut(location.instance)
            {
                element.locked = locked;
            }
        }
        let definition = self
            .linked
            .get_mut(&location.definition)
            .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
        let mut node = &mut definition.master;
        for index in &location.path {
            node = node
                .children
                .get_mut(*index)
                .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
        }
        patch.apply(node)?;
        Ok(())
    }

    /// The stored element a copy's size override belongs to: the copy itself for its root, the
    /// definition node for a layer inside it.
    fn override_owner_mut(&mut self, location: &LinkLocation) -> Option<&mut Element> {
        if location.path.is_empty() {
            return self.element_mut(location.instance);
        }
        let definition = self.linked.get_mut(&location.definition)?;
        let mut node = &mut definition.master;
        for index in &location.path {
            node = node.children.get_mut(*index)?;
        }
        Some(node)
    }

    /// The design one linked size shows, after linked-element resolution and size overrides.
    pub fn resolve_for_size(&self, page_id: Uuid, size_id: Uuid) -> Result<Vec<Element>> {
        let page = self.page(page_id)?;
        ensure(
            page.size(size_id).is_some(),
            "This size is not part of the boundary.",
        )?;
        Ok(resolve_for_size(self, page, size_id))
    }

    fn definition_node(&self, location: &LinkLocation) -> Result<&Element> {
        let definition = self
            .linked
            .get(&location.definition)
            .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
        let mut node = &definition.master;
        for index in &location.path {
            node = node
                .children
                .get(*index)
                .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
        }
        Ok(node)
    }

    /// Naming a layer makes it a linked element: its tree becomes the shared definition.
    fn promote_to_linked(&mut self, element_id: Uuid) -> Result<()> {
        let content = self
            .element(element_id)
            .map(|(_, element)| resolve_element(self, element))
            .ok_or_else(|| Error("This layer no longer exists.".into()))?;
        let name = content
            .name
            .clone()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| Error("Give this layer a name to link it.".into()))?;
        ensure(
            !self
                .linked
                .values()
                .any(|item| item.name.eq_ignore_ascii_case(&name)),
            "A linked element with this name already exists.",
        )?;
        let definition = Uuid::new_v4();
        self.linked.insert(
            definition,
            LinkedDefinition {
                id: definition,
                name: name.clone(),
                master: definition_master(&content),
            },
        );
        let element = self
            .element_mut(element_id)
            .ok_or_else(|| Error("This layer no longer exists.".into()))?;
        element.link = Some(definition);
        element.name = Some(name);
        element.children.clear();
        Ok(())
    }

    fn rename_definition(&mut self, definition: Uuid, name: &str) -> Result<()> {
        validate_text(name, "Linked element name", MAX_ELEMENT_NAME_CHARS, true)?;
        ensure(
            !self.linked.iter().any(|(other, item)| *other != definition
                && item.name.eq_ignore_ascii_case(name.trim())),
            "A linked element with this name already exists.",
        )?;
        let item = self
            .linked
            .get_mut(&definition)
            .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
        item.name = name.trim().into();
        Ok(())
    }

    fn unique_link_name(&self, base: Option<&str>) -> String {
        let taken = |name: &str| {
            self.linked
                .values()
                .any(|item| item.name.eq_ignore_ascii_case(name))
        };
        if let Some(base) = base.map(str::trim).filter(|base| !base.is_empty()) {
            let base: String = base.chars().take(MAX_ELEMENT_NAME_CHARS).collect();
            if !taken(&base) {
                return base;
            }
        }
        let mut number = 1;
        loop {
            let name = format!("Linked {number}");
            if !taken(&name) {
                return name;
            }
            number += 1;
        }
    }

    /// Detaches one copy into a plain layer with its current resolved content.
    fn detach_instance(&mut self, instance: Uuid) -> Result<()> {
        let mut concrete = self
            .element(instance)
            .map(|(_, element)| resolve_element(self, element))
            .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
        concrete.name = None;
        concrete.link = None;
        let element = self
            .element_mut(instance)
            .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
        *element = concrete;
        Ok(())
    }

    pub(crate) fn link_elements(&mut self, ids: Vec<Uuid>, source_id: Option<Uuid>) -> Result<()> {
        ensure(ids.len() >= 2, "Select at least two layers to link.")?;
        let mut unique = BTreeSet::new();
        ensure(
            ids.iter().all(|id| unique.insert(*id)),
            "A layer was selected twice.",
        )?;
        let mut contents = Vec::with_capacity(ids.len());
        for id in &ids {
            let (_, element) = self
                .element(*id)
                .ok_or_else(|| Error("These layers no longer exist.".into()))?;
            ensure(element.link.is_none(), "These layers are already linked.")?;
            contents.push((*id, resolve_element(self, element)));
        }
        let winner = source_id
            .and_then(|source| contents.iter().find(|(id, _)| *id == source))
            .unwrap_or(&contents[0])
            .1
            .clone();
        for (_, content) in &contents {
            ensure(
                same_element_structure(&winner, content),
                "Different structure, can't link"
            )?;
        }
        let name = self.unique_link_name(winner.name.as_deref());
        let definition = Uuid::new_v4();
        self.linked.insert(
            definition,
            LinkedDefinition {
                id: definition,
                name: name.clone(),
                master: definition_master(&winner),
            },
        );
        for (id, _) in &contents {
            if let Some(element) = self.element_mut(*id) {
                element.link = Some(definition);
                element.name = Some(name.clone());
                element.children.clear();
            }
        }
        Ok(())
    }

    pub(crate) fn unlink_elements(&mut self, ids: Vec<Uuid>) -> Result<()> {
        ensure(!ids.is_empty(), "Select a linked layer to unlink.")?;
        let mut instances: Vec<Uuid> = Vec::new();
        for id in &ids {
            if let Some(location) = link_location(self, *id)
                && !instances.contains(&location.instance)
            {
                instances.push(location.instance);
            }
        }
        ensure(!instances.is_empty(), "These layers are not linked.")?;
        for instance in instances {
            self.detach_instance(instance)?;
        }
        Ok(())
    }

    pub(crate) fn remove_linked_definition(&mut self, id: Uuid) -> Result<()> {
        ensure(
            self.linked.contains_key(&id),
            "This linked element no longer exists.",
        )?;
        ensure(
            !self
                .pages
                .iter()
                .any(|page| tree_has_link(&page.elements, id)),
            "This linked element is still used on the canvas.",
        )?;
        self.linked.remove(&id);
        Ok(())
    }

    pub(crate) fn paste_linked_instances(
        &mut self,
        items: Vec<LinkedPasteInstance>,
    ) -> Result<()> {
        ensure(!items.is_empty(), "Choose a layer to paste.")?;
        for item in items {
            ensure(!item.id.is_nil(), "A pasted layer needs an identifier.")?;
            ensure(
                self.element(item.id).is_none(),
                "A pasted layer already exists.",
            )?;
            let definition = match link_location(self, item.source_id) {
                Some(location) => location.definition,
                None => {
                    let content = self
                        .element(item.source_id)
                        .map(|(_, element)| resolve_element(self, element))
                        .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                    let name = self.unique_link_name(content.name.as_deref());
                    let definition = Uuid::new_v4();
                    self.linked.insert(
                        definition,
                        LinkedDefinition {
                            id: definition,
                            name: name.clone(),
                            master: definition_master(&content),
                        },
                    );
                    if let Some(element) = self.element_mut(item.source_id) {
                        element.link = Some(definition);
                        element.name = Some(name);
                        element.children.clear();
                    }
                    definition
                }
            };
            let mut instance = self
                .linked
                .get(&definition)
                .map(|item| item.master.clone())
                .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
            instance.id = item.id;
            instance.rect = item.rect;
            instance.link = Some(definition);
            instance.children.clear();
            instance.name = None;
            instance.meta = ElementMeta::default();
            self.insert_elements(
                item.page_id,
                item.parent_id,
                item.index,
                vec![instance],
                None,
                None,
            )?;
        }
        Ok(())
    }

    pub(crate) fn move_elements(&mut self, moves: Vec<ElementMove>) -> Result<()> {
        ensure(!moves.is_empty(), "Choose a layer to move.")?;
        let mut moving = BTreeSet::new();
        let mut nested = Vec::new();
        let mut targets = Vec::with_capacity(moves.len());
        for movement in &moves {
            ensure(
                moving.insert(movement.id),
                "A layer was moved twice in one step.",
            )?;
            let destination = self
                .pages
                .iter()
                .find(|page| page.id == movement.to_page_id)
                .ok_or_else(|| Error("The destination page no longer exists.".into()))?;
            let target = size_target(destination, movement.size_id, movement.all_sizes)?;
            targets.push(target);
            validate_element_rect(&movement.rect)?;
            let Some(element) = self.element(movement.id).map(|(_, element)| element) else {
                // A layer inside a linked copy moves in the definition; parent and order stay.
                let location = link_location(self, movement.id)
                    .filter(|location| !location.path.is_empty())
                    .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                let (index, parent_path) = location
                    .path
                    .split_last()
                    .expect("a nested layer has a parent path");
                ensure(
                    movement.index == *index,
                    "Reorder layers inside a linked element in one copy.",
                )?;
                if let Some(parent_id) = movement.to_parent_id {
                    let parent = link_location(self, parent_id)
                        .ok_or_else(|| Error("Move the box this layer sits in.".into()))?;
                    ensure(
                        parent.definition == location.definition
                            && parent.instance == location.instance
                            && parent.path == parent_path,
                        "A layer inside a linked element keeps its parent.",
                    )?;
                }
                nested.push((location, movement.rect, target));
                continue;
            };
            if let Some(parent_id) = movement.to_parent_id {
                let parent = self
                    .element(parent_id)
                    .map(|(_, element)| element)
                    .ok_or_else(|| Error("The destination box no longer exists.".into()))?;
                ensure(
                    parent.kind.holds_children(),
                    "Only a box or a group can hold children.",
                )?;
                let mut descendants = BTreeSet::new();
                collect_ids(&element.children, &mut descendants);
                ensure(
                    !descendants.contains(&parent_id) && parent_id != element.id,
                    "A layer cannot be moved into itself.",
                )?;
            }
        }
        for (location, rect, _target) in nested {
            let definition = self
                .linked
                .get_mut(&location.definition)
                .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
            let mut node = &mut definition.master;
            for index in &location.path {
                node = node
                    .children
                    .get_mut(*index)
                    .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
            }
            // A nested layer keeps its position in the shared definition.
            write_rect(node, rect, SizeTarget::Base);
        }
        let stored: Vec<(ElementMove, SizeTarget)> = moves
            .into_iter()
            .zip(targets)
            .filter(|(movement, _)| self.element(movement.id).is_some())
            .collect();
        if stored.is_empty() {
            return Ok(());
        }
        let mut removed = Vec::with_capacity(stored.len());
        for (movement, _) in &stored {
            let element = self
                .pages
                .iter_mut()
                .find_map(|page| remove_element(&mut page.elements, movement.id))
                .ok_or_else(|| Error("This layer no longer exists.".into()))?;
            removed.push(element);
        }
        for ((movement, target), mut element) in stored.into_iter().zip(removed) {
            write_rect(&mut element, movement.rect, target);
            let page = self
                .pages
                .iter_mut()
                .find(|page| page.id == movement.to_page_id)
                .ok_or_else(|| Error("The destination page no longer exists.".into()))?;
            let children = match movement.to_parent_id {
                None => &mut page.elements,
                Some(parent_id) => {
                    let parent = find_element_mut(&mut page.elements, parent_id).ok_or_else(|| {
                        Error("The destination box no longer exists.".into())
                    })?;
                    ensure(
                        parent.kind.holds_children(),
                        "Only a box or a group can hold children.",
                    )?;
                    &mut parent.children
                }
            };
            let at = movement.index.min(children.len());
            children.insert(at, element);
        }
        Ok(())
    }

    pub(crate) fn remove_elements(&mut self, page_id: Uuid, ids: Vec<Uuid>) -> Result<()> {
        ensure(!ids.is_empty(), "Choose a layer to remove.")?;
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .ok_or_else(|| Error("This page no longer exists.".into()))?;
        for id in &ids {
            ensure(
                find_element(&page.elements, *id).is_some(),
                "This layer no longer exists.",
            )?;
        }
        // Removing an ancestor removes its subtree; never remove the same element twice.
        let mut present = BTreeSet::new();
        for id in &ids {
            if find_element(&page.elements, *id).is_some() {
                present.insert(*id);
            }
        }
        remove_selected(&mut page.elements, &present);
        Ok(())
    }

    pub(crate) fn group_elements(&mut self, page_id: Uuid, ids: Vec<Uuid>) -> Result<()> {
        ensure(ids.len() >= 2, "Choose at least two layers to group.")?;
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .ok_or_else(|| Error("This page no longer exists.".into()))?;
        let mut unique = BTreeSet::new();
        for id in &ids {
            ensure(unique.insert(*id), "A layer was grouped twice.")?;
        }
        // Every selected layer must be a direct sibling.
        let mut parent: Option<Vec<usize>> = None;
        let mut positions = Vec::with_capacity(unique.len());
        for id in &unique {
            let path = path_to_element(&page.elements, *id)
                .ok_or_else(|| Error("These layers no longer exist.".into()))?;
            let (index, parent_path) = path.split_last().expect("an element path");
            match &parent {
                None => parent = Some(parent_path.to_vec()),
                Some(expected) if expected != parent_path => {
                    return Err(Error(
                        "Select layers with the same parent before grouping.".into(),
                    ));
                }
                Some(_) => {}
            }
            positions.push(*index);
        }
        positions.sort_unstable();
        let parent = parent.expect("selected layers have a parent");
        let top = *positions.last().expect("selected layers");
        let children = container_mut(&mut page.elements, &parent);
        let union = positions.iter().fold(None::<Rect>, |union, index| {
            let rect = children[*index].rect;
            Some(match union {
                None => rect,
                Some(union) => union_rect(&union, &rect),
            })
        })
        .expect("selected layers have a union");
        let mut grouped = Vec::with_capacity(positions.len());
        for index in &positions {
            grouped.push(children[*index].clone());
        }
        for index in positions.iter().rev() {
            children.remove(*index);
        }
        let mut group = group_element(union, grouped);
        for child in &mut group.children {
            child.rect.x -= union.x;
            child.rect.y -= union.y;
        }
        let at = top + 1 - group.children.len();
        children.insert(at.min(children.len()), group);
        Ok(())
    }

    pub(crate) fn ungroup_elements(&mut self, page_id: Uuid, id: Uuid) -> Result<()> {
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .ok_or_else(|| Error("This page no longer exists.".into()))?;
        let path = path_to_element(&page.elements, id)
            .ok_or_else(|| Error("This group no longer exists.".into()))?;
        let (index, parent_path) = path.split_last().expect("an element path");
        let children = container_mut(&mut page.elements, parent_path);
        ensure(
            matches!(children[*index].kind, ElementKind::Group),
            "Only a group can be ungrouped.",
        )?;
        let group = children.remove(*index);
        let origin = (group.rect.x, group.rect.y);
        let mut inner = group.children;
        for child in &mut inner {
            child.rect.x += origin.0;
            child.rect.y += origin.1;
            child.rotation += group.rotation;
        }
        children.splice(*index..*index, inner);
        Ok(())
    }

    pub(crate) fn update_element_context(
        &mut self,
        element_id: Uuid,
        context: Vec<RequestAttachment>,
    ) -> Result<()> {
        ensure(
            context.len() <= MAX_ELEMENT_CONTEXT,
            "A layer supports at most 32 context attachments.",
        )?;
        for attachment in &context {
            attachment.validate()?;
        }
        let element = self
            .element_mut(element_id)
            .ok_or_else(|| Error("This layer no longer exists.".into()))?;
        element.meta.context = context;
        Ok(())
    }
}

/// A patch changes only the fields it names; every other setting stays as it was.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementPatch {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ElementStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub image: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub url: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<ElementLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub name: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_type: Option<InputType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// A freshly picked picture; its bytes enter the project store in the same undo step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture_data_uri: Option<String>,
    /// The linked size this edit belongs to. The base size edits base values; any other size
    /// stores the field as an override, so the other sizes keep what they inherit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_id: Option<Uuid>,
    /// Double-click editing: change base values and clear the same fields from every override.
    #[serde(default)]
    pub all_sizes: bool,
    /// Keeps a layer hidden in one size without a placeholder (Dismiss).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed: Option<bool>,
}

impl ElementPatch {
    fn apply(self, element: &mut Element) -> Result<()> {
        if let Some(rect) = self.rect {
            validate_element_rect(&rect)?;
            element.rect = rect;
        }
        if let Some(rotation) = self.rotation {
            ensure(
                rotation.is_finite() && rotation.abs() <= 360_000.0,
                "An element rotation must be finite.",
            )?;
            element.rotation = rotation;
        }
        if let Some(style) = self.style {
            style.validate()?;
            element.style = style;
        }
        if let Some(text) = self.text {
            ensure(
                text.chars().count() <= MAX_PROMPT_CHARS && !text.contains('\0'),
                "Element text is limited to 16,384 characters.",
            )?;
            element.text = text;
        }
        if let Some(image) = self.image {
            ensure(
                matches!(element.kind, ElementKind::Image),
                "Only an image element uses a picture.",
            )?;
            element.image = image;
        }
        if let Some(url) = self.url {
            if let Some(url) = &url {
                validate_http_url(url)?;
            }
            element.url = url;
        }
        if let Some(layout) = self.layout {
            ensure(
                element.kind.holds_children(),
                "Only a box lays out its children.",
            )?;
            layout.validate()?;
            element.layout = layout;
        }
        if let Some(visible) = self.visible {
            element.visible = visible;
        }
        if let Some(locked) = self.locked {
            element.locked = locked;
        }
        if let Some(name) = self.name {
            if let Some(name) = &name {
                validate_text(name, "Layer name", MAX_ELEMENT_NAME_CHARS, false)?;
            }
            element.name = name;
        }
        if let Some(input_type) = self.input_type {
            match &mut element.kind {
                ElementKind::Input { input_type: value } => *value = input_type,
                _ => return Err(Error("Only an input has an input type.".into())),
            }
        }
        if let Some(options) = self.options {
            match &mut element.kind {
                ElementKind::Select { options: value } => *value = options,
                _ => return Err(Error("Only a select has options.".into())),
            }
        }
        if let Some(language) = self.language {
            match &mut element.kind {
                ElementKind::Code { language: value, .. } => {
                    validate_text(&language, "Code language", 40, false)?;
                    *value = language;
                }
                _ => return Err(Error("Only a code block has a language.".into())),
            }
        }
        if let Some(source) = self.source {
            match &mut element.kind {
                ElementKind::Code { source: value, .. } => {
                    ensure(
                        source.chars().count() <= MAX_ELEMENT_CODE_CHARS
                            && !source.contains('\0'),
                        "A code block is limited to 200,000 characters.",
                    )?;
                    *value = source;
                }
                _ => return Err(Error("Only a code block has source.".into())),
            }
        }
        Ok(())
    }
}

/// Where an element edit lands. The base size edits base values; any other size stores the
/// field as an override; double-click editing changes base values and clears the overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SizeTarget {
    Base,
    AllSizes,
    Size(Uuid),
}

pub(crate) fn size_target(
    page: &Page,
    size_id: Option<Uuid>,
    all_sizes: bool,
) -> Result<SizeTarget> {
    if all_sizes {
        return Ok(SizeTarget::AllSizes);
    }
    match size_id {
        None => Ok(SizeTarget::Base),
        Some(id) if page.base_size_id() == id => Ok(SizeTarget::Base),
        Some(id) if page.size(id).is_some() => Ok(SizeTarget::Size(id)),
        Some(_) => Err(Error("This size is not part of the boundary.".into())),
    }
}

/// Where a patch lands, with one exception: a placeholder hides a layer in a size with an
/// override. Include and dismiss are edits to that override even when it belongs to the base
/// size, whose other values stay inherited.
pub(crate) fn size_target_for_patch(
    project: &Project,
    page: &Page,
    patch: &ElementPatch,
) -> Result<SizeTarget> {
    let target = size_target(page, patch.size_id, patch.all_sizes)?;
    if target != SizeTarget::Base || (patch.visible.is_none() && patch.dismissed.is_none()) {
        return Ok(target);
    }
    let Some(size_id) = patch.size_id else {
        return Ok(target);
    };
    let has_override = project
        .element(patch.id)
        .is_some_and(|(_, element)| element.overrides.contains_key(&size_id))
        || link_location(project, patch.id).is_some_and(|location| {
            location.path.is_empty()
                && project
                    .element(location.instance)
                    .is_some_and(|(_, element)| element.overrides.contains_key(&size_id))
        });
    Ok(if has_override {
        SizeTarget::Size(size_id)
    } else {
        target
    })
}

/// The fields a double-click edit changes in base values and clears from every size override.
fn cleared_fields(patch: &ElementPatch) -> (bool, bool, bool, bool) {
    (
        patch.rect.is_some(),
        patch.style.is_some(),
        patch.layout.is_some(),
        patch.visible.is_some() || patch.dismissed.is_some(),
    )
}

/// The part of a single-size patch that becomes an override, and the visibility flags it sets.
fn sized_override(
    patch: &mut ElementPatch,
) -> Result<(ElementOverride, Option<bool>, Option<bool>)> {
    let mut over = ElementOverride::default();
    if let Some(rect) = patch.rect.take() {
        validate_element_rect(&rect)?;
        over.rect = Some(rect);
    }
    if let Some(style) = patch.style.take() {
        style.validate()?;
        over.style = Some(style);
    }
    if let Some(layout) = patch.layout.take() {
        layout.validate()?;
        over.layout = Some(layout);
    }
    Ok((over, patch.visible.take(), patch.dismissed.take()))
}

fn merge_override(
    entry: &mut ElementOverride,
    over: &ElementOverride,
    visible: Option<bool>,
    dismissed: Option<bool>,
) {
    if over.rect.is_some() {
        entry.rect = over.rect;
    }
    if over.style.is_some() {
        entry.style = over.style.clone();
    }
    if over.layout.is_some() {
        entry.layout = over.layout.clone();
    }
    if let Some(visible) = visible {
        entry.hidden = !visible;
        if visible {
            entry.dismissed = false;
        }
    }
    if let Some(dismissed) = dismissed {
        entry.dismissed = dismissed;
        if dismissed {
            entry.hidden = true;
        }
    }
}

/// A single-size patch changes that size's override; everything shared (text, kind, name,
/// children) still lands in the base model.
pub(crate) fn apply_sized_patch(
    element: &mut Element,
    mut patch: ElementPatch,
    target: SizeTarget,
) -> Result<()> {
    match target {
        SizeTarget::Base => {
            ensure(
                patch.dismissed.is_none(),
                "Only a size can dismiss a layer.",
            )?;
            patch.apply(element)
        }
        SizeTarget::Size(size_id) => {
            let (over, visible, dismissed) = sized_override(&mut patch)?;
            let entry = element.overrides.entry(size_id).or_default();
            merge_override(entry, &over, visible, dismissed);
            if entry.is_empty() {
                element.overrides.remove(&size_id);
            }
            patch.apply(element)
        }
        SizeTarget::AllSizes => {
            let (rect, style, layout, visible) = cleared_fields(&patch);
            patch.apply(element)?;
            let mut empty = Vec::new();
            for (size_id, over) in element.overrides.iter_mut() {
                over.clear(rect, style, layout, visible);
                if over.is_empty() {
                    empty.push(*size_id);
                }
            }
            for size_id in empty {
                element.overrides.remove(&size_id);
            }
            Ok(())
        }
    }
}

fn write_rect(element: &mut Element, rect: Rect, target: SizeTarget) {
    match target {
        SizeTarget::Size(size_id) => {
            element.overrides.entry(size_id).or_default().rect = Some(rect);
        }
        SizeTarget::AllSizes => {
            element.rect = rect;
            for over in element.overrides.values_mut() {
                over.rect = None;
            }
            element.overrides.retain(|_, over| !over.is_empty());
        }
        SizeTarget::Base => element.rect = rect,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementMove {
    pub id: Uuid,
    pub to_page_id: Uuid,
    pub to_parent_id: Option<Uuid>,
    pub index: usize,
    pub rect: Rect,
    /// The linked size the move happened in; a non-base size stores the position as an override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_id: Option<Uuid>,
    /// Double-click editing: move in base values and clear every size's position override.
    #[serde(default)]
    pub all_sizes: bool,
}

/// A named optional field can be cleared: missing keeps it, null clears it.
fn double_option<'de, D, T>(deserializer: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn union_rect(first: &Rect, second: &Rect) -> Rect {
    let x = first.x.min(second.x);
    let y = first.y.min(second.y);
    Rect {
        x,
        y,
        width: (first.x + first.width).max(second.x + second.width) - x,
        height: (first.y + first.height).max(second.y + second.height) - y,
    }
}

/// An element with the neutral visuals every new layer starts from.
pub fn empty_element(kind: ElementKind, rect: Rect) -> Element {
    Element {
        id: Uuid::new_v4(),
        kind,
        name: None,
        link: None,
        rect,
        rotation: 0.0,
        visible: true,
        locked: false,
        style: ElementStyle {
            fill: None,
            stroke: ElementStroke {
                color: None,
                width: 0.0,
            },
            radius: 0.0,
            opacity: 1.0,
            shadow: ElementShadow {
                x: 0.0,
                y: 0.0,
                blur: 0.0,
                color: None,
            },
            text_style: ElementTextStyle {
                font_family: "system-ui".into(),
                size: 16.0,
                weight: 400,
                italic: false,
                line_height: 1.4,
                letter_spacing: 0.0,
                color: "#1a1a1a".into(),
                align: TextAlign::Left,
            },
        },
        text: String::new(),
        image: None,
        url: None,
        layout: ElementLayout::Free,
        overrides: BTreeMap::new(),
        children: vec![],
        meta: ElementMeta::default(),
    }
}

/// Group visuals are empty: no fill, no stroke, no shadow, no text.
fn group_element(rect: Rect, children: Vec<Element>) -> Element {
    Element {
        children,
        ..empty_element(ElementKind::Group, rect)
    }
}

fn path_to_element(elements: &[Element], id: Uuid) -> Option<Vec<usize>> {
    for (index, element) in elements.iter().enumerate() {
        if element.id == id {
            return Some(vec![index]);
        }
        if let Some(mut path) = path_to_element(&element.children, id) {
            path.insert(0, index);
            return Some(path);
        }
    }
    None
}

fn container_mut<'a>(elements: &'a mut Vec<Element>, path: &[usize]) -> &'a mut Vec<Element> {
    match path.split_first() {
        None => elements,
        Some((index, rest)) => container_mut(&mut elements[*index].children, rest),
    }
}

fn remove_selected(elements: &mut Vec<Element>, ids: &BTreeSet<Uuid>) {
    elements.retain(|element| !ids.contains(&element.id));
    for element in elements.iter_mut() {
        remove_selected(&mut element.children, ids);
    }
}


fn count_pending(elements: &[Element]) -> usize {
    elements
        .iter()
        .map(|element| {
            usize::from(
                element
                    .meta
                    .pending
                    .as_ref()
                    .is_some_and(|pending| !pending.prompt.trim().is_empty()),
            ) + count_pending(&element.children)
        })
        .sum()
}

/// A compact subtree for the AI: kinds, rects, styles, text, layout and children, without
/// request history, context or pending requests.
pub fn element_subtree(element: &Element) -> serde_json::Value {
    serde_json::json!({
        "id": element.id,
        "kind": element.kind,
        "name": element.name,
        "link": element.link,
        "rect": element.rect,
        "rotation": element.rotation,
        "visible": element.visible,
        "locked": element.locked,
        "style": element.style,
        "text": element.text,
        "image": element.image,
        "url": element.url,
        "layout": element.layout,
        "children": element.children.iter().map(element_subtree).collect::<Vec<_>>(),
    })
}

/// A compact outline of one boundary's elements, used to describe the design to the AI.
pub fn element_outline(elements: &[Element]) -> Vec<serde_json::Value> {
    fn walk(elements: &[Element], depth: usize) -> Vec<serde_json::Value> {
        let mut rows = Vec::new();
        for element in elements {
            rows.push(serde_json::json!({
                "id": element.id,
                "kind": element.kind.label(),
                "label": element.label(),
                "rect": element.rect,
                "depth": depth,
            }));
            if depth < 32 {
                rows.extend(walk(&element.children, depth + 1));
            }
        }
        rows
    }
    walk(elements, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_accept_hex_and_transparent() {
        assert!(validate_colour("#abc", "Fill").is_ok());
        assert!(validate_colour("#aabbcc", "Fill").is_ok());
        assert!(validate_colour("#aabbccdd", "Fill").is_ok());
        assert!(validate_colour("transparent", "Fill").is_ok());
        assert!(validate_colour("red", "Fill").is_err());
    }
}
