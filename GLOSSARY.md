# Desegnos glossary

What each word means in Desegnos, and what it is called in the code. Concept detail is in [CANVAS.md](CANVAS.md); editor look and behaviour in [DESIGN.md](DESIGN.md). Terms marked *planned* are designed but not built yet; *rename* means the code still uses an older name.

## Canvas

| Term | Meaning | In code |
|---|---|---|
| **Canvas** | The one infinite surface per project where everything lives. Everything on it is visual. | `ProjectCanvas`, `src/projectCanvas/` |
| **Boundary** | A rectangle you draw that is one output: a page, slide, image or app screen. What is inside ships at export. Any boundary can be deleted; the first included one is the entry. | `Page` with `Page.canvas` |
| **Page boundary** | Fixed width, grows taller as elements are added. | `BoundaryKind::Page` |
| **Screen boundary** | Fixed size; content beyond it scrolls inside. | `BoundaryKind::Screen` |
| **Reference** | Everything outside every boundary. Material the AI can read; never shipped. | `Project.reference` |
| **Reference item** | One piece of reference: note, link card, image or swatch. | `ReferenceItem`, `ReferenceKind` |
| **Note** | Reference text. | `ReferenceKind::Note` |
| **Link card** | A pasted web address in reference. | `ReferenceKind::Link` |
| **Swatch** | A pasted colour in reference. | `ReferenceKind::Swatch` |
| **Dot grid** | The spreadsheet-like dots on the canvas bed; snapping follows them. | `dotGrid` |
| **Zoom rectangle** | Hold Z and drag a box to zoom the canvas to fit it; a Z click zooms one step at the pointer, Alt+Z+click zooms out. | `ZoomDrag`, `useCanvasCamera` |
| **Minimap** | Bottom-right overview of every boundary and reference item with the visible viewport on top. Click or drag to move the view; hides while everything fits; Shift+M toggles it. | `Minimap`, `src/projectCanvas/minimap.ts` |
| **Named view** | A saved camera position in the project file, listed in Layers. Ctrl/Cmd+Alt+1–9 saves to a slot; Alt+1–9 returns with a short glide. | `NamedView`, `Project.views`, `Command::AddNamedView`, `Command::UpdateNamedView`, `Command::RemoveNamedView` |

## Elements and layers

| Term | Meaning | In code |
|---|---|---|
| **Element** | A visual item that can have children: box, text, ellipse, line, image, button, input, video, embed, code block. Holds visual properties only, never HTML or CSS. | `Element`, `Page.elements` |
| **Box** | An element that holds children and lays them out freely, in a stack or in a grid. | `ElementKind::Box`, `Element.layout` |
| **Layer** | One element or a collection of elements (a group), with its own AI request history and context. | `Element`, `Element.meta` |
| **Group** | A layer made of several elements (Ctrl/Cmd+G). | `ElementKind::Group`, `group_elements` |
| **Request** (tool and box) | What you ask the AI for, aimed at targets you click with the Request tool (default Pick), an area you draw in a shape, or a selection (F), with text saying what to make or change. It targets whatever it covers or what was selected: layers, boundaries, reference items, pointers. | `PendingRequest`, `Project.requests`, `EditorTool` `request` |
| **Insert** | Adds any kind of element by hand (Ctrl/Cmd+I). | `INSERT_GROUPS`, `EditorTool` `insert` |
| **Name** | A layer's label, never a link. Make linked takes it for the linked name; on a copy it is the linked name every copy shares. | `Element.name`, `ElementPatch.name` |
| **Linked element** | A layer made linked whose copies are one element: edit one, all change. Each copy keeps its own position. | `Element.link`, `Project.linked`, `LinkedDefinition`, `resolve_element` |
| **Make linked / Unlink** | Make one layer linked, or join layers under one name / detach a copy (Ctrl/Cmd+Alt+K / B). | `Command::LinkElements` / `UnlinkElements`, `link_elements`, `unlink_elements` |
| **Paste choice** | Inline Linked / Unlinked choice on paste, with "Use as default". | `Command::PasteLinkedInstances`, `src/projectCanvas/linkedChoices.tsx`, `src/pastePreference.ts` |
| **Smart duplicate** | Ctrl/Cmd+D remembers the last copy's offset, so moving the copy and duplicating again repeats that offset until the selection changes. | `duplicateRepeat` in `ProjectCanvas` |
| **Style** | The visual properties a layer carries: fill, stroke, radius, opacity, shadow and text style. Clipboard and painter copy only these, never position, size or text. | `ElementStyle` |
| **Sizing mode** | Fixed dimensions, Hug content, or Fill available parent space, independently for width and height, with optional minimum and maximum limits. | `ElementStyle.sizing`, `SizingMode` |
| **Shared style** | A project-level named colour or text style referenced by layers across all outputs. Deleting it preserves the resolved local appearance. | `Project.styles`, `ElementStyle.references` |
| **Width scrubber** | A temporary linked-boundary width preview using the nearest smaller named size's overrides; release restores its named width. | `sizeAtWidth`, `ProjectCanvas` |
| **Format painter** | Copies one layer's style to another: Ctrl/Cmd+Alt+C then Ctrl/Cmd+Alt+V over a selection, or Shift+Ctrl/Cmd+Alt+C to paint the next clicked layer. | `copyStyle`, `pasteStyle`, `paintFormat` |

