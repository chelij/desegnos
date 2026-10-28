use crate::{
    Element, ElementMove, ElementPatch, Error, GenerationDossier, LinkedDefinition,
    LinkedPasteInstance, MAX_APPLIED_JOBS, RequestAttachment, Result, ensure,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 14;
/// The longest export destination the project remembers.
pub const MAX_EXPORT_DESTINATION: usize = 4_096;
pub const MAX_PAGES: usize = 100;
pub const MAX_PAGE_SIZES: usize = 8;
pub const MAX_PROMPT_CHARS: usize = 16_384;
pub const MAX_REFERENCE_ITEMS: usize = 1000;
pub const MAX_POINTERS: usize = 1000;
pub const MAX_REQUESTS: usize = 256;
pub const MAX_REQUEST_TARGETS: usize = 64;
pub const MAX_NAMED_VIEWS: usize = 50;
pub const MAX_NAMED_VIEW_NAME_CHARS: usize = 80;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GenerationMode {
    Fuzzy,
    Strict,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GridUnit {
    Pixels,
    Percent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GridSettings {
    pub spacing: f64,
    pub unit: GridUnit,
    pub snap: bool,
    /// Legacy ruler switch. The editor has no ruler UI; the field stays on the wire so
    /// projects written by earlier versions keep loading.
    #[serde(default = "default_rulers")]
    pub rulers: bool,
}

fn default_rulers() -> bool {
    true
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            spacing: 8.0,
            unit: GridUnit::Pixels,
            snap: true,
            rulers: true,
        }
    }
}

impl GridSettings {
    pub fn validate(&self) -> Result<()> {
        let valid = match self.unit {
            GridUnit::Pixels => (1.0..=512.0).contains(&self.spacing),
            GridUnit::Percent => (0.25..=100.0).contains(&self.spacing),
        };
        ensure(
            self.spacing.is_finite() && valid,
            "Grid spacing must be between 1 and 512 pixels, or 0.25 and 100 percent.",
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub fn validate(&self) -> Result<()> {
        ensure(
            (1..=100_000).contains(&self.width) && (1..=100_000).contains(&self.height),
            "Viewport dimensions must be between 1 and 100,000 pixels wide and high.",
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn validate(&self, _viewport: Viewport) -> Result<()> {
        ensure(
            [self.x, self.y, self.width, self.height]
                .iter()
                .all(|n| n.is_finite()),
            "Area coordinates must be finite.",
        )?;
        ensure(
            self.width >= 1.0 && self.height >= 1.0,
            "An area must be at least 1 pixel wide and high.",
        )?;
        ensure(
            (self.x + self.width).is_finite()
                && (self.y + self.height).is_finite()
                && self.x + self.width > self.x
                && self.y + self.height > self.y
                && self.x + 1.0 > self.x
                && self.y + 1.0 > self.y
                && self.x + self.width + 1.0 > self.x + self.width
                && self.y + self.height + 1.0 > self.y + self.height,
            "Area endpoints must be finite and precise enough to distinguish individual pixels.",
        )
    }
}

/// A point in canvas coordinates, used by request-area outlines.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestPoint {
    pub x: f64,
    pub y: f64,
}

impl RequestPoint {
    fn validate(&self) -> Result<()> {
        ensure(
            self.x.is_finite() && self.y.is_finite(),
            "Request area points must be finite.",
        )
    }
}

/// What a pending request may change. A request keeps its targets until it is applied or
/// removed, so the request box can be edited without losing its scope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RequestTarget {
    Element {
        page: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<Uuid>,
        id: Uuid,
    },
    Boundary {
        page: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<Uuid>,
    },
    Reference {
        id: Uuid,
    },
    Pointer {
        id: Uuid,
    },
}

impl RequestTarget {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Element { page, id, .. } => ensure(
                !page.is_nil() && !id.is_nil(),
                "A request target needs a boundary and a layer.",
            ),
            Self::Boundary { page, .. } => {
                ensure(!page.is_nil(), "A request target needs a boundary.")
            }
            Self::Reference { id } | Self::Pointer { id } => {
                ensure(!id.is_nil(), "A request target needs an identifier.")
            }
        }
    }

    /// The boundary a target belongs to, when it has one.
    pub fn page(&self) -> Option<Uuid> {
        match self {
            Self::Element { page, .. } | Self::Boundary { page, .. } => Some(*page),
            Self::Reference { .. } | Self::Pointer { .. } => None,
        }
    }
}

/// The outline of a request area. Coverage uses the real outline, not the bounding box.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RequestShape {
    Rect { rect: Rect },
    Ellipse { rect: Rect },
    Polygon { points: Vec<RequestPoint> },
    Freehand { points: Vec<RequestPoint> },
}

impl RequestShape {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Rect { rect } | Self::Ellipse { rect } => rect.validate(Viewport {
                width: 1,
                height: 1,
            }),
            Self::Polygon { points } | Self::Freehand { points } => {
                ensure(
                    (3..=4096).contains(&points.len()),
                    "A request area needs between 3 and 4,096 points.",
                )?;
                for point in points {
                    point.validate()?;
                }
                Ok(())
            }
        }
    }

    /// Axis-aligned bounds of the outline, used to place new content and its marker.
    pub fn bounds(&self) -> Rect {
        match self {
            Self::Rect { rect } | Self::Ellipse { rect } => *rect,
            Self::Polygon { points } | Self::Freehand { points } => {
                let left = points
                    .iter()
                    .map(|point| point.x)
                    .fold(f64::INFINITY, f64::min);
                let top = points
                    .iter()
                    .map(|point| point.y)
                    .fold(f64::INFINITY, f64::min);
                let right = points
                    .iter()
                    .map(|point| point.x)
                    .fold(f64::NEG_INFINITY, f64::max);
                let bottom = points
                    .iter()
                    .map(|point| point.y)
                    .fold(f64::NEG_INFINITY, f64::max);
                Rect {
                    x: left,
                    y: top,
                    width: (right - left).max(1.0),
                    height: (bottom - top).max(1.0),
                }
            }
        }
    }
}

/// Where a request area sits: inside one boundary size, or free on the canvas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestArea {
    /// The boundary page the area was drawn in; absent means canvas coordinates outside every
    /// boundary. Canvas coordinates use the same origin as boundary positions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Uuid>,
    pub shape: RequestShape,
}

impl RequestArea {
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.page.is_some() || self.size.is_none(),
            "A request area names a size only inside a boundary.",
        )?;
        self.shape.validate()
    }
}

/// A request waiting for Generate. Requests belong to the project, so they can target any mix
/// of layers, boundaries, reference items and pointers, with or without a drawn area.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingRequest {
    pub id: Uuid,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<RequestAttachment>,
    #[serde(default)]
    pub targets: Vec<RequestTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<RequestArea>,
    pub requested_at: String,
}

impl PendingRequest {
    pub fn validate(&self) -> Result<()> {
        ensure(!self.id.is_nil(), "A request needs a nonempty identifier.")?;
        validate_text(&self.text, "Request", MAX_PROMPT_CHARS, false)?;
        ensure(
            !self.requested_at.is_empty()
                && self.requested_at.chars().count() <= 64
                && !self.requested_at.chars().any(char::is_control),
            "A request needs a short creation time.",
        )?;
        ensure(
            self.attachments.len() <= crate::MAX_ELEMENT_CONTEXT,
            "A request supports at most 32 attachments.",
        )?;
        for attachment in &self.attachments {
            attachment.validate()?;
        }
        ensure(
            self.targets.len() <= MAX_REQUEST_TARGETS,
            "A request targets at most 64 things.",
        )?;
        for target in &self.targets {
            target.validate()?;
        }
        if let Some(area) = &self.area {
            area.validate()?;
        }
        Ok(())
    }

    /// True when the request targets this layer directly.
    pub fn targets_element(&self, page: Uuid, id: Uuid, size: Option<Uuid>) -> bool {
        self.targets.iter().any(|target| match target {
            RequestTarget::Element {
                page: target_page,
                size: target_size,
                id: target_id,
            } => {
                *target_page == page
                    && *target_id == id
                    && (target_size.is_none() || size.is_none() || *target_size == size)
            }
            _ => false,
        })
    }

    /// True when the request targets this boundary or a layer in it.
    pub fn targets_boundary(&self, page: Uuid, size: Option<Uuid>) -> bool {
        self.targets.iter().any(|target| match target {
            RequestTarget::Boundary {
                page: target_page,
                size: target_size,
            }
            | RequestTarget::Element {
                page: target_page,
                size: target_size,
                ..
            } => {
                *target_page == page
                    && (size.is_none() || target_size.is_none() || *target_size == size)
            }
            _ => false,
        })
    }
}

/// How a page behaves inside its canvas boundary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum BoundaryKind {
    /// Fixed width, grows taller as content is added.
    Page,
    /// Fixed size, content beyond it clips.
    Screen,
}

/// What a boundary ships as. Websites and apps are built by the AI from the element tree;
/// slides and images are rendered directly from it.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OutputType {
    #[default]
    Webpage,
    AppScreen,
    Slide,
    Image,
}

impl OutputType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Webpage => "Webpage",
            Self::AppScreen => "App screen",
            Self::Slide => "Slide",
            Self::Image => "Image",
        }
    }

    /// Websites and apps are written by the AI export job; slides and images render directly.
    pub fn needs_ai(self) -> bool {
        matches!(self, Self::Webpage | Self::AppScreen)
    }

    /// The reply field a page's exported files arrive in.
    pub fn reply_kind(self) -> &'static str {
        if self.needs_ai() { "code" } else { "render" }
    }
}

/// One screen size of a page boundary. A page has one or more sizes drawn side by side on the
/// canvas; the first is the base size, whose element values every other size inherits unless it
/// overrides them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageSize {
    pub id: Uuid,
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub kind: BoundaryKind,
    pub x: f64,
    pub y: f64,
}

impl PageSize {
    pub fn validate(&self) -> Result<()> {
        ensure(!self.id.is_nil(), "A size needs a nonempty identifier.")?;
        validate_text(&self.label, "Size name", 60, true)?;
        Viewport {
            width: self.width,
            height: self.height,
        }
        .validate()?;
        ensure(
            self.x.is_finite() && self.y.is_finite(),
            "Canvas positions must be finite.",
        )
    }
}

/// A common size offered when adding a linked size to a boundary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SizePreset {
    Phone,
    Tablet,
    Desktop,
    Custom,
}

impl SizePreset {
    /// The width, height and name of a preset; Custom uses the caller's values.
    pub fn dimensions(self, width: u32, height: u32) -> (String, u32, u32) {
        match self {
            Self::Phone => ("Phone".into(), 390, 844),
            Self::Tablet => ("Tablet".into(), 820, 1180),
            Self::Desktop => ("Desktop".into(), 1440, 900),
            Self::Custom => ("Size".into(), width, height),
        }
    }
}

/// The starter boundaries a new untitled project can begin with.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CanvasTemplate {
    #[default]
    Blank,
    Website,
    AppScreen,
    SlideDeck,
    SocialImage,
}

/// Space between the starter boundaries of a template, matching the canvas layout gap.
const TEMPLATE_GAP: u32 = 160;

impl CanvasTemplate {
    fn size(width: u32, height: u32, kind: BoundaryKind, x: u32) -> PageSize {
        PageSize {
            id: Uuid::new_v4(),
            label: size_label(width).into(),
            width,
            height,
            kind,
            x: f64::from(x),
            y: 0.0,
        }
    }

    fn page(
        &self,
        name: &str,
        path: &str,
        output: OutputType,
        sizes: Vec<PageSize>,
    ) -> Page {
        Page {
            id: Uuid::new_v4(),
            name: name.into(),
            path: path.into(),
            included: true,
            output,
            sizes,
            background: "#ffffff".into(),
            elements: vec![],
        }
    }

