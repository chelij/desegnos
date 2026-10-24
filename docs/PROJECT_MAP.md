# Desegnos project map

A source map for people and AI agents. Paths below are relative to this document; ownership and flows follow the current code, rather than older design documents.

## Architecture

The [Rust workspace](../Cargo.toml) has three library crates and the Tauri desktop application. Names on disk still use `desegni`; the product is Desegnos.

- **`desegni-core`** owns the project model, revision rules, generation contracts, source editing and portable website compilation. It has no desktop or provider authority.
- **`desegni-storage`** owns project archives, recovery snapshots, filesystem safety, managed project folders and website output.
- **`desegni-ai`** owns provider requests, credentials, ChatGPT authentication, streaming replies and image adapters.
- **Tauri** owns native state, dialogs, orchestration and preview resources. [main.rs](../src-tauri/src/main.rs) initializes services and registers commands and the `desegni-preview` protocol.
- **React/TypeScript** owns the editor and its interaction state. [main.tsx](../src/main.tsx) mounts [App.tsx](../src/App.tsx); [api.ts](../src/api.ts) wraps Tauri `invoke` in `native()` and `edit()`. [types.ts](../src/types.ts) mirrors native payloads.

Typical boundary: component → `native(command, args)` → Tauri command → library operation → serialized view. `WorkspaceView` returns the authoritative project and history state. Mutations normally carry an expected revision; `Session` persists a recovery snapshot before publishing the new state.

## Source directory tree

Only meaningful source and documentation folders are shown.

```text
crates/
├── desegni-core/src/       Project model, transformations, contracts and compilation.
├── desegni-storage/src/    Archives, journals, project folders and filesystem imports/exports.
└── desegni-ai/src/         Provider transports, settings, credentials and images.
    └── chatgpt/            Sign-in, account lifecycle, token storage and HTTP/callback handling.
src-tauri/
└── src/                   Desktop commands, native services and preview protocol/bridge.
src/                       React editor, canvas interactions, dialogs and frontend utilities.
└── assets/brand/           Bundled wordmarks and Archivo font.
scripts/                   Toolchain/build wrappers, generation benchmark and usage reporting.
docs/                      Technical guides, contracts, feature notes and this map.
└── archive/               Historical release-plan snapshots.
```

Links: [core](../crates/desegni-core/src/), [storage](../crates/desegni-storage/src/), [AI](../crates/desegni-ai/src/), [ChatGPT](../crates/desegni-ai/src/chatgpt/), [desktop](../src-tauri/src/), [frontend](../src/), [brand assets](../src/assets/brand/), [scripts](../scripts/), [docs](./), [archive](archive/).

## Library ownership and entry points

### `desegni-core`

[lib.rs](../crates/desegni-core/src/lib.rs) re-exports the public domain API. Most operations transform a validated `Project`; `Session` supplies transaction/history semantics.

