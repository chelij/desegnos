# Native editor checks

Run `pnpm test:native` from WebMaker. The runner builds the explicit `native-tests` feature with `src-tauri/tauri.test.conf.json`, then drives the real Tauri editor in WebKitGTK using its debug WebDriver and trusted X11 pointer/keyboard input. It calls actual Rust commands; it does not replace `invoke` or fabricate project state.

The runner uses `~/.local/bin/xvfb-run -d`, a private display numbered at least 90, a private session bus/profile/data/runtime directory, and a free local port. It checks display and window ownership before input. It never moves windows or switches workspaces on the main display. Existing desktop sockets, locks and an initially working connection are checked after cleanup.

`--project-folders-only` covers automatic creation and first Save without a picker, portable names and collisions, per-project and persistent default locations with reset, archive/live-site sync, foreign-file preservation, sync warnings, Save As cancellation/copy folders, legacy in-place saves and Move, plain-website import with immutable originals, folder reopening, recents and crash recovery. Every native scope gets an isolated XDG Documents directory, so automatic saves cannot reach the owner's Documents folder.

`--website-import-only` covers the default edit-or-brand choice, byte-preserving plain-site import, native heading editing, shared styles, original module execution and relative navigation in Live, build progress/Stop, read-only detection of `~/Projects/chelib`, a build on its private copy, separate framework-project placement and crash recovery. The original chelib folder is only read. Large recovery requests use a two-minute test timeout; dependencies copied for the build are removed before scanning retained evidence.

To reuse a test build:

```sh
node scripts/native-test.mjs --skip-build --binary /absolute/path/to/test-build/desegnos
```

Standalone browser checks use the installed Chromium and matching ChromeDriver; the separate export matrix also uses installed Firefox. No browser or driver is downloaded. A normal binary has no WebDriver server. `--ux-audit-only` runs the current core loop without the separate identity-import flow; `--identity-only` creates a real source website then exercises import. The historical `--generation-only` option aliases `--ux-audit-only`. `--layers-only` runs the four-icon anchored selection bar, element/request/blank-canvas right-click commands, real Cut/Undo, unclipped dialog menus, blocked-to-fresh drafts, native PNG downloads and failed-image placeholders with actual portable export, plus floating layer selection/order, actual system clipboard transfers, nested and negative stacking, boxless wrappers, native text shortcuts, cross-page paste, pseudo/SVG preservation, both themes/760px, Save/Open, ordinary Chromium export and crash recovery. `--canvas-input-only` runs the focused pointer, edge placement, long-pan rebasing, uncapped box drawing/moving/resizing, Save/Open, Undo/Redo and crash-recovery checks.

## Current workflow

The default run checks:

- First-use cards, an empty Light mobile page, visible Generate, guide clearance around the floating tools, and no sidebars.
- Select/Request/Pan/zoom, rulers, exactly one request per empty-space double-click, empty-request navigation, long-text expansion and grouped page requests.
- Connection-first settings, a synthetic session key, actual model access testing and truthful budget wording.
- Real loopback HTTP transport, byte-exact requirements and global interpretation, cancellation, editable nonmodal progress and transactional two-page apply.
- Page updated / Undo / Redo, inline text Enter/Escape, repeated double-click editing, toolbar placement and explicit Ask AI/Ctrl+K without provider calls for local changes.
- Preservation of an unfinished inline edit when generation completes; stale output must remain blocked.
- Native Save/Open/recent-project pickers, archive integrity, local HTML replacement and portable two-page export with real embedded images, CSS and scripts.
- Ordinary Chromium loading the exported website from disk, executing local scripts and navigating relative page links without Desegnos.
- Read-only identity sources from archives/folders/ZIPs, explicit analysis, selected findings/logos, cancellation, malformed results and save/reopen.
- Forced SIGKILL, fresh-process journal recovery, session-key clearing, no automatic paid-work replay, empty browser-error lists and credential nondisclosure.

