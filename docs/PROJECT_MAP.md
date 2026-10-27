# Desegnos project map

A source map for people and AI agents. Paths are relative to this document. Product direction is [PRODUCT.md](../PRODUCT.md), the canvas concept [CANVAS.md](../CANVAS.md), the editor look [DESIGN.md](../DESIGN.md) and vocabulary [GLOSSARY.md](../GLOSSARY.md).

## Architecture

The Rust workspace has three libraries and the Tauri desktop application. Crates use `desegnos`; the application identifier is `io.github.chelij.desegnos`.

- **`desegnos-core`** owns the project model, canvas elements, boundaries, pointers, reference, history semantics, generation contracts, direct renderers, export plans and website-to-element conversion.
- **`desegnos-storage`** owns the `.desegnos` SQLite project, versions and alternatives, folders, safe export writing, brand evidence and website snapshot readers.
- **`desegnos-ai`** owns provider requests, credentials, ChatGPT authentication, streaming replies, image APIs and safe website fetching.
- **Tauri** owns native state, dialogs, project tabs, loopback static servers, import rendering, workspace/generation orchestration and export.
- **React/TypeScript** owns the canvas editor. [main.tsx](../src/main.tsx) mounts [App.tsx](../src/App.tsx); [api.ts](../src/api.ts) wraps `invoke`; [types.ts](../src/types.ts) mirrors native payloads.

Typical boundary: component → `native(command, args)` → Tauri command → library operation → serialized view. `WorkspaceView` returns the authoritative project, its open-project tabs and history state. Mutations carry an expected revision; `Session` persists a recovery snapshot before publishing.

There is one editor: the project canvas. Code is not stored on the canvas and exists only in exports. Imported websites become canvas elements; brand import remains evidence for the project brief.

## Source tree

```text
crates/
├── desegnos-core/src/       Project model, elements, generation, renderers, export, website import.
├── desegnos-storage/src/    SQLite project, versions, safe export writing, import snapshots.
└── desegnos-ai/src/         Providers, transports, credentials, images, ChatGPT, website fetching.
    └── chatgpt/             Sign-in, account lifecycle, token storage and HTTP/callback.
src-tauri/src/               Desktop commands, workspace, export, local site, import.
src/                         React editor: canvas, dock, menus, dialogs, generation, import.
├── projectCanvas/           Canvas layers: boundaries, elements, frames, reference, pointers.
└── assets/brand/            Bundled wordmarks and Archivo font.
tests/                       Kept Node tests (canvas geometry).
scripts/                     Toolchain/release wrappers and usage reporting.
docs/                        Contracts, guides and this map.
```

## `desegnos-core`

[lib.rs](../crates/desegnos-core/src/lib.rs) re-exports the public API.

| Module | Owns |
| --- | --- |
| [model.rs](../crates/desegnos-core/src/model.rs) | `Project`, `Page`/boundary placement, `Command`, `Project::apply`/`validate`, reference items, pointers, decisions and brief. |
| [elements.rs](../crates/desegnos-core/src/elements.rs) | `Element` tree, kinds, styles, free/stack/grid layout, overrides, linked definitions, patches and moves. |
| [session.rs](../crates/desegnos-core/src/session.rs) | `Session`: revision checks, undo/redo, burst steps, export metadata outside undo, generation application and history labels. |
| [pointers.rs](../crates/desegnos-core/src/pointers.rs) | Pointer model, derived clash states and target descriptions. |
| [requirements.rs](../crates/desegnos-core/src/requirements.rs) | Frozen `RequirementsSnapshot`, capabilities, request markdown and `validate_current`. |
| [generation.rs](../crates/desegnos-core/src/generation.rs) | `GenerationCandidate`, validation, batches, images and `apply_generation_output`. |
| [request_history.rs](../crates/desegnos-core/src/request_history.rs) | Earlier-request index compaction and context. |
| [dossier.rs](../crates/desegnos-core/src/dossier.rs) | `GenerationDossier`, application status and recording. |
| [attachments.rs](../crates/desegnos-core/src/attachments.rs) | Bounded text, picture and link attachments on layers. |
| [identity.rs](../crates/desegnos-core/src/identity.rs), [identity_pages.rs](../crates/desegnos-core/src/identity_pages.rs) | `ImportedIdentity`, `ImportedContext`, evidence pages and `import_context`. |
| [project_assets.rs](../crates/desegnos-core/src/project_assets.rs) | `desegnos-asset:` references, the content-addressed picture store and `prepare_picture`. |
| [picture_scaling.rs](../crates/desegnos-core/src/picture_scaling.rs), [image_generation.rs](../crates/desegnos-core/src/image_generation.rs) | AI previews, display scaling and the image request type. |
| [provenance.rs](../crates/desegnos-core/src/provenance.rs) | Found-content address validation. |
| [website.rs](../crates/desegnos-core/src/website.rs) | `website_links`, `website_assets` and `css_urls`: page, asset and stylesheet references. |
| [website_import.rs](../crates/desegnos-core/src/website_import.rs) | Browser capture types and the pure conversion into pages, elements, script notes, behaviour pointers and linked-size overrides. |
| [export.rs](../crates/desegnos-core/src/export.rs) | Export plans, website/app bundles, pointer fingerprints and direct slide/image bundles. |
| [render.rs](../crates/desegnos-core/src/render.rs), [render/](../crates/desegnos-core/src/render/) | Direct PDF, PNG and SVG rendering from the element tree. |

