# Desegnos project map

A source map for people and AI agents, reviewed against the files present on 2026-11-08. Paths are relative to this document. Product direction is [PRODUCT.md](../PRODUCT.md), the canvas concept [CANVAS.md](../CANVAS.md), the editor look and behaviour [DESIGN.md](../DESIGN.md) and the vocabulary [GLOSSARY.md](../GLOSSARY.md). Check claims here against the source before changing behaviour.

## Architecture

The Rust workspace has three libraries and the Tauri desktop application. Crates use the `desegnos` name; the application identifier is `io.github.chelij.desegnos`.

- **`desegnos-core`** is pure project logic: the model, elements, boundaries, pointers, reference, history semantics, generation contracts and validation, direct renderers, export plans and website-to-element conversion. It performs no I/O.
- **`desegnos-storage`** owns disk: the `.desegnos` SQLite project, content-addressed versions and branches, project folders, safe staged export writing, website snapshots and brand-source readers.
- **`desegnos-ai`** owns provider communication: request bodies and schemas, credentials and secure storage, streaming transports, image APIs, ChatGPT sign-in and bounded public website fetching.
- **Tauri** (`src-tauri/src/`) owns native authority: windows and commands, dialogs, open-project tabs, loopback static servers, generation/export lifecycles, recovery and app data.
- **React/TypeScript** (`src/`) owns the canvas editor. [main.tsx](../src/main.tsx) mounts [App.tsx](../src/App.tsx); [api.ts](../src/api.ts) wraps `invoke`; [types.ts](../src/types.ts) re-exports the types generated from Rust.

Typical boundary: component → `native(command, args)` or `edit(revision, command)` → `#[tauri::command]` → library operation → serialized `WorkspaceView`. `WorkspaceView` returns the authoritative `Project`, its open-project tabs, undo/history state, recovery offers and derived pointer states. Every edit carries the revision it was made against; stale edits are rejected rather than merged. Mutations autosave to the hidden recovery branch before the view is published (throttled to every 3 seconds). All AI, file, dialog and clipboard capability stays in native code; the renderer never receives credentials or arbitrary filesystem access.

There is one editor: the project canvas. Code is not stored on the canvas and exists only in exports. Imported websites become canvas elements; brand import remains evidence for the project brief.

## Source tree

```text
Cargo.toml, package.json, index.html, vite.config.ts, rust-toolchain.toml
crates/
├── desegnos-core/src/       Project model, elements, generation, renderers, export, website import.
│   └── render/pdf.rs        Minimal PDF writer for slide decks.
├── desegnos-storage/src/    SQLite project, versions, safe export writing, website and brand sources.
└── desegnos-ai/src/         Providers, transports, credentials, images, ChatGPT, website fetching.
    └── chatgpt/             Sign-in, accounts, token storage, HTTP and loopback callback.
src-tauri/src/               Desktop commands, workspace, generation, export, local site, import.
src/                         React editor: canvas, dock, menus, dialogs, generation, import.
├── projectCanvas/           Canvas layers, gestures, clipboard/drop, minimap, named views and Play.
├── session/                 Serialized edits, project commands and editor orchestration.
├── exportJobs/              Export, preview and pointer-clash lifecycle.
└── assets/brand/            Bundled wordmarks and Archivo web font.
tests/                       Kept Node tests; native/ holds Xvfb smoke and fake provider/OAuth servers.
scripts/                     Toolchain/release wrappers, startup and vault checks, usage reporting.
docs/                        Contracts, guides and this map.
test-results/                Local evidence from smoke, startup and vault runs (gitignored).
```

## `desegnos-core`

[lib.rs](../crates/desegnos-core/src/lib.rs) declares modules and re-exports the externally consumed API; internal callers import the owning module directly.

