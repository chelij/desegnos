# Desegnos: UI verification

## Four-icon selection, right-click menus and labelled drafts: PASS

19 October 2026. The final build passes [24 focused native steps](../test-results/native/2026-10-19T14-38-06-989Z-1319453/report.json) and [25 authoring regression steps](../test-results/native/2026-10-19T14-36-35-874Z-1313744/report.json), with trusted OS input on guarded private displays. Selection shows Layers/Copy/Cut/Delete icons at its top-left, Layers remains anchored at 760px in both themes, and right-click opens context-appropriate actions without a sidebar. Copy/Cut/history manipulate actual source without AI calls. Keyboard focus and Escape remain usable. Dialog More/Advanced menus remain visible outside dialog bounds via native browser popovers.

The primary agent inspected the actual [unclipped dialog menu](../test-results/native/2026-10-19T14-38-06-989Z-1319453/layers/generation-unclipped-more-menu.png), [four-icon anchored Layers](../test-results/native/2026-10-19T14-38-06-989Z-1319453/layers/selection-four-icons-layers.png), and [downloaded picture with a clearly labelled failed-image placeholder](../test-results/native/2026-10-19T14-38-06-989Z-1319453/layers/downloaded-image-labelled-draft.png). The exported files also render in ordinary Chromium with external traffic blocked. Generated sample content and the synthetic provider are explicit fixtures, not claims about model quality or authentic customer work. The editor's existing neutral palette and layout remain in place. The release startup and core/adapter boundaries are recorded in [implementation](IMPLEMENTATION.md#selection-menus-and-complete-drafts-19-october-2026).

## Supplied identity, direct controls and project creation: PASS

19 October 2026. The [final combined native run](../test-results/native/2026-10-19T10-54-29-820Z-715292/report.json) passes 100 steps on a guarded isolated display. The primary agent inspected the actual [corner controls and wordmark](../test-results/native/2026-10-19T10-54-29-820Z-715292/generation/corner-resize-icon-toolbar-window.png), the [identity review](../test-results/native/2026-10-19T10-50-15-852Z-693764/identity/identity-review-window.png) and the [fresh normal startup](../test-results/startup/2026-10-19T10-46-42-318Z-676675/startup.png).

### Hard Gate: PASS

The new-project choice precedes name/platform/resolution; imported projects start empty and light. Trusted native input verifies direct drag, four anchored corners, Alt/free movement, configured snapping, Escape cancellation, one-step Undo, near-45-degree resize, icon toolbar alignment, contextual Edit/Generate and Space activation. Prompt fields retain native editing menus; generated inputs and links use the app menu. Native pickers, approved identity/logo persistence and forced-crash recovery pass without source changes or provider replay. No browser errors or unhandled rejections occur.

### Purpose-Gate: PASS

The supplied monochrome wordmark, local Archivo headings, neutral grey surfaces, functional blue, dashed requests and square guides preserve the canvas-first layout. There are no sidebars. The on-canvas icon toolbar has accessible names and tooltips. Files, uncertainty and logo choices support explicit identity approval; technical state stays in nested menus.

### Liveliness: PASS

Interaction feedback is short and respects reduced motion. Generation uses static progress feedback with real status/elapsed state, without a looping decorative animation. The actual reference assets remain unchanged.

### Craftsmanship and Quality Locks: PASS

