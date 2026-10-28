# Generation quality benchmark

Status: **UNVERIFIED**. The scenarios below define the owner-rated gate; no scenario has a quality rating yet. Offline fixture validation only proves valid inputs, not model quality.

This measures the success definition in [PRODUCT.md](../PRODUCT.md): "I can make an image, deck, website or app screen look how I imagined it, edit it, and keep that appearance when I export and reopen it."

Website cases remain useful, but they are only part of the canvas product. Image, slide and app-screen cases use the same request/placement, owner-rating, kept-rate and time measurements. The [three finished examples](../CANVAS.md#next-milestone-three-finished-examples) are the next milestone; this benchmark is the broader provider-quality gate.

## Fixed scenarios

| ID | Scenario | Style | Owner checks |
| --- | --- | --- | --- |
| 01-hero | Studio landing hero | Flexible | Clear hierarchy, useful action and a self-contained composition. |
| 02-pricing | Three-tier pricing | Precise | Exact prices, comparable benefits, a recommended tier and honest local behaviour. |
| 04-about | Two-column About | Flexible | Balanced columns, reading order and narrow-screen stacking. |
| 05-mobile-home | Complete 390 px homepage | Flexible | Touch targets, readable content and no horizontal overflow. |
| 06-button-revision | Bigger green booking button | Precise | Requested height and colour; existing text, behaviour and other layers preserved. |
| 08-multi-precise | Header, hero, services, footer | Precise | Header → hero → grouped services → footer. |
| 09-imported-brand | Already approved imported identity | Precise | Exact palette, actual supplied logo and consistent language. |
| 10-faq | Accessible FAQ | Precise | Five keyboard-operable disclosures and visible focus. |
| 12-navigation-footer | Navigation and footer | Precise | Navigation → content → footer, with working local links. |
| 13-mobile-contact | 390 px contact form | Precise | One-column controls, honest local review state and no overflow. |
| 15-long-copy | Long reading page | Precise | Requested section order, 80–100 words per section and a readable line length. |
| 16-two-pages | Linked Home and About | Precise | Both pages complete; native links work in the exported site. |
| 17-responsive-layout | Wide hero and narrow adaptation | Precise | Hero group before feature group; the 390 px adaptation preserves content. |
| 18-social-image | 1080 × 1080 launch image | Precise | Supplied picture/logo, exact headline, clear hierarchy and intentional crop; PNG and eligible SVG preserve the composition. |
| 19-image-revision | Revise a social image headline and accent | Precise | Requested text and colour only; picture, crop, grouping and unrelated layers survive export and reopening. |
| 20-slide-deck | Three-slide introduction | Flexible | Title, content and closing slides share typography and spacing; independent PDF opens with correct order and complete content. |
| 21-slide-revision | Revise one slide in the deck | Precise | Change the selected slide's copy/layout; other slides remain intact; PDF and reopened project preserve the result. |
| 22-app-screen | 390 × 844 task-list screen | Precise | Readable controls and empty/populated content; one simple pointer action works in Play and exported web files, with no claim of a live backend. |

Every scenario requires readable, complete content, sufficient contrast and truthful treatment of unavailable services or assets. Interactive outputs also require keyboard access and visible focus. Static images and PDF decks are checked in an independent viewer; Play actions do not imply exported interactivity. "Usable without manual fixes" means the owner could use the result for its purpose without correcting copy, layout or interactions. Cosmetic preferences alone do not fail an otherwise usable design; write them in notes.

## Measurements

For each run record:

- **Placement fidelity.** Compare each request area with the bounds of the generated elements: order, alignment, size ratio and whether they stayed inside the area; for picked targets, check the selected layers and unchanged surroundings.
- **Owner rating.** "Looks like what I imagined" yes or no.
- **Kept rate.** The share of generations kept without Undo.
- **Time to a finished output.** From new project to an exported image, deck, website or app screen the owner rates done.
- **Editing and reopening.** Record one meaningful edit, save/close/reopen, and export again. Check intended appearance, content, assets, linked sizes and relevant interactions in the reopened project and independent output viewer.
- **Provider and cost.** Provider, model, style (Flexible/Precise), estimate and actual usage or plan credits.

## Running the gate

The owner runs one scenario at a time with a real provider account. Each Generate is one paid request and is never retried automatically. A new invocation is another potentially chargeable attempt.

1. Create a fresh project, choose the output type and place the scenario's boundaries, request areas, text and pictures by hand.
2. Make one explicit Generate decision, then review the result on the canvas.
3. Rate it before editing. Keep or Undo; record the result.
4. Make one meaningful edit, save and export the output. Open it in an independent viewer or browser; record its appearance and any relevant interaction checks. Web export may require a separate explicit paid-send decision for behaviour generation; record that attempt separately.
5. Close and reopen the saved `.desegnos` project, check appearance and editability, export again and inspect the result. Keep pre-edit ratings separate from any improvements made by hand.
6. Repeat for all 18 scenarios, then summarize results here with dates, app revision, platform, provider, model, style, evidence locations and unresolved differences. Mark blocked cases explicitly; do not count skipped cases as passes.

No automated runner is required for the gate. A repeatable local fixture runner may be added later; it must not send paid requests without the same explicit decision.
