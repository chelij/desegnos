# Desegnos UX audit

Date: 2026-10-27, for the `infinite-canvas` branch. Based on the current React editor and the canvas concept in [CANVAS.md](CANVAS.md). The app was not run interactively for this audit.

## Summary

The editor is now one canvas with a tool strip, a Properties/Layers dock, project tabs and an AI request flow. The core loop is visual: draw a frame, describe it, Generate, then edit elements by hand. Import turns an existing site into elements instead of a preview.

The remaining friction is mostly in discoverability and first use: the density of tools is high, some actions still live only behind the menu, and the import flow needs a clearer before/after explanation.

## Findings

| # | Finding | Priority | Smallest fix |
| --- | --- | --- | --- |
| 1 | A first-time user sees an empty canvas and a wide tool strip. Nothing explains the draw/describe/generate loop. | High | Keep the start screen tips; add one short canvas hint that disappears after the first frame. |
| 2 | Frames are drawn with the Frame tool, but the request box only appears after drawing. A user can draw a frame and not notice where to type. | High | Focus the request immediately and keep a visible "Describe this frame" placeholder. |
| 3 | Import adds many boundaries at once. The user cannot tell what changed or what was left out. | High | Show one summary notice with pages, layers and skipped items; select the first imported boundary. |
| 4 | Project tabs show a colour dot and name, but a dirty parked tab closes only after a confirmation. That is correct but surprising. | Medium | Keep the confirmation and name the project in it. |
| 5 | Keyboard shortcuts are complete but only visible in tooltips and help. | Medium | Keep tooltips; make Menu → Keyboard shortcuts the single readable list. |
| 6 | Properties shows many fields for a multi-selection. | Medium | Show only shared values and a short "Mixed" label; keep per-field commits. |
| 7 | Pointer comments collapse to an icon when zoomed out. New users may not find the text. | Medium | Keep the popover on hover and expand the selected comment. |
| 8 | Generation progress is one pill, but a user can miss which frame is being filled. | Medium | Keep only affected frames blurred and name them in the pill. |
| 9 | Export has several states (prepared, sending, ready, failed). It is not obvious which one needs a paid-send decision. | Medium | Keep the primary action visible and label it with the decision, not the state. |
| 10 | The dock can be collapsed and hidden, but the icons alone do not explain themselves. | Low | Keep tooltips and the Tab hint. |
| 11 | Imported scripts become reference notes and some behaviours become pointers. The user needs a clear way to review them. | Low | Keep notes and pointers near the page; selecting the page can select its notes. |
| 12 | At narrow window widths the top bar wraps before the dock collapses. | Low | Collapse the dock to icons first, then the tabs. |

## Priorities

1. Make the frame request impossible to miss.
2. Make Import explain its result.
3. Make the paid-send decision unmistakable.
4. Tighten multi-selection Properties.
5. Verify the whole loop at 100% and 200% zoom with a keyboard only.

## Verification

The owner tests by hand after the Tier 1 loop works. Run a few short sessions with people in the primary audience: they start a page and import a site without help while someone watches. File what you see here.