## AI

| Term | Meaning | In code |
|---|---|---|
| **Request text** | What you ask the AI to make or change, written in a request box or a layer's request box. | `PendingRequest.text` |
| **Request box** | Where request text is typed: on a request on the canvas, or under a layer in Properties. | `RequestBoxEditor`, `RequestProperties` |
| **Request shape** | The outline of a request area: rectangle, ellipse, polygon, freehand, or taken from a shape on the canvas (Shift+F). Coverage uses the real outline. | `RequestShape`, `RequestArea`, `src/projectCanvas/requestGeometry.ts` |
| **Targets** | What a request may change: what it covers or what was selected. Its scope. | `RequestTarget`, `PendingRequest.targets`, `scope_for` |
| **Request history** | Past requests for one layer, shown in Properties. | `ElementMeta.requests`, `RequestRecord` |
| **Context** | What travels with a request: attachments, selected reference items, pointers touching it. | `RequestAttachment` |
| **Generate** | Sends pending requests; the AI answers with elements, not code. | `GenerationCandidate`, `apply_generation_output` |
| **Ask AI** | A request on the selected layer (Ctrl/Cmd+K). | — |
| **Flexible / Precise** | How closely AI follows visual request descriptions and placement. Project setting; frozen into generation jobs. | `GenerationMode` `fuzzy` / `strict` |

## Pointers

| Term | Meaning | In code |
|---|---|---|
| **Pointer** | A line from a layer or boundary to something else. Intent and permission: the comment says what to do, the kind decides what it may read or change. | `Pointer` |
| **Pointer kind** | `reference` (default) reads the target as context only, drawn dashed and thin; `change` may read and change it, drawn solid with an arrowhead. | `Pointer.kind`, `PointerKind`, `pointer_grants_change` |
| **Comment** | The pointer's instruction, written on the line. It is the spec. | `Pointer.comment` |
| **Simple pointer** | Go to, show, hide, toggle or swap; runs visually in Play. | `Pointer.play` (`PlayAction`) |
| **Wired** | The last export built the pointer and its code still matches. Solid line. | `Pointer.wiring`, `ExportRecord.pointers`, `pointer_states_with_fingerprints` |
| **Needs exporting** | Comment is new or changed since the last export. Dashed line. | `PointerState::NeedsExporting`, `ExportRecord.pointers` |
| **Fingerprint** | Hash of the code an export made for a pointer, used to spot outside changes. | `ExportRecord.pointers`, `pointer_fingerprints`, `wiring.fingerprint` |
| **Link line** | Faint automatic line where an element links to another boundary. Not stored. | link lines in pointer geometry |

