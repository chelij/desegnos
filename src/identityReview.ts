import type { IdentityCandidate, IdentitySource, SourcePage } from './types';

export type IdentityFinding = IdentityCandidate['findings'][number];
export type ReviewSection = 'Colours' | 'Fonts' | 'Logos' | 'Style & tone' | 'Other';

// CSS named colours, kept local so parsing also works outside a browser.
const namedColours = Object.fromEntries(`aliceblue:f0f8ff antiquewhite:faebd7 aqua:00ffff aquamarine:7fffd4 azure:f0ffff beige:f5f5dc bisque:ffe4c4 black:000000 blanchedalmond:ffebcd blue:0000ff blueviolet:8a2be2 brown:a52a2a burlywood:deb887 cadetblue:5f9ea0 chartreuse:7fff00 chocolate:d2691e coral:ff7f50 cornflowerblue:6495ed cornsilk:fff8dc crimson:dc143c cyan:00ffff darkblue:00008b darkcyan:008b8b darkgoldenrod:b8860b darkgray:a9a9a9 darkgreen:006400 darkgrey:a9a9a9 darkkhaki:bdb76b darkmagenta:8b008b darkolivegreen:556b2f darkorange:ff8c00 darkorchid:9932cc darkred:8b0000 darksalmon:e9967a darkseagreen:8fbc8f darkslateblue:483d8b darkslategray:2f4f4f darkslategrey:2f4f4f darkturquoise:00ced1 darkviolet:9400d3 deeppink:ff1493 deepskyblue:00bfff dimgray:696969 dimgrey:696969 dodgerblue:1e90ff firebrick:b22222 floralwhite:fffaf0 forestgreen:228b22 fuchsia:ff00ff gainsboro:dcdcdc ghostwhite:f8f8ff gold:ffd700 goldenrod:daa520 gray:808080 green:008000 greenyellow:adff2f grey:808080 honeydew:f0fff0 hotpink:ff69b4 indianred:cd5c5c indigo:4b0082 ivory:fffff0 khaki:f0e68c lavender:e6e6fa lavenderblush:fff0f5 lawn green:7cfc00 lemonchiffon:fffacd lightblue:add8e6 lightcoral:f08080 lightcyan:e0ffff lightgoldenrodyellow:fafad2 lightgray:d3d3d3 lightgreen:90ee90 lightgrey:d3d3d3 lightpink:ffb6c1 lightsalmon:ffa07a lightseagreen:20b2aa lightskyblue:87cefa lightslategray:778899 lightslategrey:778899 lightsteelblue:b0c4de lightyellow:ffffe0 lime:00ff00 limegreen:32cd32 linen:faf0e6 magenta:ff00ff maroon:800000 mediumaquamarine:66cdaa mediumblue:0000cd mediumorchid:ba55d3 mediumpurple:9370db mediumseagreen:3cb371 mediumslateblue:7b68ee mediumspringgreen:00fa9a mediumturquoise:48d1cc mediumvioletred:c71585 midnightblue:191970 mintcream:f5fffa mistyrose:ffe4e1 moccasin:ffe4b5 navajowhite:ffdead navy:000080 oldlace:fdf5e6 olive:808000 olivedrab:6b8e23 orange:ffa500 orangered:ff4500 orchid:da70d6 palegoldenrod:eee8aa palegreen:98fb98 paleturquoise:afeeee palevioletred:db7093 papayawhip:ffefd5 peachpuff:ffdab9 peru:cd853f pink:ffc0cb plum:dda0dd powderblue:b0e0e6 purple:800080 rebeccapurple:663399 red:ff0000 rosybrown:bc8f8f royalblue:4169e1 saddlebrown:8b4513 salmon:fa8072 sandybrown:f4a460 seagreen:2e8b57 seashell:fff5ee sienna:a0522d silver:c0c0c0 skyblue:87ceeb slateblue:6a5acd slategray:708090 slategrey:708090 snow:fffafa springgreen:00ff7f steelblue:4682b4 tan:d2b48c teal:008080 thistle:d8bfd8 tomato:ff6347 turquoise:40e0d0 violet:ee82ee wheat:f5deb3 white:ffffff whitesmoke:f5f5f5 yellow:ffff00 yellowgreen:9acd32 transparent:00000000`.replace('lawn green', 'lawngreen').split(' ').map(pair => pair.split(':')));
const numberToken = /^[-+]?(?:\d*\.)?\d+%?$/;
const clamp = (value: number, max = 1) => Math.max(0, Math.min(max, value));
const byte = (value: number) => Math.round(clamp(value, 255)).toString(16).padStart(2, '0');

