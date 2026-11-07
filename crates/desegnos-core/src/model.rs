use crate::{
    Element, Error, GenerationDossier, LinkedDefinition, MAX_APPLIED_JOBS, RequestAttachment,
    Result, ensure,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 14;
/// The longest export destination the project remembers.
pub use crate::limits::MAX_EXPORT_DESTINATION;
pub use crate::limits::MAX_NAMED_VIEW_NAME_CHARS;
pub use crate::limits::MAX_NAMED_VIEWS;
pub use crate::limits::MAX_PAGE_SIZES;
pub use crate::limits::MAX_PAGES;
pub use crate::limits::MAX_POINTERS;
pub use crate::limits::MAX_PROMPT_CHARS;
pub use crate::limits::MAX_REFERENCE_ITEMS;
pub use crate::limits::MAX_REQUEST_TARGETS;
pub use crate::limits::MAX_REQUESTS;

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    Light,
    Dark,
}

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GenerationMode {
    Fuzzy,
    Strict,
}

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GridUnit {
    Pixels,
    Percent,
}

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GridSettings {
    pub spacing: f64,
    pub unit: GridUnit,
    pub snap: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            spacing: 8.0,
            unit: GridUnit::Pixels,
            snap: true,
        }
    }
}

impl GridSettings {
    pub fn validate(&self) -> Result<()> {
        let valid = match self.unit {
            GridUnit::Pixels => (crate::limits::GRID_PIXEL_MIN..=crate::limits::GRID_PIXEL_MAX)
                .contains(&self.spacing),
            GridUnit::Percent => (crate::limits::GRID_PERCENT_MIN
                ..=crate::limits::GRID_PERCENT_MAX)
                .contains(&self.spacing),
        };
        ensure(
            self.spacing.is_finite() && valid,
            format!(
                "Grid spacing must be between {} and {} pixels, or {} and {} percent.",
                crate::limits::GRID_PIXEL_MIN,
                crate::limits::GRID_PIXEL_MAX,
                crate::limits::GRID_PERCENT_MIN,
                crate::limits::GRID_PERCENT_MAX
            ),
        )
    }
}

/// The widest or highest a viewport or a boundary size may be; `PageSize::validate` shares it.
pub use crate::limits::MAX_VIEWPORT_SIDE;

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub fn validate(&self) -> Result<()> {
        ensure(
            (1..=MAX_VIEWPORT_SIDE).contains(&self.width)
                && (1..=MAX_VIEWPORT_SIDE).contains(&self.height),
            format!(
                "Viewport dimensions must be between 1 and {MAX_VIEWPORT_SIDE} pixels wide and high."
            ),
        )
    }
}

#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn validate(&self) -> Result<()> {
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
#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
            Self::Rect { rect } | Self::Ellipse { rect } => rect.validate(),
            Self::Polygon { points } | Self::Freehand { points } => {
                ensure(
                    (3..=crate::limits::MAX_REQUEST_SHAPE_POINTS).contains(&points.len()),
                    format!(
                        "A request area needs between 3 and {} points.",
                        crate::limits::MAX_REQUEST_SHAPE_POINTS
                    ),
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    /// Original boundary order on the single follow-up for untouched empty boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boundary_retry_order: Vec<Uuid>,
}

impl PendingRequest {
    /// The layer this request is asked of when it targets that layer alone.
    pub(crate) fn sole_layer(&self) -> Option<Uuid> {
        match self.targets.as_slice() {
            [RequestTarget::Element { id, .. }] if self.area.is_none() => Some(*id),
            _ => None,
        }
    }

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
        ensure(
            self.boundary_retry_order.len() <= MAX_REQUEST_TARGETS
                && self.boundary_retry_order.iter().all(|id| !id.is_nil()),
            "A boundary follow-up needs at most 64 valid original boundaries.",
        )?;
        if let Some(area) = &self.area {
            area.validate()?;
        }
        Ok(())
    }
}

/// How a page behaves inside its canvas boundary.
#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum BoundaryKind {
    /// Fixed width, grows taller as content is added.
    Page,
    /// Fixed size, content beyond it clips.
    Screen,
}