| Module | Owns; key types/functions |
| --- | --- |
| [model.rs](../crates/desegni-core/src/model.rs) | Persisted `Project`, `Page`, `Region`, `ElementTarget`, `Command`, `GenerationMode`; `Project::apply`, `validate`, `Region::is_whole_page`. |
| [session.rs](../crates/desegni-core/src/session.rs) | `Session`, revision checks, undo/redo and durable mutations; `execute`, `apply_generation`, `apply_generation_draft`, `undo`, `redo`. |
| [document.rs](../crates/desegni-core/src/document.rs) | `DocumentTarget` (page or editable site HTML file), `FrameTarget`, `PageFrameTarget`; `frame_targets`, `adopt_frame_targets`, site document bytes, framed generation document source. |
| [document_context.rs](../crates/desegni-core/src/document_context.rs) | `DocumentContext`, `IncludedDocument`, `DocumentReason`, `SharedStylesheet`, `DocumentBatch`; editable-document selection for both modes: the request's page picker (`PageSelection`) always applies, Flexible adds the documents a framed request clearly concerns, and `MAX_DOCUMENT_CONTEXT_BYTES` bounds the set. |
| [requirements.rs](../crates/desegni-core/src/requirements.rs) | Frozen `RequirementsSnapshot`, provider `GenerationCapabilities`, request/context Markdown; `prepare_requirements_with_capabilities`, `prepare_targeted_requirements`, `validate_current`. |
| [generation.rs](../crates/desegni-core/src/generation.rs) | `SiteContent`, `GenerationCandidate`, `RegionOutcome`, proposals/criticism; candidate parsing/validation, `Project::apply_generation`, `candidate_schema`. |
| [element_fix.rs](../crates/desegni-core/src/element_fix.rs) | Selected-element reply format and expansion; `result_schema`, `context_follow_up`, `adopt_follow_up_reply`, `GenerationCandidate::from_reply`. |
| [generation_scope.rs](../crates/desegni-core/src/generation_scope.rs) | Structural HTML/CSS scope enforcement: unchanged outside content, local selectors, whole-page authority and Flexible rules. |
| [generation_recovery.rs](../crates/desegni-core/src/generation_recovery.rs) | `ValidationEvent`, `recover_generation_reply`, `validation_repair`, `merge_validation_repair`, `conflicted_generation`; salvage independently usable work. |
| [generation_draft.rs](../crates/desegni-core/src/generation_draft.rs) | Rejected-but-retained `GenerationDraft`; `retain_generation_drafts`, `check_source`, `rebased_content`, `apply_generation_draft`. |
| [request_history.rs](../crates/desegni-core/src/request_history.rs) | `EarlierRequest`, `source_fingerprints`, `earlier_requests`, `compact_request_history`, `with_earlier_requests`; relevant prior requests in new instructions. |
| [dossier.rs](../crates/desegni-core/src/dossier.rs) | `GenerationDossier`, `GenerationDossierContext`, application status and `record_generation`; generated brief context. |
| [editing.rs](../crates/desegni-core/src/editing.rs) | `ElementEdit`, `ElementDetails`, `ElementBounds`; inspect/edit text, links, images, inline styles and order. |
| [stable_targets.rs](../crates/desegni-core/src/stable_targets.rs) | Stable editor IDs and measured `TargetBounds`; `canonical_targets`, `delete_elements`, `refresh_element_target_bounds`. |
| [across_pages.rs](../crates/desegni-core/src/across_pages.rs) | `AcrossPagesMatch`, `AcrossPagesEdit`, `AcrossPagesChange`; `element_matches`, `edit_element_across`. One element change reaches the same element on other pages and framed site files in one revision. |
| [source.rs](../crates/desegni-core/src/source.rs) | `PageSourceEdit`, `SourceEditError`, `PageSourceMap`; `validate_page_source`, `edit_page_source`, `page_source_map`. |
| [code_format.rs](../crates/desegni-core/src/code_format.rs) | `format_site_content`, `compact_site_content`; source presentation/normalization. |
| [layers.rs](../crates/desegni-core/src/layers.rs) | `LayerTarget`, `LayerDirection`, `LayerClipboard`; layer ordering, `copy_layer`, `paste_layer`. |
| [area_geometry.rs](../crates/desegni-core/src/area_geometry.rs) | Shape polygons, `overlaps_rect`, `elements_in_area`; precise shaped selection. |
| [region_scripts.rs](../crates/desegni-core/src/region_scripts.rs), [runtime JS](../crates/desegni-core/src/region_script_runtime.js) | Request-owned inert script blocks, ownership/syntax validation and `region_script_runtime`. |
| [identity.rs](../crates/desegni-core/src/identity.rs), [identity_pages.rs](../crates/desegni-core/src/identity_pages.rs) | `ImportedIdentity`, `ImportedContext`, `IdentityAsset`, `ProjectPhoto`, source-page attribution; `import_context`, `identity_source_page`. |
| [attachments.rs](../crates/desegni-core/src/attachments.rs) | `RequestAttachment`, `AttachmentKind`; bounded request text, images and links. |
| [project_assets.rs](../crates/desegni-core/src/project_assets.rs) | `desegnos-asset:` references, inline-image storage, asset catalogues and `resolve_site_content`. |
| [images.rs](../crates/desegni-core/src/images.rs), [image_generation.rs](../crates/desegni-core/src/image_generation.rs), [picture_scaling.rs](../crates/desegni-core/src/picture_scaling.rs) | Image URL/request extraction, embedding, masks/references and display/AI scaling (`ai_preview`). |
| [provenance.rs](../crates/desegni-core/src/provenance.rs) | Found-content source attribution and placeholder metrics; `found_sources`, `found_source`. |
| [website_import.rs](../crates/desegni-core/src/website_import.rs), [website_merge.rs](../crates/desegni-core/src/website_merge.rs) | `ImportedWebsite`, original file mapping, canvas/live projections; `import_website_files`, `review_website_import`, `import_website`. |
| [website.rs](../crates/desegni-core/src/website.rs) | `WebsiteLinks`, `website_links`; extract links/resources from HTML. |
| [export.rs](../crates/desegni-core/src/export.rs) | `WebsiteExport`, `compile_website`; portable pages/assets, route rewriting, editor-metadata removal and warnings. |
| [migration.rs](../crates/desegni-core/src/migration.rs) | `DecodedProject`, `decode_project*`; schema upgrades and migration notices. |

### `desegni-storage`

| Module | Owns; key types/functions |
| --- | --- |
| [lib.rs](../crates/desegni-storage/src/lib.rs) | `ProjectFile`, `FileStamp`, `SaveFailure`, `Journal`; locks, checked atomic archive saves, previous copies, loading and recovery. |
| [project_folder.rs](../crates/desegni-storage/src/project_folder.rs) | Managed site manifest and `OutsideEdits`; `create_project_folder`, `manage_project_folder`, `sync_project_site`, `import_website_folder`. |
| [website_detection.rs](../crates/desegni-storage/src/website_detection.rs) | `WebsiteFolder`, `detect_website_folder`, `import_detected_website`; source/build-output discovery. |
| [website_merge.rs](../crates/desegni-storage/src/website_merge.rs) | `read_website_selection`; bounded selected-file reading for page merge. |
| [identity_source.rs](../crates/desegni-storage/src/identity_source.rs) | `IdentitySource` and source assets/files/photos; `collect_identity_source`, `identity_source_from_files_attributed`. |
| [request_zip.rs](../crates/desegni-storage/src/request_zip.rs) | `ZipAttachments`, `SkippedFile`, `unpack_request_zip`; bounded attachment extraction. |
| [website_export.rs](../crates/desegni-storage/src/website_export.rs) | `write_website_export`, `WrittenWebsiteExport::verify`; safe output publication and receipts. |
| [system_tools.rs](../crates/desegni-storage/src/system_tools.rs) | `ToolSearch`, executable resolution and `build_command` for imported-site preparation. |

