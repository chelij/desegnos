# Desegnos {{VERSION}} — public preview

Release date: {{YYYY-MM-DD}}

## Changes

[Summarize user-visible changes since the previous release; link the relevant changelog section.]

## Downloads and checksums

[List the exact AppImage and .deb asset names and their architectures. List Windows/macOS assets separately as unsigned, untested previews.]

Checksums file: [{{CHECKSUMS_FILENAME}}](https://github.com/chelij/desegnos/releases/download/{{TAG}}/{{CHECKSUMS_FILENAME}}). Compare your downloaded package's checksum with its entry before installing.

Installation and first site: [guide at this release](https://github.com/chelij/desegnos/blob/{{TAG}}/docs/INSTALL_AND_FIRST_SITE.md).

## Exact source and license

Desegnos is licensed **GPL-3.0-only**; see [LICENSE at this release](https://github.com/chelij/desegnos/blob/{{TAG}}/LICENSE).

These binaries were built from this exact source:

- Repository: [chelij/desegnos](https://github.com/chelij/desegnos).
- Release tag: [{{TAG}}](https://github.com/chelij/desegnos/tree/{{TAG}}).
- Full commit hash: `{{FULL_COMMIT_HASH}}` ([commit](https://github.com/chelij/desegnos/commit/{{FULL_COMMIT_HASH}})).
- **Source code (tar.gz)**: [archive for {{TAG}}](https://github.com/chelij/desegnos/archive/refs/tags/{{TAG}}.tar.gz).
- **Source code (zip)**: [archive for {{TAG}}](https://github.com/chelij/desegnos/archive/refs/tags/{{TAG}}.zip).

GitHub automatically attaches these **Source code (tar.gz/zip)** downloads to the release. Both contain the source at the tag and commit named above.

[Before publishing, replace every placeholder, confirm the tag resolves to the full commit used for all binaries, and verify the source archive and checksums links.]

## Known preview limitations

- Linux is the tested platform and requires WebKitGTK 4.1, a desktop portal with a backend, and a Secret Service provider for secure credentials.
- Windows and macOS builds are unsigned, untested previews. Signing, notarization and the signed updater are deferred.
- Updates are checked manually through **Menu → Check for updates…**; installation is manual.
- This preview covers canvas requests, generation, Keep/Undo, visual edits, save/reopen and website-folder export. React export, hosted publishing and the remaining 1.0 contract are deferred.
- External services, forms and remote assets still require their actual services; export warnings describe portability limits.
- [List release-specific known bugs and remaining limitations. State the actual validation performed and any unverified behavior; do not imply all release-plan gates passed without evidence.]

## Reports

Use [GitHub Issues](https://github.com/chelij/desegnos/issues) for bugs and features. Crash reporting is opt-in: review the scrubbed report and submit the prefilled issue yourself; nothing is sent automatically. Report vulnerabilities through [private vulnerability reporting](https://github.com/chelij/desegnos/security/advisories/new), never a public issue. See [the security policy at this release](https://github.com/chelij/desegnos/blob/{{TAG}}/SECURITY.md).
