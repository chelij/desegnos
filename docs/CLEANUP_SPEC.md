# Desegnos cleanup spec

Follow-up status, 20 October 2026: remaining integration, editable CodeMirror, exact History and selection/safe-save work are implemented. [The follow-up report](SELECTION_SAVE_CODEX.md) records the current behavior and verification. The original owner decisions and dated handoff notes below remain as their historical context.

Date: 2026-10-19. Source: owner interview plus a UI audit of the latest native screenshots and the frontend source. Where this file conflicts with UX_AUDIT.md or FEATURE_PRIORITIES.md on what to build next, this file wins. PRODUCT.md still sets direction.

## Owner decisions

| Question | Decision |
| --- | --- |
| Core flow | Place boxes on the canvas, describe them, press Generate. |
| Audience | Non-technical people. They never need to see code, tokens, HTTP data or jargon. |
| Biggest problems | Too many controls; AI results full of placeholders. |
| MVP features (keep) | Brand import, Project brief, Layers and arrange, Grid, rulers and snapping. |
| Where advanced controls live | Hidden until needed: right-click, the selection toolbar, or the Menu. No panels. |
| AI setup | Sign in with the provider where it's offered; otherwise paste an API key. Defaults for everything else. |
| After Generate | Apply instantly. Small Undo / Try again bar. No dialog. |
| Missing content | AI searches the web and fills it in, and marks what it found so the user can swap it. |
| Output | Export folder now; **Publish** shown as "coming soon". |
| Code | Dreamweaver-style **Design / Split / Code** switch. Split shows the page's code beside the live canvas. Design is the default, so non-technical users never have to see code. |
| Unfinished capability | Show a disabled "(coming soon)" placeholder instead of half-built UI. |

## Principles

1. **Canvas first.** The main window has a top bar, page tabs, the canvas, the floating canvas toolbar and a thin status bar. Nothing else stays on screen.
2. **Hidden until needed.** A control appears only when it applies: a selection toolbar for the selected item, the right-click menu for the target, and the Menu for project-level actions.
3. **Plain language.** No "snapshot", "requirements", "SHA", "schema", "HTTP", "store: false", "reasoning effort" or "provider" in default views. Technical facts go behind a "Show details" disclosure.
4. **No layout shift.** Status and progress float over the canvas; they never push it down.
5. **Placeholders over half-features.** If it isn't ready, show it disabled with "(coming soon)".

## Already done (uncommitted in the working tree, by Claude)