### `desegni-ai`

| Module | Owns; key types/functions |
| --- | --- |
| [providers.rs](../crates/desegni-ai/src/providers.rs) | `ProviderId`, `ProviderClient`, capabilities, model catalogues and provider-specific requests; OpenAI, Anthropic and Google are available. |
| [transport.rs](../crates/desegni-ai/src/transport.rs) | `OpenAiClient`, `CancellationToken`, `ProviderResponse`, `ProviderProgress`; `generate_for_auth`, `analyze_identity_for_auth`, access/model discovery. |
| [settings.rs](../crates/desegni-ai/src/settings.rs) | `ProviderSettings`, `AuthenticationMethod`, `TokenUsage`, cost estimates and request/output limits. |
| [credentials.rs](../crates/desegni-ai/src/credentials.rs) | `ApiKey`, `CredentialManager`, `CredentialStorage`; masked status, system-vault or session-only keys. |
| [error.rs](../crates/desegni-ai/src/error.rs), [sse.rs](../crates/desegni-ai/src/sse.rs) | `ProviderError`, submission-outcome classification, scrubbed diagnostics and bounded `SseDecoder`. |
| [identity.rs](../crates/desegni-ai/src/identity.rs), [website.rs](../crates/desegni-ai/src/website.rs) | Brand-analysis image/token budgeting; `fetch_website`, `WebsiteFiles`, bounded web-source collection. |
| [images.rs](../crates/desegni-ai/src/images.rs), [image_generation.rs](../crates/desegni-ai/src/image_generation.rs), [image_access.rs](../crates/desegni-ai/src/image_access.rs) | Download/embed real images, generate requested images and cache account image-access capability. |
| [chatgpt/mod.rs](../crates/desegni-ai/src/chatgpt/mod.rs) | `ChatgptManager`; browser sign-in, account selection, bearer refresh, model discovery and sign-out. |
| [chatgpt/types.rs](../crates/desegni-ai/src/chatgpt/types.rs), [storage.rs](../crates/desegni-ai/src/chatgpt/storage.rs), [http.rs](../crates/desegni-ai/src/chatgpt/http.rs), [callback.rs](../crates/desegni-ai/src/chatgpt/callback.rs) | Account/view/error types, metadata and secure token persistence, auth HTTP and loopback callback validation. |

## Desktop and frontend modules

### Native shell: `src-tauri/src/`

| Files | Owns; key entry points |
| --- | --- |
| [main.rs](../src-tauri/src/main.rs) | Service initialization, command registry, plugins, preview URI protocol and window lifecycle. |
| [workspace.rs](../src-tauri/src/workspace.rs) | `Workspace`, `WorkspaceView`, open/parked projects, journaled edits, recovery, save conflicts and page imports; `edit_project`, `edit_element`, `save_project_checked`, `recover_project`. |
| [generation.rs](../src-tauri/src/generation.rs) | `GenerationService`, `GenerationJobView`, `JobStatus`; prepare/submit/finish/cancel/retry/apply, draft actions, job persistence and AI settings. |
| [preview.rs](../src-tauri/src/preview.rs), [preview_bridge.js](../src-tauri/src/preview_bridge.js) | `PreviewRegistry`, `PreviewDescriptor`, `register`, `serve`, `release`; frozen preview resources, CSP, link/error messages. |
| [preview_health.rs](../src-tauri/src/preview_health.rs) | `PreviewHealth`, `editor_heartbeat`, `reload_workspace`; detect/recover an unresponsive editor. |
| [identity.rs](../src-tauri/src/identity.rs) | `IdentityService`, source selection/fetching, persisted analysis, approved context; `analyze_identity`, `reviewed_context_selected`, `project_from_analysis`. |
| [website_import.rs](../src-tauri/src/website_import.rs) | `WebsiteImports`, opaque folder selection IDs, `pick_website_folder`, `website_build`. |
| [export.rs](../src-tauri/src/export.rs) | `ExportService`, frozen-revision `export_website`, verified `open_export_folder`. |
| [chatgpt.rs](../src-tauri/src/chatgpt.rs) | `ChatgptState`, account epochs, authentication commands and cancellation on account changes. |
| [project_location.rs](../src-tauri/src/project_location.rs), [recent.rs](../src-tauri/src/recent.rs) | Default project parent and recent-project catalogue; get/set location, remember/forget projects. |
| [clipboard.rs](../src-tauri/src/clipboard.rs), [image_drop.rs](../src-tauri/src/image_drop.rs), [request_zip.rs](../src-tauri/src/request_zip.rs) | Native clipboard, window image drops and attachment ZIP command adapters. |
| [system_open.rs](../src-tauri/src/system_open.rs), [linux_startup.rs](../src-tauri/src/linux_startup.rs), [usage.rs](../src-tauri/src/usage.rs) | OS opening, Linux graphics initialization and bounded local interaction logs. |

Shell configuration: [Cargo.toml](../src-tauri/Cargo.toml), [tauri.conf.json](../src-tauri/tauri.conf.json), [capabilities](../src-tauri/capabilities/).

### Editor: `src/`

