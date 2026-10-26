# Frozen canvas rendering

The editable canvas loads the active page through the Live preview server in an
opaque-origin iframe (`allow-scripts allow-forms`, never `allow-same-origin`).
The bridge waits for load, fonts (bounded at 1.5 seconds), and 300 ms of idle time.
Child preview frames capture concurrently, including pages that change their
appearance when framed. The editor receives HTML and removes the running iframe.
It displays the result in its existing script-disabled, same-origin srcdoc
sandbox. Site code never executes in the editor origin. CSP remains unchanged.

`preview_source_document` adds random, disposable provenance tokens to served
markup. The early bridge remembers the actual original node for each token;
cloning a token does not copy ownership. Snapshots carry an original editor ID,
or the original source selector for files without IDs. `SourceMapping` binds
these to a separately parsed source document. Picking, layers and lookup use
source identities, while geometry comes from the frozen DOM. Runtime-only nodes
remain visible without editable identities. Changed text subtrees fail closed
for inline editing because their rendered text indexes no longer name source
text nodes safely. A navigation to another source path cannot borrow the requested
page's identities.

Moves, resizes and layer operations save only their explicit CSS delta onto the
original source inline style. Copying a layer copies its source subtree and
source inline styles, including source descendants hidden or removed by scripts.
It does not bake runtime appearance into source CSS. No rendered document is sent
to a source-saving command. Rust and browser regressions cover this boundary.

Resources remain preview URLs. Live's lazy renderer keeps binary assets by
reference and uses path placeholders for URL resolution, avoiding data-URI
expansion. Canvas pixel buffers are an exception: readable canvases become raster
snapshots. CSS animations/transitions are pinned to their captured values;
form state and dynamically inserted stylesheet rules are serialized too.

Source revisions and framed-document changes debounce capture by 180 ms. Only
the active page and the frames it loads execute, not every page in the package.
Pan, selection, layers and drag previews reuse the frozen DOM. Passive Live
overlays are disabled; only explicit Interact mounts a running iframe. Superseded captures
are aborted and their iframe removed. The old frozen canvas remains mounted until
its replacement is prepared. Capture has an 8 second timeout and snapshots have
a 16 MiB document limit and 16-frame-depth limit.

## Verification and limits

`npm run test:preview` uses headless Chromium, a random-port loopback fixture
server and a temporary browser profile. It does not launch the native editor or
use a desktop display. It requires a `chromium` executable (or `CHROMIUM`). The
20,000-element fixture captures, loads and maps its frozen DOM with a three-run
median of about 1.1 seconds on the development machine during quiet runs; this excludes native project loading
and package creation. A heavily contended run (host load average near 32) took
3.5 seconds. Timing is diagnostic by default; enforce the owner's budget on an
isolated machine with `DESEGNOS_PREVIEW_MAX_MS=1500 npm run test:preview`.
The owner's full chelib project and WebKit custom-protocol rendering still need
an end-to-end performance/appearance check; the native GUI suite was intentionally
not run.

Cross-origin embeds without the preview bridge cannot expose their DOM through
the browser's origin boundary. They retain a script-disabled network fallback,
marked `data-desegnos-snapshot-unavailable`, and their contents are not editable.
Readable inline srcdoc frames can be captured, but gain no invented source
mapping. Closed shadow roots, cross-origin canvas pixels, GIF/video frames,
SVG SMIL timing and browser-only state are not fully represented by HTML
serialization. Scripts that finish rendering later than the idle window require
a future explicit settle hook or adjustable capture policy. Re-capture runs the
active page and its frame tree again after edits; independent per-frame caching
is a possible future optimization if real-project measurements require it.


## Checks run

- `npm run build`: passed (existing bundle-size warning).
- `npm run test:editor`: 45 passed.
- Preview policy, headless capture and overlay lifecycle tests: 24 passed.
- Measurement, frame overlay geometry, framed documents and selection helpers: 39 passed.
- Core Rust unit tests: 63 passed, 2 existing opt-in tests ignored.
- Source, editing, document, stable-target and layer Rust contracts: 54 passed.
- Desktop preview registry tests: 10 passed; CSP and command permissions: 2 passed.

Rust commands used `CARGO_TARGET_DIR=~/.cache/desegnos-parity-target`
and `node scripts/cargo.mjs`. The workspace formatting check reports existing
formatting differences outside this change. `git diff --check` passes.

## Changed files

- Canvas: `src/Preview.tsx`, `src/frozenPreview.ts`, `src/Canvas.tsx`, `src/FrameOverlays.tsx`.
- Preview runtime: `src-tauri/src/preview.rs`, `src-tauri/src/preview_bridge.js`.
- Core: `crates/desegnos-core/src/lib.rs`, `crates/desegnos-core/src/preview_source.rs`, `crates/desegnos-core/src/website_import.rs`.
- Regression coverage: `tests/frozen-preview.test.mjs`, `tests/frame-overlay-lifecycle.test.mjs`, `crates/desegnos-core/tests/frozen_source_contracts.rs`.
- Test command and design notes: `package.json`, `docs/frozen-canvas.md`.
