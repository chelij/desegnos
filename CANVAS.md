# Desegnos infinite canvas

Date: 2026-10-27. The canvas format starts from scratch: old project files are not supported. Concept for the infinite canvas; [DESIGN.md](DESIGN.md) covers the editor UI and [GLOSSARY.md](GLOSSARY.md) names everything. Where this file and [PRODUCT.md](PRODUCT.md) disagree, PRODUCT.md wins.

## Idea

One infinite canvas per project. Everything on it is visual: you place elements by hand or ask the AI to fill a request, and what you see is the design. Code exists only at export, when the AI does the heavy lifting of turning the design into a real website or app. Slides and images export directly. The bounds you draw decide what ships. Placement and control still win over AI convenience.

## Primitives

| Primitive | User draws | Becomes |
|---|---|---|
| **Element** | Text, box, shape, line, image, control… | A visual item that can have children |
| **Layer** | — | One element, or a collection of elements, with its own AI request history and context |
| **Request** | A box with what you want in it | A box element the AI fills with visual elements |
| **Boundary** | A rectangle on empty canvas | An output: a page, slide, image or app screen, with its own size and type |
| **Pointer** | A line from an element or boundary to something else | Behaviour or reference, described by its comment |
| **Reference** | Anything placed outside every boundary | Material the AI can read; never shipped |

## Bounds decide what ships

- Inside a boundary: the design that ships at export.
- Outside every boundary: reference (images, pasted URLs, notes, swatches).
- Pasting from another app follows the same rule: text or an image pasted inside a boundary becomes a layer, and outside every boundary it becomes reference.
- Dropping files from the file manager follows the same rule: pictures and text become layers inside a boundary or reference outside it, and other accepted files attach to a request under the pointer.
- Dragging between the two converts. A reference image dragged into a boundary becomes an image element; an element dragged out becomes reference.
- Selecting elements or a boundary plus reference items and writing a request sends those items as context. No attach step.

## Elements and layers