| Files | Owns; key entry points |
| --- | --- |
| [App.tsx](../src/App.tsx) | `Editor`, workspace queue, synchronization, project/menu actions, generation result review and top-level surfaces. |
| [api.ts](../src/api.ts), [types.ts](../src/types.ts) | Native command bridge and shared view/command types. |
| [Canvas.tsx](../src/Canvas.tsx) | `Canvas`, request boxes, select/pan/zoom, drag/resize, inline text, local edits and area problem callouts; `CanvasHandle::synchronize`. Framed requests are created against the frame the box is drawn over. Request boxes and Ask AI share the inline [RequestPages](../src/RequestPages.tsx) picker, whose state lives in [requestPages.ts](../src/requestPages.ts). |
| [Preview.tsx](../src/Preview.tsx) | `PreviewFrame`, `CanvasPreviewHandle`, `PagePreview`, `SourceView`; static/live frames, framed-document registry, DOM measurement and double-buffered reloads. |
| [frameDocuments.ts](../src/frameDocuments.ts) | Pure inner↔canvas mapping (scroll/scale), frame hit-testing, framed-request rect mapping and framed document session URLs. |
| [canvasGeometry.ts](../src/canvasGeometry.ts), [elementEditing.ts](../src/elementEditing.ts), [elementTargets.ts](../src/elementTargets.ts), [wholePageScope.ts](../src/wholePageScope.ts) | Coordinates/snapping/resize math, translated styles/images, target comparisons, `autoWholePage`/`isWholePage`. |
| [CanvasTools.tsx](../src/CanvasTools.tsx), [ElementTools.tsx](../src/ElementTools.tsx), [StyleBar.tsx](../src/StyleBar.tsx), [RegionOutline.tsx](../src/RegionOutline.tsx) | Canvas tools, element dialogs, quick styles and shape outlines. |
| [Layers.tsx](../src/Layers.tsx), [siteLayers.ts](../src/siteLayers.ts), [layerOrder.ts](../src/layerOrder.ts) | Layers UI, DOM-derived site tree, visual/copy styles and stacking order. |
| [CodePane.tsx](../src/CodePane.tsx), [CodeEditor.tsx](../src/CodeEditor.tsx), [codeDraftLifecycle.ts](../src/codeDraftLifecycle.ts) | Design/split/code view, CodeMirror, dirty-draft flush/validation/conflict handling; `synchronizeCodeDrafts`. |
| [editorHtml.ts](../src/editorHtml.ts), [codeMirrorMetadata.ts](../src/codeMirrorMetadata.ts) | Visible/raw source projection and hidden editor metadata preservation. |
| [useGeneration.ts](../src/useGeneration.ts), [Generation.tsx](../src/Generation.tsx), [GenerationLog.tsx](../src/GenerationLog.tsx), [generationDraftState.ts](../src/generationDraftState.ts) | Job lifecycle/polling/automatic apply, feedback/details, current/past requests and pending drafts. |
| [generationProblems.ts](../src/generationProblems.ts) | `areaProblems`, `placeCallouts`; associate rejected/conflicting results with canvas boxes. |
| [AiSettings.tsx](../src/AiSettings.tsx) | Provider/model/key/sign-in settings, access check and ChatGPT welcome/account controls. |
| [ImportContext.tsx](../src/ImportContext.tsx), [ImportWebsite.tsx](../src/ImportWebsite.tsx), [WebsiteChoice.tsx](../src/WebsiteChoice.tsx), [identityReview.ts](../src/identityReview.ts) | Import source/build choice, page merge review, brand review and finding/page grouping. |
| [NewProject.tsx](../src/NewProject.tsx), [ProjectLocation.tsx](../src/ProjectLocation.tsx), [Onboarding.tsx](../src/Onboarding.tsx) | Start screen, project creation/location and first-use tips. |
| [Notes.tsx](../src/Notes.tsx), [StyleGuide.tsx](../src/StyleGuide.tsx), [PageCheck.tsx](../src/PageCheck.tsx) | Project brief/decisions/exceptions, brand view and `checkProject` issue list. |
| [ProblemBanner.tsx](../src/ProblemBanner.tsx), [userErrors.ts](../src/userErrors.ts), [noticeSummary.ts](../src/noticeSummary.ts) | Notice queue/host, error translation and concise change summaries. |
| [RecoveryBanner.tsx](../src/RecoveryBanner.tsx), [SaveConflict.tsx](../src/SaveConflict.tsx), [OutsideEditsNotice.tsx](../src/OutsideEditsNotice.tsx) | Recovery choices, checked-save conflict resolution and outside-edit actions. |
| [WorkspaceSurface.tsx](../src/WorkspaceSurface.tsx), [Menus.tsx](../src/Menus.tsx), [shortcuts.ts](../src/shortcuts.ts) | Floating surface/focus management, menus and keyboard command mapping. |
| [useHistory.ts](../src/useHistory.ts), [DraftText.tsx](../src/DraftText.tsx), [deletePreference.ts](../src/deletePreference.ts), [usage.ts](../src/usage.ts) | UI undo-history labels, text drafts, delete preference and local usage instrumentation. |
| [styles.css](../src/styles.css), [brand.css](../src/brand.css), [shell.css](../src/shell.css), [canvas.css](../src/canvas.css), [dialogs.css](../src/dialogs.css), [notice.css](../src/notice.css), [newProject.css](../src/newProject.css) | Style entry point, brand tokens, shell/canvas/dialog/notice/start-screen presentation. |

### Scripts and documents

