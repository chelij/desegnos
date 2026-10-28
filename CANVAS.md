# Desegnos infinite canvas

Date: 2026-10-27. The canvas format starts from scratch: old project files are not supported. Concept for the infinite canvas; [DESIGN.md](DESIGN.md) covers the editor UI and [GLOSSARY.md](GLOSSARY.md) names everything. Where this file and [PRODUCT.md](PRODUCT.md) disagree, PRODUCT.md wins.

## Idea

One infinite canvas per project. Everything on it is visual: you place elements by hand or ask the AI to fill a request, and what you see is the design. Code exists only at export: the model compiles into website or app-screen HTML/CSS, and AI can supply missing pointer behaviour. Slides and images export directly. The bounds you draw decide what ships. Placement and control still win over AI convenience.

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

- A name is a label, not a link: naming a layer, or clearing its name, never links or detaches it. On a linked copy the name is the linked name every copy shares.
- **Make linked** (Ctrl/Cmd+Alt+K) is its own action, on the selection bar and in Properties. On one layer it makes that layer a linked element; on several it joins them under one linked name. It uses the layer's name when it has one, otherwise the next free name; when copies differ in content it asks which copy wins. Every copy is then one element, so editing one edits all of them, across boundaries and sizes. Each copy keeps its own position.
- Ctrl/Cmd+V shows a small inline choice at the paste point: Linked or Unlinked, with "Use as default". Once a default is set, Ctrl/Cmd+V uses it; Ctrl/Cmd+Shift+V always shows the choice.
- Unlink (Ctrl/Cmd+Alt+B) detaches a copy. Elements with different structure (different element kinds or different children) cannot be linked and say so inline.
- The same linked element can sit in many places on the canvas and be used in different ways around it.

## Pointers

- The comment is the spec. "Clicking a project opens it here" on a shelf → viewer pointer becomes behaviour.
- **A pointer is intent and permission.** It can target anything: an element, a boundary, a reference item or an external URL. The comment says what is intended. A pointer's kind decides what it may do: **reference** (the default) lets the AI read the target as context only, and **change** (solid line with an arrowhead) lets it read and change the target within the project. Reference pointers draw dashed and thin. External URLs and reference items can only be read. Without a pointer, a request never reaches outside its own layer and boundary. A pointer is not a live copy; linked elements are for that.
- The kind is a compact toggle in the comment editor; the line shows it, and generation and export read it wherever pointer permission is decided.
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
| Webpage | HTML, CSS, JS | Desegnos; AI for missing behaviour | Link, embed or behaviour |
| App screen | HTML, CSS, JS | Desegnos; AI for missing behaviour | Navigation |
| Slide | PDF (deck) | Desegnos, directly | Order and transition |
| Image | PNG or SVG | Desegnos, directly | Variant |

For websites and app screens, export compiles the resolved element tree and linked-size overrides into ordinary HTML/CSS. Missing change-pointer behaviours can be sent as frozen AI requests for marked JavaScript. Existing accepted behaviour can be reused. This path is implemented; responsive appearance and interaction quality still need the finished-example gate below. App-screen output is web files, not a native application package.

## Layout and sizes

- One canvas per project, saved in the project file.
- Element positions and sizes are exact on the canvas. At export the resolved tree and linked sizes supply the HTML/CSS layout.
- Each screen size is its own boundary (phone, tablet, desktop), linked to the others and laid out side by side. Linked sizes share their elements; each size can override position, size and style.
- Two editing modes on linked sizes, chosen with the **This size / All sizes** control on the selection bar and in the Properties header while the boundary has more than one linked size. **This size** edits that size only; **All sizes** edits every linked size together. The selection highlights in every size it affects. Double-click still enters a group or edits text.
- New elements are shared and visible in every linked size. Narrower sizes scale the horizontal position and shrink the width only when needed to fit. This size edits can hide an element in an individual size; hidden elements show a faint dashed placeholder until dismissed.
- Two kinds of boundary. **Screen** has a fixed size, like an app screen or one-screen page; content beyond it scrolls inside. **Page** has a fixed width and grows taller as elements are added.
- Boundaries never overlap. Moving or growing one pushes its neighbours aside, so every element belongs to exactly one boundary.
- Boundary **Background** uses the shared swatch and hex control, with white, off-white, light grey, dark grey, near-black and black presets. New boundaries start at `#ffffff`; new text and lines choose readable defaults from background luminance. The editor theme changes only the editor. Canvas and SVG/PNG/PDF/web exports use the boundary background.
- The toolbar **Export** panel lists every boundary with Include, name, Web/App/Slide/Image and web/app Preview controls. Click a name to select its boundary; Preview also works for excluded web/app boundaries. Folder and export/send controls follow the list.
- Any boundary can be deleted, including the first one drawn. The first remaining included boundary becomes the entry that export opens first and the editor focuses.

## Rendering, Play and Preview export

- The canvas draws elements directly. There are no snapshots of code and no per-page editor; everything is edited on the canvas.
- **Play** (Shift+Space on a boundary): the boundary becomes clickable and simple pointers run visually. In slide decks, Play presents one slide at a time, starting from the selected slide or the first; arrows, Space and Page Up/Down navigate. Escape stops.
- **Preview export**: exports the focused webpage/app-screen boundary in the background, requesting AI behaviour only when needed and runs the real result in place over the boundary, in a sandbox. It shows the last preview until the design changes; Escape returns to the design.
- Web exports set CSS `color-scheme` on `:root` and a matching meta tag. AI behaviour requests judge light/dark suitability from the boundary background and content colours; direct exports and invalid AI values use background luminance.
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
| Make linked / Unlink | Ctrl/Cmd+Alt+K / Ctrl/Cmd+Alt+B |
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
| Select | Select in this size | Enter group or edit text |
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

