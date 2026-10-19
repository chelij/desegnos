# Desegnos feature priorities

Date: 2026-10-19. Companion to [UX_AUDIT.md](UX_AUDIT.md). Product direction (Dreamweaver with AI, placement and control first, BYOK now and paid hosted generation later, all major AI providers, all platforms) is in [PRODUCT.md](PRODUCT.md). This file re-orders work; it does not replace [RELEASE_PLAN.md](RELEASE_PLAN.md) or relax its safety contracts.

## Why re-order

The release plan scopes 54–72 engineering weeks and builds infrastructure for every feature in parallel. Meanwhile the core value has not been proven: [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) records that live OpenAI generation, model behavior and placement fidelity are unverified, and no paid calls have been made.

Work in this order instead. Each tier should leave the app more useful on its own:

> **Tier 1:** a person can describe a page, get a good result, refine it, and take the website away.
> **Tier 2:** results look finished and work on every screen size.
> **Tier 3:** reach more users and save setup effort.

## Constraints that still apply

- **No sidebars.** Floating tool bars, popovers and dialogs only (see DESIGN.md).
- **No simulated results.** Every visible control does real work, as the release plan already requires.
- **Keep existing invariants:** project validation, transactional apply, Undo/Redo, recovery, and keys kept out of project files.
- **Keep checks proportional.** Each feature needs one native workflow test plus screenshots of its key states. Skip the full light/dark/760px matrix unless the feature changes layout.

## Summary

| # | Feature | Tier | Release plan today | Effort |
| --- | --- | --- | --- | --- |
| 1 | Real generation quality pass | 1 | Unverified gate | S–M |
| 2 | Export website folder | 1 | Stage 8 | M |
| 3 | Streaming results onto the canvas | 1 | Not planned | M |
| 4 | Variations | 1 | Not planned | M |
| 5 | Reference images in requests | 1 | Not planned | M |
| 6 | Visible version history | 1 | Stage 9 | M |
| 7 | Responsive preview and per-size requests | 2 | Stage 3 / responsive section | M |
| 8 | Global styles popover | 2 | Stage 4 (shared tokens) | S–M |
| 9 | Images: generated and stock | 2 | Stage 5 (approved images) | M |
| 10 | One-click publish | 2 | FEATURE_RESEARCH candidate | M–L |
| 11 | Additional AI providers | 2 | Not planned (plugin boundary exists) | M |
| 12 | Starter sections | 3 | Not planned | S |
| 13 | Split design / code view | 2 | Read-only source views only | M–L |

---

## Tier 1: make the core loop real

### 1. Real generation quality pass

**Why.** If a request box does not reliably become a good section, nothing else matters.

**Scope.**
- **Benchmark set.** Write `docs/GENERATION_BENCHMARK.md` with 15–20 fixed scenarios:
  - landing page hero
  - pricing table
  - contact form
  - two-column about section
  - a mobile page
  - an element revision ("make this button bigger and green")
  - a multi-request page in Fuzzy mode and the same page in Strict mode
  - a page that uses an imported identity
- **Runner.** Add a debug-only script that runs one benchmark project end to end through the normal generation path and saves the requirements, raw output and a screenshot to `test-results/benchmark/<date>/`.
- **Who runs it.** The project owner runs it with their own key; Codex must not make paid calls itself. Codex prepares the projects and the runner and reviews the results.
- **Iterate.** Change the requirements Markdown and system instructions based on what fails. Keep a short log of each change and its effect in the benchmark doc.

- **Placement fidelity score.** For each request box and the element generated for it, the runner records relative order, alignment, size ratio and containment (see PRODUCT.md, "Success measure"). This is the main quality number.

**Done when.**
- At least 80% of scenarios produce output the owner rates "looks like what I imagined".
- Placement in Strict mode matches the request boxes' relative order and grouping, and its fidelity score is recorded for every run.
- Each result's cost and duration are recorded.

### 2. Export website folder

**Why.** Users cannot currently take a website out of Desegnos.

**Scope.**
- **Commands.** **Menu → Project → Export website…** plus a top-bar **Export** button once any page has content.
- **Output.** Write to a chosen folder (ZIP optional):
  - `index.html` for the home page and `<path>/index.html` for each included page
  - one shared `styles.css`, plus a page stylesheet where needed
  - page scripts as `.js` files
  - embedded images written out as real files under `assets/`
- **Linking.** Internal links resolve relatively, so the folder works from `file://` and from a web server at the root or a subdirectory.
- **Exclusions.** Hidden/draft pages are left out. Prompts, the brief, history, generation records and credentials are never exported.
- **Afterwards.** Show a notice with **Open folder**.
- This is the plan's "portable web export" in its minimal form. React export stays deferred.