- [build-release.mjs](../scripts/build-release.mjs) and [build-release.sh](../scripts/build-release.sh): local release orchestration and Linux shell entry point.
- [toolchain.mjs](../scripts/toolchain.mjs), [cargo.mjs](../scripts/cargo.mjs), [tauri.mjs](../scripts/tauri.mjs), [launch-paths.mjs](../scripts/launch-paths.mjs): tool discovery, Cargo/Tauri wrappers and portable launch paths.
- [generation-benchmark.mjs](../scripts/generation-benchmark.mjs): generation scenario runner/reporting; [usage-report.mjs](../scripts/usage-report.mjs): local interaction/job summaries.
- [docs/](./): focused contracts and implementation notes, including [generation](GENERATION_CONTRACT.md), [request history](REQUEST_HISTORY.md), [import](IDENTITY_IMPORT.md) and [export](WEBSITE_EXPORT.md). Check their claims against source before changing behavior.

## Key flows

### AI generation and review

1. [App.tsx](../src/App.tsx) `generate`/`generationCall` synchronizes canvas and code drafts → [useGeneration.ts](../src/useGeneration.ts) `prepare` → native `prepare_generation`.
2. [generation.rs](../src-tauri/src/generation.rs) `GenerationService::prepare_requests` freezes project, provider/auth/settings/capabilities and history → [requirements.rs](../crates/desegni-core/src/requirements.rs) builds `RequirementsSnapshot` → persists `REQUIREMENTS.md` and job record.
3. Native `start_generation`/`retry_generation` → `submit`/`begin` → [providers.rs](../crates/desegni-ai/src/providers.rs) `ProviderClient` → provider transport ([transport.rs](../crates/desegni-ai/src/transport.rs) for OpenAI). Cancellation/progress stay bound to the job; an element fix can request one context follow-up.
4. `submit` → [generation_recovery.rs](../crates/desegni-core/src/generation_recovery.rs) `recover_generation_reply` → candidate/scope validation. [generation_draft.rs](../crates/desegni-core/src/generation_draft.rs) retains rejected source before any repair. At most one automatic validation repair uses frozen settings and cost/cancellation checks; `merge_validation_repair` preserves usable first-round work. Image download/generation adapters then embed assets; `finish` validates/persists the result.
5. [useGeneration.ts](../src/useGeneration.ts) polls and automatically applies a ready result → native `apply_generation` writes a before-apply checkpoint → [Session::apply_generation](../crates/desegni-core/src/session.rs) journals and installs ready regions. Conflicts remain pending; `generation_draft` supports checking, explicitly applying, dismissing or repairing retained drafts.
6. [App.tsx](../src/App.tsx) `generationApplied` opens review. `keepResult` dismisses it; `rejectResult` checks job/revision and calls `undo_project`. Try again undoes then prepares a new job. Accepted identity proposals are separate from keeping the canvas result.

### Flexible versus Precise scope

1. [App.tsx](../src/App.tsx) maps **Flexible** → `fuzzy`, **Precise** → `strict` → `set_generation_mode` in [model.rs](../crates/desegni-core/src/model.rs).
2. [requirements.rs](../crates/desegni-core/src/requirements.rs) uses current-format Flexible instructions: boxes anchor placement, related HTML/shared CSS/JS may change as needed, unrelated content stays unchanged. Both modes receive the pages chosen on the request's page picker (this page by default, then specific pages, a path group or all pages); Flexible also receives the pages and site files a framed request clearly concerns (same directory, or named in the prompt) within a 400 KiB budget. Precise may change only the framed document and the chosen pages; a selection that cannot fit one request is rejected with how many pages fit. [element_fix.rs](../crates/desegni-core/src/element_fix.rs) handles selected-element replies.
3. [generation.rs](../crates/desegni-core/src/generation.rs) validation → [generation_scope.rs](../crates/desegni-core/src/generation_scope.rs) and [region_scripts.rs](../crates/desegni-core/src/region_scripts.rs). Whole-page requests grant page-wide authority and compose by layer order; local requests alone do not. Scope detection lives in `Region::auto_whole_page` and mirrored [wholePageScope.ts](../src/wholePageScope.ts). Older frozen requirements retain their versioned rules.

### Region scripts runtime

1. Generated/local source stores inert `<script type="application/x-desegnos-region" data-desegnos-script="REQUEST_UUID">` blocks inside their owned root → [region_scripts.rs](../crates/desegni-core/src/region_scripts.rs) validates syntax/ownership and preservation of shared JavaScript for precise local changes.
2. `region_script_runtime` expands [region_script_runtime.js](../crates/desegni-core/src/region_script_runtime.js), passing each block's parent as `root`; source stays in HTML so deletion and undo follow its owner.
3. [preview.rs](../src-tauri/src/preview.rs) serves runtime resources when scripts are enabled; [export.rs](../crates/desegni-core/src/export.rs) emits region runtime after page JavaScript. Static editing never executes it.

### Preview frames and reload

1. [Preview.tsx](../src/Preview.tsx) `PreviewFrame` → native [create_preview](../src-tauri/src/generation.rs) → [PreviewRegistry::register](../src-tauri/src/preview.rs); page/job/draft source becomes a frozen session with a source hash.
2. Static canvas uses script-disabled `srcDoc` with same-origin DOM access for picking/measurement; CSS, fonts and image decoding settle before measurement. Editable iframes render their framed document as a nested same-origin `srcdoc`; a changed frame reloads behind a fixed copy so only that document re-renders. Live/device preview uses protocol resources, an opaque sandbox origin and optional `allow-scripts`; [preview_bridge.js](../src-tauri/src/preview_bridge.js) reports links/errors.
3. `PreviewFrame` keeps visible and hidden pending frames mounted together. After load and paint opportunities, it promotes the replacement in one React commit, then releases the old native session. Page/revision/session checks invalidate stale editing handles.

