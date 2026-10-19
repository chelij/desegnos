# Cleanup backend interface for the frontend

Codex owns the implementation in the isolated `cleanup-codex` worktree. Frontend files stay with Claude. No production editor relaunch is authorized. The owner authorized one separate live generation through the saved ChatGPT plan; it completed with 3 web searches, 1 downloaded photograph and 0 placeholders. Source links were present; citation annotation handling is checked separately with synthetic streams.

## C9: source editing (X7)

- `edit_page_source({ projectId, pageId, revision, html?, css?, javascript? })` returns the ordinary **WorkspaceView**, with no extra envelope. Omit unchanged languages. Empty strings intentionally clear that source. A byte-identical edit is a no-op; a successful change records one durable project Undo step.
- Rejection is a structured object `{ message, diagnostics: [{ language, line, column, endLine, endColumn, message }] }`. Languages are `html`, `css`, `javascript`; project/revision/storage failures use `project`. Lines and columns start at **1**; columns count UTF-16 units. Keep the user's draft on every rejection. Do not silently retry a stale revision over new project content.
- `get_page_source_map({ projectId, pageId })` returns `{ projectId, pageId, revision, sourceSha256, htmlSha256, ranges: [{ selector, startLine, endLine, startColumn, endColumn, implicit }] }`. Selectors use the same structural `body > tag:nth-of-type(n)` paths as canvas selections. `endColumn` is exclusive. Only browser-resolvable selectors are returned. An implicit wrapper has no authored opening tag; its range covers its source content.
- Map data is source-bound: discard it when `sourceSha256` no longer matches the current page. Prefer the narrowest range containing the code cursor. There may be several elements on the same line.
- Accept the returned workspace exactly as for local element editing. The changed source fingerprint makes the canvas render again and refresh derived element bounds. Native code does not invent browser coordinates. Pending AI requests are remapped only when the same item can be identified; edits that would lose a written request are refused.
- SWC parses HTML/JavaScript and Lightning CSS parses CSS without executing code. Authored source stays byte-exact in the project through save/reopen. Portable export preserves the content and behavior while normalizing HTML, extracting assets and removing editor metadata. Remote script imports and unsafe navigation are rejected; preview isolation remains unchanged.

## C5: found content (X2)

`data-desegnos-found="https://public-source.example/page"` is editor metadata on the actual image/text element. The source URL is validated, retained in the saved project and removed from exported HTML. Ordinary visible attribution/source links remain in the website. Do not use the metadata attribute to drive site styling or behavior.

## C4: provider setup (X4)

`get_ai_settings` now additionally returns `provider` (`openai`, `anthropic`, `google`) and `providers`: `[{ id, name, available, signInSupported, capabilities: { webSearch, imageDownloads } }]`. OpenRouter and Local remain present with `available: false`; OpenAI alone has `signInSupported: true`. Render this native list rather than hard-coding availability.

- `set_ai_provider({ provider })` returns the complete updated AI settings view. Unsupported choices are refused without changing the saved choice. A running generation blocks switching. Each provider retains separate settings and credentials. OpenAI's existing keyring and sign-in records stay compatible.
- Existing `set_ai_key({ key, storage })`, `remove_ai_key`, `save_ai_settings({ settings })`, `test_ai_access` and generation commands operate on the selected provider. Credentials remain entirely native. Do not use the ChatGPT sign-in commands for Anthropic/Google.
- OpenAI uses the existing API-key/ChatGPT authentication choice. Select `openai` before calling `set_ai_authentication({ authentication: "chatgpt" })`. Anthropic/Google use `apiKey` and expose their actual model catalogues and supported effort choices. Unimplemented connections never send traffic.
- Jobs now have `provider`, defaulting to OpenAI only when reading old records. Retry uses the frozen job's provider, model, authentication and capabilities, not the current setup. Display source/service labels from that field in Details. Before enabling new choices in C4, use service-specific data-sharing text: only the OpenAI transport sends `store: false`; do not reuse OpenAI’s privacy claims or links for other services.
- Anthropic: Messages SSE, native server search capped at five uses, a strict `submit_result` tool for the final JSON, separate citation/thinking blocks, cancellation and reported usage including cached tokens. The tool is a result envelope; Desegnos never executes provider-supplied client tools.
- Google: Generate Content SSE with JSON schema and Google Search, separate grounding/thought parts, cancellation and usage including thought tokens. Key goes in `x-goog-api-key`, never a query parameter. This documented API remains supported although Google calls it the legacy API.
- Both clients have fixed HTTPS origins, no redirects, proxies or automatic retries, a five-minute deadline and bounded request/stream/output sizes. Image downloading stays in the separate credential-free image adapter. Estimates cover model tokens; search charges and final billing can differ. Google's catalogue records the current promotional rates; spending estimates use the announced higher post-promotion rates as a conservative ceiling.

## Additional X2/C5 details

`open_found_source({ projectId, pageId, target })` opens the validated source of an exact current-source `ElementTarget`. The frontend cannot supply an arbitrary destination to this command. Local text changes and image replacement clear stale origin metadata; moving/restyling keeps it. Undo restores the original metadata with the content.

## Requirements compatibility

New generation packets use **format 9**. Format 8 was already released before this cleanup, so its bytes and capability defaults remain frozen along with formats 3–7. Format 9 captures the chosen provider's capabilities, requests found-content metadata and offers up to eight downloads within unchanged image/source byte budgets. Do not relabel an old packet or reconstruct it from current defaults.

## Frontend handoff

The native interfaces are implemented and have focused core/storage/native checks. Claude still owns C5 badges, C9 editable CodeMirror/source-map wiring and C4's provider-list consumption. The current frontend snapshot keeps these pending controls disabled/read-only; backend availability is not a claim that those UI items are finished.
