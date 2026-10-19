# Desegnos plugin boundaries

Reviewed 18 October 2026 against the [full release plan](RELEASE_PLAN.md) and [eight further feature candidates](FEATURE_RESEARCH.md).

Publishing, visitor search, translation workflows, bulk find/replace, client review, redirects, and animation are good plugin candidates. Content collections and localization also fit optional products, but require coordinated project/rendering contracts. Several promised release features can have replaceable implementations, including provider adapters, image generation/optimization, and format-specific exporters. Project memory, requirements assembly, identity rules, safe editing, persistence, and validation belong in the core.

These are proposed boundaries. Every feature already promised for 1.0 remains required. Packaging a feature as a complete bundled extension can preserve that promise; postponing it requires a separate scope decision. The new AI/memory requirements are included in the updated release plan.

## 1. Recommended release structure

Use a dependable editor core with cohesive first-party feature modules. Give real variation points narrow interfaces: the two required exporters, provider operations, and qualified component entries. Keep specialized services and optional authoring workflows outside the domain model's authority.

First-party modules compiled and delivered with the application are different from independently installable end-user plugins. A public plugin host needs package installation, compatibility, isolation, permissions, migrations, updates, and recovery. The current release does not promise that host or a marketplace. Do not advertise folders of internal modules as an installable plugin system.

When an installable host is selected, retain the same ownership boundaries. Prove its interface with actual complete extensions before expanding it into a general framework. Optional plugins should reveal controls when useful and preserve the initial empty canvas and area-selector workflow.

## 2. Existing release features

| Feature | Recommended home | Boundary |
| --- | --- | --- |
| New document, free workspace and screen guide, inline prompts, grid/snapping, percentage rulers, page tabs, nested menus, one Fuzzy/Strict setting | Core | These define the sidebar-free authoring flow and placement-intent contract. A plugin must not be necessary to begin a project or work outside its screen guide. |
| AI request coordination and code generation | Core workflow; provider adapter can be replaceable | Core assembles requirements, owns job scope/retries/costs, validates outputs, and applies changes. OpenAI remains complete in 1.0. Additional providers can be plugins. |
| API-key and ChatGPT-plan authentication | Native credential/account broker and OpenAI provider module | Tokens, OAuth validation, refresh/revocation and storage repair stay native. The job coordinator freezes account and billing method. Plugins cannot read credentials, redirect requests, invent entitlement or trigger billing fallback. |
| PROJECT.md, automatic generation dossier and REQUIREMENTS.md | Core | Desegnos maintains accepted decisions and distinguishable draft/applied/undone generation records, freezes relevant context on Generate, and sends that requirements file to the agent. Plugins can propose scoped contributions through the core, without bypassing durability, protection or validation. |
| Identity decisions and creative feedback | Core baseline; specialist critique can be a plugin | Accepted brand facts/tokens/protections and deliberate exceptions must remain available across providers and projects. Optional specialist advice cannot become the source of truth. |
| React component discovery/reuse | Core resolver and dependency policy; additional catalogues can be plugins/packs | AI recognition triggers verified lookup. Core checks available code, props, versions, licenses, and both exports. Catalogue membership cannot be invented by a model. |
| Pages, routes, anchors, menus | Core | Stable links and shared page identity affect generation, editing, checks, and every export. Publishing-specific routing rules belong in provider plugins. |
| Visual/text/layer editing and responsive layout | Core | All authoring tools must agree on selection, transactions, layout, breakpoints, and state precedence. Specialized commands can use these operations. |
| Reusable components, shared tokens, saved sections | Core primitives; optional content packs | Definitions, instances, overrides, references, and copying dependencies must work without a particular pack. Packs supply real reusable content. |
| Hover/focus/disabled/loading/success/error states | Core | These are shared authoring and runtime semantics. An animation timeline or specialized behavior editor can be optional. |
| Form fields, validation, GET/POST/fetch bindings | Core baseline; service connectors can be plugins | Real configured submission and response states remain required. A connector can configure a specific service; it cannot fabricate a backend or success. |
| Real custom JavaScript and React | Core compilation and restricted execution | A component pack can supply browser code. The application must control imports, permissions, compilation, recovery, and export parity. |
| Image import, asset references, storage, originals | Core | Projects must retain actual bytes and licenses independently of a provider or plugin. |
| OpenAI image generation and editing | Bundled extension candidate | Optional use does not mean optional implementation in 1.0. Use the core AI/credential/cost broker and store returned bytes as ordinary project assets. |
| Image optimization | Bundled extension candidate with core asset integration | The processor returns real derivatives and metadata. Core owns hashes, references, originals, history, and export selection. Required JPEG/PNG/WebP behavior still ships. |
| Fonts, audio/video, captions | Core baseline; specialist import/processing can be plugins | Existing saved media must remain playable/exportable after an optional processor disappears. Font selection and asset dependencies affect layout checks. |
| Page titles, language, canonical/indexing/social metadata, favicon, sitemap/robots | Core baseline; advanced advice can be a plugin | These describe every exported site. Additional structured-data tools or specialist audits can contribute validated settings/findings. |
| Durable history, autosave, undo, recovery, schema migrations | Core | Plugins use the transaction/checkpoint system. They cannot own the only recoverable copy of project state. |
| Minimum site check and export blocking rules | Core; extra diagnostic rules can be plugins | Required dependency, configuration, rendering, and security checks cannot disappear when a plugin is disabled. Extra opinions remain identifiable advice. |
| Portable HTML/CSS/JavaScript export | Core required exporter | A website can leave the application with real content, interactions, and assets. Never require a hosted plugin service for baseline export. |
| React export | Bundled exporter candidate | A clean format-specific implementation behind the shared compiler is reasonable. It remains installed, supported, and fully tested in 1.0 under the existing promise. |
| Read-only source views | Core | They reflect the accepted project/compiler output. Optional format tooling cannot create an undocumented source round-trip. |
| BYOK secrets, permissions, preview security, native installation/update | Core | These protect all modules and must remain under the application publisher's control. Plugin permissions cannot override them. |

