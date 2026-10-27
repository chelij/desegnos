# Desegnos product direction

Date: 2026-10-19. Owner decisions. Where this file and [RELEASE_PLAN.md](RELEASE_PLAN.md) disagree on product direction, this file wins; the release plan still governs technical and safety contracts. Work order is in [FEATURE_PRIORITIES.md](FEATURE_PRIORITIES.md) and [UX_AUDIT.md](UX_AUDIT.md).

## What Desegnos is

**A visual canvas with AI.** A desktop design tool where you place things exactly where you want them on an infinite canvas, by hand or by describing them. Everything stays visual while you work; at export the AI builds real HTML, CSS and JavaScript that you own, and slides and images export directly. See [CANVAS.md](CANVAS.md).

## What makes it different

**Placement and control.** Other AI site builders are chat-first: you describe a page and accept what comes back. In Desegnos, you decide where things go and the AI fills them in. When the choice is between AI convenience and user control, control wins.

What this means in practice:
- **Placement fidelity is the core quality measure.** AI-made elements land in the request that asked for them, and exports keep elements where they were placed, at about that size, in that order and grouping. Precise mode honours this closely.
- **Visual first.** Work happens on the canvas, not in code. Code exists only in exports, as ordinary files users can read and edit.
- **Changes are scoped.** A request changes only the layer it targets, and everything else stays as it was. Pointers are the explicit way to let a request reach further.
- **The output is ordinary files** that open without Desegnos and are not locked to any host.

## Who it is for

**Primary:** people who know what they want their site to look like and want to place it themselves, without hand-writing all the code. Examples: designers, hobbyists, small-business owners and freelancers, i.e. people who would have used Dreamweaver or a visual builder.

**Secondary:** developers who want a fast visual starting point and clean exported code.

**Not the target:** people who want a site generated from a single sentence with no further input. Chat-first tools already serve them.

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

**"I can get a site looking like how I imagined it."**

To make this testable:
- **Placement fidelity score.** For each benchmark scenario, compare each request box with the bounds of the element generated for it: relative order, alignment, size ratio, and whether it stayed inside its area. Record the score per run in `docs/GENERATION_BENCHMARK.md`.
- **Owner rating.** For each scenario: "looks like what I imagined" yes or no.
- **Kept rate.** The share of generations kept without Undo, measured in benchmark runs.
- **Time to a finished page.** From a new project to an exported page that the owner rates "done".

## Feedback and diagnostics

- **Opt-in only.** Crash and error reporting is off by default and is offered once, in plain language. Reports never include prompts, site content, keys or file paths.
- **Send feedback.** Add a **Menu → Help → Send feedback** entry that opens a pre-filled issue or email with the app version and platform only.

## Testing with people

The owner tests by hand after the UX audit and Tier 1 features land. After that, run a few short sessions with people in the primary audience: they build a page without help while someone watches. File what you see against UX_AUDIT.md.

## Launch (later)

Not planned in detail until Tier 1 works. Meanwhile:
- **Capture demo material as you go:** save good benchmark results as sample projects and screen recordings.
- **Before announcing, check the name.** Do a trademark and domain check for "Desegnos".

## Working process

- **Version control from now on.** Every change goes into git, one branch per feature or audit item, merged once its checks pass.
- **Small CI:** frontend build, Rust tests, clippy, canvas tests. Native workflow tests run locally until runners exist.
- **Evidence per change:** one native workflow test and screenshots of key states. `test-results/` is not committed. Keep only the latest run per feature locally.
- **Document order of authority:** PRODUCT.md (direction) → FEATURE_PRIORITIES.md and UX_AUDIT.md (order of work) → RELEASE_PLAN.md (technical contracts) → DESIGN.md (visual rules).