function colourHex(value: string): string | null {
  const token = value.toLowerCase();
  if (Object.hasOwn(namedColours, token)) return `#${namedColours[token]}`;
  if (/^#(?:[\da-f]{3,4}|[\da-f]{6}|[\da-f]{8})$/.test(token)) return token.length <= 5 ? `#${[...token.slice(1)].map(char => char + char).join('')}` : token;
  const fn = /^(rgba?|hsla?)\(([^)]+)\)$/.exec(token);
  if (!fn) return null;
  const parts = fn[2].trim().split(/[\s,/]+/);
  if (parts.length < 3 || parts.length > 4 || parts.slice(1).some(part => !numberToken.test(part))) return null;
  const alpha = parts[3] === undefined ? 1 : clamp(parseFloat(parts[3]) / (parts[3].endsWith('%') ? 100 : 1));
  let rgb: number[];
  if (fn[1].startsWith('rgb')) {
    if (!numberToken.test(parts[0])) return null;
    rgb = parts.slice(0, 3).map(part => clamp(part.endsWith('%') ? parseFloat(part) / 100 * 255 : parseFloat(part), 255));
  } else {
    const hue = /^([-+]?(?:\d*\.)?\d+)(deg|grad|rad|turn)?$/.exec(parts[0]);
    if (!hue || !parts[1].endsWith('%') || !parts[2].endsWith('%')) return null;
    const degrees = Number(hue[1]) * ({ deg: 1, grad: 0.9, rad: 180 / Math.PI, turn: 360 }[hue[2] || 'deg'] ?? 1);
    if (!Number.isFinite(degrees)) return null;
    const h = ((degrees % 360) + 360) % 360 / 60;
    const s = clamp(parseFloat(parts[1]) / 100), l = clamp(parseFloat(parts[2]) / 100);
    const c = (1 - Math.abs(2 * l - 1)) * s, x = c * (1 - Math.abs(h % 2 - 1)), m = l - c / 2;
    rgb = (h < 1 ? [c, x, 0] : h < 2 ? [x, c, 0] : h < 3 ? [0, c, x] : h < 4 ? [0, x, c] : h < 5 ? [x, 0, c] : [c, 0, x]).map(channel => (channel + m) * 255);
  }
  return `#${rgb.map(byte).join('')}${alpha < 1 ? byte(alpha * 255) : ''}`;
}

const colourRole = (text: string) => {
  const roles = [...text.matchAll(/\b(background|paper|surface|accent|primary|secondary|text|ink|foreground|border|highlight)\b/gi)];
  const role = roles.at(-1)?.[1].toLowerCase();
  return role ? ({ paper: 'Background', surface: 'Surface', ink: 'Text', foreground: 'Text', highlight: 'Accent' }[role] ?? role[0].toUpperCase() + role.slice(1)) : null;
};

