//! Visual elements: the things drawn on the canvas. Elements hold visual properties only;
//! code appears at export. Every page boundary owns a back-to-front tree of root elements.
use crate::limits::{
    LINE_HEIGHT_MAX, MAX_ELEMENT_CODE_CHARS, MAX_ELEMENT_CONTEXT, MAX_ELEMENT_DEPTH,
    MAX_ELEMENT_NAME_CHARS, MAX_ELEMENT_OPTIONS, MAX_ELEMENT_REQUESTS, MAX_ELEMENTS,
    MAX_FONT_FAMILY_CHARS, MAX_FONT_WEIGHT, MAX_GRID_COLUMNS, MAX_LAYOUT_SPACING,
    MAX_LETTER_SPACING, MAX_RADIUS, MAX_SHADOW_BLUR, MAX_SHADOW_OFFSET, MAX_STROKE_WIDTH,
    TEXT_SIZE_RANGE,
};
use crate::{
    Error, Result, attachments::RequestAttachment, limits::MAX_PROMPT_CHARS, model::Page,
    model::Project, model::Rect, validation::ensure, validation::validate_text,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StackDirection {
    Row,
    Column,
}

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StackAlign {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StackJustify {
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(ts_rs::TS, Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// CSS text decoration, shared by canvas and exports. Missing means undecorated.
#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TextDecoration {
    None,
    Underline,
    LineThrough,
}

/// What a visual element is. Kind-specific settings live on the variant, so an input always
/// carries its type, a select its options and a code block its language and source.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    Input {
        input_type: InputType,
    },
    Textarea,
    Select {
        options: Vec<String>,
    },
    Checkbox,
    Radio,
    Video,
    Audio,
    Embed,
    Code {
        language: String,
        source: String,
    },
}

/// The name new layers of a kind are numbered from; a box reads as a rectangle.
pub fn default_layer_name_base(kind: &ElementKind) -> &'static str {
    match kind {
        ElementKind::Box => "Rectangle",
        kind => kind.label(),
    }
}

/// The placeholder text a kind starts with; a layer still showing it reads by its name.
pub fn default_element_text(kind: &ElementKind) -> &'static str {
    match kind {
        ElementKind::Text => "Text",
        ElementKind::Button => "Button",
        ElementKind::Link => "Link",
        ElementKind::Input { .. } => "Input",
        ElementKind::Textarea => "Textarea",
        ElementKind::Select { .. } => "Option 1",
        ElementKind::Checkbox => "Checkbox",
        ElementKind::Radio => "Radio",
        _ => "",
    }
}

pub fn default_element_size(kind: &ElementKind) -> (u32, u32) {
    match kind {
        ElementKind::Ellipse => (160, 120),
        ElementKind::Line => (160, 0),
        ElementKind::Button => (160, 44),
        ElementKind::Link => (120, 28),
        ElementKind::Input { .. } | ElementKind::Select { .. } => (240, 40),
        ElementKind::Textarea => (240, 100),
        ElementKind::Checkbox | ElementKind::Radio => (160, 24),
        ElementKind::Video | ElementKind::Embed => (320, 180),
        ElementKind::Audio => (320, 64),
        ElementKind::Code { .. } => (360, 160),
        ElementKind::Image => (320, 200),
        _ => (240, 160),
    }
}