**Done when.**
- A multi-page generated project exports.
- The output opens and navigates correctly in Firefox and Chromium from disk and from a local static server.
- The native test verifies the file list and that no private data appears in the output.

### 3. Streaming results onto the canvas

**Why.** A blurred box and a timer for 30–90 seconds feels broken. Seeing the page build feels fast.

**Scope.**
- **Partial preview.** While the response streams, parse partial HTML/CSS into a read-only preview layer over the affected areas. Use a sanitized, scriptless frame and keep scripts paused.
- **Status.** Show the progress strip from UX audit item 5 with the current stage.
- **Apply unchanged.** The final result still goes through the existing validation and transactional apply. The streamed preview is never saved, and Cancel discards it.
- **Malformed partials.** If partial markup can't be parsed, keep the last good frame rather than flickering.

**Done when.**
- During a controlled-HTTP streamed generation, the canvas visibly updates at least several times before completion.
- Cancel leaves the project byte-identical to before.
- The final applied source equals what the non-streaming path would have produced.

### 4. Variations

**Why.** AI output varies, and choosing between versions is faster than re-prompting. This suits Fuzzy mode.

**Scope.**
- **Request.** The Generate button dropdown gets **Variations: 1 / 2 / 3** (default 1). The cost estimate multiplies accordingly and is shown before sending.
- **Generation.** Each variation is a separate candidate from the same frozen requirements.
- **Choosing.** Show results as a small row of thumbnails on the canvas strip. Clicking one previews it in place; **Use this** applies it through the normal path. The others stay in history (item 6) and are not discarded silently.
- **Charges.** If a candidate fails, the others remain usable, and each request's charge is shown honestly.

**Done when.**
- Generating 3 variations yields 3 previewable candidates.
- Applying one is a single undoable change.
- The two unchosen candidates can still be applied later from history without another request.

### 5. Reference images in requests

**Why.** "Make it look like this" is the most natural way to point and describe. It fits the product better than any planned feature.

**Scope.**
- **Attaching.** Paste or drop an image onto a request box, or use a small **Attach image** button on the box. Show thumbnails inside the box, with a remove action. Limit: 3 images per request.
- **Sending.** Images go to the model as vision input with the request. The requirements Markdown records each image's hash and purpose.
- **Purpose.** Choose per image:
  - **Style reference** (default): it guides the look and nothing is copied.
  - **Use this image**: the image itself is placed on the page.
- **Storage.** Images are kept in the project archive using the existing local-image limits (PNG/JPEG/WebP/GIF/SVG, 512 KB, scaled down if larger).

**Done when.**
- A request with a screenshot of a reference hero produces a visibly related layout.
- "Use this image" places that exact image on the page.
- The images survive save, reopen and recovery.

### 6. Visible version history

**Why.** AI edits are large and destructive. People need to see and restore earlier versions, not press Undo 30 times. Undo history today is lost on restart.

**Scope.**
- **Checkpoints.** Create one automatically on each applied generation, each direct edit session (debounced), and on Save. Users can also create one with a name.
- **History popover.** A **History** button opens a popover (not a sidebar) of thumbnails with time, label, and generation summary.
  - Clicking a thumbnail previews that version read-only.
  - **Restore** applies it as a new forward revision, so restoring is itself undoable.
- **Persistence.** Store checkpoints in the project archive and keep them across restarts. Limit to the latest 50 automatic checkpoints plus all named ones.
- **Variations.** Unchosen variations from item 4 appear here too.
- This is the plan's persistent checkpoint / history UI (stage 9), pulled forward with a reduced first scope: no compare view yet.

**Done when.**
- After generate → edit → generate → quit → reopen, all three states are visible.
- Any state can be restored, and the restore can be undone.

---

## Tier 2: make results look finished

### 7. Responsive preview and per-size requests

**Scope.**
- **Size toggle.** A **Desktop / Tablet / Mobile** toggle on the canvas tool bar re-renders the accepted site at 1440 / 768 / 390 px inside the screen guide.
- **Per-size requests.** A request created while a size is active is tagged with that size ("At mobile: stack these cards"). The requirements include the breakpoint, and the result is limited to a media query for that size.
- **Breakpoints.** Only the plan's default breakpoints. Breakpoints that users add come later.

**Done when.**
- A generated desktop page can be fixed at mobile with one request, without changing desktop rendering.
- Pixel comparison of the desktop output before and after the mobile fix shows no change.

### 8. Global styles popover

