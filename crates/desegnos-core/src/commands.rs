use crate::model::normalized_swatch;
use crate::{
    Error, Result, attachments::RequestAttachment, element_edit::ElementMove,
    element_edit::ElementPatch, elements::Element, elements::LinkedPasteInstance,
    limits::MAX_NAMED_VIEWS, limits::is_hex_colour, model::BoundaryKind, model::Brief,
    model::CanvasPlacement, model::ColourStyle, model::Decision, model::DesignException,
    model::ExportRecord, model::ExportedPointer, model::GenerationMode, model::GridSettings,
    model::NamedView, model::OutputType, model::Page, model::PageSize, model::PendingRequest,
    model::Pointer, model::PointerKind, model::PointerSource, model::PointerTarget,
    model::PointerWiring, model::Project, model::Rect, model::ReferenceItem, model::ReferenceKind,
    model::ReferencePlacement, model::Scope, model::TextStyle, validation::ensure,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(ts_rs::TS, Debug, Clone, Deserialize, Serialize)]
#[ts(optional_fields = nullable)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    AddColourStyle {
        style: ColourStyle,
    },
    UpdateColourStyle {
        style: ColourStyle,
    },
    RemoveColourStyle {
        id: Uuid,
    },
    AddTextStyle {
        style: TextStyle,
    },
    UpdateTextStyle {
        style: TextStyle,
    },
    RemoveTextStyle {
        id: Uuid,
    },
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
    /// Rebuild this size's automatic responsive overrides as one undoable edit.
    AdaptSize {
        page_id: Uuid,
        size_id: Uuid,
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
        #[ts(optional = nullable)]
        moves: Vec<CanvasPlacement>,
        #[serde(default)]
        #[ts(optional = nullable)]
        reference: Vec<ReferencePlacement>,
    },
    /// Add a page and place it on the project canvas in one step. `moves` carries the
    /// boundaries the new one pushes aside, so drawing is a single undoable gesture.
    CreateBoundary {
        page: Page,
        #[serde(default)]
        #[ts(optional = nullable)]
        after_page_id: Option<Uuid>,
        #[serde(default)]
        #[ts(optional = nullable)]
        moves: Vec<CanvasPlacement>,
        #[serde(default)]
        #[ts(optional = nullable)]
        reference: Vec<ReferencePlacement>,
    },
    AddCanvasPage {
        name: String,
        path: String,
        size: PageSize,
        #[serde(default)]
        #[ts(optional = nullable)]
        moves: Vec<CanvasPlacement>,
        /// Reference items the new boundary pushes aside, in the same undo step.
        #[serde(default)]
        #[ts(optional = nullable)]
        reference: Vec<ReferencePlacement>,
    },
    /// Add a linked size to a page, placed to the right of it, with the neighbours it pushes.
    AddPageSize {
        page_id: Uuid,
        size: PageSize,
        #[serde(default)]
        #[ts(optional = nullable)]
        moves: Vec<CanvasPlacement>,
        #[serde(default)]
        #[ts(optional = nullable)]
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
        #[ts(optional = nullable)]
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
        #[ts(optional = nullable)]
        size_id: Option<Uuid>,
    },
    /// A note, link, colour or picture dragged into a boundary becomes a layer there.
    ReferenceToElement {
        item_id: Uuid,
        page_id: Uuid,
        rect: Rect,
        #[serde(default)]
        #[ts(optional = nullable)]
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
    NudgeElements {
        moves: Vec<ElementMove>,
        dx: f64,
        dy: f64,
    },
    StackElements {
        page_id: Uuid,
        ids: Vec<Uuid>,
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
        #[ts(optional = nullable)]
        elements: Vec<(Uuid, Uuid)>,
        #[serde(default)]
        #[ts(optional = nullable)]
        reference_ids: Vec<Uuid>,
        #[serde(default)]
        #[ts(optional = nullable)]
        pointer_ids: Vec<Uuid>,
        #[serde(default)]
        #[ts(optional = nullable)]
        request_ids: Vec<Uuid>,
        #[serde(default)]
        #[ts(optional = nullable)]
        page_ids: Vec<Uuid>,
        #[serde(default)]
        #[ts(optional = nullable)]
        size_ids: Vec<(Uuid, Uuid)>,
        #[serde(default)]
        #[ts(optional = nullable)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryBurst {
    Edit(String),
    Nudge(String),
    FirstText(Uuid),
}

impl HistoryBurst {
    pub(crate) fn pause(&self) -> Option<std::time::Duration> {
        match self {
            Self::Nudge(_) => Some(std::time::Duration::from_millis(500)),
            Self::Edit(_) => Some(std::time::Duration::from_secs(5)),
            Self::FirstText(_) => None,
        }
    }
    pub(crate) fn survives_focus_change(&self) -> bool {
        matches!(self, Self::FirstText(_))
    }
}

pub(crate) struct CommandHistory {
    pub label: &'static str,
    pub burst: Option<HistoryBurst>,
}

impl Command {
    pub(crate) fn history(&self, project: &Project) -> CommandHistory {
        CommandHistory {
            label: self.version_label(),
            burst: self.history_burst(project),
        }
    }

    fn history_burst(&self, project: &Project) -> Option<HistoryBurst> {
        match self {
            Command::NudgeElements { moves, .. } => {
                let mut targets: Vec<_> = moves
                    .iter()
                    .map(|movement| {
                        format!(
                            "{}:{:?}:{}",
                            movement.id, movement.size_id, movement.all_sizes
                        )
                    })
                    .collect();
                targets.sort();
                Some(HistoryBurst::Nudge(targets.join(",")))
            }
            // Only the text of a request changed (its targets and attachments stayed put).
            Command::UpdateRequest { request } => project
                .requests
                .iter()
                .find(|existing| existing.id == request.id)
                .filter(|existing| {
                    existing.attachments == request.attachments
                        && existing.targets == request.targets
                })
                .map(|existing| HistoryBurst::Edit(format!("request:{}", existing.id))),
            Command::UpdatePage { page_id, .. } => {
                Some(HistoryBurst::Edit(format!("page:{page_id}")))
            }
            Command::UpdatePageSize {
                page_id, size_id, ..
            } => Some(HistoryBurst::Edit(format!("size:{page_id}:{size_id}"))),
            // Only the comment changed; endpoints stayed put.
            Command::UpdatePointer {
                pointer_id,
                from: None,
                to: None,
                ..
            } => Some(HistoryBurst::Edit(format!("pointer-comment:{pointer_id}"))),
            Command::RenameProject { .. } => Some(HistoryBurst::Edit("project-name".into())),
            Command::SetGridSettings { .. } => Some(HistoryBurst::Edit("grid".into())),
            // Placing a layer and typing its first text is one step, however long the typing.
            Command::InsertElements { elements, .. } if elements.len() == 1 => {
                Some(HistoryBurst::FirstText(elements[0].id))
            }
            Command::UpdateElements { patches, .. }
                if patches.len() == 1 && patches[0].text.is_some() =>
            {
                Some(HistoryBurst::FirstText(patches[0].id))
            }
            _ => None,
        }
    }

    fn version_label(&self) -> &'static str {
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
            Self::AdaptSize { .. } => "Adapted layout",
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
            Self::NudgeElements { .. } => "Nudged elements",
            Self::StackElements { .. } => "Added Stack",
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
    pub fn apply(&self, expected_revision: u64, command: Command) -> Result<Self> {
        ensure(
            expected_revision == self.revision,
            "The project changed. Refresh the document before applying this edit.",
        )?;
        let mut next = self.clone();
        next.revision = crate::session::next_revision(next.revision)?;
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
                    .ok_or_else(|| missing("style"))? = style;
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
                    .ok_or_else(|| missing("style"))? = style;
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
                ensure(
                    is_hex_colour(&background),
                    "Boundary background must be a hex colour.",
                )?;
                p.background = normalized_swatch(&background);
                p.included = included;
            }
            Command::SetPageOutput { page_id, output } => {
                next.page_mut(page_id)?.output = output;
            }
            Command::AdaptSize { page_id, size_id } => {
                let styles = next.styles.clone();
                crate::adapt::adapt_with_styles(next.page_mut(page_id)?, size_id, &styles)?;
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
                let size = p.size_mut(size_id).ok_or_else(|| missing("size"))?;
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
                    next.pages
                        .iter()
                        .position(|existing| existing.id == id)
                        .ok_or_else(|| missing("boundary"))?
                        + 1
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
                    background: crate::limits::DEFAULT_PAGE_BACKGROUND.into(),
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
                size.validate()?;
                page.sizes.push(size);
                for placement in moves {
                    placement.place(&mut next.pages)?;
                }
                next.place_reference(reference)?;
            }
            Command::RemovePageSize { page_id, size_id } => {
                next.remove_page_size(page_id, size_id)?;
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
            Command::AddNamedView { name, x, y, zoom } => {
                ensure(
                    next.views.len() < MAX_NAMED_VIEWS,
                    format!(
                        "A project holds at most {} named views.",
                        crate::limits::MAX_NAMED_VIEWS
                    ),
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
                    .ok_or_else(|| missing("view"))?;
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
                exists(next.views.iter().any(|view| view.id == view_id), "view")?;
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
                    exists(
                        next.reference.iter().any(|item| item.id == *id),
                        "reference item",
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
                    exists(
                        next.pointers.iter().any(|pointer| pointer.id == *id),
                        "pointer",
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
                    exists(
                        next.pointers.iter().any(|pointer| pointer.id == *id),
                        "pointer",
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
                for (index, mut page) in pages.into_iter().enumerate() {
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
                    // Imported layers are new layers too: number the ones without a name.
                    let mut used = crate::element_edit::used_default_names(&next);
                    crate::element_edit::assign_default_names(&mut page.elements, &mut used);
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
            Command::NudgeElements { moves, dx, dy } => next.nudge_elements(moves, dx, dy)?,
            Command::StackElements { page_id, ids } => next.stack_elements(page_id, ids)?,
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
                    exists(
                        next.requests.iter().any(|request| request.id == id),
                        "request",
                    )?;
                    next.requests.retain(|request| request.id != id);
                }
                let mut by_page: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
                for (page_id, element_id) in elements {
                    next.page(page_id)?;
                    exists(
                        crate::elements::find_element(&next.page(page_id)?.elements, element_id)
                            .is_some(),
                        "layer",
                    )?;
                    by_page.entry(page_id).or_default().push(element_id);
                }
                for (page_id, ids) in by_page {
                    next.remove_elements(page_id, ids)?;
                }
                for id in reference_ids {
                    exists(
                        next.reference.iter().any(|item| item.id == id),
                        "reference item",
                    )?;
                    next.reference.retain(|item| item.id != id);
                }
                for id in pointer_ids {
                    exists(
                        next.pointers.iter().any(|pointer| pointer.id == id),
                        "pointer",
                    )?;
                    next.pointers.retain(|pointer| pointer.id != id);
                }
                for id in linked_ids {
                    next.delete_linked_definition(id)?;
                }
                for (page_id, size_id) in size_ids {
                    next.remove_page_size(page_id, size_id)?;
                }
                for page_id in page_ids {
                    exists(next.pages.iter().any(|page| page.id == page_id), "boundary")?;
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
                exists(
                    next.decisions.iter().any(|d| d.id == decision_id),
                    "decision",
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
                exists(
                    next.exceptions.iter().any(|e| e.id == exception_id),
                    "exception",
                )?;
                next.exceptions.retain(|e| e.id != exception_id);
            }
        }
        next.prune_picture_store()?;
        next.validate()?;
        if crate::text_layout::sync_fit_rects(&mut next) {
            next.validate()?;
        }
        crate::adapt::adapt_changed_pages(self, &mut next)?;
        next.validate()?;
        Ok(next)
    }
}

fn missing(what: &str) -> Error {
    Error(format!("This {what} no longer exists."))
}

fn exists(found: bool, what: &str) -> Result<()> {
    if found { Ok(()) } else { Err(missing(what)) }
}

impl Project {
    fn remove_page_size(&mut self, page_id: Uuid, size_id: Uuid) -> Result<()> {
        let page = self.page_mut(page_id)?;
        ensure(page.sizes.len() > 1, "A boundary keeps at least one size.")?;
        ensure(
            page.base_size_id() != size_id,
            "The base size cannot be removed.",
        )?;
        exists(page.size(size_id).is_some(), "size")?;
        page.sizes.retain(|size| size.id != size_id);
        for element in page.elements.iter_mut() {
            clear_size_overrides(element, size_id);
        }
        for definition in self.linked.values_mut() {
            clear_size_overrides(&mut definition.master, size_id);
        }
        Ok(())
    }
}

fn clear_size_overrides(element: &mut Element, size_id: Uuid) {
    element.overrides.remove(&size_id);
    for child in &mut element.children {
        clear_size_overrides(child, size_id);
    }
}
