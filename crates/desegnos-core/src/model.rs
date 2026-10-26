use crate::{
    Error, GenerationDossier, MAX_APPLIED_JOBS, MAX_PROJECT_SOURCE_BYTES, Result, SiteContent,
    ensure,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 10;
pub const MAX_PAGES: usize = 100;
pub const MAX_PROMPT_CHARS: usize = 16_384;

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
    pub rulers: bool,
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
            (1..=32_768).contains(&self.width) && (1..=100_000).contains(&self.height),
            "Viewport dimensions must be between 1 and 32,768 pixels wide and 1 and 100,000 pixels high.",
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementTarget {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub selector: String,
    pub tag: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_sha256: String,
}

impl ElementTarget {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.label, "Element label", 240, false)?;
        ensure(
            !self.id.is_empty()
                || (self.source_sha256.len() == 64
                    && self
                        .source_sha256
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())),
            "The selected element has an invalid source fingerprint.",
        )?;
        ensure(
            self.id.is_empty() || crate::stable_targets::valid_id(&self.id),
            "The selected element has an invalid editor ID.",
        )?;
        let parts: Vec<_> = self.selector.split(" > ").collect();
        ensure(
            parts.first() == Some(&"body") && parts.len() <= 64 && self.selector.len() <= 4096,
            "The selected element has an invalid selector.",
        )?;
        for part in parts.iter().skip(1) {
            let Some((tag, position)) = part.split_once(":nth-of-type(") else {
                return Err(Error(
                    "The selected element has an invalid selector.".into(),
                ));
            };
            ensure(
                valid_tag(tag)
                    && position
                        .strip_suffix(')')
                        .and_then(|n| n.parse::<u32>().ok())
                        .is_some_and(|n| n > 0),
                "The selected element has an invalid selector.",
            )?;
        }
        ensure(
            valid_tag(&self.tag)
                && (self.selector == "body" && self.tag == "body"
                    || parts.last().is_some_and(|part| {
                        part.starts_with(&format!("{}:nth-of-type(", self.tag))
                    })),
            "The selected element tag disagrees with its selector.",
        )
    }
}

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 80
        && tag.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RegionShape {
    #[default]
    Rect,
    Ellipse,
    Freehand {
        points: Vec<[f64; 2]>,
    },
}

impl RegionShape {
    pub fn is_rect(&self) -> bool {
        matches!(self, Self::Rect)
    }

    pub fn validate(&self) -> Result<()> {
        if let Self::Freehand { points } = self {
            ensure(
                (3..=500).contains(&points.len()),
                "A freehand area needs between 3 and 500 points.",
            )?;
            ensure(
                points
                    .iter()
                    .flatten()
                    .all(|n| n.is_finite() && (0.0..=1.0).contains(n)),
                "Freehand points must be finite fractions between 0 and 1.",
            )?;
        }
        Ok(())
    }
}

/// A page or page group a request may also change. An empty request selection means its own
/// page or framed document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PageSelection {
    /// One project page.
    Page { page_id: Uuid },
    /// Every page and editable site document below one path, for example `projects`.
    Prefix { path: String },
    /// Every page and editable site document.
    All,
}

impl PageSelection {
    pub fn page(page_id: Uuid) -> Self {
        Self::Page { page_id }
    }

    /// Accept `/projects/`, `projects` and `/projects` as the same directory prefix.
    pub fn prefix(path: &str) -> Result<Self> {
        let path = path.trim().trim_matches('/');
        ensure(
            !path.is_empty()
                && path.chars().count() <= 1024
                && !path.split('/').any(|part| part.is_empty() || part == ".."),
            "Choose a page group for this request.",
        )?;
        Ok(Self::Prefix { path: path.into() })
    }

