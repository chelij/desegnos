# Selection, save and cleanup follow-up

Implemented 20 October 2026 in WebMaker. This completes the selection/save specification and the remaining cleanup integration, including History and native acceptance. The running editor was not relaunched or controlled. The rebuilt executable is `target/release/desegnos`.

## Delivered behavior

- Selections keep a page-unique editor ID through text edits, moves, replacements and other generation results. Shift, Control or Command click combines up to eight elements. Nested selections keep their outer element. Ask AI creates one request around the combined bounds; deleting several selected elements takes one Undo step.
- A missing target shows **This element was removed**, **Pick again** and **Remove request**. Reassignment updates the existing request atomically and keeps its description and brief references. Request boxes fade and let clicks through during reassignment. The missing-target state also updates when Undo changes a request without changing its page source.
- Targeted generation preserves the rest of the document, shared CSS/JavaScript, embedded styles/scripts and event handlers. Every selected root must remain unless the user explicitly allows removal. IDs are assigned to new elements after the scope check. The AI cannot authorize its own scope expansion or removal permission.
- Ordinary Save refuses an externally changed, different or unreadable archive. A changed file offers **Save as copy**, **Replace it** and **Cancel**, with copy focused. Save as over another project requires the additional in-app confirmation, focused on Cancel. An unreadable file never offers Replace. Replacing saves the exact observed archive in `.previous`; a second external change requires a fresh choice. Cancelling a conflict or copy picker preserves the draft and recovery journal.
- Recovery rows show the last saved filename/path and draft age. Explicitly reopening the current file reuses its held writer lock while staging a fresh session and independent save baseline. A failed reopen preserves the active session, baseline and lock.
- History uses the native session, undo cursor and retained range, including duplicate content states, branch changes, settings, layer order and the 200-step limit. History jumps serialize their Undo/Redo operations. Opening or recovering a project starts a new History session.
- Design/Split/Code uses reviewed CodeMirror 6, loaded when needed. Editor identity attributes are physically hidden from the editable text by default and retained through exact edit ranges, text Undo/Redo and in-flight native saves. Fast typing cannot be replaced by a delayed React acknowledgement. Code edits remain local; project Undo stays separate.
- Save, close, export, generation, History and page/view changes flush all code and canvas drafts first. Invalid or conflicting code stays visible and blocks the transition. **Discard code changes** discards only that file. Ctrl/Cmd+S saves the newest text into the actual project archive.
- Real Linux file drops reach image replacement through the OS drop event, rather than trusting a renderer-supplied path. Browsing and dropping a picture preserve its authored dimensions and participate in Undo, save and reopen. Device previews use their exact inner layout widths, including 1440, 768 and 390 pixels.

## Contracts and boundaries

Projects now use schema **7** and newly prepared generation requirements use format **10**. Original prepared/ready job packets, receipts and usage remain intact.

The Rust core owns IDs, selection resolution, edit/scope validation and undo transactions. Storage owns validated file stamps, locking, atomic writes and backups. The desktop layer owns native pickers, conflict permissions, recovery coordination and trusted OS file drops. The frontend owns presentation and draft coordination; it cannot grant overwrite authority by supplying hashes or a path. No plugin boundary was moved. [The backend contract](CLEANUP_BACKEND_CONTRACT.md) records the exact commands and types.

CodeMirror uses nine pinned direct packages and twelve transitive packages, all MIT licensed. [The dependency review](CODEMIRROR_DEPENDENCY_REVIEW.json) records provenance, integrity hashes, licenses and the audit result; [the notices](CODEMIRROR_NOTICES.md) include the upstream licenses. The final dependency install used the frozen lockfile and disabled lifecycle scripts, reusing 30 packages and downloading the 21 reviewed editor packages into the configured `.pnpm-store`. The normal `pnpm build` command passes. The previous dependency directory is retained at `.tools/node_modules-before-selection-save-2026-10-20`.

## Verification

Evidence: [`verified-checks.json`](../test-results/cleanup-followup-2026-10-20/verified-checks.json). Native reports contain the exact binary hash, each completed step, screenshots, browser errors, actual archive/export checks and original desktop connection/resource checks.

| Check | Result |
| --- | --- |
| Rust workspace, all features | 349 passed |
| Strict Clippy, all targets and features | Passed with warnings denied |
| Canvas geometry, selection projection, CodeMirror state and draft lifecycle | 59 passed |
| TypeScript and production frontend build | Passed |
| Expanded native workflow | 25 steps passed |
| Required native UX audit | 25 steps passed |
| Required native Layers | 24 steps passed |
| Native source edit, archive, export and recovery | 8 steps passed |
| Native identity import, project-logo reuse and local image edit/history | 15 steps passed |
| Normal release executable, without test automation | Built successfully; not launched |

The 97 native steps ran sequentially using disposable profiles/session buses on guarded private Xvfb displays. They used actual Rust commands, GTK file pickers, system clipboard and trusted pointer/key input. The picture drop came from a real GTK drag source. Exported pages ran in ordinary Chromium. Crash recovery used a forced test-process termination followed by fresh initialization. All five suites reported no browser errors/unhandled rejections and preserved the original desktop connection and X11 resources.

## Verification limits

This follow-up used local AI fixtures; it made no additional paid generation. The earlier owner-authorized single ChatGPT search/image generation remains documented in [the cleanup report](CLEANUP_CODEX.md). Anthropic/Google live credentials and Windows/macOS native runs were unavailable. The separate owner-rated release benchmark remains unverified.

The stamp check coordinates Desegnos writers through the sidecar lock. A non-cooperating external writer can still race the final existing-file check and rename; every preceding observed change is refused, and a new-file commit cannot clobber a destination that appears concurrently. Structural target checks do not prove arbitrary custom-code runtime behavior or visual geometry. Area-only requests retain explicit page-source editing authority because a rectangle does not identify a trustworthy DOM subtree. Multi-element movement, resize and style editing remain outside this specification.
