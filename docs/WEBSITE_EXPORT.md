# Website export

Export website writes the current accepted project revision to a new folder selected through the native destination dialog. It does not send a request to an AI provider. Opening `index.html` works without Desegnos. The entire folder can also be uploaded to a static website host.

## What is written

- Every included page: `index.html` for Home and `<page-path>/index.html` for nested pages. Included blank pages remain blank and produce a warning; an entirely empty site cannot be exported.
- Authored HTML, head metadata, body attributes, inline code and event handlers. Missing title/viewport/UTF-8 metadata receive real page defaults; character encoding matches the emitted UTF-8 files.
- Actual CSS. Identical page styles share `styles.css`. Pages with different CSS link to separate hashed style files under `assets/`, so another page's rules cannot change their cascade. Authored inline styles stay in their documents.
- Actual page JavaScript in hashed classic script files under `assets/`. No authored code is evaluated by Rust, a build tool, a plugin or a provider during export.
- Embedded PNG, JPEG, WebP, GIF, SVG, AVIF and icon data written as content-addressed files. HTML image URLs, responsive `srcset` candidates, inline CSS, stylesheet `url()` values and CSS image sets point to the actual output. Other embedded resource data, such as font data, stays embedded.
- A `README.txt` with the exported revision and the actual review warnings.

Internal anchor links to included routes become relative links to their actual HTML files, preserving query strings and fragments. Root, nested, encoded and relative page URLs use the same included route map. Embedded page frames can also resolve included routes. Navigation from arbitrary JavaScript is preserved and requires manual review.

Project notes, requirements files, prompt annotations, editor controls, undo history, credentials and unused identity reference files are excluded. An identity image is exported only if the authored site actually references its image bytes.

## Safe destination ownership

The renderer provides only a project ID and expected revision. Native code reads the accepted project, opens the folder picker and rejects an outdated revision. Compilation and writing run outside the UI thread. One export runs at a time.

Storage always creates a newly named `<project>-website-<UUID>` folder. It never replaces an existing export. The compiler rejects case-insensitive file/directory collisions; storage independently rejects unsafe paths. Symbolic-link destination folders are rejected.

On Linux and macOS, files are staged in a private directory and written through held directory descriptors without following symbolic links. Closed files and directories are synced before an atomic, no-replacement rename publishes the folder. A failure before publication removes only the stage entries that this operation created. A durability failure after publication preserves the complete output and reports its actual path. Windows uses exclusive file creation and a directory rename that refuses an existing destination; Windows acceptance remains part of the native platform release gate.

Open folder accepts only the exact native receipt for the most recent completed export. Native code rechecks the folder identity, paths and file hashes before asking the operating system to open it. It cannot open an arbitrary path supplied by page content or renderer code.

## Review warnings and current bounds

External resources are preserved rather than downloaded. Missing local resources and links to excluded pages are reported. JavaScript, module scripts, forms, embedded HTML, authored content-security policies and meta refresh behavior receive specific manual-review warnings where present. No external form, network endpoint or custom script is run merely by exporting. A `<base href>` blocks portable export with a concrete explanation because it changes every relative destination.

The current accepted-source export is bounded to the existing 100-page / 64 MiB source model, at most 10,000 files and 128 MiB of output. It does not advertise the complete future publication/site-check or React export contracts in `RELEASE_PLAN.md`. In particular, an export notice does not certify accessibility, external services, search metadata, arbitrary module compatibility or server routing. Direct-file and hosted browser behavior must be checked separately before sharing a site that relies on those features.

## Verification

Compiler tests cover actual multi-page route rewriting, query strings/fragments, encoded paths, authored head/body preservation, private-material exclusion, exact script sources, responsive and CSS image bytes, escaped CSS tokens, CSS cascade isolation, excluded and blank pages, malformed resources, deterministic output and output-path collisions.

Storage tests cover repeated exports preserving old data, exact output bytes, traversal/case-collision rejection, real partial-write cleanup, no-replacement publication over empty/populated/symlink destinations, symlink parents/resources, and modified output receipts.

A direct-file WebKit check on an isolated virtual display opened a real two-page exported fixture, decoded its PNG, loaded CSS background images, executed the exported page scripts, navigated to a nested page with query and fragment, verified independent page colors, and returned to Home without loading a network resource. Other browser engines and native operating systems remain separate release checks.
