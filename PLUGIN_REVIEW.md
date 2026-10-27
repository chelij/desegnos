# Desegnos plugin boundaries

Reviewed 2026-10-27 against [RELEASE_PLAN.md](RELEASE_PLAN.md) and [FEATURE_RESEARCH.md](FEATURE_RESEARCH.md).

Desegnos has no installable plugin host today. First-party modules compiled with the app are not plugins. Do not advertise internal modules as an installable plugin system until a host with packaging, permissions, migrations and recovery exists.

## Core authority

These stay in the core and are not replaceable by a plugin:

- The project model: boundaries, elements, layers, reference, pointers, decisions and brief.
- Validation at trust boundaries: project format, element/style/layout bounds, pointer scope, export plans.
- History and saving: revisions, undo steps, versions, alternatives, SQLite storage and recovery.
- AI requirements assembly, frozen jobs, cancellation semantics and the result-application seam.
- Credential isolation and native file/destination authority.
- Import safety: public-address-only fetching, bounded snapshots, script-disabled rendering and the picture-size cap.

## Replaceable boundaries

A plugin or adapter may implement these behind a narrow interface:

- **AI providers.** An adapter declares capabilities (streaming, vision, image generation, web search) and implements request, cancel and usage reporting. Hosted generation will be one more adapter.
- **Export formats.** Website/app export is one bundled planner and writer; slides and image output are direct renderers. A new format is a new adapter with its own validation and destination rules.
- **Image services.** Image generation and optimisation can be replaced without changing the project model.
- **Import readers.** Folder, ZIP and website snapshots can gain new source readers as long as they return the same bounded file/page shape.
- **Optional authoring workflows.** Content collections, localisation, publishing, search, review, redirects and animation are candidates for separate first-party modules.
- **Editor extensions.** A future host could add element kinds or panels, but it must not gain credential, filesystem or command authority by default.

## Interface rules

- A plugin receives only the data its operation needs; never secrets, the whole project database or arbitrary filesystem paths.
- Writes go through the same validation and one undo step as a hand edit.
- A plugin cannot send a paid AI request without the same explicit paid-send decision.
- Failures are plain sentences and leave the project unchanged.
- Removing a plugin leaves projects readable and exports reproducible from the remaining core.

## Deliberate limits

A general plugin marketplace is out of scope for the preview. Prove the interface with complete first-party extensions before opening it to third parties.
