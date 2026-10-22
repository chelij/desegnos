# Earlier requests

Requirements format 19 replaces “Previous generation notes” with “Earlier requests for these areas (context only — the current requests are the authority)”. Formats 3–18 retain their rendering, hashes and serialized shape.

Each project's generation directory contains a text-only `request-history.json` index. Its first use reads that project's existing validated `job.json` packets; later job saves update it. Frozen request JSON provides the exact prompt and original bounds; candidate summaries and region outcomes provide the result and clarification question. Discarded jobs are marked rejected by user. The durable project dossier and earlier frozen dossier contexts supply known undo/redo outcomes. Historical undo state that was never retained in a dossier cannot be reconstructed. Missing AI summaries are labelled explicitly.

The index retains the newest five records per historical area. Generate and Save requirements match region IDs, whole-page requests, and overlapping or nearby rectangles (within 32 CSS pixels) on the same page. Each current target selects at most five earlier requests. The combined section is ordered newest first, capped at 3,000 Unicode characters including heading and data fences, and drops the oldest entries first. Prompts are capped at 300 characters, summaries at 200; JSON quoting preserves request text and treats it as data. Attachments, image bytes, source and candidate code are never copied into the index or section.

The rendered context and relative ages are frozen in the requirements snapshot and survive restart and retry. It is advisory context and grants no authority beyond the current requests. Legacy snapshots omit the new field.

Each index record retains the complete original target list for its job-wide summary. The section renders that summary only when the current request includes every original target; otherwise `summary` is null. Older index records without this scope information also render a null summary, while keeping their area-specific request and outcome. This scope information survives index compaction even when other areas' records are dropped.

Focused verification: `cargo test -q -p desegni-core request_history --lib`.
