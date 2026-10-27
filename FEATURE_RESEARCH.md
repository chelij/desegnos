# Further feature research for Desegnos

Reviewed 2026-10-27 for the visual canvas. The current release plan is [RELEASE_PLAN.md](RELEASE_PLAN.md), the order of work is [FEATURE_PRIORITIES.md](FEATURE_PRIORITIES.md), and plugin boundaries are assessed in [PLUGIN_REVIEW.md](PLUGIN_REVIEW.md).

These are candidates after the canvas, generation, export and import loop is solid. They are product judgments from primary documentation, not measured customer demand.

| Candidate | User problem it addresses | Dependency and scope implication |
| --- | --- | --- |
| Local content collections | Repeated articles, products and profiles are hard to maintain as separate hand-edited layers. | Adds content bindings, templates, generated routes and export rules. Decide early for content-heavy users. |
| Linked multilingual sites | Copying pages by hand does not track translation completeness. | Adds locale-aware elements, routes and review. |
| Visitor site search | Visitors cannot find material on a larger export. | Needs a real exportable index and result UI, with exclusions. |
| Publishing and staging | Users need to put the exported site online and recover a bad deployment. | Needs provider credentials, deployment verification and rollback. Export stays independent. |
| Project-wide find and replace | A phone number or product term can occur on many layers. | Works on the element model and one undo step; respect locked layers and shared linked elements. |
| Client review workflows | Reviewers need to point at a layer and comment against a version. | Adds review packages, stable annotations and feedback import. Live co-editing is separate and larger. |
| URL migration and redirects | Renamed paths leave inbound links broken. | Needs real hosting rules and HTTP verification. |
| Visual animation authoring | Some pages need deliberate sequences without hand-written code. | Adds an animation model, visual controls and reduced-motion output. |
| Reusable blocks | Repeatable sections (header, cards, footer) should be inserted and edited once. | Extends linked elements into named blocks with per-project overrides. |
| Design tokens | Colours, type and spacing should stay consistent across a site. | Builds on the project brief and Styles; exports as variables and utility rules. |
| Asset library | Pictures and logos should be reusable and replaceable. | Extends the project picture store with names, variants and provenance. |

## Selection order

After Tier 1, the strongest candidates are reusable blocks and design tokens: they reduce repeated work for the primary audience and fit the existing linked-element and brief model. Content collections and localisation come next if blogs, catalogues or translated sites are central uses. Publishing and client review are separate hosted products.
