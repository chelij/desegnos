# Further feature research for Desegnos

Reviewed 18 October 2026. The [release plan](RELEASE_PLAN.md) includes multi-page sites, reusable components/shared styles, interaction/form states, search/sharing settings, fonts/media, persistent checkpoints, site checks, and the project brief. It also specifies Desegnos's automatic project memory, requirements Markdown sent to the agent, verified component reuse, creative design feedback, and native ChatGPT-plan access alongside API keys. The eight capabilities below remain additional candidates; they are excluded from the current 54 to 72 week full-project delivery budget. Their possible plugin boundaries are assessed in the [plugin review](PLUGIN_REVIEW.md).

The strongest next decisions are local content collections and linked multilingual content if blogs, catalogues, or translated sites are central uses. Visitor search and publishing/staging follow naturally as sites grow. Project-wide find/replace is a smaller, broadly useful editing improvement. These priorities are product judgments informed by primary documentation, not measured customer-demand statistics.

| Candidate | User problem it addresses | Dependency and scope implication |
| --- | --- | --- |
| Local content collections | Repeated articles, products, team profiles, and listings become difficult to maintain as separate hand-edited pages. | Changes content bindings, templates, generated routes, AI edit scope, and both exporters. Decide early for content-heavy target users. |
| Linked multilingual sites | Copying pages manually does not track translation completeness or language relationships. | Adds locale-aware content/components, routes, metadata, and translation review. Decide early for multilingual target users. |
| Visitor site search | Visitors cannot find material once a site has many pages. | Requires a real exportable index and result UI, with content exclusions and locale rules. |
| Publishing and staging | Users need to put the exported site online, test it, and recover a bad deployment. | Requires real provider credentials/APIs, deployment verification, and rollback; independent exporting remains available. |
| Project-wide find/replace | A phone number, URL, product term, or metadata change can occur in many pages/components. | Uses the canonical model and transactional history; preserve read-only source and protected-content boundaries. |
| Client review workflows | Reviewers need to identify an element and give feedback against a particular version. | Adds review packages, stable annotations, feedback import, and privacy controls. Live co-editing is a separate, larger capability. |
| URL migration and redirects | Renamed published paths leave old inbound links pointing to missing content. | Requires real hosting rules and HTTP verification, not just updating internal links. |
| Visual animation authoring | Some sites need deliberate sequences or scroll-linked motion without hand-editing custom code. | Adds an animation model, visual controls, reduced-motion alternatives, and runtime/export parity. |

## 1. Local content collections

