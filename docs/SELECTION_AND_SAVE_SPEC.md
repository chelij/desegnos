# Selection targets and safe save spec

Implementation status, 20 October 2026: completed and integrated in WebMaker. Current projects use schema 7 and new requirements use format 10; the starting-state table and earlier frontend handoff notes below describe the state before this work. See [implementation and verification](SELECTION_SAVE_CODEX.md) and [the current backend contract](CLEANUP_BACKEND_CONTRACT.md).

Date: 2026-10-20. Source: comparison with codex-openpencil (README only, not installed), checked against the current code. Two ideas carry over: selections that stay attached to their elements, and saves that never quietly overwrite other work. PRODUCT.md and CLEANUP_SPEC.md still set direction; this file covers only these two features.

## What exists today

| Area | Current behavior | Where |
| --- | --- | --- |
| Ask AI target | One element per request, stored as `ElementTarget { selector, tag, label, sourceSha256 }`. The selector is a positional `tag:nth-of-type(n)` path. | `src/types.ts`, `crates/desegni-core/src/editing.rs` `locate` |
| Scope check | `validate_scope` (format 8+) removes the targeted nodes and rejects the result if anything else in the body changed. | `crates/desegni-core/src/generation_scope.rs` |
| Duplicate requests | `pickElement` reuses an open request only when both the selector and `sourceSha256` match. | `src/App.tsx` `pickElement` |
| Recovery | A per-project recovery journal of full snapshots. **Recover unsaved projects** lists them on start and in the Menu. | `crates/desegni-storage/src/lib.rs`, `src-tauri/src/workspace.rs` |
| Save | Atomic write with a backup. Before overwriting, it only checks that the existing file loads. | `ProjectFile::save`, `save_project` |

## Problems

1. **Requests go stale.** A request points at one source hash and one positional path. Any edit before Generate, such as typing text, moving an element, or another request applying, changes the hash or shifts sibling indexes. The request then fails with "Select it again." That breaks the core flow of placing several requests, then generating.
2. **One element per request.** Users can't select a card and its neighbour and say "make these match."
3. **Save can overwrite unrelated work.** Save writes over the file even if it changed on disk after it was opened, for example in a second window or from a synced folder. Save as can replace a different project's file after a single OS confirmation.

## Part 1: stable element targets

### Element IDs

- Every element in page content gets a `data-desegnos-id` attribute holding a short random ID, such as 8 base32 characters, unique within the page.
- IDs are assigned when content enters the project: generation apply, brand-import page creation, paste, and migration of existing pages. Elements created by local edits get IDs right away.
- Generated output keeps existing IDs. The requirements tell the model to keep `data-desegnos-id` on the elements it keeps, and to leave it off new elements. Desegnos assigns IDs to new elements after the scope check passes.
- **Export removes the attribute.** Code view hides it by default, under a "Show editor attributes" option in Split/Code. Non-technical users never see it.

### Target model

```
ElementTarget { id: String, tag: String, label: String, selector: String /* fallback only */ }
Region.element: Option<ElementTarget>  ->  Region.elements: Vec<ElementTarget>  (1..=8)
```

- Resolve by `id` first. Fall back to the positional `selector` only for content from before migration that has no IDs yet.
- Remove `sourceSha256` from the target. The job snapshot still records the page source hash for provenance, but a target no longer depends on it.
- A target is stale only when its ID is gone from the page. In that case the request shows "This element was removed" and offers **Remove request** and **Pick again**.
- `pickElement` deduplicates by ID.

### Multi-select

- Shift+click (or Ctrl/Cmd+click) adds an element to the selection or removes it. Only elements on the same page can be combined. The outline is drawn around each one.
- With 2 or more selected, the selection toolbar shows **Ask AI** and **Delete** only. Edit and drag-move stay single-select for now.
- Ask AI with N elements creates **one** request with `elements = [...]`. The request box sits over the union of their bounds.
- If one selected element contains another, keep the outer one and drop the inner one. Its descendants are in scope anyway.
- The status bar shows "3 elements selected", like the OpenPencil footer, with no names.

### Scope check

- `validate_scope` removes each targeted ID's subtree from the before and after documents, then compares the rest canonically, the same way it does now.
- Each targeted ID must still exist in the output, unless the request asks for deletion. If one is missing, reject with "The AI removed a selected element. Your page is unchanged."
- A new element counts as in scope only if it is inside a targeted subtree.
- Bump `requirements` format to 9. Format 8 and older keep the current positional path.

### Migration

- On open, `migration.rs` assigns IDs to every page's elements and maps each open request's positional target to an ID. A target that won't resolve stays as it is and shows the stale state.
- Adding IDs counts as a change that is not saved yet, so the file updates on the next Save. Nothing writes the file without a Save.

## Part 2: safe save

### Conflict check on Save