### Canvas editing

1. [Canvas.tsx](../src/Canvas.tsx) converts pointer coordinates through [canvasGeometry.ts](../src/canvasGeometry.ts) → [Preview.tsx](../src/Preview.tsx) `pick`/`textAt` → stable `ElementTarget`; selection controls and [Layers.tsx](../src/Layers.tsx) share that target.
2. Move/resize gestures call `beginMove`/`beginResize` for temporary DOM feedback; geometry/snapping and `translatedStyle` compute the final inline style. Double-click text → `InlineTextEditor` → `finishText`.
3. App `editElement` → native [edit_element](../src-tauri/src/workspace.rs) → [Session::edit_element](../crates/desegni-core/src/session.rs) → [editing.rs](../crates/desegni-core/src/editing.rs), with stable-target validation and journaled history. A multi-selection sends one native `edit_elements` batch ([Project::edit_elements_in](../crates/desegni-core/src/editing.rs)) so every target changes in one revision and one undo step; a stale target rejects the whole batch. The element toolbar's **Also on pages** toggle sends one `edit_element_across_pages` request instead; [across_pages.rs](../crates/desegni-core/src/across_pages.rs) matches tag, class list and ancestor position (identical outerHTML first), skips text blocks that differed, and keeps every reachable document in one revision and undo step. Canvas `synchronize` flushes text, queued edits and measured bounds before dependent actions.

### Project save, open, recovery and journal

1. [App.tsx](../src/App.tsx) synchronizes edits → native [workspace.rs](../src-tauri/src/workspace.rs) open/create/save commands. `Workspace` owns `Session`, `ProjectFile`, `Journal`, recovery pointer and parked projects.
2. [ProjectFile](../crates/desegni-storage/src/lib.rs) locks the archive, checks its stamp, saves atomically and retains `.previous`; `save_project_checked` returns conflicts to [SaveConflict.tsx](../src/SaveConflict.tsx). Loading verifies archive contents and calls [migration.rs](../crates/desegni-core/src/migration.rs).
3. Session mutations call `Journal::append` before publishing. Despite the method name, current writes atomically replace a full checksummed snapshot; recovery also accepts older multi-record journals.
4. Startup/open scans recovery pointers and compares saved state → [RecoveryBanner.tsx](../src/RecoveryBanner.tsx) → `recover_project` or `dismiss_recovery`. Dismissal moves the pointer aside and preserves the journal. Interrupted AI work is reconciled from saved jobs, without automatic resubmission.

### Site sync and outside edits

1. Archive save → [project_folder.rs](../crates/desegni-storage/src/project_folder.rs) `sync_project_site` → `compile_website`; `.desegnos-site.json` records owned hashes and pending publication for safe retries.
2. Changed/missing owned files and stale files are detected before replacement/removal; archive save remains durable even if site sync warns. [OutsideEditsNotice.tsx](../src/OutsideEditsNotice.tsx) presents the returned `OutsideEdits`.
3. [App.tsx](../src/App.tsx) `importOutsideChanges` → `project_site_import_source` → page import review; explicit overwrite → `overwrite_project_site`/`ProjectFile::overwrite_site`, backing up outside edits first. Standalone legacy archives do not claim their parent folder automatically.

### Import…: pages and brand

1. [App.tsx](../src/App.tsx) menu → [ImportContext.tsx](../src/ImportContext.tsx). [WebsiteChoice.tsx](../src/WebsiteChoice.tsx) and native [website_import.rs](../src-tauri/src/website_import.rs) select/detect a folder or prepare build output; native [identity.rs](../src-tauri/src/identity.rs) also accepts project/ZIP/web sources.
2. Pages: [ImportWebsite.tsx](../src/ImportWebsite.tsx) → `prepare_website_import` in [workspace.rs](../src-tauri/src/workspace.rs) → [review_website_import](../crates/desegni-core/src/website_merge.rs) → user choices → `apply_website_import` → journaled `Session::import_website`. Editing an entire existing folder instead uses `edit_website_folder` and preserves originals.
3. Brand: source/selected files → `prepare_identity_selection` → `analyze_identity` → provider analysis → findings/assets/pages reviewed in `ImportContext` and [identityReview.ts](../src/identityReview.ts) → native `reviewed_context_selected` → `import_context` or new project creation. Only approved context enters the project; existing generated pages can be offered regeneration.

### Export

1. [App.tsx](../src/App.tsx) synchronizes drafts → native [export_website](../src-tauri/src/export.rs) freezes project ID/revision → [compile_website](../crates/desegni-core/src/export.rs) builds portable files without executing authored code.
2. Destination picker → revision recheck → [write_website_export](../crates/desegni-storage/src/website_export.rs). Returned counts/warnings feed a result notice; `open_export_folder` verifies the native receipt before opening it.

### Notices and errors

