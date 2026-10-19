# Desegnos UX audit

Date: 2026-10-19. Based on the frontend source (`src/`) and the most recent native-test screenshots in `test-results/native/2026-10-19T10-*`. The app was not run interactively for this audit.

## Summary

The engineering foundation is strong: validation, recovery, credential isolation and test coverage are careful. The user experience lags behind because the no-sidebar rule in [DESIGN.md](DESIGN.md) has been read as "no visible controls at all". No sidebars is a deliberate product decision and stays. But as a result almost every capability is either a hidden gesture or an entry in the single **Menu**. A first-time user sees a grey canvas and three plain buttons, with no visible path from "I want a website" to "I have a website".

The core loop that must feel effortless:

> Start project → place a request → describe it → Generate → review → tweak → export

Today, steps 2, 5 and 7 are weak or missing.

## Priorities

| # | Item | Priority | Effort |
| --- | --- | --- | --- |
| 1 | Export a usable website | P0 | M |
| 2 | Onboarding and empty states | P0 | S |
| 3 | Visible tools instead of gesture-only input | P0 | M |
| 4 | Double-click edits text inline; AI prompt gets its own trigger | P0 | M |
| 5 | Non-blocking generation with on-canvas review | P1 | M |
| 6 | Request overview and request box readability | P1 | S |
| 7 | Element toolbar placement and labelling | P1 | S |
| 8 | AI settings order and connection status | P1 | S |
| 9 | Plain-language copy | P1 | S |
| 10 | Status bar, rulers, measurements and dialog polish | P2 | S |
| 11 | DESIGN.md rule changes | P0 (process) | S |

Do these in order. Each item lists the problem, the change, and how to tell it is done.

---

## 1. Export a usable website (P0)

**Problem.** Save only writes a `.desegni` archive. The README lists export as a later release gate, so a user can never get a website out of the tool. This is the single largest product gap.

**Change.**
- Add **Menu → Project → Export website…** (and a top-bar **Export** button once any page has content).
- Minimum viable export: a folder (or ZIP) with one `index.html` / `<path>/index.html` per included page, a shared `styles.css`, page scripts, and embedded images written out as files.
- After export, show a notice with **Open folder**.

**Done when.** A generated project can be exported and the output opens correctly in an ordinary browser from disk, with no Desegnos runtime.

## 2. Onboarding and empty states (P0)

**Problem.**
- The start screen (`src/NewProject.tsx`, "Create your website") is three unlabeled choices. "Import identity from project" is jargon.
- There is no recent-projects list.
- A blank page is grey canvas plus a small status-bar hint (`src/App.tsx`, `status-bar`). Nothing on the canvas says what to do.
- **Generate** is hidden until a request has text (`src/App.tsx`, `generation-controls`), so new users don't know it exists, and the top bar shifts when it appears.

**Change.**
- Start screen: one sentence explaining the loop, then cards with a short description each:
  - **Blank website**: "Start from an empty page."
  - **Use my existing brand**: "Read colors, fonts and logos from a website or project." Use this instead of "Import identity".
  - **Open project**, plus a **Recent projects** list underneath.
- Empty page: centered hint inside the screen guide, shown until the first request exists: "Double-click anywhere to describe what goes here." Include a visible **Add request** button with the same effect.
- Always render **Generate**. When there are no filled requests, disable it with a tooltip: "Describe at least one request first."

**Done when.** A user who has never read the README can create a page and reach Generate without opening Menu or Help.

## 3. Visible tools instead of gesture-only input (P0)

**Problem.** Add request (double-click), area (Shift+drag), pan (drag/Space), context actions (right-click) and zoom (Ctrl+wheel) have no visible controls.

**Change.**
- Add a small floating tool bar at the bottom center of the canvas: **Select** (V), **Add request** (R), **Pan** (H), plus zoom −/% /+ moved here from the status bar.
- Keep every existing gesture as a shortcut. The tools only make them discoverable.
- Every tool and icon button gets a tooltip that shows its shortcut.

**Done when.** Every canvas capability can be reached with a visible control, and the gestures still work.

## 4. Double-click edits text inline; AI gets its own trigger (P0)

**Problem.** In every site builder, double-clicking text edits it in place. Here, double-click on a generated element opens an AI prompt (`src/Canvas.tsx`), and editing text requires opening the **Edit** modal (`src/ElementTools.tsx`). The most common action takes the most steps, and an AI action fires where users expect a local edit.

**Change.**
- Double-click on a text element: inline `contenteditable` editing on the canvas, committed on blur or Enter and cancelled with Escape. It uses the existing `edit_element` command and Undo.
- **Ask AI** gets a labelled button in the selection toolbar and a shortcut (`/` or Ctrl/Cmd+K). It opens the existing prompt box beside the element.
- Double-click on empty canvas still creates a request.
- Keep the Edit dialog for links, images and styles.

**Done when.** Changing a heading's text is double-click → type → Enter, with no dialog and no provider call.

## 5. Non-blocking generation with on-canvas review (P1)

**Problem.**
- The progress dialog (`src/Generation.tsx`) is modal and covers the canvas while you wait.
- The latest progress screenshot shows a stray text caret at the top of the dialog body, which looks like an empty focused element.
- When the result arrives it replaces the site directly. Review happens only through Undo or the Details screens.
- Fuzzy/Strict is an unexplained segmented control in the top bar.