    fn pages(self) -> Vec<Page> {
        match self {
            Self::Blank => vec![],
            Self::Website => {
                // Desktop is the base size; Tablet and Phone inherit from it.
                let desktop = Self::size(1440, 900, BoundaryKind::Page, 0);
                let tablet =
                    Self::size(820, 1180, BoundaryKind::Page, desktop.width + TEMPLATE_GAP);
                let phone = Self::size(
                    390,
                    844,
                    BoundaryKind::Page,
                    tablet.x as u32 + tablet.width + TEMPLATE_GAP,
                );
                vec![self.page(
                    "Page 1",
                    "/",
                    OutputType::Webpage,
                    vec![desktop, tablet, phone],
                )]
            }
            Self::AppScreen => vec![self.page(
                "Screen 1",
                "/",
                OutputType::AppScreen,
                vec![Self::size(390, 844, BoundaryKind::Screen, 0)],
            )],
            Self::SlideDeck => (0..3)
                .map(|index| {
                    let x = index * (1920 + TEMPLATE_GAP);
                    self.page(
                        &format!("Slide {}", index + 1),
                        &format!("/slide-{}", index + 1),
                        OutputType::Slide,
                        vec![Self::size(1920, 1080, BoundaryKind::Screen, x)],
                    )
                })
                .collect(),
            Self::SocialImage => vec![self.page(
                "Image 1",
                "/",
                OutputType::Image,
                vec![Self::size(1080, 1080, BoundaryKind::Screen, 0)],
            )],
        }
    }
}

/// The name a new size gets from its width, so a boundary reads as Phone, Tablet or Desktop.
pub fn size_label(width: u32) -> &'static str {
    if width >= 1024 {
        "Desktop"
    } else if width >= 700 {
        "Tablet"
    } else {
        "Phone"
    }
}

/// One size's spot on the project canvas, as sent by a layout command. Field names stay
/// snake_case like every other command argument.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasPlacement {
    pub page_id: Uuid,
    pub size_id: Uuid,
    pub x: f64,
    pub y: f64,
    pub kind: BoundaryKind,
}

impl CanvasPlacement {
    fn place(self, pages: &mut [Page]) -> Result<()> {
        let page = pages
            .iter_mut()
            .find(|page| page.id == self.page_id)
            .ok_or_else(|| Error("A page on the canvas no longer exists.".into()))?;
        let size = page
            .size_mut(self.size_id)
            .ok_or_else(|| Error("This size on the canvas no longer exists.".into()))?;
        size.x = self.x;
        size.y = self.y;
        size.kind = self.kind;
        Ok(())
    }
}

/// A saved camera position, so the user can return to a part of the canvas. Views belong to
/// the project file and never affect what ships.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NamedView {
    pub id: Uuid,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl NamedView {
    pub fn validate(&self) -> Result<()> {
        ensure(!self.id.is_nil(), "View identifiers must be nonempty.")?;
        validate_text(
            &self.name,
            "View name",
            MAX_NAMED_VIEW_NAME_CHARS,
            true,
        )?;
        ensure(
            self.x.is_finite() && self.y.is_finite() && self.zoom.is_finite(),
            "A view's camera must be finite.",
        )?;
        ensure(
            self.zoom > 0.0 && self.zoom <= 1000.0,
            "A view's zoom must be greater than 0 and at most 1000.",
        )
    }
}

/// One reference item's spot on the project canvas, moved when a boundary pushes it aside.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReferencePlacement {
    pub item_id: Uuid,
    pub x: f64,
    pub y: f64,
}

/// A reference item's content. Reference items are material the AI may read, and
/// nothing outside every boundary ever ships.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ReferenceKind {
    /// A design layer outside every boundary, retained for a lossless round trip.
    Element {
        element: Box<crate::Element>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<Uuid>,
    },
    Note {
        text: String,
    },
    Link {
        url: String,
        title: String,
    },
    /// A picture already stored in `project.pictures`; bytes never leave the project.
    Image {
        picture: Uuid,
        /// The original file name, when the project still knows it; context sends names only.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        name: String,
    },
    Swatch {
        color: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceItem {
    pub id: Uuid,
    pub rect: Rect,
    pub kind: ReferenceKind,
}

impl ReferenceItem {
    pub fn validate(&self, project: &Project) -> Result<()> {
        self.rect.validate(Viewport {
            width: 1,
            height: 1,
        })?;
        match &self.kind {
            // Stored trees are validated with the project-wide element ids and limits.
            ReferenceKind::Element { .. } => {}
            ReferenceKind::Note { text } => {
                ensure(
                    text.chars().count() <= MAX_PROMPT_CHARS && !text.contains('\0'),
                    "Reference notes are limited to 16,384 characters.",
                )?;
            }
            ReferenceKind::Link { url, title } => {
                ensure(
                    url.len() <= 2048
                        && url::Url::parse(url).is_ok_and(|parsed| {
                            matches!(parsed.scheme(), "https" | "http")
                                && parsed.host_str().is_some()
                                && parsed.username().is_empty()
                                && parsed.password().is_none()
                        }),
                    "A reference link must start with http:// or https://.",
                )?;
                validate_text(title, "Link title", 240, false)?;
            }
            ReferenceKind::Image { picture, name } => {
                ensure(
                    project.picture_uri(*picture).is_some(),
                    "A reference picture is not in this project.",
                )?;
                validate_text(name, "Picture name", 120, false)?;
            }
            ReferenceKind::Swatch { color } => ensure(
                valid_swatch(color),
                "A swatch colour must be #rgb, #rrggbb or #rrggbbaa.",
            )?,
        }
        Ok(())
    }
}

/// Where a pointer starts: a layer inside a page, or the page boundary itself. `size_id` names
/// the linked size the endpoint was drawn in, when the page has more than one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PointerSource {
    Element {
        page_id: Uuid,
        element_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_id: Option<Uuid>,
    },
    Boundary {
        page_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_id: Option<Uuid>,
    },
}

/// Where a pointer goes. A change pointer grants edit permission; a reference pointer only
/// reads its target. Reference items and URLs are always read only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PointerTarget {
    Element {
        page_id: Uuid,
        element_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_id: Option<Uuid>,
    },
    Boundary {
        page_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_id: Option<Uuid>,
    },
    Reference {
        item_id: Uuid,
    },
    Url {
        url: String,
    },
}

/// What Play can do with a simple pointer. Everything else exports only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlayAction {
    GoTo { boundary: Uuid },
    Show { element: Uuid },
    Hide { element: Uuid },
    Toggle { element: Uuid },
    Swap { element: Uuid, with_element: Uuid },
}

/// The generated code a pointer produced: a fingerprint of its marked fragments and the comment
/// it was generated from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointerWiring {
    pub fingerprint: String,
    pub generated_comment: String,
}

/// One boundary as an export wrote it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportedPage {
    pub page_id: Uuid,
    pub path: String,
    pub output: OutputType,
}

/// The code one pointer produced: a fingerprint of its marked fragments and the comment the
/// export was written for. A pointer with no marked fragments is not recorded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportedPointer {
    pub fingerprint: String,
    pub comment: String,
}

/// What the last export produced. Kept in the project so the canvas can compare its comments
/// and recompute pointer fingerprints from the destination folder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    /// The project database version the export was written from.
    pub version: i64,
    /// The absolute export folder.
    pub destination: String,
    pub exported_at: String,
    pub pages: Vec<ExportedPage>,
    #[serde(default)]
    pub pointers: BTreeMap<Uuid, ExportedPointer>,
    /// Accepted behaviour code, independent of visual/text edits.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub behaviours: BTreeMap<Uuid, crate::ExportBehaviour>,
}

impl ExportRecord {
    pub fn validate(&self) -> Result<()> {
        ensure(!self.id.is_nil(), "An export needs a nonempty identifier.")?;
        ensure(
            !self.project_id.is_nil(),
            "An export needs the project it was written from.",
        )?;
        ensure(self.version >= 0, "An export version cannot be negative.")?;
        ensure(
            self.destination.len() <= MAX_EXPORT_DESTINATION
                && (!self.destination.contains('\0'))
                && std::path::Path::new(&self.destination).is_absolute(),
            "An export destination must be an absolute folder path.",
        )?;
        validate_text(&self.exported_at, "Export time", 64, true)?;
        ensure(
            self.pages.len() <= MAX_PAGES,
            "An export records at most 100 boundaries.",
        )?;
        for page in &self.pages {
            ensure(
                !page.page_id.is_nil(),
                "An exported boundary needs a nonempty identifier.",
            )?;
            validate_path(&page.path)?;
        }
        ensure(
            self.pointers.len() <= MAX_POINTERS,
            "An export records at most 1,000 pointers.",
        )?;
        for exported in self.pointers.values() {
            ensure(
                exported.fingerprint.len() == 64
                    && exported
                        .fingerprint
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit()),
                "An export fingerprint must be a SHA-256 digest.",
            )?;
            validate_text(
                &exported.comment,
                "Pointer comment",
                MAX_PROMPT_CHARS,
                false,
            )?;
        }
        crate::validate_behaviours(&self.behaviours)?;
        Ok(())
    }
}

/// What a pointer lets a request do with its target.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PointerKind {
    /// The AI may read the target as context, never change it.
    #[default]
    Reference,
    /// The AI may read and change the target; behaviour comments compile at export.
    Change,
}

/// A line from a frame or boundary to what it grants the AI permission to read or change.
/// The comment is the spec; a missing endpoint leaves the pointer in place and broken.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pointer {
    pub id: Uuid,
    pub from: PointerSource,
    pub to: PointerTarget,
    pub comment: String,
    #[serde(default)]
    pub kind: PointerKind,
    #[serde(default)]
    pub wiring: Option<PointerWiring>,
    /// A simple action the comment asks for, and the comment it was classified from.
    #[serde(
        default,
        deserialize_with = "crate::required_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub play: Option<PlayAction>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub play_comment: String,
}

impl Pointer {
    pub fn validate(&self) -> Result<()> {
        ensure(!self.id.is_nil(), "A pointer needs a nonempty identifier.")?;
        validate_text(&self.comment, "Pointer comment", MAX_PROMPT_CHARS, false)?;
        if let PointerTarget::Url { url } = &self.to {
            ensure(
                valid_http_url(url),
                "A pointer URL must start with http:// or https://.",
            )?;
        }
        if let Some(wiring) = &self.wiring {
            ensure(
                wiring.fingerprint.len() <= 256 && !wiring.fingerprint.contains('\0'),
                "A pointer fingerprint is limited to 256 characters.",
            )?;
            validate_text(
                &wiring.generated_comment,
                "Wiring comment",
                MAX_PROMPT_CHARS,
                false,
            )?;
        }
        validate_text(&self.play_comment, "Play comment", MAX_PROMPT_CHARS, false)?;
        if let Some(PlayAction::Swap {
            element,
            with_element,
        }) = &self.play
        {
            ensure(
                element != with_element,
                "A swap pointer needs two different layers.",
            )?;
        }
        if let Some(PlayAction::GoTo { boundary }) = &self.play {
            ensure(!boundary.is_nil(), "A go-to pointer needs a boundary.")?;
        }
        Ok(())
    }

    pub fn is_broken(&self, project: &Project) -> bool {
        pointer_is_broken(project, self)
    }
}

fn valid_http_url(url: &str) -> bool {
    url.len() <= 2048
        && url::Url::parse(url).is_ok_and(|parsed| {
            matches!(parsed.scheme(), "https" | "http")
                && parsed.host_str().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none()
        })
}

fn pointer_source_missing(project: &Project, source: &PointerSource) -> bool {
    match source {
        PointerSource::Element {
            page_id,
            element_id,
            size_id,
        } => project
            .pages
            .iter()
            .find(|page| page.id == *page_id)
            .is_none_or(|page| {
                crate::elements::find_in(&page.elements, *element_id).is_none()
                    || size_id.is_some_and(|size| page.size(size).is_none())
            }),
        PointerSource::Boundary { page_id, size_id } => project
            .pages
            .iter()
            .find(|page| page.id == *page_id)
            .is_none_or(|page| size_id.is_some_and(|size| page.size(size).is_none())),
    }
}