- On open and after each successful save, record the file's `manifest.project_id`, `manifest.revision` and its SHA-256 as `FileStamp`. Keep it in memory only.
- Before **Save** overwrites, read the manifest on disk and compare:
  - Same stamp: save normally.
  - Same project, different stamp: show **"This file changed since you opened it."** with the options **Save as copy** (default), **Replace it**, and **Cancel**. Replace still keeps the existing backup.
  - Different project, or a file that can't be read: never overwrite. Show "Save as" with a new name filled in.
- Plain language only. "Revision" and "hash" go under "Show details".

### Guarded Save as

- If the chosen path holds a Desegnos project with a different `project_id`, ask a second time inside Desegnos: **"<file> is a different project. Replace it?"** The default is Cancel. The OS overwrite prompt alone isn't enough.
- If the path holds a non-project file, refuse: "Choose a name ending in .desegnos."

### Recovery: keep as-is, small additions

- The journal already covers crash recovery. Two additions:
  - In the recovery list, show the last-saved file path and how long ago the draft changed ("Edited 5 min ago · website.desegnos"), so users can tell drafts apart.
  - After a conflict choice of **Save as copy**, keep the journal until the copy saves, so cancelling the dialog loses nothing.
- No per-chat drafts, since Desegnos has no chats. One journal per project stays.

## Tests

- Core: an ID survives text edits, moves, and another request applying. A positional fallback still resolves on content from before migration. A multi-target request is accepted when only the targeted subtrees change and rejected when a sibling changes. A missing target ID is rejected. Export output has no `data-desegnos-id`.
- Storage: Save refuses when the stamp differs, refuses a different `project_id`, and allows a matching stamp. Save as into another project's file needs the in-app confirm.
- UI (`tests/`, native screenshots): Shift+click selects two elements, Ask AI creates one request over the union of their bounds, and a text edit made before Generate does not make the request stale.

## Ownership

Same split as CLEANUP_SPEC.md: Codex owns `crates/**`, `src-tauri/**`, `tests/**`; Claude owns `src/**`. Codex goes first and records the new command and type shapes in `docs/CLEANUP_BACKEND_CONTRACT.md`. Claude then builds the UI against that contract.

| Codex (backend) | Claude (frontend) |
| --- | --- |
| Assign `data-desegnos-id` (apply, import, paste, local edits, migration); strip on export | Hide `data-desegnos-id` in the code pane, with "Show editor attributes" in Split/Code |
| `ElementTarget` by ID, `Region.elements` (1–8), positional fallback, stale detection | Multi-select: Shift/Ctrl/Cmd+click, outlines, union request box, nested-element pruning, "3 elements selected" |
| `validate_scope` per target ID, missing-target rejection, new requirements format | Stale request state: "This element was removed" with **Remove request** / **Pick again** |
| `FileStamp` check on Save; guarded Save as (other project / non-project file) | Conflict dialogs: **Save as copy** / **Replace it** / **Cancel**, and the in-app "different project" confirm, with details behind "Show details" |
| Recovery metadata: last-saved path and last-edit time; keep journal through Save as copy | Recovery list rows: "Edited 5 min ago · website.desegnos" |
| Core and storage tests; native UI tests | Update Page check, code-map selection and `pickElement` dedupe to use IDs instead of selectors |

### Frontend done ahead of the backend (2026-10-20, Claude)

- **Multi-select:** Shift+click (unmoved) or Ctrl/Cmd+click adds or removes elements. Each extra element gets a dashed outline. Nested picks keep the outer element. The status bar says "N elements selected", and drag-move is off while several are selected. The toolbar shows "N selected", **Ask AI** (disabled placeholder until `Region.elements` exists) and **Delete**. Delete works now: one confirm, then items are removed back to front in document order, one Undo step each. Checked in the app: 2 selected → deleted → 2× Undo restored the page.
- **Stale request banner:** an element request the canvas can't resolve shows "This element changed or was removed" with **Pick again** (click the right element; the request moves there with its text, via add/update/remove commands) and **Remove**. It's a safety net only. In the app, the current backend already re-attaches requests when other elements are edited, refuses deleting an element that has a written request, and refuses requests on stale page versions, so I couldn't trigger the banner.
- **Editor attributes in code:** `data-desegnos-*` attributes are drawn faint in the code pane, with a **Show editor attributes** toggle that appears when the page has any. They're faded, not removed: stripping them from the edited text would make `edit_page_source` lose the IDs. **Codex:** if hiding them fully is wanted, `edit_page_source` would need to restore omitted IDs on unchanged elements.
- Native `--ux-audit-only` (25), `--layers-only` (24) and `--source-edit-only` (8) still pass.

**Problem 1 is mostly solved already.** "Requests go stale" no longer reproduces: the backend now updates request targets after edits. IDs are still worth doing for multi-element requests and for generation output, which can't be matched by position.

**Note:** format 9 is already in use since the cleanup (provider capabilities, found content). This change needs the **next** format, 10, and formats ≤ 9 keep their current behavior.

## Out of scope

- Multi-element drag, resize, or style editing.
- Detecting changes made by another app on disk while the project is open. That is checked only at Save.
- Anything from OpenPencil's vector/`.op` model.
