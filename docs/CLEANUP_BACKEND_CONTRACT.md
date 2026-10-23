# Cleanup backend interface for the frontend

Codex owns the implementation in the isolated `cleanup-codex` worktree. Frontend files stay with Claude. No production editor relaunch is authorized. The owner authorized one separate live generation through the saved ChatGPT plan; it completed with 3 web searches, 1 downloaded photograph and 0 placeholders. Source links were present; citation annotation handling is checked separately with synthetic streams.

## C9: source editing (X7)

- `edit_page_source({ projectId, pageId, revision, html?, css?, javascript? })` returns the ordinary **WorkspaceView**, with no extra envelope. Omit unchanged languages. Empty strings intentionally clear that source, including HTML; local clearing stores the exact empty string and remains undoable. AI-generated pages still require nonempty HTML. A byte-identical edit is a no-op; a successful change records one durable project Undo step.
- Rejection is a structured object `{ message, diagnostics: [{ language, line, column, endLine, endColumn, message }] }`. Languages are `html`, `css`, `javascript`; project/revision/storage failures use `project`. Lines and columns start at **1**; columns count UTF-16 units. Keep the user's draft on every rejection. Do not silently retry a stale revision over new project content.
- `get_page_source_map({ projectId, pageId })` returns `{ projectId, pageId, revision, sourceSha256, htmlSha256, ranges: [{ selector, startLine, endLine, startColumn, endColumn, implicit }] }`. Selectors use the same structural `body > tag:nth-of-type(n)` paths as canvas selections. `endColumn` is exclusive. Only browser-resolvable selectors are returned. An implicit wrapper has no authored opening tag; its range covers its source content.
- Map data is source-bound: discard it when `sourceSha256` no longer matches the current page. Prefer the narrowest range containing the code cursor. There may be several elements on the same line.
- Accept the returned workspace exactly as for local element editing. The changed source fingerprint makes the canvas render again and refresh derived element bounds. Native code does not invent browser coordinates. Pending AI requests are remapped only when the same item can be identified; edits that would lose a written request are refused.
- SWC parses HTML/JavaScript and Lightning CSS parses CSS without executing code. Editor identity attributes may be inserted; all other authored formatting and code stay exact in the project through save/reopen. Portable export preserves the content and behavior while normalizing HTML, extracting assets and removing editor metadata. Remote script imports and unsafe navigation are rejected; preview isolation remains unchanged.

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

The provider cleanup introduced **format 9**. The stable-selection update now creates **format 10**, described below. Formats 3–9 keep their original bytes and capability defaults. Format 9 captures the chosen provider's capabilities, requests found-content metadata and offers up to eight downloads within unchanged image/source byte budgets. Do not relabel an old packet or reconstruct it from current defaults.

## Frontend handoff

The combined editor consumes the provider list and found-content controls, and uses reviewed CodeMirror 6 for editable HTML/CSS/JavaScript. Design remains the default. Hidden editor attributes are projected out of the text while exact editor transactions retain their attachment to surviving tags, including text Undo/Redo and in-flight saves. Code drafts flush before Save, project/page/view changes, export, generation, project History and native close. Invalid or conflicting drafts block those transitions and offer explicit per-file discard. A failed source save never silently discards the draft. The stable-selection and safe-save interfaces below are wired through the same editor.

## Exact History positions

WorkspaceView.history is null without a project; otherwise it contains `{sessionId,start,position,end,version}`. Session IDs change on open/recover, including reopening the same project. Position is the native undo cursor; start advances only when the 200-step retained limit evicts an entry. Version changes only for a newly committed edit, so branching at the same depth is distinct from Redo. Background bounds/dossier refreshes do not create History steps. The frontend uses these coordinates instead of comparing content. History jumps hold the mutation queue and flush drafts first. These are session metadata and do not change project archives or requirements formats.

Explicitly reopening the currently held archive shares its owned writer lock while staging a separate save baseline and fresh session. Activation commits only after the journal and recovery pointer succeed. A failed reopen preserves the active session, its original baseline and its lock; another window cannot write during reopening.
## Safe save and recovery (2026-10-20)

The editor should use `save_project_checked({ saveAs: boolean })` rather than the older `save_project`. It opens the native Save dialog when requested or when no saved path exists. Both commands now refuse a changed archive; the older command reports a plain error and has no replacement bypass.

The checked command resolves to one of:

```ts
type FileStamp = {
  projectId: string;
  revision: number;
  manifestSha256: string;
};
type SaveProjectOutcome =
  | { status: 'saved'; workspace: WorkspaceView }
  | { status: 'cancelled' }
  | { status: 'conflict'; conflict: {
      token: string;
      kind: 'changed' | 'differentProject' | 'unreadable';
      path: string;
      fileName: string;
      message: string;
      defaultChoice: 'copy' | 'cancel';
      canReplace: boolean;
      details: { expected: FileStamp | null; actual: FileStamp | null };
    } };
```

