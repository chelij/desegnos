# OpenAI generation contract

New requests use **format 18**, including rectangle, element, shaped and imported-page requests.
Formats 3–17 retain their frozen Markdown, schemas and application behavior.

Format 18 uses the request layers shown on the canvas: ascending `zIndex`, with
original request order breaking ties. The bottom-most unfinished request is the
base layer. It becomes page-scoped when its rectangle covers at least 95% of measured
page width (viewport fallback) and either 90% of the reference viewport near the top
(within 10% of viewport height), or 90% of measured content height (viewport fallback).
Full-width rectangles with explicit page-layout intent also become page-scoped,
even below the fold or above the base: phrases include “one page one screen”,
“one screen only”, “single screen”, “fit on/in one screen”, “fit the/this page to”,
“page height”, “page layout”, and “viewport height”. This bounded phrase recognizer
is not a semantic classifier; unrecognized wording can use the explicit toggle.
Coverage uses intersection, so a displaced box does not gain authority merely by
being large. Element selections, shaped areas and phone fixes do not auto-expand.
The request's **Whole page** control forces page scope; turning it off persists
`wholePageDisabled`, preventing auto-detection from turning it back on. Both fields
default false and are omitted when false. Rectangular requests now receive the same
validated element measurements as shaped requests before generation.

Page requests receive complete source and may restructure HTML and change shared
CSS/JavaScript. Existing IDs remain unless selected-root `allowTargetRemoval`
authorizes removal of that subtree, or a removal prompt explicitly names the ID.
Broad layout prompts such as “one screen only” do not authorize guessing which
content to delete. Protected region content and protected literal decision values
already present in the source remain protected; semantic design decisions remain
model instructions. Source syntax, IDs, assets, remote-code and page ownership checks
still apply. Failed whole-page validation rejects the result instead of attempting
to reconstruct it as independent area roots.

When a page request and ordinary areas share a page, the implemented page request
owns shared CSS/JavaScript and surrounding structure. Apply the local requests
inside that result from bottom to top, retaining their selected roots/IDs or region
roots. Where boxes overlap, the higher request wins inside its own box. A below-fold
page-layout request refines the page-wide layout in this same order. Requirements
list requests in that order and include `layerOrder`; frozen requirements also hash
all unfinished request layers, so moving even an unsubmitted layer invalidates a
snapshot whose base classification could change. Global changes come from the page
request, never from an area. Element-fix and partial/conflict rules cannot freeze
the page request's shared code or surrounding structure. When native image editing
is available, a transparent-image request uses the original asset as the reference;
it does not conflict merely because the operation needs new pixels. A conflicted page request grants no
page authority; conflicted local targets/roots remain unchanged. Other pages keep
their own scope rules. A single combined page reply cannot attribute each HTML change
to an individual prompt; validation checks the combined authorized page scope and
local root/ID requirements, while requirements instruct the model on precedence.

Format 17 delivers regions independently: return pages containing implemented regions even
when another region conflicts. Apply completes only implemented requests, leaves conflicts
open, and retains their explanations in the job and project notes. Conflicted selected
elements/region roots stay unchanged; new conflicts use a short local placeholder.
Mixed existing pages preserve shared CSS/JavaScript. If a conflicting rectangle has no
old DOM mapping, existing content outside ready roots/selected elements must remain unchanged.
Pages containing only conflicts are omitted. Jobs with any validated ready source can apply.

OpenAI jobs with confirmed Image API access advertise optional `capabilities.imageGeneration: {maxImages: 2,
sizes: ["1024x1024"]}`. The settings switch defaults on for supported connections.
An HTML `img` requests native processing with `data-desegnos-generate="prompt"`;
`data-desegnos-source` optionally names a frozen request attachment or exact
`desegnos-asset:<uuid>` reference. `data-desegnos-background="transparent"` requests
an alpha PNG; the default is opaque. Size is fixed to `1024x1024`.