| Module | Owns |
| --- | --- |
| [model.rs](../crates/desegnos-core/src/model.rs) | `Project`, `Page` (the spec's boundary) and `PageSize`, reference items, pointers and wiring, pending requests with their targets and areas, brief, decisions, exceptions, export records, `CanvasTemplate` and project validation. |
| [elements.rs](../crates/desegnos-core/src/elements.rs) | `Element` tree, element kinds, styles, free/stack/grid layout, per-size overrides, linked definitions, `resolve_element`, shared style resolution and nested ids. |
| [limits.rs](../crates/desegnos-core/src/limits.rs) | Shared limits and URL, colour, SHA-256 and embedded-image validation. |
| [commands.rs](../crates/desegnos-core/src/commands.rs) | `Command`, labels, burst history policy and the project edit interpreter. |
| [element_edit.rs](../crates/desegnos-core/src/element_edit.rs) | Element patches/moves, project tree editing, linked-size resolution, outlines and layer naming. |
| [project_new.rs](../crates/desegnos-core/src/project_new.rs) | Project construction and template initialization. |
| [project_styles.rs](../crates/desegnos-core/src/project_styles.rs) | Shared style removal and detachment. |
| [pointer_edit.rs](../crates/desegnos-core/src/pointer_edit.rs) | Pointer mutation and validation when editing. |
| [reference_edit.rs](../crates/desegnos-core/src/reference_edit.rs) | Reference mutation, transfers between canvas and reference, and attachment promotion. |
| [request_edit.rs](../crates/desegnos-core/src/request_edit.rs) | Pending request mutation and one-request-per-layer normalization. |
| [session.rs](../crates/desegnos-core/src/session.rs) | `Session`: revision checks, undo/redo, burst-step merging, version labels, export metadata outside undo, generation recording/application, context import, restore and history position. |
| [pointers.rs](../crates/desegnos-core/src/pointers.rs) | Pointer states (unwired, needs generating/exporting, code changed, wired, contradicts, broken), contradiction detection and target descriptions. |
| [requirements.rs](../crates/desegnos-core/src/requirements.rs) | Frozen `RequirementsSnapshot`, capabilities, request markdown, targeted requirements and `validate_current`. |
| [generation.rs](../crates/desegnos-core/src/generation.rs) | `GenerationCandidate`, `ElementOperation` (including area-bound `InsertInBoundary`/`AddReference`), outcomes, batches, generated images, pointer classifications and typed validation. |
| [adapt.rs](../crates/desegnos-core/src/adapt.rs) | Boundary size adaptation. |
| [json_parse.rs](../crates/desegnos-core/src/json_parse.rs) | Provider JSON extraction, repair and reply parsing. |
| [schema_conformance.rs](../crates/desegnos-core/src/schema_conformance.rs) | Schema-guided reply conformance, independent of validation and application. |
| [generation_scope.rs](../crates/desegnos-core/src/generation_scope.rs) | Request permissions and area geometry. |
| [generation_apply.rs](../crates/desegnos-core/src/generation_apply.rs) | Operation interpretation, boundary coverage and generation application. |
| [generation_schema.rs](../crates/desegnos-core/src/generation_schema.rs) | Candidate and result JSON schemas. |
| [svg_validation.rs](../crates/desegnos-core/src/svg_validation.rs) | Portable embedded SVG validation. |
| [validation.rs](../crates/desegnos-core/src/validation.rs) | Text validation, error construction, hashing and nullable deserialization. |
| [request_history.rs](../crates/desegnos-core/src/request_history.rs) | Text-only earlier-request context derived from frozen requests, plus retry rejection history. |
| [dossier.rs](../crates/desegnos-core/src/dossier.rs) | `GenerationDossier`, application status, acceptance data and recording on the project. |
| [attachments.rs](../crates/desegnos-core/src/attachments.rs) | Bounded text, picture and link attachments on requests, previews and validation. |
| [identity.rs](../crates/desegnos-core/src/identity.rs) | `ImportedIdentity`, `ImportedContext`, evidence pages, project photos/items and merging approved brand context into the brief. |
| [identity_pages.rs](../crates/desegnos-core/src/identity_pages.rs) | Source-page kinds and portable route names for imported evidence. |
| [project_assets.rs](../crates/desegnos-core/src/project_assets.rs) | `desegnos-asset:` references, the content-addressed picture store, `prepare_picture`, image catalogues and unused-picture cleanup. |
| [picture_scaling.rs](../crates/desegnos-core/src/picture_scaling.rs) | AI preview size/quality and display-scaled picture copies. |
| [image_generation.rs](../crates/desegnos-core/src/image_generation.rs) | The downloaded-image byte ceiling. |
| [website.rs](../crates/desegnos-core/src/website.rs) | `website_links`, `website_assets` and `css_urls`: stylesheets, logos, pages, photos, media frames and CSS references. |
| [website_import.rs](../crates/desegnos-core/src/website_import.rs) | `WebCapture` types and the pure conversion into pages, elements, reference notes, behaviour pointers and linked-size overrides. |
| [export.rs](../crates/desegnos-core/src/export.rs) | Export plans, exact AI request markdown, schemas, reply parsing/validation, AI and direct bundles, manifests, pointer fingerprints and `RecordExport`. |
| [web_codegen.rs](../crates/desegnos-core/src/web_codegen.rs) | Deterministic HTML/CSS from resolved element trees and linked sizes, packaged assets and behaviour hooks and DOM fingerprints. |
| [element_style.rs](../crates/desegnos-core/src/element_style.rs) | Shared element padding, clipping, text/control metrics, input hooks and font fallback policy for Rust emitters. |
| [text_layout.rs](../crates/desegnos-core/src/text_layout.rs) | Font resolution, glyph measurement, wrapping, intrinsic sizing and persisted fit geometry. |
| [geometry.rs](../crates/desegnos-core/src/geometry.rs) | Shared paths, points and colour parsing for layout and render emitters. |
| [render.rs](../crates/desegnos-core/src/render.rs) | Direct SVG/PNG/PDF scene painting from resolved trees and shared text layout, with raster limits. |
| [render/pdf.rs](../crates/desegnos-core/src/render/pdf.rs) | Minimal vector/raster PDF deck writer. No embedded font program. |

## `desegnos-storage`

| Module | Owns |
| --- | --- |
| [lib.rs](../crates/desegnos-storage/src/lib.rs) | Module wiring, atomic small-file writes, `.previous` backups and `write_text`. |
| [project_db.rs](../crates/desegnos-storage/src/project_db.rs) | `ProjectDatabase`: the `.desegnos` SQLite file, blobs/versions/branches/head tables, rollback-journal transactions, checked saves, the hidden `__autosave` branch, alternatives, restore, compaction and save-conflict stamps. |
| [project_parts.rs](../crates/desegnos-storage/src/project_parts.rs) | `split_project`/`join_project`: one part per boundary, plus `reference`, `pointers`, `brief`, `identity`, `dossier`, `settings` and one part per picture. |
| [project_folder.rs](../crates/desegnos-storage/src/project_folder.rs) | Project-folder naming, one-project-file lookup, collision-safe creation and name sanitising. |
| [website_export.rs](../crates/desegnos-storage/src/website_export.rs) | Bounded, staged export writing, no-replace destination rules, output verification, receipts and `read_export_file`. |
| [source_files.rs](../crates/desegnos-storage/src/source_files.rs) | Shared source paths, archive safety, file classification and bounded reads. |
| [website_snapshot.rs](../crates/desegnos-storage/src/website_snapshot.rs) | Shared URL/folder/ZIP snapshot and page types, route/file rules, titles, charset/percent decoding, bounded local collection and private import folders. |
| [identity_source.rs](../crates/desegnos-storage/src/identity_source.rs) | Read-only brand evidence from files, folders, archives and websites, including photos and assets. |
| [request_zip.rs](../crates/desegnos-storage/src/request_zip.rs) | Bounded in-memory ZIP extraction into request attachments. |
| [system_tools.rs](../crates/desegnos-storage/src/system_tools.rs) | Executable lookup for OS launchers using inherited and common tool directories. |

## `desegnos-ai`

[lib.rs](../crates/desegnos-ai/src/lib.rs) declares modules and re-exports the externally consumed API; internal callers import the owning module directly.

| Module | Owns |
| --- | --- |
| [providers.rs](../crates/desegnos-ai/src/providers.rs) | `ProviderId`, `ProviderDescriptor`, provider catalogue, `ProviderAdapter`, `ProviderClient`, estimates, model access checks and generation/identity dispatch. |
| [transport.rs](../crates/desegnos-ai/src/transport.rs) | `OpenAiClient`, HTTP/SSE streaming, deadlines/idle limits, response identity and correlation, and ChatGPT bearer use. |
| [estimate.rs](../crates/desegnos-ai/src/estimate.rs) | Shared cost, pricing overrides, image fees, spending and context-window arithmetic with explicit wire-format reserves. |
| [model_discovery.rs](../crates/desegnos-ai/src/model_discovery.rs) | Provider discovery profiles, model parsing, authorization, shared connectivity probes and ChatGPT catalogue discovery. |
| [retry.rs](../crates/desegnos-ai/src/retry.rs) | Pre-output retry budgets, backoff and bounded Retry-After handling, including token revocation. |
| [schema_wire.rs](../crates/desegnos-ai/src/schema_wire.rs) | Wire-only schema property ordering, independent of persisted canonical hashes. |
| [settings.rs](../crates/desegnos-ai/src/settings.rs) | `ProviderSettings`, model catalogue, request bodies, token/size limits, cost estimates and billed output caps. |
| [request.rs](../crates/desegnos-ai/src/request.rs) | Shared frozen-packet/schema validation and task content: instructions, documents, images and reply schema before provider-specific encoding. |
| [credentials.rs](../crates/desegnos-ai/src/credentials.rs) | `ApiKey`, `CredentialManager`, system/session storage, masked identifiers and credential status. |
| [error.rs](../crates/desegnos-ai/src/error.rs) | `ProviderError`, error kinds, submission outcomes, diagnostics scrubbing and authentication context. |
| [sse.rs](../crates/desegnos-ai/src/sse.rs) | Bounded SSE decoder, event framing, terminal output extraction and stream validation. |
| [identity.rs](../crates/desegnos-ai/src/identity.rs) | Brand analysis bodies/budgets, logo and photo images, identity schema checks and estimates. |
| [website.rs](../crates/desegnos-ai/src/website.rs) | `WebsiteFetcher` snapshot and image methods under public-address, size, redirect and timeout rules. |
| [public_fetch.rs](../crates/desegnos-ai/src/public_fetch.rs) | Public web fetch requests, DNS pinning, fixture boundary and redirect policy. |
| [net.rs](../crates/desegnos-ai/src/net.rs) | Hardened HTTP clients, shared deadlines and cancellable bounded response reads. |
| [image_generation.rs](../crates/desegnos-ai/src/image_generation.rs) | Bounded Image API generation, its model and fixed output dimensions, reply decoding and generated bytes. |
| [image_access.rs](../crates/desegnos-ai/src/image_access.rs) | Account-specific image capability discovery and caching. |
| [chatgpt/mod.rs](../crates/desegnos-ai/src/chatgpt/mod.rs) | `ChatgptManager`: browser sign-in lifecycle, accounts, sign-in state and sign-out; refresh and catalogue operations have separate modules. |
| [chatgpt/callback.rs](../crates/desegnos-ai/src/chatgpt/callback.rs) | Loopback OAuth callback listener with state checks and cancellation. |
| [chatgpt/http.rs](../crates/desegnos-ai/src/chatgpt/http.rs) | OAuth discovery, authorize/token/JWKS calls, JWT validation, revocation and model access. |
| [chatgpt/storage.rs](../crates/desegnos-ai/src/chatgpt/storage.rs) | Registration/token records, system vault abstraction, test vault and bounded private files. |
| [chatgpt/refresh.rs](../crates/desegnos-ai/src/chatgpt/refresh.rs) | Refresh-token rotation and recovery after secure-storage failures. |
| [chatgpt/models.rs](../crates/desegnos-ai/src/chatgpt/models.rs) | Account-bound catalogue refresh and stale-result checks. |
| [chatgpt/lock.rs](../crates/desegnos-ai/src/chatgpt/lock.rs) | Shared lock polling and deadlines, with cancellable asynchronous leases. |
| [chatgpt/types.rs](../crates/desegnos-ai/src/chatgpt/types.rs) | ChatGPT error/view/account types. |
| [tests.rs](../crates/desegnos-ai/src/tests.rs), [identity_tests.rs](../crates/desegnos-ai/src/identity_tests.rs) | Rust provider/transport/identity tests, including loopback fake servers. |
| [chatgpt/tests.rs](../crates/desegnos-ai/src/chatgpt/tests.rs) | ChatGPT OAuth/token/storage tests against loopback servers. |
| [chatgpt/README.md](../crates/desegnos-ai/src/chatgpt/README.md), [chatgpt/dependency-review.json](../crates/desegnos-ai/src/chatgpt/dependency-review.json), [chatgpt/fixtures/test-jwk.json](../crates/desegnos-ai/src/chatgpt/fixtures/test-jwk.json), [chatgpt/fixtures/test-key.der](../crates/desegnos-ai/src/chatgpt/fixtures/test-key.der) | Documents that boundary and supplies its test JWK/key. |

## Native shell: `src-tauri/src/`

| File | Owns |
| --- | --- |
| [main.rs](../src-tauri/src/main.rs) | Service initialization on the chosen data folder, plugins, window lifecycle and crash finish, with `shutdown.rs` installing the quit and OS shutdown handlers; the command registry comes from `commands.txt`. |
| [commands.txt](../src-tauri/commands.txt), [build.rs](../src-tauri/build.rs) | The one list of native commands. `build.rs` turns it into the `registered_commands!` macro, the permission manifest and the editor capability (written to `OUT_DIR`, not checked in); `COMMAND_NAMES` feeds the contract test. Every line is module-qualified. Tauri writes `permissions/autogenerated/` during the build; it is git-ignored. |
| [native_error.rs](../src-tauri/src/native_error.rs) | Typed failure envelope at every native command boundary; retains recovery codes, provider details and browser URLs. |
| [contract.rs](../src-tauri/src/contract.rs) | Test-only generator for `src/generated/` (`types.ts` from the `ts-rs` derives, `limits.ts` from core limits, element defaults, size labels and template presets, `commands.ts` with `CommandName` and the event name). The test fails when the committed files are stale; regenerate with `npm run generate:contract`. |
| [workspace.rs](../src-tauri/src/workspace.rs) | `Workspace`, `WorkspaceView`, active plus parked projects, tabs, untitled projects and their recovery databases, autosave, recovery, checked saves/conflicts, project folders, recents, history/alternatives and the generation/import publication seams. |
| [ai_job.rs](../src-tauri/src/ai_job.rs) | What generation, identity and export share about a paid AI job: `ActiveJobs` (live-attempt guard), shared `SwitchGuard` enforced by workspace tab transitions, `Failure::of`, stop/sign-out/interrupted notices, and the construction-time `SwitchGuard` every service receives (export registers its sends through `ActiveJobs` too). |
| [ai_access.rs](../src-tauri/src/ai_access.rs) | The one AI access step: `Connection`, `bound_account`, `ChatgptAccount::usable` checks, `resolve` (account or key, model, epoch) into an `Access` with the shared input check. |
| [native_files.rs](../src-tauri/src/native_files.rs) | `NativeResult`, poison-tolerant `LockExt::locked`, private directories, regular-file checks, size-limited JSON reads/writes (`SMALL_RECORD_BYTES`) and lock-file opening and the bounded update lock shared by the services. |
| [ai_settings.rs](../src-tauri/src/ai_settings.rs) | `AiSettingsStore`: provider settings, credentials, model catalogue, image-access probes and AI settings commands, shared directly with ChatGPT, identity and export. |
| [generation_job_store.rs](../src-tauri/src/generation_job_store.rs) | Generation record persistence, bounded in-memory cache, current-format history indexes, disk validation and retained unsaved records. |
| [generation.rs](../src-tauri/src/generation.rs) | `GenerationService`: prepare/submit/retry/cancel/discard/apply, reply validation, dossier maintenance and usage. Borrows selected settings from the independently managed settings store. |
| [export.rs](../src-tauri/src/export.rs) | `ExportService`: frozen export jobs, previews, destination writes, safe commits, `RecordExport`, recheck, pointer code/comment clash commands and open-in-browser. |
| [website_import.rs](../src-tauri/src/website_import.rs) | Fetch/read a website, install a token-served snapshot, validate captures, resolve pictures into the project store and apply an import as one undo step. |
| [identity.rs](../src-tauri/src/identity.rs) | `IdentityService`: brand source selection/fetching, analysis lifecycle, photo descriptions and approved-context handoff. |
| [local_site.rs](../src-tauri/src/local_site.rs) | `LocalSite`: loopback GET/HEAD static servers with token prefixes, path rules and stop control. |
| [chatgpt.rs](../src-tauri/src/chatgpt.rs) | `ChatgptState`: accounts, sign-in commands, epochs/revocation, secure-storage repair and usage links. |
| [project_history.rs](../src-tauri/src/project_history.rs) | Renderer history and save-conflict contract types, converted explicitly from native storage values. |
| [project_location.rs](../src-tauri/src/project_location.rs), [recent.rs](../src-tauri/src/recent.rs) | App settings in `project-settings.json` (default project location, show start page on launch) and the recent-project catalogue with pins and last-opened times. |
| [clipboard.rs](../src-tauri/src/clipboard.rs), [image_drop.rs](../src-tauri/src/image_drop.rs) | Editor-only clipboard adapter and OS image-drop events. |
| [fonts.rs](../src-tauri/src/fonts.rs) | `resolve_canvas_font`, `list_canvas_fonts` and `load_canvas_font`: the canvas font handshake; a face must be resolved before its id can be loaded. |
| [shutdown.rs](../src-tauri/src/shutdown.rs) | Handlers for OS close, logoff and shutdown that finish the crash record and exit within the few seconds the OS allows. |
| [testing.rs](../src-tauri/src/testing.rs) | The one place native-test fixtures meet production commands (OpenAI endpoint, website fixture origin, OAuth opener, benchmark capture); real behaviour without `native-tests`. |
| [system_open.rs](../src-tauri/src/system_open.rs), [file_dialogs.rs](../src-tauri/src/file_dialogs.rs), [portal.rs](../src-tauri/src/portal.rs), [linux_startup.rs](../src-tauri/src/linux_startup.rs) | OS opening with Linux fallbacks (and the `open_preview_link` command), dialog result handling, XDG desktop-portal detection and the NVIDIA/Wayland startup workaround. |
| [data_location.rs](../src-tauri/src/data_location.rs), [crash_report.rs](../src-tauri/src/crash_report.rs), [updates.rs](../src-tauri/src/updates.rs), [usage.rs](../src-tauri/src/usage.rs), [release.rs](../src-tauri/src/release.rs) | Writable data-folder choice, opt-in crash reports, manual GitHub release check, local usage JSONL and the shared repository constant. |

## Editor: `src/`

| File | Owns |
| --- | --- |
| [App.tsx](../src/App.tsx), [AppBar.tsx](../src/AppBar.tsx), [CanvasWorkspace.tsx](../src/CanvasWorkspace.tsx), [StatusBar.tsx](../src/StatusBar.tsx), [ShellNotices.tsx](../src/ShellNotices.tsx), [PanelSurfaces.tsx](../src/PanelSurfaces.tsx), [EditorDialogs.tsx](../src/EditorDialogs.tsx) | Root screen. `App` mounts `useEditor` and the start page; the other files each render one part of the editor (app bar and tabs, tool strip + canvas + dock, status bar, notices, side panels, dialogs) and take only the fields they need from the `Editor` result. |
| [main.tsx](../src/main.tsx) | Mounts `App`, installs the usage log and history-step listener, and loads global CSS. |
| [api.ts](../src/api.ts) | `native` invocation (command names typed by `CommandName`) plus usage recording; `edit(revision, command)` for revision-checked edits. |
| [types.ts](../src/types.ts) | Re-exports `src/generated/types.ts` and holds the few frontend-only types (`Viewport`, `ElementKind`, `kindFields`). |
| [generated/](../src/generated) | Written from Rust by `npm run generate:contract`; never edit: `types.ts` (every payload, view, `Command`, error kind), `limits.ts` (limits, MIME/route lists, element/template defaults, size labels and grid default) and `commands.ts` (`CommandName`, `IMAGE_DRAG_EVENT`); `elementFacts.json` cross-checks live frontend labels against Rust, including Unicode. |
| [ProjectCanvas.tsx](../src/ProjectCanvas.tsx) | Canvas rendering and overlays; `useProjectCanvas.tsx` assembles camera, gestures, Request tools, selection, inline editing and drops. |
| [projectCanvas/camera.ts](../src/projectCanvas/camera.ts) | Zoom levels, grid snap, camera fit/pan/zoom, dot grid and rect helpers shared by everything on the canvas. |
| [projectCanvas/boundaryLayout.ts](../src/projectCanvas/boundaryLayout.ts) | Boundary layout from pages and sizes, drawing, push-apart and moves, boundary hit tests. |
| [projectCanvas/referenceGeometry.ts](../src/projectCanvas/referenceGeometry.ts) | Reference-item rects: notes, attachments, resize, push-apart and moves. Its `FrameCorner`, `frameBox`, `drawnFrameRect` and `referenceFrameRect` names predate the Frame → Request rename. |
| [projectCanvas/canvasTargets.ts](../src/projectCanvas/canvasTargets.ts) | Selection target types (`CanvasTarget`, `RequestBox`) and the helpers that filter, toggle and key them. |
| [projectCanvas/pointerGeometry.ts](../src/projectCanvas/pointerGeometry.ts), [pointerNames.ts](../src/projectCanvas/pointerNames.ts), [autoLinks.ts](../src/projectCanvas/autoLinks.ts), [pointerLayout.ts](../src/projectCanvas/pointerLayout.ts) | Pointer routes and hit tests, endpoint names and labels, automatic link lines between boundaries, and the layout (element boxes, moving boundaries) a pointer is drawn against. |
| [EditorDock.tsx](../src/EditorDock.tsx) | Dock host, panel weights, splitters and resize behavior. |
| [dock/panelState.tsx](../src/dock/panelState.tsx), [dock/sections.tsx](../src/dock/sections.tsx) | Persisted dock width/visibility and shared section collapse state. |
| [dock/controls.tsx](../src/dock/controls.tsx), [dock/properties.tsx](../src/dock/properties.tsx) | Reusable property fields and boundary/reference/request/project/pointer editors. |
| [dock/layers.tsx](../src/dock/layers.tsx) | Layers, requests, pointers, styles and named-view tree. |
| [dock/propertiesPanel.tsx](../src/dock/propertiesPanel.tsx), [dock/selection.ts](../src/dock/selection.ts) | Selection derivation and property-editor/behaviour wiring. |
| [projectCanvas/elementFacts.ts](../src/projectCanvas/elementFacts.ts) | Canvas element labels and tree lookup; kind labels, placeholders and default layer names come from the generated Rust contract. Live label/name predicates are cross-checked against generated Rust cases. |
| [EditorTools.tsx](../src/EditorTools.tsx) | Left tool strip, Request area-shape flyout (Pick, Rectangle, Ellipse, Polygon, Freehand), insert flyout, zoom control, labels and shortcut tooltips. |
| [shortcuts.ts](../src/shortcuts.ts) | Action metadata, tool mapping, keys, derived display labels, matching and keyboard help. F starts a Request on the selection or picks the Request tool, Shift+F takes a selected shape's outline, and Delete/Backspace removes the selection. |
| [CommandPalette.tsx](../src/CommandPalette.tsx), [palette.css](../src/palette.css) | Searchable quick actions, ranking, recent commands and keyboard navigation. |
| [CompactField.tsx](../src/CompactField.tsx), [numberExpression.ts](../src/numberExpression.ts) | Compact draft fields, commit/cancel, number expressions, rounding and clamping. |
| [ModalDialog.tsx](../src/ModalDialog.tsx) | Shared modal focus, Escape and focus-return handling for consequential choices such as unsaved work. |
| [editorActions.ts](../src/editorActions.ts) | The action registry: every command (keymap actions and menu-only ones) has one entry with `enabled` and `run`; the canvas supplies its own entries and wins when it has one. Menus, palette, buttons and shortcuts all call `can` and `perform`. |
| [escape.ts](../src/escape.ts), [textField.ts](../src/textField.ts) | Escape: overlays (side panels, banners, layer drags) register and the editor's key handler closes the newest first, then asks the canvas. `isTextField` tells typing from shortcuts. |
| [editorShared.tsx](../src/editorShared.tsx) | Shared editor types/helpers, selection geometry, menu-to-palette commands and the small panel forms. |
| [requestAttachments.ts](../src/requestAttachments.ts) | Shared attachment reading and UI limits for pictures, text and links; native validation remains authoritative. |
| [session/useProjectSession.ts](../src/session/useProjectSession.ts) | Serialized mutation queue, current authoritative view, revision-based edits and settlement; edit failures are reported once and resolve to null. |
| [session/useEditor.tsx](../src/session/useEditor.tsx), [session/projectCommands.ts](../src/session/projectCommands.ts) | `useEditor` connects the feature hooks below and owns the one window key handler (Escape, then the canvas, then shortcuts); `projectCommands` has project open/new/save/close, recovery, replacement and confirmation flows. |
| [session/editorPanel.ts](../src/session/editorPanel.ts), [session/editorMenu.ts](../src/session/editorMenu.ts) | The one open side panel (`EditorPanel`) and the menu and palette built from the action registry. |
| [session/useNotices.ts](../src/session/useNotices.ts), [session/useUpdateCheck.ts](../src/session/useUpdateCheck.ts), [session/useWindowClose.ts](../src/session/useWindowClose.ts), [session/useStartPageData.ts](../src/session/useStartPageData.ts) | Toasts with Undo, the manual update check, the close-window confirmation, and recent projects, launch preference and default project folder. |
| [session/useCanvasTool.ts](../src/session/useCanvasTool.ts), [session/useCanvasSelection.ts](../src/session/useCanvasSelection.ts), [session/useCanvasEdits.ts](../src/session/useCanvasEdits.ts), [session/useRequestEdits.ts](../src/session/useRequestEdits.ts), [session/useGenerationFlow.ts](../src/session/useGenerationFlow.ts) | Armed tool and flyout choices, selection and focus, canvas edits that move the selection or announce Undo, per-layer requests, and preparing/applying generations. |
| [session/useAiSettings.ts](../src/session/useAiSettings.ts), [session/useEditorTheme.ts](../src/session/useEditorTheme.ts) | Serialized AI settings access, sign-in polling and persisted editor appearance. |
| [session/useBoundaryCommands.ts](../src/session/useBoundaryCommands.ts), [session/useNamedViews.ts](../src/session/useNamedViews.ts) | Boundary creation/resizing/layout and named camera-view edits. |
| [session/projectEdits.ts](../src/session/projectEdits.ts) | Feature editing API shared by canvas and property controls. |
| [preferences.ts](../src/preferences.ts), [usePoll.ts](../src/usePoll.ts) | Optional browser preference storage and cancellable, non-overlapping native progress polling. |
| [exportJobs/useExportJobs.ts](../src/exportJobs/useExportJobs.ts) | Export jobs, preview, Play coordination and debounced pointer-clash rechecks. |
| [Menus.tsx](../src/Menus.tsx) | Accessible menu button, popover positioning and keyboard navigation. |
| [WorkspaceSurface.tsx](../src/WorkspaceSurface.tsx) | Panel/banner/start/canvas surface host, stacking and focus return; each open surface registers with `escape.ts`. |
| [ProblemBanner.tsx](../src/ProblemBanner.tsx), [noticeSummary.ts](../src/noticeSummary.ts) | Notice queue/host, problem banners and the changed-pages summary for generation results. |
| [Disclosure.tsx](../src/Disclosure.tsx), [StatusChip.tsx](../src/StatusChip.tsx), [icons.tsx](../src/icons.tsx) | Shared disclosure row, status badge and inline icon set. |
| [DraftText.tsx](../src/DraftText.tsx) | `useDraftText`: batched text commits that never overwrite newer typing. |
| [dock/historyPanel.tsx](../src/dock/historyPanel.tsx), [historySteps.ts](../src/historySteps.ts) | Versions/alternatives in the dock, derived history labels, and ending the native history step when a text field loses focus. |
| [Generation.tsx](../src/Generation.tsx), [GenerationLog.tsx](../src/GenerationLog.tsx) | Generation status and AI conversation: usage, retries, proposals, criticism and raw reply. |
| [useGeneration.ts](../src/useGeneration.ts), [generationSession.ts](../src/generationSession.ts) | Job lifecycle/polling/submit/cancel/retry, owned-session notices and restored-job silence. |
| [errorMessage.ts](../src/errorMessage.ts) | One leaf extractor for error text and preserved typed failures; state retains native/provider kinds until presentation. |
| [generationProblems.ts](../src/generationProblems.ts), [userErrors.ts](../src/userErrors.ts) | Per-area problem callouts/outcomes, typed generation outcomes and native/provider failure presentation; actions use error kinds. |
| [AiSettings.tsx](../src/AiSettings.tsx) | Provider/model/key, ChatGPT accounts, secure-storage status and access checks. |
| [ImportContext.tsx](../src/ImportContext.tsx), [identityReview.ts](../src/identityReview.ts) | Brand evidence review/approval and parsing findings into colours, fonts, logos and sections. |
| [WebsiteImport.tsx](../src/WebsiteImport.tsx), [websiteImportExtract.ts](../src/websiteImportExtract.ts) | Import dialog, hidden page rendering and the script-disabled extraction wrapper that posts a compact page tree. |
| [StartPage.tsx](../src/StartPage.tsx), [ProjectLocation.tsx](../src/ProjectLocation.tsx) | Home rail, template tiles, recent projects with pin/remove, launch setting; default project-location settings. |
| [RecoveryBanner.tsx](../src/RecoveryBanner.tsx), [SaveConflict.tsx](../src/SaveConflict.tsx) | Recovery choice and checked-save conflict resolution with typed results. |
| [Notes.tsx](../src/Notes.tsx), [StyleGuide.tsx](../src/StyleGuide.tsx) | Project brief, decisions and exceptions with scope labels; visual brand rules and samples. |
| [CodeEditor.tsx](../src/CodeEditor.tsx), [codeMirrorMetadata.ts](../src/codeMirrorMetadata.ts), [editorHtml.ts](../src/editorHtml.ts) | CodeMirror for `code` elements, metadata anchor mapping across edits, and reversible projection of inline resources. No page-source editor exists. |
| [pastePreference.ts](../src/pastePreference.ts) | Linked/unlinked paste default. |
| [updateCheck.ts](../src/updateCheck.ts), [usage.ts](../src/usage.ts) | Update-result copy and batched local usage events. |
| [CrashReport.tsx](../src/CrashReport.tsx), [crashReportText.ts](../src/crashReportText.ts), [visibleActions.ts](../src/visibleActions.ts) | Editor error boundary/crash notice, crash copy and issue-address fallback, and conditional-control visibility rules. |
| [styles.css](../src/styles.css), [brand.css](../src/brand.css), [shell.css](../src/shell.css), [canvas.css](../src/canvas.css), [ui.css](../src/ui.css), [dialogs.css](../src/dialogs.css), [notice.css](../src/notice.css), [startPage.css](../src/startPage.css) | Base tokens/reset, brand type/wordmarks, app shell and tabs, canvas layers, shared controls, dialogs/surfaces, notices and Home. |
| [assets/brand/desegnos-wordmark-light.svg](../src/assets/brand/desegnos-wordmark-light.svg), [desegnos-wordmark-dark.svg](../src/assets/brand/desegnos-wordmark-dark.svg), [archivo-variable.woff2](../src/assets/brand/archivo-variable.woff2) | Bundled wordmarks and Archivo web font referenced by the shell and brand CSS. |

## `src/projectCanvas/`

| Module | Owns |
| --- | --- |
| [useProjectCanvas.tsx](../src/projectCanvas/useProjectCanvas.tsx), [useNativeDrop.ts](../src/projectCanvas/useNativeDrop.ts) | Canvas state and composition of drawing, moving, picking, paste/drop, pointer and keyboard actions; files dropped from the operating system. |
| [interactionTypes.tsx](../src/projectCanvas/interactionTypes.tsx) | Canvas props/handle, drag/clipboard/selection types and shared hit-testing helpers. |
| [interaction.ts](../src/projectCanvas/interaction.ts) | Pointer sessions, gesture dispatch, drag updates/completion and cancellation. |
| [drawing.ts](../src/projectCanvas/drawing.ts), [moving.ts](../src/projectCanvas/moving.ts) | Element/boundary drawing; selection movement, resize, reparenting and duplicate-drag operations. |
| [requestTool.ts](../src/projectCanvas/requestTool.ts), [pointerTool.ts](../src/projectCanvas/pointerTool.ts) | Request Pick/area drafts and target collection; pointer drawing and endpoint reattachment. |
| [keyboardActions.ts](../src/projectCanvas/keyboardActions.ts), [arrange.ts](../src/projectCanvas/arrange.ts) | Canvas keyboard actions, selection transforms and pure sibling paint-order operations. |
| [pasteDrop.ts](../src/projectCanvas/pasteDrop.ts) | Clipboard ownership, linked/unlinked paste choices, external paste and drop coordination. |
| [clipboardPaste.ts](../src/projectCanvas/clipboardPaste.ts), [fileDrop.ts](../src/projectCanvas/fileDrop.ts) | Clipboard markers, image/text paste helpers and sizing; dropped-file classification and reading. |
| [selectionBar.tsx](../src/projectCanvas/selectionBar.tsx) | Selection actions and This size / All sizes editing scope. |
| [Minimap.tsx](../src/projectCanvas/Minimap.tsx), [minimap.ts](../src/projectCanvas/minimap.ts) | Overview UI, fit visibility, coordinate transforms and camera navigation. |
| [canvasView.ts](../src/projectCanvas/canvasView.ts) | Cached resolved geometry per project revision, model hit boxes and viewport visibility checks. |
| [boundaries.tsx](../src/projectCanvas/boundaries.tsx) | Boundary content per linked size: element trees, hidden-size placeholders and Play rendering. |
| [boundaryLabels.tsx](../src/projectCanvas/boundaryLabels.tsx) | Boundary names above each corner in screen pixels; labels double as drag handles. |
| [elementGeometry.ts](../src/projectCanvas/elementGeometry.ts) | Element paths, text measurement, picture URLs, labels, insert groups, origins and parent-relative rects. |
| [dock/elementLayers.tsx](../src/dock/elementLayers.tsx) | Layers tree rows for one size, collapse state, visibility/lock/delete buttons, drag reordering and drop targets. |
| [dock/elementProperties.tsx](../src/dock/elementProperties.tsx) | A layer's Properties: geometry, style, text, image, layout, code, request box, history and context attachments. |
| [elements.tsx](../src/projectCanvas/elements.tsx) | Element drawing, selection overlay, resize handles and inline text editing. |
| [dock/exportPanel.tsx](../src/dock/exportPanel.tsx) | Export dock panel: boundary/output rows, status, warnings, estimate and result actions. |
| [requests.tsx](../src/projectCanvas/requests.tsx) | Requests on the canvas: pending request markers, area outlines and target highlights, covered-target collection, target labels and the request box editor. |
| [requestGeometry.ts](../src/projectCanvas/requestGeometry.ts) | Request area outlines: shape bounds, point-in-outline coverage for rect/ellipse/polygon/freehand and SVG path data. |
| [linkedChoices.tsx](../src/projectCanvas/linkedChoices.tsx) | Inline paste Linked/Unlinked, link-copy and size-placeholder choices. |
| [links.ts](../src/projectCanvas/links.ts) | Linked-definition resolution, shared path/id hashing with the core, copy inspection and structure comparison. |
| [play.ts](../src/projectCanvas/play.ts) | Transient Play state, click-to-pointer lookup and visual show/hide/toggle/swap application. |
| [pointers.tsx](../src/projectCanvas/pointers.tsx) | Pointer lines, comments on lines, handles, hit routes, automatic link lines and comment editors. |
| [reference.tsx](../src/projectCanvas/reference.tsx) | Reference items (note, link, image, swatch), picture lookup and the reference note editor. |
| [sizes.ts](../src/projectCanvas/sizes.ts) | Size presets, base-size rules, per-size override resolution and boundary names. |
| [surface.ts](../src/projectCanvas/surface.ts) | Camera hook, wheel/Space pan, zoom-at-pointer, screen rects and native drag types. |

## Key data flows

### Project file: open, save, version, restore

1. **Open** (`open_project`, recent, or Home) → `Workspace::open_file_tab` → `ProjectDatabase::open` on the `.desegnos` file, taking a sidecar `.lock` and enabling WAL. `load_project` joins the content-addressed parts back into a `Project`.
2. **Untitled** (`new_untitled_project`) creates `Project::untitled` named `Canvas N` (blank or a template's starter boundaries) and a real SQLite database under app data `untitled/<id>.desegnos`, plus a `<project-id>.recovery.json` pointer. Opening, creating and recovering add a tab; `switch_project`/`close_project` change the active one. Closing the last tab returns to Home.
3. **Autosave** while dirty runs at most every 3 seconds (`autosave_project`) into the hidden `__autosave` branch, so a crash keeps edits. Saves seconds apart amend the previous version instead of adding one.
4. **Save** (`save_untitled_project`, `save_project_checked`, `save_project_copy`) writes one transaction and labels the new version from `Session` history. `save_untitled_project` asks for a name and file for an untitled project and removes its recovery database; `save_project_copy` writes one version without history. Checked saves compare the stored `FileStamp` (project id + head version); a changed/different/unreadable file returns a conflict resolved by `resolve_save_conflict` as copy, replace or cancel.
5. **Versions** are manifests of part hashes with parent, branch, time and label. `restore_version` writes the restored state as a new version and applies it as **one undo step**; history is never rewritten. `switch_alternative` / `create_alternative` / `rename_alternative` / `delete_alternative` move between named branches (`Main` cannot be deleted); `compact_history` thins old versions.
6. **Recovery** at startup scans `.recovery.json` pointers, reads the autosave branch with `read_autosave` and publishes `WorkspaceView.recoveries`. `recover_project` loads the unsaved state, `dismiss_recovery` discards the autosave copy and clears the offer; [RecoveryBanner.tsx](../src/RecoveryBanner.tsx) offers Recover or Open saved version.

### Edit commands and undo

1. A canvas gesture or panel control builds a `Command` (`InsertElements`, `UpdateElements`, `MoveElements`, `GroupElements`, `LinkElements`, `AddRequest`, `UpdateRequest`, `RemoveRequests`, `DeleteItems`, `SetCanvasLayout`, `ImportWebsite`, `RecordExport`, …).
2. [useProjectSession.ts](../src/session/useProjectSession.ts) serializes mutations and calls `edit(revision, command)` → `edit_project` → `Session::execute`. `assert_revision` rejects a stale base; `Project::apply` validates the command, enforces bounds and increments `Project.revision`.
3. `Session` pushes the previous project onto the undo stack as one history step. A burst of edits to one request text, page/size setting, pointer comment, project name or grid merges into one step until the pause (5 seconds) or `end_history_step` (`historySteps.ts` calls it when a text field loses focus).
4. `undo_project`/`redo_project` traverse the same session; autosave then runs if due. Element designs, requests, reference, pointers, brief and boundaries all share this history and durability.
5. Recording an export is metadata, not a design change: `Session::record_export` updates the project without pushing an undo step. Applying a generation reply is one design edit and one undo step.

### Requests: targets and areas

1. **Pick** is the Request tool's default mode. With a layer, reference item, pointer or boundary selected, F starts a request on the selection; with nothing selected it selects the tool. A click starts the request on the target under the cursor, or removes it when it is already the target; Shift-click adds or removes one target of a multi-target request.
2. **Areas** come from the tool's flyout: Rectangle, Ellipse, Polygon and Freehand. [requestGeometry.ts](../src/projectCanvas/requestGeometry.ts) tests the real outline (`requestAreaContains`), not its bounding box, and [requests.tsx](../src/projectCanvas/requests.tsx) collects `coveredTargets`: layers mostly inside, boundaries wholly inside, reference items inside and pointers whose line crosses. An empty area inside a boundary targets that boundary, so new layers can fill the area.
3. **Shift+F** uses the selected shape's own outline (its rectangle, or its ellipse for an ellipse layer) as the area and keeps the shape as a target. Polygon points close with Enter or a click near the first point; Escape cancels a draft.
4. The box is `RequestBoxEditor` in screen pixels beside the targets. Enter queues a `PendingRequest` (`text`, `attachments`, `targets`, `area`, `requestedAt`) through `add_request`; Escape cancels. `RequestLayer` draws each pending request's marker, area outline and, when selected, its target highlights. The dock's Requests group and [RequestProperties](../src/dock/properties.tsx) edit text, show targets and the area label, attach reference items and delete the request.

### Deleting selections

1. **Delete/Backspace** removes every selection kind in one gesture: layers, reference items, pointers and requests, or the focused boundary when nothing is selected. The canvas action modules collect the selection and call `removeCanvasSelection` in [useEditor.tsx](../src/session/useEditor.tsx), which sends one `delete_items` command.
2. The dock's row trash buttons use the same path: a layer row, reference row, pointer row or request row deletes just that item, and a boundary is removed as a page or as one linked size through `delete_items` page/size ids. [RequestProperties](../src/EditorDock.tsx) has its own Delete request action.
3. Core removes explicitly selected requests first, then their targets, then `prune_requests` drops dangling targets and requests left with nothing to act on. The gesture is one undoable step and its toast offers Undo.

### Generation: request → element operations → apply

1. Each `PendingRequest` in `Project.requests` carries its text, attachments, targets and optional area. Requests are built on the canvas by Pick clicks, a drawn area or F/Shift+F, or edited in the request dock or a layer's Properties box (which edits the one request that targets only that layer).
2. **Generate** → `prepare_generation` → `GenerationService::prepare_requests` → `Project::prepare_targeted_requirements` freezes a `RequirementsSnapshot`: brief, decisions, exceptions, each request's text, targets and area, all linked sizes and element trees, reference, pointers, pictures and capabilities. Every pending request is frozen with its own digest, so changing its text, targets or area makes the snapshot stale. It carries a SHA-256 digest. The job is persisted before any paid send.
3. `start_generation` / `retry_generation` stream from the selected provider with cancellation, deadline, usage and validation. The accepted reply is a `GenerationCandidate`: per-request `ElementOperation`s (`ReplaceChildren`, `Update`, `Insert`, `Remove`, `InsertInBoundary`, `AddReference`), generated image requests, pointer classifications, decision proposals and design criticism. `get_generation` polls the latest job; `list_generations`/`get_past_generation` expose history; `cancel_generation` and `discard_generation` stop or drop it.
4. `apply_generation` rechecks that the snapshot is still current, resolves permitted images through the provider and project picture store, then `Workspace::apply_generated` → `Session::apply_generation` → `apply_generation_output`. The core validates ids, scope and layout, assigns element ids, applies overrides/Play actions, records the applied job id and publishes the new elements. This is one undo step.
5. `record_generation` stores the draft dossier (`GenerationDossier`: attempt, base revision, requirements/candidate digests, summary, targets, proposals, criticism, application status) for history and audit. Automatic site application never silently accepts identity proposals.

### Export: model visuals, generated behaviour, PDF/PNG/SVG and clashes

1. **Export** → [useExportJobs.ts](../src/exportJobs/useExportJobs.ts) → native `prepare_export`. Native checks project id/revision, inclusion and saved-version requirements, chooses a destination and freezes `plan_export`.
2. [web_codegen.rs](../crates/desegnos-core/src/web_codegen.rs) compiles HTML/CSS from the resolved website/app-screen model and linked sizes. Missing, uncached change-pointer behaviours can require an AI job; AI replies supply marked JavaScript, not replacement markup or CSS. Direct exports can write without a provider request; missing behaviours are reported rather than invented.
3. [render.rs](../crates/desegnos-core/src/render.rs) and [render/pdf.rs](../crates/desegnos-core/src/render/pdf.rs) write slides as `slides.pdf` and images as PNG/eligible SVG. Bundles pass to storage's staged writer. Full exports record version, destination, page files, pointer fingerprints and accepted behaviour cache; previews use temporary folders.
4. A prepared AI job waits for the explicit `start_export` paid-send decision. Parsing and bundle validation check frozen identity, behaviour ownership, syntax, paths and size limits before writing.
5. `recheck_export` detects drift in pointer-marked code. Keep code rewrites the comment to match; Keep comment forgets wiring for rebuilding. Contradicting pointers resolve with `ResolvePointerClash`; broken endpoints are reattached or removed. [useExportJobs.ts](../src/exportJobs/useExportJobs.ts) schedules checks on project updates and window focus.
6. `open_in_browser` serves the last export through `LocalSite`; `open_export_folder` checks its destination; `desegnos-export.json` is the manifest. These are ordinary output files. Implementation alone does not prove responsive or visual quality; see [capability status](../CANVAS.md#capability-status).

### Preview export and Play

1. **Preview export** (Ctrl/Cmd+Enter) calls `prepare_export` with `preview: true` and exactly one included webpage/app-screen boundary. It writes to a fresh temporary preview folder without the manifest, serves it through `LocalSite` at a token-prefixed loopback URL, and [ProjectCanvas.tsx](../src/ProjectCanvas.tsx) draws it in an opaque sandboxed iframe over the boundary. A revision change marks it stale; Refresh re-exports, Escape/Close calls `close_preview` and stops the server.
2. **Play** (Shift+Space) is client-only and never saved. `PlayState` (`src/projectCanvas/play.ts`) holds the active boundary plus hidden elements and swaps. Clicking an element runs the matching pointer whose `Pointer.play` action was classified by generation: go to boundary, show, hide, toggle or swap. Go-to re-fits the camera on the target boundary. Escape exits.
3. Preview and Play share `preview-overlay`/`.playing` canvas layers, but Preview shows real exported code while Play only restyles the in-memory design.

### Website import to elements

1. **Import website…** calls `prepare_website_import_url` or `pick_website_import_source` → bounded fetch (`fetch_website_snapshot`) or read (`collect_import_snapshot`/`collect_import_html`) into a private import folder, served by `LocalSite` and held in `WebsiteImportState`.
2. [WebsiteImport.tsx](../src/WebsiteImport.tsx) renders each page in a hidden iframe through [websiteImportExtract.ts](../src/websiteImportExtract.ts). The page itself runs in a nested script-disabled frame; only the extractor reads the DOM and posts a compact `WebCapture` tree of positions, text, computed styles, links, pictures and script tags.
3. `import_website_pages` validates routes and picture sources, downloads or reads bounded images, stores them through `Project::prepare_picture`, then calls the pure `import_website` conversion in core. It returns pages, elements, reference notes, behaviour pointers where recognisable, linked-size overrides and pictures.
4. `Command::ImportWebsite` applies the whole import in **one undo step**. Page code never runs; scripts become notes or pointer comments.

Brand import is separate: `prepare_website_import_url` and `pick_website_import_source` (website evidence) feed [identity.rs](../src-tauri/src/identity.rs) and [identity_source.rs](../crates/desegnos-storage/src/identity_source.rs) collect evidence; `analyze_identity` asks the configured provider; approved findings and logos go through `import_context` into brief decisions and project pictures as one undo step. Evidence pages and source files never gain file or network authority.

### Start page, untitled projects and recovery

1. Launch asks `get_app_settings`. With **Show start page on launch** on, [StartPage.tsx](../src/StartPage.tsx) shows the Home rail, Blank canvas and template tiles, then recent projects. Turning the setting off calls `new_untitled_project` immediately and, if recoveries exist, offers them.
2. A template (`website`, `appScreen`, `slideDeck`, `socialImage`) creates its own starter boundaries with common sizes; `blank` creates one untitled canvas. Untitled projects get the lowest unused `Canvas N` and live in app data until their first save names them.
3. Recent projects support pin, remove and Enter/double-click open. `forget_recent_project` and `set_recent_project_pin` only change the catalogue; the project files are untouched.
4. Recovery offers appear on Home and in the project surface. Recover opens the hidden autosave branch in its own tab; Open saved version opens the file and discards the autosave; dismiss discards the autosave copy. A failed checked save shows [SaveConflict.tsx](../src/SaveConflict.tsx) instead of overwriting.

## Terminology: spec vs code

[GLOSSARY.md](../GLOSSARY.md) is authoritative for names. Current code still carries older identifiers:

| Spec term | In code | Note |
| --- | --- | --- |
| **Boundary** | `Page`, `PageSize`, `Page.sizes`, `BoundaryKind::{Page, Screen}` | One `Page` draws one boundary per linked size. "Page" here is a canvas output, not an HTML page or a website home page. |
| **Layer** | `Element`, `ElementKind::Group` | A layer is one element or a group; `ElementMeta` carries its request history and context. |
| **Request** | `PendingRequest`, `Project.requests`, `RequestLayer`, `RequestBoxEditor`, [RequestProperties](../src/EditorDock.tsx), `request_history.rs` | Requests belong to the project, not to one layer, so one request can target a mix of layers, boundaries, reference items and pointers. The old **Frame** word survives in geometry helpers (`FrameCorner`, `frameBox`, `drawnFrameRect`, `referenceFrameRect` in [referenceGeometry.ts](../src/projectCanvas/referenceGeometry.ts)); read all of them as Request. The session label `"AI filled frames"` is historical wording for a generation step. |
| **Reference** | `Project.reference`, `ReferenceItem`, `ReferenceKind`, [reference.tsx](../src/projectCanvas/reference.tsx) | The spec renamed **Moodboard** to Reference. No `moodboard` identifier remains in `src/`, `crates/` or `src-tauri/`; only the glossary's Gone table still records the old word. |
| **Pointer / comment / wired** | `Pointer`, `Pointer.comment`, `PointerWiring`, `PointerState` | `Pointer.play` holds a classified simple action. |
| **Clash** | `PointerState::{CodeChanged, Contradicts, Broken}`, `ExportRecord.pointers`, `pointer_fingerprints` | Resolved by `ResolvePointerClash`, `SetPointerWiring`/Keep code and `ForgetExportPointer`/Keep comment. |
| **Linked sizes / base size / override / placeholder** | `PageSize`, `Page.sizes`, `ElementOverride`, `Element.overrides`, `resolve_for_size`, [sizes.ts](../src/projectCanvas/sizes.ts) | This size / All sizes controls choose scope; double-click enters groups or text. Placeholders mark hidden elements. |
| **Play / Preview export** | `PlayState`, `playClick`/`playPointerAt`, `prepare_export { preview: true }`, `LocalSite` | Play is transient; Preview runs the real export in a sandbox. |
| **Export / Open in browser** | `ExportPlan`, `plan_export`, `ai_export_bundle`, `direct_export_bundle`, `ExportRecord`, `LocalSite` | Websites/app screens compile visuals from the model and may use AI for missing behaviour; slides/images render directly. |
| **Project file / version / history / alternative** | `ProjectDatabase`, `versions` table, `ProjectHistoryView`, `branches` table, `create_alternative` | Restore is a new version and one undo step. |
| **Flexible / Precise** | `GenerationMode::{Fuzzy, Strict}` | Project-level setting frozen into the generation job. |
| **Scope / targets** | `Scope` (project, boundary or layer) and `PendingRequest.targets` | Decisions and exceptions carry a `Scope`; a request's scope is its targets plus what pointers grant. |
| **Targets** | `RequestTarget` (`element`, `boundary`, `reference`, `pointer`), `coveredTargets`, `requestTargetKey`, `scope_for` | What a request may change. Pick targets what you click, an area targets what its outline covers, and F/Shift+F target the selection. Core validates targets on every edit and prunes those that disappear. |
| **Request shape** | `RequestShape` (`rect`, `ellipse`, `polygon`, `freehand`), `RequestArea`, [requestGeometry.ts](../src/projectCanvas/requestGeometry.ts) | Coverage uses the real outline: `requestAreaContains`, `requestShapePath` and `requestShapeBounds` in the renderer, `RequestShape` validation in core. Rectangle, ellipse, polygon and freehand are implemented; Shift+F takes a selected shape's outline. |

Named views persist in `Project.views`; commands and validation live in [model.rs](../crates/desegnos-core/src/model.rs), the Layers UI in [layers.tsx](../src/dock/layers.tsx), and camera/shortcut handling in [useProjectCanvas.tsx](../src/projectCanvas/useProjectCanvas.tsx) and [keyboardActions.ts](../src/projectCanvas/keyboardActions.ts). [views.rs](../crates/desegnos-core/tests/views.rs) covers the model contract.

## Tests, builds and evidence

| Command | Runs |
| --- | --- |
| `npm run dev` | Vite frontend on `127.0.0.1:1420` (strict port), derived from `src-tauri/tauri.conf.json` → `build.devUrl`. |
| `npm run desktop` | `scripts/run.mjs tauri` → Tauri dev app. |
| `npm run typecheck` | `tsc --noEmit`. |
| `npm run build` | `tsc --noEmit && vite build`. |
| `npm test` | `cargo test --workspace` through `scripts/run.mjs cargo`. |
| `npm run check:rust` | Clippy for the workspace, all targets, warnings denied. |
| `npm run test:editor` | `scripts/test-editor.mjs` discovers every top-level `tests/*.test.mjs` except the three headless-browser suites (AI models, canvas UX, notices). Includes Play, release orchestration and launcher tests. |
| `npm run test:canvas-ux` | `tests/canvas-ux-browser.test.mjs`: canvas interactions in headless Chromium. |
| `npm run test:ai-models` | `tests/ai-models-browser.test.mjs`: AI model list in headless Chromium. |
| `npm run generate:contract` | `scripts/generate-contract.mjs`: regenerates `src/generated/` from Rust. |
| `npm run test:notices` | `tests/notice-system.test.mjs`: notice queue and layout in headless Chromium (~10s, kept out of `test:editor`). |
| `npm run test:smoke` | `tests/native/canvas-smoke.mjs`: builds the `native-tests,gtk-dialogs` app and drives the real editor on a guarded Xvfb display and private session bus. Linux only. |
| `npm run test:vault` | `scripts/test-vault.mjs`: native secret-service vault flow against a loopback provider. |
| `npm run release:local` | `scripts/build-release.mjs`: Tauri release build (no bundle), then the guarded Linux startup check. `--dist` keeps the full LTO release profile. |

Every top-level test file runs under one of these scripts. Packages are installed with `npm` (`package-lock.json`). `node scripts/startup-test.mjs --binary <path>` runs the guarded startup check directly; set `DESEGNOS_XVFB_GUARD_DIR` to a directory inside the private runtime dir if your Xvfb wrapper reserves per-display lock files there.

| Test file | Covers |
| --- | --- |
| [hug_synchronization.rs](../crates/desegnos-core/tests/hug_synchronization.rs) | Fixed-geometry edits and Hug synchronization in descendants and size overrides; replaces the former timing probe. |
| [play.test.mjs](../tests/play.test.mjs) | Play click routing, pointer precedence and ancestor targets. |
| [build-release.test.mjs](../tests/build-release.test.mjs) | Release orchestration, `--dist` profile and failure stopping. |
| [behaviours.test.mjs](../tests/behaviours.test.mjs) | Behaviour pointers, add/update/remove edits and Built/Runs dock status. |
| [element-layers.test.mjs](../tests/element-layers.test.mjs) | Layer-row drop targets: before, after, into a group, self-drops. |
| [export-folder.test.mjs](../tests/export-folder.test.mjs) | Default and explicit export destinations across project renames. |
| [export-panel.test.mjs](../tests/export-panel.test.mjs) | Export counts for reused behaviour code. |
| [project-commands.test.mjs](../tests/project-commands.test.mjs) | Open/recover/save flows and disk-conflict resolution. |
| [provider-errors.test.mjs](../tests/provider-errors.test.mjs) | Native failure wording and typed recovery actions. |
| [canvas-fonts.test.mjs](../tests/canvas-fonts.test.mjs) | Canvas font loading, alias sharing and private measuring. |
| [canvas-ux-browser.test.mjs](../tests/canvas-ux-browser.test.mjs) | Real dock clicks and text geometry in headless Chromium. |
| [ai-models-browser.test.mjs](../tests/ai-models-browser.test.mjs) | AI model list UI in headless Chromium. |
| [number-expression.test.mjs](../tests/number-expression.test.mjs) | Pure number-expression parsing and field resolution. |
| [canvas-model.test.mjs](../tests/canvas-model.test.mjs) | Linked-size helpers, overrides, placeholders, origins, id parity with Rust, keymap uniqueness and keyboard dispatch. |
| [code-mirror-state.test.mjs](../tests/code-mirror-state.test.mjs) | CodeMirror metadata, exact edits, undo/redo and lagging React echoes. |
| [crash-report.test.mjs](../tests/crash-report.test.mjs) | Crash notice copy and issue-address fallback. |
| [editor-shell-ssr.test.mjs](../tests/editor-shell-ssr.test.mjs) | Server render of the editor shell with an unwired pointer. |
| [generation-cancellation.test.mjs](../tests/generation-cancellation.test.mjs) | Prepare/start/stop ordering and no request after Stop. |
| [generation-conversation.test.mjs](../tests/generation-conversation.test.mjs) | Declared-question, rejection and open-request actions in the AI conversation. |
| [generation-problems.test.mjs](../tests/generation-problems.test.mjs) | Per-area callout placement, pan and dismissal. |
| [generation-session.test.mjs](../tests/generation-session.test.mjs) | Owned vs restored job notices. |
| [generation-status.test.mjs](../tests/generation-status.test.mjs) | Status-bar retry, fix and question logic. |
| [identity-review.test.mjs](../tests/identity-review.test.mjs) | Colour/font parsing, roles and deduplication. |
| [import-flow.test.mjs](../tests/import-flow.test.mjs) | Brand source review, resume and page-import handoff. |
| [notice-summary.test.mjs](../tests/notice-summary.test.mjs) | Outside-change summaries and fallbacks. |
| [notice-system.test.mjs](../tests/notice-system.test.mjs) | Headless notice queue, preemption, duplicates and layout. |
| [problem-banner.test.mjs](../tests/problem-banner.test.mjs) | Banner variants, callbacks, close and priority order. |
| [project-canvas-geometry.test.mjs](../tests/project-canvas-geometry.test.mjs) | Boundary layout/push/snap, sizes and camera fitting. |
| [recovery-banner.test.mjs](../tests/recovery-banner.test.mjs) | Recovery error typing, choice markup and persistence beside open projects. |
| [start-page.test.mjs](../tests/start-page.test.mjs) | Home rail, templates, recents, pin/remove and launch toggle. |
| [tauri-launcher.test.mjs](../tests/tauri-launcher.test.mjs) | Launcher `CI` normalization and environment. |
| [update-check.test.mjs](../tests/update-check.test.mjs) | Newer/current/no-release update messages. |
| [visible-actions.test.mjs](../tests/visible-actions.test.mjs) | Conditional control visibility across surfaces. |
| [workspace-surfaces.test.mjs](../tests/workspace-surfaces.test.mjs) | Non-modal editor panels (with the unsaved-work modal exempted), panel switching, save-conflict focus and Escape. |
| [typescript.mjs](../tests/typescript.mjs) | Compiles browser TS entry points into an importable Node tree for tests. |
| [native/canvas-smoke.mjs](../tests/native/canvas-smoke.mjs) | Real-app Xvfb/WebDriver smoke: Home, templates, tools, Request Pick and drawn area shapes with queued requests, boundary/element drawing, Properties, Layers, sizes, History and save/reopen. |
| [native/webdriver.mjs](../tests/native/webdriver.mjs) | Minimal WebDriver client and wait helpers. |
| [native/provider-server.mjs](../tests/native/provider-server.mjs) | Loopback provider fixture that asserts no synthetic keys leak. |
| [native/oauth-server.mjs](../tests/native/oauth-server.mjs) | Loopback OAuth/JWKS fixture. |
| [native/x11-input.py](../tests/native/x11-input.py) | X11 input helper for the native save dialog. |

Evidence lands under `test-results/`, which is gitignored:

- `test-results/smoke/<ISO>-<pid>/`: `app.log`, `xvfb.log`, `report.json`, `final.png`, `saved-project.sha256`, dialog JSON, `browser-errors.json`, `desktop-cleanup.json`; failures add `failure.png`, `failure.html` and `failure-steps.json`.
- `test-results/startup/<ISO>-<pid>/`: `app.log`, `startup.png`, `report.json`, `cleanup.json`, Xvfb log.
- `test-results/system-vault/<ISO>-<pid>/`: vault test evidence.

Older logs, screenshots and JSON at the root of `test-results/` are historical records from earlier work; they are not regenerated by the current scripts.

## Scripts and documents

- [run.mjs](../scripts/run.mjs), [toolchain.mjs](../scripts/toolchain.mjs): shared Cargo/Tauri process runner and pinned toolchain discovery.
- [test-editor.mjs](../scripts/test-editor.mjs): discovers and runs the non-browser Node test files.
- [build-release.mjs](../scripts/build-release.mjs): local release build and startup check; `--dist` for publishing.
- [startup-test.mjs](../scripts/startup-test.mjs), [test-vault.mjs](../scripts/test-vault.mjs), [xvfb.mjs](../scripts/xvfb.mjs), [native-guard.mjs](../scripts/native-guard.mjs): guarded native diagnostics; `native-guard.mjs` holds the process-group, private-XDG and desktop-unchanged checks shared by the startup, vault and smoke tests.
- [usage-report.mjs](../scripts/usage-report.mjs): summarises local usage JSONL and generation folders (`job.json`, `REQUIREMENTS.md`) into Markdown from the data folder given as an argument or `DESEGNOS_DATA_DIR`. It sends nothing.
- [docs/](./): contracts and guides, including [CHATGPT_CONTRACT.md](./CHATGPT_CONTRACT.md), [IDENTITY_IMPORT.md](./IDENTITY_IMPORT.md), [INSTALL_AND_FIRST_SITE.md](./INSTALL_AND_FIRST_SITE.md), [GENERATION_BENCHMARK.md](./GENERATION_BENCHMARK.md) and the CodeMirror dependency records. Check their claims against source before changing behaviour.