## `desegnos-storage`

| Module | Owns |
| --- | --- |
| [lib.rs](../crates/desegnos-storage/src/lib.rs) | Module wiring, `load_project` and safe small-file writes. |
| [project_db.rs](../crates/desegnos-storage/src/project_db.rs) | `ProjectDatabase`: the `.desegnos` SQLite file, content-addressed versions, autosave, alternatives, history, restore, save conflicts and compaction. |
| [project_parts.rs](../crates/desegnos-storage/src/project_parts.rs) | `split_project`/`join_project`: one part per boundary, shared settings, reference, pointers, brief, identity, dossier and pictures. |
| [project_folder.rs](../crates/desegnos-storage/src/project_folder.rs) | Project-folder naming and one-project-file lookup. |
| [website_export.rs](../crates/desegnos-storage/src/website_export.rs) | Safe staged destination writing and receipts for exports. |
| [website_snapshot.rs](../crates/desegnos-storage/src/website_snapshot.rs) | Bounded folder/ZIP website snapshots, page routes and the private import folder. |
| [identity_source.rs](../crates/desegnos-storage/src/identity_source.rs) | Brand evidence collection from files, folders, archives and websites. |
| [request_zip.rs](../crates/desegnos-storage/src/request_zip.rs) | Bounded attachment ZIP extraction. |
| [system_tools.rs](../crates/desegnos-storage/src/system_tools.rs) | Executable discovery for external tools. |

## `desegnos-ai`

| Module | Owns |
| --- | --- |
| [providers.rs](../crates/desegnos-ai/src/providers.rs), [transport.rs](../crates/desegnos-ai/src/transport.rs) | Provider/model catalogues, `ProviderClient`, streaming, identity analysis and access checks. |
| [settings.rs](../crates/desegnos-ai/src/settings.rs), [credentials.rs](../crates/desegnos-ai/src/credentials.rs) | `ProviderSettings`, authentication, usage/cost estimates and masked key storage. |
| [error.rs](../crates/desegnos-ai/src/error.rs), [sse.rs](../crates/desegnos-ai/src/sse.rs) | Provider errors, submission outcomes, scrubbed diagnostics and the bounded SSE decoder. |
| [identity.rs](../crates/desegnos-ai/src/identity.rs), [website.rs](../crates/desegnos-ai/src/website.rs) | Brand budgets; `fetch_website`, `fetch_website_snapshot` and `fetch_website_image` under public-address, size and timeout rules. |
| [images.rs](../crates/desegnos-ai/src/images.rs), [image_generation.rs](../crates/desegnos-ai/src/image_generation.rs), [image_access.rs](../crates/desegnos-ai/src/image_access.rs) | Public image fetching/decoding, the image API and cached image access. |
| [chatgpt/](../crates/desegnos-ai/src/chatgpt/) | `ChatgptManager`: browser sign-in, accounts, token refresh, model discovery and sign-out. |

## Native shell: `src-tauri/src/`