/// True when a name is the numbered default new layers receive, not one the user chose.
pub fn is_default_layer_name(element: &Element) -> bool {
    let Some(name) = element.name.as_deref().map(str::trim) else {
        return false;
    };
    name.strip_prefix(default_layer_name_base(&element.kind))
        .and_then(|rest| rest.strip_prefix(' '))
        .is_some_and(|number| {
            !number.is_empty()
                && !number.starts_with('0')
                && number.chars().all(|character| character.is_ascii_digit())
        })
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

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementStroke {
    /// `None` draws no stroke.
    pub color: Option<String>,
    pub width: f64,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementShadow {
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    /// `None` draws no shadow.
    pub color: Option<String>,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoration: Option<TextDecoration>,
}

/// The text style a new layer starts with; exported CSS resets every layer to these values so a
/// layer only has to state the text properties that differ.
impl Default for ElementTextStyle {
    fn default() -> Self {
        Self {
            font_family: "system-ui".into(),
            size: 16.0,
            weight: 400,
            italic: false,
            line_height: 1.4,
            letter_spacing: 0.0,
            color: "#1a1a1a".into(),
            align: TextAlign::Left,
            decoration: None,
        }
    }
}

#[derive(ts_rs::TS, Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SizingMode {
    #[default]
    Fixed,
    Fit,
    Fill,
}

#[derive(ts_rs::TS, Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[ts(optional_fields = nullable)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementSizing {
    #[serde(default)]
    pub width: SizingMode,
    #[serde(default)]
    pub height: SizingMode,
    pub min_width: Option<f64>,
    pub max_width: Option<f64>,
    pub min_height: Option<f64>,
    pub max_height: Option<f64>,
}
impl ElementSizing {
    pub fn constrain(&self, width: f64, height: f64) -> (f64, f64) {
        (
            width
                .max(self.min_width.unwrap_or(0.0))
                .min(self.max_width.unwrap_or(f64::MAX)),
            height
                .max(self.min_height.unwrap_or(0.0))
                .min(self.max_height.unwrap_or(f64::MAX)),
        )
    }
    pub(crate) fn validate(&self) -> Result<()> {
        for (min, max) in [
            (self.min_width, self.max_width),
            (self.min_height, self.max_height),
        ] {
            ensure(
                [min, max]
                    .into_iter()
                    .flatten()
                    .all(|v| v.is_finite() && (0.0..=40_000.0).contains(&v)),
                "Size limits must be between 0 and 40,000 pixels.",
            )?;
            ensure(
                min.zip(max).is_none_or(|(min, max)| min <= max),
                "Minimum size cannot exceed maximum size.",
            )?;
        }
        Ok(())
    }
}
#[derive(ts_rs::TS, Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[ts(optional_fields = nullable)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StyleReferences {
    pub fill: Option<Uuid>,
    pub stroke: Option<Uuid>,
    pub text_colour: Option<Uuid>,
    pub text: Option<Uuid>,
}

/// Content inset for a stack or grid box, one value per side. The layout keeps its own
/// all-sides `padding` as the fallback for files and renderers that predate per-side insets.
#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementPadding {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl ElementPadding {
    pub fn validate(&self) -> Result<()> {
        ensure(
            [self.top, self.right, self.bottom, self.left]
                .iter()
                .all(|value| value.is_finite() && (0.0..=MAX_LAYOUT_SPACING).contains(value)),
            "Padding must be between 0 and 4,096 pixels on every side.",
        )
    }
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementStyle {
    #[serde(default)]
    #[ts(optional = nullable)]
    pub flip_x: bool,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub flip_y: bool,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub sizing: ElementSizing,
    /// Stack or grid padding per side; `None` uses the layout's all-sides padding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<ElementPadding>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub references: StyleReferences,
    /// `None` fills nothing.
    pub fill: Option<String>,
    pub stroke: ElementStroke,
    pub radius: f64,
    pub opacity: f64,
    pub shadow: ElementShadow,
    pub text_style: ElementTextStyle,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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

    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Free => Ok(()),
            Self::Stack { gap, padding, .. } => {
                ensure(
                    gap.is_finite() && (0.0..=MAX_LAYOUT_SPACING).contains(gap),
                    "A stack gap must be between 0 and 4,096 pixels.",
                )?;
                ensure(
                    padding.is_finite() && (0.0..=MAX_LAYOUT_SPACING).contains(padding),
                    "Stack padding must be between 0 and 4,096 pixels.",
                )
            }
            Self::Grid {
                columns,
                gap,
                padding,
            } => {
                ensure(
                    (1..=MAX_GRID_COLUMNS).contains(columns),
                    "A grid needs between 1 and 64 columns.",
                )?;
                ensure(
                    gap.is_finite() && (0.0..=MAX_LAYOUT_SPACING).contains(gap),
                    "A grid gap must be between 0 and 4,096 pixels.",
                )?;
                ensure(
                    padding.is_finite() && (0.0..=MAX_LAYOUT_SPACING).contains(padding),
                    "Grid padding must be between 0 and 4,096 pixels.",
                )
            }
        }
    }
}

