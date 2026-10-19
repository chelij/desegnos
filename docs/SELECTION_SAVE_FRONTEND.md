# Stable selection and safe save frontend

Implemented against `SELECTION_AND_SAVE_SPEC.md` and the native contracts from the stable-target and safe-save branches. No new dependency, sidebar, network generation or production app launch.

- Canvas selections use editor IDs. A positional selector and source fingerprint remain supported only for legacy targets. Shift, Control or Command click toggles a selection; contained selections reduce to their outer element. One request can target up to eight elements, with a union placement rectangle.
- Multiple selections expose Ask AI and Delete. Delete calls one atomic native command and therefore takes one project Undo. Multi-element drag, resize, cut, copy and arrange remain unavailable; their single-element keyboard actions cannot silently modify just one of several selected elements.
- Pending request bounds refresh through `refresh_element_target_bounds`. Existence is checked independently of visible bounds: hiding an element does not make its request stale. A missing ID shows “This element was removed”, with Pick again and Remove request. Retargeting preserves the request ID, prompt and linked brief rules in one command. AI root deletion needs the explicit request context-menu choice “Allow AI to remove selected items”.
- Page check and code navigation carry IDs and an expected page ID. A delayed page switch cannot select a matching positional path from the previous page. A failed lookup cannot turn an element-specific fix into an unrelated page-area request. Fragment HTML uses the exported page name as its title; only an explicitly empty title is flagged.
- Code view removes editor attributes from visible HTML by default. The native source map is projected into visible UTF-16 positions. The attributes remain tracked outside the textarea and are restored on save, using actual edit ranges so inserting similar tags does not move existing IDs to new elements. Text undo/redo can restore the corresponding metadata state. The Show editor attributes toggle is disabled while an HTML draft is unsaved. Invalid draft text remains in place with projected error positions.
- Browsing or dropping onto an image placeholder preserves authored safe attributes, including width and height, while replacing the placeholder and clearing old provenance. This uses the native validator for the final HTML.
- Save uses `save_project_checked` and `resolve_save_conflict`, including repeated conflicts if a disk file changes again. Changed-file conflicts default to Save as copy. Replacing another project through Save as defaults to Cancel. A file belonging to a different project during ordinary Save can only be copied to a new name. Revision and file hashes are only under Show details. Cancelling a choice or the file dialog keeps the recovery journal.
- Recovery rows show the saved file name and relative edit time, with the full path on hover.

## Verification

`node node_modules/typescript/bin/tsc --noEmit`, the production Vite build, all 37 existing canvas geometry tests, and six new `tests/selection-projection.test.mjs` regressions pass in the isolated selection-save-ui worktree. The new regressions cover stable/legacy identity, canonical selection and signed union geometry, hidden metadata across text/name/style edits, inserted/deleted siblings, quoted markup/raw text, and multiline UTF-16 source-map positions.

Native end-to-end acceptance is coordinated by the parent task against the integrated backend and frontend. This document does not claim it has run before that integration.