fn pointer_target_missing(project: &Project, target: &PointerTarget) -> bool {
    match target {
        PointerTarget::Element {
            page_id,
            element_id,
            size_id,
        } => project
            .pages
            .iter()
            .find(|page| page.id == *page_id)
            .is_none_or(|page| {
                crate::elements::find_in(&page.elements, *element_id).is_none()
                    || size_id.is_some_and(|size| page.size(size).is_none())
            }),
        PointerTarget::Boundary { page_id, size_id } => project
            .pages
            .iter()
            .find(|page| page.id == *page_id)
            .is_none_or(|page| size_id.is_some_and(|size| page.size(size).is_none())),
        PointerTarget::Reference { item_id } => {
            !project.reference.iter().any(|item| item.id == *item_id)
        }
        // A URL is drawn on its reference card; without that card the line has no target.
        PointerTarget::Url { url } => !project
            .reference
            .iter()
            .any(|item| matches!(&item.kind, ReferenceKind::Link { url: card, .. } if card == url)),
    }
}

/// Pure: a pointer is broken when either endpoint no longer exists. Pointers are never
/// deleted with what they reference, so this decides how they are shown and offered.
pub fn pointer_is_broken(project: &Project, pointer: &Pointer) -> bool {
    pointer_source_missing(project, &pointer.from) || pointer_target_missing(project, &pointer.to)
}

/// Every pointer whose source or target is missing, in project order.
pub fn broken_pointers(project: &Project) -> Vec<Uuid> {
    project
        .pointers
        .iter()
        .filter(|pointer| pointer.is_broken(project))
        .map(|pointer| pointer.id)
        .collect()
}

fn valid_swatch(color: &str) -> bool {
    let Some(hex) = color.strip_prefix('#') else {
        return false;
    };
    (hex.len() == 3 || hex.len() == 6 || hex.len() == 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

fn normalized_swatch(color: &str) -> String {
    let hex = color.trim().trim_start_matches('#').to_ascii_lowercase();
    // #rgb and #rrggbb describe the same colour; store the canonical six digits.
    let hex = if hex.len() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect()
    } else {
        hex
    };
    format!("#{hex}")
}

fn attachment_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].into()
}