React component catalogues are the clearest extension opportunity arising from the new input workflow. The initial qualified Radix entries remain part of the promised release. Later packs can add a specific supported component family without replacing identity memory or the agent coordinator.

## 3. All eight further candidates

| Candidate | Plugin suitability | Complete boundary and needed foundation |
| --- | --- | --- |
| Local content collections | Hybrid: optional authoring plugin plus coordinated data/compiler support | Plugin owns collection/schema/import UI and content operations. Core must validate dependency graphs, generated routes, transactions, assets, and the actual render/export representation. A private JSON blob alone does not implement bindings, filtering, pagination, or item pages. |
| Linked multilingual sites | Hybrid: locale-management/translation plugin | Existing language/direction remains core. Plugin manages linked translations, locale content, status and BYOK translation review. Core must persist validated relationships and accept correct routes/metadata/export contributions. All localized pages must be real output. |
| Visitor site search | Strong plugin candidate | Plugin builds the index from the current public included content and supplies accessible search UI/browser runtime. Core provides the frozen site view and validates exclusions/assets. Bundle the real index and runtime into both exports; no app plugin or hosted search account is required by visitors. |
| Publishing and staging | Strong plugin candidate, usually per provider | Plugin owns its provider's deployment/staging/promotion/rollback workflow. Core supplies the frozen validated export and credential/network broker. Verify actual deployment state and URLs; local checkpoints do not roll back a live site. |
| Project-wide find/replace | Strong authoring-tool plugin candidate | Read scoped canonical data and propose one reviewed replacement transaction. Core resolves shared-definition impact, protected content, checkpointing, undo, and revisions. No new website runtime or source-editing bypass is needed. |
| Client review workflows | Strong workflow plugin candidate | Produce a frozen review package and import versioned annotations against stable page/element IDs. Core retains version mapping/privacy controls. Hosted commenting or live co-editing requires real services and is separate from offline feedback import. |
| URL migration and redirects | Strong plugin candidate, paired with publishing support | Plugin tracks old/new paths, validates loops/chains, and generates rules for the chosen host. Core retains current route/reference integrity. Test actual HTTP redirects; JavaScript navigation is not a server redirect. |
| Visual animation authoring | Hybrid: optional timeline/editor plus declared runtime data | Plugin owns sequences, triggers, easing, and specialized contextual/dialog controls. Core validates targets, explicit layout/protection requirements, reduced-motion requirements, and persisted dependencies. Include actual browser animation code in preview and both exports. |

Collections and localization can remain optional without being architecturally trivial. Select their complete data and compiler interfaces when choosing those features; do not add empty CMS/locale abstractions solely to anticipate them.

The clearest first installable-plugin examples, if that host is later selected, are a publishing adapter and a project-wide find/replace tool. They exercise service access and transactional authoring without first introducing new page-layout semantics. This is an implementation recommendation, not approval to add either candidate to 1.0.

## 4. Three kinds of extension

A content pack supplies validated templates, sections, tokens, or assets. Copy required dependencies into the project and preserve license/provenance. Data-only packs do not need executable app-plugin privileges. A pack containing custom code is executable and must use the corresponding restricted contract.

A website component supplies browser code used by the generated site. It runs under the custom-component preview restrictions and is bundled into output. It does not gain permission to read local files, the project brief, or API keys. Removing the authoring pack must not remove bytes already retained by a project.

An application plugin adds authoring UI/commands, diagnostics, transformations, or service adapters. It needs an app-level contract and controlled access to project information. Its editor interface is not exported into the website.