## Capability status

Reviewed 2026-10-28 against the current working tree. **Implemented** means a code path exists, with quality still unverified. **Verified** applies only to the checks stated in the cell, not to a finished output. **Not yet** means the capability is absent. Tests cited here were inspected, not rerun for this documentation update. No current execution record for the output-specific appearance gate was established, so no whole capability is marked Verified.

| Output type | Design | Interact (Play/pointers) | Export | Reopen |
| --- | --- | --- | --- | --- |
| Social image / Image | Implemented | Implemented (shared Play) | Implemented (PNG; eligible SVG) | Implemented |
| Slide deck | Implemented | Implemented (shared Play; presentation transitions: Not yet) | Implemented (PDF) | Implemented |
| Website | Implemented | Implemented | Implemented | Implemented |
| App screen | Implemented | Implemented | Implemented (web files; native app packaging: Not yet) | Implemented |

Design uses the shared element tree, properties, linked sizes and editing tools. [canvas_templates.rs](crates/desegnos-core/tests/canvas_templates.rs) checks starter geometry in `app_screen_and_social_image_are_single_fixed_screens`, `slide_deck_starts_with_three_slides_side_by_side` and `website_starts_with_linked_desktop_tablet_and_phone_sizes`; these are template checks, not design-quality proof. [elements_and_session.rs](crates/desegnos-core/tests/elements_and_session.rs) checks editing and undo with `insert_update_move_reparent_and_remove_are_one_undo_step_each`.

`slides_render_a_valid_pdf_and_images_render_png_and_svg` in [export.rs](crates/desegnos-core/tests/export.rs) asserts PNG signature/chunks, SVG dimensions, and a PDF header, page marker and EOF. These structural assertions do not prove appearance or a usable deck. No passing execution record for the current export changes was identified during this review.

Play implements go-to, show, hide, toggle and swap in [play.ts](src/projectCanvas/play.ts). That shared path does not prove slide presentation, exported interaction fidelity or usable app behaviour. Static PNG/SVG/PDF outputs do not carry Play interactions.

Reopen uses the shared `.desegnos` database. `split_and_join_round_trip_and_unchanged_parts_keep_their_hash` in [storage.rs](crates/desegnos-storage/tests/storage.rs) checks model round-trip equality; [canvas-smoke.mjs](tests/native/canvas-smoke.mjs) includes “Reopen the saved project from Home”. Neither establishes intended appearance for each output type. No complete edit → export → reopen appearance evidence for the three finished examples was identified in the reviewed sources.

## Next milestone: three finished examples

Deliver three editable `.desegnos` sample projects with their ordinary exported files. All three remain pending. The gate is intended appearance after a meaningful edit, export, closing and reopening the saved project, and a second export. Record the app revision, platform, provider/model if used, exact edit, output files, owner verdict and screenshots of canvas, reopened canvas and opened export. This is a recorded review of real examples; it does not require new test infrastructure.

| Example | Needs before it is finished |
| --- | --- |
| Social image | One 1080 × 1080 image boundary with final headline, supporting copy, supplied picture/logo and intentional typography, crop, spacing and colour. Edit text and move/resize an element; save and reopen with assets intact. Open PNG and eligible SVG independently; check dimensions, clipping, opacity, text and image appearance against the design. |
| Short slide deck | Three 1920 × 1080 slides: title, content and closing slide, with consistent type, colours and imagery. Edit copy and layout on one slide without changing the others. Save and reopen all three; open the PDF independently and check slide order, page dimensions, text, images and clipping. Record any unsupported transition rather than presenting it as shipped. |
| Responsive two-page website | Finished Home and About boundaries with linked desktop/tablet/390 px phone sizes, shared navigation, real copy and packaged assets. Exercise This size / All sizes edits and one pointer interaction; save and reopen with overrides and links intact. Export, open both pages in a browser, check local navigation, keyboard focus, pointer behaviour, content order and overflow at named and intermediate widths, then re-export after reopening. |

The [release plan](RELEASE_PLAN.md) uses these examples as the next preview gate. The [generation benchmark](docs/GENERATION_BENCHMARK.md) separately measures provider quality; file validity and build success do not satisfy either appearance gate.

### Sizing and shared styles

Every element has independent width and height modes: Fixed keeps its dimensions, Hug fits text or children, and Fill takes the available space. In a stack, Fill shares the remaining main-axis space and fills the cross axis; in a grid it fills its cell. In free layout it reaches the parent's edge from its offset. Optional minimum and maximum dimensions constrain every mode. These visual settings live in `ElementStyle.sizing`, so linked-size style overrides carry them too. Properties places the mode beside each dimension; Min / max reveals the optional limits.

A linked boundary's top-edge width scrubber previews any width between its narrowest and widest named sizes. The nearest named size at or below that width supplies the overrides, matching CSS min-width breakpoints. This preview changes no project data or history and returns to the named width on release or cancellation.

`Project.styles` contains named colour and text styles. A layer can reference colours for its fill, stroke and text, plus one text style for typography. References resolve after size overrides for canvas, image/slide rendering and website CSS. Style edits update every reference; deleting a style preserves each reference's current resolved appearance as local properties. Add, update and remove commands each form one undo step. Properties offers style selection and creation; the dock's Styles section edits names and values and deletes styles.
