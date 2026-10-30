# Desegnos product direction

Date: 2026-10-28. Owner decisions. Where this file and [RELEASE_PLAN.md](RELEASE_PLAN.md) disagree on product direction, this file wins; the release plan still governs technical and safety contracts. The next milestone is [three finished examples](CANVAS.md#next-milestone-three-finished-examples). [FEATURE_PRIORITIES.md](FEATURE_PRIORITIES.md) and [UX_AUDIT.md](UX_AUDIT.md) provide earlier research and audit context.

## What Desegnos is

**A visual canvas with AI.** A desktop design tool where you place things exactly where you want them on an infinite canvas, by hand or by describing them. Everything stays visual while you work; export compiles real HTML/CSS from the model and can use AI for missing pointer behaviour, while slides and images export directly. You own the output files. See [CANVAS.md](CANVAS.md).

## What makes it different

**Placement and control.** Many AI creation tools start with a chat prompt and a generated result. In Desegnos, you decide where things go and the AI fills them in. When the choice is between AI convenience and user control, control wins.

What this means in practice:
- **Placement fidelity is the core quality measure.** AI-made elements land in the request that asked for them, and exports keep elements where they were placed, at about that size, in that order and grouping. Precise mode honours this closely.
- **One Page type for web output.** Website and App screen are templates, not different boundary types. Keep desktop, tablet, phone and custom sizes, with growing or fixed heights. Existing Screen projects retain their content and layout.
- **Visual first.** Work happens on the canvas, not in code. Code exists only in exports, as ordinary files users can read and edit.
- **Changes are scoped.** A request changes only its selected or covered targets, and everything else stays as it was. Pointers are the explicit way to let a request reach further.
- **The output is ordinary files** that open without Desegnos and are not locked to any host.

## Who it is for

**Primary:** designers, creators, hobbyists, small-business owners and freelancers who want to compose a social image, slide deck, website or app screen themselves, with AI helping inside their chosen areas. They need direct placement, editable layers and an output that keeps the intended appearance.

**Secondary:** developers who want a visual starting point for websites and app screens, and ordinary exported web files. App screens currently export as web files; native application packaging is not the promised output.

**Not the target:** people who want a finished output generated from a single sentence with no further input. Chat-first tools already serve them.

## Business model

| Phase | Model |
| --- | --- |
| Now | Free, GPL-3.0, **bring your own key (BYOK)**. |
| Later | **Paid hosted generation:** a Desegnos service runs generation, so users don't need their own key. BYOK remains available. |

Implications for current work:
- **Hosted generation is a provider.** Build it as one more adapter behind the `desegnos-ai` provider boundary, with the same frozen requirements, streaming, cancellation and usage reporting as the others. Nothing in the editor should assume a user-supplied key.
- **Report usage in provider-neutral terms.** Usage and cost display must already cover "plan/credits" vs. "USD estimate", as the ChatGPT path does today.
- **Accounts later.** A paid service will need sign-in, billing and a server. Don't build them now, but keep account state isolated in native code, the way ChatGPT accounts are handled.

## AI providers

The ChatGPT-plan path and OpenAI-only support exist **for testing only**. The goal is **any major AI provider**: OpenAI, Anthropic, Google, OpenRouter, and local OpenAI-compatible servers.

- **Don't advertise ChatGPT-plan sign-in.** It stays available but should not be the headline. Further work on it is low priority.
- **Choose models per task.** Each provider declares its capabilities (vision, image generation, streaming), and features ask for a capability rather than a provider.

## Platforms

**Windows, macOS and Linux**, all as first-class platforms. Only Linux is verified today.
- **Code signing.** Start Apple Developer and Windows code-signing enrollment early, since lead times are long.
- **Test runners.** Native tests need Windows and macOS runners before release.

## Success measure

**"I can make an image, deck, website or app screen look how I imagined it, edit it, and keep that appearance when I export and reopen it."**

To make this testable:
- **Placement fidelity score.** For each benchmark scenario, compare each request box with the bounds of the element generated for it: relative order, alignment, size ratio, and whether it stayed inside its area. Record the score per run in [the generation benchmark](docs/GENERATION_BENCHMARK.md).
- **Owner rating.** For each scenario: "looks like what I imagined" yes or no.
- **Kept rate.** The share of generations kept without Undo, measured in benchmark runs.
- **Time to a finished output.** From a new project to an exported image, deck, website or app screen that the owner rates "done".
- **Round-trip appearance.** Check the edited design, opened export and reopened project for each of the [three milestone examples](CANVAS.md#next-milestone-three-finished-examples). Current capability evidence is scoped in [CANVAS.md](CANVAS.md#capability-status).

## Feedback and diagnostics

- **Opt-in only.** Crash and error reporting is off by default and is offered once, in plain language. Reports never include prompts, site content, keys or file paths.
- **Send feedback.** Add a **Menu → Help → Send feedback** entry that opens a pre-filled issue or email with the app version and platform only.

## Testing with people

The owner tests by hand after the UX audit and Tier 1 features land. After that, run a few short sessions with people in the primary audience: they make an image, short deck or two-page website without help while someone watches. File what you see against UX_AUDIT.md.

## Launch (later)

Not planned in detail until Tier 1 works. Meanwhile:
- **Capture demo material as you go:** save good benchmark results as sample projects and screen recordings.
- **Before announcing, check the name.** Do a trademark and domain check for "Desegnos".

## Working process

- **Version control from now on.** Every change goes into git, one branch per feature or audit item, merged once its checks pass.
- **Small CI:** frontend build, Rust tests, clippy, canvas tests. Native workflow tests run locally until runners exist.
- **Evidence per change:** one native workflow test and screenshots of key states. `test-results/` is not committed. Keep only the latest run per feature locally.
- **Document order of authority:** PRODUCT.md (direction) → CANVAS.md and RELEASE_PLAN.md (current milestone and gates) → DESIGN.md (visual rules). FEATURE_PRIORITIES.md and UX_AUDIT.md retain earlier research and audit context.