export function parseColours(value: string, topic = ''): { hex: string; label: string }[] {
  const matches = [...value.matchAll(/#[\da-f]+\b|\b(?:rgba?|hsla?|var)\([^)]*\)|\b[a-z]+\b/gi)];
  const colours: { hex: string; label: string }[] = [];
  let previousEnd = 0;
  for (let index = 0; index < matches.length; index++) {
    const match = matches[index], start = match.index;
    const hex = colourHex(match[0]);
    if (!hex) continue;
    const next = matches[index + 1];
    // In “brand blue #225577”, blue describes the explicit value, not a second swatch.
    if (/^[a-z]+$/i.test(match[0]) && next && /^(#|rgb|hsl)/i.test(next[0]) && /^[\s:(=]*$/.test(value.slice(start + match[0].length, next.index))) continue;
    const local = value.slice(previousEnd, start);
    const followingRole = /^\s+(?:for|as|on)\s+(?:the\s+)?(background|paper|surface|accent|primary|secondary|text|ink|foreground|border|highlight)\b/i.exec(value.slice(start + match[0].length));
    const label = colourRole(followingRole?.[1] ?? '') ?? colourRole(local) ?? colourRole(topic) ?? 'Colour';
    if (!colours.some(colour => colour.hex === hex)) colours.push({ hex, label });
    previousEnd = start + match[0].length;
  }
  return colours;
}

export function parseFonts(value: string, topic = ''): { family: string; role: 'Headings' | 'Body' | 'Font' }[] {
  const fonts: ReturnType<typeof parseFonts> = [];
  const generic = /^(?:serif|sans-serif|monospace|cursive|fantasy|system-ui|ui-serif|ui-sans-serif|ui-monospace|ui-rounded|inherit|initial|unset)$/i;
  const clauses = value.split(/;|\n|\.\s+(?=(?:body|headings?|titles?)\b)|,\s*(?=(?:body|headings?|titles?)\b)|\s+and\s+(?=[^.;]*\bfor\s+(?:body|headings?)\b)/i);
  for (const clause of clauses) {
    const roleContext = /\b(headings?|titles?|body|paragraphs?|h[1-6])\b/i.exec(clause)?.[1] ?? topic;
    const role = /\b(body|paragraphs?)\b/i.test(roleContext) ? 'Body' : /\b(headings?|titles?|display|h[1-6])\b/i.test(roleContext) ? 'Headings' : 'Font';
    const cleaned = clause.trim().replace(/^(?:use\s+)?(?:(?:headings?|titles?|body|paragraphs?|display)(?:\s+font)?\s*(?::|=|use[sd]?|in|are|is)?\s*)?(?:(?:font(?:-family)?|typeface|typography)\s*[:=]\s*)?/i, '')
      .replace(/\s+(?:for|on|as)\s+(?:the\s+)?(?:headings?|titles?|body|paragraphs?|body text|display).*$/i, '').replace(/[.!]+$/, '').trim();
    const quoted = [...cleaned.matchAll(/"([^"]+)"|'([^']+)'/g)].map(match => match[1] ?? match[2]);
    const families = quoted.length ? quoted : cleaned.split(',');
    for (const part of families) {
      const family = part.trim().replace(/^['"]|['"]$/g, '').trim();
      if (!family || family.length > 70 || /[{}:;()=<>]/.test(family)) continue;
      if (generic.test(family) && families.some(part => !generic.test(part.trim().replace(/^['"]|['"]$/g, '')))) continue;
      if (!fonts.some(font => font.family.toLowerCase() === family.toLowerCase() && font.role === role)) fonts.push({ family, role });
    }
  }
  return fonts;
}

export function parseTone(value: string): string[] {
  const known: [RegExp, string][] = [
    [/\b(?:lots of |generous |ample )?(?:white\s*space|negative space)\b/i, 'Lots of white space'],
    [/\brounded(?: corners?| edges?)?\b/i, 'Rounded corners'],
    [/\b(?:sharp|square) corners?\b/i, 'Square corners'],
    [/\b(?:minimal|minimalist)\b/i, 'Minimal'],
    ...['Friendly', 'Playful', 'Warm', 'Clear', 'Confident', 'Calm', 'Welcoming', 'Bold', 'Professional', 'Conversational', 'Helpful', 'Concise', 'Elegant', 'Colourful', 'Colorful', 'Clean', 'Spacious', 'Approachable', 'Energetic', 'Editorial', 'Formal', 'Casual'].map(label => [new RegExp(`\\b${label}\\b`, 'i'), label] as [RegExp, string]),
  ];
  const labels = known.flatMap(([pattern, label]) => {
    const match = pattern.exec(value);
    return match && !/\b(?:not|no|avoid|without)\s+(?:a\s+)?$/i.test(value.slice(0, match.index)) ? [{ label, index: match.index }] : [];
  }).sort((a, b) => a.index - b.index).map(item => item.label);
  if (labels.length) return [...new Set(labels)];
  return [...new Set(value.split(/[,;.!\n]|\s+and\s+/i).map(part => part.trim().replace(/^(?:use|keep|a|an|the)\s+/i, '')).filter(Boolean).map(part => {
    const short = part.split(/\s+/).slice(0, 5).join(' ');
    return short[0].toUpperCase() + (short.length > 38 ? short.slice(1, 35) + '…' : short.slice(1)) + (part !== short && short.length <= 38 ? '…' : '');
  }))];
}

export function findingSection(finding: IdentityFinding): ReviewSection {
  if (/\b(fonts?|typefaces?|typography)\b/i.test(finding.topic) || /font-family\s*:/i.test(finding.value)) return 'Fonts';
  if (/\b(logo|logos|wordmark|brand mark)\b/i.test(finding.topic)) return 'Logos';
  if (/colou?r|palette|background|accent/i.test(finding.topic) || /#[\da-f]{3,8}\b|\b(?:rgb|hsl)a?\(/i.test(finding.value)) return 'Colours';
  if (/tone|voice|style|personality|spacing|corners?|layout|shape/i.test(finding.topic)) return 'Style & tone';
  return 'Other';
}

export function groupIdentityPages(source: IdentitySource | null, content: NonNullable<IdentityCandidate['content']> = []) {
  const groups = new Map<string, { page: SourcePage; photos: NonNullable<IdentitySource['photos']>; items: { item: typeof content[number]; index: number }[] }>();
  const add = (page: SourcePage) => {
    let group = groups.get(page.path);
    if (!group) { group = { page, photos: [], items: [] }; groups.set(page.path, group); }
    return group;
  };
  for (const page of source?.pages ?? []) add(page);
  const fallback: SourcePage = { path: '/', title: 'Home', kind: 'Home', photoCount: 0, itemCount: 0 };
  const refs = (pages?: { path: string; title: string; url?: string | null }[]) => pages?.length ? pages : [fallback];
  for (const photo of source?.photos ?? []) for (const page of refs(photo.sourcePages)) {
    const group = add({ ...page, kind: page.path === '/' ? 'Home' : 'Other', photoCount: 0, itemCount: 0 });
    if (!group.photos.some(item => item.id === photo.id)) group.photos.push(photo);
  }
  content.forEach((item, index) => { for (const page of refs(item.sourcePages)) {
    const group = add({ ...page, kind: page.path === '/' ? 'Home' : 'Other', photoCount: 0, itemCount: 0 });
    if (!group.items.some(item => item.index === index)) group.items.push({ item, index });
  } });
  return [...groups.values()].sort((a, b) => a.page.path === '/' ? -1 : b.page.path === '/' ? 1 : a.page.path.localeCompare(b.page.path));
}

// Child choices are independent of page choices, so a shared photo can remain on another
// selected page and toggling a page off/on does not erase the user's individual choices.
export function selectedPageContent(groups: ReturnType<typeof groupIdentityPages>, paths: string[], photos: string[], items: number[]) {
  const selected = groups.filter(group => paths.includes(group.page.path));
  return {
    photos: [...new Set(selected.flatMap(group => group.photos.filter(photo => photos.includes(photo.id)).map(photo => photo.id)))],
    items: [...new Set(selected.flatMap(group => group.items.filter(item => items.includes(item.index)).map(item => item.index)))],
  };
}