**Scope.**
- **Popover.** A **Styles** button opens a popover with brand colors (primary, accent, background, text), heading and body fonts from a bundled set, and corner-radius and spacing scale presets.
- **Source of values.** Values come from identity import when available and are stored as project tokens.
- **Use in generation.** Every generation receives the tokens as authoritative and emits CSS custom properties, so changing a token updates existing pages without a new request.

**Done when.** Changing the primary color in the popover recolors every generated page immediately, with no AI request, and the change can be undone.

### 9. Images: generated and stock

**Scope.**
- **New sources.** In the image picker (Edit/Replace dialog and requests), add **Generate image**, which uses the configured provider with a visible cost, and **Search free photos**, which uses one stock provider with a commercial-use license such as Openverse or Unsplash.
- **Requests.** A request may say "use a generated image". The plan's "approved images" flow requires confirming the cost before sending.
- **Storage.** Images are written into the project. Attribution, where the license requires it, is recorded and included in the export.

**Done when.**
- A page can get a generated hero image and a stock photo.
- Both survive save, reopen and export, and stock attribution appears in the exported files.

### 10. One-click publish

**Scope.**
- **Target.** One provider end to end first. Recommended: Cloudflare Pages or Netlify, which have direct-upload APIs and need no git.
- **Flow.** Connect the account with the token in the OS keyring, the same pattern as AI keys. **Publish** uploads the item 2 export and shows the live URL.
- **Redeploys.** Republishing updates the same site. Show the last published time and revision.
- **Scope limits.** Staging, rollback, custom domains and redirects stay deferred (see FEATURE_RESEARCH).

**Done when.**
- A real project publishes to a real URL that loads externally.
- Republishing after an edit updates it.
- No token appears in the project, logs or export.

### 11. Additional AI providers

**Why.** The product targets every major AI provider (PRODUCT.md). Paid hosted generation will later be one more adapter, so the boundary must be provider-neutral now.

**Scope.**
- **Adapters.** Add provider adapters behind the existing `desegni-ai` boundary. Start with Anthropic (Claude) and OpenRouter, then Google. A local OpenAI-compatible endpoint (Ollama, LM Studio) is optional.
- **Capabilities.** Each provider declares its capabilities (vision, image generation, streaming). Features 3, 5 and 9 ask for a capability, not a provider.
- **Behavior.** Each adapter uses the same frozen requirements, streaming, cancellation and cost estimate. Show an estimate only where pricing is known.
- **Settings.** AI settings gains a provider choice. The model list comes from the verified catalogue for each provider.

**Done when.**
- The benchmark from item 1 runs through at least one non-OpenAI provider with results logged.
- Switching providers never redirects a job that has already been prepared.

### 13. Split design / code view

**Why.** Dreamweaver's defining feature, and the strongest form of "more control" (PRODUCT.md).

**Scope.**
- **Code view.** A **Design / Split / Code** toggle in the top bar. Split shows the canvas and the page source together, divided either top/bottom or side by side. It is a view mode, not a docked panel, so it is consistent with the no-sidebar rule.
- **Two-way selection.** Selecting an element on the canvas highlights its source, and placing the cursor in the source selects the element.
- **Editing source.** Edits to HTML, CSS and JS go through the same validation and transactional apply as direct edits. Invalid source shows an inline error and is not applied. Undo/Redo cover source edits.
- **Requests stay linked.** AI requests still target elements by source fingerprint, and a source edit updates those fingerprints.
- **Step 1:** syntax highlighting and two-way selection, read-only.
- **Step 2:** editing.

**Done when.**
- Selecting a button on the canvas scrolls to and highlights its markup.
- Editing its text in code updates the canvas after validation.
- Undo restores the previous source byte-for-byte.

---

## Tier 3: reach and convenience

### 12. Starter sections

**Scope.**
- **Insert menu.** **Add request → Starter** offers Hero, Features, Pricing, Testimonials, FAQ, Contact and Footer.
- **What a starter is.** Each inserts a request box with a pre-written, editable prompt and a sensible default size. It is not pre-built HTML, so the result still follows the project's identity and styles.

**Done when.** A user can build a complete landing page from starters and Generate in under two minutes, without writing a prompt.

---

## Deferred until Tier 1 and Tier 2 are complete

These release-plan and FEATURE_RESEARCH items remain valid but should not take effort before then:

- React export and JSX compilation
- the qualified Radix component catalogue
- executable plugins
- content collections / CMS
- multilingual sites
- visitor search
- client review packages
- redirects
- animation authoring
- the full layer organizer
- user-added breakpoints
- signed installers, notarization and auto-updates
- Windows/macOS certification

Revisit the order after the item 1 benchmark: if generation quality is poor, more prompt and requirements work comes before items 3–12.
