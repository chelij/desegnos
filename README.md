# Desegnos

A visual canvas with AI. Place elements exactly where you want them on an infinite canvas, by hand or by describing them in a request. Everything stays visual while you work; export compiles website and app-screen HTML/CSS from the design, AI can supply missing pointer behaviour, and slides and images render directly.

Desegnos is a Tauri desktop app: React 19 and TypeScript for the editor, Rust for the project model, storage, AI providers and export.

- [Product direction](PRODUCT.md)
- [Canvas concept](CANVAS.md)
- [Editor design](DESIGN.md)
- [Glossary](GLOSSARY.md)
- [Project map](docs/PROJECT_MAP.md)

## What the app does

**One canvas per project.** Boundaries are the outputs: a page, slide or image. Pages cover websites and app layouts, with growing or fixed heights; Website and App screen remain starter templates. Everything inside a boundary ships at export. Everything outside every boundary is reference for the AI and never ships. Boundaries sit side by side, never overlap, and linked sizes (phone, tablet, desktop) share their elements with per-size overrides.

**Elements, layers and requests.** Boxes, text, shapes, lines, images, buttons, links, form controls, video, embeds and code blocks are visual elements. A box can lay its children out freely, in a stack or in a grid. A layer is one element or a group, with its own AI request history and context. A request picks targets or covers an area; the AI returns editable elements.

**Pointers.** A pointer is a line from a layer or boundary to its target. Its comment is the instruction. A reference pointer grants read-only context; a change pointer permits changes to its target within the project. Simple pointers run visually in Play; the rest compile at export.

**Play and Preview.** Play makes a boundary clickable and runs simple pointers on the canvas. Preview export builds the real page in the background and runs it over the boundary in a sandbox. Escape returns to the design.

**Import.** Import context reads a website, folder or ZIP as untrusted brand evidence. **Import website…** renders a site's pages in a hidden, script-disabled frame and converts them into elements: containers become boxes, text becomes text, images join the project picture store, controls become their kinds, and links between imported pages draw automatic link lines. Scripts become reference notes and recognisable behaviours become pointers with comments, left unclassified.

**Projects and history.** Several projects can be open as project tabs. A project is one `.desegnos` SQLite file holding the design and its version history. Saves create labelled versions; History restores one as a single undo step; Alternatives are named branches. Exports stay ordinary folders outside the project.

**Keyboard.** Every tool and action has a shortcut from one keymap in [src/shortcuts.ts](src/shortcuts.ts). The tool strip tooltips, the menu and **Menu → Keyboard shortcuts** read the same table. Shortcuts stay out of text fields except where saving and preview still make sense.

The next milestone is [three finished examples](CANVAS.md#next-milestone-three-finished-examples): a social image, a short slide deck and a responsive two-page website that preserve their intended appearance through editing, export and reopening. Current [capability status](CANVAS.md#capability-status) records implementation and limited test evidence separately; finished-output quality remains unverified. App screens export as web files, not native packages.

## Run it

Requirements: Node.js, a Rust toolchain, and the desktop dependencies for Tauri (WebKitGTK 4.1, a desktop portal and a Secret Service provider on Linux).

```sh
npm install
npm run dev        # Vite frontend at 127.0.0.1:1420
npm run desktop    # Tauri desktop app
```

Checks:

```sh
npm run typecheck
npm run build
node scripts/run.mjs cargo check --workspace --all-targets
```

Build a local release with `npm run release:local`. Linux is the tested platform; Windows and macOS builds are unsigned previews.

## Project layout

```text
crates/desegnos-core/     Project model, canvas elements, generation, export plans, website import.
crates/desegnos-storage/  .desegnos SQLite project, versions, safe writing, import snapshots.
crates/desegnos-ai/       Providers, credentials, ChatGPT, image APIs, safe website fetching.
src-tauri/                Desktop commands, workspace, generation, export, local static server.
src/                      React editor: canvas, dock, menus, dialogs, import, keyboard map.
src/projectCanvas/        Boundaries, elements, requests, reference, pointers and canvas helpers.
docs/                     Contracts, guides and the project map.
```

Only current project formats are supported. Keys, sign-in tokens and crash reports stay separate from project files. See [SECURITY.md](SECURITY.md) for reporting a vulnerability.

Desegnos is licensed under GPL-3.0-only; see [LICENSE](LICENSE).