    /// Whether this selection covers a page route or site path, written without a leading slash.
    pub fn covers(&self, location: &str, page_id: Option<Uuid>) -> bool {
        let location = location.trim_matches('/');
        match self {
            Self::Page { page_id: selected } => page_id == Some(*selected),
            Self::Prefix { path } => {
                location == path
                    || location
                        .strip_prefix(path.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            }
            Self::All => true,
        }
    }

    pub fn validate(&self, project: &Project) -> Result<()> {
        match self {
            Self::Page { page_id } => {
                project.page(*page_id)?;
            }
            Self::Prefix { path } => {
                ensure(
                    !path.trim().is_empty() && path.chars().count() <= 1024,
                    "Choose a page group for this request.",
                )?;
            }
            Self::All => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Region {
    pub id: Uuid,
    pub rect: Rect,
    #[serde(default, skip_serializing_if = "RegionShape::is_rect")]
    pub shape: RegionShape,
    /// Measured page elements for shape scope and whole-page coverage; refreshed before generation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub area_bounds: Vec<crate::TargetBounds>,
    pub z_index: i32,
    pub prompt: String,
    /// This request edits a document shown inside one of the page's iframes; its rect is mapped
    /// into that document's CSS pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_target: Option<crate::FrameTarget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<ElementTarget>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_target_removal: bool,
    /// A phone-layout fix: only CSS inside small-screen media queries may change.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mobile_only: bool,
    /// Bounded preview errors; this request uses whole-page repair authority.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_errors: Vec<String>,
    /// Force page scope, or suppress automatic detection after the user turns it off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub whole_page: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub whole_page_disabled: bool,
    /// Pages this request may also change; empty means only its own page or framed document.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<PageSelection>,
    /// Files and links dropped on the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::RequestAttachment>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub completed: bool,
}

impl Region {
    /// A covering base layer, or full-width page-layout intent, grants automatic page scope.
    /// Coverage uses measured page extents, falling back to the guide.
    /// Phone fixes and element selections stay local unless explicitly overridden.
    pub fn is_whole_page(&self, page: &Page) -> bool {
        if self.frame_target.is_some() {
            return false;
        }
        !self.runtime_errors.is_empty()
            || self.whole_page
            || (!self.whole_page_disabled && self.auto_whole_page(page))
    }

    pub fn auto_whole_page(&self, page: &Page) -> bool {
        if self.frame_target.is_some()
            || self.mobile_only
            || self.has_targets()
            || !self.shape.is_rect()
        {
            return false;
        }
        let width = self
            .area_bounds
            .iter()
            .map(|b| b.rect.x + b.rect.width)
            .fold(f64::from(page.viewport.width), f64::max);
        let height = self
            .area_bounds
            .iter()
            .map(|b| b.rect.y + b.rect.height)
            .reduce(f64::max)
            .unwrap_or(f64::from(page.viewport.height));
        let overlap = |start: f64, length: f64, extent: f64| {
            ((start + length).min(extent) - start.max(0.0)).max(0.0)
        };
        let base_layer = page
            .request_layers()
            .first()
            .is_some_and(|region| region.id == self.id);
        overlap(self.rect.x, self.rect.width, width) >= width * 0.95
            && (self.has_page_layout_intent()
                || base_layer
                    && ((self.rect.y.abs() <= f64::from(page.viewport.height) * 0.1
                        && overlap(
                            self.rect.y,
                            self.rect.height,
                            f64::from(page.viewport.height),
                        ) >= f64::from(page.viewport.height) * 0.9)
                        || overlap(self.rect.y, self.rect.height, height) >= height * 0.9))
    }

    /// Deliberately narrow page-layout phrases, never a general keyword like "background".
    /// A full-width request may carry this intent even when drawn below the viewport.
    fn has_page_layout_intent(&self) -> bool {
        let prompt = self
            .prompt
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        [
            "one page one screen",
            "one screen only",
            "single screen",
            "fit the page to",
            "fit this page to",
            "fit on one screen",
            "fit in one screen",
            "page height",
            "page layout",
            "viewport height",
        ]
        .iter()
        .any(|phrase| format!(" {prompt} ").contains(&format!(" {phrase} ")))
    }

    pub fn targets(&self) -> Vec<&ElementTarget> {
        self.elements.iter().collect()
    }
    pub fn has_targets(&self) -> bool {
        !self.elements.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Page {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub included: bool,
    pub viewport: Viewport,
    pub appearance: Appearance,
    pub regions: Vec<Region>,
    #[serde(deserialize_with = "crate::required_nullable")]
    pub content: Option<SiteContent>,
}

impl Page {
    /// Bottom to top, matching the canvas and Layers list. Stable sort retains array-order ties.
    pub fn request_layers(&self) -> Vec<&Region> {
        let mut layers: Vec<_> = self
            .regions
            .iter()
            .filter(|region| !region.completed)
            .collect();
        layers.sort_by_key(|region| region.z_index);
        layers
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
    Region(Uuid),
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
pub struct Project {
    pub schema_version: u32,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<crate::ImportedWebsite>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pictures: BTreeMap<Uuid, std::sync::Arc<str>>,
    pub decisions: Vec<Decision>,
    pub exceptions: Vec<DesignException>,
    pub extensions: BTreeMap<String, ExtensionRecord>,
    pub applied_job_ids: Vec<Uuid>,
    #[serde(deserialize_with = "crate::required_nullable")]
    pub latest_generation: Option<GenerationDossier>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    RenameProject {
        name: String,
    },
    /// Lay a page's code out readably (Code view → Tidy code), without the AI.
    FormatPageCode {
        page_id: Uuid,
    },
    SetGrid {
        enabled: bool,
    },
    SetGridSettings {
        settings: GridSettings,
    },
    SetGenerationMode {
        mode: GenerationMode,
    },
    AddPage {
        name: String,
        path: String,
    },
    UpdatePage {
        page_id: Uuid,
        name: String,
        path: String,
        viewport: Viewport,
        appearance: Appearance,
        included: bool,
    },
    DeletePage {
        page_id: Uuid,
    },
    AddRegion {
        page_id: Uuid,
        rect: Rect,
    },
    AddShapedRegion {
        page_id: Uuid,
        rect: Rect,
        shape: RegionShape,
    },
    AddElementRegion {
        page_id: Uuid,
        rect: Rect,
        element: ElementTarget,
    },
    AddElementRegions {
        page_id: Uuid,
        elements: Vec<ElementTarget>,
        bounds: Vec<crate::TargetBounds>,
        #[serde(default)]
        allow_target_removal: bool,
    },
    RetargetRegion {
        page_id: Uuid,
        region_id: Uuid,
        elements: Vec<ElementTarget>,
        bounds: Vec<crate::TargetBounds>,
        #[serde(default)]
        allow_target_removal: bool,
    },
    SetRegionTargetRemoval {
        page_id: Uuid,
        region_id: Uuid,
        allow_target_removal: bool,
    },
    SetRegionWholePage {
        page_id: Uuid,
        region_id: Uuid,
        whole_page: bool,
    },
    SetRegionRuntimeErrors {
        page_id: Uuid,
        region_id: Uuid,
        errors: Vec<String>,
    },
    /// Target a document shown by one of the page's iframes, with the box in its coordinates.
    SetRegionFrameTarget {
        page_id: Uuid,
        region_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame_target: Option<crate::FrameTarget>,
    },
    SetRegionMobileOnly {
        page_id: Uuid,
        region_id: Uuid,
        mobile_only: bool,
    },
    SetRegionPages {
        page_id: Uuid,
        region_id: Uuid,
        pages: Vec<PageSelection>,
    },
    AddRegionAttachment {
        page_id: Uuid,
        region_id: Uuid,
        attachment: crate::RequestAttachment,
    },
    RemoveRegionAttachment {
        page_id: Uuid,
        region_id: Uuid,
        attachment_id: String,
    },
    DeleteElements {
        page_id: Uuid,
        elements: Vec<ElementTarget>,
        /// The document to delete from; defaults to the current page.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        document: Option<crate::DocumentTarget>,
    },
    UpdateRegion {
        page_id: Uuid,
        region_id: Uuid,
        rect: Rect,
        prompt: String,
    },
    DismissEmptyRegions {
        keep_region_id: Option<Uuid>,
    },
    RemoveRegion {
        page_id: Uuid,
        region_id: Uuid,
    },
    ArrangeRegion {
        page_id: Uuid,
        region_id: Uuid,
        direction: crate::LayerDirection,
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

impl Project {
    pub fn new(name: String, viewport: Viewport, appearance: Appearance) -> Result<Self> {
        let home_page_id = Uuid::new_v4();
        let project = Self {
            schema_version: SCHEMA_VERSION,
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
                name: "Home".into(),
                path: "/".into(),
                included: true,
                viewport,
                appearance,
                regions: vec![],
                content: None,
            }],
            brief: Brief::default(),
            imported_identity: None,
            website: None,
            pictures: BTreeMap::new(),
            decisions: vec![],
            exceptions: vec![],
            extensions: BTreeMap::new(),
            applied_job_ids: vec![],
            latest_generation: None,
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

    fn page_mut(&mut self, id: Uuid) -> Result<&mut Page> {
        self.pages
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| Error("This page no longer exists.".into()))
    }

    pub fn pending_count(&self) -> usize {
        self.pages
            .iter()
            .flat_map(|p| &p.regions)
            .filter(|r| !r.completed && !r.prompt.trim().is_empty())
            .count()
    }

    pub fn validate(&self) -> Result<()> {
        ensure(
            self.schema_version == SCHEMA_VERSION,
            "This project uses an unsupported schema version. The original has not been changed.",
        )?;
        validate_text(&self.name, "Project name", 120, true)?;
        self.viewport.validate()?;
        self.grid_settings.validate()?;
        ensure(
            !self.pages.is_empty() && self.pages.len() <= MAX_PAGES,
            "A project must contain between 1 and 100 pages.",
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
        self.validate_picture_store()?;
        if let Some(site) = &self.website {
            site.validate(self)?;
        }
        let mut paths = BTreeSet::new();
        let mut source_bytes = 0_usize;
        for page in &self.pages {
            ensure(ids.insert(page.id), "Project identifiers must be unique.")?;
            validate_text(&page.name, "Page name", 120, true)?;
            validate_path(&page.path)?;
            ensure(
                paths.insert(page.path.to_lowercase()),
                "Two pages cannot use the same public path.",
            )?;
            page.viewport.validate()?;
            if let Some(content) = &page.content {
                content.validate()?;
                crate::stable_targets::validate_ids(&crate::editing::parse(&content.html)?)?;
                source_bytes += content.byte_len();
                ensure(
                    source_bytes <= MAX_PROJECT_SOURCE_BYTES,
                    "The project exceeds its 64 MiB generated source limit.",
                )?;
            }
            for region in &page.regions {
                ensure(ids.insert(region.id), "Area identifiers must be unique.")?;
                region.rect.validate(page.viewport)?;
                region.shape.validate()?;
                ensure(
                    !region.has_targets() || region.shape.is_rect(),
                    "Element requests must use rectangular bounds.",
                )?;
                ensure(
                    region.area_bounds.len() <= 10_000,
                    "An area supports at most 10,000 measured elements.",
                )?;
                let mut measured_ids = BTreeSet::new();
                for bound in &region.area_bounds {
                    bound.rect.validate(page.viewport)?;
                    bound.target.validate()?;
                    ensure(
                        !bound.target.id.is_empty() && measured_ids.insert(&bound.target.id),
                        "Measured elements need unique editor IDs.",
                    )?;
                }
                ensure(
                    region.elements.len() <= crate::MAX_BATCH_ELEMENT_EDITS,
                    "A request supports at most 500 selected elements.",
                )?;
                let mut target_ids = BTreeSet::new();
                for element in region.targets() {
                    element.validate()?;
                    ensure(
                        element.id.is_empty() || target_ids.insert(&element.id),
                        "A selected element was supplied twice.",
                    )?;
                }
                ensure(
                    region.runtime_errors.len() <= 10
                        && region
                            .runtime_errors
                            .iter()
                            .all(|error| !error.is_empty() && error.chars().count() <= 4096),
                    "Preview errors exceed the repair request limit.",
                )?;
                ensure(
                    region.runtime_errors.is_empty()
                        || (!region.mobile_only && !region.has_targets()),
                    "Runtime repairs require a page request.",
                )?;
                validate_text(&region.prompt, "Area description", MAX_PROMPT_CHARS, false)?;
                ensure(
                    region.pages.len() <= MAX_PAGES,
                    "A request supports at most 100 pages.",
                )?;
                for selection in &region.pages {
                    selection.validate(self)?;
                }
                for attachment in &region.attachments {
                    attachment.validate()?;
                }
            }
        }
        let home = self.page(self.home_page_id)?;
        ensure(
            home.path == "/" && home.included,
            "The home page must be included and use the root path.",
        )?;
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
        Ok(())
    }

    pub fn validate_scope(&self, scope: &Scope) -> Result<()> {
        match scope {
            Scope::Project => Ok(()),
            Scope::Page(id) => self.page(*id).map(|_| ()),
            Scope::Region(id) => ensure(
                self.pages
                    .iter()
                    .any(|p| p.regions.iter().any(|r| r.id == *id)),
                "This decision's area no longer exists.",
            ),
        }
    }

    fn dismiss_empty_regions(&mut self, keep: Option<Uuid>) {
        for page in &mut self.pages {
            for region in &mut page.regions {
                if Some(region.id) != keep && region.prompt.trim().is_empty() {
                    region.completed = true;
                }
            }
        }
    }

    pub fn apply(&self, expected_revision: u64, command: Command) -> Result<Self> {
        ensure(
            expected_revision == self.revision,
            "The project changed. Refresh the document before applying this edit.",
        )?;
        if let Command::FormatPageCode { page_id } = &command {
            let content = self
                .page(*page_id)?
                .content
                .as_ref()
                .ok_or_else(|| Error("This page has no code yet.".into()))?;
            if crate::format_site_content(content) == *content {
                self.validate()?;
                return Ok(self.clone());
            }
        }
        if let Command::DismissEmptyRegions { keep_region_id } = &command {
            if let Some(id) = keep_region_id {
                self.validate_scope(&Scope::Region(*id))?;
            }
            if !self
                .pages
                .iter()
                .flat_map(|page| &page.regions)
                .any(|region| {
                    !region.completed
                        && Some(region.id) != *keep_region_id
                        && region.prompt.trim().is_empty()
                })
            {
                self.validate()?;
                return Ok(self.clone());
            }
        }
        if let Command::ArrangeRegion {
            page_id,
            region_id,
            direction,
        } = &command
        {
            self.validate()?;
            let page = self.page(*page_id)?;
            let mut order = page
                .regions
                .iter()
                .enumerate()
                .filter(|(_, region)| !region.completed)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            order.sort_by_key(|index| page.regions[*index].z_index);
            let position = order
                .iter()
                .position(|index| page.regions[*index].id == *region_id)
                .ok_or_else(|| {
                    Error("This request no longer exists or is already in history.".into())
                })?;
            let destination = match direction {
                crate::LayerDirection::Forward => (position + 1).min(order.len() - 1),
                crate::LayerDirection::Backward => position.saturating_sub(1),
                crate::LayerDirection::Front => order.len() - 1,
                crate::LayerDirection::Back => 0,
            };
            if destination == position {
                return Ok(self.clone());
            }
            let index = order.remove(position);
            order.insert(destination, index);
            let mut next = self.clone();
            next.revision = next
                .revision
                .checked_add(1)
                .ok_or_else(|| Error("Project revision limit reached.".into()))?;
            let page = next.page_mut(*page_id)?;
            for (rank, index) in order.into_iter().enumerate() {
                page.regions[index].z_index = i32::try_from(rank)
                    .map_err(|_| Error("This page has too many request layers.".into()))?;
            }
            next.store_inline_images()?;
            next.validate()?;
            return Ok(next);
        }
        let mut next = self.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        let area_shape = match &command {
            Command::AddShapedRegion { shape, .. } => shape.clone(),
            _ => RegionShape::Rect,
        };
        match command {
            Command::RenameProject { name } => next.name = name.trim().into(),
            Command::FormatPageCode { page_id } => {
                let page = next
                    .pages
                    .iter_mut()
                    .find(|page| page.id == page_id)
                    .ok_or_else(|| Error("This page no longer exists.".into()))?;
                if let Some(content) = &page.content {
                    page.content = Some(crate::format_site_content(content));
                }
            }
            Command::SetGrid { enabled } => next.show_grid = enabled,
            Command::SetGridSettings { settings } => next.grid_settings = settings,
            Command::SetGenerationMode { mode } => next.generation_mode = mode,
            Command::AddPage { name, path } => next.pages.push(Page {
                id: Uuid::new_v4(),
                name: name.trim().into(),
                path,
                included: true,
                viewport: next.viewport,
                appearance: next.appearance,
                regions: vec![],
                content: None,
            }),
            Command::UpdatePage {
                page_id,
                name,
                path,
                viewport,
                appearance,
                included,
            } => {
                let p = next.page_mut(page_id)?;
                p.name = name.trim().into();
                p.path = path;
                p.viewport = viewport;
                p.appearance = appearance;
                p.included = included;
            }
            Command::DeletePage { page_id } => {
                next.page(page_id)?;
                ensure(
                    page_id != next.home_page_id,
                    "The home page cannot be deleted.",
                )?;
                next.pages.retain(|p| p.id != page_id);
            }
            Command::AddRegion { page_id, rect }
            | Command::AddShapedRegion { page_id, rect, .. } => {
                next.dismiss_empty_regions(None);
                let p = next.page_mut(page_id)?;
                let z_index = p
                    .regions
                    .iter()
                    .map(|r| r.z_index)
                    .max()
                    .unwrap_or(-1)
                    .saturating_add(1);
                p.regions.push(Region {
                    pages: vec![],
                    id: Uuid::new_v4(),
                    rect,
                    shape: area_shape,
                    area_bounds: vec![],
                    z_index,
                    prompt: String::new(),

                    frame_target: None,
                    elements: vec![],
                    allow_target_removal: false,
                    mobile_only: false,
                    runtime_errors: vec![],
                    whole_page: false,
                    whole_page_disabled: false,
                    attachments: vec![],
                    completed: false,
                });
            }
            Command::AddElementRegion {
                page_id,
                rect,
                element,
            } => {
                next.dismiss_empty_regions(None);
                let canonical = next.canonical_targets(page_id, std::slice::from_ref(&element))?;
                let page = next.page_mut(page_id)?;
                let z_index = page
                    .regions
                    .iter()
                    .map(|r| r.z_index)
                    .max()
                    .unwrap_or(-1)
                    .saturating_add(1);
                page.regions.push(Region {
                    pages: vec![],
                    id: Uuid::new_v4(),
                    rect,
                    shape: RegionShape::Rect,
                    area_bounds: vec![],
                    z_index,
                    prompt: String::new(),

                    frame_target: None,
                    elements: canonical,
                    allow_target_removal: false,
                    mobile_only: false,
                    runtime_errors: vec![],
                    whole_page: false,
                    whole_page_disabled: false,
                    attachments: vec![],
                    completed: false,
                });
            }
            Command::AddElementRegions {
                page_id,
                elements,
                bounds,
                allow_target_removal,
            } => {
                let targets = next.canonical_targets(page_id, &elements)?;
                let rect = next.target_bounds_union(page_id, &targets, &bounds)?;
                next.dismiss_empty_regions(None);
                let page = next.page_mut(page_id)?;
                let z_index = page
                    .regions
                    .iter()
                    .map(|r| r.z_index)
                    .max()
                    .unwrap_or(-1)
                    .saturating_add(1);
                page.regions.push(Region {
                    pages: vec![],
                    id: Uuid::new_v4(),
                    rect,
                    shape: RegionShape::Rect,
                    area_bounds: vec![],
                    z_index,
                    prompt: String::new(),

                    frame_target: None,
                    elements: targets,
                    allow_target_removal,
                    mobile_only: false,
                    runtime_errors: vec![],
                    whole_page: false,
                    whole_page_disabled: false,
                    attachments: vec![],
                    completed: false,
                });
            }
            Command::RetargetRegion {
                page_id,
                region_id,
                elements,
                bounds,
                allow_target_removal,
            } => {
                ensure(
                    !next
                        .page(page_id)?
                        .regions
                        .iter()
                        .any(|region| region.id == region_id && region.frame_target.is_some()),
                    "A framed request changes only its framed document.",
                )?;
                let targets = next.canonical_targets(page_id, &elements)?;
                let rect = next.target_bounds_union(page_id, &targets, &bounds)?;
                let region = next
                    .page_mut(page_id)?
                    .regions
                    .iter_mut()
                    .find(|r| r.id == region_id && !r.completed)
                    .ok_or_else(|| Error("This request no longer exists.".into()))?;
                region.elements = targets;

                region.rect = rect;
                region.shape = RegionShape::Rect;
                region.area_bounds.clear();
                region.allow_target_removal = allow_target_removal;
            }
            Command::SetRegionTargetRemoval {
                page_id,
                region_id,
                allow_target_removal,
            } => {
                let region = next
                    .page_mut(page_id)?
                    .regions
                    .iter_mut()
                    .find(|r| r.id == region_id && !r.completed && r.has_targets())
                    .ok_or_else(|| Error("Select a request for existing elements first.".into()))?;
                region.allow_target_removal = allow_target_removal;
            }
            Command::SetRegionWholePage {
                page_id,
                region_id,
                whole_page,
            } => {
                let region = next
                    .page_mut(page_id)?
                    .regions
                    .iter_mut()
                    .find(|r| r.id == region_id && !r.completed)
                    .ok_or_else(|| Error("This request no longer exists.".into()))?;
                ensure(
                    region.frame_target.is_none() || !whole_page,
                    "A framed request changes only its framed document.",
                )?;
                region.whole_page = whole_page;
                region.whole_page_disabled = !whole_page;
                if whole_page {
                    region.mobile_only = false;
                }
            }
            Command::SetRegionRuntimeErrors {
                page_id,
                region_id,
                errors,
            } => {
                ensure(
                    !errors.is_empty()
                        && errors.len() <= 10
                        && errors
                            .iter()
                            .all(|e| !e.is_empty() && e.chars().count() <= 4096),
                    "Preview errors exceed the repair request limit.",
                )?;
                let region = next
                    .page_mut(page_id)?
                    .regions
                    .iter_mut()
                    .find(|r| r.id == region_id && !r.completed)
                    .ok_or_else(|| Error("This request no longer exists.".into()))?;
                ensure(
                    !region.has_targets() && region.frame_target.is_none(),
                    "Runtime repairs require a page request.",
                )?;
                region.runtime_errors = errors;
                region.mobile_only = false;
                region.prompt = "Fix these runtime errors on this page. Edit this page's shared JavaScript, region scripts, and HTML/CSS as needed to fix them. Keep the rest of the design and content unchanged.".into();
            }
            Command::SetRegionMobileOnly {
                page_id,
                region_id,
                mobile_only,
            } => {
                let region = next
                    .page_mut(page_id)?
                    .regions
                    .iter_mut()
                    .find(|r| r.id == region_id && !r.completed)
                    .ok_or_else(|| Error("This request no longer exists.".into()))?;
                ensure(
                    region.frame_target.is_none() || !mobile_only,
                    "A framed request changes only its framed document.",
                )?;
                region.mobile_only = mobile_only;
                if mobile_only {
                    region.whole_page = false;
                }
            }
            Command::SetRegionPages {
                page_id,
                region_id,
                pages,
            } => {
                ensure(
                    pages.len() <= MAX_PAGES,
                    "A request supports at most 100 pages.",
                )?;
                let mut unique = BTreeSet::new();
                let mut selections = Vec::new();
                for selection in pages {
                    let selection = match selection {
                        PageSelection::Prefix { path } => PageSelection::prefix(&path)?,
                        selection => selection,
                    };
                    selection.validate(&next)?;
                    if unique.insert(selection.clone()) {
                        selections.push(selection);
                    }
                }
                let region = next
                    .page_mut(page_id)?
                    .regions
                    .iter_mut()
                    .find(|r| r.id == region_id && !r.completed)
                    .ok_or_else(|| Error("This request no longer exists.".into()))?;
                region.pages = selections;
            }
            Command::SetRegionFrameTarget {
                page_id,
                region_id,
                frame_target,
            } => {
                if let Some(frame) = &frame_target {
                    ensure(
                        frame.page_id == page_id,
                        "A framed request must belong to the page that holds its iframe.",
                    )?;
                }
                let region = next
                    .page_mut(page_id)?
                    .regions
                    .iter_mut()
                    .find(|r| r.id == region_id && !r.completed)
                    .ok_or_else(|| Error("This request no longer exists.".into()))?;
                region.frame_target = frame_target.clone();
                if frame_target.is_some() {
                    region.elements.clear();
                    region.area_bounds.clear();
                    region.shape = RegionShape::Rect;
                    region.allow_target_removal = false;
                    region.mobile_only = false;
                    region.runtime_errors.clear();
                    region.whole_page = false;
                    region.whole_page_disabled = false;
                }
            }
            Command::AddRegionAttachment {
                page_id,
                region_id,
                mut attachment,
            } => {
                let page = next.page_mut(page_id)?;
                let used: BTreeSet<String> = page
                    .regions
                    .iter()
                    .flat_map(|r| r.attachments.iter().map(|a| a.id.clone()))
                    .collect();
                let region = page
                    .regions
                    .iter_mut()
                    .find(|r| r.id == region_id && !r.completed)
                    .ok_or_else(|| Error("This request no longer exists.".into()))?;
                ensure(
                    region.attachments.len() < 16,
                    "A request supports at most 16 attachments.",
                )?;
                // Identifiers are assigned here so they stay unique on the page.
                attachment.id = (1..)
                    .map(|n| format!("a{}{n}", &region_id.simple().to_string()[..4]))
                    .find(|id| !used.contains(id))
                    .expect("an unused identifier");
                // Decoding a large photo is slow, so its AI preview is made once, here.
                attachment.preview = match (attachment.kind, attachment.data_uri.as_deref()) {
                    (crate::AttachmentKind::Image, Some(data)) => {
                        crate::ai_preview(data).map(std::sync::Arc::from)
                    }
                    _ => None,
                };
                attachment.validate()?;
                region.attachments.push(attachment);
            }
            Command::RemoveRegionAttachment {
                page_id,
                region_id,
                attachment_id,
            } => {
                let region = next
                    .page_mut(page_id)?
                    .regions
                    .iter_mut()
                    .find(|r| r.id == region_id && !r.completed)
                    .ok_or_else(|| Error("This request no longer exists.".into()))?;
                let before = region.attachments.len();
                region.attachments.retain(|a| a.id != attachment_id);
                ensure(
                    region.attachments.len() < before,
                    "That attachment was already removed.",
                )?;
            }
            Command::DeleteElements {
                page_id,
                elements,
                document,
            } => {
                let document = document.unwrap_or(crate::DocumentTarget::Page { page_id });
                return self.delete_elements_in(expected_revision, &document, &elements);
            }
            Command::UpdateRegion {
                page_id,
                region_id,
                rect,
                prompt,
            } => {
                let region = next
                    .page_mut(page_id)?
                    .regions
                    .iter_mut()
                    .find(|r| r.id == region_id)
                    .ok_or_else(|| Error("This area no longer exists.".into()))?;
                region.rect = rect;
                region.prompt = prompt;
                region.completed = false;
            }
            Command::DismissEmptyRegions { keep_region_id } => {
                next.dismiss_empty_regions(keep_region_id)
            }
            Command::RemoveRegion { page_id, region_id } => {
                let page = next.page_mut(page_id)?;
                ensure(
                    page.regions.iter().any(|r| r.id == region_id),
                    "This area no longer exists.",
                )?;
                page.regions.retain(|r| r.id != region_id);
            }
            Command::ArrangeRegion { .. } => {
                unreachable!("Layer arrangements are handled before ordinary edits")
            }
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
        Ok(next)
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