## Clashes

| Term | Meaning | Choices |
|---|---|---|
| **Clash** | Comment and exported code disagree, or two pointers do. Line turns amber. Never fixed silently. | — |
| **Code changed** | A pointer's exported code was edited some other way. | Keep comment / Keep code |
| **Contradicts** | Two pointers from one source want different things. | Keep this one / Merge |
| **Broken** | A pointer's source or target was deleted. | Reattach / Remove |

## Sizes

| Term | Meaning | In code |
|---|---|---|
| **Linked sizes** | Phone, tablet and desktop boundaries of one page, side by side, sharing elements with per-size overrides. | `PageSize`, `Page.sizes`, `ElementOverride`, `Element.overrides`, `resolve_for_size`, `src/projectCanvas/sizes.ts` |
| **Base size** | The first linked size; its element values are what the other sizes inherit. | `Page::base_size`, `baseSize` |
| **This size / All sizes** | The selection bar and Properties control that chooses which linked sizes an edit lands in. Double-click enters a group or edits text. | `ElementPatch.size_id` / `all_sizes`, `ElementTarget.sizeId`, `EditScopeChoice` |
| **Placeholder** | Faint dashed ghost of an element that exists in another size but not this one. | `ElementOverride.hidden` / `dismissed`, `element-placeholder`, `PlaceholderChoiceRow` |

## Running and output

| Term | Meaning | In code |
|---|---|---|
| **Play** | Makes a boundary clickable and runs simple pointers visually (Shift+Space). | `PlayState`, `playClick`, `src/projectCanvas/play.ts`, `Pointer.play` |
| **Preview export** | Exports the selected boundary in the background and runs the real result over it (Ctrl/Cmd+Enter). | `prepare_export` with `preview`, `LocalSite`, `preview-overlay` |
| **Export** | Websites and app screens: the model compiles HTML/CSS, with AI for missing pointer behaviour in JavaScript. Slides: PDF. Images: PNG or eligible SVG. App screens export web files. (Ctrl/Cmd+E) | `ExportPlan`, `plan_export`, `ai_export_bundle`, `website_export.rs`, `ExportRecord` |
| **Open in browser** | Serves the last export for a final check in a real browser. | `LocalSite::serve`, `export::open_in_browser` |
| **Import** | Converts an existing website into boundaries, layers and elements. | `website_import.rs` |

## Saving

| Term | Meaning | In code |
|---|---|---|
| **Project file** | One `.desegnos` SQLite file holding the design and its history. | `ProjectDatabase`, `crates/desegnos-storage/src/project_db.rs` |
| **Version** | A saved state of the project, made automatically on save and labelled from its undo steps. | `versions` table, `VersionSummary`, `project_parts.rs` |
| **History** | The list of versions; restoring one is a single undo step. | `ProjectHistoryView`, `src/HistoryPanel.tsx`, `Session::restore` |
| **Alternative** | A named branch of versions to switch between. | `branches` table, `BranchSummary`, `create_alternative` |

## Editor

| Term | Meaning |
|---|---|
| **Tool strip** | Left edge: Select V, Request F, Boundary B, Pointer P, Text T, Shape R/O/L, Image, Note N, Insert, Pan H. |
| **Dock** | Right edge: Properties above Layers. Collapses to icons; Tab hides all panels. |
| **Properties** | Settings of the selection: layer settings, then request box, history and context. |
| **Layers** | Boundaries with their layers, then Reference, then Pointers. |
| **Project brief** | Project-wide decisions, identity and rules the AI follows. |
| **Import context** | Reads a website, archive or folder as untrusted reference for brand. |

## Gone

| Term | Why |
|---|---|
| **Moodboard** | Renamed Reference. |
| **Frame** | Renamed Request. |
| **Open in editor**, per-page editor | Everything is edited visually on the canvas. |
| **Snapshot**, live boundary | The canvas draws elements, not code. Play and Preview export replace running in place. |
| **Design / Split / Code** | No code exists before export. |
