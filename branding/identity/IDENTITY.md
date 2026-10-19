# Desegnos design identity

Built from the final "Blank to built" logo (source: `~/Projects/WebMaker/branding/`). Open `index.html` for the visual guide.

## Story

Left of the tall line, the D's stem is a dashed outline: a request still in words. Right of it, the bowl is solid: the built page. The line between them is the moment you press Generate. Everything below extends that idea.

## Logo

| File | Use |
|---|---|
| `logo/desegnos-light.svg` | Dark ink `#262626`, for light grounds |
| `logo/desegnos-dark.svg` | Light ink `#F2F2F2`, for dark grounds; the app icon |
| `logo/desegnos-auto.svg` | Switches with `prefers-color-scheme`; favicons and web |
| `logo/png/` | Both inks at 16–1024 px |

- Monochrome only, no tile, no accent color.
- Clear space: ¼ of the mark's height on every side.
- Minimum size: 16 px.
- Never recolor, rotate, stretch, outline or fill in the dashed stem.

**Wordmark.** The mark is the D, followed by `esegnos` in Archivo Bold (700), tracking −2%. The D body is sized to the cap height, so the seam overshoots the letters like a caret. Use the `.ds-wordmark` class in `tokens.css`. Outlined lockups: `logo/desegnos-wordmark-light.svg`, `-dark.svg` and `-auto.svg`. Rebuild them with `python3 -I scripts/build-wordmark.py` (needs fontTools) after changing the mark or font settings.

## Color

Every neutral has equal R, G and B channels, matching the editor chrome. Blue is functional only: selection, focus, guides and the primary action. It never appears in the logo or as decoration.

| Token | Light | Dark |
|---|---|---|
| ink | `#262626` | `#F2F2F2` |
| bg | `#F2F2F2` | `#1A1A1A` |
| surface | `#FFFFFF` | `#262626` |
| text-muted | `#4B4B4B` | `#B1B1B1` |
| line | `#626262` | `#8A8A8A` |
| accent / focus | `#005A9E` | `#8AC4F5` |
| guide | `#0070B8` | `#5291C4` |
| danger | `#A4262C` | `#FFB5B5` |

Full grey scale (0–950) is in `tokens.css` and `tokens.json`.

## Type

- **Archivo** 500–700: display, headings, wordmark. Self-hosted in `fonts/archivo/` (variable weight and width, TTF and WOFF2, SIL OFL). Tracking −1.5% at large sizes, −2% in the wordmark.
- **Inter** 400–600: body and UI on the web. 16/1.55.
- **JetBrains Mono** 400–500: code, measurements, small uppercase labels (+8% tracking).
- The desktop editor keeps system UI fonts for native controls, as `WebMaker/DESIGN.md` specifies.

Scale: 12 · 14 · 16 · 20 · 28 · 40 · 56 · 80.

## Language: dashed, solid, seam

- **Dashed** (`.ds-request`, 2 px dashed line): anything still described — inputs, placeholders, empty states, "add" actions.
- **Solid** (`.ds-built`, ink fill): results, the built site, primary content blocks.
- **Seam** (`.ds-seam`, 3 px): a vertical divider that slightly overshoots what it separates. Use it between intent and result, not as a generic rule.

## Layout and shape

- 8 px grid (4 px half-step). Generous whitespace; the canvas is the hero.
- Frames, canvases and content blocks are square. Controls get 4 px radius, cards 6 px.
- Strokes: 1 px hairline, 2 px line, 3 px seam.

## Motion and tone

- Motion is short control feedback (120–180 ms) and respects reduced motion. No looping animation.
- No gradients, glows or decorative AI imagery.
- Pair every color signal with text or an outline.
- Copy is short and ordinary: "Generate", "Add area", "Preview".