**Change.**
- Show progress in the existing horizontal strip above the site, with elapsed time and **Cancel**. Open the dialog only from **Details**.
- Fix the stray caret.
- When a result is applied, show an on-canvas bar: **Keep** · **Undo** · **Try again** · **Details**. Briefly highlight the changed elements.
- Move Fuzzy/Strict into a dropdown on the Generate button ("Interpretation: Flexible / Precise") with a one-line description of each.

**Done when.** A user can keep scrolling and editing during generation, and can accept or reject the result without opening a menu.

## 6. Request overview and request box readability (P1)

**Problem.**
- Long prompts get a fixed-size dashed box with an inner scrollbar (first native screenshot).
- There is no way to see how many requests exist, which pages they are on, or which ones are done.

**Change.**
- Grow request boxes to fit their text up to a sensible maximum height. Show a "…more" expander instead of an inner scrollbar.
- Add a **Requests (n)** button next to Generate. It opens a popover listing pending requests grouped by page. Clicking one selects it and scrolls to it. This keeps to the spirit of "no permanent sidebars".

**Done when.** Every pending request is readable and reachable from one list.

## 7. Element toolbar placement and labelling (P1)

**Problem.**
- The floating toolbar sits above the selected element and covers the content above it. In `direct-selection-window.png` it hides "Scripts are paused until enabled".
- The toolbar is icon-only (edit, replace, move, more).

**Change.**
- Place the toolbar above the element. Flip it below when it would cover other content or leave the visible canvas (`src/Canvas.tsx`, toolbar position).
- Tooltips on every icon. Show a text label for **Ask AI**.
- Show the element type (e.g. "Button") as a small tag on the selection outline.

**Done when.** Selecting any element never hides the content immediately around it.

## 8. AI settings order and connection status (P1)

**Problem.**
- In `src/AiSettings.tsx` the order is method → model → allowance → **Save AI settings** → key status → key field. The key, which a new user needs first, is below the fold.
- **Test access** is disabled until a key exists, but nothing explains why.
- The top-bar **AI** button is plain text and doesn't show whether AI is connected.

**Change.**
- Order: method → key (or ChatGPT sign-in) → **Test access** → model → spending limit → advanced. Use a single save action at the bottom.
- Top bar: replace **AI** with a status pill: "AI: Not connected" (attention style), "AI: OpenAI key" or "AI: ChatGPT · name".
- Rename "Estimated request allowance (USD)" to "Spending limit per generation (USD)".

**Done when.** A new user can connect AI in one pass, from the top of the dialog to the bottom, without scrolling back up.

## 9. Plain-language copy (P1)

**Problem.** Internal vocabulary leaks into the UI and README: dossier, requirements, frozen, identity, candidate, Draft/Include in website, Technical.

**Change.** Suggested renames for UI labels only (keep technical identifiers):

| Current | Suggested |
| --- | --- |
| Project dossier | Project brief |
| Import identity | Use my existing brand |
| Review requirements | View AI instructions |
| Include in website / Draft | Published / Hidden page |
| Fuzzy / Strict | Flexible / Precise |
| Estimated request allowance | Spending limit |

Rewrite the README's first screen as a short user guide. Move the protocol, storage and security detail into `docs/`.

**Done when.** No visible label requires reading docs to understand.

## 10. Polish (P2)

- **Status bar.**
  - "Page appearance" is a stacked label over a dropdown that is taller than the bar. Replace it with a sun/moon toggle.
  - Move zoom to the canvas tool bar (item 3).
- **Measurements.** The selection readout (e.g. `20.5% · 74%  73.1% × 22.7%`) floats in the bottom-right corner, far from the selection. Show it next to the selection, in px by default with % on hover or in settings.
- **Rulers.** The default percentage rulers from −150% to 250% are noisy. Default to px and keep % as an option.
- **View stability.** After generation the view can land scrolled into empty space (`generated-canvas-window.png`). Keep the user's view, or frame the changed elements.
- **Mobile guide.** At 75% zoom the 390×844 guide is cut off at the bottom. Mobile should fit height on open.
- **Dialogs.**
  - Use a standard × close icon instead of a "Close" text button.
  - Use consistent button order. Today **Create project | Back** and **Cancel | Save changes** are mirrored between dialogs.
- **Redo.** Only Undo has a top-bar button. Add Redo, or remove both and rely on the menu and shortcuts.

## 11. DESIGN.md rule changes (P0, process)

[DESIGN.md](DESIGN.md) currently forbids the fixes above. Replace the absolute rules with goals:

| Current rule | Proposed goal |
| --- | --- |
| "There are no sidebars, inspectors, layer panels…" | Keep as is (owner decision). Add: "Small floating tool bars and transient popovers are not sidebars and are allowed." |
| "…or numeric placement fields" | "Placement is visual first. Numeric fields may appear in Edit dialogs for precision." |
| "Double-clicking … a generated element opens a text box" | "Double-click edits text in place. AI prompts open from an explicit Ask AI control or shortcut." |
| Generation progress as a dialog | "Generation never blocks the canvas. Progress lives in the strip; the dialog is for details." |

Add one new rule: **"Every capability has a visible control. Gestures and shortcuts are accelerators, never the only path."**

---

## Process notes

- **Put the project under version control before further agent work.** The folder is not a git repository today.
- **Reduce verification overhead per change.** `test-results/` holds 78 run folders and logs, and `RELEASE_PLAN.md` is 122 KB. Keep the safety contracts, but measure progress against the core loop above.
- **Pair each UX item with one native screenshot test** of the before/after state, not a full matrix of light/dark/760px variants for every step.
