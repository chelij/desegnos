# Desegnos infinite canvas

Date: 2026-10-27. Draft concept, not yet decided. Where this file and [PRODUCT.md](PRODUCT.md) or [DESIGN.md](DESIGN.md) disagree, those win until this draft is accepted.

## Idea

One infinite canvas per project replaces per-page workspaces, page tabs, Preview and Live. You draw what you want; the bounds you draw decide what ships. Placement and control still win over AI convenience.

## Primitives

| Primitive | User draws | Becomes |
|---|---|---|
| **Frame** | A rectangle | A region of a page (`data-desegnos-region`) |
| **Boundary** | A rectangle around frames | An output: a page, slide, image or app screen, with its own size and type |
| **Pointer** | A line from a frame to something else | Behaviour or reference, described by its comment |
| **Moodboard item** | Anything dropped outside every boundary | Reference the AI can read; never shipped |

Today's request text boxes become frames: a frame holds a description until it is generated, then holds the generated content.

## Bounds decide what ships

- Inside a boundary: real output.
- Outside every boundary: moodboard (images, pasted URLs, notes, swatches).
- Dragging between the two converts. Moodboard image into a frame becomes content; a section dragged out becomes reference.
- Selecting a boundary plus moodboard items and writing a request sends those items as context. No attach step.

## Pointers

- The comment is the spec. "Clicking a project opens it here" on a shelf → frame pointer is compiled by the AI into the wiring (today's `target="project-view"`).
- No comment: the frame shows the target's content.
- Targets: another boundary, an external URL (lands as a reference card), or a moodboard item ("match this lighting").
- A pointer into the moodboard affects only the frame it starts from; point from the boundary to affect the whole output.
- The comment is written on the line itself. Zoomed out, it collapses to an icon.
- Links inside generated pages (a nav link to `/about`) draw their own lines; nobody draws those by hand.
- **A pointer is intent and permission.** It can target anything, including a single frame of another page. The comment says what is intended; the line grants the AI full permission to read and change the target, within the project. External URLs can only be read. Requests stay scoped: without a pointer, a request never reaches outside its own boundary. A pointer is not a live copy, so there is no component syncing.

## Clashes

The canvas never resolves disagreements silently. A clash turns the affected lines amber; clicking one offers two choices in place, no dialog.

| Clash | Choices |
|---|---|
| Code behind a pointer changed outside it (hand edit, another request) | **Keep comment** (regenerate code) or **Keep code** (rewrite the comment to match) |
| Two pointers contradict | Pick one, or merge |
| Target deleted or moved out of its boundary | Reattach or remove |

Desegnos stores a fingerprint of each pointer's generated code to detect the first case.

## Output types

| Boundary type | Exports as | Pointer between boundaries means |
|---|---|---|
| Webpage | HTML | Link or embed |
| Slide | PDF (deck) | Order and transition |
| Image | PNG or SVG | Variant |
| App screen | HTML | Navigation |

## Layout and sizes

- One canvas per project, saved in its own file beside the site.
- Frame positions are placement intent, as today. The AI turns them into responsive grid or flex layout; Flexible/Precise still applies.
- Each screen size is its own boundary (phone, tablet, desktop), linked to the others and laid out side by side.
- Two editing modes on linked sizes. **Single click** edits this size only. **Double click** edits all linked sizes together. The canvas always shows what is being edited and where: the selection highlights in every size it affects.
- When a frame is added to one size only, the other linked sizes show a faint dashed placeholder where it would go. Click it to include the frame in that size, or dismiss it to hide it there.
- Pointers can also connect linked sizes of the same page, e.g. phone menu → desktop nav: "collapse this into a hamburger here".
- Two kinds of boundary. **Screen** has a fixed size, like an app screen or one-screen page; content beyond it scrolls inside. **Page** has a fixed width and grows taller as frames are added.
- Boundaries never overlap. Moving or growing one pushes its neighbours aside, so every frame belongs to exactly one boundary.

## Tools

The active tool decides what a click does, as in Photoshop. Each tool has its own cursor, a button in the floating canvas toolbar and a single-key shortcut.

| Tool | Click | Double click |
|---|---|---|
| Select | Select in this size | Select in all linked sizes |
| Text | Edit text in place, in the size of the current selection mode | — |
| Frame | Draw a frame | — |
| Boundary | Draw a screen or page boundary | — |
| Pointer | Drag from a frame to its target | — |
| Pan | Drag the canvas | — |

## Rendering

The canvas is the preview. Every boundary is drawn from its real output.

- Zoomed out and unfocused: frozen snapshots (existing frame renderer). Pan and zoom move images, not iframes.
- Focused boundary: runs live in place, including pointer frames inside it. No separate Preview or Live screen.
- Snapshots re-render in the background when shared styles change, at more than one resolution so zoomed text stays sharp.
- Open in browser stays as the final real-browser check of the exported site.

## Example: chelib

- Boundary `index` holds frames for the chat background, portrait, project shelf and project viewer.
- Each project page (solopong, papan, sheets-ribbon…) is its own boundary nearby.
- Pointer shelf → viewer: "clicking a project opens it here".
- Moodboard beside it: reference sites and images the AI can draw from.

## Build plan

Adapt, don't restart. Keep the Rust backend (AI, storage, import, export, local site, preview bridge), the frame renderer (`frozenPreview`, `liveFrame`, `previewFrames`, `frameDocuments`, `frameAdoption`), element editing and the plain-HTML site format. Rewrite `Canvas.tsx`, `canvasGeometry.ts` and the `App.tsx` shell. The canvas layout lives in its own file beside the site.

1. Canvas surface: pan, zoom, existing pages shown as boundaries from snapshots.
2. Draw frames and boundaries; focus to run live.
3. Moodboard items.
4. Pointers with comments, then clash detection.
