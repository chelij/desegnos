use crate::*;
use std::collections::BTreeMap;
use uuid::Uuid;

impl Project {
    pub fn new(name: String, viewport: Viewport, appearance: Appearance) -> Result<Self> {
        viewport.validate()?;
        let mut project = Self::untitled(name, appearance, CanvasTemplate::Website)?;
        project.pages[0].sizes.truncate(1);
        let base = project.pages[0]
            .sizes
            .first_mut()
            .expect("website template has a size");
        base.width = viewport.width;
        base.height = viewport.height;
        base.label = size_label(viewport.width).into();
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
        let project = Self {
            schema_version: SCHEMA_VERSION,
            template,
            id: Uuid::new_v4(),
            name: name.trim().into(),
            revision: 0,
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
            applied_job_ids: vec![],
            latest_generation: None,
            last_export: None,
        };
        project.validate()?;
        Ok(project)
    }
}