Show `message` and the choices **Save as copy**, **Replace it** (only when `canReplace`), and **Cancel**. Keep stamp details behind **Show details**. A changed file defaults to copy. A different project defaults to Cancel; its message is `"<file> is a different project. Replace it?"` only after Save as. A different project encountered during ordinary Save, or any unreadable/non-project destination, is never replaceable. Invalid extensions and operational failures reject the command with a plain message.

Call `resolve_save_conflict({ token, choice: 'copy' | 'replace' | 'cancel' })`; it returns the same outcome. Copy opens another native Save dialog suggesting `<stem>-copy.desegnos`. Cancelling either dialog leaves the full recovery journal and saved path intact. A copy that selects another existing project goes through the same additional in-app confirmation.

Tokens are opaque, single-use, and bound in memory to the current project ID/revision, locked canonical destination, and exact observed manifest. Replace revalidates the disk stamp. If the file changed again, it returns a new conflict/token; render that new choice. If the project changed, Copy/Replace reject with a message to save again. Cancel still dismisses a matching token after a local edit. New save attempts and project switches invalidate old tokens. Never infer permission from an operating-system overwrite prompt or pass stamp details back as permission.

`RecoveryInfo` adds `savedPath: string | null` and `editedAtMs: number` (Unix epoch milliseconds). `editedAtMs` is the full journal snapshot's filesystem modification time, with zero for unavailable timestamps. Display the draft's age and last saved filename; the full path may be shown in details. These fields appear in both `WorkspaceView.recovery` and `WorkspaceView.recoveries`.

Storage records `FileStamp` from a fully validated archive's actual `manifest.json` bytes before migration, and after successful saves. It checks the recorded stamp before preparing a write, again before backup, and again immediately before atomic replacement. Explicit replacement preserves the exact observed archive in the existing `.previous` backup. New-file commits use an atomic no-replace operation (Linux rename, or a same-directory hard link on other platforms) so a destination that appears after the last check is never overwritten. The sidecar writer lock coordinates Desegnos windows; external programs do not necessarily honor it. Existing-file replacement has the filesystem's unavoidable check-to-rename interval against a non-cooperating writer; a manifest change at any preceding check is rejected, and no polling or silent merge is introduced. Recovery metadata verification never adopts a newly changed file as its baseline.
## Stable selections and multiple targets (2026-10-20)

New projects use schema **7** and new generation packets use requirements **10**. Existing schemas 1–6 load through the migration decoder: original archive integrity and PROJECT.md are checked first, then every authored HTML element receives a random, page-unique `data-desegnos-id`. Each resolvable legacy request is mapped to its actual element before positional indexes can change. Migration is in memory only, does not advance the user revision and becomes a saved upgrade only on Save. A missing legacy target remains unresolved; it is never reassigned to a different sibling. Formats **3–9 remain frozen** and are reconstructed using their original paths and capability rules. A saved result tied to pre-migration source becomes explicitly stale after IDs are assigned; keep its original packet/receipt intact and require a fresh user-initiated Generate. Migration never resubmits a provider request. Frozen format-8 and format-9 examples are regression fixtures; format 9 was captured from the unmodified core at `19be5b4`.

- `ElementTarget` now has `{id, tag, label, selector}`. `id` resolves first and survives text changes, reordering, local replacement, and tag changes. `selector` and labels are context; an ID that is missing never falls back to its old selector. A stable target has no `sourceSha256`. The old `sourceSha256` shape remains accepted only for actual pre-migration records/consumers, with a current page fingerprint check before positional resolution.
- `Region.elements` is an array of zero to eight targets; an empty array is an area request. New records use `elements`, leaving legacy `element` absent. Pending requests retain their original request ID, prompt and brief references. A removed selected ID produces **This element was removed.** instead of selecting a new sibling. A local deletion can leave a written request stale, so the frontend can offer Remove request / Pick again without losing the prompt.
- `Command::AddElementRegions` is the regular `edit_project` command `{type:"add_element_regions",page_id,elements,bounds:[{target,rect}],allow_target_removal?:false}`. IDs are resolved on that page, duplicate selections are collapsed, contained descendants are pruned and supplied rectangles are checked. The saved request rectangle is the union of the retained outer elements' actual supplied bounds. A sibling rectangle or a missing outer bound is rejected.
- `Command::RetargetRegion` is `{type:"retarget_region",page_id,region_id,elements,bounds:[{target,rect}],allow_target_removal?:false}`. It updates that request's selection and rectangle atomically while preserving its prompt and ID.
- `Command::DeleteElements` is `{type:"delete_elements",page_id,elements}`. It validates and prunes the selection first, then deletes the selected subtrees as one durable, undoable project edit. A failed durable write changes nothing.
- `Command::SetRegionTargetRemoval` is `{type:"set_region_target_removal",page_id,region_id,allow_target_removal}`. `Region.allowTargetRemoval` defaults false; set it only through an explicit user choice. The provider cannot grant this permission. Requests must keep every selected root unless that request has explicit removal permission.
- Existing `add_element_region` and `refresh_element_bounds` commands remain accepted for existing single-selection consumers. New UI code should send the array commands.
- `refresh_element_target_bounds({projectId,pageId,viewport,bounds:[{regionId,bounds:[{target,rect}]}]})` returns **WorkspaceView**. Each request's rectangle becomes the checked union. A no-op writes nothing; changed geometry is durable without adding an Undo entry, matching existing bounds refresh behavior. Invisible but still present IDs remain valid targets.
- `get_page_source_map` ranges additionally contain `id` (empty for implicit wrappers). The map remains source-bound and uses current UTF-16 source positions. Source edits add IDs by inserting only those attributes, preserving all other authored bytes. The frontend must retain hidden IDs when editing source; deliberately deleting one makes its written request stale.

