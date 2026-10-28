# Desegnos release plan

Updated 2026-10-28 for the `infinite-canvas` branch. Product direction is [PRODUCT.md](PRODUCT.md); the canvas concept is [CANVAS.md](CANVAS.md); editor look and behaviour are [DESIGN.md](DESIGN.md). Where this file and PRODUCT.md disagree on direction, PRODUCT.md wins.

## What ships

Desegnos is a desktop visual design tool. A project is one infinite canvas. Boundaries define what ships; elements, requests, pointers and reference hold the design. AI requests produce editable elements. Export compiles website/app-screen visuals from the model and can generate missing pointer behaviour; slides and images render directly. App-screen export means web files, not native app packages.

This branch replaces the earlier HTML-source editor and preview pipeline. There are no code snapshots, per-page editors, source documents or Design/Split/Code views. Code exists only in exports. Project files are SQLite databases with content-addressed versions.

## Current state

Implemented paths on `infinite-canvas` (source inspection, not a fresh build or quality verdict):

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

Capability status and narrowly scoped test evidence are in [CANVAS.md](CANVAS.md#capability-status). The existence of the paths above does not establish finished appearance, reliable exported interactions or output-specific reopening fidelity. No finished-example quality gate is recorded as passed.

## Next milestone: three finished examples

Complete the [social image, short slide deck and responsive two-page website](CANVAS.md#next-milestone-three-finished-examples). That table defines each example's content, editing, export and reopening checks. Deliver the three `.desegnos` projects, their exported files and recorded owner review of the intended appearance before and after a meaningful edit and reopening. All three are pending; a valid PNG/PDF header, passing build or template test does not finish an example.

Use these examples to find and fix concrete gaps in rendering, typography, images, clipping, linked-size overrides, navigation and pointer behaviour. Verify exported files in independent viewers and the website at both named and intermediate widths. Keep the [generation benchmark](docs/GENERATION_BENCHMARK.md) owner ratings separate from implementation and structural checks.

Further preview work remains: website-import fidelity for complex CSS and script behaviour; keyboard, accessibility and window-size review; real-provider benchmark runs; platform-specific native checks; packaging, signing and hosted-generation integration. These remain open until evidence is recorded, even where a control or implementation already exists.

## Contracts that do not change

- **Placement fidelity.** AI-made elements land in the request area that asked for them. Exports keep positions, size and grouping close to the canvas; Flexible/Precise is a project setting.
- **Scoped changes.** A request changes only its picked or covered targets unless a change pointer explicitly grants wider permission; reference pointers grant read-only context.
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
4. All three finished examples pass editing, export and reopening with their intended appearance; the two-page website works in a browser with no Desegnos runtime.
5. No AI request is sent without an explicit paid-send decision.
6. An imported website can be reviewed and undone in one step.

## After the preview

- Paid hosted generation as another provider adapter, with the same frozen requirements, streaming, cancellation and usage reporting.
- Accounts and billing in native code; accounts stay out of the canvas.
- Further reuse workflows, content collections, localisation, publishing and other research candidates in [FEATURE_RESEARCH.md](FEATURE_RESEARCH.md).
- Windows and macOS first-class support, code signing and CI runners.