Evidence is retained in `test-results/native/<timestamp>/`: step report, native logs, screenshots of the owned window, physical-input calibration/events, requirements, archives, exported files and cleanup results. This run uses synthetic local provider/OAuth fixtures and makes no paid inference calls. Linux WebKitGTK is exercised; Windows/macOS need their own native runs.

## Retained earlier coverage

Historical workflow modules and their evidence remain available. Their shared helpers and menu/control paths have been migrated, but the older combined sequence is not the default acceptance path or fresh evidence for this cleanup. The previous combined 100-step run is recorded in `test-results/native/2026-10-19T10-54-29-820Z-715292/report.json`. It covered direct dragging/corner resizing/context menus, grid snapping, signed ChatGPT/account/rotation, provider failure and stream diagnostics, retained-preview script/IPC isolation, legacy migration, contrast and identity import.

Rust tests continue to check fragmented and mislabeled event streams, completed-output merging, exact job/digest correlation, source staleness, size/credential bounds, durable transactions, retry archival and archive validation. `--source-edit-only` additionally exercises the source-edit command, line errors, selector map, one-step history, native save/reopen/export and crash recovery. Run native display suites sequentially so their guarded Xvfb display allocation cannot race.

The separately guarded OS-vault test is opt-in (`pnpm test:vault`); ordinary runs never touch the user's system vault. Paid generation quality is a distinct owner-run benchmark gate, not established by fixture success.

The cleanup search check used exactly one owner-authorized connected ChatGPT-plan generation. It is separate from native fixtures and the still-unverified 18-scenario quality gate; see [the cleanup record](../../docs/CLEANUP_CODEX.md).

## Cleanup additions

`--cleanup-extras-only` drives History with duplicate content, Undo/Redo, branching and settings; all three first-run tips and dismissal through a fresh process; cross-page Page check selection and request reuse; real native file-picker picture replacement and GTK/Xdnd drag/drop with trusted input; Desktop/Tablet/Phone layout widths and enabled preview interaction; Split/Code search, canvas selection, invalid draft preservation and valid source saving; archive reopen and crash recovery. File drags use a test-owned GTK source with the same text/uri-list format as file managers. Both source and destination PIDs/display/window bounds are checked before XTEST input. No paid inference is needed.

The expanded suite also covers stable-ID multi-selection, a single union request, a local text edit before preparation, atomic multi-delete/Undo, stale request reassignment/removal/Undo, checked-save Cancel/copy/replace, changed-during-confirmation rejection, guarded Save as, unreadable destinations, code-only Undo/Redo and immediate Ctrl+S into a verified archive. Current projects use schema 7 and new requirements use format 10; current selection assertions compare editor IDs with the actual rendered element. See [the integrated results](../../docs/SELECTION_SAVE_CODEX.md).

`--image-resize-only` verifies a decodable PNG padded beyond the former 32 MB limit through file selection and a real GTK drag: original preview before Generate, automatic bounded compression on Generate, local-only processing without provider usage, Undo, and crash recovery. The focused file-selection check supplies a real File to the input; it does not exercise the native file-picker dialog.

## Real use (`--real-use-only`)

`node scripts/native-test.mjs --real-use-only` runs the flows the owner actually uses, on copies of their real data. Before launch, `real-use-fixtures.mjs` copies the recent projects and every unsaved-work recovery record (with its journal) from `~/.local/share/io.github.chelij.desegni` (or `DESEGNI_REAL_USE_SOURCE`) into the run's private profile and `test-results/native/<run>/real-projects/`. Originals are only read: recovery records are rewritten to point at the copies, so nothing the test app saves can reach the real files. `test-results/` is git-ignored. A journal identical to its saved file is not unsaved work, and the app rightly does not offer it.

The steps come from `scripts/usage-report.mjs`, which summarizes the local usage log (`usage/*.jsonl`: control names, shortcuts, keystroke counts per field and command timings, never typed text) and the generation history (every request sent and its outcome):