Mature visual editors treat structured content and its presentation separately. [Webflow's collection documentation](https://help.webflow.com/hc/en-us/articles/33961244391059-Manage-CMS-Collections) describes schemas, entries, template pages, and references; its [CSV import/export documentation](https://help.webflow.com/hc/en-us/articles/33961290794771-How-do-I-import-content-into-the-Webflow-CMS) shows the bulk-editing workflow.

A complete Desegnos capability would keep typed collections inside the project: text/rich text, images/assets, dates, numbers, links, options, and item references. Users would create real items, bind component fields to them, filter/sort lists, paginate, and generate item pages from a reusable template. CSV/JSON import would show field mappings, validation failures, and update-versus-add behavior before changing data. Export would include the actual content and required asset bytes, with no dependency on a temporary vendor image URL or an app account.

AI changes would target a collection schema, template, or selected items explicitly. A template update must report all affected pages. Deleting a referenced item must resolve its uses. Drafts and missing required fields cannot become fictional published entries. Scheduled publication would require a real scheduled deployment mechanism and should not be advertised merely because an item has a date field.

Acceptance would import a real licensed dataset, edit items, update a template, save/reopen/restore a checkpoint, and export both formats. Verify generated routes, pagination, references, metadata, assets, and deliberate draft exclusion. This would expand the 1.0 data and export contracts if selected now; it is not satisfied by AI generating a repeated set of cards once.

## 2. Linked multilingual sites

The accepted release can author Unicode/right-to-left content and set page language, but it does not yet link translations. [Google's localized-page guidance](https://developers.google.com/search/docs/specialty/international/localized-versions) specifies language/region relationships, fully qualified alternate URLs, and links from each language version to itself and its alternatives.

A complete capability would have locale settings, linked page/item translations, locale-specific text/assets/metadata, localized paths, a working language switcher, and visible missing/outdated translation status. Components could share layout while exposing translated content. Users would choose fallback behavior rather than silently publishing untranslated text. Translation through BYOK would respect fixed brand facts and protected terms, display cost/scope, and require review before applying changes.

Exports would contain real localized pages, correct language/direction, alternate relationships, canonical URLs, and sitemap entries. Acceptance would use at least two actual languages plus a right-to-left example, test incomplete translations and shared-component edits, and verify browser navigation and metadata in both exports. Add this before the final project schema if multilingual sites are a launch requirement; copying two unrelated pages does not implement translation management.

## 3. Visitor site search

[Webflow documents a visitor search workflow](https://help.webflow.com/hc/en-us/articles/33961242348179-Site-search) with results, snippets, and page/element exclusions. Its documentation also states that its search does not work in exported sites. For Desegnos, export independence makes an included search index a central acceptance requirement.

A complete capability would index the actual included site text and metadata during export, provide a keyboard-accessible search field/results view, and support useful snippets, highlights, pagination, and real empty results. Users could exclude pages or repeated navigation content separately from search-engine indexing. Never include draft content, the private brief, history, credentials, or form values. If collections or locales are selected, their published content and language relationships must be reflected in the index.

Bundle the index/runtime so portable output does not fetch local JSON or depend on a hosted search account. The editor preview must search actual current content. Acceptance would search known present/absent terms, Unicode and accented text, update content and regenerate the index, verify exclusions, and open both exported search experiences independently. Large-index budgets must be measured on representative sites before advertising support limits.

## 4. Publishing and staging

Publishing introduces a different kind of version history from the accepted local project checkpoints. [Framer's staging documentation](https://www.framer.com/help/articles/staging-and-versions/) describes version-specific previews, testing before deploying to a custom domain, and rollback. [GitHub Pages documentation](https://docs.github.com/en/pages/getting-started-with-github-pages/what-is-github-pages) is an example of a real static hosting target for HTML/CSS/JavaScript; it is evidence of a deployment route, not a decision to make it the only provider.

A complete capability would integrate one explicitly selected hosting provider end to end: connect the user's account, store credentials in the OS keyring, upload a frozen export, show the actual deployment identifier/status, open a real staging URL, and promote the reviewed artifact to production. Configure and verify domain/base-path, TLS, cache behavior, nested-page entry, error-page status, and asset availability. The publish action must show the concrete target and content revision.

Interrupted requests require reconciliation with provider state rather than duplicate deployments. Rollback must restore the previous deployed artifact without pretending that a local project restore changes the live website. Keep provider credentials and deployment-only configuration out of public site files. Acceptance would create a real staging deployment, verify it externally, publish the same tested artifact, interrupt a deployment, and roll back. Account access is optional for local authoring/export, and this work adds external operations and maintenance beyond the current desktop-only release.

## 5. Project-wide find and replace

[Dreamweaver's find/replace documentation](https://helpx.adobe.com/nz/dreamweaver/desktop/page-content-and-assets/find-replace-text.html) includes document, folder, and whole-site scopes. That workflow is directly relevant once Desegnos owns a multi-page project.

A complete capability would search editable page text, shared definitions, instance overrides, link destinations, alternative text, and metadata. Users could filter scope, navigate matches, preview proposed replacements and affected instances, and commit one atomic transaction with a safety checkpoint. Changing a definition should replace the source value once and report its instance impact, rather than accidentally replacing repeated rendered copies. Protected content needs a deliberate unlock or explicit protected-target change.

Generated source remains inspectable but read-only; find/replace cannot become an undocumented source-editing route. Acceptance would update real repeated text and URLs across pages/components, verify exact match counts and excluded/protected matches, undo the change, reopen the project, and compare both exports. This is a strong usability candidate with less architectural disruption than collections or localization.

## 6. Client review workflows

[Webflow documents comment-only sharing for stakeholders](https://help.webflow.com/hc/en-us/articles/33961254829843-Invite-new-Workspace-members-or-share-a-sandbox-site-link). [Framer's collaboration documentation](https://www.framer.com/help/articles/live-collaboration-in-framer/) shows that simultaneous editing additionally involves identity, presence, and permissions. These are related but different product commitments.

An initial complete review capability could export a frozen review package, let a reviewer annotate a specific page/element/viewport, and import a versioned feedback file into the desktop project. Include resolve/reopen, author labels that are not presented as verified identities, and clear handling of notes attached to changed or deleted elements. Sharing must exclude the private brief, prompts, checkpoint history, and credentials unless the user explicitly includes appropriate material.

A shared live comment service would require a chosen hosting/identity/access model; a local read-only flag is not confidentiality or authentication. Actual simultaneous editing would also require shared-state synchronization, conflict resolution, reconnection, and authorization tests. Do not expose a live-collaboration control while implementing only offline feedback. Acceptance for review packages would exchange real notes against a fixed revision and preserve them after edits/reopening, without allowing feedback import to execute code or alter the site automatically.

## 7. URL migration and redirects

The accepted release updates internal page links after a path change. Existing links from other websites require a separate solution. [Google's site-move guidance](https://developers.google.com/search/docs/crawling-indexing/site-move-with-url-changes) recommends server-side permanent redirects where possible and avoiding redirect chains.

A complete capability would record old-to-new path mappings, validate destinations, detect loops/chains/collisions, and generate real configuration for the selected supported host. A publishing adapter would apply and verify those rules. A static export could include a documented host-specific redirect manifest, but a JavaScript navigation or ordinary HTML file must not be labelled an HTTP 301/308 response.

Acceptance would request old URLs on a real controlled host, verify the response status and final destination, and test base paths, query handling, missing destinations, and rollback. Prioritize this alongside publishing if users maintain already-public sites; it is less urgent for projects that have never been deployed.

## 8. Visual animation authoring

The [Web Animations API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Animations_API) provides browser animation primitives, and [`prefers-reduced-motion`](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/At-rules/@media/prefers-reduced-motion) exposes the visitor's motion preference. These support a standards-based route without selecting an unrelated animation library now.

A complete capability would expose deliberate targets, timing/easing, sequences, user/scroll triggers, and start/end states through contextual tools or dialogs, with a reduced-motion alternative. Users must be able to preview, pause, remove, and undo an animation, including on reusable components and at breakpoints. Preserve reading order, keyboard focus, content visibility, and explicit site layout constraints. Do not add entrance effects to every generated page by default or introduce an editor sidebar.

Acceptance would exercise actual triggers, scroll/resize, reduced motion, component instances, save/reopen/restore, and equivalent behavior in both exports and native engines. The accepted interaction-state styling remains useful without a full animation timeline, so this is a lower-priority addition unless motion design is a central target use.

## Selection order

For blogs/catalogues, prioritize collections, then visitor search. For multilingual business sites, prioritize linked translations. For users delivering sites to clients, prioritize publishing/staging, review, and redirects. Project-wide find/replace is useful across these audiences and can be selected without promising a hosted platform.

Before accepting another capability, add its complete behavior, storage/AI boundaries, export contract, acceptance scenarios, and work estimate to the main plan. Keep the approved release usable on its own; do not add speculative package dependencies, disabled menu items, or simulated services for candidates that have not been selected.
