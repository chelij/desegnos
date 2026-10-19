use crate::{
    Error, GenerationDossier, MAX_APPLIED_JOBS, MAX_PROJECT_SOURCE_BYTES, Result, SiteContent,
    ensure,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 6;
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
    pub selector: String,
    pub tag: String,
    pub label: String,
    pub source_sha256: String,
}

impl ElementTarget {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.label, "Element label", 240, false)?;
        ensure(
            self.source_sha256.len() == 64
                && self
                    .source_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "The selected element has an invalid source fingerprint.",
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
        && tag.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && tag
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Region {
    pub id: Uuid,
    pub rect: Rect,
    pub z_index: i32,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<ElementTarget>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub completed: bool,
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
    AddElementRegion {
        page_id: Uuid,
        rect: Rect,
        element: ElementTarget,
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
                source_bytes += content.byte_len();
                ensure(
                    source_bytes <= MAX_PROJECT_SOURCE_BYTES,
                    "The project exceeds its 64 MiB generated source limit.",
                )?;
            }
            for region in &page.regions {
                ensure(ids.insert(region.id), "Area identifiers must be unique.")?;
                region.rect.validate(page.viewport)?;
                if let Some(element) = &region.element {
                    element.validate()?;
                }
                validate_text(&region.prompt, "Area description", MAX_PROMPT_CHARS, false)?;
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
        let mut next = self.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| Error("Project revision limit reached.".into()))?;
        match command {
            Command::RenameProject { name } => next.name = name.trim().into(),
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
            Command::AddRegion { page_id, rect } => {
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
                    id: Uuid::new_v4(),
                    rect,
                    z_index,
                    prompt: String::new(),
                    element: None,
                    completed: false,
                });
            }
            Command::AddElementRegion {
                page_id,
                rect,
                element,
            } => {
                next.dismiss_empty_regions(None);
                element.validate()?;
                let page = next.page_mut(page_id)?;
                ensure(
                    page.content
                        .as_ref()
                        .map(SiteContent::sha256)
                        .transpose()?
                        .as_deref()
                        == Some(&element.source_sha256),
                    "The page changed. Select the element again.",
                )?;
                let z_index = page
                    .regions
                    .iter()
                    .map(|r| r.z_index)
                    .max()
                    .unwrap_or(-1)
                    .saturating_add(1);
                page.regions.push(Region {
                    id: Uuid::new_v4(),
                    rect,
                    z_index,
                    prompt: String::new(),
                    element: Some(element),
                    completed: false,
                });
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

fn validate_path(path: &str) -> Result<()> {
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
