mod adapt;
mod attachments;
mod commands;
mod dossier;
mod element_edit;
mod element_style;
mod elements;
mod export;
mod generation;
mod generation_apply;
mod generation_schema;
mod generation_scope;
mod geometry;
mod identity;
mod identity_pages;
mod image_generation;
mod json_parse;
pub mod limits;
mod model;
mod picture_scaling;
mod pointer_edit;
mod pointers;
mod project_assets;
mod project_new;
mod project_styles;
mod reference_edit;
mod render;
mod request_edit;
mod request_history;
mod requirements;
mod schema_conformance;
mod session;
mod svg_validation;
mod text_layout;
mod validation;
mod web_codegen;
mod website;
mod website_import;

pub use adapt::adapt_size;
pub use attachments::{AttachmentKind, RequestAttachment};
pub use commands::Command;
pub use dossier::{
    DossierTarget, GenerationApplicationStatus, GenerationDossier, GenerationDossierContext,
};
pub use elements::{
    Element, ElementKind, ElementLayout, ElementMeta, ElementOverride, ElementPadding,
    ElementShadow, ElementSizing, ElementStroke, ElementStyle, ElementTextStyle, InputType,
    LinkedDefinition, LinkedPasteInstance, RequestRecord, SizingMode, StackAlign, StackDirection,
    StackJustify, StyleReferences, TextAlign, TextDecoration, default_element_size,
    default_element_text, default_layer_name_base, is_default_layer_name, link_location,
    nested_element_id, resolve_element, resolve_for_size,
};
pub use export::{
    EXPORT_MANIFEST_FILE, ExportBehaviour, ExportBundle, ExportPlan, ExportPlanKind, ExportReply,
    ExportReplyPage, ExportReplyPointer, ExportReplyShared, WebsiteExport, ai_export_bundle,
    behaviour_fingerprint, direct_export_bundle, export_manifest, export_schema, file_stem,
    marked_pointer_code, page_file, parse_comment_reply, parse_export_reply, plan_comment,
    plan_export, plan_snapshot, pointer_fingerprints, preview_page_ids,
    reused_export_behaviour_count, validate_export_reply,
};
pub use generation::{
    ClassifiedAction, DecisionProposal, DesignCriticism, ElementDraft, ElementOperation,
    ElementUpdate, GeneratedImage, GenerationCandidate, GenerationExpectations,
    GenerationValidationError, PointerClassification, ReferenceDraft, RequestOutcome,
    RequestStatus, ValidationEvent, ValidationEventKind,
};
pub use identity::{
    IdentityAsset, ImportedContext, ImportedIdentity, ImportedItem, ImportedPageContext,
    ProjectPhoto, SourcePage, SourcePageRef, raster_dimensions,
};
pub use identity_pages::{
    identity_source_page, page_references_image, source_page_at_route, source_page_kind,
    source_page_path, validate_source_page_ref,
};
pub use image_generation::ImageGenerationRequest;
pub use json_parse::{complete_truncated_operations, parse_provider_json, parse_structured_reply};
pub use model::{
    Appearance, BoundaryKind, Brief, CanvasPlacement, CanvasTemplate, ColourStyle, Decision,
    DesignException, ExportRecord, ExportedPage, ExportedPointer, GenerationMode, GridSettings,
    GridUnit, NamedView, OutputType, Page, PageSize, PendingRequest, PlayAction, Pointer,
    PointerKind, PointerSource, PointerTarget, PointerWiring, Project, ProjectStyles, Rect,
    ReferenceItem, ReferenceKind, ReferencePlacement, RequestArea, RequestPoint, RequestShape,
    RequestTarget, SIZE_LABELS, Scope, SizePreset, TEMPLATE_GAP, TextStyle, Viewport, size_label,
};

pub use pointers::{PointerState, PointerStateView, broken_pointers, pointer_needs_classifying};

pub use geometry::Rgba;
pub use request_history::{
    EarlierRequest, FrozenLayerRequest, RejectionKind, RequestHistoryOutcome,
    compact_request_history, earlier_requests, record_validation_rejection,
};
pub use requirements::{
    CURRENT_REQUIREMENTS_FORMAT, GenerationCapabilities, ImageGenerationCapability, RequestImage,
    RequirementsSnapshot, current_requirements_heading, fenced_json,
};
pub use session::{HistoryEntry, Session};
pub use text_layout::{ResolvedFont, font_families, resolve_font, standalone_face};
pub use website::{css_urls, website_assets, website_links};
pub use website_import::{
    ImportedWebsite, WebCapture, WebNode, WebScript, WebStyles, import_website,
    web_capture_image_sources,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Error {}

pub use generation_apply::apply_generation_output;
pub use generation_schema::{candidate_schema, result_schema};

pub use element_edit::{ElementMove, ElementPatch, empty_element};

pub use validation::digest;