- An element is any visual item that can have children: box, text, ellipse, line, image, button, input and other controls, video, embed, and code blocks carried into the export. Boxes hold children and lay them out freely, in a stack (row or column, gap, padding) or in a grid.
- Elements carry visual properties only: position, size, fill, stroke, radius, shadow, opacity, text style, image, layout. No HTML, CSS or JavaScript lives on the canvas.
- A layer is one element or a collection of elements (a group). Each layer carries its AI request history and context (attachments, reference items, pointers touching it), shown in Properties under the layer's own settings.
- Users add elements by hand; the AI request box is the fallback for anything they would rather describe. Common elements sit in the tool strip; Insert lists every kind.
- The Request tool works like a pointer by default: the cursor is an arrow, clicking anything targets it (Shift-click adds more, click again removes), and a request box appears beside the targets to type in. Enter sends it to the pending requests; Escape cancels.
- A request can target anything on the canvas, two ways. **By area** (choose a shape in the tool's flyout): draw a request anywhere, over anything; whatever it covers (layers, part of a boundary, whole boundaries, reference items, pointers) is what the AI may change. A request over empty space inside a boundary makes new content there; over empty space outside every boundary it makes new reference. **By selection:** select any mix of things and press F (or Ctrl/Cmd+K); the request targets exactly the selection.
- A request area can be any shape. The Request tool's flyout offers Pick (the default), then Rectangle, Ellipse, Polygon and Freehand (lasso); coverage uses the real outline, not its bounding box. Any shape already on the canvas can define one: select a shape and press Shift+F to turn its outline into a request area (the shape itself stays).
- A request's targets are its scope: the AI changes only those, plus what pointers from them grant. Requests show their targets highlighted while selected, and the AI's result replaces the request box with the edited or new layers; the request text moves into those layers' history.

## Linked elements

- Any element can be given a name, which makes it a linked element: every copy with that name is one element, so editing one edits all of them, across boundaries and sizes. Each copy keeps its own position.
- Ctrl/Cmd+V shows a small inline choice at the paste point: Linked or Unlinked, with "Use as default". Once a default is set, Ctrl/Cmd+V uses it; Ctrl/Cmd+Shift+V always shows the choice.
- Link (Ctrl/Cmd+Alt+K) joins selected unlinked elements under one name; Unlink (Ctrl/Cmd+Alt+B) detaches a copy. Elements with different structure (different element kinds or different children) cannot be linked and say so inline; when only text, images or styles differ, the user picks which copy wins.
- The same linked element can sit in many places on the canvas and be used in different ways around it.

## Pointers

- The comment is the spec. "Clicking a project opens it here" on a shelf → viewer pointer becomes behaviour.
- **A pointer is intent and permission.** It can target anything: an element, a boundary, a reference item or an external URL. The comment says what is intended; the line grants the AI full permission to read and change the target, within the project. External URLs can only be read. Without a pointer, a request never reaches outside its own layer and boundary. A pointer is not a live copy; linked elements are for that.
- Simple pointers work visually on the canvas in Play: go to a boundary, show, hide, toggle, swap content. The AI recognises these from the comment. Everything else compiles at export.
- The comment is written on the line. Zoomed out, it collapses to an icon.
- Pointers can connect linked sizes of the same page, e.g. phone menu → desktop nav: "collapse this into a hamburger here".
- Link lines: when a text or button element links to another boundary, a faint line draws itself; nobody draws those by hand.

## Clashes

The canvas never resolves disagreements silently. A clash turns the affected lines amber; clicking one offers the choices in place, no dialog.

| Clash | Choices |
|---|---|
| A pointer's exported code was changed outside it (hand edit of the exported files, another request) | **Keep comment** (rebuild at next export) or **Keep code** (rewrite the comment to match) |
| Two pointers contradict | **Keep this one** or **Merge** |
| Source or target deleted | **Reattach** or **Remove** |

Each export records a fingerprint of the code it made for every pointer. A pointer whose comment changed since the last export is not a clash; it simply needs exporting (dashed line).

## Output types

| Boundary type | Exports as | Who builds it | Pointer between boundaries means |
|---|---|---|---|
| Webpage | HTML, CSS, JS | AI | Link, embed or behaviour |
| App screen | HTML, CSS, JS | AI | Navigation |
| Slide | PDF (deck) | Desegnos, directly | Order and transition |
| Image | PNG or SVG | Desegnos, directly | Variant |

For websites and apps, export sends the visual design (element tree with positions, sizes and styles), pointers, references and the project brief to the AI, which writes real, responsive, ordinary files. Flexible/Precise decides how closely layout follows positions.

## Layout and sizes

- One canvas per project, saved in the project file.
- Element positions and sizes are exact on the canvas. At export they are placement intent: the AI turns them into responsive layout.
- Each screen size is its own boundary (phone, tablet, desktop), linked to the others and laid out side by side. Linked sizes share their elements; each size can override position, size and style.
- Two editing modes on linked sizes. **Single click** edits this size only. **Double click** edits all linked sizes together. The selection highlights in every size it affects.
- When an element is added to one size only, the other linked sizes show a faint dashed placeholder where it would go. Click it to include the element in that size, or dismiss it to hide it there.
- Two kinds of boundary. **Screen** has a fixed size, like an app screen or one-screen page; content beyond it scrolls inside. **Page** has a fixed width and grows taller as elements are added.
- Boundaries never overlap. Moving or growing one pushes its neighbours aside, so every element belongs to exactly one boundary.
- Any boundary can be deleted, including the first one drawn. The first remaining included boundary becomes the entry that export opens first and the editor focuses.

## Rendering, Play and Preview export

- The canvas draws elements directly. There are no snapshots of code and no per-page editor; everything is edited on the canvas.
- **Play** (Shift+Space on a boundary): the boundary becomes clickable and simple pointers run visually. Escape stops.
- **Preview export**: exports the focused boundary in the background with the AI and runs the real result in place over the boundary, in a sandbox. It shows the last preview until the design changes; Escape returns to the design.
- **Open in browser** serves the last export for a final check in a real browser.
- **Minimap**: a small overview in the bottom-right corner draws every boundary and reference item with the visible viewport on top; click or drag it to move the view. It hides while the whole canvas fits on screen; Shift+M shows or hides it.
- **Named views**: Layers lists saved cameras. Click one to glide back to it, double-click to rename it, and use '+' to save the current camera. Ctrl/Cmd+Alt+1–9 saves the current camera to that slot; Alt+1–9 returns to it.

## Import

Importing a website (address, folder, archive) renders it and converts it into elements and layers on the canvas: one boundary per page, boxes for containers, text, images and controls with their computed positions and styles. Scripts and behaviour come across as pointers with comments where they can be recognised, otherwise as reference notes.

## Saving and versions

- A project is one `.desegnos` file: a SQLite database holding the current design and its whole history. Exports stay ordinary folders and record the version they came from.
- Content-addressed: boundaries, reference, pointers, the brief and pictures are stored as blobs by hash, so unchanged parts are stored once across all versions. A version is a manifest of hashes with a parent, time and label.
- Every save is one transaction (write-ahead logging), so a crash never leaves a half-written project. Saves create versions automatically, labelled from their undo steps; saves seconds apart are combined.
- History (top bar) lists versions; restoring one is a single undo step. Alternatives are named branches of versions to switch between, e.g. "dark version".
- Save a copy without history writes only the current version; Compact history thins out old versions.

## Keyboard

Every tool and action has a shortcut, using the keys other design tools use.

| Action | Keys |
|---|---|
| Select / Request / Boundary / Pointer / Text / Pan | V / F / B / P / T / H (hold Space to pan) |
| Rectangle / Ellipse / Line / Image | R / O / L / Shift+Ctrl/Cmd+K |
| Note (reference) | N |
| Request on selection / request from a shape's outline | F / Shift+F |
| Insert any element | Ctrl/Cmd+I |
| Copy / Paste / Paste with choice / Duplicate | Ctrl/Cmd+C / V / Shift+V / D |
| Copy style / Paste style / Paint format | Ctrl/Cmd+Alt+C / Ctrl/Cmd+Alt+V / Shift+Ctrl/Cmd+Alt+C |
| Link / Unlink | Ctrl/Cmd+Alt+K / Ctrl/Cmd+Alt+B |
| Group / Ungroup | Ctrl/Cmd+G / Shift+Ctrl/Cmd+G |
| Bring forward / backward | Ctrl/Cmd+] / [ |
| Select all / child / parent | Ctrl/Cmd+A / Enter / Shift+Enter |
| Nudge / far nudge | Arrows / Shift+Arrows |
| Delete | Delete or Backspace |
| Zoom in / out / 100% / fit all / fit selection | Ctrl/Cmd+= / - / 0, Shift+1, Shift+2 |
| Save view / go to view (slots 1–9) | Ctrl/Cmd+Alt+1–9 / Alt+1–9 |
| Minimap | Shift+M |
| Play / Preview export | Shift+Space / Ctrl/Cmd+Enter |
| Hide panels | Tab |
| Ask AI on selection | Ctrl/Cmd+K |
| Export | Ctrl/Cmd+E |
| Undo / Redo | Ctrl/Cmd+Z / Shift+Ctrl/Cmd+Z |

Transform keys follow the conventions of Figma, Photoshop and Excel:

- Shift keeps a layer's ratio while resizing (images keep it by default and Shift frees it); Alt resizes from the centre; both combine. Shift snaps a drawn line's angle to 15° steps.
- Shift draws a square or circle while drawing a rectangle, ellipse, boundary or request area; Alt draws it from the centre.
- Shift locks a layer drag (and a reference drag) to the dominant axis.

Holding Z turns the pointer into a zoom tool: drag a rectangle to zoom to fit it, or click to zoom one step at the pointer (Alt+Z+click zooms out). Middle-mouse drag pans from any tool without changing the selection. Escape cancels the armed format painter.

Ctrl/Cmd+D duplicates and remembers the offset: move the copy, press Ctrl/Cmd+D again, and each new copy repeats the same offset until the selection changes. An Alt-drag duplicate followed by Ctrl/Cmd+D repeats that drag's offset too. Ctrl/Cmd+Alt+C copies the selected layer's style and Ctrl/Cmd+Alt+V pastes it over the selection (one undo step); Shift+Ctrl/Cmd+Alt+C arms the painter so the next clicked layer receives the style.

## Tools

The active tool decides what a click does, as in Photoshop. Each tool has its own cursor, a button in the tool strip and a single-key shortcut.

| Tool | Click or drag | Double click |
|---|---|---|
| Select | Select in this size | Select in all linked sizes |
| Text | Place or edit text | — |
| Rectangle / Ellipse / Line | Draw the shape | — |
| Image | Place a picture | — |
| Request | Pick targets (default), or draw a request area in the flyout's shape; then type what to make or change | — |
| Boundary | Draw a Page or Screen boundary | — |
| Pointer | Drag from an element or boundary to its target | — |
| Note | Place a reference note | — |
| Pan | Drag the canvas (or middle-mouse drag from any tool) | — |

## Example: chelib

- Boundary `index` holds layers for the chat background, portrait, project shelf and project viewer.
- Each project page (solopong, papan, sheets-ribbon…) is its own boundary nearby.
- Pointer shelf → viewer: "clicking a project opens it here". It works in Play by swapping the viewer's content, and becomes real code at export.
- Reference beside it: sites and images the AI can draw from.

## Build plan

Progress checklist for the `infinite-canvas` branch, kept current as work lands.

- [x] Canvas surface: pan, zoom, dot grid
- [x] Boundaries: draw, move, push apart, Page/Screen, saved
- [x] Tool strip and Properties/Layers dock
- [x] Frames, reference items, pointers with comments, link lines, clash UI
- [x] Specs updated for visual-first (CANVAS, DESIGN, GLOSSARY, PRODUCT)
- [x] Element model and renderer: element tree per boundary, drawn on the canvas; Text, shapes, Image, Note, Insert; select, move, resize, group, copy/paste; Layers tree; Properties with request box, history and context
- [x] Remove the per-page editor, snapshot pipeline and HTML-editing core
- [x] Saving and versions: `.desegnos` SQLite file, content-addressed versions, History, alternatives
- [x] AI fills frames and changes layers with elements; Reference rename in code
- [x] Linked elements and the paste choice
- [x] Linked sizes with overrides, single and double click modes, placeholders
- [x] Export: AI builds websites and apps; slides and images render directly; pointer fingerprints and export clashes
- [x] Preview export and Play
- [x] Import converts websites into elements
- [x] Full keyboard pass
- [x] Test and fix everything once
- [x] Release build and startup check (hands-on canvas check pending)
