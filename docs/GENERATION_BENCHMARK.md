# Generation quality benchmark

Status: **UNVERIFIED**. The scenarios below define the owner-rated gate; no scenario has a quality rating yet. Offline fixture validation only proves valid inputs, not model quality.

This measures the success definition in [PRODUCT.md](../PRODUCT.md): "I can get a site looking like how I imagined it."

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

Every scenario also requires readable, complete content, keyboard access, sufficient contrast and truthful treatment of unavailable services or assets. "Usable without manual fixes" means the owner could use the result for its purpose without correcting copy, layout or interactions. Cosmetic preferences alone do not fail an otherwise usable design; write them in notes.

## Measurements

For each run record:

- **Placement fidelity.** Compare each frame with the bounds of the generated elements: order, alignment, size ratio and whether they stayed inside the frame.
- **Owner rating.** "Looks like what I imagined" yes or no.
- **Kept rate.** The share of generations kept without Undo.
- **Time to a finished page.** From new project to an exported page the owner rates done.
- **Provider and cost.** Provider, model, style (Flexible/Precise), estimate and actual usage or plan credits.

## Running the gate

The owner runs one scenario at a time with a real provider account. Each Generate is one paid request and is never retried automatically. A new invocation is another potentially chargeable attempt.

1. Create a fresh project and place the scenario's frames, text and pictures by hand.
2. Make one explicit Generate decision, then review the result on the canvas.
3. Rate it before editing. Keep or Undo; record the result.
4. Save the project and export the page for the final check.
5. Repeat for all 13 scenarios, then summarize the results here with dates, provider, model and style.

No automated runner is required for the gate. A repeatable local fixture runner may be added later; it must not send paid requests without the same explicit decision.
