# Identity import

New projects offer **New blank page** and **Import identity from project**, followed by Web/Mobile, resolution and name. Both create one empty light page. Page appearance can change afterward. Import preserves approved identity rather than copying the original site.

## Authority and storage

- `desegni-storage::collect_identity_source` reads a Desegnos archive, website folder or ZIP without executing code, extracting archives, locking source projects for writing or modifying source files. It excludes dependencies, build output, credential/configuration files and symbolic links. ZIP paths, expansion, file counts and sizes are checked. Credential screening is heuristic; users can inspect and deselect source files.
- The native identity service retains immutable snapshots. The editor receives file names, byte counts and safe image previews. IPC accepts snapshot/file IDs, not replacement source text or arbitrary model-selected paths.
- **Analyze identity** records durable intent before checking access or sending. It uses the configured API key or official ChatGPT-plan adapter, verified model and current account. The key never enters the editor, project, source snapshot or analysis receipt. Sign-out invalidates access and blocks late results even if a cancellation receipt cannot be saved.
- `desegni-ai` sends exact frozen `IDENTITY-SOURCES.md`, its SHA-256 and job ID, plus numbered supported logo images. Imported text and images are untrusted evidence. The analysis adapter cannot edit projects, execute source, fetch source URLs, install components or grant plugin permissions.
- Native validation checks the strict candidate, job/digest, source evidence, unique topics, image IDs and text limits. Findings are suggestions. Only checked findings and logos enter the new project; unchecked brand-name/tone fields and summary text do not become requirements.
- Core owns the typed imported identity, validated portable image bytes, accepted decisions and their generation dependencies. `PROJECT.md` records provenance/metadata; generation requirements include the actual selected data URIs. Current accepted choices and creative exceptions take precedence over the initial identity summary.

Identity analysis has one process owner, enforced by a held file lock. Another app instance can edit ordinary projects but cannot repair or replace that owner's analysis. Source snapshots and receipts live in private app-local identity storage, outside imported source folders. Damaged records remain preserved and produce a visible recovery error. Explicit replacement writes an atomic previous-version backup.

## Submission and recovery

The status moves from analyzing to complete, failed, uncertain or cancelled. Closing the wizard cancels its active analysis. Cancellation blocks late output; submitted work may still consume API or plan usage. A restart marks an interrupted request uncertain and never resends it. Analyze after a terminal status creates a new explicit request; duplicate submission while active is rejected. Completed analysis and its source manifest can be resumed for review.

OpenAI receives selected source text and supported logo images directly. Requests use `store: false`; OpenAI's data controls still apply. The ordinary site generator retains its own job, fuzzy/strict setting and requirements; identity analysis does not generate a website or inherit another project's generation job.

## Bounds and compatibility

Collection supports at most 128 usable files, 256 KiB per text file, 2 MiB total text, a 4 MiB source snapshot, 4096 traversed entries and 64 MiB ZIP/expanded content. Omissions are reported. Four reusable logo assets can be selected, up to 128 KiB each during collection. Persisted core identity supports four checked images up to 512 KiB each and raster dimensions from 1 to 16,384 pixels per side. Raster headers are validated without decoding codecs. SVG permits self-contained static geometry, styles and gradients; active/external content is rejected without rewriting original bytes.

Frozen analysis Markdown has a 512 KiB limit including framing. Oversized selections fail before submission without truncation. Vision accepts PNG/JPEG/WebP, at most four images, with 256 KiB per data URI and 512 KiB in total. SVG is text evidence; GIF and unattached raster images are explicitly excluded from visual inference. Conservative input/output estimates apply before connection.

Projects persist as schema 6. Legacy schemas 1–5 are verified before conversion, and explicit save retains the exact original archive as `.previous`. Ordinary projects retain requirements format 5 and compatible frozen formats 3–5. Imported-identity projects use requirements format 6. Project import accepts both `.desegnos` and legacy `.desegni` files. New saves default to `.desegnos`; existing `.desegni` files keep their path on Save. App storage, keyring, archive protocol and registration identifiers remain unchanged.

Source-format adapters can later extend collection without receiving provider, file-write or project-application authority. Core identity memory and approval remain core responsibilities.