1. Raw errors stay in state/job records; [userErrors.ts](../src/userErrors.ts) `userError`, `generationError`, `runtimeError` translate them at presentation boundaries.
2. [ProblemBanner.tsx](../src/ProblemBanner.tsx) routes `Notice`/`ProblemBanner` into `NoticeProvider` → `NoticeHost`: errors, warnings, result notices, then information; insertion order breaks ties. Hidden notices remain queued; matching visible duplicates dismiss together.
3. [generationProblems.ts](../src/generationProblems.ts) `areaProblems`/`placeCallouts` → [Canvas.tsx](../src/Canvas.tsx) box-anchored callouts with Apply anyway/Try again/Edit request. Preview runtime reports feed page errors and Fix with AI; [GenerationLog.tsx](../src/GenerationLog.tsx) retains details.

### AI settings and authentication

1. [AiSettings.tsx](../src/AiSettings.tsx) → [generation.rs](../src-tauri/src/generation.rs) settings/provider/key/access commands → [ProviderSettings](../crates/desegni-ai/src/settings.rs), [ProviderId](../crates/desegni-ai/src/providers.rs), [CredentialManager](../crates/desegni-ai/src/credentials.rs). API keys use secure device storage or an explicitly chosen session-only lifetime.
2. ChatGPT actions → native [chatgpt.rs](../src-tauri/src/chatgpt.rs) → [ChatgptManager](../crates/desegni-ai/src/chatgpt/mod.rs) → browser authorization/loopback callback, account metadata and vault tokens. Account epochs prevent stale access from submitting after sign-out/account changes; jobs freeze their authentication/account.

### Request history

1. [GenerationService](../src-tauri/src/generation.rs) persists jobs and compacts per-project request records; `request_history` compares current source fingerprints and application/draft state.
2. [request_history.rs](../crates/desegni-core/src/request_history.rs) filters relevant `EarlierRequest` entries → `with_earlier_requests` enriches frozen requirements. Undone or unapplied source is context, not accepted project authority; [dossier.rs](../crates/desegni-core/src/dossier.rs) maintains related brief context.
3. [GenerationLog.tsx](../src/GenerationLog.tsx) calls `list_generations`/`get_past_generation` for current and past AI requests. [useHistory.ts](../src/useHistory.ts) is the separate UI undo-history view.

## To change X, edit Y

| Change | Start here |
| --- | --- |
| Project fields, commands or schema | [model.rs](../crates/desegni-core/src/model.rs), [migration.rs](../crates/desegni-core/src/migration.rs), [types.ts](../src/types.ts). |
| Generation instructions/result schema | [requirements.rs](../crates/desegni-core/src/requirements.rs), [element_fix.rs](../crates/desegni-core/src/element_fix.rs), [generation.rs](../crates/desegni-core/src/generation.rs). |
| Flexible/Precise or whole-page scope | [generation_scope.rs](../crates/desegni-core/src/generation_scope.rs), [requirements.rs](../crates/desegni-core/src/requirements.rs), [wholePageScope.ts](../src/wholePageScope.ts), `Region` in [model.rs](../crates/desegni-core/src/model.rs). |
| Recovery, automatic repair or retained drafts | [generation_recovery.rs](../crates/desegni-core/src/generation_recovery.rs), [generation_draft.rs](../crates/desegni-core/src/generation_draft.rs), native [generation.rs](../src-tauri/src/generation.rs). |
| Generate/stop/retry/Keep/Undo UX | [useGeneration.ts](../src/useGeneration.ts), [App.tsx](../src/App.tsx), [Generation.tsx](../src/Generation.tsx). |
| Provider, model, budget or credentials | [providers.rs](../crates/desegni-ai/src/providers.rs), [settings.rs](../crates/desegni-ai/src/settings.rs), [credentials.rs](../crates/desegni-ai/src/credentials.rs), [AiSettings.tsx](../src/AiSettings.tsx). |
| ChatGPT account/sign-in behavior | [chatgpt/](../crates/desegni-ai/src/chatgpt/), native [chatgpt.rs](../src-tauri/src/chatgpt.rs). |
| Preview isolation/reload/runtime | [Preview.tsx](../src/Preview.tsx), native [preview.rs](../src-tauri/src/preview.rs), [preview_bridge.js](../src-tauri/src/preview_bridge.js), [region_scripts.rs](../crates/desegni-core/src/region_scripts.rs). |
| Selection/move/resize/text/layers | [Canvas.tsx](../src/Canvas.tsx), [Preview.tsx](../src/Preview.tsx), [frameDocuments.ts](../src/frameDocuments.ts), [editing.rs](../crates/desegni-core/src/editing.rs), [stable_targets.rs](../crates/desegni-core/src/stable_targets.rs), [layers.rs](../crates/desegni-core/src/layers.rs). |
| Change an element on other pages | [across_pages.rs](../crates/desegni-core/src/across_pages.rs), [acrossPages.ts](../src/acrossPages.ts), [Canvas.tsx](../src/Canvas.tsx), native `element_matches`/`edit_element_across_pages` in [workspace.rs](../src-tauri/src/workspace.rs). |
| Framed document requests (iframe targets) | [document.rs](../crates/desegni-core/src/document.rs), `Region.frame_target` in [model.rs](../crates/desegni-core/src/model.rs), [requirements.rs](../crates/desegni-core/src/requirements.rs), [generation.rs](../crates/desegni-core/src/generation.rs), [frameDocuments.ts](../src/frameDocuments.ts), native `get_frame_targets`/`get_frame_documents`/`ensure_element_id` in [workspace.rs](../src-tauri/src/workspace.rs). |
| Source editor and unsaved code handling | [CodePane.tsx](../src/CodePane.tsx), [codeDraftLifecycle.ts](../src/codeDraftLifecycle.ts), [editorHtml.ts](../src/editorHtml.ts), [source.rs](../crates/desegni-core/src/source.rs). |
| Save/open/recovery/conflicts | [workspace.rs](../src-tauri/src/workspace.rs), storage [lib.rs](../crates/desegni-storage/src/lib.rs), [SaveConflict.tsx](../src/SaveConflict.tsx), [RecoveryBanner.tsx](../src/RecoveryBanner.tsx). |
| Site synchronization/outside edits | [project_folder.rs](../crates/desegni-storage/src/project_folder.rs), [OutsideEditsNotice.tsx](../src/OutsideEditsNotice.tsx), [workspace.rs](../src-tauri/src/workspace.rs). |
| Page/brand import | [ImportContext.tsx](../src/ImportContext.tsx), [ImportWebsite.tsx](../src/ImportWebsite.tsx), native [identity.rs](../src-tauri/src/identity.rs), core [website_merge.rs](../crates/desegni-core/src/website_merge.rs). |
| Exported HTML/assets/routes | Core [export.rs](../crates/desegni-core/src/export.rs), [website_import.rs](../crates/desegni-core/src/website_import.rs), storage [website_export.rs](../crates/desegni-storage/src/website_export.rs). |
| Notice wording/priority/canvas errors | [userErrors.ts](../src/userErrors.ts), [ProblemBanner.tsx](../src/ProblemBanner.tsx), [generationProblems.ts](../src/generationProblems.ts), [notice.css](../src/notice.css). |
| Request context and AI log | Core [request_history.rs](../crates/desegni-core/src/request_history.rs), native [generation.rs](../src-tauri/src/generation.rs), [GenerationLog.tsx](../src/GenerationLog.tsx). |
| Default project location/recent files | [project_location.rs](../src-tauri/src/project_location.rs), [recent.rs](../src-tauri/src/recent.rs), [ProjectLocation.tsx](../src/ProjectLocation.tsx). |

