import type { IdentityCandidate, IdentitySource, SourcePage } from './types';
import { colourHex } from './colourValue.js';

export type IdentityFinding = IdentityCandidate['findings'][number];
export type ReviewSection = 'Colours' | 'Fonts' | 'Logos' | 'Style & tone' | 'Other';

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
    const hex = colourHex(match[0], false);
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
  const refs = (pages?: { path: string; title: string; url?: string | null }[]) => pages ?? [];
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