- Flat Menu: New, Open, Save, Save as, Recover (only when there's something to recover), Add page, Page settings, Remove page, Preview, View code, Export, Publish online *(coming soon)*, AI settings, AI style, Project brief, Editor theme, Keyboard shortcuts.
- Top bar: Undo, Redo, Preview, Export, **Connect AI** (only when not connected), Generate. Removed the Requests popover, the interpretation dropdown and the AI status pill.
- Removed from the main window: ChatGPT welcome modal, Project details, View AI instructions, PROJECT.md viewer, page Light/Dark status toggle, long export path banner.
- Selection toolbar: **Edit**, **Ask AI**, **Delete**. Copy, cut and arrange moved to right-click and shortcuts.
- AI settings: provider dropdown with Anthropic, Google, OpenRouter and Local as "coming soon".
- Generation request: OpenAI `web_search` tool is enabled for requirements format 7, and the requirements tell the model to search before using placeholders (`crates/desegni-ai/src/settings.rs`, `crates/desegni-core/src/requirements.rs`, `sse.rs` accepts `web_search_call` items). Covered by unit tests; **not yet tried against the live API.**

## Issues found in the audit

| # | Issue | Where |
| --- | --- | --- |
| A1 | Request box text overflows its dashed border and spills over the page; long prompts look broken. | `RequestBox` in `src/Canvas.tsx`, `src/canvas.css` |
| A2 | Generation progress strip pushes the canvas down (layout shift) and says "Waiting for OpenAI". | `generation-strip` in `src/App.tsx` |
| A3 | The failed-generation dialog shows HTTP status, request ID, body size, data-retention text and "Retry same snapshot". | `src/Generation.tsx` |
| A4 | The API budget field shows "010" (leading zero from a number input bound to a float). | `src/AiSettings.tsx:167` |
| A5 | AI settings is a long form: key storage radios, Test access, model, budget, advanced settings. Too much for a first run. | `src/AiSettings.tsx` |
| A6 | "Replace" element dialog offers "Custom HTML" and tag types (Paragraph, Container…), which is a developer view. | `src/ElementTools.tsx` |
| A7 | No direct way to swap an image: the user must use Edit → attributes. | `src/ElementTools.tsx`, selection toolbar |
| A8 | Mobile pages open at 50% and look tiny; the grid draws over the whole grey workspace, not just the page. | `initialZoom` in `src/App.tsx`, `canvas-grid` in `src/Canvas.tsx` |
| A9 | Project brief uses "Choices", "Exceptions", "Scope" and "Applies to: Page · Request 3", which is jargon. | `src/Notes.tsx` |
| A10 | Native workflow tests still drive the old grouped menu (`['Project','New project']`, `.generation-controls`, `Requests (n)`…). Several references were already stale before this cleanup. | `tests/native/*.mjs`, `tests/native/webdriver.mjs` |

## Work items

Owner key: **Claude** = frontend shell and UI copy. **Codex** = Rust core, AI providers, generation contract, native tests. Split by file so the two never edit the same file.

### Claude: frontend (`src/App.tsx`, `src/Canvas.tsx`, `src/canvas.css`, `src/shell.css`, `src/AiSettings.tsx`, `src/Generation.tsx`, `src/ElementTools.tsx`, `src/Notes.tsx`, `src/NewProject.tsx`)

| ID | Item | Done when |
| --- | --- | --- |
| C1 | **Floating progress.** Replace the generation strip and result-review strip with one floating pill above the canvas toolbar: "Building your page… 0:12 · Cancel", then "Page updated · Undo · Try again". (A2) | The canvas never moves during or after generation. |
| C2 | **Plain failure.** Failed generation shows one sentence ("Couldn't build the page. Your work is safe.") with **Try again** and **Show details**. All provider metadata goes inside Show details. Drop "snapshot" wording. (A3) | No technical terms are visible without opening Show details. |
| C3 | **Request boxes.** Clip the prompt inside the box with internal scroll, show a "Describe what goes here…" placeholder, and keep the number badge. (A1) | A 20-line prompt stays inside its box. |
| C4 | **Connect AI screen.** Step 1: pick a provider (others "coming soon"). Step 2: **Sign in with …** if supported (ChatGPT today), otherwise one key field with Save on this device as the default. Model, budget, effort and storage go under Advanced. Fix the budget "010" bug. (A4, A5) | A new user connects in two clicks and one paste. |
| C5 | **Found-content badges.** Elements marked `data-desegnos-found` (see X2) show a small "Found online" tag when selected, with **Replace** and **Source** in the selection toolbar. | Every AI-sourced image or text can be traced and swapped in one click. |
| C6 | **Image swap.** When an image is selected, the toolbar shows **Replace image**: choose a file, or **Find with AI** (adds an element request). Hide "Custom HTML" and tag types from the default Replace dialog. (A6, A7) | Changing a photo takes no dialog fields. |
| C7 | **Mobile fit + grid.** Open pages fit to height (mobile) or width (web), with a minimum of about 75%. Draw the grid inside the screen guide only. (A8) | A new mobile page fills the canvas height. |
| C9 | **Design / Split / Code view.** A three-way switch in the top bar (and Menu → View code becomes this switch). **Split** puts a code pane beside the canvas with a draggable divider, and the pane can sit on the right or bottom. It has HTML, CSS and JavaScript tabs, syntax highlighting, line numbers and search. **Code** fills the window with the code pane. Selecting an element on the canvas scrolls to and highlights its source line, and clicking in the code selects the matching element. Edits apply about 0.5 s after typing stops or on blur, through X7, and a validation error shows inline without losing the text. Ctrl/Cmd+Z in the pane is text undo; the top-bar Undo is project undo. Editor library: CodeMirror 6, which needs a dependency review entry like the existing ones. The choice is remembered per project. | Typing in Split updates the canvas, and a canvas selection finds its code. |
| C8 | **Brief copy.** Rename to plain language: "Rules the AI must follow" and "Allowed exceptions". Use "Whole site" / "This page" / "This box". Keep the features. (A9) | No "scope" or "decision" jargon in the brief. |

### Codex: Rust and tests (`crates/**`, `src-tauri/**`, `tests/**`, `scripts/**`)

| ID | Item | Done when |
| --- | --- | --- |
| X1 | **Live check of web search.** Run one real generation with an API key and a request that needs a photo. Confirm `web_search_call` items stream through `sse.rs`, a downloaded image lands, and citations don't break the JSON-schema output. Fix the parser if annotations or new event types are rejected. | One benchmark scenario produces a real photo and real copy with no placeholders. |
| X2 | **Mark found content.** Requirements (format 8, keeping format 7 readable) tell the model to add `data-desegnos-found="<source url>"` on content taken from search. Core validation keeps the attribute in the project; the website exporter strips it. | The attribute survives apply and save, and is absent from exported HTML. |
| X3 | **Fewer placeholders by default.** Raise image downloads from 4 to about 8 per generation if size budgets allow. Make "ask one short question" the fallback only for private facts (business name, phone). Add a benchmark metric: placeholders per page. | The benchmark shows a lower placeholder count than before. |
| X4 | **Provider adapters.** Add a provider trait behind `desegni-ai` with Anthropic and Google key adapters (streaming, cancel, usage), each with its own search capability flag. Expose `sign_in_supported` per provider so the C4 screen can choose between sign-in and key. Until an adapter ships, it stays "coming soon" in the UI. | Switching provider requires no frontend change beyond the list. |
| X5 | **Native tests.** Update `tests/native/webdriver.mjs` menu paths and every workflow for the flat menu, the new top bar and the selection toolbar. Replace removed-control steps with shortcuts or right-click paths; don't drop coverage. (A10) | `node scripts/native-test.mjs --ux-audit-only` and `--layers-only` pass. |
| X7 | **Source edit command.** Add an `edit_page_source` native command: `{projectId, pageId, revision, html?, css?, javascript?}` → validates with the same rules as generated candidates (no remote scripts, size budgets, sanitizing), records one undo step, refreshes element bounds and returns the workspace. Rejected source returns line-numbered errors. Also expose a source map from element selector to HTML line range for C9's two-way selection. | A typed edit round-trips: save, reopen and export keep it; invalid code is refused with a line number. |
| X6 | **Publish placeholder backend (later).** No work now; keep the menu item disabled. | n/a |

## Status and handoff (2026-10-19, Claude)

Frontend items done, uncommitted: **C1, C2, C3, C4, C6, C7, C8**, and the read-only half of **C9**. Blocked: **C5** waits for X2. **C9 editing** waits for X7.

C9 so far (`src/CodePane.tsx`): a Design / Split / Code switch in the top bar (`role="group"`, `aria-label="Editor view"`), also under Menu → **View**. It's remembered per project. Split has a draggable divider whose width is remembered. The code pane has HTML/CSS/JavaScript tabs, line numbers, light colouring and **Find in code** (Enter / Shift+Enter cycle). Minified output is pretty-printed for display only. Selecting an element highlights its line, matched by id, then class, then tag; X7's source map will replace that guess. The pane says "Read only for now". No new dependency: CodeMirror waits until editing lands. The old "View code" menu item and "Site source" modal were removed.

Changes that affect native tests (X5):
- `.generation-strip`, `.result-review` and `.generation-controls` are gone. Progress, failure and result now appear in one floating `.canvas-pill` (`role="region"`, `aria-label="Generation status"`). Buttons: **Cancel**, **Try again** / **Build draft**, **Details**, **AI settings**, and for a result **Undo**, **Try again**, `×` (`aria-label="Keep changes"`). Generate is a plain `.primary` button in `.shell-primary-actions`.
- Status headings are now "Building your page…", "Couldn’t build the page", "Page updated" and so on (`src/Generation.tsx`, `GenerationFeedback`). The dialog's retry button reads **Try again** (was "Generate again"), and the blocked-job button reads **Build draft** (was "Generate draft").
- AI settings: the key storage radios sit inside a `details` "Where to keep the key" (ChatGPT: "Where to keep the sign-in"). The model, budget and effort form is inside `details.ai-more` "Model and spending". It opens automatically when settings are dirty or invalid. **Test access** shows only once connected.
- Brief labels: tab **Choices → Rules**, Edit/Remove choice → **Edit/Remove rule**, field **Choice → Rule**, "Keep this choice fixed" → **Never change this rule**, "Choice or design concern" → **Which rule?**, scope "Whole project" → **Whole site**, "Page · Request n" → **Page · Box n**.
- Selection toolbar for images: **Replace image** (hidden file input) and **Find with AI**. Other elements: **Edit**, **Ask AI**. The Replace dialog no longer offers Custom HTML.
- Pages open fitted (initial zoom isn't one of the ZOOM_LEVELS steps any more). The grid draws only inside the page.

## C4 / C5 / C9 wired (2026-10-20, Claude)

- **C4:** The AI provider list comes from native `providers`: OpenAI (API key), ChatGPT plan (sign in), Anthropic, Google, plus OpenRouter and Local as "coming soon". `set_ai_provider` is called on change, and OpenAI then sets key or ChatGPT authentication. Key labels and generation details name the job's provider. The OpenAI `store: false` wording only shows for OpenAI jobs.
- **C5:** A selected element with `data-desegnos-found` shows **Found online · host ↗** in the selection toolbar, which opens the source via `open_found_source`. Page check lists all found content under "Found online — check and swap if needed", with **Show** and **Source**. Found items don't count toward the status-bar chip.
- **C9:** The code pane is editable (textarea over coloured lines; no new dependency). Edits save 0.7 s after typing stops, on blur, or with Ctrl/Cmd+S, through `edit_page_source`. It sends only the changed language, using the current revision, and **only if that page's source still matches what the draft started from**; otherwise it shows "changed elsewhere · Reload latest code". Rejections mark lines and list up to three "Line n" links. Compacted output is shown tidied; the first edit saves that layout, after which `get_page_source_map` gives exact two-way selection (clicking in code selects the canvas element; selecting on canvas highlights its range). Tab inserts two spaces.
- Checked in the app: provider switching, an HTML edit saved and rendered, code click → `h1` selected, invalid JS refused with "Line 20", found badge and page-check group. Native `--ux-audit-only` (25) and `--source-edit-only` (8) still pass.

## Extra frontend features (owner-approved, 2026-10-19, Claude)

| Feature | Where | Notes for tests (X5) |
| --- | --- | --- |
| **First-run tips:** 3 dismissible tips over the canvas, shown once per device | `src/Onboarding.tsx` | Fresh test profiles show the card in the top-right of the canvas. Set `localStorage['desegnos.first-run-tips'] = 'done'` before canvas clicks there, or press **Skip tips**. |
| **Start screen and empty page:** side-by-side start cards, recent projects as cards, a "Connect now" note, and "This page is empty" with **Add request** | `src/NewProject.tsx`, `src/Canvas.tsx` | **Open project** is now a link-style button with the same text. The intro still matches `/describe.*generate.*edit.*export/`. |
| **Device preview:** Desktop / Tablet / Phone in Preview. Renders at the real width, scaled to fit | `src/Preview.tsx` (`PagePreview`) | "Enable interactions" was renamed **Try buttons and links**. |
| **Page check:** placeholders (image placeholders, placeholder/sample text) and basics (missing alt, links to nowhere, unlabeled buttons, H1 count, skipped heading levels, missing title). **Show** selects the element; **Ask AI to fix** adds a request with a fix prompt | `src/PageCheck.tsx`; status-bar chip `aria-label="Open page check"`; Menu → **Check page** | Static analysis of saved HTML with DOMParser (no scripts run). Contrast isn't checked. A native page-title fix would need a page-level request. |
| **Fill pictures by drop or browse:** drag a picture from the file manager onto any image on the canvas (it highlights first). Image placeholders show "Drop a picture here · or click to browse", and clicking opens the file picker. Placeholders are replaced with a fresh `<img>` that keeps their styling and description. Other images get `src` updated | `src/Canvas.tsx`, `src/Preview.tsx` (`imageAt`, `placeholders`), **`src-tauri/tauri.conf.json` `dragDropEnabled: false`** so the webview receives browser drop events | Placeholder overlays are `.image-drop-target` buttons above the canvas. `data-desegnos-placeholder` isn't in the native editable-attribute list, so the frontend uses `replace`. Codex: whitelisting its removal would allow a lighter edit (optional). |
| **Change history:** **History** menu next to Undo/Redo lists this session's changes with times; picking one undoes or redoes until the project matches | `src/useHistory.ts` | Matches by project fingerprint, so unlisted native undo steps are passed through safely. A backend `undoDepth`/labels in `WorkspaceView` would make labels exact (nice-to-have for Codex, not required). |

## Order

1. X5 (tests green again) and C1–C3 (the main window feels calm), in parallel.
2. X1 → X2 → C5 (real content, traceable).
3. X7 → C9 (split code view).
4. C4 + X4 (AI setup).
5. C6–C8, X3.

## Not doing now

Publishing, hosted accounts, chat-first generation, and removing any MVP feature listed above.