After the main structured reply and image downloads, native code calls
`POST https://api.openai.com/v1/images/generations` with `gpt-image-2.5-flare`,
or `/v1/images/edits` with `gpt-image-2.5-sunburst` and a multipart reference image.
Models are listed together in `image_generation.rs`. Both use low quality and PNG output.
`data-desegnos-mask` optionally names a PNG attachment or project asset; its alpha mask
applies to the first reference. Original reference/mask bytes are sent within the existing
16 MiB request bound, preserving mask dimensions and alpha. Remote references are forbidden.
Image API JSON (`data[].b64_json`) is separate from the candidate-text SSE parser.
At most two distinct calls and 64 placements are accepted, with 4,096 characters per
prompt, a 12 MiB wire-response ceiling, bounded PNG decoding, 256 KiB per stored PNG
and 512 KiB total. PNGs are reduced as needed without flattening their alpha.
The estimate reserves USD 0.50 per permitted image before main submission; the
spending limit checks the combined estimate. This is an allowance, not a billing cap.
Calls use cancellation/deadlines and never automatically retry.
Failures, refusals, unavailable references at processing time, and excess requests leave
a visible “Image generation failed” slot while keeping other content applicable.
On apply, existing picture storage converts accepted inline results into shared
`desegnos-asset:` references. The job log retains thumbnails and the original prompts.