| File | Owns |
| --- | --- |
| [main.rs](../src-tauri/src/main.rs) | Service initialization, command registry, plugins and window lifecycle. |
| [workspace.rs](../src-tauri/src/workspace.rs) | `Workspace`, `WorkspaceView`, open projects/tabs, untitled projects and their recovery databases, autosave, recovery, save conflicts, history/alternatives, folders and recents. |
| [generation.rs](../src-tauri/src/generation.rs) | `GenerationService`: prepare/submit/finish/cancel/retry, usage, job persistence, settings and the apply seam. |
| [website_import.rs](../src-tauri/src/website_import.rs) | Fetches or reads a website, serves the snapshot, resolves captured pictures into the project store and applies the import as one undo step. |
| [local_site.rs](../src-tauri/src/local_site.rs) | `LocalSite` and `ImportSite`: token-prefixed GET/HEAD static servers for previews and imported pages. |
| [identity.rs](../src-tauri/src/identity.rs) | `IdentityService`: brand source selection/fetching, analysis and approved context. |
| [export.rs](../src-tauri/src/export.rs) | `ExportService`: frozen-revision export, preview servers, pointer checks and open-in-browser. |
| [chatgpt.rs](../src-tauri/src/chatgpt.rs) | `ChatgptState`, account epochs and auth commands. |
| [project_location.rs](../src-tauri/src/project_location.rs), [recent.rs](../src-tauri/src/recent.rs) | App settings in `project-settings.json` (default project parent, show start page on launch) and the recent-project catalogue with pins and last-opened times. |
| [clipboard.rs](../src-tauri/src/clipboard.rs), [image_drop.rs](../src-tauri/src/image_drop.rs), [request_zip.rs](../src-tauri/src/request_zip.rs) | Clipboard, window image drops and attachment ZIP adapters. |
| [system_open.rs](../src-tauri/src/system_open.rs), [file_dialogs.rs](../src-tauri/src/file_dialogs.rs), [portal.rs](../src-tauri/src/portal.rs), [linux_startup.rs](../src-tauri/src/linux_startup.rs) | OS opening, dialogs/portal detection and Linux graphics startup. |
| [data_location.rs](../src-tauri/src/data_location.rs), [crash_report.rs](../src-tauri/src/crash_report.rs), [updates.rs](../src-tauri/src/updates.rs), [usage.rs](../src-tauri/src/usage.rs) | Data folder selection, crash reports, update checks and local usage logs. |

## Editor: `src/`