/// One past request on a layer. Outcome and job arrive with the AI step; until then a manual
/// Ask AI records the prompt and leaves them empty.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
            self.attachments.len() <= crate::limits::MAX_REQUEST_ATTACHMENTS,
            "A request supports at most 32 attachments.",
        )?;
        for attachment in &self.attachments {
            attachment.validate()?;
        }
        Ok(())
    }
}

#[derive(ts_rs::TS, Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementMeta {
    /// Newest last on disk; Properties lists them newest first.
    pub requests: Vec<RequestRecord>,
    pub context: Vec<RequestAttachment>,
}

impl ElementMeta {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure(
            self.requests.len() <= MAX_ELEMENT_REQUESTS,
            format!("A layer keeps at most {MAX_ELEMENT_REQUESTS} past requests."),
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
        Ok(())
    }
}

/// What one linked size changes about an element. Missing fields inherit the base values, so a
/// size only stores what it overrides. `hidden` removes the element from that size and draws a
/// placeholder where it would go; `dismissed` keeps it hidden without the placeholder.
#[derive(ts_rs::TS, Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementOverride {
    /// True only for overrides owned by deterministic responsive adaptation.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub automatic: bool,
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

    pub(crate) fn validate(&self, element: &Element) -> Result<()> {
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

    pub(crate) fn clear(&mut self, rect: bool, style: bool, layout: bool, visible: bool) {
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

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Element {
    pub id: Uuid,
    /// Flat kind and its kind-specific fields, matching the editor's element type.
    #[serde(flatten)]
    pub kind: ElementKind,
    /// Set to name a layer.
    #[serde(deserialize_with = "crate::validation::required_nullable")]
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
    #[serde(default)]
    #[ts(optional = nullable)]
    pub overrides: BTreeMap<Uuid, ElementOverride>,
    pub children: Vec<Element>,
    pub meta: ElementMeta,
}

impl Element {
    pub fn validate(&self, project: &Project) -> Result<()> {
        ensure(!self.id.is_nil(), "An element needs a nonempty identifier.")?;
        validate_element_rect(&self.rect)?;
        ensure(
            self.rotation.is_finite() && self.rotation.abs() <= 360_000.0,
            "An element rotation must be finite.",
        )?;
        if let Some(name) = &self.name {
            validate_text(name, "Layer name", MAX_ELEMENT_NAME_CHARS, false)?;
        }
        for over in self.overrides.values() {
            over.validate(self)?;
            if let Some(style) = &over.style {
                project.styles.validate_references(&style.references)?;
            }
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
        project.styles.validate_references(&self.style.references)?;
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
                validate_element_url(url)?;
            }
            ElementKind::Video | ElementKind::Audio => {
                let url = self
                    .url
                    .as_deref()
                    .ok_or_else(|| Error("This element needs a web address.".into()))?;
                crate::limits::validate_http_url(url)?;
            }
            ElementKind::Embed => {
                let url = self.url.as_deref().ok_or_else(|| {
                    Error("This element needs a web address or page path.".into())
                })?;
                // A frame shows an external address or another boundary of this project.
                validate_element_url(url)?;
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
                    source.chars().count() <= MAX_ELEMENT_CODE_CHARS && !source.contains('\0'),
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
            ensure(self.layout.is_free(), "Only a box lays out its children.")?;
        }
        if let Some(url) = &self.url {
            validate_element_url(url)?;
        }
        if let ElementKind::Select { options } = &self.kind {
            ensure(
                (1..=MAX_ELEMENT_OPTIONS).contains(&options.len()),
                format!("A select needs between 1 and {MAX_ELEMENT_OPTIONS} options."),
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
        let name = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty());
        if let Some(name) = name
            && !is_default_layer_name(self)
        {
            return name.to_string();
        }
        // A layer still showing its kind's placeholder text reads by its numbered name; once the
        // text is its own, text layers and controls show that text, as they always have.
        let line = self.text.trim().split('\n').next().unwrap_or("");
        if !line.is_empty() && line != default_element_text(&self.kind) {
            return line
                .chars()
                .take(crate::limits::MAX_LAYER_LABEL_CHARS)
                .collect();
        }
        if let ElementKind::Code { language, .. } = &self.kind {
            return format!("{language} code");
        }
        match name {
            Some(name) => name.to_string(),
            None if !line.is_empty() => line
                .chars()
                .take(crate::limits::MAX_LAYER_LABEL_CHARS)
                .collect(),
            None => self.kind.label().into(),
        }
    }
}

impl ElementStyle {
    pub(crate) fn validate(&self) -> Result<()> {
        self.sizing.validate()?;
        if let Some(padding) = &self.padding {
            padding.validate()?;
        }
        if let Some(fill) = &self.fill {
            validate_colour(fill, "Fill")?;
        }
        if let Some(colour) = &self.stroke.color {
            validate_colour(colour, "Stroke")?;
        }
        ensure(
            self.stroke.width.is_finite() && (0.0..=MAX_STROKE_WIDTH).contains(&self.stroke.width),
            "A stroke width must be between 0 and 256 pixels.",
        )?;
        ensure(
            self.radius.is_finite() && (0.0..=MAX_RADIUS).contains(&self.radius),
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
                && shadow.blur <= MAX_SHADOW_BLUR
                && shadow.x.abs() <= MAX_SHADOW_OFFSET
                && shadow.y.abs() <= MAX_SHADOW_OFFSET,
            "A shadow must use finite offsets and a blur between 0 and 4,096 pixels.",
        )?;
        if let Some(colour) = &shadow.color {
            validate_colour(colour, "Shadow")?;
        }
        let text = &self.text_style;
        validate_text(
            &text.font_family,
            "Font family",
            MAX_FONT_FAMILY_CHARS,
            false,
        )?;
        ensure(
            text.size.is_finite() && (TEXT_SIZE_RANGE.0..=TEXT_SIZE_RANGE.1).contains(&text.size),
            format!(
                "Text size must be between {} and {} pixels.",
                TEXT_SIZE_RANGE.0, TEXT_SIZE_RANGE.1
            ),
        )?;
        ensure(
            (1..=MAX_FONT_WEIGHT).contains(&text.weight),
            format!("Text weight must be between 1 and {MAX_FONT_WEIGHT}."),
        )?;
        ensure(
            text.line_height.is_finite() && (0.0..=LINE_HEIGHT_MAX).contains(&text.line_height),
            format!("Line height must be between 0 and {LINE_HEIGHT_MAX}."),
        )?;
        ensure(
            text.letter_spacing.is_finite() && text.letter_spacing.abs() <= MAX_LETTER_SPACING,
            format!(
                "Letter spacing must be between -{MAX_LETTER_SPACING} and {MAX_LETTER_SPACING} pixels."
            ),
        )?;
        validate_colour(&text.color, "Text colour")?;
        Ok(())
    }
}

pub(crate) fn validate_element_rect(rect: &Rect) -> Result<()> {
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

/// A hyperlink target: an external web address, or a path to a boundary in this project.
pub(crate) fn validate_element_url(url: &str) -> Result<()> {
    if url.starts_with('/') {
        crate::model::validate_path(url)
    } else {
        crate::limits::validate_http_url(url)
    }
}

pub(crate) fn validate_colour(value: &str, label: &str) -> Result<()> {
    ensure(
        crate::limits::is_hex_colour(value) || value.eq_ignore_ascii_case("transparent"),
        format!("{label} must be a #rgb, #rrggbb or #rrggbbaa colour."),
    )
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

pub(crate) fn validate_element(
    element: &Element,
    project: &Project,
    sizes: &BTreeSet<Uuid>,
    ids: &mut BTreeSet<Uuid>,
    count: &mut usize,
    depth: usize,
) -> Result<()> {
    ensure(
        depth <= MAX_ELEMENT_DEPTH,
        format!(
            "Elements cannot be nested more than {} deep.",
            crate::limits::MAX_ELEMENT_DEPTH
        ),
    )?;
    *count += 1;
    ensure(
        *count <= MAX_ELEMENTS,
        format!(
            "A project holds at most {} elements.",
            crate::limits::MAX_ELEMENTS
        ),
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

/// One named element every copy shares. The master holds no position: its root sits at the
/// origin and children keep rects relative to their parent.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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

pub(crate) fn find_element(elements: &[Element], id: Uuid) -> Option<&Element> {
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

pub(crate) fn find_element_mut(elements: &mut [Element], id: Uuid) -> Option<&mut Element> {
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

pub(crate) fn collect_ids(elements: &[Element], ids: &mut BTreeSet<Uuid>) {
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

pub(crate) fn resolve_nested_instances(project: &Project, elements: &mut [Element]) {
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
    resolve_shared_styles(project, &mut elements);
    elements
}

/// Resolve tokens after size overrides so all outputs use the same values.
pub fn resolve_shared_styles(project: &Project, elements: &mut [Element]) {
    for element in elements {
        project.styles.resolve(&mut element.style);
        resolve_shared_styles(project, &mut element.children);
    }
}

pub(crate) fn apply_size_overrides(elements: &mut [Element], size_id: Uuid) {
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
pub(crate) fn resolved_instance(instance: &Element, definition: &LinkedDefinition) -> Element {
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
    // The copy keeps its own name; the shared definition names the linked group elsewhere.
    concrete.name = instance.name.clone();
    let mut path = Vec::new();
    assign_nested_ids(instance.id, &mut concrete, &mut path);
    concrete
}

pub(crate) fn assign_nested_ids(root: Uuid, element: &mut Element, path: &mut Vec<usize>) {
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

pub(crate) fn nested_path_hash(path: &[usize]) -> u32 {
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
pub(crate) fn nested_link_location(
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

pub(crate) fn path_to_nested_id(master: &Element, root: Uuid, target: Uuid) -> Option<Vec<usize>> {
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
pub(crate) fn same_element_structure(first: &Element, second: &Element) -> bool {
    first.kind.label() == second.kind.label()
        && first.children.len() == second.children.len()
        && first
            .children
            .iter()
            .zip(&second.children)
            .all(|(left, right)| same_element_structure(left, right))
}

/// The shared tree for a new definition: no position, no request history, no link of its own.
pub(crate) fn definition_master(content: &Element) -> Element {
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

pub(crate) fn collect_link_instances(elements: &[Element], link: Uuid, ids: &mut Vec<Uuid>) {
    for element in elements {
        if element.link == Some(link) {
            ids.push(element.id);
        }
        collect_link_instances(&element.children, link, ids);
    }
}

pub(crate) fn tree_has_link(elements: &[Element], link: Uuid) -> bool {
    elements
        .iter()
        .any(|element| element.link == Some(link) || tree_has_link(&element.children, link))
}

pub(crate) fn collect_resolved_ids(
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

pub(crate) fn collect_nested_ids(
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkedPasteInstance {
    pub id: Uuid,
    pub source_id: Uuid,
    pub page_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub index: usize,
    pub rect: Rect,
}
