# Generation quality benchmark

Status: **UNVERIFIED**. Prepared on 19 October 2026. Codex has made **no paid calls**, loaded **no owner credentials**, and assigned **no quality ratings**. Offline fixture validation proves valid inputs, not model quality.

This implements preparation for priority 1 in `FEATURE_PRIORITIES.md`. Later feature work does not establish this gate.

## Fixed scenarios

The complete prompts, page sizes, placement boxes, existing source and review criteria are frozen in [scenarios.json](../benchmarks/generation/scenarios.json). Each row has a real, validated `.desegni` input in [projects](../benchmarks/generation/projects). [fixtures.json](../benchmarks/generation/fixtures.json) records archive fingerprints and project IDs. The runner verifies both the fingerprints and the current core/storage validation before execution.

| ID | Scenario | Interpretation | Owner checks beyond basic usability |
| --- | --- | --- | --- |
| 01-hero | Studio landing hero | Flexible | Clear hierarchy, useful action and self-contained visual |
| 02-pricing | Three-tier pricing | Precise | Exact prices, comparable benefits, recommended tier, honest local interaction |
| 03-contact | Contact form | Precise | Labels, native validation and no false “message sent” claim |
| 04-about | Two-column About | Flexible | Balanced columns, reading order and narrow-screen stacking |
| 05-mobile-home | Complete 390 px homepage | Flexible | Touch targets, readable content and no horizontal overflow |
| 06-button-revision | Bigger green booking button | Precise | Requested height/color; existing text, behavior and other content preserved |
| 07-multi-flexible | Header, hero, services, footer | Flexible | Coherent interpretation of all four requests |
| 08-multi-precise | The same four requests | Precise | Header → hero → grouped services → footer; compare with 07 |
| 09-imported-brand | Already approved imported identity | Precise | Exact palette, actual supplied star logo and consistent language |
| 10-faq | Accessible FAQ | Precise | Five native keyboard-operable disclosures and visible focus |
| 11-testimonials | Sample testimonial cards | Flexible | Clear sample labels, balanced copy, no invented client claims |
| 12-navigation-footer | Navigation and footer | Precise | Navigation → content sections → footer, working local section links |
| 13-mobile-contact | 390 px contact form | Precise | One-column controls, honest local review state and no overflow |
| 14-dashboard | Summary cards and project list | Flexible | Scannable hierarchy and visibly identified demonstration data |
| 15-long-copy | Long reading page | Precise | Requested section order, 80–100 words per section, readable line length |
| 16-two-pages | Linked Home and About | Precise | Both pages complete; native links work in the exported site |
| 17-responsive-layout | Wide hero and narrow adaptation | Precise | Hero group before feature group; 390 px adaptation preserves content |
| 18-preserve-revision | Targeted booking-button revision | Precise | Only intended button changes; unrelated source and behavior remain intact |

Every scenario also requires readable, complete content, keyboard access, sufficient contrast, and truthful treatment of unavailable services/assets. “Usable without manual fixes” means the owner could use the generated result for its stated purpose without correcting code, copy, layout or interactions. Cosmetic preferences alone need not fail an otherwise usable design; record them in notes.

The imported-brand case uses a deliberately fixed, already approved identity with actual portable SVG bytes. It does **not** pretend that an AI identity-analysis request was performed. The element-revision cases start from real accepted HTML/CSS/JavaScript and a source-hash-bound button target. Scenarios 07 and 08 have identical descriptions and placement, with only their global interpretation differing.

## Owner execution

Run one scenario at a time. The runner submits once; it never retries. A new explicit invocation is another potentially chargeable attempt.

1. Build the current **debug-only native-tests** app. This build exposes local automation and must not be distributed or used as the normal editor:

   ```sh
   CARGO_TARGET_DIR="$PWD/target/benchmark-native" node scripts/tauri.mjs build --debug --no-bundle --features native-tests --config src-tauri/tauri.test.conf.json
   ```

2. Validate the inputs and inspect the chosen case without connecting AI:

   ```sh
   node scripts/generation-benchmark.mjs --validate-fixtures
   node scripts/generation-benchmark.mjs --dry-run --scenario 01-hero
   ```

3. Run it yourself with a hidden password prompt. The key goes through stdin into native **session** storage; never put it in arguments, an environment variable, a project or a shell-history command. On this Linux system:

   ```sh
   systemd-ask-password 'OpenAI API key' | node scripts/generation-benchmark.mjs --scenario 01-hero --binary "$PWD/target/benchmark-native/debug/desegnos" --budget-usd 1 --max-seconds 180 --key-stdin --allow-paid-generation
   ```

   The explicit flag authorizes this owner-run attempt. Codex must not run that command. The budget blocks requests whose conservative native estimate exceeds it; it is **not a guaranteed billing cap**. A timeout cancels waiting, and submitted work can still incur charges. The default model and reasoning/output settings come from the fresh native profile and are recorded in the result.