All 450 measured contrast comparisons across 18 existing editor/settings/progress contexts pass, including both themes at 760px. Rust (244), geometry/shortcuts (32), strict Clippy, formatting, frontend and normal debug/release builds pass. Source and portable-logo bounds are native/core checks. The [implementation record](IMPLEMENTATION.md#brand-direct-controls-and-identity-import-19-october-2026) and [identity contract](IDENTITY_IMPORT.md) state limits. Live AI identity quality, other operating systems, screen-reader, broader input-method and DPI checks are not claimed.


## Desegnos interactions: PASS

19 October 2026. The [full native run](../test-results/native/2026-10-19T08-23-45-572Z-314737/report.json) passes 84 steps, including actual Desegnos window/title/wordmark, trusted pan/draw/wheel/keyboard input, text versus project undo, abandoned request cleanup, help/Escape, tab cycling, synthetic generation and crash recovery. The primary agent inspected the actual [shortcut-help window](../test-results/native/2026-10-19T08-23-45-572Z-314737/desegnos-shortcuts-window.png). Tests use a guarded virtual display and preserve the desktop connection and display resources. Grey chrome, no-sidebars layout and inline prompts remain in place. See [the current implementation record](IMPLEMENTATION.md#desegnos-interactions-19-october-2026) for scope and unverified platforms.

The following dated sections retain historical evidence for earlier builds.


## Item-correlation diagnostics: PASS

19 October 2026. Scope: four static diagnostic rows behind the existing Technical menu. The quiet grey editor, page appearance, canvas controls and authoring layout are unchanged. Final [native evidence](../test-results/native/2026-10-19T06-00-28-803Z-68141/report.json) passes 39 steps. The primary agent inspected both [light](../test-results/native/2026-10-19T06-00-28-803Z-68141/chatgpt/correlation-light-window.png) and [dark](../test-results/native/2026-10-19T06-00-28-803Z-68141/chatgpt/correlation-dark-window.png) actual-screen captures after private-display/window ownership checks.

### Hard Gate: PASS

- R-03, R-17, R-18, R-23, R-24, R-25, R-28, R-34, R-36 PASS: both diagnostic windows fit at 760px without horizontal overflow. Existing real Tab/Escape/focus behavior passes. [550 measured contrast pairs](../test-results/item-correlation-2026-10-19/contrast.json) across 22 contexts meet 4.5:1 text and 3:1 control/guide bounds, with minima 4.970071:1 and 3.111086:1. All 198 checked chrome/text/border/request-surface colors are neutral grey.
- R-26, R-27, R-32 PASS: rows display the actual native error cause, counts and indices. Reading them submits no request. The controlled missing-part reply fails without changing source or dossier; valid sparse replies still reach review and Apply. The correlation object survives restart and retry archival in coordinator tests.
- R-33, R-35, R-37 PASS: product code supplies the rows, without injected styling or simulated state. Required builds, 180 Rust tests, strict lint and format checks pass. Unchanged canvas/guide/startup behavior reuses the applicable baseline below.

### Purpose-Gate: PASS

- R-01, R-07 PASS: no new gradient, glow or decorative surface appears. Rows use the existing definition list.
- R-04, R-06, R-08, R-09 PASS: static labels translate fixed validation enums; numbers identify the failed stream position. Technical data stays inside the existing technical menu, with no new sidebar, control or icon.
- R-10, R-12, R-13, R-14, R-19, R-22 PASS: no imagery, glass, feature card or animation was introduced. Grey surfaces keep the working page dominant.

### Liveliness: PASS

- Dials PASS: ENERGY 1 / RHYTHM 2 / MOTION 1 remains consistent with [DESIGN.md](../DESIGN.md).
- Focus and rhythm PASS: the new rows use existing spacing, typography and scrollable dialog structure; real generation status and primary actions retain their hierarchy.
- Accent and identity PASS: existing blue focus/actions/placement guides and neutral chrome remain intact. Request appearance continues to follow the page's independent light/dark choice.
- Design Read PASS: the change serves diagnosis of an actual failed generation, with a controlled negative fixture and inspected final captures.

### Craftsmanship and Quality Locks: PASS

- C-1, C-3, R-05, R-11, R-20, R-21, R-29, R-30, R-31 PASS: existing semantic structure, light/dark palette, control sizes and radius hierarchy remain. The Photoshop reference continues to inform grey chrome; no copied panel layout was added.
- C-2, C-4, R-15, R-16 PASS: singular/plural labels describe real counts, hidden by default. Correlation values contain no raw model text or message IDs. No placeholder, promotional copy or new dependency appears.
- C-5 PASS: this proves controlled native response handling, not live account success. [Browser errors](../test-results/native/2026-10-19T06-00-28-803Z-68141/browser-errors.json) and [recovery errors](../test-results/native/2026-10-19T06-00-28-803Z-68141/recovery-browser-errors.json) are empty. [Credential scanning](../test-results/native/2026-10-19T06-00-28-803Z-68141/credential-nondisclosure.json) and [desktop resource/connection cleanup](../test-results/native/2026-10-19T06-00-28-803Z-68141/desktop-cleanup.json) pass. A fresh private user generation remains the explicit live check.

## Completion and grey palette: PASS

19 October 2026. The primary agent verified the requested neutral Photoshop-style grey palette and completed-response correction with [81 full native steps](../test-results/native/2026-10-18T18-05-30-171Z-1459854/report.json) and [38 focused generation steps on the final binary](../test-results/native/2026-10-18T18-07-02-333Z-1467717/report.json). All windows ran on guarded private display `:90`; no main-monitor interaction or paid OpenAI call occurred. The final instrumented binary SHA-256 is `831a745670ce3d35fd43ec4883a1cdf05627d8d490460723b18b3d6502867cf1`. The full authoring run predates only the final joined-text credential check; the final run verifies generation/application/recovery after that change. The historical baseline reports below describe their original palettes; this section and DESIGN.md define the current grey palette.

### Hard Gate: PASS

- R-02, R-17, R-18, R-23, R-24, R-28, R-36, R-38 PASS: the change adds no em dash, invented metric, testimonial, asset, navigation destination or FAQ. Diagnostics render actual native counters and closed rejection codes through static labels. Saka and provider output in evidence are explicit controlled fixtures.
- R-03, R-34 PASS: both grey themes fit the supported 760px window. The primary agent inspected actual-screen captures of the [light editor](../test-results/native/2026-10-18T18-05-30-171Z-1459854/editor-light-screen.png), [dark editor](../test-results/native/2026-10-18T18-05-30-171Z-1459854/editor-dark-screen.png), [light validation details](../test-results/native/2026-10-18T18-07-02-333Z-1467717/generation/validation-light-window.png) and [dark validation details](../test-results/native/2026-10-18T18-07-02-333Z-1467717/generation/validation-dark-window.png). Both canvas text boxes and grips paint correctly; diagnostic values/actions remain inside their scrollable dialog. Website appearance remains independent.
- R-25 PASS: [500 unrounded contrast comparisons across 20 contexts](../test-results/completion-grey-2026-10-19/contrast.json) pass. Minimum normal text is 4.970071:1; minimum measured non-text is 3.111086:1. [Ten guide/background comparisons](../test-results/completion-grey-2026-10-19/guide-contrast.json) pass with minimum 3.103750:1. The same evidence verifies 180 base-surface/text/border/request-surface RGB comparisons are neutral grey, including valid three-digit CSS notation.
- R-26, R-27, R-32 PASS: existing controls retain real Rust operations. The native runs cover empty canvas, prepare/send/wait/receive/cancel, candidate review/application, explicit retries, rejection details, menus/tabs, keyboard placement, six zooms, Escape/focus restoration and restart recovery. Reading validation details makes no provider request; invalid output preserves source/dossier.
- R-33, R-35, R-37 PASS: colors and behavior are implemented in product sources, not test-injected styling. The user's grey direction is recorded in [DESIGN.md](../DESIGN.md) before verification, with ENERGY 1 / RHYTHM 2 / MOTION 1. Final normal/instrumented builds, 175 Rust tests and strict lint/format checks pass. The [implementation record](IMPLEMENTATION.md#completion-and-grey-palette-baseline-verification) identifies their actual evidence and binaries.

### Purpose-Gate: PASS

- R-01, R-07 PASS: only the requested functional grid uses line gradients; grey surfaces keep attention on the page. No ornamental gradients or glows were introduced. Grid/ruler/guide authoring passes through actual native interaction.
- R-04, R-06, R-08, R-09 PASS: existing carets and move/resize glyphs identify implemented actions. System typography remains readable; monospace remains confined to source/technical values. Real status labels and one validation row report actual state.
- R-10, R-12, R-13, R-14, R-19, R-22 PASS: no glass, decorative illustration, floating feature cards or new animation appears. Existing temporary-dialog elevation and direct control feedback remain appropriate. The palette and static diagnostic row add no decorative treatment or dependency.

### Liveliness: PASS

- Dials PASS: ENERGY 1 / RHYTHM 2 / MOTION 1 remains consistent with a quiet authoring workspace.
- Focus and rhythm PASS: the page and on-canvas requests remain the focal point; compact chrome and temporary dialogs provide structural spacing.
- Accent and identity PASS: a restrained blue distinguishes primary actions, selection, keyboard focus and placement guides against neutral grey surfaces. The percentage rulers and inline requests remain the repeated authoring motif. This is the user's palette direction applied to Desegnos's existing layout.
- Design Read PASS: DESIGN.md records the grey-chrome purpose, independent page appearance, guide colors and unchanged interaction before delivery.

### Craftsmanship and Quality Locks: PASS

- C-1, C-3, R-05, R-11, R-20, R-21, R-29, R-30, R-31 PASS: the user's Photoshop reference informs grey color only. No panels, branding or copied navigation were added. Existing no-sidebar layout, radius hierarchy and light/dark choice stay functional. Red remains limited to actual error/destructive states; blue carries the deliberate action accent.
- C-2, C-4, R-15, R-16 PASS: actionable labels, keyboard behavior, all generation states and responsive dialogs remain real. A specific failed check replaces the generic malformed explanation when recorded; no new promotional copy or filler section appears.
- C-5 PASS: the original live reply's exact failed check remains unknown. Controlled HTTP/OAuth establishes response handling without implying live account success. Final [browser errors](../test-results/native/2026-10-18T18-07-02-333Z-1467717/browser-errors.json) and [recovery errors](../test-results/native/2026-10-18T18-07-02-333Z-1467717/recovery-browser-errors.json) are empty; [desktop cleanup](../test-results/native/2026-10-18T18-07-02-333Z-1467717/desktop-cleanup.json) preserves resources and connection. [50 synthetic credentials across 150 final-run owned files](../test-results/native/2026-10-18T18-07-02-333Z-1467717/credential-nondisclosure.json) pass nondisclosure. Rust additionally checks a credential split between final text parts for both authentication methods.

The first two native failures were test assumptions: shorthand RGB notation and opening a new prepared job instead of the failed job's status. Both are corrected; their runs remain available. Actual-screen captures are used for visual conclusions because a window drawable can omit native control layers. The harness verifies the private display/PID/window before capture. No product rendering hack was added.

## Stream-routing correction: PASS

19 October 2026. The primary agent verified [38 focused native generation/account/recovery steps](../test-results/native/2026-10-18T17-21-59-558Z-1333875/report.json) on guarded private display `:90`, using controlled HTTP/OAuth and synthetic credentials. Native test binary SHA-256 is `441eff74c9e97e4baf1beb2ac2994feafc5eae9e7a485683b0a2a65ec5c2cd7e`. A valid headerless stream larger than 65,536 bytes was generated, reviewed and applied through real Rust commands. This proves the corrected routing and desktop workflow; the original live reply was not retained.

- Hard Gate PASS: the changed partial-body count and Read limit row fit the supported 760px dialog in both themes. All 450 contrast comparisons across 18 current contexts pass: normal text is at least 4.678582:1 and non-text at least 3.123300:1. Values come from actual reader state. [50 synthetic credentials across 144 owned files](../test-results/native/2026-10-18T17-21-59-558Z-1333875/credential-nondisclosure.json) pass nondisclosure; raw bodies and echoed bearer text remain absent. No new controls, colors, typography or assets were introduced. The unchanged authoring/keyboard/theme rules retain their baseline evidence below.
- Purpose-Gate PASS: the existing Technical detail rows explain which reply was received and which reader bound was reached. A partial byte count describes consumed data without inventing a total response size. The existing menu keeps these diagnostics out of the default overview. No decorative element or dependency was added.
- Liveliness PASS: ENERGY 1 / RHYTHM 2 / MOTION 1 and the established design read remain applicable. The actual generation state stays the focal point; additional values reuse the existing type hierarchy and spacing.
- Craftsmanship and Quality Locks PASS: large valid headerless output reaches the normal validator and applies exact source; oversized HTML diagnostics retain an uncertain outcome with the correct limit and leave project source/dossier intact. No request is sent when reading details. Explicit retry preserves frozen requirements/mode/account. Existing cancellation, account binding, validation and fresh-process recovery checks pass. [Browser errors](../test-results/native/2026-10-18T17-21-59-558Z-1333875/browser-errors.json), [recovery errors](../test-results/native/2026-10-18T17-21-59-558Z-1333875/recovery-browser-errors.json) and [desktop cleanup](../test-results/native/2026-10-18T17-21-59-558Z-1333875/desktop-cleanup.json) pass.

The primary agent inspected the changed owned-window captures in [light](../test-results/native/2026-10-18T17-21-59-558Z-1333875/chatgpt/non-stream-html-light-window.png) and [dark](../test-results/native/2026-10-18T17-21-59-558Z-1333875/chatgpt/non-stream-html-dark-window.png). The full diagnostic values and actions paint correctly within the dialog. The first native run is retained separately: it had already accepted/applied the valid large reply, then failed an assertion that assumed thousands separators. The corrected test permits both locale renderings while checking the exact native integer. This required no product change. See the [implementation record](IMPLEMENTATION.md#stream-routing-correction-baseline-verification) for 168 passing Rust tests, normal build hashes and remaining live-check limits.

## Response-format correction: PASS

19 October 2026. The primary agent verified the correction with [38 focused native generation/account/recovery steps](../test-results/native/2026-10-18T17-02-04-997Z-1293546/report.json) on private display `:90`, using synthetic credentials and controlled HTTP/OAuth servers. Native test binary SHA-256 is `2e42f9f9aea562dc97551160873f73a2dc3970d2eb9e382e88f42c6efee7de8b`. The original live response body was discarded by the previous build; this gate proves response handling and diagnostics, not resolution of the unobserved OpenAI rejection.

- Hard Gate PASS: the new diagnostic rows fit the supported 760px window in both themes. All 450 contrast checks across 18 current generation/settings contexts pass; normal text is at least 4.678582:1 and non-text at least 3.123300:1. Counts/status/IDs derive from actual native replies. Raw provider bodies and echoed bearer text never appear. Valid explanations without error codes remain readable as bounded plain text; credential-like or unsafe explanations are withheld. [50 synthetic credentials across 144 owned files](../test-results/native/2026-10-18T17-02-04-997Z-1293546/credential-nondisclosure.json) pass nondisclosure. No new controls, typography, colors or assets were introduced. Unchanged authoring/keyboard/theme rules retain the baseline evidence below.
- Purpose-Gate PASS: ordinary definition rows reuse the established detail style because users need to identify a failed response. The existing Technical menu keeps those details out of the default overview. No decorative treatment or dependency was added.
- Liveliness PASS: ENERGY 1 / RHYTHM 2 / MOTION 1 and the established design read remain applicable. The real error state stays the focal point; diagnostic rows use the existing spacing, amber action family and type hierarchy.
- Craftsmanship and Quality Locks PASS: JSON errors with and without codes and unexpected HTML fixtures produce distinct honest outcomes. Reading details makes no inference request; retry requires confirmation, retains the exact Markdown/mode/account, and only accepts a subsequent validated completed stream. Native restart preserves source/history without repeating requests. [Browser errors](../test-results/native/2026-10-18T17-02-04-997Z-1293546/browser-errors.json), [recovery errors](../test-results/native/2026-10-18T17-02-04-997Z-1293546/recovery-browser-errors.json), and [desktop cleanup](../test-results/native/2026-10-18T17-02-04-997Z-1293546/desktop-cleanup.json) pass.

The primary agent inspected owned-window captures of [JSON error, light](../test-results/native/2026-10-18T17-02-04-997Z-1293546/chatgpt/non-stream-error-light-window.png), [JSON error, dark](../test-results/native/2026-10-18T17-02-04-997Z-1293546/chatgpt/non-stream-error-dark-window.png), [unexpected HTML, light](../test-results/native/2026-10-18T17-02-04-997Z-1293546/chatgpt/non-stream-html-light-window.png), and [unexpected HTML, dark](../test-results/native/2026-10-18T17-02-04-997Z-1293546/chatgpt/non-stream-html-dark-window.png). All diagnostic labels/values and actions paint correctly and remain inside the dialog. Provider explanations without codes were inspected in [light](../test-results/native/2026-10-18T17-02-04-997Z-1293546/chatgpt/non-stream-message-light-window.png) and [dark](../test-results/native/2026-10-18T17-02-04-997Z-1293546/chatgpt/non-stream-message-dark-window.png). The earlier failed native run is retained separately; its test had explicitly opened Technical details before asserting the default overview. The test now selects Overview before sending; no product behavior changed for that setup mistake.

## Workspace revision baseline

18 October 2026. **PASS** for the implemented Linux desktop editor revision. The primary agent ran the integrated checks and reviewed the actual native screenshots. The four delivery blocks below cover the free workspace, page tabs, nested menus, percentage guides/grid, automatic dossier and visible generation workflow. This is a development-build gate, not the complete 1.0 release.

[80 native steps](../test-results/native/2026-10-18T16-31-22-849Z-1237999/report.json) pass through actual Rust commands, WebKitGTK, trusted pointer/keyboard input, native file pickers, controlled provider/OAuth servers and a fresh process after SIGKILL. [Browser errors](../test-results/native/2026-10-18T16-31-22-849Z-1237999/browser-errors.json) and [recovery errors](../test-results/native/2026-10-18T16-31-22-849Z-1237999/recovery-browser-errors.json) are empty. [Desktop cleanup](../test-results/native/2026-10-18T16-31-22-849Z-1237999/desktop-cleanup.json) passes resource and connection preservation. All test windows used guarded, task-owned virtual displays. No real credentials or paid generation were used.

[Contrast evidence](../test-results/workspace-revision-contrast.json) contains 300 current native comparisons across 12 light/dark settings, progress, candidate and account-limit contexts. Minimum normal text contrast is 4.678582:1; minimum non-text contrast is 3.123300:1; minimum measured focus contrast is 4.452815:1. Assertions use unrounded values. Six additional source-checked guide/background combinations exceed 3:1. The [six-zoom record](../test-results/native/2026-10-18T16-31-22-849Z-1237999/zoom-legibility.json) establishes 14px request text, 28px grips and 3px inset keyboard focus. These are editor checks, not accessibility certification for arbitrary generated websites.

The [design direction](../DESIGN.md) records the user's empty-workspace interaction and ENERGY 1 / RHYTHM 2 / MOTION 1. The dependency manifests and locks [match the named baseline](../test-results/workspace-revision-dependencies.json).

## Hard Gate: PASS

- R-02 PASS: current editor-authored frontend text contains no em dash; literal user requests and displayed source remain unchanged.
- R-03 PASS: the supported 760px desktop window, menus, settings, progress strip and review dialogs stay within the viewport in both themes. Deliberate workspace/source scrolling remains contained.
- R-17 PASS: counts, placements, job state, elapsed time, revisions and digests derive from actual native state; requirements and project-details checks compare the displayed values with Rust.
- R-18 PASS: no testimonials or fictional customers appear in the product. Saka and Native fixture content are explicit test inputs.
- R-23 PASS: the user requested nested navigation and page tabs. Menu carets and existing movement/resize SVGs describe real actions; this revision adds no imagery, avatars or branding assets.
- R-24 PASS: page tabs select existing native pages; menu entries open implemented operations. Duplicate/invalid routes are rejected and deliberate page removal is undoable.
- R-25 PASS: all 300 current unrounded contrast comparisons pass. Guide colors pass the six standard page/workspace backgrounds. Request focus is inset in its opaque surface, preserving contrast when page/editor themes differ.
- R-26 PASS: revised menus, tabs, grid controls, dossier review, technical views and generation controls perform real operations. Sending attaches the exact reviewed file; cancellation, retries, Apply/Discard and recovery have checked outcomes.
- R-27 PASS: the empty project, loading, waiting before response bytes, receiving, complete/conflicting output, stale work, credential failures and account limits have actual visible states. Failed output preserves authored source.
- R-28 PASS: no FAQ exists in the editor.
- R-32 PASS: trusted input checks cover canvas Enter, textarea Escape, rapid movement/resize, Tab focus, page-tab arrows/Home/End, nested menu navigation and nested-dialog Escape/focus restoration. Six zooms preserve visible focus and usable grips.
- R-33 PASS: product behavior is implemented in the Rust/React/CSS sources. The native harness drives and observes that implementation; it supplies no runtime styling or replacement project model.
- R-34 PASS: actual light/dark screenshots and state comparisons pass. The final renderer projects world positions directly into screen pixels instead of transforming the entire workspace. Owned-window captures verify that both populated requests remain visible after the six-zoom cycle and theme changes; WebKit snapshots alone were insufficient evidence for this failure. Website appearance stays independent of the editor theme.
- R-35 PASS: frontend, normal debug/release and instrumented builds pass, alongside 159 Rust tests, 26 geometry checks and 80 native steps. The control record below covers the revised interface and established workflow. Error-only alternatives retain source-backed handlers and their narrower coverage is stated below.
- R-36 PASS: the UI makes no fabricated security, compliance, performance or customer claims. Progress uses actual stages/time, with no invented completion percentage.
- R-37 PASS: the user's directions and DESIGN.md establish the interaction, layout, palette and dials before this revision's delivery.
- R-38 PASS: startup remains blank; the controlled fixture sites/accounts are identified as test content. They are not presented as live OpenAI output or initial product examples.

## Purpose-Gate: PASS

- R-01 PASS: CSS line gradients draw the requested placement grid; there are no ornamental gradients or glows.
- R-04 PASS: movement/resize grips and menu carets communicate their concrete operations; no generic AI icon set was introduced.
- R-06 PASS: system typography serves control readability; monospace is confined to real source, Markdown and technical data.
- R-07 PASS: the grid supports requested placement/snapping, starts on for new projects, and has persisted pixel/percentage settings.
- R-08 PASS: arrows/carets indicate actual submenu or resize behavior; action labels have no decorative arrows.
- R-09 PASS: Draft, Applied, Undone and Fixed labels represent real dossier/protection state, not promotional badges.
- R-10 PASS: no glass effects are used.
- R-12 PASS: dialog elevation identifies the active temporary task; the workspace and screen guide do not float as decorative cards.
- R-13 PASS: no glow is used. The dirty marker reflects actual unsaved state and carries an accessible label.
- R-14 PASS: dossier records show actual choices, rationale and feedback; there is no filler feature-card grid.
- R-19 PASS: motion is limited to direct control feedback; the ticking timer reflects a real operation, stops at completion and is not a live announcement.
- R-22 PASS: no generic illustrations are used.

## Liveliness: PASS

- Dials PASS: ENERGY 1 / RHYTHM 2 / MOTION 1 are explicit in DESIGN.md.
- Consistency PASS: quiet shell controls, browser-style tabs, an open workspace and temporary dialogs follow those dials in both themes.
- Focal point PASS: startup focuses the project form; authoring centers the user's request/site; review centers the generated site and Apply action.
- Whitespace PASS: blank space is the editable workspace; the screen outline guides scale without limiting placement.
- Accent PASS: amber marks selection, placement guidance, focus and the primary action. Error color identifies real failures or destructive actions.
- Identity motif PASS: users point by placing plain prompt boxes where content belongs, consistently across page tabs.
- Design Read PASS: the accepted direction and each major visual purpose are recorded in DESIGN.md and the release/plugin contracts.

## Craftsmanship and Quality Locks: PASS

- C-1 PASS: DESIGN.md explains the workspace, guide, type, grid, menus, palette, focus and motion choices.
- C-2 PASS: controls have implemented operations; the native record below exercises the changed surfaces plus actual provider/file workflows. No visual placeholder represents an unfinished feature.
- C-3 PASS: the shell, page tabs, workspace and optional dialogs serve authoring tasks; there are no promotional or filler sections.
- C-4 PASS: checked empty/loading/error states, both themes, 760px layout, six zooms and trusted keyboard workflows hold together. Broader assistive-technology/platform coverage remains outside this Linux gate.
- C-5 PASS: displayed facts come from project/provider/native state; fixture claims are labeled test content and no model-quality or performance guarantee is made.
- R-05 PASS: the full workspace and compact shell follow the requested authoring flow and RHYTHM 2, rather than a landing-page template.
- R-11 PASS: small control radii and tab corners differ from the square screen guide; pills are not a default layout device.
- R-15 PASS: Create project, Generate, Send and generate, Apply changes and Save name the concrete action.
- R-16 PASS: labels use ordinary task language; no AI marketing slogans are present.
- R-20 PASS: inline spatial prompts and a reference screen inside a free workspace give the editor its own interaction identity.
- R-21 PASS: editor System/Light/Dark and per-page appearance are separate implemented choices; theme changes preserve project data.
- R-29 PASS: neutral surfaces and one amber accent family form the palette; red is reserved for errors/destructive actions. The guide shade has a measured contrast purpose.
- R-30 PASS: browser-style page tabs follow the user's request; the resulting composition is specific to spatial site authoring.
- R-31 PASS: major decisions have concise reasons in DESIGN.md, and the core/plugin contracts identify ownership.

## Recorded control outcomes

| Controls | Native result |
| --- | --- |
| Canvas Enter/click/drag; textareas | Real requests persist; literal multiline input survives; text selection does not move content. A negative off-guide request enters the exact requirements file. |
| Move/resize grips; keyboard placement | Trusted pointer geometry and rapid keys persist without lost deltas; a click alone creates no edit. Six zooms retain text, grip size and keyboard focus. |
| Grid and rulers; Unit/spacing; visibility/snap/rulers | New grid defaults pass. 5% spacing persists; real drag and keyboard movement use the per-axis interval. Toggling rulers removes/restores both scales. |
| View and footer zoom menus; Show selection; Center screen guide | Actual guide transforms change; selection is brought into view; centering restores the guide without changing saved placement. |
| Nested Menu; project page tabs | Real arrows/Home/End/Escape and outside interaction navigate/dismiss correctly. Menus and page navigation do not mutate project data. |
| New/Rename/Add page/Page settings/Remove | Real forms and validation commit or cancel; route conflicts preserve state; Undo restores removed records. Nested replacement dialogs restore parent/opener focus. |
| Project dossier; Brief/Choices/Exceptions; PROJECT.md | Manual choices persist. A completed draft appears automatically in Overview, including actual proposals; Review generation opens its real nested review. Displayed Markdown matches Rust byte for byte. |
| Technical project details and Site source | Displayed revision/schema/identity match native state. HTML/CSS/JavaScript match accepted page source; opening these views makes no edit. |
| Save/Save as/Open/Save requirements/recovery | Real pickers write and inspect archives/Markdown, preserve cancellation and backups, recover the full project after SIGKILL. |
| AI settings; More/Advanced; model/limits; key lifecycle | Settings persist; password fields clear; access tests use real controlled HTTP without generation; vault failure requires explicit storage choice. |
| Generate/More/Send/View progress/Cancel | Prepare sends nothing; Send delivers byte-exact requirements/global mode; sticky actions and the workspace strip stay visible; reopening does not resend. |
| Retry/Apply/Discard; choices/feedback/source | Explicit retries retain the frozen request. Compatible choices start selected and can be unchecked. Only selected choices and source commit together; stale/partial output cannot apply. |
| Undo/Redo; automatic dossier | Draft/applied/undone provenance stays consistent, duplicate Apply is harmless and no provider request is replayed. Saved notes/source and pre-apply checkpoints are inspected. |
| Enable interactions; preview links/buttons; native reload | Trusted generated interactions run in an opaque frame; tested network/parent/native access is denied. Paused navigation remains scriptless. Actual infinite-loop recovery preserves the project and pauses scripts. |
| ChatGPT sign-in/accounts/limits/usage/storage repair | Real signed loopback OAuth, separate registrations, frozen-account generation, plan-limit failures, revocation and no-HTTP storage repair pass. No API billing fallback occurs. |

The primary agent inspected actual [light](../test-results/native/2026-10-18T16-31-22-849Z-1237999/editor-light-window.png) and [dark](../test-results/native/2026-10-18T16-31-22-849Z-1237999/editor-dark-window.png) workspaces, [API settings](../test-results/native/2026-10-18T16-31-22-849Z-1237999/generation/settings-light-760px.png), [candidate review](../test-results/native/2026-10-18T16-31-22-849Z-1237999/generation/candidate-light-760px.png), [automatic dossier](../test-results/native/2026-10-18T16-31-22-849Z-1237999/generation/automatic-dossier.png), [ChatGPT settings](../test-results/native/2026-10-18T16-31-22-849Z-1237999/chatgpt/settings-light-760px.png) and [plan-limit state](../test-results/native/2026-10-18T16-31-22-849Z-1237999/chatgpt/plan-stream-limit-dark-760px.png). Technical/raw views in other fixture screenshots were explicitly opened by the test and are not the default review.

Existing API/ChatGPT secure-store and startup implementations are unchanged. Their earlier isolated real Secret Service and normal feature-disabled startup evidence is reused as identified in [IMPLEMENTATION.md](IMPLEMENTATION.md); those launches were not repeated for this UI revision. The current normal binaries were rebuilt, and the instrumented build exercised fresh initialization and crash recovery. Source inspection covers alternate error-only retry/dismiss/reset/welcome paths; this report does not claim every failure variant was induced.

Live OpenAI consent/output quality, Windows/macOS, screen readers, IME, broader DPI/text magnification, installers and the automatic watchdog dialog remain separate verification gates. The remaining full 1.0 features are listed in IMPLEMENTATION.md and RELEASE_PLAN.md.
