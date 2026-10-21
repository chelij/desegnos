# Identity import

Import context uses three steps: **Where is your brand?**, **Read it**, and
**Choose what to keep**. A summary replaces the default file list. **Choose files
yourself** holds the file choices, **Pick for me**, grouped omissions and a nested
full skipped list. **Read my brand** stays disabled until the selected files fit;
**Add to project** stays disabled until there are results to keep. AI settings are
offered only when access or spending settings need attention.

Folder and ZIP collection reads Astro, Vue, Svelte and the other supported text
templates. Global styles/theme settings, layouts, home/about pages, header/footer
components, brand notes and logos precede other pages in deterministic order.
Licences, lockfiles, dependencies, vendor/minified files and unrelated asset app/game
code are excluded. Build folders are used only if no original sources are found.
Automatic picks use the configured provider's complete request check, including
framing, output allowance, image sizes and spending limit. Files are not truncated.
Photo reading samples at most eight thumbnails within 96 KiB; all collected photos
remain available to keep. Unread photos keep their filenames until described later.

Current import review lets the user choose findings, logos, photos and existing work.
Photos and work start selected. `import_context` and `create_project` accept optional
`identityPhotoIds` and `identityItemIndices`; empty lists exclude those categories,
while omitted fields preserve existing callers' all-selected behavior. Every choice
is validated against the completed analysis. Any single selected category can enable
Add to project. PNG, JPEG and WebP photos are decoded within the existing size limits
and retained as bounded JPEGs. Folder traversal sorts children before applying limits.

Website address import follows same-origin page links with at most 20 page attempts,
256 KiB per page, a 2 MiB HTML budget and a bounded queue. It prioritizes work,
project, tutorial and blog links, deduplicates fragments and excludes query links,
non-page files and common account/action paths. Page redirects must remain on the
source host, and every hop retains public-address/DNS/MIME/timeout checks. The root's
final URL supplies the base for relative links. Each retained page carries its public
source URL so analysis can report actual destinations and summaries. Source pages
are read without executing scripts or sending account credentials.

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

Collection supports at most 128 usable files, 256 KiB per text file, 2 MiB total text, a 4 MiB source snapshot, 4096 traversed entries and 2 GiB ZIP/expanded content. Every photo is kept (no count limit); each is stored at most 1280 px / 200 KiB, with a 256 px thumbnail available for the bounded reading sample. Omissions are reported. Four reusable logo assets can be selected, up to 128 KiB each during collection. Persisted core identity supports four checked images up to 512 KiB each and raster dimensions from 1 to 16,384 pixels per side. Raster headers are validated without decoding codecs. SVG permits self-contained static geometry, styles and gradients; active/external content is rejected without rewriting original bytes.

Frozen analysis Markdown has a 512 KiB limit including framing. Oversized selections fail before submission without truncation. Vision accepts PNG/JPEG/WebP, at most four images, with 256 KiB per data URI and 512 KiB in total. SVG is text evidence; GIF and unattached raster images are explicitly excluded from visual inference. Conservative input/output estimates apply before connection.

Projects persist as schema 6. Legacy schemas 1–5 are verified before conversion, and explicit save retains the exact original archive as `.previous`. Ordinary projects retain requirements format 5 and compatible frozen formats 3–5. Imported-identity projects use requirements format 6. Project import accepts both `.desegnos` and legacy `.desegni` files. New saves default to `.desegnos`; existing `.desegni` files keep their path on Save. App storage, keyring, archive protocol and registration identifiers remain unchanged.

Source-format adapters can later extend collection without receiving provider, file-write or project-application authority. Core identity memory and approval remain core responsibilities.

Page review groups photos and existing work by website route. Optional `sourcePages` references record the originating page path, title and public URL; folder imports resolve literal HTML, Astro and Markdown image references, then fall back to the containing folder. Optional source `pages` includes kind guesses and photo/work counts. Shared images retain all referring pages. URL import reads up to 20 pages and downloads up to 32 referenced photographs within an 8 MiB download budget using the existing public-address checks.

`identityPagePaths` selects the routes to create. Empty selections import only the chosen project-wide brand context. Existing-project import can attach to matching paths (adding missing routes), or add separate pages with numbered paths on collision. Home always uses the project's home page. Selection, page creation, brand findings and content attachments form one undoable import. Repeating an attachment merges the same route and work items. New-project import starts with Home and creates only the selected additional routes.

Optional `ImportedIdentity.pages` maps source pages to actual project page IDs, shared photo IDs and work items. Page assignments select requirements format 16, which nests pictures and work under the affected page's current name/path; only colours, fonts, logos, tone and any older unassigned content remain project-wide. Formats 3–15 keep their previous rendering and hashing. Existing projects and saved analysis receipts omit these fields by default and remain readable; reviewing a legacy flat receipt as Home upgrades the resulting context without rewriting that receipt.
