# Desegnos feature priorities

Date: 2026-10-27. Product direction is [PRODUCT.md](PRODUCT.md). This file orders work on the `infinite-canvas` branch; technical and safety contracts are in [RELEASE_PLAN.md](RELEASE_PLAN.md) and UX findings in [UX_AUDIT.md](UX_AUDIT.md).

The core promise is placement and control: a person can make a page look like what they imagined without hand-writing code. Everything below serves that promise.

## Tier 1: one complete page

1. **Element model and renderer.** Every element kind draws, selects, moves, resizes and edits on the canvas. Boxes lay out freely, in a stack or in a grid. Text edits in place. Images come from the project picture store.
2. **Frames and generation.** A frame request is frozen into a native job and answered with elements. Only the targeted layer changes. Progress stays in one floating pill; failures say your work is safe.
3. **Pointers and Play.** Comments are the spec. Simple pointers run in Play. Clashes turn amber and never resolve silently.
4. **Saving and versions.** One `.desegnos` SQLite file, automatic versions, History, alternatives, crash recovery. Export metadata save without adding undo steps.
5. **Export.** A webpage export is real HTML, CSS and JS built from the element tree. Slides export as PDF and images as PNG or SVG directly. Export writes only to a destination the user chose.

Tier 1 is done when a new project can go from blank canvas to a saved page and an exported folder that opens in a browser.

## Tier 2: a finished site

1. **Linked sizes.** Phone, tablet and desktop boundaries share elements with per-size overrides, click modes and placeholders.
2. **Website import.** A site address, folder or ZIP becomes editable boundaries and elements. Imported sites are a starting point, not a live copy.
3. **Linked elements.** Named layers whose copies change together, with the linked/unlinked paste choice.
4. **AI changes across layers.** Pointers let a request reach a target deliberately. Project brief and decisions constrain the result.
5. **Design feedback.** Gaps, overlaps and clashes are reported in plain language without blocking intentional choices.

## Tier 3: reach

1. Hosted generation as a provider adapter; BYOK stays.
2. Provider choice per task: vision, image generation, streaming, web search.
3. Content collections, localisation, publishing and other candidates in [FEATURE_RESEARCH.md](FEATURE_RESEARCH.md).
4. Windows and macOS verification, signing and CI runners.
5. Onboarding, sample projects and benchmark evidence for the primary audience.

## Constraints

- **Visual first.** Work happens on the canvas. Code exists only in exports.
- **No sidebars.** Docked tool strip and Properties/Layers panels are the permanent chrome; other surfaces are transient.
- **No simulated results.** Every visible control performs a real operation.
- **No silent scope.** A request never reaches outside its layer without a pointer.
- **Plain language.** No invented metrics, fake progress or decorative AI imagery.
- **Small checks.** Each change ships with the smallest check that proves the outcome, plus screenshots for user-visible states.
