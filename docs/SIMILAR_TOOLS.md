# Similar tools and what Desegnos can adapt

Reviewed 2026-10-27 for the visual canvas. Scope: tools close to Desegnos's loop (place elements, describe a frame, AI fills it in, export ordinary files), for non-technical users. Platform features (CMS, translations, site search, publishing) are in [FEATURE_RESEARCH.md](../FEATURE_RESEARCH.md).

| Tool | Why it is close | Idea worth taking |
| --- | --- | --- |
| **tldraw Make Real** | Draw a rough UI, select it, AI turns the selection into a working page. | Draw inside a frame, revise by drawing or writing notes over the result, send only the selected area. |
| **Google Stitch** | Upload a sketch or screenshot and get an editable design plus HTML/CSS. | Reference image per frame ("make it look like this") and optional 2–3 variations. |
| **Relume** | Describe a business, approve a sitemap, wireframes, style guide, then the design. | AI drafts a plan as boundaries and frames that the user moves, edits or deletes before generating. |
| **Webflow AI builder** | Generates a style guide with the site; new sections reuse existing styles. | A visual style guide from the project brief, and new sections that reuse it. |
| **Wix Studio Responsive AI** | Proposes a mobile layout for one section with preview, apply or discard. | **Fix on phones** for one selected layer or size, previewed before applying. |
| **Lovable visual edits** | Select and change text, size or colour directly; prompt for bigger changes. | A compact style row in Properties; no dialog for ordinary visual edits. |
| **Pinegrow** | Dreamweaver-like editor with BYOK AI, smart insert and repeatable edits. | Duplicate with AI, repeat my edit, saved requests. |
| **Bootstrap Studio** | Desktop builder with a BYOK agent across providers and URL import. | Brand/site URL import and the estimated cost before Generate. |
| **Onlook** | Visual editor where AI is limited to the project's own components. | Reuse existing layers and linked elements instead of inventing new styles. |
| **GrapesJS / Webstudio** | Open-source builders with blocks, a style manager and static export. | Blocks could become reusable presets later; confirms the portable export direction. |

## Ideas ranked for Desegnos

| Idea | Fit | Effort |
| --- | --- | --- |
| AI drafts a plan as boundaries and frames | High: keeps placement in the user's hands and fixes the blank-canvas problem. | M |
| Reference image in a frame | High: the request attachment path already carries pictures. | M |
| Style row on the selection | High: most controls already exist in Properties. | S |
| Visual style guide from the brief | High: makes decisions legible. | M |
| Fix on phones for one layer or size | High: linked sizes already exist. | M |
| Sketch inside a frame | High, but a larger drawing layer. | L |
| Variations for one frame | Medium: costs several requests, so opt-in with the estimate. | M |
| Cost before Generate | Medium: prevents bill surprises. | S |
| Duplicate with AI and saved requests | Medium. | S–M |

## Not worth copying

- **Chat-first building.** It reverses Desegnos's placement-first idea; keep chat out of the main flow.
- **Hosted-only output.** Portable exports are the advantage; keep them.
- **Huge component marketplaces.** Reusable blocks and the AI plan give most of the benefit with less maintenance.
