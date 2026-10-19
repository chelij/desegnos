# Desegnos brand stories

Each story starts from something Desegnos actually does (see [README](../README.md) and [RELEASE_PLAN](../RELEASE_PLAN.md)). The logo is drawn from the story, always as a D.

Shared palette: neutral grey with equal RGB channels, like the editor chrome, with no accent color in the final logo. Earlier concepts used the editor's selection blue `#5291c4`.

## Final logo — Blank to built

Story 2, refined. Left of the tall line, the D's stem is a dashed outline: a request still in words. Right of it, the D's bowl is solid: the built page. The tall line between them is the moment you press Generate. Monochrome, with no accent color.

No background tile. The D fills a square canvas top to bottom on a transparent background.

- `desegnos-dark.svg`: light D for dark backgrounds, used as the app icon (`src-tauri/icons/icon.svg` and `icon.png`)
- `desegnos-light.svg`: dark D for light backgrounds
- `desegnos-auto.svg`: switches with `prefers-color-scheme`, for web and favicon use
- `drafts/`: every earlier concept and variant, including the versions on a rounded tile
- `png/`: both versions at 16 to 1024 px
- `previous-icon/`: the icon this replaced

## 1. Point and say — `drafts/story-1.svg`

**Story.** Desegnos works like sitting next to a designer. There are no panels or inspectors. You point at the page, draw an area, and say what belongs there. The release plan describes it as "like a client pointing at where something belongs."

**Logo.** The D is a dashed selection, the area you drew, with grips on its corners. Inside is a blue text cursor: what you say about it. This also matches the plan's icon brief, which calls for "a selection rectangle and text caret."

## 2. Blank to built — `drafts/story-2.svg`

**Story.** Every page starts empty. You place words, press Generate, and they become a real site that you keep editing on the same canvas. Description and result live in one place.

**Logo.** The left half of the D is a dashed outline, a request still in words. The right half is solid, the built page. The blue line between them is Generate.

## 3. Design in plain language — `drafts/story-3.svg`

**Story.** *Desegno* is Esperanto for "drawing" or "design". Esperanto was made so anyone could be understood without learning a hard language first. Desegnos does the same for web design: you don't need to know CSS or layout. You describe it, and Desegnos builds it.

**Logo.** A solid D carrying the green Esperanto star.

## 4. It remembers — `drafts/story-4.svg`

**Story.** Desegnos keeps a project dossier. Every generated draft, every accepted choice, and every protected rule is recorded, and Undo and Redo restore whole results without another AI call. You can experiment freely because nothing you decided gets lost.

**Logo.** Three D's stacked like drafts. The older ones fade back and the current one sits in front.

## 5. Yours — `drafts/story-5.svg`

**Story.** Desegnos is free, open source, and runs on your own computer. There's no Desegnos account and no hosted proxy. You bring your own OpenAI key or ChatGPT plan, the key stays in your system's credential store, and your project files stay yours.

**Logo.** A key whose head is a D. The program is the key to the site, and the key is yours.

## 6. Real code — `drafts/story-6.svg`

**Story.** What comes out of Desegnos isn't a mockup. It's real HTML, CSS, and JavaScript, exported as a portable website or a React project that you can host anywhere.

**Logo.** The D's curve sharpens into a `>`, and the blue slash inside makes it read as `</D>`, a closing tag.