/// What a boundary ships as. Websites and apps are built by the AI from the element tree;
/// slides and images are rendered directly from it.
#[derive(ts_rs::TS, Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OutputType {
    #[default]
    Webpage,
    /// Legacy web output retained for saved projects and history; new pages use Webpage.
    AppScreen,
    Slide,
    Image,
}

impl OutputType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Webpage | Self::AppScreen => "Page",
            Self::Slide => "Slide",
            Self::Image => "Image",
        }
    }

    /// Websites and apps are written by the AI export job; slides and images render directly.
    pub fn needs_ai(self) -> bool {
        matches!(self, Self::Webpage | Self::AppScreen)
    }
}

/// One screen size of a page boundary. A page has one or more sizes drawn side by side on the
/// canvas; the first is the base size, whose element values every other size inherits unless it
/// overrides them.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(ts_rs::TS, Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
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
pub const TEMPLATE_GAP: u32 = 160;

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

    fn page(&self, name: &str, path: &str, output: OutputType, sizes: Vec<PageSize>) -> Page {
        Page {
            id: Uuid::new_v4(),
            name: name.into(),
            path: path.into(),
            included: true,
            output,
            sizes,
            background: crate::limits::DEFAULT_PAGE_BACKGROUND.into(),
            elements: vec![],
        }
    }

    pub(crate) fn pages(self) -> Vec<Page> {
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
                "Page 1",
                "/",
                OutputType::Webpage,
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
#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasPlacement {
    pub page_id: Uuid,
    pub size_id: Uuid,
    pub x: f64,
    pub y: f64,
    pub kind: BoundaryKind,
}

impl CanvasPlacement {
    pub(crate) fn place(self, pages: &mut [Page]) -> Result<()> {
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        validate_text(&self.name, "View name", MAX_NAMED_VIEW_NAME_CHARS, true)?;
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
#[derive(ts_rs::TS, Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReferencePlacement {
    pub item_id: Uuid,
    pub x: f64,
    pub y: f64,
}

/// A reference item's content. Reference items are material the AI may read, and
/// nothing outside every boundary ever ships.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceItem {
    pub id: Uuid,
    pub rect: Rect,
    pub kind: ReferenceKind,
}

impl ReferenceItem {
    pub fn validate(&self, project: &Project) -> Result<()> {
        self.rect.validate()?;
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
                validate_text(name, "Picture name", crate::limits::MAX_NAME_CHARS, false)?;
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointerWiring {
    pub fingerprint: String,
    pub generated_comment: String,
}

/// One boundary as an export wrote it.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportedPage {
    pub page_id: Uuid,
    pub path: String,
    pub output: OutputType,
}

/// The code one pointer produced: a fingerprint of its marked fragments and the comment the
/// export was written for. A pointer with no marked fragments is not recorded.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportedPointer {
    pub fingerprint: String,
    pub comment: String,
}

/// What the last export produced. Kept in the project so the canvas can compare its comments
/// and recompute pointer fingerprints from the destination folder.
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
                crate::limits::is_sha256_hex(&exported.fingerprint),
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
#[derive(ts_rs::TS, Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// A layer or boundary's own export-only behaviour, shared by all linked sizes.
    pub fn is_behaviour(&self) -> bool {
        match (&self.from, &self.to) {
            (
                PointerSource::Element {
                    page_id: a,
                    element_id: x,
                    size_id: None,
                },
                PointerTarget::Element {
                    page_id: b,
                    element_id: y,
                    size_id: None,
                },
            ) => a == b && x == y,
            (
                PointerSource::Boundary {
                    page_id: a,
                    size_id: None,
                },
                PointerTarget::Boundary {
                    page_id: b,
                    size_id: None,
                },
            ) => a == b,
            _ => false,
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure(!self.id.is_nil(), "A pointer needs a nonempty identifier.")?;
        ensure(
            !self.is_behaviour() || self.kind == PointerKind::Change,
            "A behaviour pointer must be kind Change.",
        )?;
        validate_text(&self.comment, "Pointer comment", MAX_PROMPT_CHARS, false)?;
        if let PointerTarget::Url { url } = &self.to {
            ensure(
                crate::limits::valid_http_url(url),
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
        crate::pointer_is_broken(project, self)
    }
}

pub(crate) use crate::limits::is_hex_colour as valid_swatch;

pub(crate) fn normalized_swatch(color: &str) -> String {
    let hex = color.trim().trim_start_matches('#').to_ascii_lowercase();
    // #rgb and #rrggbb describe the same colour; store the canonical six digits.
    let hex = if hex.len() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect()
    } else {
        hex
    };
    format!("#{hex}")
}

pub(crate) fn attachment_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].into()
}

