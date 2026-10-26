# Desegnos filenames

New desktop builds produce the actual `desegnos` executable (`desegnos.exe` on Windows). New projects use `<Location>/<Project Name>/<Project Name>.desegnos` alongside their live website files. Names are sanitized and numbered on collision. Save As defaults to the project name and creates its own folder for a new destination; existing standalone project paths remain supported.

Open and brand import accept `.desegnos` and legacy `.desegni` projects. Saving an already opened `.desegni` project keeps its selected path. Both extensions use the same validated archive structure. Readers accept `desegni` and `desegnos` markers and canonical project-note branding; new saves write `desegnos` and Desegnos. Schema and integrity checks remain enforced. Existing projects and benchmark fixtures are not renamed.

Crates and packages use `desegnos-*`, the application identifier is `io.github.chelij.desegnos`, and previews use `desegnos-preview`. Environment variables use `DESEGNOS_*` exclusively. OAuth account, host and client IDs and grants stay unchanged.

## Startup migration

Before probing the data folder, startup reads the current or legacy `data-location.json` choice. `DESEGNOS_DATA_DIR` and saved custom locations take precedence; their contents are not moved. Missing default data/config/cache folders move from `io.github.chelij.desegni` to `io.github.chelij.desegnos`. Existing destinations are never merged or replaced. On platforms sharing config and data, a data override also prevents migration of that shared folder.

A same-filesystem rename preserves the contents directly. Across devices, startup copies files, directories, permissions and symbolic links into a temporary sibling, then publishes the complete directory with a rename. It retains the old folder with `MOVED.txt` pointing to the destination. Copy errors leave the old contents and remove the partial staging copy. A process killed during copying can leave an unpublished `.desegnos-migration-*` sibling; the next startup retries from the old folder. A failed directory migration stops startup before a fresh empty data folder can hide it.

API-key and ChatGPT token bundles copy from each owned legacy service to the current service, verify exact read-back, then delete the old entry. Account UUIDs select ChatGPT entries from persisted registrations. An unavailable vault, conflicting destination, failed write/read-back or failed removal retains the old entry and logs once per entry per process; later startup retries. Tokens and OAuth client IDs are not rewritten. The editor theme key also migrates on first read.

Older binaries cannot read newly written archive markers. Retain `.previous` archives for project rollback; moving an app directory back is only safe after closing all app versions and preserving any newer data. A cross-device source copy is a snapshot, not a synchronized backup.

## Build migration and rollback

The desktop build script removes a recognized old `desegnos → desegni` link **before Cargo runs**, so Cargo writes a regular `desegnos` executable rather than following the alias. A verified ownership receipt lets an interrupted migration resume safely. A failed build restores the previous product launch link when no new executable exists.

After a successful build, the old `desegni` path becomes a compatibility link to `desegnos`. Windows uses a verified compatibility copy. Before replacing a managed old executable, the script retains it under `desegni.before-desegnos-` followed by its fingerprint prefix, or the equivalent `.exe` name, and prints the exact backup path. Unrecognized files, links, receipts and changed binaries are preserved and reported instead of overwritten.

For rollback, close Desegnos and restore that preserved executable at the canonical `desegnos` launch path. On Windows, restore the compatibility copy too. Older executables may require the `.desegni` filename extension and reject the new archive marker or newly enlarged areas outside their former canvas range.

## Focused checks

```sh
node --test --test-isolation=none --test-reporter=spec tests/launch-paths.test.mjs
node scripts/cargo.mjs test --offline -p desegnos-storage
node scripts/cargo.mjs clippy --offline -p desegnos-storage --all-targets -- -D warnings
```

These verify migration policy and actual archive/import behavior without launching a desktop app. The integrated native workflow saves a `.desegnos` project; existing legacy import and migration checks remain in place. Native build/startup validation checks the real executable separately. Windows filesystem-copy policy tests on Linux do not replace native Windows verification.