4. Open the printed evidence folder. Review its screenshots and `result.desegni` in Desegnos, including interaction behavior and the narrow-screen requirements where relevant. Fill `owner-rating.json` with your name, review timestamp, `usableWithoutManualFixes` and, for Precise scenarios, `strictOrderAndGrouping`. Enter confirmed billing in `actualBilledUsd` if available; otherwise leave it null. Leave explanatory notes for any failure.

5. Repeat for the remaining fixed cases, then summarize the whole evidence directory:

   ```sh
   node scripts/generation-benchmark.mjs --summarize test-results/benchmark
   ```

The Linux runner uses the existing guarded Xvfb route with an unused display ≥90, private session bus, private profile and app-owned window verification. It preserves and rechecks the main desktop's display resources. No test window is moved to the main monitor. Windows/macOS execution remains unsupported and is reported as such.

## Evidence and boundaries

Each run has a unique directory under `test-results/benchmark/<UTC-date>/`; rerunning cannot replace an earlier attempt. Evidence is private to the owner:

- `input.desegni`: unchanged copy of the fixed input; `working.desegni` and `result.desegni`: the real result saved through the native pipeline.
- `REQUIREMENTS.md` and `prepared.json`: exact native requirements, digest, settings and conservative pre-submission estimate.
- `provider-output.txt`: exact successful transport output text before candidate parsing, copied from the opt-in debug capture. This is **not reconstructed candidate JSON** and **not raw HTTP headers or the SSE envelope**. Unsafe/rejected transport bodies are not retained; missing output is reported honestly.
- `job.json`, `project.json` and `result.json`: actual outcome, source, usage, native attempts, duration and frozen catalogue prices. A provider failure remains a failure; no fabricated page is substituted.
- `page-<id>.png` and `layout-<id>.json`: actual owned-window captures and rendered semantic-element positions for each page. Captures show the current viewport; use the saved result to inspect content below it and interactions.
- `owner-rating.json`: deliberately unrated until the owner reviews it. Generated content cannot rate itself.

The runner uses the existing native `prepare_generation`, `start_generation` and transactional `apply_generation` commands. It never creates a provider HTTP request, changes core validation, bypasses stale-source checks, or installs components/plugins. Preview scripts remain paused for captures. The debug benchmark flag selects an unavailable system vault with the fixed production HTTPS client; loopback/test endpoint environment variables are excluded. Native successful-output capture is compiled only with debug assertions plus `native-tests` and requires the opt-in flag. Production builds gain no benchmark endpoint or raw-output retention.

API keys stay in the runner/native process memory and are cleared from native session storage during cleanup. JavaScript strings cannot promise memory zeroization; no key is written to the run profile, output, logs or archive. The owner should review content before sharing evidence. Native auth headers and rejected credential-echo bodies are never captured.

## Passing the gate

The gate stays **UNVERIFIED** until all 18 cases have real attempts, available outcome evidence, owner ratings, and recorded duration and cost information. At least **15 of 18** must have complete applied output and be rated usable without manual fixes. Failed attempts count as unusable, rather than disappearing from the set. Every Precise case must also pass the owner's relative-order/grouping check. No failed, cancelled, missing or unrated case is silently removed from the denominator.

The summary uses the **newest attempt for each scenario**, including a newer failure. Token-based cost is a labelled estimate using the frozen native catalogue; it is not confirmed billing and can differ because of cached input, reasoning, long-context pricing or provider adjustments. Report confirmed billing separately when available. Missing usage requires confirmed billing from the owner to establish cost; missing duration blocks the gate.

## Preparation checks and iteration log

Offline checks performed: all 18 archives load through current core/storage, validate against their exact fixed definitions, prepare real requirements without provider access, and match recorded SHA-256 fingerprints. Help, dry-run, missing-consent rejection and unrated-summary behavior are checked separately. No live benchmark, cost observation, owner rating or paid request has been performed by Codex. The owner-run native/live path remains unexercised until the owner runs it.

| Date | Change | Evidence/effect |
| --- | --- | --- |
| 2026-10-19 | Established version 1, 18 fixed native project inputs and owner-run evidence capture | Offline inputs valid; generation quality **UNVERIFIED** |

After a real failure, append its scenario/run, the concrete problem, the exact requirement or instruction change and the next measured result. Preserve old runs and version changed fixtures. Do not claim prompt improvements from synthetic provider responses or offline validation.

The small [fixture builder](../benchmarks/generation/fixture-builder/src/main.rs) uses existing core/storage libraries only. Check mode is read-only. Write mode refuses existing archives; intentional benchmark revisions should use reviewed new inputs rather than overwrite evidence.