Tauri's native plugins are a separate developer mechanism: they consist of Rust crates with optional JavaScript bindings and can expose native commands and lifecycle hooks. They do not by themselves provide Desegnos's proposed installable end-user plugin host. Desegnos should treat compiled native extensions as trusted application code. This distinction follows from [Tauri's plugin documentation](https://v2.tauri.app/develop/plugins/).

## 5. Core contracts for future installable plugins

The application remains the authority over mutations, identity, files, paid requests, and permissions. A plugin reads a scoped immutable snapshot and returns typed proposed operations. The Rust domain checks ownership, protections, revision/dependency hashes, references, and schema before one undoable commit with the appropriate durable checkpoint. A plugin cannot mutate the trusted editor DOM or write around project commands.

Use narrow contributions with actual consumers: commands, contextual controls/dialogs, component catalogue entries, transformations, diagnostic findings, provider operations, and export contributions. Contributions must preserve the full canvas, inline prompt text boxes, single job mode, and no-sidebar interaction. Export contributions return bounded artifacts/dependency declarations to the host, which validates paths/content/references before writing. They do not receive an unrestricted output-directory handle or authority to run shell/build scripts.

PROJECT.md and REQUIREMENTS.md stay host-owned. A plugin can propose namespaced requirements tied to selected targets or provide verified capabilities. The host validates, delimits, and includes that information in the frozen requirements file. Plugins cannot replace user prompts, suppress required targets, change accepted facts, or turn Markdown into permission grants. The same constraints apply to AI-provider plugins.

A plugin manifest declares stable identity/version, compatible host-contract versions, supported platforms, owned data schemas, dependencies, licenses, and required capabilities. Project access is limited to granted scopes. Network access is limited to declared service origins; redirects must not escape those grants. Credentials stay in the host keyring/broker. The AI broker retains cost approval, model/endpoint policy, cancellation, and paid-job reconciliation rather than handing raw keys to arbitrary plugin code.

Separately installed executable code needs a genuine constrained runtime, bounded memory/time/output, cancellation, and an isolated UI surface. A separate process alone is not a filesystem/network sandbox. Loading a native library into the main process cannot provide the above restrictions through a manifest alone. Choose and verify that host as its own concrete feature; the current review adds no runtime library or speculative loader to the application.

Installation and updates are explicit application operations. Opening a project must never automatically download or execute a requested plugin. Show publisher/source, compatibility, capabilities, and data effects. Updates that add permissions need renewed grants. Stage and validate schema migrations with recoverable backups; retain compatible versions where project recovery requires them. Signatures establish package identity/integrity, not correctness.

## 6. Missing plugins and independent websites

Persist extension data under stable namespaces with schema versions, plus dependency manifests and actual generated code/assets where relevant. Unknown extension records survive save, undo, and checkpoint restoration; they are not silently discarded. Bound all such data by the project's validated storage limits.

When a plugin materializes standard nodes, assets, or a standalone browser component, retain those results and their exact dependency hashes. If those dependencies are unchanged and the host can validate the saved artifacts, the project can preview/export them without the authoring plugin. Their special editing controls can be unavailable without replacing working output with screenshots or dummy content.

If dependent content changes and the missing plugin must rebuild it, mark the affected result stale and block the affected full-site export until resolved. Offer installing a compatible trusted plugin or an explicit conversion to ordinary editable output where that conversion is real and preserves the chosen behavior. Do not silently export an outdated search index, translation relationship, generated collection page, or animation binding.

Plugin-dependent websites must work after export without Desegnos, its app-plugin host, or the author's key. Include required browser code, data, assets, and distribution notices in both formats. Preserve the release plan's separation between editor licensing and exported runtime/template code. Private PROJECT.md/REQUIREMENTS.md, credentials, job history, and review notes are excluded from public site output.

## 7. Verification for a real plugin release

Test the actual extension enabled, disabled, missing, incompatible, upgraded, and interrupted during a write or migration. Verify that the core can open and preserve the project; failed extension operations cannot corrupt it. A permission change, malicious contribution, hung task, or forged message must not bypass core authority.

Test contributed code in real native preview and both independent site exports. Test exact component versions and licensing, required assets, route collisions, data-size limits, invalidated dependencies, and restoration of projects containing old extension data. Service plugins require real controlled service calls and uncertain-outcome reconciliation. Passing a mock service or a package build is insufficient evidence.

The mandatory 1.0 features remain usable on every advertised desktop platform, including any bundled first-party extension used to deliver them. A future optional plugin can advertise a narrower platform matrix only if the UI and project compatibility behavior state that accurately.

## 8. Scope and estimate consequence

The updated full-release estimate is 45 to 60 engineering weeks plus 9 to 12 weeks of reserve: **54 to 72 weeks** for the stated one-developer assumptions. This retains the AI responsibilities, qualified component reuse, automatic project memory, requirements-file delivery and creative critique/exception handling, and adds complete native ChatGPT-plan authorization/lifecycle and release acceptance alongside BYOK. It is a full-project planning budget, not measured remaining work.

Plugin packaging does not remove the underlying implementation, documentation, integration, export, and recovery work. The estimate includes complete first-party release features and excludes the eight optional candidates, a public installable plugin host, and a marketplace. A smaller launch requires explicitly moving complete features out of 1.0 and recalculating their dependencies and acceptance work; renaming them plugins cannot justify a shorter estimate by itself.
