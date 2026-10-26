# Cleanup backend implementation — 19 October 2026

The subsequent 20 October selection/save, History and frontend integration is complete; [the follow-up report](SELECTION_SAVE_CODEX.md) records current versions and checks. Results below describe the original cleanup run.

Codex completed X1, X2, X3, X4, X5 and X7 from `CLEANUP_SPEC.md`. X6 was explicitly excluded. Frontend files remain Claude-owned. The user's running editor was neither relaunched nor controlled; native checks used disposable profiles, session buses and guarded Xvfb displays. See [the frontend contract](CLEANUP_BACKEND_CONTRACT.md) for the actual commands and remaining C4/C5/C9 wiring.

## Result

- Current format **26** captures provider capabilities, asks for research before placeholders, limits missing-private-fact questions to essentials, marks found content, and allows eight image downloads within the existing byte/size/decoding/network budgets.
- Found content retains its validated HTTPS source in project storage. Export removes the editor attribute while preserving ordinary credits and links. Local text/image replacement clears stale provenance; style/position changes preserve it. Source opening derives its destination from an exact current-source selection, rather than accepting an arbitrary frontend URL.
- `edit_page_source` validates source, checks project/revision/target ownership, protects written pending requests, checkpoints before changing memory, and makes one Undo step. Identical edits do not write or add history. HTML/JavaScript use SWC; CSS uses Lightning CSS plus strict token/block checks. Project storage preserves authored bytes. Portable export applies its existing HTML normalization and asset extraction.
- Source maps expose browser-resolvable selectors and one-based UTF-16 line/column ranges, including implicit wrappers. They are tied to the exact source fingerprint. Tests cover multiline attributes, CRLF, Unicode, SVG, table wrappers, pending-request remapping, stale edits and write failure.
- Native provider selection, separate settings/keyring services, model catalogues and capability/sign-in descriptors now support OpenAI, Anthropic and Google. OpenAI's existing ChatGPT/key storage remains compatible. Retries retain their frozen provider and credentials. OpenRouter/Local are unavailable, with no request implementation exposed.
- Anthropic and Google use their documented native protocols, bounded streams, cancellation/deadlines and reported usage. Search/citations/thoughts stay separate from the final JSON. Anthropic's strict result tool is an envelope that Desegnos reads; no client tool is executed. There is no automatic paid retry or service fallback. The credential-free image adapter remains separate from provider authentication.
- Native workflows use flat menus, the floating status/result pill, browser page tabs, Edit/Ask AI/Delete selection actions, right-click and current shortcuts. Removed pre-send technical controls are checked through their actual native contracts rather than recreated in default UI. Historical workflow sequences are retained; the current required acceptance flows were rerun.

## Verification

Evidence index: [`test-results/cleanup-codex-2026-10-19/verified-checks.json`](../test-results/cleanup-codex-2026-10-19/verified-checks.json). Native reports also record screenshots, exact binary hashes, browser errors, clipboard/recovery results and the original desktop connection/resource checks.

- Required UX audit: **25 steps passed**, including long-prompt containment, one-click generation/application, cancellation, local edits, stale-output protection, Undo/Redo, native archive/pickers, real two-page export, ordinary Chromium execution/navigation and fresh-process recovery.
- Required Layers: **24 steps passed**, including visual stacking/hit testing, nested groups, real system clipboard, cut/copy/paste, large requests, history, blocked/draft generation, real downloaded images, export and fresh-process recovery.
- Native source editing: accepted source rendered in the actual canvas; invalid/stale/wrong-project edits preserved data; one-step history, exact save/reopen, metadata stripping at export and crash recovery passed.
- Native identity import: project/folder/ZIP intake, frozen source analysis and recovery passed through the refactored OpenAI boundary.
- Isolated Secret Service: separate OpenAI/Anthropic/Google keys, independent removal, process/daemon persistence, and ChatGPT exchange/rotation/revocation passed. No real user keyring was used by this test.
- Rust workspace tests with all features, strict Clippy, formatting, TypeScript/Vite and canvas geometry checks passed. Provider HTTP tests cover fragmented UTF-8/SSE, credential headers/redaction, cancellation, bounded/complete JSON, citations/thinking, unexpected tools and usage accounting. New adapters were tested with local fixtures; no Anthropic or Google live key was available.

The owner substituted the connected **ChatGPT plan** for X1's API-key check. Exactly **one** live generation ran: **3 web searches, 1 downloaded real lighthouse photograph, 0 placeholders**, accepted JSON and reported token usage. The live response contained source links but **0 citation annotations**; annotation handling was verified separately with stream fixtures. A fresh offline export of that result rendered in ordinary Chromium with the photo and visible credits, and no found-content attributes. No second live generation ran. Confirmed billing and owner quality ratings remain unknown.

The controlled image-capacity benchmark used the same eight local image URLs and actual native decoding/download processing: format 8 downloaded **4** with **4** missing-image slots; format 9 downloaded **8** with **0**. This proves the capacity change, not a general model-quality improvement. The separate 18-scenario owner-rated release gate remains **UNVERIFIED**.

## Dependency review and limits

Pinned SWC common 26.0.0, ECMAScript parser 46.1.1 and HTML parser 26.1.0 are Apache-2.0; Lightning CSS 1.0.0-alpha.72 is MPL-2.0. They are established parser toolchains, used without transforming or emitting the authored project source. Optional plugin runtimes/bundling/Node bindings were not added. Existing `url` 2.5.8 is reused for URL validation.

The initially considered SWC CSS parser was removed because it pulled advisory-affected `lexical`/`lexical-core`; neither remains in either lockfile. [The dependency review](../crates/desegnos-core/source-dependency-review.json) records the exact new package graph, licenses, repositories and official RustSec database commit. Its only package-name advisory match is hashbrown 0.14.5, which is explicitly unaffected by that advisory. This review covers newly added packages, not every preexisting desktop dependency.

Native UI verification is Linux/WebKitGTK; Windows/macOS runs remain outside this environment. Model token estimates are conservative checks, not guaranteed billing caps; search fees and future service prices can differ. Google's catalogue records the verified promotional rates while estimates use its announced higher rates. Provider API/account policies can change. References: [Claude tools](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-search-tool), [strict outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs), [Gemini JSON/search](https://ai.google.dev/gemini-api/docs/generate-content/structured-output), [Lightning CSS](https://lightningcss.dev/docs.html).
