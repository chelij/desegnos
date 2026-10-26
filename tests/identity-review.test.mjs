import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.identity-review-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/identityReview.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { parseColours, parseFonts, parseTone, findingSection, groupIdentityPages, selectedPageContent } = await import(pathToFileURL(join(temporary, 'identityReview.js')).href);
  const hexes = value => parseColours(value).map(colour => colour.hex);
  await test('several hex colours keep local roles and expand shorthand', () => {
    assert.deepEqual(parseColours('Background: #faf8f2; Accent: #257; Text: #18232a'), [
      { label: 'Background', hex: '#faf8f2' }, { label: 'Accent', hex: '#225577' }, { label: 'Text', hex: '#18232a' },
    ]);
  });
  await test('descriptive names before explicit values do not create extra swatches', () => {
    assert.deepEqual(hexes('Use Northstar blue #225577, warm white #faf8f2, and ink #18232a.'), ['#225577', '#faf8f2', '#18232a']);
  });
  await test('colour roles can follow each explicit colour', () => {
    assert.deepEqual(parseColours('Use #fff for the background and #222 for text.'), [{ hex: '#ffffff', label: 'Background' }, { hex: '#222222', label: 'Text' }]);
  });
  await test('named colours, aliases, case and duplicates normalize', () => {
    assert.deepEqual(hexes('Rebeccapurple, navy, white, grey, gray, #fff, lawngreen'), ['#663399', '#000080', '#ffffff', '#808080', '#7cfc00']);
  });
  await test('RGB legacy and modern syntax, percentages and alpha normalize', () => {
    assert.deepEqual(hexes('rgb(34, 85, 119); rgb(100% 0% 50%); rgba(0, 0, 0, 0.5); rgb(255 0 0 / 25%)'), ['#225577', '#ff0080', '#00000080', '#ff000040']);
  });
  await test('HSL supports hue wrapping, units, achromatic values and alpha', () => {
    assert.deepEqual(hexes('hsl(480, 100%, 50%); hsl(.5turn 100% 50%); hsl(200grad 0% 50% / 50%); hsl(3.141592653589793rad 100% 50%)'), ['#00ff00', '#00ffff', '#80808080']);
  });
  await test('alpha hex and transparent are retained', () => {
    assert.deepEqual(hexes('#aBcD, #ABCDEF80, transparent'), ['#aabbccdd', '#abcdef80', '#00000000']);
  });
  await test('invalid colours and context-dependent values are ignored', () => {
    assert.deepEqual(hexes('#12345 #123456789 rgb(no 2 3) hsl(10 20 30) var(--red) currentColor constructor'), []);
  });
  await test('CSS channel ranges are clamped and labels fall back to the topic', () => {
    assert.deepEqual(parseColours('rgb(300 -20 0)', 'Accent colour'), [{ hex: '#ff0000', label: 'Accent' }]);
  });
  await test('missing values produce no choices', () => {
    assert.deepEqual(parseColours(''), []); assert.deepEqual(parseFonts(''), []); assert.deepEqual(parseTone(''), []);
  });
  await test('fonts retain family names and distinguish headings from body', () => {
    assert.deepEqual(parseFonts('Headings: Bricolage Grotesque; Body: Inter, system-ui, sans-serif.'), [{ family: 'Bricolage Grotesque', role: 'Headings' }, { family: 'Inter', role: 'Body' }]);
  });
  await test('CSS font stacks omit generic fallbacks and remove quotes', () => {
    assert.deepEqual(parseFonts('font-family: "Bricolage Grotesque", sans-serif', 'Heading font'), [{ family: 'Bricolage Grotesque', role: 'Headings' }]);
  });
  await test('quoted families can be extracted from prose', () => {
    assert.deepEqual(parseFonts('The site uses "Bricolage Grotesque" for headings.'), [{ family: 'Bricolage Grotesque', role: 'Headings' }]);
  });
  await test('Display within a family name does not override its stated role', () => {
    assert.deepEqual(parseFonts('Body: Playfair Display'), [{ family: 'Playfair Display', role: 'Body' }]);
    assert.deepEqual(parseFonts('Playfair Display'), [{ family: 'Playfair Display', role: 'Font' }]);
  });
  await test('natural language font roles and conjunctions parse', () => {
    assert.deepEqual(parseFonts('Use Bricolage Grotesque for headings and Inter for body.'), [{ family: 'Bricolage Grotesque', role: 'Headings' }, { family: 'Inter', role: 'Body' }]);
  });
  await test('font families deduplicate within roles and generic-only fonts remain usable', () => {
    assert.deepEqual(parseFonts('Arial, Arial, sans-serif', 'Body typography'), [{ family: 'Arial', role: 'Body' }]);
    assert.deepEqual(parseFonts('monospace'), [{ family: 'monospace', role: 'Font' }]);
  });
  await test('font values cannot become injected CSS declarations or URLs', () => {
    assert.deepEqual(parseFonts('url(https://example.com/font.woff2)'), []);
    assert.deepEqual(parseFonts('<style>oops</style>'), []);
  });
  await test('tone prose becomes short ordered pills', () => {
    assert.deepEqual(parseTone('Use a friendly, playful voice, rounded corners and lots of white space.'), ['Friendly', 'Playful', 'Rounded corners', 'Lots of white space']);
  });
  await test('tone synonyms normalize and repeated words deduplicate', () => {
    assert.deepEqual(parseTone('Minimalist, minimal, with ample negative space. Warm and warm.'), ['Minimal', 'Lots of white space', 'Warm']);
  });
  await test('negated tone traits are not offered as positive choices', () => {
    assert.deepEqual(parseTone('Not playful; avoid formal. Friendly and clear.'), ['Friendly', 'Clear']);
  });
  await test('unfamiliar style values keep a compact fallback', () => {
    assert.deepEqual(parseTone('Hand drawn illustrations; Textured paper'), ['Hand drawn illustrations', 'Textured paper']);
    assert.ok(parseTone('Use a very distinctive treatment with many elaborate details and decorations.')[0].length <= 39);
  });
  await test('findings group by purpose without turning white space into a colour', () => {
    for (const [topic, value, expected] of [
      ['Brand palette', 'navy and white', 'Colours'], ['Brand detail', 'Use #225577', 'Colours'],
      ['Body typography', 'Inter', 'Fonts'], ['Logo usage', 'Allow clear space', 'Logos'],
      ['Visual style', 'Lots of white space', 'Style & tone'], ['Writing voice', 'Friendly', 'Style & tone'],
      ['Possible audience', 'Creative studios', 'Other'], ['Brand name', 'Northstar', 'Other'],
    ]) assert.equal(findingSection({ topic, value }), expected);
  });
  await test('page grouping preserves empty pages and each photo/work source', () => {
    const page = (path, title) => ({ path, title, kind: 'Other', photoCount: 0, itemCount: 0 });
    const home = page('/', 'Home'), projects = page('/projects', 'Projects'), about = page('/about', 'About');
    const source = { pages: [projects, about, home], photos: [{ id: 'a', sourcePages: [home] }, { id: 'b', sourcePages: [projects] }, { id: 'shared', sourcePages: [home, projects] }] };
    const content = [{ title: 'A project', sourcePages: [projects] }];
    const grouped = groupIdentityPages(source, content);
    assert.deepEqual(grouped.map(group => group.page.path), ['/', '/about', '/projects']);
    assert.deepEqual(grouped[0].photos.map(photo => photo.id), ['a', 'shared']);
    assert.equal(grouped[1].photos.length, 0);
    assert.deepEqual(grouped[2].items.map(item => item.index), [0]);
    assert.deepEqual(selectedPageContent(grouped, ['/projects'], ['a', 'b', 'shared'], [0]), { photos: ['b', 'shared'], items: [0] });
    assert.deepEqual(selectedPageContent(grouped, ['/', '/projects'], ['shared'], []), { photos: ['shared'], items: [] });
  });


} finally { await rm(temporary, { recursive: true, force: true }); }