| Files | Owns |
| --- | --- |
| [App.tsx](../src/App.tsx) | Workspace queue, project tabs, menu actions, navigation surfaces, generation/export orchestration and global shortcut dispatch. |
| [ProjectCanvas.tsx](../src/ProjectCanvas.tsx), [projectCanvasGeometry.ts](../src/projectCanvasGeometry.ts) | Canvas surface: camera, drag/draw/resize, boundaries, frames, reference, pointers and elements; pure geometry helpers. |
| [projectCanvas/boundaries.tsx](../src/projectCanvas/boundaries.tsx) | Boundaries and page names. |
| [projectCanvas/elements.tsx](../src/projectCanvas/elements.tsx), [elementGeometry.ts](../src/projectCanvas/elementGeometry.ts) | Element rendering, selection overlay, inline text editing, creation and measurement. |
| [projectCanvas/frames.tsx](../src/projectCanvas/frames.tsx), [reference.tsx](../src/projectCanvas/reference.tsx), [pointers.tsx](../src/projectCanvas/pointers.tsx) | Frame boxes/request editor, reference items, pointer lines/comments/handles and automatic link lines. |
| [projectCanvas/elementProperties.tsx](../src/projectCanvas/elementProperties.tsx), [elementLayers.tsx](../src/projectCanvas/elementLayers.tsx) | Element Properties and the Layers tree. |
| [projectCanvas/sizes.ts](../src/projectCanvas/sizes.ts), [links.ts](../src/projectCanvas/links.ts), [play.ts](../src/projectCanvas/play.ts) | Linked-size resolution, linked elements and Play actions. |
| [projectCanvas/surface.ts](../src/projectCanvas/surface.ts), [linkedChoices.tsx](../src/projectCanvas/linkedChoices.tsx), [exportPanel.tsx](../src/projectCanvas/exportPanel.tsx) | Camera/wheel/Space pan, inline choices and the export panel. |
| [EditorDock.tsx](../src/EditorDock.tsx), [EditorTools.tsx](../src/EditorTools.tsx) | Right dock (Properties/Layers), the tool strip and their tooltips. |
| [shortcuts.ts](../src/shortcuts.ts) | The single keymap: actions, keys, display strings, matching and the generated keyboard help. |
| [WebsiteImport.tsx](../src/WebsiteImport.tsx), [websiteImportExtract.ts](../src/websiteImportExtract.ts) | Import dialog, hidden page rendering and the script-disabled extraction wrapper. |
| [ImportContext.tsx](../src/ImportContext.tsx), [identityReview.ts](../src/identityReview.ts) | Brand evidence review; approved context becomes brief decisions and project images. |
| [CodeEditor.tsx](../src/CodeEditor.tsx), [codeMirrorMetadata.ts](../src/codeMirrorMetadata.ts), [editorHtml.ts](../src/editorHtml.ts) | CodeMirror used by `code` elements. No page source editor exists. |
| [useGeneration.ts](../src/useGeneration.ts), [Generation.tsx](../src/Generation.tsx), [GenerationLog.tsx](../src/GenerationLog.tsx), [generationProblems.ts](../src/generationProblems.ts), [generationSession.ts](../src/generationSession.ts) | Job lifecycle/polling, feedback/details, request history and notices. |
| [AiSettings.tsx](../src/AiSettings.tsx) | Provider/model/key/sign-in settings and access checks. |
| [StartPage.tsx](../src/StartPage.tsx), [ProjectLocation.tsx](../src/ProjectLocation.tsx), [Onboarding.tsx](../src/Onboarding.tsx) | Home: Blank canvas and template tiles, recovery and recent projects with pin/remove. Default project location, first-use tips and the start-page launch setting. New projects are untitled tabs whose first save names them inline in the dock. |
| [Notes.tsx](../src/Notes.tsx), [StyleGuide.tsx](../src/StyleGuide.tsx) | Project brief/decisions/exceptions and the project style view. |
| [ProblemBanner.tsx](../src/ProblemBanner.tsx), [userErrors.ts](../src/userErrors.ts), [noticeSummary.ts](../src/noticeSummary.ts) | Notice queue/host, error translation and change summaries. |
| [RecoveryBanner.tsx](../src/RecoveryBanner.tsx), [SaveConflict.tsx](../src/SaveConflict.tsx) | Recovery choices and checked-save conflict resolution. |
| [WorkspaceSurface.tsx](../src/WorkspaceSurface.tsx), [Menus.tsx](../src/Menus.tsx), [visibleActions.ts](../src/visibleActions.ts) | Surfaces/focus, menus and opener visibility. |
| [HistoryPanel.tsx](../src/HistoryPanel.tsx), [useHistory.ts](../src/useHistory.ts), [DraftText.tsx](../src/DraftText.tsx), [deletePreference.ts](../src/deletePreference.ts), [pastePreference.ts](../src/pastePreference.ts), [usage.ts](../src/usage.ts), [CrashReport.tsx](../src/CrashReport.tsx) | History/alternatives, history labels, text drafts, preferences, usage and crash surfaces. |
| [types.ts](../src/types.ts) | TypeScript mirror of native payloads, including website captures and import views. |
| [styles.css](../src/styles.css), [brand.css](../src/brand.css), [shell.css](../src/shell.css), [canvas.css](../src/canvas.css), [ui.css](../src/ui.css), [dialogs.css](../src/dialogs.css), [notice.css](../src/notice.css), [startPage.css](../src/startPage.css) | Shell, canvas, dialogs, notices, Home and import presentation. |

## Key flows

### Editing canvas elements

1. [ProjectCanvas.tsx](../src/ProjectCanvas.tsx) turns pointer gestures into element commands.
2. [App.tsx](../src/App.tsx) queues one `Command` per gesture → [model.rs](../crates/desegnos-core/src/model.rs) `Project::apply` validates and bumps the revision → [Session](../crates/desegnos-core/src/session.rs) records the history step and the labels a saved version is named from.
3. Elements carry visual properties only. Pictures are referenced by id from the project store.
4. Undo/redo use the same session; restoring a saved version is stored as a new version and applied as one undo step.

### Frames and AI requests