Format-10 targeted requests are checked by removing every selected-ID subtree from before/after documents and comparing the remaining canonical document (including head and html attributes). Missing selected roots, duplicate/forged editor IDs, new content outside selected subtrees and moving existing IDs into another target's scope are rejected. A generation assigns IDs to new nodes only after validation succeeds. Existing shared CSS and JavaScript must remain byte-identical for target edits. Embedded style/script/link nodes must remain canonically unchanged, and event handlers may not be introduced or changed; their effects can extend outside the target. Local styling belongs in selected elements’ inline style attributes. Mixed targeted and new-area requests must identify new-area roots and preserve the rest. Area-only requests on an existing page retain the previous explicit page-source editing behavior because a rectangle has no trustworthy DOM mapping. Structural scope checks do not prove arbitrary custom-code runtime behavior or measured visual geometry.

Local replacement retains the original root identity. Pasting a layer discards copied editor IDs and assigns fresh IDs to the inserted subtree, retaining ordinary authored IDs/reference rewriting. Portable export removes every `data-desegnos-id` without removing authored IDs or public source credits. Do not use editor identity attributes as website styling or behavior hooks.

## Phone fixes and website brand import (2026-10-20)

- `edit_project` command `{ type: 'set_region_mobile_only', page_id, region_id, mobile_only }` marks a request as a phone-layout fix (`Region.mobileOnly`). Format-10 requirements add `"mobileOnly": true` to that area plus phone instructions. The scope check accepts only CSS changes inside `@media` rules whose width limits are ≤ 820 px (no print or orientation queries), with HTML and JavaScript unchanged. A page whose requests mix phone fixes with other requests is refused by `prepare_requirements`.
- `fetch_identity_source({ url })` returns the same `IdentitySourceManifest` as `pick_identity_source`. It accepts `example.com` or a full `https://` address, and errors are plain sentences ("That website couldn’t be read: it …"). Debug native-test builds read `DESEGNI_NATIVE_TEST_WEBSITE_ORIGIN` for a local fixture site.

## Projects, context and attachments (2026-10-20)

- **Several open projects.** `create_project`, `open_project`, `open_recent_project` and `recover_project` now open alongside the active project; the active one is parked with its undo history, file lock and recovery journal (the `discard` argument is accepted and ignored). `switch_project({ projectId })` makes another open project active (blocked while generating). `close_project({ projectId?, discard })` closes that open project tab, defaulting to the active one (unsaved changes need Save or `discard: true`, which keeps the journal on disk but stops offering it). Closing the active project activates the next tab in tab order, or returns to the start screen when none remain. `WorkspaceView` adds `openProjects: [{ id, name, dirty, active, pages: [{ id, name }] }]` in tab order and `anyDirty`. Opening a file that is already open just switches to it. Parked projects hold their locks, so they never appear as recoverable.
- **Import context.** `import_context({ revision, identityAnalysisId, identityFindingIndices, identityAssetIds })` merges a completed, reviewed analysis into the open project as one Undo step: findings become project rules (protected rules are never overwritten; same topic is updated), brand name and tone fill the brief, logos join the project images (max four). New projects no longer start from an identity in the UI; `create_project` still accepts it.
- **Request attachments.** `edit_project` commands `add_region_attachment { page_id, region_id, attachment }` and `remove_region_attachment { page_id, region_id, attachment_id }`. Attachments are `{ id, name, kind: 'image' | 'text' | 'link', dataUri?, text?, url? }` (no count or size limit; PNG/JPEG/WebP/GIF checked by signature; non-empty text; http(s) links). The core assigns ids. Format-10 requirements list attachments by id/name with text and links and tell the AI to place pictures with `<img data-desegnos-attachment="ID" src="#">`; `apply_generation` inserts the stored picture and removes the marker. Format 13 also sends a small preview of each picture and keeps only a copy scaled to its displayed width (see `GENERATION_CONTRACT.md`).