- every piece of unsaved work is offered after a restart, "Not now" puts one aside, and all recover;
- the real project files open, show their pages and close (Discard sets the unsaved work aside instead of re-offering it);
- typing a several-line request on a real element: no toolbar button flips back and forth, and the request is saved within 1.5 s of the last key arriving (Xvfb draws without a GPU, so a burst of keys reaches the box slowly; `real-typing-keys.json` records the per-key cost);
- an element fix is generated, discarded, retried and kept, each attempt sending format 13;
- a blank page gets two several-line boxes and a 4000×3000 camera photo; the AI gets one low-detail preview, and only a 640×480 copy (twice its 320 px display width) is kept;
- the new project saves and reopens unchanged.

Rerun the usage report after a stretch of normal use and update this suite when the patterns change. These checks use the synthetic provider; generation quality on a real account is still owner-run.

## Accessibility and 2026-10-21 coverage

`--canvas-input-only` also draws a Shift-constrained circle and an automatically closed
freehand area through trusted XTEST input. It checks circle corner exclusion, shape
picker shortcuts/memory, normalized lasso points through move/resize and Undo/Redo,
Escape cancellation, shape outlines, archive reopen, and crash recovery.

Set `DESEGNI_NATIVE_TEST_ACCESSIBILITY=1` to exercise the app with an AT-SPI listener
on the private display/session bus. The runner selects the accessibility launcher's
`dbus-daemon` mode before starting that bus, so it does not depend on a desktop
systemd session. It checks the app PID and never queries the main desktop tree.
The UX suite includes existing Unicode text, shorter replacements, native canvas
text editing, a real `needs` follow-up, summed usage, failed raw replies and read-only
past conversations. The identity suite checks default and unticked photo/work choices.
It also checks the three-step Import context flow, closed file details, Astro page
support and ranking, automatic file picking, a disabled Read my brand button when
manual choices are too large, repeated Pick for me resets, and website-address import.
The cleanup editor check waits for the observable Undo/Redo result after native keys.

## Workspace screenshot tour

`node scripts/native-test.mjs --ui-tour-only` builds and captures the workspace panels,
inline confirmations, canvas previews and start/recovery screens. Add `--skip-build`
when the explicit native-test binary is current. Each surface runs independently;
unreachable steps are listed in `index.md`. Captures use `PhysicalInput.captureWindow`
on guarded Xvfb at the default 1380 × 900 window and 900 px width, in both themes.
`DESEGNI_UI_TOUR_OUTPUT` overrides the tour's default scratchpad destination. The tour
uses disposable project files, the six-picture fixture and the synthetic provider;
it never sends paid AI requests or changes application source.
For a focused recapture, set `DESEGNI_UI_TOUR_SURFACES=2` (or comma-separated
surface numbers from the index); existing index entries for other surfaces remain.

## Canvas flash regression

`DESEGNI_FLASH_LABEL=after node scripts/native-test.mjs --flash-only` captures the copied
owner project `chelib-redesign` and a small fixture at 60 Hz through trusted X11 drags.
The scope uses `seedRealUse` without recovery prompts, verifies the source archive's
SHA-256 before/after, and never opens the original. It checks a continuously visible
canvas document, the committed offset, no snap after drop, selection alignment,
cancelled dragging, Undo/Redo, and live reloads with startup script errors. Only the
debug native-test binary is built; `--skip-build` reuses it.

Evidence goes to `test-results/flash/<label>/`: lossless owned-window videos, DOM/size/
notice traces, live-reload samples and source integrity. To capture an unfixed baseline,
set `DESEGNI_FLASH_LABEL=before DESEGNI_FLASH_ASSERT=0`. Then run
`python tests/native/analyze-flash.py test-results/flash/before test-results/flash/after`
to retain exact anomalous frame indices and PNG strips. The analyzer uses installed
FFmpeg, Pillow and NumPy; its fixed unchanged regions match these two fixtures and
exclude the moved element, drag measurements and notices. The ordinary native report
and desktop-cleanup checks remain in the run's `test-results/native/` directory.