1. A frame is a box with a pending request; any layer can carry a request too.
2. **Generate** → `prepare_generation` → [requirements.rs](../crates/desegnos-core/src/requirements.rs) freezes the task (brief, decisions, frames, elements, reference, pointers, pictures) → job persisted.
3. `start_generation`/`retry_generation` stream from the selected provider with cancellation, usage and retry limits. The reply is stored as a candidate.
4. Applying the candidate calls `apply_generation_output` and publishes elements as editable layers in one undo step.

### Project tabs, save and recovery

1. [workspace.rs](../src-tauri/src/workspace.rs) keeps the active `Session` plus parked projects, each with its own `ProjectDatabase`.
2. Launch shows Home unless **Menu → Preferences → Show start page on launch** is off, which opens a blank untitled canvas instead. Closing the last tab returns to Home. `new_untitled_project` adds an untitled tab named with the lowest unused `Canvas N`, with no boundaries or with a template's starter boundaries; `switch_project` and `close_project` change the active canvas without replacing the others.
3. An untitled project keeps its recovery database in the app data folder. Edits stay in memory; while a project is dirty they reach the hidden autosave branch every few seconds, so a crash keeps them on disk.
4. `save_untitled_project` asks for a name and file, writes one transaction and removes the recovery copy. `save_project_checked` does the same for projects that already have a file. Stamp conflicts resolve through [SaveConflict.tsx](../src/SaveConflict.tsx).
5. A newer autosave is offered through [RecoveryBanner.tsx](../src/RecoveryBanner.tsx) and the recovery list on launch. History and alternatives are shown in the dock by [HistoryPanel.tsx](../src/HistoryPanel.tsx).

### Import (brand evidence and website elements)

1. [ImportContext.tsx](../src/ImportContext.tsx) fetches a website, archive or folder through `pick_identity_source`/`fetch_identity_source` → [identity.rs](../src-tauri/src/identity.rs) and [identity_source.rs](../crates/desegnos-storage/src/identity_source.rs). Approved findings become brief decisions and pictures; pages stay evidence.
2. **Import website…** calls `prepare_website_import_url` or `pick_website_import_source` → [website_import.rs](../src-tauri/src/website_import.rs). Native fetches with [website.rs](../crates/desegnos-ai/src/website.rs) or reads with [website_snapshot.rs](../crates/desegnos-storage/src/website_snapshot.rs), writes a private temp folder and serves it through `ImportSite`.
3. [WebsiteImport.tsx](../src/WebsiteImport.tsx) renders each page in a hidden iframe through the extraction wrapper in [websiteImportExtract.ts](../src/websiteImportExtract.ts). The site page itself runs in a nested script-disabled frame.
4. Captures go to `import_website_pages`. Native downloads or reads pictures through the project picture store, then [website_import.rs](../crates/desegnos-core/src/website_import.rs) converts the tree into pages, elements, script notes and behaviour pointers. `Command::ImportWebsite` applies the whole import as one undo step.

### Export

1. **Export…** freezes an export job on a saved revision. Websites and app screens send one AI request after an explicit paid-send decision; slides and images render directly.
2. [export.rs](../crates/desegnos-core/src/export.rs) plans and fingerprints the output. [website_export.rs](../crates/desegnos-storage/src/website_export.rs) writes it safely.
3. The project records the export destination, page paths and pointer fingerprints. Recording an export is metadata, not an undo step.

### Keyboard

[shortcuts.ts](../src/shortcuts.ts) is the only keymap. The tool strip ([EditorTools.tsx](../src/EditorTools.tsx)), the menu ([App.tsx](../src/App.tsx)) and the keyboard help read its labels; canvas actions match its actions in [ProjectCanvas.tsx](../src/ProjectCanvas.tsx). Text fields keep their own editing keys.

## Scripts and documents

- [cargo.mjs](../scripts/cargo.mjs), [tauri.mjs](../scripts/tauri.mjs), [toolchain.mjs](../scripts/toolchain.mjs): tool discovery and Cargo/Tauri wrappers.
- [build-release.mjs](../scripts/build-release.mjs), [build-release.sh](../scripts/build-release.sh): local release orchestration.
- [usage-report.mjs](../scripts/usage-report.mjs), [startup-test.mjs](../scripts/startup-test.mjs), [test-vault.mjs](../scripts/test-vault.mjs): local diagnostics.
- [docs/](./): contracts and notes. Check their claims against source before changing behaviour.