fn reference_picture_name(picture: Uuid, name: &str) -> String {
    if name.trim().is_empty() {
        format!("Picture {}", &picture.simple().to_string()[..8])
    } else {
        name.chars().take(120).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Page {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub included: bool,
    /// What this boundary ships as: a webpage, app screen, slide or image.
    #[serde(default)]
    pub output: OutputType,
    /// Linked sizes, base first. Every size draws its own boundary on the canvas.
    pub sizes: Vec<PageSize>,
    pub background: String,
    /// Root layers of this boundary, back to front.
    pub elements: Vec<Element>,
}

impl Page {
    pub fn color_scheme(&self) -> &'static str {
        let hex = normalized_swatch(&self.background);
        let channel = |index| {
            let value = u8::from_str_radix(&hex[index..index + 2], 16).unwrap_or(255) as f64 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        if channel(1) * 0.2126 + channel(3) * 0.7152 + channel(5) * 0.0722 < 0.179 {
            "dark"
        } else {
            "light"
        }
    }

    /// The base size: its element values are the ones the other sizes inherit.
    pub fn base_size(&self) -> &PageSize {
        self.sizes.first().expect("a validated page has a size")
    }

    pub fn base_size_id(&self) -> Uuid {
        self.base_size().id
    }

    pub fn size(&self, id: Uuid) -> Option<&PageSize> {
        self.sizes.iter().find(|size| size.id == id)
    }

    pub fn size_mut(&mut self, id: Uuid) -> Option<&mut PageSize> {
        self.sizes.iter_mut().find(|size| size.id == id)
    }

    /// The page's base viewport, the size the design starts from.
    pub fn viewport(&self) -> Viewport {
        let base = self.base_size();
        Viewport {
            width: base.width,
            height: base.height,
        }
    }

    pub fn has_many_sizes(&self) -> bool {
        self.sizes.len() > 1
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Brief {
    pub purpose: String,
    pub audience: String,
    pub brand_name: String,
    pub tone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum Scope {
    Project,
    Page(Uuid),
    Element(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Decision {
    pub id: Uuid,
    pub topic: String,
    pub value: String,
    pub scope: Scope,
    pub protected: bool,
    pub accepted_at_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignException {
    pub id: Uuid,
    pub rule: String,
    pub reason: String,
    pub scope: Scope,
    pub accepted_at_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionRecord {
    pub schema_version: u32,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColourStyle {
    pub id: Uuid,
    pub name: String,
    pub value: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextStyle {
    pub id: Uuid,
    pub name: String,
    pub font_family: String,
    pub size: f64,
    pub weight: u16,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub italic: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectStyles {
    pub colours: Vec<ColourStyle>,
    pub text: Vec<TextStyle>,
}
impl ProjectStyles {
    pub fn resolve(&self, style: &mut crate::ElementStyle) {
        let colour = |id| {
            self.colours
                .iter()
                .find(|c| Some(c.id) == id)
                .map(|c| c.value.clone())
        };
        if let Some(value) = colour(style.references.fill) {
            style.fill = Some(value);
        }
        if let Some(value) = colour(style.references.stroke) {
            style.stroke.color = Some(value);
        }
        if let Some(value) = colour(style.references.text_colour) {
            style.text_style.color = value;
        }
        if let Some(text) = self
            .text
            .iter()
            .find(|t| Some(t.id) == style.references.text)
        {
            style.text_style.font_family = text.font_family.clone();
            style.text_style.size = text.size;
            style.text_style.weight = text.weight;
            style.text_style.line_height = text.line_height;
            style.text_style.letter_spacing = text.letter_spacing;
            style.text_style.italic = text.italic;
        }
    }
    pub fn validate_references(&self, refs: &crate::StyleReferences) -> Result<()> {
        for id in [refs.fill, refs.stroke, refs.text_colour]
            .into_iter()
            .flatten()
        {
            ensure(
                self.colours.iter().any(|c| c.id == id),
                "A colour style no longer exists.",
            )?;
        }
        ensure(
            refs.text
                .is_none_or(|id| self.text.iter().any(|t| t.id == id)),
            "A text style no longer exists.",
        )
    }
    fn validate(&self) -> Result<()> {
        ensure(
            self.colours.len() + self.text.len() <= 1000,
            "A project supports at most 1,000 styles.",
        )?;
        let mut ids = std::collections::BTreeSet::new();
        for (id, name) in self
            .colours
            .iter()
            .map(|s| (s.id, &s.name))
            .chain(self.text.iter().map(|s| (s.id, &s.name)))
        {
            ensure(
                !id.is_nil() && ids.insert(id),
                "Style identifiers must be nonempty and unique.",
            )?;
            validate_text(name, "Style name", 120, false)?;
        }
        for colour in &self.colours {
            crate::elements::validate_colour(&colour.value, "Style colour")?;
        }
        for text in &self.text {
            let mut style = crate::empty_element(
                crate::ElementKind::Text,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
            )
            .style;
            style.references.text = Some(text.id);
            self.resolve(&mut style);
            style.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    #[serde(default)]
    pub styles: ProjectStyles,
    pub schema_version: u32,
    #[serde(default)]
    pub template: CanvasTemplate,
    pub id: Uuid,
    pub name: String,
    pub revision: u64,
    pub viewport: Viewport,
    pub appearance: Appearance,
    pub show_grid: bool,
    pub grid_settings: GridSettings,
    pub generation_mode: GenerationMode,
    pub home_page_id: Uuid,
    pub pages: Vec<Page>,
    pub brief: Brief,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_identity: Option<crate::ImportedIdentity>,
    #[serde(default)]
    pub pictures: BTreeMap<Uuid, std::sync::Arc<str>>,
    /// Everything outside every boundary: reference the AI can read that never ships.
    #[serde(default)]
    pub reference: Vec<ReferenceItem>,
    /// Lines from a frame or boundary to what its comment may read or change. A pointer
    /// outlives what it references so a deleted target can be reattached.
    #[serde(default)]
    pub pointers: Vec<Pointer>,
    /// Named elements every copy shares. Copies store their own position, parent and request
    /// history; a definition outlives its last copy.
    #[serde(default)]
    pub linked: BTreeMap<Uuid, LinkedDefinition>,
    /// Requests waiting for Generate. They belong to the project, not to one layer, because a
    /// request can target several layers, boundaries, reference items and pointers at once.
    #[serde(default)]
    pub requests: Vec<PendingRequest>,
    /// Saved camera positions for returning to a part of the canvas.
    #[serde(default)]
    pub views: Vec<NamedView>,
    pub decisions: Vec<Decision>,
    pub exceptions: Vec<DesignException>,
    pub extensions: BTreeMap<String, ExtensionRecord>,
    pub applied_job_ids: Vec<Uuid>,
    #[serde(deserialize_with = "crate::required_nullable")]
    pub latest_generation: Option<GenerationDossier>,
    /// The last export written from this project, so pointer clashes can compare against it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_export: Option<ExportRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    AddColourStyle { style: ColourStyle },
    UpdateColourStyle { style: ColourStyle },
    RemoveColourStyle { id: Uuid },
    AddTextStyle { style: TextStyle },
    UpdateTextStyle { style: TextStyle },
    RemoveTextStyle { id: Uuid },
    RenameProject {
        name: String,
    },
    /// Lay a page's code out readably (Code view → Tidy code), without the AI.
    SetGrid {
        enabled: bool,
    },
    SetGridSettings {
        settings: GridSettings,
    },
    SetGenerationMode {
        mode: GenerationMode,
    },
    UpdatePage {
        page_id: Uuid,
        name: String,
        path: String,
        background: String,
        included: bool,
    },
    /// Change what a boundary ships as.
    SetPageOutput {
        page_id: Uuid,
        output: OutputType,
    },
    /// Change one linked size of a boundary: its name, dimensions or kind.
    UpdatePageSize {
        page_id: Uuid,
        size_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<BoundaryKind>,
        /// Neighbours a grown size pushes aside, in the same undo step.
        #[serde(default)]
        moves: Vec<CanvasPlacement>,
        #[serde(default)]
        reference: Vec<ReferencePlacement>,
    },
    /// Add a page and place it on the project canvas in one step. `moves` carries the
    /// boundaries the new one pushes aside, so drawing is a single undoable gesture.
    CreateBoundary {
        page: Page,
        #[serde(default)]
        after_page_id: Option<Uuid>,
        #[serde(default)]
        moves: Vec<CanvasPlacement>,
        #[serde(default)]
        reference: Vec<ReferencePlacement>,
    },
    AddCanvasPage {
        name: String,
        path: String,
        size: PageSize,
        #[serde(default)]
        moves: Vec<CanvasPlacement>,
        /// Reference items the new boundary pushes aside, in the same undo step.
        #[serde(default)]
        reference: Vec<ReferencePlacement>,
    },
    /// Add a linked size to a page, placed to the right of it, with the neighbours it pushes.
    AddPageSize {
        page_id: Uuid,
        size: PageSize,
        #[serde(default)]
        moves: Vec<CanvasPlacement>,
        #[serde(default)]
        reference: Vec<ReferencePlacement>,
    },
    /// Remove a linked size. The base size cannot be removed.
    RemovePageSize {
        page_id: Uuid,
        size_id: Uuid,
    },
    /// Move one or more boundaries in one step, e.g. a drag and the neighbours it pushes.
    SetCanvasLayout {
        placements: Vec<CanvasPlacement>,
        /// Reference items the moved or drawn boundaries push aside, in the same undo step.
        #[serde(default)]
        reference: Vec<ReferencePlacement>,
    },
    /// Save the current camera as a named view.
    AddNamedView {
        name: String,
        x: f64,
        y: f64,
        zoom: f64,
    },
    /// Rename a view and/or move its camera in one undo step.
    UpdateNamedView {
        view_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zoom: Option<f64>,
    },
    RemoveNamedView {
        view_id: Uuid,
    },
    AddReferenceItem {
        item: ReferenceItem,
        /// Picture bytes for a dropped Image item; the command derives and stores its id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        picture_data_uri: Option<String>,
    },
    /// Move an item and/or change its content in one undo step.
    UpdateReferenceItem {
        item_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rect: Option<Rect>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
    },
    RemoveReferenceItems {
        ids: Vec<Uuid>,
    },
    AddPointer {
        pointer: Pointer,
    },
    /// Change a comment, kind and/or either endpoint in one undo step.
    UpdatePointer {
        pointer_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<PointerSource>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<PointerTarget>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<PointerKind>,
    },
    RemovePointers {
        ids: Vec<Uuid>,
    },
    /// Set or clear the generated code a pointer is wired to. A new comment in the same command
    /// keeps Keep code to one undo step; a missing wiring clears it so it is generated again.
    SetPointerWiring {
        pointer_id: Uuid,
        wiring: Option<PointerWiring>,
        comment: Option<String>,
    },
    /// Resolve contradicting pointers from one source: keep this one, remove the others, and
    /// optionally replace its comment (Merge, which also clears the wiring).
    ResolvePointerClash {
        keep: Uuid,
        drop: Vec<Uuid>,
        merged_comment: Option<String>,
    },
    /// Record a completed export: the version, folder, boundaries and the code fingerprint of
    /// every pointer it built. Also wires those pointers to the code it produced.
    RecordExport {
        record: ExportRecord,
    },
    /// Keep a pointer's comment after its exported code changed: forget the recorded code so the
    /// next export rebuilds it.
    ForgetExportPointer {
        pointer_id: Uuid,
    },
    /// A layer dragged out of every boundary retains its editable tree in reference.
    ElementToReference {
        page_id: Uuid,
        element_id: Uuid,
        rect: Rect,
        #[serde(default)]
        size_id: Option<Uuid>,
    },
    /// A note, link, colour or picture dragged into a boundary becomes a layer there.
    ReferenceToElement {
        item_id: Uuid,
        page_id: Uuid,
        rect: Rect,
        #[serde(default)]
        size_id: Option<Uuid>,
    },
    /// Selected reference items travel with these requests as context, through the same
    /// request attachments generation already sends.
    AttachReferenceContext {
        request_ids: Vec<Uuid>,
        item_ids: Vec<Uuid>,
    },
    DeletePage {
        page_id: Uuid,
    },
    InsertElements {
        page_id: Uuid,
        parent_id: Option<Uuid>,
        index: usize,
        elements: Vec<Element>,
        /// Picture bytes for a placed image; the command stores them and sets the reference.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        picture_data_uri: Option<String>,
        /// The size the geometry was drawn in; new layers are visible in every linked size.
        /// Narrower sizes receive fitting geometry overrides. Omitted uses the base size.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_id: Option<Uuid>,
    },
    /// Add a rendered website as boundaries, reference notes, behaviour pointers and pictures
    /// in one undo step. The native importer has already turned pictures into data URIs.
    ImportWebsite {
        pages: Vec<Page>,
        reference: Vec<ReferenceItem>,
        pointers: Vec<Pointer>,
        pictures: BTreeMap<Uuid, String>,
    },
    UpdateElements {
        page_id: Uuid,
        patches: Vec<ElementPatch>,
    },
    /// Join 2 or more selected layers under one new linked definition. `source_id` is the
    /// copy whose content wins; every selected layer becomes an instance of it.
    LinkElements {
        ids: Vec<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_id: Option<Uuid>,
    },
    /// Detach selected instances into plain layers with their current resolved content.
    UnlinkElements {
        ids: Vec<Uuid>,
    },
    /// Forget a linked definition that no copy uses.
    RemoveLinkedDefinition {
        id: Uuid,
    },
    /// Paste new copies of a linked element, making the source linked when it is not.
    PasteLinkedInstances {
        items: Vec<LinkedPasteInstance>,
    },
    MoveElements {
        moves: Vec<ElementMove>,
    },
    RemoveElements {
        page_id: Uuid,
        ids: Vec<Uuid>,
    },
    /// One Delete gesture across every selected kind: layers, reference items, pointers,
    /// requests, boundaries, linked sizes and linked definitions, in one undo step.
    DeleteItems {
        #[serde(default)]
        elements: Vec<(Uuid, Uuid)>,
        #[serde(default)]
        reference_ids: Vec<Uuid>,
        #[serde(default)]
        pointer_ids: Vec<Uuid>,
        #[serde(default)]
        request_ids: Vec<Uuid>,
        #[serde(default)]
        page_ids: Vec<Uuid>,
        #[serde(default)]
        size_ids: Vec<(Uuid, Uuid)>,
        #[serde(default)]
        linked_ids: Vec<Uuid>,
    },
    GroupElements {
        page_id: Uuid,
        ids: Vec<Uuid>,
    },
    UngroupElements {
        page_id: Uuid,
        id: Uuid,
    },
    /// Queue a request on its targets. Adding, editing and removing a request are each one
    /// undo step.
    AddRequest {
        request: PendingRequest,
    },
    /// Replace a request's text, attachments, targets or area.
    UpdateRequest {
        request: PendingRequest,
    },
    RemoveRequests {
        ids: Vec<Uuid>,
    },
    UpdateElementContext {
        element_id: Uuid,
        context: Vec<RequestAttachment>,
    },
    SetBrief {
        brief: Brief,
    },
    SetDecision {
        topic: String,
        value: String,
        scope: Scope,
        protected: bool,
    },
    RemoveDecision {
        decision_id: Uuid,
    },
    AddException {
        rule: String,
        reason: String,
        scope: Scope,
    },
    RemoveException {
        exception_id: Uuid,
    },
}

impl Command {
    /// A short, plain label for the change. Saved versions are named from these.
    pub fn version_label(&self) -> &'static str {
        match self {
            Self::AddColourStyle { .. } | Self::AddTextStyle { .. } => "Added a style",
            Self::UpdateColourStyle { .. } | Self::UpdateTextStyle { .. } => "Changed a style",
            Self::RemoveColourStyle { .. } | Self::RemoveTextStyle { .. } => "Removed a style",
            Self::RenameProject { .. } => "Renamed the project",
            Self::SetGrid { .. } => "Changed the grid",
            Self::SetGridSettings { .. } => "Changed the grid",
            Self::SetGenerationMode { .. } => "Changed the AI style",
            Self::UpdatePage { .. } => "Changed a boundary",
            Self::SetPageOutput { .. } => "Changed a boundary's output",
            Self::UpdatePageSize { .. } => "Changed a size",
            Self::CreateBoundary { .. } | Self::AddCanvasPage { .. } => "Added a boundary",
            Self::AddPageSize { .. } => "Added a size",
            Self::RemovePageSize { .. } => "Removed a size",
            Self::SetCanvasLayout { .. } => "Moved boundaries",
            Self::AddNamedView { .. } => "Saved a view",
            Self::UpdateNamedView { .. } => "Changed a view",
            Self::RemoveNamedView { .. } => "Removed a view",
            Self::AddReferenceItem { item, .. } => match item.kind {
                ReferenceKind::Element { .. } => "Added a reference layer",
                ReferenceKind::Note { .. } => "Added a note",
                ReferenceKind::Link { .. } => "Added a link",
                ReferenceKind::Image { .. } => "Added a picture",
                ReferenceKind::Swatch { .. } => "Added a colour",
            },
            Self::UpdateReferenceItem { .. } => "Changed reference",
            Self::RemoveReferenceItems { .. } => "Removed reference",
            Self::AddPointer { .. } => "Added a pointer",
            Self::UpdatePointer { .. } => "Changed a pointer",
            Self::RemovePointers { .. } => "Removed pointers",
            Self::SetPointerWiring { .. } => "Wired a pointer",
            Self::ResolvePointerClash { .. } => "Resolved a pointer clash",
            Self::RecordExport { .. } => "Exported the project",
            Self::ForgetExportPointer { .. } => "Rebuilt a pointer at the next export",
            Self::ElementToReference { .. } => "Moved a layer to reference",
            Self::ReferenceToElement { .. } => "Moved reference into a boundary",
            Self::AttachReferenceContext { .. } => "Attached reference",
            Self::DeletePage { .. } => "Removed a boundary",
            Self::InsertElements { .. } => "Added elements",
            Self::ImportWebsite { .. } => "Imported a website",
            Self::UpdateElements { .. } => "Changed elements",
            Self::LinkElements { .. } => "Linked elements",
            Self::UnlinkElements { .. } => "Unlinked elements",
            Self::RemoveLinkedDefinition { .. } => "Removed a linked element",
            Self::PasteLinkedInstances { .. } => "Pasted linked elements",
            Self::MoveElements { .. } => "Moved elements",
            Self::RemoveElements { .. } => "Removed elements",
            Self::DeleteItems { .. } => "Removed items",
            Self::GroupElements { .. } => "Grouped elements",
            Self::UngroupElements { .. } => "Ungrouped elements",
            Self::AddRequest { .. } => "Asked AI",
            Self::UpdateRequest { .. } => "Changed a request",
            Self::RemoveRequests { .. } => "Removed a request",
            Self::UpdateElementContext { .. } => "Changed layer context",
            Self::SetBrief { .. } => "Changed the project brief",
            Self::SetDecision { .. } => "Kept a decision",
            Self::RemoveDecision { .. } => "Removed a decision",
            Self::AddException { .. } => "Added an exception",
            Self::RemoveException { .. } => "Removed an exception",
        }
    }
}

impl Project {
    fn detach_style(&mut self, id: Uuid, text: bool) -> Result<()> {
        ensure(
            if text {
                self.styles.text.iter().any(|s| s.id == id)
            } else {
                self.styles.colours.iter().any(|s| s.id == id)
            },
            "This style no longer exists.",
        )?;
        fn detach(styles: &ProjectStyles, elements: &mut [Element], id: Uuid) {
            for element in elements {
                for style in std::iter::once(&mut element.style).chain(
                    element
                        .overrides
                        .values_mut()
                        .filter_map(|o| o.style.as_mut()),
                ) {
                    styles.resolve(style);
                    for reference in [
                        &mut style.references.fill,
                        &mut style.references.stroke,
                        &mut style.references.text_colour,
                        &mut style.references.text,
                    ] {
                        if *reference == Some(id) {
                            *reference = None;
                        }
                    }
                }
                detach(styles, &mut element.children, id);
            }
        }
        for page in &mut self.pages {
            detach(&self.styles, &mut page.elements, id);
        }
        for item in &mut self.reference {
            if let ReferenceKind::Element { element, .. } = &mut item.kind {
                detach(&self.styles, std::slice::from_mut(element.as_mut()), id);
            }
        }
        for definition in self.linked.values_mut() {
            detach(
                &self.styles,
                std::slice::from_mut(&mut definition.master),
                id,
            );
        }
        Ok(())
    }
    pub fn new(name: String, viewport: Viewport, appearance: Appearance) -> Result<Self> {
        let home_page_id = Uuid::new_v4();
        let project = Self {
            schema_version: SCHEMA_VERSION,
            template: CanvasTemplate::Website,
            id: Uuid::new_v4(),
            name: name.trim().into(),
            revision: 0,
            viewport,
            appearance,
            show_grid: true,
            grid_settings: GridSettings::default(),
            generation_mode: GenerationMode::Fuzzy,
            home_page_id,
            pages: vec![Page {
                id: home_page_id,
                name: "Page 1".into(),
                path: "/".into(),
                included: true,
                output: OutputType::default(),
                sizes: vec![PageSize {
                    id: Uuid::new_v4(),
                    label: size_label(viewport.width).into(),
                    width: viewport.width,
                    height: viewport.height,
                    kind: BoundaryKind::Page,
                    x: 0.0,
                    y: 0.0,
                }],
                background: "#ffffff".into(),
                elements: vec![],
            }],
            brief: Brief::default(),
            imported_identity: None,
            pictures: BTreeMap::new(),
            reference: vec![],
            pointers: vec![],
            styles: ProjectStyles::default(),
            linked: BTreeMap::new(),
            requests: vec![],
            views: vec![],
            decisions: vec![],
            exceptions: vec![],
            extensions: BTreeMap::new(),
            applied_job_ids: vec![],
            latest_generation: None,
            last_export: None,
        };
        project.validate()?;
        Ok(project)
    }

    /// A new untitled canvas: no boundaries yet, so the first one becomes the entry boundary.
    pub fn blank(name: String, appearance: Appearance) -> Result<Self> {
        Self::untitled(name, appearance, CanvasTemplate::Blank)
    }

    /// A new untitled canvas with the starter boundaries of a template. The boundaries are
    /// the project's initial state, not an undoable change.
    pub fn untitled(
        name: String,
        appearance: Appearance,
        template: CanvasTemplate,
    ) -> Result<Self> {
        let pages = template.pages();
        let home_page_id = pages
            .first()
            .map(|page| page.id)
            .unwrap_or_else(Uuid::new_v4);
        let viewport = pages
            .first()
            .map(|page| Viewport {
                width: page.base_size().width,
                height: page.base_size().height,
            })
            .unwrap_or(Viewport {
                width: 1440,
                height: 900,
            });
        let project = Self {
            schema_version: SCHEMA_VERSION,
            template,
            id: Uuid::new_v4(),
            name: name.trim().into(),
            revision: 0,
            viewport,
            appearance,
            show_grid: true,
            grid_settings: GridSettings::default(),
            generation_mode: GenerationMode::Fuzzy,
            home_page_id,
            pages,
            brief: Brief::default(),
            imported_identity: None,
            pictures: BTreeMap::new(),
            reference: vec![],
            pointers: vec![],
            styles: ProjectStyles::default(),
            linked: BTreeMap::new(),
            requests: vec![],
            views: vec![],
            decisions: vec![],
            exceptions: vec![],
            extensions: BTreeMap::new(),
            applied_job_ids: vec![],
            latest_generation: None,
            last_export: None,
        };
        project.validate()?;
        Ok(project)
    }

    pub fn page(&self, id: Uuid) -> Result<&Page> {
        self.pages
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error("This page no longer exists.".into()))
    }

    pub(crate) fn page_mut(&mut self, id: Uuid) -> Result<&mut Page> {
        self.pages
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| Error("This page no longer exists.".into()))
    }

    /// A picture id resolves from the shared project store or an imported identity catalogue;
    /// its bytes stay in the project.
    pub(crate) fn picture_uri(&self, id: Uuid) -> Option<std::sync::Arc<str>> {
        self.pictures.get(&id).cloned().or_else(|| {
            let identity = self.imported_identity.as_ref()?;
            identity
                .assets
                .iter()
                .find(|asset| asset.id == id)
                .map(|asset| std::sync::Arc::from(asset.data_uri.as_str()))
                .or_else(|| {
                    identity
                        .photos
                        .iter()
                        .find(|photo| photo.id == id)
                        .map(|photo| photo.data_uri.clone())
                })
        })
    }

    fn pointer_mut(&mut self, id: Uuid) -> Result<&mut Pointer> {
        self.pointers
            .iter_mut()
            .find(|pointer| pointer.id == id)
            .ok_or_else(|| Error("This pointer no longer exists.".into()))
    }

    fn add_pointer(&mut self, pointer: Pointer) -> Result<()> {
        ensure(
            self.pointers.len() < MAX_POINTERS,
            "A project holds at most 1,000 pointers.",
        )?;
        ensure(
            !self
                .pointers
                .iter()
                .any(|existing| existing.id == pointer.id),
            "This pointer already exists.",
        )?;
        self.pointers.push(pointer);
        Ok(())
    }

    fn update_pointer(
        &mut self,
        pointer_id: Uuid,
        from: Option<PointerSource>,
        to: Option<PointerTarget>,
        comment: Option<String>,
        kind: Option<PointerKind>,
    ) -> Result<()> {
        let pointer = self.pointer_mut(pointer_id)?;
        if let Some(from) = from {
            pointer.from = from;
        }
        if let Some(to) = to {
            pointer.to = to;
        }
        if let Some(comment) = comment {
            pointer.comment = comment;
        }
        if let Some(kind) = kind {
            pointer.kind = kind;
        }
        Ok(())
    }

    fn reference_mut(&mut self, id: Uuid) -> Result<&mut ReferenceItem> {
        self.reference
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| Error("This reference item no longer exists.".into()))
    }

    fn place_reference(&mut self, placements: Vec<ReferencePlacement>) -> Result<()> {
        for placement in placements {
            let item = self.reference_mut(placement.item_id)?;
            item.rect.x = placement.x;
            item.rect.y = placement.y;
        }
        Ok(())
    }

    pub(crate) fn add_reference_item(
        &mut self,
        mut item: ReferenceItem,
        picture_data_uri: Option<String>,
    ) -> Result<()> {
        if let ReferenceKind::Swatch { color } = &mut item.kind {
            *color = normalized_swatch(color);
        }
        if let ReferenceKind::Image { picture, .. } = &mut item.kind {
            if let Some(uri) = picture_data_uri {
                let id = crate::project_assets::store_picture_uri(&uri)?;
                self.pictures.insert(id, std::sync::Arc::from(uri.as_str()));
                *picture = id;
            }
        } else {
            ensure(
                picture_data_uri.is_none(),
                "Only a reference picture takes picture bytes.",
            )?;
        }
        ensure(
            self.reference.len() < MAX_REFERENCE_ITEMS,
            "A reference holds at most 1,000 items.",
        )?;
        ensure(
            !self.reference.iter().any(|existing| existing.id == item.id),
            "This reference item already exists.",
        )?;
        self.reference.push(item);
        Ok(())
    }

    fn update_reference_item(
        &mut self,
        item_id: Uuid,
        rect: Option<Rect>,
        text: Option<String>,
        url: Option<String>,
        title: Option<String>,
        color: Option<String>,
    ) -> Result<()> {
        let has_content = text.is_some() || url.is_some() || title.is_some() || color.is_some();
        let item = self.reference_mut(item_id)?;
        if let Some(rect) = rect {
            if (rect.width != item.rect.width || rect.height != item.rect.height)
                && let ReferenceKind::Element { element, size } = &mut item.kind
            {
                let resized = Rect {
                    width: rect.width,
                    height: rect.height,
                    ..element.rect
                };
                if let Some(size) = size {
                    let original = element
                        .overrides
                        .get(size)
                        .and_then(|over| over.rect)
                        .unwrap_or(element.rect);
                    element.overrides.entry(*size).or_default().rect = Some(Rect {
                        width: rect.width,
                        height: rect.height,
                        ..original
                    });
                } else {
                    element.rect = resized;
                }
            }
            item.rect = rect;
        }
        if !has_content {
            return Ok(());
        }
        match &mut item.kind {
            ReferenceKind::Note { text: value } => {
                ensure(
                    url.is_none() && title.is_none() && color.is_none(),
                    "This reference item is not a link or a swatch.",
                )?;
                if let Some(next) = text {
                    *value = next;
                }
            }
            ReferenceKind::Link {
                url: link,
                title: name,
            } => {
                ensure(
                    text.is_none() && color.is_none(),
                    "This reference item is not a note or a swatch.",
                )?;
                if let Some(next) = url {
                    *link = next;
                }
                if let Some(next) = title {
                    *name = next;
                }
            }
            ReferenceKind::Swatch { color: value } => {
                ensure(
                    text.is_none() && url.is_none() && title.is_none(),
                    "This reference item is not a note or a link.",
                )?;
                if let Some(next) = color {
                    *value = normalized_swatch(&next);
                }
            }
            ReferenceKind::Image { .. } | ReferenceKind::Element { .. } => {
                return Err(Error(
                    "Move this reference layer into a boundary to edit its content.".into(),
                ));
            }
        }
        Ok(())
    }

    /// Moving a layer into reference preserves its complete editable tree. Requests follow
    /// their targets into reference, rather than turning layer names into prompts.
    fn element_to_reference(
        &mut self,
        page_id: Uuid,
        element_id: Uuid,
        rect: Rect,
        size_id: Option<Uuid>,
    ) -> Result<()> {
        ensure(
            rect.width >= 0.0 && rect.height >= 0.0,
            "Layer dimensions cannot be negative.",
        )?;
        let page = self.page_mut(page_id)?;
        ensure(
            size_id.is_none_or(|id| page.sizes.iter().any(|size| size.id == id)),
            "This boundary size no longer exists.",
        )?;
        let size_id = size_id.filter(|id| *id != page.base_size_id());
        let element = crate::elements::remove_element(&mut page.elements, element_id)
            .ok_or_else(|| Error("This layer no longer exists.".into()))?;
        let item_id = Uuid::new_v4();
        fn contains(element: &crate::Element, id: Uuid) -> bool {
            element.id == id || element.children.iter().any(|child| contains(child, id))
        }
        for request in &mut self.requests {
            for target in &mut request.targets {
                if matches!(target, RequestTarget::Element { id, .. } if contains(&element, *id)) {
                    *target = RequestTarget::Reference { id: item_id };
                }
            }
            request.targets.dedup();
        }
        self.reference.push(ReferenceItem {
            id: item_id,
            rect: Rect {
                width: rect.width.max(1.0),
                height: rect.height.max(1.0),
                ..rect
            },
            kind: ReferenceKind::Element {
                element: Box::new(element),
                size: size_id,
            },
        });
        Ok(())
    }

    /// Selected reference items become request attachments on these pending requests.
    fn attach_reference_context(&mut self, request_ids: &[Uuid], item_ids: &[Uuid]) -> Result<()> {
        ensure(!request_ids.is_empty(), "Write a request first.")?;
        let contexts: Vec<(
            String,
            crate::AttachmentKind,
            Option<String>,
            Option<String>,
        )> = item_ids
            .iter()
            .map(|id| {
                let item = self
                    .reference
                    .iter()
                    .find(|item| item.id == *id)
                    .ok_or_else(|| Error("This reference item no longer exists.".into()))?;
                Ok(match &item.kind {
                    ReferenceKind::Element { element, .. } => (element.label(), crate::AttachmentKind::Text, Some(crate::requirements::reference_element_value(element).to_string()), None),
                    ReferenceKind::Note { text } => (
                        "Reference note".to_string(),
                        crate::AttachmentKind::Text,
                        Some(text.clone()),
                        None,
                    ),
                    ReferenceKind::Link { url, title } => (
                        if title.trim().is_empty() {
                            url.clone()
                        } else {
                            title.clone()
                        },
                        crate::AttachmentKind::Link,
                        None,
                        Some(url.clone()),
                    ),
                    // Reference pictures travel by name only; their bytes never leave the project.
                    ReferenceKind::Image { picture, name } => (
                        reference_picture_name(*picture, name),
                        crate::AttachmentKind::Text,
                        Some(reference_picture_name(*picture, name)),
                        None,
                    ),
                    ReferenceKind::Swatch { color } => (
                        "Reference colour".to_string(),
                        crate::AttachmentKind::Text,
                        Some(color.clone()),
                        None,
                    ),
                })
            })
            .collect::<Result<_>>()?;
        for request_id in request_ids {
            let request = self
                .requests
                .iter_mut()
                .find(|request| request.id == *request_id)
                .ok_or_else(|| Error("This request no longer exists.".into()))?;
            let mut used: BTreeSet<String> = request
                .attachments
                .iter()
                .map(|item| item.id.clone())
                .collect();
            let mut additions = Vec::new();
            for (name, kind, text, url) in &contexts {
                // An empty note carries no reference.
                if *kind == crate::AttachmentKind::Text && text.as_deref().is_none_or(str::is_empty)
                {
                    continue;
                }
                let duplicate =
                    request
                        .attachments
                        .iter()
                        .chain(additions.iter())
                        .any(|existing| {
                            existing.kind == *kind && existing.text == *text && existing.url == *url
                        });
                if duplicate {
                    continue;
                }
                let id = (1..)
                    .map(|n| format!("a{n}"))
                    .find(|id| !used.contains(id))
                    .expect("an unused identifier");
                used.insert(id.clone());
                additions.push(crate::RequestAttachment {
                    id,
                    name: name.chars().take(120).collect(),
                    kind: *kind,
                    data_uri: None,
                    preview: None,
                    text: text.clone(),
                    url: url.clone(),
                });
            }
            ensure(
                request.attachments.len() + additions.len() <= 16,
                "A request supports at most 16 attachments.",
            )?;
            request.attachments.extend(additions);
        }
        Ok(())
    }

    fn add_request(&mut self, request: PendingRequest) -> Result<()> {
        request.validate()?;
        ensure(
            self.requests.len() < MAX_REQUESTS,
            "A project holds at most 256 pending requests.",
        )?;
        ensure(
            !self
                .requests
                .iter()
                .any(|existing| existing.id == request.id),
            "This request already exists.",
        )?;
        self.requests.push(request);
        Ok(())
    }

    fn update_request(&mut self, request: PendingRequest) -> Result<()> {
        request.validate()?;
        let existing = self
            .requests
            .iter_mut()
            .find(|existing| existing.id == request.id)
            .ok_or_else(|| Error("This request no longer exists.".into()))?;
        *existing = request;
        Ok(())
    }

    fn remove_requests(&mut self, ids: &[Uuid]) -> Result<()> {
        ensure(!ids.is_empty(), "Select a request first.")?;
        for id in ids {
            ensure(
                self.requests.iter().any(|request| request.id == *id),
                "This request no longer exists.",
            )?;
        }
        self.requests.retain(|request| !ids.contains(&request.id));
        Ok(())
    }

    /// Restore a stored layer without adding a request. Other reference cards become layers
    /// with draft requests carrying their content; only an explicit Generate action runs them.
    fn reference_to_element(
        &mut self,
        item_id: Uuid,
        page_id: Uuid,
        rect: Rect,
        size_id: Option<Uuid>,
    ) -> Result<()> {
        let item = self
            .reference
            .iter()
            .position(|item| item.id == item_id)
            .map(|index| self.reference.remove(index))
            .ok_or_else(|| Error("This reference item no longer exists.".into()))?;
        let attachment = |id: String,
                          name: String,
                          kind: crate::AttachmentKind,
                          data_uri: Option<std::sync::Arc<str>>,
                          url: Option<String>| crate::RequestAttachment {
            id,
            name,
            kind,
            data_uri,
            preview: None,
            text: None,
            url,
        };
        let mut request: Option<(String, Vec<crate::RequestAttachment>)> = None;
        let restored = matches!(&item.kind, ReferenceKind::Element { .. });
        let mut element = match item.kind {
            ReferenceKind::Element { element, size } => {
                let mut element = *element;
                let page = self.page(page_id)?;
                // On a different page, carry the visible design into the destination rather
                // than leaving overrides tied to sizes that page does not own.
                fn rebase(element: &mut crate::Element, sizes: &[PageSize], source: Option<Uuid>) {
                    if element
                        .overrides
                        .keys()
                        .any(|id| !sizes.iter().any(|size| size.id == *id))
                    {
                        if let Some(over) = source.and_then(|id| element.overrides.get(&id)) {
                            if let Some(rect) = over.rect {
                                element.rect = rect;
                            }
                            if let Some(style) = &over.style {
                                element.style = style.clone();
                            }
                            if let Some(layout) = &over.layout {
                                element.layout = layout.clone();
                            }
                        }
                        element
                            .overrides
                            .retain(|id, _| sizes.iter().any(|size| size.id == *id));
                    }
                    for child in &mut element.children {
                        rebase(child, sizes, source);
                    }
                }
                rebase(&mut element, &page.sizes, size);
                element
            }
            ReferenceKind::Image { picture, .. } => {
                let mut element = crate::empty_element(crate::ElementKind::Image, rect);
                element.image = Some(picture);
                element
            }
            ReferenceKind::Note { text } => {
                let element = crate::empty_element(crate::ElementKind::Box, rect);
                if !text.trim().is_empty() {
                    request = Some((text, vec![]));
                }
                element
            }
            ReferenceKind::Link { url, title } => {
                let name: String = if title.trim().is_empty() {
                    url.clone()
                } else {
                    title
                }
                .chars()
                .take(120)
                .collect();
                let element = crate::empty_element(crate::ElementKind::Box, rect);
                request = Some((
                    String::new(),
                    vec![attachment(
                        attachment_id(),
                        name,
                        crate::AttachmentKind::Link,
                        None,
                        Some(url),
                    )],
                ));
                element
            }
            ReferenceKind::Swatch { color } => {
                let element = crate::empty_element(crate::ElementKind::Box, rect);
                request = Some((format!("Use {color}"), vec![]));
                element
            }
        };
        let page = self.page(page_id)?;
        ensure(
            size_id.is_none_or(|id| page.sizes.iter().any(|size| size.id == id)),
            "This boundary size no longer exists.",
        )?;
        if restored {
            if let Some(size) = size_id.filter(|id| *id != page.base_size_id()) {
                let original = element
                    .overrides
                    .get(&size)
                    .and_then(|over| over.rect)
                    .unwrap_or(element.rect);
                element.overrides.entry(size).or_default().rect = Some(Rect {
                    x: rect.x,
                    y: rect.y,
                    ..original
                });
            } else {
                element.rect.x = rect.x;
                element.rect.y = rect.y;
            }
        } else {
            element.rect = rect;
        }
        let page = self.page_mut(page_id)?;
        page.elements.push(element.clone());
        let target_size = size_id.or(Some(page.base_size_id()));
        for request in &mut self.requests {
            for target in &mut request.targets {
                if matches!(target, RequestTarget::Reference { id } if *id == item_id) {
                    *target = RequestTarget::Element {
                        page: page_id,
                        size: target_size,
                        id: element.id,
                    };
                }
            }
        }
        if let Some((text, attachments)) = request {
            let request = PendingRequest {
                id: Uuid::new_v4(),
                text,
                attachments,
                targets: vec![RequestTarget::Element {
                    page: page_id,
                    size: target_size,
                    id: element.id,
                }],
                area: None,
                requested_at: "reference".into(),
            };
            self.add_request(request)?;
        }
        Ok(())
    }

    /// Drop request targets that no longer exist, and requests left with nothing to act on.
    /// Any boundary can be deleted; the first remaining included one becomes the entry boundary.
    pub(crate) fn rehome(&mut self) {
        if self.pages.iter().any(|page| page.id == self.home_page_id) {
            return;
        }
        if let Some(page) = self.pages.iter().find(|page| page.included) {
            self.home_page_id = page.id;
        } else if let Some(page) = self.pages.first_mut() {
            page.included = true;
            self.home_page_id = page.id;
        }
    }

    pub(crate) fn prune_requests(&mut self) {
        let pages: BTreeMap<Uuid, &Page> = self.pages.iter().map(|page| (page.id, page)).collect();
        let references: BTreeSet<Uuid> = self.reference.iter().map(|item| item.id).collect();
        let pointers: BTreeSet<Uuid> = self.pointers.iter().map(|pointer| pointer.id).collect();
        self.requests.retain_mut(|request| {
            request.targets.retain(|target| match target {
                RequestTarget::Element { page, size, id } => pages.get(page).is_some_and(|page| {
                    size.is_none_or(|size| page.sizes.iter().any(|item| item.id == size))
                        && crate::elements::find_in(&page.elements, *id).is_some()
                }),
                RequestTarget::Boundary { page, size } => pages.get(page).is_some_and(|page| {
                    size.is_none_or(|size| page.sizes.iter().any(|item| item.id == size))
                }),
                RequestTarget::Reference { id } => references.contains(id),
                RequestTarget::Pointer { id } => pointers.contains(id),
            });
            if let Some(area) = &request.area
                && area.page.is_some_and(|page| !pages.contains_key(&page))
            {
                request.area = None;
            }
            !request.targets.is_empty() || request.area.is_some()
        });
    }

    pub fn pending_count(&self) -> usize {
        self.pending_request_count()
    }

    pub fn validate(&self) -> Result<()> {
        ensure(
            self.schema_version == SCHEMA_VERSION,
            "This project uses an unsupported schema version. The original has not been changed.",
        )?;
        validate_text(&self.name, "Project name", 120, true)?;
        self.viewport.validate()?;
        self.grid_settings.validate()?;
        self.styles.validate()?;
        ensure(
            self.requests.len() <= MAX_REQUESTS,
            "A project holds at most 256 pending requests.",
        )?;
        let mut request_ids = BTreeSet::new();
        for request in &self.requests {
            ensure(
                request_ids.insert(request.id),
                "Request identifiers must be unique.",
            )?;
            request.validate()?;
            for target in &request.targets {
                match target {
                    RequestTarget::Element { page, size, id } => {
                        let page = self.page(*page)?;
                        ensure(
                            page.sizes
                                .iter()
                                .any(|item| size.is_none_or(|size| size == item.id)),
                            "A request target names a size this boundary does not have.",
                        )?;
                        ensure(
                            crate::elements::find_in(&page.elements, *id).is_some(),
                            "A request targets a layer that no longer exists.",
                        )?;
                    }
                    RequestTarget::Boundary { page, size } => {
                        let page = self.page(*page)?;
                        ensure(
                            page.sizes
                                .iter()
                                .any(|item| size.is_none_or(|size| size == item.id)),
                            "A request target names a size this boundary does not have.",
                        )?;
                    }
                    RequestTarget::Reference { id } => ensure(
                        self.reference.iter().any(|item| item.id == *id),
                        "A request targets a reference item that no longer exists.",
                    )?,
                    RequestTarget::Pointer { id } => ensure(
                        self.pointers.iter().any(|pointer| pointer.id == *id),
                        "A request targets a pointer that no longer exists.",
                    )?,
                }
            }
            if let Some(area) = &request.area
                && let Some(page_id) = area.page
            {
                let page = self.page(page_id)?;
                ensure(
                    area.size
                        .is_none_or(|size| page.sizes.iter().any(|item| item.id == size)),
                    "A request area names a size this boundary does not have.",
                )?;
            }
        }
        ensure(
            self.views.len() <= MAX_NAMED_VIEWS,
            "A project holds at most 50 named views.",
        )?;
        let mut view_ids = BTreeSet::new();
        for view in &self.views {
            ensure(
                view_ids.insert(view.id),
                "View identifiers must be unique.",
            )?;
            view.validate()?;
        }
        ensure(
            self.pages.len() <= MAX_PAGES,
            "A project contains at most 100 pages.",
        )?;
        let mut ids = BTreeSet::from([self.id]);
        if let Some(identity) = &self.imported_identity {
            identity.validate()?;
            for asset in &identity.assets {
                ensure(
                    ids.insert(asset.id),
                    "Identity asset identifiers must be unique.",
                )?;
            }
        }
        if let Some(identity) = &self.imported_identity {
            for photo in &identity.photos {
                ensure(
                    ids.insert(photo.id),
                    "Project photo identifiers must be unique.",
                )?;
            }
        }
        for id in self.pictures.keys() {
            ensure(
                ids.insert(*id),
                "Project picture identifiers must be unique.",
            )?;
        }
        ensure(
            self.reference.len() <= MAX_REFERENCE_ITEMS,
            "A reference holds at most 1,000 items.",
        )?;
        for item in &self.reference {
            ensure(ids.insert(item.id), "Project identifiers must be unique.")?;
            item.validate(self)?;
        }
        ensure(
            self.pointers.len() <= MAX_POINTERS,
            "A project holds at most 1,000 pointers.",
        )?;
        for pointer in &self.pointers {
            ensure(
                ids.insert(pointer.id),
                "Project identifiers must be unique.",
            )?;
            pointer.validate()?;
        }
        ensure(
            self.linked.len() <= crate::MAX_LINKED_DEFINITIONS,
            "A project holds at most 1,000 linked elements.",
        )?;
        let mut linked_names = BTreeSet::new();
        for (id, definition) in &self.linked {
            ensure(
                !id.is_nil() && definition.id == *id && ids.insert(*id),
                "Linked element identifiers must be nonempty and unique.",
            )?;
            validate_text(
                &definition.name,
                "Linked element name",
                crate::MAX_ELEMENT_NAME_CHARS,
                true,
            )?;
            ensure(
                linked_names.insert(definition.name.trim().to_lowercase()),
                "Two linked elements cannot share a name.",
            )?;
            definition.validate(self)?;
        }
        self.validate_picture_store()?;
        let mut paths = BTreeSet::new();
        let mut element_count = 0_usize;
        for page in &self.pages {
            ensure(ids.insert(page.id), "Project identifiers must be unique.")?;
            validate_text(&page.name, "Page name", 120, true)?;
            ensure(valid_swatch(&page.background), "Boundary background must be a hex colour.")?;
            validate_path(&page.path)?;
            ensure(
                paths.insert(page.path.to_lowercase()),
                "Two pages cannot use the same public path.",
            )?;
            ensure(
                !page.sizes.is_empty() && page.sizes.len() <= MAX_PAGE_SIZES,
                "A boundary needs between 1 and 8 sizes.",
            )?;
            let mut size_ids = BTreeSet::new();
            for size in &page.sizes {
                ensure(
                    ids.insert(size.id) && size_ids.insert(size.id),
                    "Size identifiers must be nonempty and unique.",
                )?;
                size.validate()?;
            }
            crate::elements::validate_elements(
                &page.elements,
                self,
                &size_ids,
                &mut ids,
                &mut element_count,
            )?;
        }
        for item in &self.reference {
            if let ReferenceKind::Element { element, .. } = &item.kind {
                fn sizes(element: &crate::Element, ids: &mut BTreeSet<Uuid>) {
                    ids.extend(element.overrides.keys().copied());
                    for child in &element.children { sizes(child, ids); }
                }
                let mut size_ids = BTreeSet::new();
                sizes(element, &mut size_ids);
                crate::elements::validate_elements(std::slice::from_ref(element.as_ref()), self, &size_ids, &mut ids, &mut element_count)?;
            }
        }
        if !self.pages.is_empty() {
            // The entry points at a real boundary, but any included boundary may be the entry.
            self.page(self.home_page_id)?;
            ensure(
                self.pages.iter().any(|page| page.included),
                "At least one boundary must be included.",
            )?;
        }
        for text in [
            &self.brief.purpose,
            &self.brief.audience,
            &self.brief.brand_name,
            &self.brief.tone,
        ] {
            validate_text(text, "Brief field", MAX_PROMPT_CHARS, false)?;
        }
        let mut topics = BTreeSet::new();
        for decision in &self.decisions {
            ensure(
                ids.insert(decision.id),
                "Decision identifiers must be unique.",
            )?;
            self.validate_scope(&decision.scope)?;
            validate_text(&decision.topic, "Decision topic", 120, true)?;
            validate_text(&decision.value, "Decision value", MAX_PROMPT_CHARS, true)?;
            ensure(
                decision.accepted_at_revision <= self.revision,
                "A decision cannot come from a future revision.",
            )?;
            let topic = format!(
                "{}:{}",
                serde_json::to_string(&decision.scope).map_err(|e| Error(e.to_string()))?,
                decision.topic.to_lowercase()
            );
            ensure(
                topics.insert(topic),
                "A scope cannot have duplicate decision topics.",
            )?;
        }
        for exception in &self.exceptions {
            ensure(
                ids.insert(exception.id),
                "Exception identifiers must be unique.",
            )?;
            self.validate_scope(&exception.scope)?;
            validate_text(&exception.rule, "Exception rule", 120, true)?;
            validate_text(
                &exception.reason,
                "Exception reason",
                MAX_PROMPT_CHARS,
                true,
            )?;
            ensure(
                exception.accepted_at_revision <= self.revision,
                "An exception cannot come from a future revision.",
            )?;
        }
        for (namespace, record) in &self.extensions {
            ensure(
                !namespace.is_empty()
                    && namespace.len() <= 128
                    && namespace
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b)),
                "Extension namespaces must be stable ASCII identifiers.",
            )?;
            ensure(
                record.schema_version > 0,
                "Extension data must declare a schema version.",
            )?;
        }
        ensure(
            self.applied_job_ids.len() <= MAX_APPLIED_JOBS,
            "The project has reached its applied generation job limit.",
        )?;
        let mut jobs = BTreeSet::new();
        for job_id in &self.applied_job_ids {
            ensure(
                !job_id.is_nil() && jobs.insert(*job_id),
                "Applied generation job identifiers must be nonempty and unique.",
            )?;
        }
        if let Some(dossier) = &self.latest_generation {
            dossier.validate(self)?;
        }
        if let Some(record) = &self.last_export {
            record.validate()?;
        }
        Ok(())
    }

    pub fn validate_scope(&self, scope: &Scope) -> Result<()> {
        match scope {
            Scope::Project => Ok(()),
            Scope::Page(id) => self.page(*id).map(|_| ()),
            Scope::Element(id) => ensure(
                self.element(*id).is_some(),
                "This decision's layer no longer exists.",
            ),
        }
    }

    pub fn apply(&self, expected_revision: u64, command: Command) -> Result<Self> {
        ensure(
            expected_revision == self.revision,
            "The project changed. Refresh the document before applying this edit.",
        )?;
        let mut next = self.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        match command {
            Command::AddColourStyle { style } => {
                ensure(
                    !next.styles.colours.iter().any(|s| s.id == style.id),
                    "This style already exists.",
                )?;
                next.styles.colours.push(style);
            }
            Command::UpdateColourStyle { style } => {
                let id = style.id;
                *next
                    .styles
                    .colours
                    .iter_mut()
                    .find(|s| s.id == id)
                    .ok_or_else(|| Error("This style no longer exists.".into()))? = style;
            }
            Command::RemoveColourStyle { id } => {
                next.detach_style(id, false)?;
                next.styles.colours.retain(|s| s.id != id);
            }
            Command::AddTextStyle { style } => {
                ensure(
                    !next.styles.text.iter().any(|s| s.id == style.id),
                    "This style already exists.",
                )?;
                next.styles.text.push(style);
            }
            Command::UpdateTextStyle { style } => {
                let id = style.id;
                *next
                    .styles
                    .text
                    .iter_mut()
                    .find(|s| s.id == id)
                    .ok_or_else(|| Error("This style no longer exists.".into()))? = style;
            }
            Command::RemoveTextStyle { id } => {
                next.detach_style(id, true)?;
                next.styles.text.retain(|s| s.id != id);
            }
            Command::RenameProject { name } => next.name = name.trim().into(),
            Command::SetGrid { enabled } => next.show_grid = enabled,
            Command::SetGridSettings { settings } => next.grid_settings = settings,
            Command::SetGenerationMode { mode } => next.generation_mode = mode,
            Command::UpdatePage {
                page_id,
                name,
                path,
                background,
                included,
            } => {
                let p = next.page_mut(page_id)?;
                p.name = name.trim().into();
                p.path = path;
                ensure(valid_swatch(&background), "Boundary background must be a hex colour.")?;
                p.background = normalized_swatch(&background);
                p.included = included;
            }
            Command::SetPageOutput { page_id, output } => {
                next.page_mut(page_id)?.output = output;
            }
            Command::UpdatePageSize {
                page_id,
                size_id,
                label,
                width,
                height,
                kind,
                moves,
                reference,
            } => {
                let p = next.page_mut(page_id)?;
                let size = p
                    .size_mut(size_id)
                    .ok_or_else(|| Error("This size no longer exists.".into()))?;
                if let Some(label) = label {
                    size.label = label.trim().into();
                }
                if let Some(width) = width {
                    size.width = width;
                }
                if let Some(height) = height {
                    size.height = height;
                }
                if let Some(kind) = kind {
                    size.kind = kind;
                }
                size.validate()?;
                ensure(
                    p.viewport().validate().is_ok(),
                    "The base size must stay a valid viewport.",
                )?;
                for placement in moves {
                    placement.place(&mut next.pages)?;
                }
                next.place_reference(reference)?;
            }
            Command::CreateBoundary {
                page,
                after_page_id,
                moves,
                reference,
            } => {
                ensure(
                    !next.pages.iter().any(|existing| existing.id == page.id),
                    "This boundary already exists.",
                )?;
                let index = if let Some(id) = after_page_id {
                    next.pages.iter().position(|existing| existing.id == id)
                        .ok_or_else(|| Error("This boundary no longer exists.".into()))? + 1
                } else {
                    next.pages.len()
                };
                if next.pages.is_empty() {
                    next.home_page_id = page.id;
                }
                next.pages.insert(index, page);
                for placement in moves {
                    placement.place(&mut next.pages)?;
                }
                next.place_reference(reference)?;
            }
            Command::AddCanvasPage {
                name,
                path,
                size,
                moves,
                reference,
            } => {
                ensure(
                    !next
                        .pages
                        .iter()
                        .any(|page| page.sizes.iter().any(|item| item.id == size.id)),
                    "This size already exists.",
                )?;
                let page_id = Uuid::new_v4();
                if next.pages.is_empty() {
                    next.home_page_id = page_id;
                }
                next.pages.push(Page {
                    id: page_id,
                    name: name.trim().into(),
                    path: if next.pages.is_empty() && path != "/" {
                        "/".into()
                    } else {
                        path
                    },
                    included: true,
                    output: OutputType::default(),
                    sizes: vec![size],
                    background: "#ffffff".into(),
                    elements: vec![],
                });
                for placement in moves {
                    placement.place(&mut next.pages)?;
                }
                next.place_reference(reference)?;
            }
            Command::AddPageSize {
                page_id,
                size,
                moves,
                reference,
            } => {
                ensure(
                    !next
                        .pages
                        .iter()
                        .any(|page| page.sizes.iter().any(|item| item.id == size.id)),
                    "This size already exists.",
                )?;
                let page = next.page_mut(page_id)?;
                ensure(
                    page.sizes.len() < MAX_PAGE_SIZES,
                    "A boundary holds at most 8 sizes.",
                )?;
                size.validate()?;
                page.sizes.push(size);
                for placement in moves {
                    placement.place(&mut next.pages)?;
                }
                next.place_reference(reference)?;
            }
            Command::RemovePageSize { page_id, size_id } => {
                let page = next.page_mut(page_id)?;
                ensure(page.sizes.len() > 1, "A boundary keeps at least one size.")?;
                ensure(
                    page.base_size_id() != size_id,
                    "The base size cannot be removed.",
                )?;
                ensure(page.size(size_id).is_some(), "This size no longer exists.")?;
                page.sizes.retain(|size| size.id != size_id);
                for element in page.elements.iter_mut() {
                    clear_size_overrides(element, size_id);
                }
                for definition in next.linked.values_mut() {
                    clear_size_overrides(&mut definition.master, size_id);
                }
                next.prune_requests();
            }
            Command::SetCanvasLayout {
                placements,
                reference,
            } => {
                for placement in placements {
                    placement.place(&mut next.pages)?;
                }
                next.place_reference(reference)?;
            }
            Command::AddNamedView {
                name,
                x,
                y,
                zoom,
            } => {
                ensure(
                    next.views.len() < MAX_NAMED_VIEWS,
                    "A project holds at most 50 named views.",
                )?;
                let view = NamedView {
                    id: Uuid::new_v4(),
                    name: name.trim().into(),
                    x,
                    y,
                    zoom,
                };
                view.validate()?;
                next.views.push(view);
            }
            Command::UpdateNamedView {
                view_id,
                name,
                x,
                y,
                zoom,
            } => {
                let view = next
                    .views
                    .iter_mut()
                    .find(|view| view.id == view_id)
                    .ok_or_else(|| Error("This view no longer exists.".into()))?;
                if let Some(name) = name {
                    view.name = name.trim().into();
                }
                if let Some(x) = x {
                    view.x = x;
                }
                if let Some(y) = y {
                    view.y = y;
                }
                if let Some(zoom) = zoom {
                    view.zoom = zoom;
                }
                view.validate()?;
            }
            Command::RemoveNamedView { view_id } => {
                ensure(
                    next.views.iter().any(|view| view.id == view_id),
                    "This view no longer exists.",
                )?;
                next.views.retain(|view| view.id != view_id);
            }
            Command::AddReferenceItem {
                item,
                picture_data_uri,
            } => next.add_reference_item(item, picture_data_uri)?,
            Command::UpdateReferenceItem {
                item_id,
                rect,
                text,
                url,
                title,
                color,
            } => next.update_reference_item(item_id, rect, text, url, title, color)?,
            Command::RemoveReferenceItems { ids } => {
                ensure(!ids.is_empty(), "Choose a reference item to remove.")?;
                for id in &ids {
                    ensure(
                        next.reference.iter().any(|item| item.id == *id),
                        "This reference item no longer exists.",
                    )?;
                }
                next.reference.retain(|item| !ids.contains(&item.id));
                next.prune_requests();
            }
            Command::AddPointer { pointer } => next.add_pointer(pointer)?,
            Command::UpdatePointer {
                pointer_id,
                from,
                to,
                comment,
                kind,
            } => next.update_pointer(pointer_id, from, to, comment, kind)?,
            Command::RemovePointers { ids } => {
                ensure(!ids.is_empty(), "Choose a pointer to remove.")?;
                for id in &ids {
                    ensure(
                        next.pointers.iter().any(|pointer| pointer.id == *id),
                        "This pointer no longer exists.",
                    )?;
                }
                next.pointers.retain(|pointer| !ids.contains(&pointer.id));
                next.prune_requests();
            }
            Command::SetPointerWiring {
                pointer_id,
                wiring,
                comment,
            } => {
                if let Some(wiring) = &wiring
                    && let Some(record) = &mut next.last_export
                {
                    record.pointers.insert(
                        pointer_id,
                        ExportedPointer {
                            fingerprint: wiring.fingerprint.clone(),
                            comment: wiring.generated_comment.clone(),
                        },
                    );
                }
                let pointer = next.pointer_mut(pointer_id)?;
                if let Some(comment) = comment {
                    pointer.comment = comment;
                }
                pointer.wiring = wiring;
            }
            Command::ResolvePointerClash {
                keep,
                drop,
                merged_comment,
            } => {
                ensure(!drop.is_empty(), "Choose the other pointer to resolve.")?;
                ensure(!drop.contains(&keep), "Keep one of the two pointers.")?;
                next.pointer_mut(keep)?;
                for id in &drop {
                    ensure(
                        next.pointers.iter().any(|pointer| pointer.id == *id),
                        "This pointer no longer exists.",
                    )?;
                }
                let pointer = next.pointer_mut(keep)?;
                if let Some(comment) = merged_comment {
                    pointer.comment = comment;
                    pointer.wiring = None;
                }
                next.pointers.retain(|pointer| !drop.contains(&pointer.id));
            }
            Command::RecordExport { record } => {
                record.validate()?;
                for pointer in &mut next.pointers {
                    if let Some(exported) = record.pointers.get(&pointer.id) {
                        pointer.wiring = Some(PointerWiring {
                            fingerprint: exported.fingerprint.clone(),
                            generated_comment: exported.comment.clone(),
                        });
                    }
                }
                next.last_export = Some(record);
            }
            Command::ForgetExportPointer { pointer_id } => {
                next.pointer_mut(pointer_id)?;
                if let Some(record) = &mut next.last_export {
                    record.pointers.remove(&pointer_id);
                }
                next.pointer_mut(pointer_id)?.wiring = None;
            }
            Command::ElementToReference {
                page_id,
                element_id,
                rect,
                size_id,
            } => next.element_to_reference(page_id, element_id, rect, size_id)?,
            Command::ReferenceToElement {
                item_id,
                page_id,
                rect,
                size_id,
            } => next.reference_to_element(item_id, page_id, rect, size_id)?,
            Command::AttachReferenceContext {
                request_ids,
                item_ids,
            } => next.attach_reference_context(&request_ids, &item_ids)?,
            Command::DeletePage { page_id } => {
                next.page(page_id)?;
                next.pages.retain(|p| p.id != page_id);
                next.rehome();
                next.prune_requests();
            }
            Command::AddRequest { request } => next.add_request(request)?,
            Command::UpdateRequest { request } => next.update_request(request)?,
            Command::RemoveRequests { ids } => next.remove_requests(&ids)?,
            Command::ImportWebsite {
                pages,
                reference,
                pointers,
                pictures,
            } => {
                for (id, uri) in pictures {
                    ensure(
                        crate::project_assets::store_picture_uri(&uri)? == id,
                        "An imported picture failed its integrity check.",
                    )?;
                    next.pictures.insert(id, std::sync::Arc::from(uri.as_str()));
                }
                let mut paths: BTreeSet<String> = next
                    .pages
                    .iter()
                    .map(|page| page.path.to_ascii_lowercase())
                    .collect();
                let first = next.pages.is_empty();
                for (index, page) in pages.into_iter().enumerate() {
                    ensure(
                        !next.pages.iter().any(|existing| existing.id == page.id),
                        "This imported boundary already exists.",
                    )?;
                    ensure(
                        paths.insert(page.path.to_ascii_lowercase()),
                        "Two imported pages cannot use the same public path.",
                    )?;
                    if first && index == 0 {
                        next.home_page_id = page.id;
                    }
                    next.pages.push(page);
                }
                next.reference.extend(reference);
                next.pointers.extend(pointers);
            }
            Command::InsertElements {
                page_id,
                parent_id,
                index,
                elements,
                picture_data_uri,
                size_id,
            } => next.insert_elements(
                page_id,
                parent_id,
                index,
                elements,
                picture_data_uri,
                size_id,
            )?,
            Command::UpdateElements { page_id, patches } => {
                next.update_elements(page_id, patches)?
            }
            Command::LinkElements { ids, source_id } => next.link_elements(ids, source_id)?,
            Command::UnlinkElements { ids } => next.unlink_elements(ids)?,
            Command::RemoveLinkedDefinition { id } => next.remove_linked_definition(id)?,
            Command::PasteLinkedInstances { items } => next.paste_linked_instances(items)?,
            Command::MoveElements { moves } => next.move_elements(moves)?,
            Command::RemoveElements { page_id, ids } => next.remove_elements(page_id, ids)?,
            Command::DeleteItems {
                elements,
                reference_ids,
                pointer_ids,
                request_ids,
                page_ids,
                size_ids,
                linked_ids,
            } => {
                // Explicitly selected requests go first, so removing their targets in the same
                // gesture cannot prune them out from under the request check.
                for id in request_ids {
                    ensure(
                        next.requests.iter().any(|request| request.id == id),
                        "This request no longer exists.",
                    )?;
                    next.requests.retain(|request| request.id != id);
                }
                let mut by_page: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
                for (page_id, element_id) in elements {
                    next.page(page_id)?;
                    ensure(
                        crate::elements::find_in(&next.page(page_id)?.elements, element_id)
                            .is_some(),
                        "This layer no longer exists.",
                    )?;
                    by_page.entry(page_id).or_default().push(element_id);
                }
                for (page_id, ids) in by_page {
                    next.remove_elements(page_id, ids)?;
                }
                for id in reference_ids {
                    ensure(
                        next.reference.iter().any(|item| item.id == id),
                        "This reference item no longer exists.",
                    )?;
                    next.reference.retain(|item| item.id != id);
                }
                for id in pointer_ids {
                    ensure(
                        next.pointers.iter().any(|pointer| pointer.id == id),
                        "This pointer no longer exists.",
                    )?;
                    next.pointers.retain(|pointer| pointer.id != id);
                }
                for id in linked_ids {
                    next.delete_linked_definition(id)?;
                }
                for (page_id, size_id) in size_ids {
                    let page = next.page_mut(page_id)?;
                    ensure(page.sizes.len() > 1, "A boundary keeps at least one size.")?;
                    ensure(
                        page.base_size_id() != size_id,
                        "The base size cannot be removed.",
                    )?;
                    ensure(page.size(size_id).is_some(), "This size no longer exists.")?;
                    page.sizes.retain(|size| size.id != size_id);
                    for element in page.elements.iter_mut() {
                        clear_size_overrides(element, size_id);
                    }
                    for definition in next.linked.values_mut() {
                        clear_size_overrides(&mut definition.master, size_id);
                    }
                }
                for page_id in page_ids {
                    ensure(
                        next.pages.iter().any(|page| page.id == page_id),
                        "This boundary no longer exists.",
                    )?;
                    next.pages.retain(|page| page.id != page_id);
                }
                next.rehome();
                next.prune_requests();
            }
            Command::GroupElements { page_id, ids } => next.group_elements(page_id, ids)?,
            Command::UngroupElements { page_id, id } => next.ungroup_elements(page_id, id)?,
            Command::UpdateElementContext {
                element_id,
                context,
            } => next.update_element_context(element_id, context)?,
            Command::SetBrief { brief } => next.brief = brief,
            Command::SetDecision {
                topic,
                value,
                scope,
                protected,
            } => {
                if let Some(existing) = next
                    .decisions
                    .iter_mut()
                    .find(|d| d.scope == scope && d.topic.eq_ignore_ascii_case(topic.trim()))
                {
                    existing.value = value.trim().into();
                    existing.protected = protected;
                    existing.accepted_at_revision = next.revision;
                } else {
                    next.decisions.push(Decision {
                        id: Uuid::new_v4(),
                        topic: topic.trim().into(),
                        value: value.trim().into(),
                        scope,
                        protected,
                        accepted_at_revision: next.revision,
                    });
                }
            }
            Command::RemoveDecision { decision_id } => {
                ensure(
                    next.decisions.iter().any(|d| d.id == decision_id),
                    "This decision no longer exists.",
                )?;
                next.decisions.retain(|d| d.id != decision_id);
            }
            Command::AddException {
                rule,
                reason,
                scope,
            } => next.exceptions.push(DesignException {
                id: Uuid::new_v4(),
                rule: rule.trim().into(),
                reason: reason.trim().into(),
                scope,
                accepted_at_revision: next.revision,
            }),
            Command::RemoveException { exception_id } => {
                ensure(
                    next.exceptions.iter().any(|e| e.id == exception_id),
                    "This exception no longer exists.",
                )?;
                next.exceptions.retain(|e| e.id != exception_id);
            }
        }
        next.store_inline_images()?;
        next.validate()?;
        if crate::render::sync_fit_rects(&mut next) {
            next.validate()?;
        }
        Ok(next)
    }
}

fn clear_size_overrides(element: &mut Element, size_id: Uuid) {
    element.overrides.remove(&size_id);
    for child in &mut element.children {
        clear_size_overrides(child, size_id);
    }
}

pub(crate) fn validate_text(
    value: &str,
    label: &str,
    maximum: usize,
    required: bool,
) -> Result<()> {
    ensure(
        !required || !value.trim().is_empty(),
        format!("{label} is required."),
    )?;
    ensure(
        value.chars().count() <= maximum,
        format!("{label} exceeds {maximum} characters."),
    )?;
    ensure(
        !value.contains('\0'),
        format!("{label} cannot contain null characters."),
    )
}

pub(crate) fn validate_path(path: &str) -> Result<()> {
    ensure(
        path.starts_with('/') && (path == "/" || !path.ends_with('/')),
        "Page paths start with / and have no trailing slash.",
    )?;
    if path == "/" {
        return Ok(());
    }
    ensure(
        path.len() <= 1024 && !path.contains(['\\', '?', '#', '%', ':']),
        "The page path contains an unsupported character.",
    )?;
    for segment in path[1..].split('/') {
        ensure(
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment.ends_with(['.', ' '])
                && !segment
                    .chars()
                    .any(|c| c.is_control() || "<>\"|*".contains(c)),
            "The page path contains an invalid segment.",
        )?;
        let stem = segment.split('.').next().unwrap_or("").to_ascii_lowercase();
        ensure(
            !["assets", "src", "node_modules", "con", "prn", "aux", "nul"].contains(&stem.as_str())
                && !(stem.len() == 4
                    && (stem.starts_with("com") || stem.starts_with("lpt"))
                    && stem.ends_with(['1', '2', '3', '4', '5', '6', '7', '8', '9'])),
            "The page path uses a reserved name.",
        )?;
    }
    Ok(())
}
