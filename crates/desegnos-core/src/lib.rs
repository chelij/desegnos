mod adapt;
mod attachments;
mod commands;
mod dossier;
mod element_edit;
mod element_style;
mod elements;
mod export;
mod generation;
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
mod session;
mod text_layout;
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
    Element, ElementKind, ElementLayout, ElementMeta, ElementMove, ElementOverride, ElementPadding,
    ElementPatch, ElementShadow, ElementSizing, ElementStroke, ElementStyle, ElementTextStyle,
    InputType, LinkLocation, LinkedDefinition, LinkedPasteInstance, MAX_ELEMENT_CODE_CHARS,
    MAX_ELEMENT_CONTEXT, MAX_ELEMENT_DEPTH, MAX_ELEMENT_NAME_CHARS, MAX_ELEMENT_OPTIONS,
    MAX_ELEMENT_REQUESTS, MAX_ELEMENTS, MAX_LINKED_DEFINITIONS, RequestRecord, SizingMode,
    StackAlign, StackDirection, StackJustify, StyleReferences, TextAlign, TextDecoration,
    default_element_size, default_element_text, default_layer_name_base, element_outline,
    element_subtree, empty_element, is_default_layer_name, link_location, nested_element_id,
    resolve_element, resolve_elements, resolve_for_size, resolve_shared_styles,
};
pub use export::{
    CommentReply, EXPORT_MANIFEST_FILE, EXPORT_MANIFEST_VERSION, ExportAssetPlan, ExportBehaviour,
    ExportBundle, ExportPagePlan, ExportPlan, ExportPlanKind, ExportPointerPlan, ExportReply,
    ExportReplyPage, ExportReplyPointer, ExportReplyShared, MAX_EXPORT_BYTES,
    MAX_EXPORT_FILE_BYTES, MAX_EXPORT_FILES, WebsiteExport, ai_export_bundle,
    behaviour_fingerprint, direct_export_bundle, export_manifest, export_schema, file_stem,
    included_pages, is_vector_representable, marked_pointer_code, page_file, parse_comment_reply,
    parse_export_reply, plan_comment, plan_export, plan_snapshot, pointer_fingerprints,
    preview_page_ids, reused_export_behaviour_count, slide_order, validate_behaviours,
    validate_export_reply,
};
pub use generation::{
    ClassifiedAction, DecisionProposal, DesignCriticism, ElementDraft, ElementOperation,
    ElementUpdate, GeneratedImage, GenerationCandidate, GenerationExpectations,
    GenerationValidationError, MAX_APPLIED_JOBS, MAX_DECISION_PROPOSALS, MAX_DESIGN_CRITICISMS,
    MAX_GENERATED_IMAGES, MAX_GENERATION_BYTES, MAX_GENERATION_REQUESTS,
    MAX_POINTER_CLASSIFICATIONS, MAX_REQUIREMENTS_BYTES, PointerClassification, ReferenceDraft,
    RequestOutcome, RequestStatus, ValidationEvent, ValidationEventKind, ValidationFailureKind,
    apply_generation_output, candidate_schema, conform_to_schema, result_schema,
    validation_failure_kind,
};
pub use identity::{
    IdentityAsset, ImportedContext, ImportedIdentity, ImportedItem, ImportedPageContext,
    MAX_IDENTITY_ASSET_BYTES, MAX_IDENTITY_ASSETS, MAX_IMPORTED_ITEMS, MAX_PHOTO_DESCRIPTION_CHARS,
    MAX_PROJECT_PHOTO_BYTES, ProjectPhoto, SourcePage, SourcePageRef,
};
pub use identity_pages::{
    identity_source_page, page_frontmatter, page_references_image, source_page_at_route,
    source_page_kind, source_page_path, validate_source_page_ref,
};
pub use image_generation::{ImageGenerationRequest, MAX_DOWNLOADED_IMAGE_BYTES};
pub use json_parse::{
    complete_truncated_operations, fill_reply_echoes, parse_provider_json, parse_structured_reply,
};
pub use model::{
    Appearance, BoundaryKind, Brief, CanvasPlacement, CanvasTemplate, ColourStyle, Decision,
    DesignException, ExportRecord, ExportedPage, ExportedPointer, GenerationMode, GridSettings,
    GridUnit, MAX_EXPORT_DESTINATION, MAX_NAMED_VIEW_NAME_CHARS, MAX_NAMED_VIEWS, MAX_PAGE_SIZES,
    MAX_PAGES, MAX_POINTERS, MAX_PROMPT_CHARS, MAX_REFERENCE_ITEMS, MAX_REQUEST_TARGETS,
    MAX_REQUESTS, MAX_VIEWPORT_SIDE, NamedView, OutputType, Page, PageSize, PendingRequest,
    PlayAction, Pointer, PointerKind, PointerSource, PointerTarget, PointerWiring, Project,
    ProjectStyles, Rect, ReferenceItem, ReferenceKind, ReferencePlacement, RequestArea,
    RequestPoint, RequestShape, RequestTarget, SCHEMA_VERSION, SIZE_LABELS, Scope, SizePreset,
    TEMPLATE_GAP, TextStyle, Viewport, size_label,
};
pub use picture_scaling::{
    AI_PREVIEW_BYTES, AI_PREVIEW_EDGE, AI_PREVIEW_TOKEN_RESERVE, ai_preview, scaled_for_display,
};
pub use pointers::{
    PointerState, PointerStateView, broken_pointers, pointed_reference_ids, pointer_context_value,
    pointer_grants_change, pointer_is_broken, pointer_needs_classifying,
    pointer_target_description, pointer_target_value, reference_text, source_description,
};
pub use project_assets::{PROJECT_ASSET_SCHEME, project_asset_reference};
pub use render::{
    MAX_RENDER_EDGE, MAX_RENDER_PIXELS, RenderPage, ResolvedFont, Rgba, Scene, boundary_png,
    boundary_svg, font_families, resolve_font, slide_pdf, standalone_face,
};
pub use request_history::{
    EarlierRequest, FrozenLayerRequest, REQUEST_HISTORY_CHARS, RejectionKind,
    RequestHistoryOutcome, compact_request_history, earlier_requests, record_validation_rejection,
};
pub use requirements::{
    CURRENT_REQUIREMENTS_FORMAT, GenerationCapabilities, ImageGenerationCapability, RequestImage,
    RequirementsSnapshot, current_requirements_heading, fenced_json,
};
pub use session::{HistoryEntry, Session, SessionHistory};
pub use website::{WebsiteAssets, WebsiteLinks, css_urls, website_assets, website_links};
pub use website_import::{
    ImportedWebsite, WebAttributes, WebCapture, WebNode, WebScript, WebStyles, import_website,
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

pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn required_nullable<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(deserializer)
}

pub(crate) fn ensure(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error(message.into()))
    }
}

pub(crate) use model::validate_text;