pub(crate) fn reference_picture_name(picture: Uuid, name: &str) -> String {
    if name.trim().is_empty() {
        format!("Picture {}", &picture.simple().to_string()[..8])
    } else {
        name.chars().take(crate::limits::MAX_NAME_CHARS).collect()
    }
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Page {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub included: bool,
    /// What this boundary ships as: a page, slide or image (including legacy app-screen output).
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
        if !crate::limits::is_hex_colour(&self.background) {
            return "light";
        }
        let hex = normalized_swatch(&self.background);
        let channel = |index| {
            let value =
                u8::from_str_radix(&hex[index..index + 2], 16).unwrap_or(255) as f64 / 255.0;
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
}

#[derive(ts_rs::TS, Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Brief {
    pub purpose: String,
    pub audience: String,
    pub brand_name: String,
    pub tone: String,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Decision {
    pub id: Uuid,
    pub topic: String,
    pub value: String,
    pub scope: Scope,
    pub protected: bool,
    pub accepted_at_revision: u64,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignException {
    pub id: Uuid,
    pub rule: String,
    pub reason: String,
    pub scope: Scope,
    pub accepted_at_revision: u64,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColourStyle {
    pub id: Uuid,
    pub name: String,
    pub value: String,
}
#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(ts_rs::TS, Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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
            validate_text(name, "Style name", crate::limits::MAX_NAME_CHARS, false)?;
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

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub applied_job_ids: Vec<Uuid>,
    #[serde(deserialize_with = "crate::required_nullable")]
    pub latest_generation: Option<GenerationDossier>,
    /// The last export written from this project, so pointer clashes can compare against it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_export: Option<ExportRecord>,
}

impl Project {
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
                        && crate::elements::find_element(&page.elements, *id).is_some()
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

    pub fn validate(&self) -> Result<()> {
        ensure(
            self.schema_version == SCHEMA_VERSION,
            "This project uses an unsupported schema version. The original has not been changed.",
        )?;
        validate_text(
            &self.name,
            "Project name",
            crate::limits::MAX_NAME_CHARS,
            true,
        )?;
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
                            crate::elements::find_element(&page.elements, *id).is_some(),
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
            ensure(view_ids.insert(view.id), "View identifiers must be unique.")?;
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
        let mut behaviour_sources = Vec::new();
        for pointer in &self.pointers {
            if pointer.is_behaviour() {
                ensure(
                    !behaviour_sources.contains(&pointer.from),
                    "A source can have at most one behaviour pointer.",
                )?;
                behaviour_sources.push(pointer.from.clone());
            }
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
            validate_text(&page.name, "Page name", crate::limits::MAX_NAME_CHARS, true)?;
            ensure(
                valid_swatch(&page.background),
                "Boundary background must be a hex colour.",
            )?;
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
                    for child in &element.children {
                        sizes(child, ids);
                    }
                }
                let mut size_ids = BTreeSet::new();
                sizes(element, &mut size_ids);
                crate::elements::validate_elements(
                    std::slice::from_ref(element.as_ref()),
                    self,
                    &size_ids,
                    &mut ids,
                    &mut element_count,
                )?;
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
            validate_text(
                &decision.topic,
                "Decision topic",
                crate::limits::MAX_NAME_CHARS,
                true,
            )?;
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
            validate_text(
                &exception.rule,
                "Exception rule",
                crate::limits::MAX_NAME_CHARS,
                true,
            )?;
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