## Runtime data locations

`DATA` below is Tauri `app_local_data_dir()`, resolved by native initialization. The identifier remains `io.github.chelij.desegni` in [tauri.conf.json](../src-tauri/tauri.conf.json); on Linux this is normally `$XDG_DATA_HOME/io.github.chelij.desegni` (fallback `~/.local/share/io.github.chelij.desegni`). Use the platform resolver rather than hardcoding this path.

| Location | Contents / owner |
| --- | --- |
| `DATA/project-settings.json`, `recent-projects.json` | Preferred parent folder and recent catalogue; [project_location.rs](../src-tauri/src/project_location.rs), [recent.rs](../src-tauri/src/recent.rs). Default projects live in `Documents/Desegnos`; fallback `DATA/Projects`. |
| `DATA/<project UUID>-<journal UUID>.journal` | Checksummed full project recovery snapshot; [Journal](../crates/desegni-storage/src/lib.rs). |
| `DATA/<project UUID>.recovery.json`, `.recovery.discarded.json` | Journal pointer, saved-file identity/revision and dismissed recovery marker; [workspace.rs](../src-tauri/src/workspace.rs). |
| `DATA/generation/` | Provider/settings/authentication preference JSON, image access cache and validation counters; [GenerationService](../src-tauri/src/generation.rs). Secrets are stored separately. |
| `DATA/generation/<project UUID>/latest.json`, `request-history.json` | Latest-job pointer and compact request context history. |
| `DATA/generation/<project UUID>/<job UUID>/` | `job.json`, immutable `REQUIREMENTS.md`, optional `CONTEXT.md`, scrubbed `REPLY.txt`, attempt archives and `before-apply-<revision>.desegni` safety checkpoints. Job records include status, candidates, drafts, usage and validation events. |
| `DATA/generation/chatgpt/accounts.json` | Account registration metadata/locking; [chatgpt/storage.rs](../crates/desegni-ai/src/chatgpt/storage.rs). Tokens use secure storage or memory, never project archives. |
| `DATA/identity/<analysis UUID>.json`, `identity/latest.json` | Persisted brand-analysis records/pointer; [IdentityService](../src-tauri/src/identity.rs). |
| `DATA/usage/<YYYY-MM-DD>.jsonl` | Local bounded interaction/command logs; [usage.rs](../src-tauri/src/usage.rs). |
| `<project folder>/<name>.desegnos` | Editable ZIP archive: `manifest.json`, `project.json`, `PROJECT.md`; legacy `.desegni` remains supported. [ProjectFile](../crates/desegni-storage/src/lib.rs). |
| `<archive>.previous`, `<archive>.lock` | Previous saved archive and file-lock sidecar. |
| `<project folder>/.desegnos-site.json` | Project/archive identity, owned website hashes, pending writes and original-file hashes; [project_folder.rs](../crates/desegni-storage/src/project_folder.rs). |
| `<project folder>/index.html`, page files, `assets/` | Compiled site synchronized after archive save; only managed paths are owned. |
| `<project folder>/.desegnos-original/`, `.desegnos-outside-edits/` | Preserved imported originals and backups made by explicit outside-edit overwrite. |
| Export destination | A separately published portable website; no project brief, prompts or request history. [website_export.rs](../crates/desegni-storage/src/website_export.rs). |

Preview sessions live in the in-memory `PreviewRegistry`, not a project directory. Renderer `localStorage` holds UI preferences such as theme, request shape, code view/width and deletion confirmation; it is separate from project and credential storage.
