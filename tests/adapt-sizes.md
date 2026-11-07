# Responsive adaptation verification

Generation previously pushed built elements directly into `page.elements`
(`generation.rs`, `InsertInBoundary`), bypassing `insert_elements` and its x/width
fitter. The fixture's autosave contains three desktop-only pages with no overrides;
its saved head contains an empty page. Export's widest-edge fallback then used
1440 as the pixel denominator at every breakpoint, shrinking phone text to 27%.

`adapt_size` now derives automatic overrides from the base tree. Free-layout rows
stack when they overflow; decorative art scales separately. Stacks and grids
reduce their horizontal composition, text uses fit-text's font measurement with
readable minimum sizes, images retain proportions, and controls retain tap height.
Growth propagates through parents to the page. Coincident text effects keep their
frontmost copy at narrow sizes. Manual overrides retain ownership of the whole
override; editing an automatic override makes it manual.

Project edits and generation adapt changed pages before committing their existing
undo step. Shared text-style edits also invalidate adaptation. The selected size's
Properties has **Adapt layout**, also available through **Ctrl/Cmd+Alt+Shift+A**.
Canvas, Preview, Play and export consume the stored overrides. Export uses each
breakpoint's width as its pixel denominator. Breakpoint CSS now diffs against the
preceding breakpoint, preventing tablet values from leaking into desktop.

## Repeat the checks

```sh
node scripts/cargo.mjs test -p desegnos-core --lib --tests
node scripts/cargo.mjs test -p desegnos-storage
node scripts/cargo.mjs clippy -p desegnos-core -p desegnos-storage --all-targets -- -D warnings
node node_modules/typescript/bin/tsc --noEmit
node --test tests/project-commands.test.mjs tests/visible-actions.test.mjs tests/editor-shell-ssr.test.mjs tests/canvas-model.test.mjs tests/project-canvas-geometry.test.mjs tests/export-panel.test.mjs
node tests/adapt-sizes.test.mjs
```

The fixture test skips cleanly when the fixture or Chromium is absent. Override
locations with `DESEGNOS_ADAPT_FIXTURE`, `CHROMIUM`, and `DESEGNOS_ADAPT_SHOTS`.
Set `DESEGNOS_KEEP_ADAPT_OUTPUT=1` to retain temporary before/after exports.
The Rust harness copies the database before opening it with desegnos-storage and
uses the existing direct export path, retaining accepted behaviour code. It never
saves or opens the source fixture through storage.

The browser checks all three pages at 390×844, 820×1180 and 1440×900. They assert
no horizontal scroll, no phone text below 13px, and no narrow-layout text boxes
intersecting by more than 2px. They wait for the accepted typing animation to
complete. Desktop checks compare shared markup (normalizing the responsive
visibility hook) and computed CSS geometry; the harness also compares every base
model element. Desktop's original overlapping text effects remain unchanged.

Results: **203 core tests**, **41 storage tests**, all six focused Node check files,
TypeScript, Clippy, and all nine browser viewport checks passed. The absent-fixture
skip path also passed.

Six adaptations measured **71–74 ms in debug**, including font discovery. The harness
allows 1000 ms in debug and 100 ms in release. No release build was run, so release
performance remains unmeasured.

## Changed files

- Core: `adapt.rs`, `elements.rs`, `generation.rs`, `model.rs`, `render.rs`,
  `web_codegen.rs`, `website_import.rs`, `lib.rs`.
- UI: `App.tsx`, `EditorDock.tsx`, `editorShortcuts.ts`, `session/useEditor.tsx`,
  `shortcuts.ts`, `types.ts`.
- Checks: `adapt_sizes.rs`, `elements_and_session.rs`, `frontend_contract.rs`,
  `generation.rs` tests; storage `examples/adapt_fixture.rs`;
  `tests/adapt-sizes.test.mjs` and this report.

## Screenshots

Directory:
`/tmp/claude-1000/-home-cheli-Projects-WebMaker/d472d531-9403-4f03-b2c9-4b4dcbdb7b60/scratchpad/adapt-shots/`

- `index-phone.png`
- `index-tablet.png`
- `lineup-phone.png`
- `lineup-tablet.png`
- `tickets-phone.png`
- `tickets-tablet.png`
