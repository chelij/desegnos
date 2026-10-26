# Desegnos filenames and formats

Desktop builds produce `desegnos` (`desegnos.exe` on Windows). New projects use `<Location>/<Project Name>/<Project Name>.desegnos` alongside their live website files. Names are sanitized and numbered on collision. Save As defaults to the project name and creates its own folder for a new destination. Standalone `.desegnos` archives save in place.

The application identifier is `io.github.chelij.desegnos`; crates use `desegnos-*`, previews use `desegnos-preview`, and environment variables use `DESEGNOS_*`. The owner manages the location of their existing app data manually. Startup selects `DESEGNOS_DATA_DIR`, the saved data-location choice, or the platform default, in that order.

Only current formats are supported: project schema 10, generation requirements format 27, and each settings/identity/job format's current version. A project or settings file of another version is an error; stored generation jobs and request history of another requirements format are ignored, stay untouched on disk and never fail a command. Archives use the `desegnos` marker and canonical Desegnos project notes. A journal requires the current `sha256`, `project`, and `files` shape, with imported site files in content-addressed blobs.

Saving preserves the previous archive as `.previous`. Integrity checks, file ownership and writer locks, outside-change detection, atomic writes, bounded inputs, and current-format crash recovery remain enabled. Recovery never resubmits a paid request. Existing OAuth client registrations are not renamed or replaced.
