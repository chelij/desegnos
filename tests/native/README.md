# Native editor checks

Run `pnpm test:native` from WebMaker. The runner builds the explicit `native-tests` feature with `src-tauri/tauri.test.conf.json`, then drives the real Tauri editor in WebKitGTK using its debug WebDriver and trusted X11 pointer/keyboard input. It calls actual Rust commands; it does not replace `invoke` or fabricate project state.

The runner uses `~/.local/bin/xvfb-run -d`, a private display numbered at least 90, a private session bus/profile/data/runtime directory, and a free local port. It checks display and window ownership before input. It never moves windows or switches workspaces on the main display. Existing desktop sockets, locks and an initially working connection are checked after cleanup.

To reuse a test build:

```sh
node scripts/native-test.mjs --skip-build --binary /absolute/path/to/test-build/desegnos
```

A normal binary has no WebDriver server. `--ux-audit-only` runs the current core loop without the separate identity-import flow; `--identity-only` creates a real source website then exercises import. The historical `--generation-only` option aliases `--ux-audit-only`.

## Current workflow

The default run checks:

- First-use cards, an empty Light mobile page, visible Generate, guide clearance around the floating tools, and no sidebars.
- Select/Request/Pan/zoom, rulers, exactly one request per empty-space double-click, empty-request navigation, long-text expansion and grouped page requests.
- Connection-first settings, a synthetic session key, actual model access testing and truthful budget wording.
- Real loopback HTTP transport, byte-exact requirements and global interpretation, cancellation, editable nonmodal progress and transactional two-page apply.
- Keep/Undo/Redo, inline text Enter/Escape, repeated double-click editing, toolbar placement and explicit Ask AI/Ctrl+K without provider calls for local changes.
- Preservation of an unfinished inline edit when generation completes; stale output must remain blocked.
- Native Save/Open/recent-project pickers, archive integrity, local HTML replacement and portable two-page export with real embedded images, CSS and scripts.
- Ordinary Chromium loading the exported website from disk, executing local scripts and navigating relative page links without Desegnos.
- Read-only identity sources from archives/folders/ZIPs, explicit analysis, selected findings/logos, cancellation, malformed results and save/reopen.
- Forced SIGKILL, fresh-process journal recovery, session-key clearing, no automatic paid-work replay, empty browser-error lists and credential nondisclosure.

Evidence is retained in `test-results/native/<timestamp>/`: step report, native logs, screenshots of the owned window, physical-input calibration/events, requirements, archives, exported files and cleanup results. This run uses synthetic local provider/OAuth fixtures and makes no paid inference calls. Linux WebKitGTK is exercised; Windows/macOS need their own native runs.

## Retained earlier coverage

Historical workflow modules and their evidence remain available. Their labels and sequencing describe earlier interfaces and are not the default acceptance path. The previous combined 100-step run is recorded in `test-results/native/2026-10-19T10-54-29-820Z-715292/report.json`. It covered direct dragging/corner resizing/context menus, grid snapping, signed ChatGPT/account/rotation, provider failure and stream diagnostics, retained-preview script/IPC isolation, legacy migration, contrast and identity import.

Rust tests continue to check fragmented and mislabeled event streams, completed-output merging, exact job/digest correlation, source staleness, size/credential bounds, durable transactions, retry archival and archive validation. The separately guarded OS-vault test is opt-in (`pnpm test:vault`); ordinary runs never touch the user's system vault. Paid generation quality is a distinct owner-run benchmark gate, not established by fixture success.
