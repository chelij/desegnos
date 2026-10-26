use crate::{
    Appearance, Brief, Decision, DesignException, Error, ExtensionRecord, GenerationMode,
    GridSettings, Page, Project, Rect, Region, Result, SCHEMA_VERSION, Viewport, digest, ensure,
    requirements::project_notes,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const LEGACY_GENERATION_NAMESPACE: &str = "io.desegnos.legacy-generation";
pub const GENERATION_MIGRATION_NOTICE: &str = "Review the global generation setting before generating; it defaults to Fuzzy. Generate again to replace older results.";
pub const CONTENT_MIGRATION_NOTICE: &str = "This project has no saved generated page source. Save to upgrade this copy; the previous project file will be retained as a backup. Generate again to replace older results.";
pub const ELEMENT_MIGRATION_NOTICE: &str = "Save to upgrade this copy and keep the previous project file as a backup. Generate again to replace older results.";
pub const IDENTITY_MIGRATION_NOTICE: &str = "Save to upgrade this copy and keep the previous project file as a backup. Generate again to replace older results.";
pub const CANVAS_MIGRATION_NOTICE: &str = "Save to upgrade this copy and keep the previous project file as a backup. Generate again to replace older results.";

pub struct DecodedProject {
    pub project: Project,
    pub source_schema_version: u32,
    pub source_sha256: String,
    pub source_project_markdown: String,
}

impl DecodedProject {
    pub fn migrated(&self) -> bool {
        self.source_schema_version != SCHEMA_VERSION
    }

    pub fn migration_notice(&self) -> Option<&'static str> {
        match self.source_schema_version {
            1 => Some(GENERATION_MIGRATION_NOTICE),
            2 => Some(CONTENT_MIGRATION_NOTICE),
            3 => Some(CANVAS_MIGRATION_NOTICE),
            4 => Some(ELEMENT_MIGRATION_NOTICE),
            5 => Some(IDENTITY_MIGRATION_NOTICE),
            6 => Some(
                "Save to upgrade this copy and keep the previous file as a backup. Generate again to replace older results.",
            ),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SchemaHeader {
    schema_version: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRegion {
    id: Uuid,
    rect: Rect,
    z_index: i32,
    prompt: String,
    mode: GenerationMode,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyPage {
    id: Uuid,
    name: String,
    path: String,
    included: bool,
    viewport: Viewport,
    appearance: Appearance,
    regions: Vec<LegacyRegion>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyProject {
    schema_version: u32,
    id: Uuid,
    name: String,
    revision: u64,
    viewport: Viewport,
    appearance: Appearance,
    show_grid: bool,
    home_page_id: Uuid,
    pages: Vec<LegacyPage>,
    brief: Brief,
    decisions: Vec<Decision>,
    exceptions: Vec<DesignException>,
    extensions: BTreeMap<String, ExtensionRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VersionTwoPage {
    id: Uuid,
    name: String,
    path: String,
    included: bool,
    viewport: Viewport,
    appearance: Appearance,
    regions: Vec<Region>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VersionTwoProject {
    schema_version: u32,
    id: Uuid,
    name: String,
    revision: u64,
    viewport: Viewport,
    appearance: Appearance,
    show_grid: bool,
    generation_mode: GenerationMode,
    home_page_id: Uuid,
    pages: Vec<VersionTwoPage>,
    brief: Brief,
    decisions: Vec<Decision>,
    exceptions: Vec<DesignException>,
    extensions: BTreeMap<String, ExtensionRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VersionThreeProject {
    schema_version: u32,
    id: Uuid,
    name: String,
    revision: u64,
    viewport: Viewport,
    appearance: Appearance,
    show_grid: bool,
    generation_mode: GenerationMode,
    home_page_id: Uuid,
    pages: Vec<Page>,
    brief: Brief,
    decisions: Vec<Decision>,
    exceptions: Vec<DesignException>,
    extensions: BTreeMap<String, ExtensionRecord>,
    applied_job_ids: Vec<Uuid>,
}

impl VersionThreeProject {
    fn into_current(self) -> Result<Project> {
        ensure(
            self.pages
                .iter()
                .flat_map(|page| &page.regions)
                .all(|region| region.element.is_none() && !region.completed),
            "Legacy projects cannot contain element-editing metadata.",
        )?;
        let project = Project {
            schema_version: SCHEMA_VERSION,
            id: self.id,
            name: self.name,
            revision: self.revision,
            viewport: self.viewport,
            appearance: self.appearance,
            show_grid: self.show_grid,
            grid_settings: GridSettings::default(),
            generation_mode: self.generation_mode,
            home_page_id: self.home_page_id,
            pages: self.pages,
            brief: self.brief,
            website: None,
            imported_identity: None,
            pictures: BTreeMap::new(),
            decisions: self.decisions,
            exceptions: self.exceptions,
            extensions: self.extensions,
            applied_job_ids: self.applied_job_ids,
            latest_generation: None,
        };
        project.validate()?;
        Ok(project)
    }
}

impl VersionTwoProject {
    fn into_current(self) -> Result<Project> {
        ensure(
            self.pages
                .iter()
                .flat_map(|page| &page.regions)
                .all(|region| region.element.is_none() && !region.completed),
            "Legacy projects cannot contain element-editing metadata.",
        )?;
        let project = Project {
            schema_version: SCHEMA_VERSION,
            id: self.id,
            name: self.name,
            revision: self.revision,
            viewport: self.viewport,
            appearance: self.appearance,
            show_grid: self.show_grid,
            grid_settings: GridSettings::default(),
            generation_mode: self.generation_mode,
            home_page_id: self.home_page_id,
            pages: self
                .pages
                .into_iter()
                .map(|page| Page {
                    id: page.id,
                    name: page.name,
                    path: page.path,
                    included: page.included,
                    viewport: page.viewport,
                    appearance: page.appearance,
                    regions: page.regions,
                    content: None,
                })
                .collect(),
            brief: self.brief,
            website: None,
            imported_identity: None,
            pictures: BTreeMap::new(),
            decisions: self.decisions,
            exceptions: self.exceptions,
            extensions: self.extensions,
            applied_job_ids: vec![],
            latest_generation: None,
        };
        project.validate()?;
        Ok(project)
    }
}

impl LegacyProject {
    fn into_current(self) -> Result<Project> {
        let modes: Vec<_> = self
            .pages
            .iter()
            .flat_map(|page| {
                page.regions.iter().map(move |region| {
                    serde_json::json!({
                        "pageId": page.id,
                        "regionId": region.id,
                        "mode": region.mode
                    })
                })
            })
            .collect();
        let mut extensions = self.extensions;
        let mut namespace = LEGACY_GENERATION_NAMESPACE.to_owned();
        let mut suffix = 0_usize;
        while extensions.contains_key(&namespace) {
            suffix = suffix
                .checked_add(1)
                .ok_or_else(|| Error("Legacy extension namespace limit reached.".into()))?;
            namespace = format!("{LEGACY_GENERATION_NAMESPACE}.{suffix}");
        }
        extensions.insert(
            namespace,
            ExtensionRecord {
                schema_version: 1,
                data: serde_json::json!({"sourceSchemaVersion":1,"perAreaModes":modes}),
            },
        );
        VersionTwoProject {
            schema_version: 2,
            id: self.id,
            name: self.name,
            revision: self.revision,
            viewport: self.viewport,
            appearance: self.appearance,
            show_grid: self.show_grid,
            generation_mode: GenerationMode::Fuzzy,
            home_page_id: self.home_page_id,
            pages: self
                .pages
                .into_iter()
                .map(|page| VersionTwoPage {
                    id: page.id,
                    name: page.name,
                    path: page.path,
                    included: page.included,
                    viewport: page.viewport,
                    appearance: page.appearance,
                    regions: page
                        .regions
                        .into_iter()
                        .map(|region| Region {
                            pages: vec![],
                            shape: Default::default(),
                            area_bounds: vec![],
                            id: region.id,
                            rect: region.rect,
                            z_index: region.z_index,
                            prompt: region.prompt,
                            element: None,
                            frame_target: None,
                            elements: vec![],
                            allow_target_removal: false,
                            mobile_only: false,
                            runtime_errors: vec![],
                            whole_page: false,
                            whole_page_disabled: false,
                            attachments: vec![],
                            completed: false,
                        })
                        .collect(),
                })
                .collect(),
            brief: self.brief,
            decisions: self.decisions,
            exceptions: self.exceptions,
            extensions,
        }
        .into_current()
    }
}

pub fn decode_project(bytes: &[u8]) -> Result<DecodedProject> {
    decode_source(bytes, None, None)
}

pub fn decode_project_with_notes(bytes: &[u8], expected_notes: &[u8]) -> Result<DecodedProject> {
    decode_source(bytes, None, Some(expected_notes))
}

pub fn decode_project_with_source_hash(
    bytes: &[u8],
    expected_sha256: &str,
) -> Result<DecodedProject> {
    decode_source(bytes, Some(expected_sha256), None)
}

// Only canonical boilerplate changes: user-authored content is never normalized.
fn legacy_project_notes(notes: &str) -> String {
    notes.replacen(
        "# Project requirements\n\nAccepted project decisions maintained by Desegnos.\n\n",
        "# Project requirements\n\nAccepted project decisions maintained by Desegni.\n\n",
        1,
    ).replacen(
        "\n## Latest generation\n\nGenerated summary, identity proposals, and design feedback maintained by Desegnos.",
        "\n## Latest generation\n\nGenerated summary, identity proposals, and design feedback maintained by Desegni.",
        1,
    )
}

fn verify_source(
    source: &impl Serialize,
    notes: &str,
    expected_sha256: Option<&str>,
    expected_notes: Option<&[u8]>,
) -> Result<String> {
    let hash = digest(&serde_json::to_vec(source).map_err(|error| Error(error.to_string()))?);
    ensure(
        expected_sha256.is_none_or(|expected| expected == hash),
        "The project source failed its integrity check.",
    )?;
    ensure(
        expected_notes.is_none_or(|expected| {
            expected == notes.as_bytes() || expected == legacy_project_notes(notes).as_bytes()
        }),
        "The saved project notes disagree with the accepted project data.",
    )?;
    Ok(hash)
}

fn decode_source(
    bytes: &[u8],
    expected_sha256: Option<&str>,
    expected_notes: Option<&[u8]>,
) -> Result<DecodedProject> {
    let header: SchemaHeader =
        serde_json::from_slice(bytes).map_err(|error| Error(error.to_string()))?;
    let mut decoded = match header.schema_version {
        1 => {
            let legacy: LegacyProject =
                serde_json::from_slice(bytes).map_err(|error| Error(error.to_string()))?;
            let source_project_markdown = project_notes(
                legacy.id,
                &legacy.name,
                legacy.revision,
                None,
                &legacy.brief,
                &legacy.decisions,
                &legacy.exceptions,
            )?;
            let source_sha256 = verify_source(
                &legacy,
                &source_project_markdown,
                expected_sha256,
                expected_notes,
            )?;
            let project = legacy.into_current()?;
            Ok(DecodedProject {
                project,
                source_schema_version: 1,
                source_sha256,
                source_project_markdown,
            })
        }
        2 => {
            let legacy: VersionTwoProject =
                serde_json::from_slice(bytes).map_err(|error| Error(error.to_string()))?;
            let source_project_markdown = project_notes(
                legacy.id,
                &legacy.name,
                legacy.revision,
                Some(legacy.generation_mode),
                &legacy.brief,
                &legacy.decisions,
                &legacy.exceptions,
            )?;
            let source_sha256 = verify_source(
                &legacy,
                &source_project_markdown,
                expected_sha256,
                expected_notes,
            )?;
            Ok(DecodedProject {
                project: legacy.into_current()?,
                source_schema_version: 2,
                source_sha256,
                source_project_markdown,
            })
        }
        3 => {
            let legacy: VersionThreeProject =
                serde_json::from_slice(bytes).map_err(|error| Error(error.to_string()))?;
            let source_project_markdown = project_notes(
                legacy.id,
                &legacy.name,
                legacy.revision,
                Some(legacy.generation_mode),
                &legacy.brief,
                &legacy.decisions,
                &legacy.exceptions,
            )?;
            let source_sha256 = verify_source(
                &legacy,
                &source_project_markdown,
                expected_sha256,
                expected_notes,
            )?;
            Ok(DecodedProject {
                project: legacy.into_current()?,
                source_schema_version: 3,
                source_sha256,
                source_project_markdown,
            })
        }
        4 | 5 => {
            let raw: serde_json::Value =
                serde_json::from_slice(bytes).map_err(|error| Error(error.to_string()))?;
            ensure(
                raw.get("importedIdentity").is_none(),
                "Legacy projects cannot contain imported identity metadata.",
            )?;
            let mut project: Project =
                serde_json::from_slice(bytes).map_err(|error| Error(error.to_string()))?;
            ensure(
                header.schema_version == 5
                    || project
                        .pages
                        .iter()
                        .flat_map(|p| &p.regions)
                        .all(|r| r.element.is_none() && !r.completed),
                "Version 4 projects cannot contain element-editing metadata.",
            )?;
            let source = project.clone();
            project.schema_version = SCHEMA_VERSION;
            project.validate()?;
            let source_project_markdown = project.project_markdown()?;
            let source_sha256 = verify_source(
                &source,
                &source_project_markdown,
                expected_sha256,
                expected_notes,
            )?;
            if header.schema_version == 4
                && let Some(dossier) = &project.latest_generation
                && dossier.application_status == crate::GenerationApplicationStatus::Applied
                && dossier.applied_at_revision == Some(project.revision)
            {
                for page in &mut project.pages {
                    for region in &mut page.regions {
                        region.completed = dossier.targets.iter().any(|target| {
                            target.page_id == page.id && target.region_id == region.id
                        });
                    }
                }
            }
            Ok(DecodedProject {
                project,
                source_schema_version: header.schema_version,
                source_sha256,
                source_project_markdown,
            })
        }
        6 | 7 | 8 | 9 | SCHEMA_VERSION => {
            let mut project: Project =
                serde_json::from_slice(bytes).map_err(|error| Error(error.to_string()))?;
            let source = project.clone();
            project.schema_version = SCHEMA_VERSION;
            project.validate()?;
            let source_project_markdown = project.project_markdown()?;
            let source_sha256 = verify_source(
                &source,
                &source_project_markdown,
                expected_sha256,
                expected_notes,
            )?;
            Ok(DecodedProject {
                project,
                source_schema_version: header.schema_version,
                source_sha256,
                source_project_markdown,
            })
        }
        _ => Err(Error(
            "This project uses an unsupported schema version. The original has not been changed."
                .into(),
        )),
    }?;
    if let Some(notes) = expected_notes {
        decoded.source_project_markdown =
            String::from_utf8(notes.to_vec()).map_err(|error| Error(error.to_string()))?;
    }
    if decoded.source_schema_version < SCHEMA_VERSION {
        decoded.project.assign_stable_targets()?;
    }
    decoded.project.store_inline_images()?;
    decoded.project.verify_picture_bytes()?;
    decoded.project.validate()?;
    Ok(decoded)
}
