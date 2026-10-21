# Desegnos filenames

New desktop builds produce the actual `desegnos` executable (`desegnos.exe` on Windows). New projects use `<Location>/<Project Name>/<Project Name>.desegnos` alongside their live website files. Names are sanitized and numbered on collision. Save As defaults to the project name and creates its own folder for a new destination; existing standalone project paths remain supported.

Open and brand import accept `.desegnos` and legacy `.desegni` projects. Saving an already opened `.desegni` project keeps its selected path. Both extensions use the same validated archive format; its `desegni` protocol marker, canonical Markdown, schema and integrity checks are unchanged. Existing projects and benchmark fixtures are not renamed.

Crate/package names, the application identifier, storage directories, keyring services, OAuth registrations, account selection and tokens keep their existing identifiers. Filename changes require no credential or account migration.

## Build migration and rollback

The desktop build script removes a recognized old `desegnos → desegni` link **before Cargo runs**, so Cargo writes a regular `desegnos` executable rather than following the alias. A verified ownership receipt lets an interrupted migration resume safely. A failed build restores the previous product launch link when no new executable exists.

After a successful build, the old `desegni` path becomes a compatibility link to `desegnos`. Windows uses a verified compatibility copy. Before replacing a managed old executable, the script retains it under `desegni.before-desegnos-` followed by its fingerprint prefix, or the equivalent `.exe` name, and prints the exact backup path. Unrecognized files, links, receipts and changed binaries are preserved and reported instead of overwritten.

For rollback, close Desegnos and restore that preserved executable at the canonical `desegnos` launch path. On Windows, restore the compatibility copy too. Older executables may require the `.desegni` filename extension even though the archive protocol is unchanged, and may reject newly enlarged areas outside their former canvas range. The task's pre-change release backup is an additional rollback copy.

## Focused checks

```sh
node --test --test-isolation=none --test-reporter=spec tests/launch-paths.test.mjs
node scripts/cargo.mjs test --offline -p desegni-storage
node scripts/cargo.mjs clippy --offline -p desegni-storage --all-targets -- -D warnings
```

These verify migration policy and actual archive/import behavior without launching a desktop app. The integrated native workflow saves a `.desegnos` project; existing legacy import and migration checks remain in place. Native build/startup validation checks the real executable separately. Windows filesystem-copy policy tests on Linux do not replace native Windows verification.