Both existing OpenAI text paths use `/v1/responses`: API keys use bearer keys;
ChatGPT sign-in uses the OAuth resource token obtained for
`https://api.openai.com/v1` with `resource.invoke chatgpt.tokens.use.direct`.
Image access is checked independently for the active credential first, then the optional
Image API key. The free probe posts the default image model with invalid `size: "16x16"`:
only a 400 validation error about that parameter confirms access. Authorization, subscription
sharing and organization-verification errors disable that credential; transient failures
remain unknown. Error details are scrubbed before caching in `image-api-access.json`.
This separate cache intentionally ignores old Responses-tool probe results.
The main credential wins when both have confirmed access; otherwise a confirmed image key
is used. Its secret lives in the same secure credential store/session mechanism as API keys,
under a separate service, never in plain settings or job files. AI settings report both
probe results and the selected credential. ChatGPT text usage can stay on the plan while
images use the separately billed key and existing spending allowance. API Organization
Verification may be required. Other providers omit the image capability.
Responses bodies for ChatGPT sign-ins never include the built-in `image_generation` tool;
the existing structured `data-desegnos-generate` request mechanism remains in use.
See the [official Image API guide](https://developers.openai.com/api/docs/guides/image-generation).

The following notes describe earlier versions. Rectangle and element requests previously
used **format 14**. Shaped area requests used **format 15**:
`placementHint` remains the bounding box, with an inscribed ellipse or rounded page-pixel
freehand polygon alongside it. The browser measures existing element boxes before
generation; Rust includes an element when its box intersects the actual shape, using
32 vertices for ellipses. Format 15 lists those IDs and protects elements outside the
shape, including descendants of intersecting containers. Existing CSS/JavaScript stay
unchanged, with the existing selected-ID CSS addition rules applied to in-area IDs.
Freehand paths use even-odd fill and normalized points in schema 8; absent shape means
rectangle, so no project migration is needed. Measurement is bounded to 10,000 elements
per page; larger pages can use element-target requests. Formats 3–14 retain their frozen
wording and default rectangle serialization.

Format 14 is format 13 plus readable code at no token cost.
The AI writes every page as one long line; after a generation Desegnos stores HTML and CSS
in a readable layout (`format_site_content`), and requirements send pages compact again
(`compact_site_content`). HTML only gains or loses whitespace-only text where it cannot
render: between block-level elements, never inside pre/textarea/script/style and never
beside elements the page's CSS makes inline, floated, positioned, hidden or whitespace-
preserving; a page with nothing to lay out stays byte for byte as written. CSS is reprinted
by lightningcss (comments are dropped). JavaScript is left exactly as written. Scope checks
compact both the stored page and the AI's answer before comparing them.

Format 13 is format 12 plus picture previews: each
image attachment is sent beside REQUIREMENTS.md as a low-detail JPEG, shrunk by a
quarter at a time until it is at most 512 px and 48 KiB (one image tile on every
provider). The estimate reserves 512 tokens per preview instead of counting its
Base64 bytes. The AI places the picture with its largest displayed `width`/`height`;
on apply the original is scaled to twice that width (or the page width when no
width is given), stored once in `pictures`, and the request's picture attachments
are dropped, so only the displayed copy is kept. GIFs are inserted unchanged.
Pictures no page uses any more are removed from the store; Undo keeps its own copy.
There is no limit on attachment count, picture size or picture store size.

Format 12: the result schema travels only as the strict
structured-output schema; REQUIREMENTS.md contains the instructions without a
second written schema. Formats 3–11 retain their frozen rendering. Existing-element
requests still send selected outer HTML, a stylesheet and a bounded outline. Their
reply contains replacements, `needs`, and a per-page `css` field for additions only.
New CSS selectors must match a selected root's own `data-desegnos-id`, optionally
with supported state/pseudo-element selectors. Media queries are allowed; descendant,
sibling, selector-function, nested-selector and global rules are rejected. Existing
CSS and JavaScript remain unchanged.

An automatic `needs` round is frozen as **CONTEXT.md** before sending. The job view
exposes `context`, and usage includes both rounds, including a failed second round.
Failed final answers and non-stream replies retain at most 64 KiB of scrubbed text
as `rawReply` and **REPLY.txt**; a reply containing the active credential is omitted.
Retry archives prior context and replies. These local diagnostics are outside project
archives. The AI log shows both rounds, failed replies and read-only past jobs.

`list_generations({projectId})` returns up to 100 newest local job summaries (time,
status, first frozen request, summary and tokens), plus skipped/truncated indicators.
Scanning is bounded to 4,096 job directories. `get_past_generation({projectId, jobId})`
opens an existing record without changing the active job or allowing replay.

Project schema **8** stores placed image URLs as `desegnos-asset:<uuid>` references
and shared, content-addressed bytes in `pictures`. Opening an older project extracts
inline images after checking its original integrity. The source archive stays intact
until explicit Save, which retains its normal previous-file backup. Previews, image
editing/copying and export resolve references at their output boundary. Placed bytes
survive replacement of the import catalogue. The older version notes below describe
the earlier contract history.

Current draft policy and native image adapter: [Draft content and downloaded images](DRAFT_IMAGES.md). Requirements format 7 permits visibly labelled missing-content placeholders, honest disabled actions and system-font fallbacks. Explicit image markers request bounded native PNG/JPEG downloads; provider tools and preview networking remain disabled. Formats 3–6 retain their frozen contracts. Successful generations apply directly on the canvas; the earlier review/send description below is historical.

Implementation gate: Generate freezes Desegnos's requirements Markdown and global mode, sends the exact UTF-8 file bytes to OpenAI, receives a bounded structured candidate, previews it in an unprivileged frame, and offers transactional Apply/Discard. Provider HTTP and credentials cannot mutate projects. Core validation and the native job coordinator cannot be bypassed by a provider or plugin.

API keys and authorized ChatGPT plans share this core contract. The [ChatGPT authentication contract](CHATGPT_CONTRACT.md) adds native verified identity, account registration/storage/refresh/revocation and account-specific discovery. Every job freezes `authentication: apiKey | chatgpt`, nullable `chatgptAccountId` and safe nullable `failureKind`; missing fields in old records default to API-key mode. No selection/preference change can redirect an existing send or retry. Sign-out cancels bound work before revocation. Plan limits and failed streams never fall back to API billing.

## Ownership and work

- Core: immutable requirements and dependency hashes; typed site source/candidate/schema; target coverage; proposal validation; atomic apply/undo; schema migration and preserved extension data.
- Provider module: fixed OpenAI HTTPS endpoint, native credential vault/session memory, verified model catalogue, request/stream parsing, bounded usage and transport errors. It accepts frozen requirements and an output schema, returns source text/usage, and has no project or filesystem authority.
- Native coordinator: credential/config lifecycle, disclosure/cost gate, durable job intent/attempts/results, cancellation, stale checks, checkpoints, preview resource registration and project application.
- Editor: top-bar settings, global mode and Generate, disclosure/review dialogs, progress/cancel, isolated preview, selected identity proposals and Apply/Discard. No sidebars or secret persistence in JavaScript.

Generate immediately shows preparation, then Ready to send with a visible top-level Send and generate action. The AI has not started at that point. The concise review shows requests, affected pages, global mode and cost/plan use. Its More menu exposes exact requirements, digest, source, attempts, native byte counts and detailed model settings. After sending, the dialog and a horizontal strip above the canvas display waiting/response/completion state and elapsed time from the actual native attempt. Preparation and other editor operations use their own local start time. Closing review preserves polling and returns keyboard focus to View progress; reopening never submits a request. Terminal attempt durations stop at their native finish timestamp. Status stage changes are polite announcements; second-by-second timer/byte updates are not live announcements. No percentage or completion estimate is fabricated.

## Core data

Project schema 4 retains `Page.content: SiteContent | null` and `Project.appliedJobIds: UUID[]`, and adds persisted grid settings and the latest complete generation dossier. Request rectangles use signed workspace coordinates bounded to ±100,000 CSS pixels, independently of the screen guide. Requirements format 4 freezes these coordinates, placement percentages relative to the guide and relevant dossier context. Existing format 3 snapshots retain their original rendering and digest, with absent dossier context omitted. SiteContent is `{html, css, javascript}`: an HTML body fragment, stylesheet, and browser JavaScript source. Generated code runs only in an opaque-origin sandbox. Core bounds all source and IDs, preserves untouched pages, and never accepts model filenames, commands or installs. Source and dossier are saved/recovered project state, not plugin extensions.

GenerationCandidate is `{jobId, requirementsSha256, summary, regions, pages, componentRequests, decisionProposals, criticism}`.

- regions: `{regionId, status: 'implemented' | 'conflict', explanation}`; exactly one entry for every frozen target.
- pages: `{pageId, content: SiteContent}`; exactly the affected pages when all required regions are implemented, no unrequested pages.
- componentRequests: `{regionId, capability, resolution: 'native' | 'unavailable', reason}`. This gate supplies no qualified external components. Native HTML/DOM behavior is available; unavailable required dependencies must produce explicit conflicts. Providers cannot invent a catalogue entry or install a package.
- decisionProposals: `{topic, value, scope, rationale}` using the existing scoped decision shape. Compatible proposals start selected and remain suggestions until the user applies that review. Users can uncheck them. Applying a candidate cannot replace a conflicting accepted/protected decision. New accepted proposals and source commit together.
- criticism: `{regionId: UUID | null, message, suggestion}`; advisory design feedback, never measured accessibility claims.

Core exports `candidate_schema() -> serde_json::Value`, `GenerationCandidate::validate(snapshot, project)` and readiness/conflict checks. `Project::apply_generation(snapshot, candidate, accepted_decision_indices)` returns a validated next revision. `Session::apply_generation(snapshot, candidate, accepted_decision_indices, durable)` uses the same durable-before-commit and undo rules as ordinary commands. Candidate application is not an arbitrary frontend edit command. Requirements include current affected-page source and its hashes; source changes make dependent jobs stale. Schema 1, 2 and 3 readers validate original hashes/PROJECT.md before migrating without changing original bytes.

`Project::record_generation(snapshot, candidate, attempt_ordinal)` and the corresponding durable Session operation automatically record a complete validated draft: real summary, targets, proposals, criticism, digests and provenance. Repeating the same attempt/candidate is a no-op. Older or stale output cannot replace current context. Drafts remain nonbinding; accepted choices and protection stay authoritative. Recording does not add an Undo item, clear Redo, or make the job invalidate its own requirements. The session accepts a renderer revision across automatic dossier-only writes, while still rejecting revisions superseded by user edits. Apply marks the dossier applied; Undo marks that application undone; Redo records the revision that restores it.

An applied job ID remains consumed through Undo, saved archives and recovery. A duplicate Apply cannot replay the transaction. A complete candidate blocked by changed dependencies can become ready again only when the exact frozen dependencies are restored and core validation passes; restoring a candidate never sends another provider request.

## Native command shapes

All JSON uses camelCase. Only the trusted editor can call these commands.

- `get_ai_settings() -> AiSettingsView`
- `save_ai_settings({settings: ProviderSettings}) -> AiSettingsView`
- `set_ai_key({key: string, storage: 'system' | 'session'}) -> AiSettingsView`
- `remove_ai_key() -> AiSettingsView`
- `test_ai_access() -> {message: string}`: authenticated model-access read, no generation call.
- `prepare_generation() -> GenerationJobView`: freezes the Rust requirements and settings, estimates cost, persists a prepared job and returns the exact Markdown for disclosure.
- `get_generation({projectId: UUID}) -> GenerationJobView | null`: most recent job for the active project; includes terminal results after restart and reconciles eligible dossier writes locally. This operation never sends a provider request.
- `start_generation({jobId: UUID}) -> GenerationJobView`: explicit send of the reviewed frozen job, returns promptly and streams in the background.
- `cancel_generation({jobId: UUID}) -> GenerationJobView`: persists cancellation and prevents late responses becoming candidates.
- `discard_generation({jobId: UUID}) -> GenerationJobView`: no project mutation.
- `retry_generation({jobId: UUID}) -> GenerationJobView`: explicit retry of the same frozen snapshot; preserves prior attempt/usage/uncertain-outcome records.
- `apply_generation({jobId: UUID, acceptedDecisionIndices: number[]}) -> WorkspaceView`: validates current dependencies, durably checkpoints, commits source/selected identity/job ID atomically, records completion without replaying paid calls.
- `create_preview({pageId: UUID, jobId: UUID | null, runScripts: boolean}) -> {url: string, sessionId: UUID}`: immutable registered bytes, accepted page or complete candidate page; scripts paused unless explicitly enabled.

ProviderSettings is `{model, maxOutputTokens, reasoningEffort, spendingLimitUsd}`. Initial verified catalogue contains `gpt-6.1-sol`, low/medium/high reasoning and an API cap of 32,768 output tokens. API defaults: medium, 32,768 tokens, USD 2 estimated job allowance. AiSettingsView includes `settings`, masked `credential`, verified `catalogue`, `configurationError`, `authentication` and the public `chatgpt` view defined in the linked contract. The plan catalogue is the account's ordered verified intersection, with server display names. ModelInfo exposes verified limits, ID/name, dated API prices and supported reasoning levels. Keys/tokens never appear in returned views, projects, job records or diagnostics. The strict wire schema uses the documented object/array/anyOf/pattern subset; Desegnos independently enforces every text and source byte limit.

GenerationJobView includes `{jobId, projectId, baseRevision, requirements, generationMode, settings, authentication, chatgptAccountId, failureKind, providerError, status, estimateUsd, receivedBytes, message, candidate, usage, attempts, acceptedDecisionIndices, recommendedDecisionIndices, dossierError}`. `providerError` and `dossierError` default to null and `recommendedDecisionIndices` to an empty array in older saved jobs. Requirements is the existing RequirementsSnapshot. Nullable candidate/usage are actual absence, not sample data. Status is prepared, generating, ready, blocked, failed, unknown, cancelled, discarded, applying or applied. Usage is `{inputTokens, outputTokens, totalTokens}` when reported by OpenAI. Attempts record ordinal, status, timestamps, response ID and optional usage. Core rejection is blocked with an actionable message; refusal/incomplete/invalid output never becomes ready. Restarted in-flight work is interrupted/unknown and is never automatically resubmitted. Retry requires the original snapshot still be valid. Local job/dossier save errors are visible and can recover without a provider retry.

`providerError` exposes the native error's bounded code, parameter, HTTP status and request ID through More → Technical → Request details. Errors can add `responseFormat: {contentType, bodyKind, bodyBytes, providerMessage?, bodyTruncated, limitBytes?, rejection?}`. Older records default `bodyTruncated` to false and `limitBytes`/`rejection` to absent. `bodyKind` can identify a generation event stream as well as JSON errors, JSON details, other JSON, HTML, text, empty, binary or unavailable replies. A partial body count describes bytes actually consumed, which can exceed the retained diagnostic prefix; it does not claim the total reply size. `limitBytes` identifies the particular reader's bound. `rejection` is a closed native enum identifying the failed validation check: event/response identity, usage, message role/state/phase, text location/identity/order/consistency, output/content shape, final-answer ambiguity, credential echo, candidate JSON/object shape or exact job/requirements binding. Technical details render static labels for those codes. No response text, provider-selected field name, invalid value or JSON excerpt is retained in this diagnostic.

Media types compare without case sensitivity. A declared event stream enters the normal decoder immediately. If its Content-Type is absent or mislabeled, the native HTTP reader incrementally recognizes an initial SSE field (`data`, `event`, `id` or `retry`) or comment, allowing a leading UTF-8 BOM, blank lines and fields split between chunks. It passes the buffered bytes unchanged into the same decoder. The probe shares the original request, cancellation, deadline and 32 MiB total stream bound. This is a native transport fallback, not a claim about browser EventSource's HTTP acceptance rules. The parser uses the [SSE line and field grammar](https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation); typed Responses events retain their existing [completion contract](https://developers.openai.com/api/docs/guides/streaming-responses).

When HTTP 200 carries a non-stream body, the diagnostic reader retains at most 65,536 bytes under the same cancellation/deadline constraints. It classifies complete explicit JSON errors separately from HTML, text, empty, binary and other JSON replies. Error codes select the existing authentication/plan recovery; explicit errors become failed, while replies without a confirmed terminal result remain unknown. Crossing the diagnostic retention bound reports an unexpected response format with partial-byte/limit metadata, rather than claiming the generated site exceeded its output bound. Clipped JSON is never parsed as a complete error or candidate. Neither diagnostic path becomes a candidate. Raw bodies are never exposed or saved. The optional error-message/admission-detail explanation is limited to 1,024 bytes, normalized as plain text, and withheld if it contains the submitted credential, bearer/key/token patterns, unsafe control characters or directional overrides. It is diagnostic text, never a machine code or instruction. Header metadata containing the submitted credential is withheld. Received-byte counts include consumed error bodies; stream-reported usage is retained. A failure does not claim zero billing or plan use.

Only a validated `response.completed` SSE event can return a candidate; a JSON response received instead of the requested stream is not treated as completion. Job ID, requirements digest, typed event consistency, source/schema and core validation remain mandatory. The existing bounds remain 32 MiB for the wire stream, 16 MiB plus 65,536 bytes for an event and 8 MiB for candidate text; byte-limit errors identify the actual bound. No response-format failure triggers a second request, changes billing, or weakens core validation. An explicit retry archives the previous provider error as `attempt-N-error.json` before recording the new intent and keeps the frozen requirements, mode and account. Older saved non-stream failures get an honest derived message explaining that the original response details were not retained. The specific older zero-byte HTTP 200 diagnostic-limit record explains that its 64 KiB error reader stopped before the reply was identified; original site size and usage remain unknown. Reading either legacy view neither rewrites the original record nor sends a request. This follows OpenAI's [inference completion](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference) and [error metadata guidance](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery).

Output text is tracked separately by wire output index, content index and item identity. Wire indices remain stable during a stream, but may contain gaps and are not positions in the terminal output array. Completion correlates text by unique message ID plus its unchanged content index. Each observed delta/done must match that completed text part; later deltas after a part or item is finalized are rejected. The earlier single-part format may omit indices/identity only when it remains unambiguous; it cannot identify multiple parts by arrival order. All intermediate and final text shares the existing 8 MiB bound. At completion, native code validates every assistant message/content item, excludes only explicitly labeled `commentary`, and requires exactly one remaining answer. A single unphased assistant message remains supported. Unknown phases, multiple final answers, additional ambiguous unphased answers, refusals and inconsistent parts are rejected. Ordered text parts of the selected answer are joined before the existing JSON/job/digest and core checks. Commentary never becomes source or dossier content. This follows the official [output-message phase definition](https://raw.githubusercontent.com/openai/openai-python/main/src/openai/types/responses/response_output_message.py) and [indexed text events](https://developers.openai.com/api/reference/resources/responses/streaming-events). Older completed failures without a recorded rejection code explain that the exact failed check is unknown; reading them preserves their original bytes, attempts and usage.

`response.output_item.added` establishes an index/identity/type binding; empty added items are ignored and counted. `response.output_item.done` validates and caches completed assistant messages. A successful `response.completed` confirms the response, but its output list need not repeat those finalized items. Desegnos merges all omitted finalized messages by stable identity, including final answers. Overlapping terminal messages must agree with their finalized content. This applies to compact, reordered, partial and empty terminal lists; absent/null output also uses finalized items. Unfinished items omitted from terminal output, duplicate IDs, changed bindings and changed messages are rejected. After merging, every observed text part must still match and exactly one non-commentary answer is required. An empty list or partial deltas alone cannot supply an answer. Failure, incomplete, interrupted and malformed terminal events never become success. OpenAI's [Codex Responses parser and explicit-empty-output test](https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/sse/responses.rs#L1345-L1435) treat finalized item content and terminal completion independently. The previous application rule requiring final answers to be repeated was too strict.

Correlation failures add an optional, closed `correlation` object to `responseFormat`: terminal-output presence (`notReceived`, `absent`, `null`, `array`, `invalid`), saturated counts for terminal items/text parts, streamed text parts, finalized items and empty added items; a fixed cause enum; and optional failing output/content indices. It contains no provider message IDs, text, hashes, prompts or credentials. The object survives restart and explicit-retry archival and appears only in More → Technical → Request details. Older records without it remain readable without rewriting. The exact original failed wire reply was not retained, so this metadata diagnoses future failures without claiming to reconstruct that private reply.

The identity-less legacy fallback requires exactly one tracked text part and one completed text part, matching content indices, and no item lifecycle bindings. Any explicit legacy output index must match its terminal array position; omitted indices remain supported for that one unambiguous part. This fallback cannot override established message bindings or correlate multiple messages by text.

## Wire and execution

Use Responses `input_file` with filename REQUIREMENTS.md and Base64 of the exact UTF-8 Markdown; include an input_text record with the job ID, requirements digest and global generation mode. Use `text.format` with the strict core JSON schema, `store: false`, `stream: true`, verified reasoning settings and no tools. API-key requests add standard service tier and the configured output cap. ChatGPT requests omit unsupported output-cap/service-tier fields, reserve the full verified 128,000-token model output capacity and show plan/credit disclosure rather than API estimates. No persistent Files upload or remote file URL. All three responsibilities are represented in the structured candidate; they need not purchase three separate requests.

The provider copies the core schema and binds its `jobId` and `requirementsSha256` properties to single-value enums from the frozen snapshot. Independent exact-value checks still run on the completed answer. The core's shared schema stays generic and provider-neutral. Enums use the documented [Structured Outputs subset](https://developers.openai.com/api/docs/guides/structured-outputs). A per-job schema may incur provider schema preprocessing latency; progress reports elapsed time and actual stages without claiming a fixed completion time.

One active job per project. Persist intent before network submission. Limit input/output bytes; never silently summarize/truncate requirements or targets. Use a five-minute deadline and visible cancellation. No automatic paid retry after an uncertain submission. Cost is a conservative local estimate with a visible verification date; it is not a guaranteed billing cap. Explicit retries may incur another charge.

The preview resource scheme serves only registered immutable HTML/CSS/JS bytes by random session/page IDs. It exposes no filesystem paths. Paused frames use sandbox="" and CSP script-src 'none', with no bridge script. Explicitly enabled frames use sandbox="allow-scripts". Both omit allow-same-origin, forms, popups and top navigation privileges; CSP denies network, external frames, objects, native commands and unregistered resources. No provider key reaches the frame. Only the editor authorizes project application. Saved custom code starts paused when reopened.

Linux setup uses WebKit's public `register_uri_scheme_as_cors_enabled` for the preview scheme. Registered stylesheet/script resources use anonymous CORS and the native handler supplies their CORS header. This permits the opaque frame to load its own registered source without granting same-origin privileges, universal access or CSP bypass. The actual WebKitGTK checks exercise CSS loading, browser interaction, parent/native-command denial and scriptless navigation. The corresponding Windows/macOS loading paths still require native verification.

The native Workspace menu and Ctrl/Cmd+Shift+R recreate the editor with scripts paused while retaining Rust-owned accepted state. A native heartbeat watchdog also offers recovery for an unresponsive enabled preview. Shortcut recovery is exercised against an actual infinite loop; the automatic watchdog dialog remains a separate unverified path.

## Verification gate

Test the provider wire contract with an actual controlled HTTP server: decode the attachment, compare every Markdown byte/hash and mode, stream a complete typed result, and exercise refusals, malformed/truncated output, authentication/quota/rate limits, redirects, size limits, cancellation and uncertain failures. Clearly distinguish this from a live OpenAI check. Exercise the real native UI on a guarded virtual display, including settings, disclosure, progress/cancel, candidate preview/Apply/Discard, stale rejection, saved/recovered generated content and credential non-disclosure. Live access requires private user sign-in/plan consent or key entry in Desegnos. The ChatGPT contract adds signed OAuth, account isolation, rotating-token recovery, plan failures and native secure-store checks.

This first generation gate stores and renders native HTML/CSS/browser-JavaScript source. It does not complete the planned visual document compiler, qualified React catalogue, JSX compilation, media pipeline, portable/React exports or executable plugin host. Submitted annotations remain editable; archiving/clearing submitted drafts and targeted visual-subtree revision are remaining release work. Their absence must stay visible in the implementation record rather than being represented by simulated results.

Requirements format 16 adds imported page context. It is selected only when the project has reviewed source-page assignments. The catalogue nests photos and work under actual affected page IDs and current routes, while identity assets and accepted brand decisions remain project-wide. Photo bytes still travel through verified `desegnos-asset:` references. Format recognition accepts 16; renderers for formats 3–15 are unchanged, including their byte/hash contracts.

Image generation requires an enabled setting and a fresh accepted account probe (models discovery plus a streamed Responses image tool declaration with tool_choice none); unsupported runtime image calls disable that account’s images and finish with placeholders and one notice.

## Validation recovery (format 17)

Recovery runs before application, against the frozen project, in three layers:

1. Normalise deterministic omissions before validation. Empty or missing HTML, CSS or
   JavaScript preserves nonempty source. Omitted existing pages and areas preserve source.
   Structural HTML comparison parses attributes (so order, quotes and void-element spelling
   do not matter); existing conservative compaction ignores only non-rendering whitespace.
   Every preservation is recorded in the local job's validation notes.
2. Salvage independently. Reconstruct a failed page from original source and transplant one
   requested root at a time, validating every intermediate result. Failed areas become open
   conflicts with the validator's reason. Shared scripts stay unchanged. CSS is parsed with
   lightningcss; original rules stay intact and additions use the element-fix selector policy,
   accepting an implemented region root or IDs inside it, optional supported states and media
   queries. Unscoped additions are dropped and recorded. No unvalidated page can apply.
3. Repair once (`MAX_VALIDATION_REPAIR_ROUNDS = 1`). Freeze precise failures in CONTEXT.md,
   request corrections from the same provider/model and validate again through layers 1–2.
   Preserve already accepted areas while merging corrections. Usage includes both exchanges,
   including failed repair usage. Stop cancels both; provider/network failures do not start
   repair. The combined conservative estimates must fit the spending allowance. An earlier
   `needs` context exchange is archived before freezing the repair context. Unresolved areas stay open.

The job record keeps validation kinds, dispositions and readable reasons. The separate
`validation-counts.json` contains only kind/disposition keys and saturated counts: no page
content, IDs or network telemetry. Formats 3–16 keep their frozen requirements and behavior.
The AI conversation shows the correction exchange and validation notes; partial application
reports “Applied N of M areas” and keeps each unresolved area's reason visible.

## Retained unapplied output

Format 17 keeps rejected page output from both the initial answer and repair in
`GenerationJobView.drafts`, before recovery rewrites it. A page draft includes its
area IDs, exact HTML/CSS/JavaScript strings (or element replacement fragments) in
`code`, an optional reconstructed page, validator reasons, base source digest,
and applied/dismissed flags. Unparseable answers remain copyable as exact text.
Each provider round is bounded by the existing 8 MiB response envelope; optional
reconstruction is bounded to another 4 MiB per round. The existing 20-attempt
limit bounds retained history; job files have a 768 MiB ceiling. Reconstruction
beyond its bound is copy-only; retained code is never truncated. Retry and Dismiss
retain earlier code in the job file. Past generations exposes the same draft actions.

`generation_draft({jobId, draftId, action: 'inspect' | 'apply' | 'dismiss',
confirmedRevision?: number})` returns the current native eligibility, confirmation,
job and optional workspace. Applying requires the revision reviewed in the inline
confirmation, rechecks the current source, and commits through the durable Session
history as one Undo step. Draft IDs are consumed like generation IDs, including
through Undo and restart. Protected decision names appear in the confirmation.
A retained page replaces that whole page, including shared code; the confirmation
says so. Later page edits make that replacement stale; unchanged pages remain
eligible across unrelated revisions. Normal safe application updates draft base
digests to the resulting page. Stale drafts stay previewable and copyable.

Source syntax, parser identity round-trip, duplicate/invented editor IDs, invalid
area IDs and external scripts cannot be overridden. Changed JavaScript, embedded
scripts and handlers pass a conservative parsed capability gate for local DOM
behavior; dynamic code loading, reflection, computed access and unknown script
capabilities remain copy-only. This deliberately favors retaining code over
claiming arbitrary JavaScript is safe. Broaden the supported capability set only
with corresponding security cases. `create_preview` accepts optional `draftId`,
uses immutable retained source in the existing preview, and disables draft scripts.
Security-blocked source has Copy code and Dismiss only.
