# Desegnos glossary

What each word means in Desegnos, and what it is called in the code. Concept detail is in [CANVAS.md](CANVAS.md); editor look and behaviour in [DESIGN.md](DESIGN.md). Terms marked *planned* are designed but not built yet; *rename* means the code still uses an older name.

## Canvas

| Term | Meaning | In code |
|---|---|---|
| **Canvas** | The one infinite surface per project where everything lives. Everything on it is visual. | `ProjectCanvas`, `src/projectCanvas/` |
| **Boundary** | A rectangle you draw that is one output: a page, slide, image or app screen. What is inside ships at export. | `Page` with `Page.canvas` |
| **Page boundary** | Fixed width, grows taller as elements are added. | `BoundaryKind::Page` |
| **Screen boundary** | Fixed size; content beyond it scrolls inside. | `BoundaryKind::Screen` |
| **Reference** | Everything outside every boundary. Material the AI can read; never shipped. | `Project.reference` |
| **Reference item** | One piece of reference: note, link card, image or swatch. | `ReferenceItem`, `ReferenceKind` |
| **Note** | Reference text. | `ReferenceKind::Note` |
| **Link card** | A pasted web address in reference. | `ReferenceKind::Link` |
| **Swatch** | A pasted colour in reference. | `ReferenceKind::Swatch` |
| **Dot grid** | The spreadsheet-like dots on the canvas bed; snapping follows them. | `dotGrid` |

## Elements and layers

| Term | Meaning | In code |
|---|---|---|
| **Element** | A visual item that can have children: box, text, ellipse, line, image, button, input, video, embed, code block. Holds visual properties only, never HTML or CSS. | `Element`, `Page.elements` |
| **Box** | An element that holds children and lays them out freely, in a stack or in a grid. | `ElementKind::Box`, `Element.layout` |
| **Layer** | One element or a collection of elements (a group), with its own AI request history and context. | `Element`, `Element.meta` |
| **Group** | A layer made of several elements (Ctrl/Cmd+G). | `ElementKind::Group`, `group_elements` |
| **Frame** | A box drawn with a request; the AI fills it with elements. | `ElementKind::Box`, `ElementMeta.pending` |
| **Insert** | Adds any kind of element by hand (Ctrl/Cmd+I). | `INSERT_GROUPS`, `EditorTool` `insert` |
| **Linked element** | A named element whose copies are one element: edit one, all change. Each copy keeps its own position. | `Element.link`, `Project.linked`, `LinkedDefinition`, `resolve_element` |
| **Link / Unlink** | Join elements under one name / detach a copy (Ctrl/Cmd+Alt+K / B). | `Command::LinkElements` / `UnlinkElements`, `link_elements`, `unlink_elements` |
| **Paste choice** | Inline Linked / Unlinked choice on paste, with "Use as default". | `Command::PasteLinkedInstances`, `src/projectCanvas/linkedChoices.tsx`, `src/pastePreference.ts` |

## AI

| Term | Meaning | In code |
|---|---|---|
| **Request** | What you ask the AI to make or change, written in a frame or a layer's request box. | `PendingRequest`, `ElementMeta.pending` |
| **Request box** | Where a request is typed: on a frame, or under a layer in Properties. | — |
| **Request history** | Past requests for one layer, shown in Properties. | `ElementMeta.requests`, `RequestRecord` |
| **Context** | What travels with a request: attachments, selected reference items, pointers touching it. | `RequestAttachment` |
| **Generate** | Sends pending requests; the AI answers with elements, not code. | `GenerationCandidate`, `apply_generation_output` |
| **Ask AI** | A request on the selected layer (Ctrl/Cmd+K). | — |
| **Flexible / Precise** | How closely the export follows positions. Project setting. | `GenerationMode` `fuzzy` / `strict` |

## Pointers

| Term | Meaning | In code |
|---|---|---|
| **Pointer** | A line from a layer or boundary to something else. Intent and permission: the comment says what to do, the line lets the AI read and change the target within the project. | `Pointer` |
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
| **Single click / double click** | With Select: edit this size only / all linked sizes. | `ElementPatch.size_id` / `all_sizes`, `ElementTarget.sizeId`, `elementTargetAt`, `doubleClick` |
| **Placeholder** | Faint dashed ghost of an element that exists in another size but not this one. | `ElementOverride.hidden` / `dismissed`, `element-placeholder`, `PlaceholderChoiceRow` |

## Running and output

| Term | Meaning | In code |
|---|---|---|
| **Play** | Makes a boundary clickable and runs simple pointers visually (Shift+Space). | `PlayState`, `playClick`, `src/projectCanvas/play.ts`, `Pointer.play` |
| **Preview export** | Exports the selected boundary in the background and runs the real result over it (Ctrl/Cmd+Enter). | `prepare_export` with `preview`, `LocalSite`, `preview-overlay` |
| **Export** | Websites and apps: the AI builds real HTML, CSS and JS from the design. Slides: PDF. Images: PNG or SVG. (Ctrl/Cmd+E) | `ExportPlan`, `plan_export`, `ai_export_bundle`, `website_export.rs`, `ExportRecord` |
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
| **Tool strip** | Left edge: Select V, Frame F, Boundary B, Pointer P, Text T, Shape R/O/L, Image, Note N, Insert, Pan H. |
| **Dock** | Right edge: Properties above Layers. Collapses to icons; Tab hides all panels. |
| **Properties** | Settings of the selection: layer settings, then request box, history and context. |
| **Layers** | Boundaries with their layers, then Reference, then Pointers. |
| **Project brief** | Project-wide decisions, identity and rules the AI follows. |
| **Import context** | Reads a website, archive or folder as untrusted reference for brand. |

## Gone

| Term | Why |
|---|---|
| **Moodboard** | Renamed Reference. |
| **Open in editor**, per-page editor | Everything is edited visually on the canvas. |
| **Snapshot**, live boundary | The canvas draws elements, not code. Play and Preview export replace running in place. |
| **Design / Split / Code** | No code exists before export. |
