# Desegnos release plan

Updated 2026-10-27 for the `infinite-canvas` branch. Product direction is [PRODUCT.md](PRODUCT.md); the canvas concept is [CANVAS.md](CANVAS.md); editor look and behaviour are [DESIGN.md](DESIGN.md). Where this file and PRODUCT.md disagree on direction, PRODUCT.md wins.

## What ships

Desegnos is a desktop visual design tool. A project is one infinite canvas. Boundaries define what ships; elements, frames, pointers and reference hold the design; the AI fills frames and builds websites and apps at export; slides and images render directly.

This branch replaces the earlier HTML-source editor and preview pipeline. There are no code snapshots, per-page editors, source documents or Design/Split/Code views. Code exists only in exports. Project files are SQLite databases with content-addressed versions.

## Current state

Built and compiling on `infinite-canvas`:

- Canvas surface with pan, zoom and dot grid; draw, move and push-apart boundaries; Page and Screen kinds.
- Element tree per boundary; boxes with free, stack and grid layout; text, shapes, lines, images, buttons, links, form controls, video, audio, embeds and code blocks.
- Selection, move, resize, inline text edit, group, copy/paste with linked or unlinked choice, linked elements and size overrides.
- Reference items (notes, links, pictures, swatches), pointers with comments, Play actions, link lines and clash choices.
- Tool strip, Properties/Layers dock, project brief, history and alternatives.
- Central keyboard map covering the CANVAS.md table, plus save/open/new/close and F2 rename.
- Project tabs: several projects open at once, each with its own canvas.
- Import: website address, folder or ZIP rendered at 1440 px and 390 px and converted into boundaries and elements; project pictures; script notes; behaviour pointers; one undo step.
- Export records and pointer wiring stay outside undo history but are saved in the project database.
- AI generation uses provider adapters (API keys, ChatGPT plan, OpenAI-compatible local servers), frozen requirements, cancellation and usage reporting.

Work that remains before a confident preview (see the checklist in CANVAS.md):

- Finish the element model and renderer polish where the canvas checklist is still open.
- Verify AI fills frames and changes layers with element output end to end.
- Finish linked-size placeholders and the paste-choice edge cases.
- Export: AI-built websites and apps, direct slide and image output, pointer fingerprints and export clashes.
- Import fidelity: complex CSS backgrounds, responsive-only elements and script behaviour recognition.
- Full keyboard, accessibility and responsive-window pass.
- Benchmarks with real provider calls; Linux hands-on verification; Windows/macOS runners.
- Release packaging, signing and the hosted-generation provider boundary.

## Contracts that do not change

- **Placement fidelity.** AI-made elements land in the frame that asked for them. Exports keep positions, size and grouping close to the canvas; Flexible/Precise is a project setting.
- **Scoped changes.** A request changes only its layer unless a pointer explicitly grants wider permission.
- **Ordinary output.** Exports open without Desegnos and are not locked to a host.
- **Untrusted imports.** Fetched sites are bounded, public-address-only downloads. Page code never runs during import, rendering or analysis. Imported bytes are data, not instructions.
- **Local authority.** Credentials live in native secure storage or session memory. The renderer never reads a secret. Export destinations and file writes are chosen and performed natively.
- **History never rewrites itself.** Saves append versions. Restoring a version is one undo step. Export metadata is not an undo step.
- **Plain language.** Controls perform implemented operations; unfinished capabilities are disabled and say so.
- **Opt-in diagnostics.** Crash and error reports are off by default and never include prompts, site content, keys or file paths.

## Preview gates

1. Typecheck, production frontend build and `cargo check --workspace --all-targets` pass.
2. The canvas, element, pointer, reference, project-tab and import flows work in the desktop app on Linux.
3. A project saves, reopens, recovers after a crash and restores history.
4. A website export opens in a browser with no Desegnos runtime.
5. No AI request is sent without an explicit paid-send decision.
6. An imported website can be reviewed and undone in one step.

## After the preview

- Paid hosted generation as another provider adapter, with the same frozen requirements, streaming, cancellation and usage reporting.
- Accounts and billing in native code; accounts stay out of the canvas.
- Block/component reuse, content collections, localisation, publishing and other research candidates in [FEATURE_RESEARCH.md](FEATURE_RESEARCH.md).
- Windows and macOS first-class support, code signing and CI runners.
