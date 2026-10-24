import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.element-selection-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/elementSelection.ts', 'src/elementTargets.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { MAX_SELECTION, marqueeSelections, rectsIntersect, sameProfile, similarSelections, topmostSelections } = await import(pathToFileURL(join(temporary, 'elementSelection.js')).href);

  const target = (id, selector, tag = 'div', document) => ({ id, selector, tag, label: '', ...(document ? { document } : {}) });
  const item = (id, selector, rect, tag = 'div', document) => ({ element: target(id, selector, tag, document), rect });
  const rect = (x, y, width = 50, height = 20) => ({ x, y, width, height });
  const frame = { kind: 'siteFile', path: 'play/game.html' };

  await test('rectangles only match with a real overlapping area', () => {
    assert.ok(rectsIntersect(rect(0, 0, 100, 100), rect(90, 90, 100, 100)));
    assert.ok(!rectsIntersect(rect(0, 0, 100, 100), rect(100, 0, 100, 100)), 'touching edges do not select');
    assert.ok(!rectsIntersect(rect(0, 0, 100, 100), rect(0, 100, 100, 100)));
    assert.ok(!rectsIntersect(rect(0, 0, 100, 100), rect(-200, 0, 10, 10)));
  });

  await test('marquee selects intersecting elements and keeps the outermost match', () => {
    const card = item('card', 'body > section:nth-of-type(1)', rect(10, 10, 300, 200));
    const heading = item('heading', 'body > section:nth-of-type(1) > h2:nth-of-type(1)', rect(20, 20, 120, 30));
    const aside = item('aside', 'body > aside:nth-of-type(1)', rect(400, 400, 100, 100));
    const body = item('body', 'body', rect(0, 0, 1200, 900), 'body');
    const selected = marqueeSelections([card, heading, aside, body], rect(0, 0, 200, 200));
    assert.deepEqual(selected.map(value => value.element.id), ['card']);
    assert.deepEqual(marqueeSelections([card, heading, aside, body], rect(390, 390, 200, 200)).map(value => value.element.id), ['aside']);
    assert.deepEqual(marqueeSelections([card, heading, aside, body], rect(500, 500, 10, 10)), [], 'empty space selects nothing');
  });

  await test('marquee adds to the existing selection and never repeats a target', () => {
    const first = item('a', 'body > p:nth-of-type(1)', rect(0, 0, 40, 20), 'p');
    const second = item('b', 'body > p:nth-of-type(2)', rect(80, 0, 40, 20), 'p');
    assert.deepEqual(marqueeSelections([first, second], rect(0, 0, 100, 20)).map(value => value.element.id), ['a', 'b']);
    assert.deepEqual(marqueeSelections([first, second], rect(0, 0, 40, 20), [first]).map(value => value.element.id), ['a'], 'a target already selected is not duplicated');
    assert.deepEqual(marqueeSelections([first, second], rect(80, 0, 40, 20), [first]).map(value => value.element.id), ['a', 'b']);
  });

  await test('nested wrappers stay out of the selection, including inside framed documents', () => {
    const outer = item('outer', 'body > div:nth-of-type(1)', rect(0, 0, 200, 200), 'div', frame);
    const inner = item('inner', 'body > div:nth-of-type(1) > p:nth-of-type(1)', rect(10, 10, 50, 20), 'p', frame);
    const pagePeer = item('page', 'body > div:nth-of-type(1)', rect(300, 0, 50, 50));
    const selected = topmostSelections([inner, outer, pagePeer]);
    assert.deepEqual(selected.map(value => value.element.id), ['outer', 'page'], 'equal selectors in different documents stay separate');
  });

  await test('marquee stops at the technical selection limit', () => {
    const items = Array.from({ length: MAX_SELECTION + 20 }, (_, index) => item(`i${index}`, `body > p:nth-of-type(${index + 1})`, rect(index * 40, 0, 30, 20), 'p'));
    assert.equal(marqueeSelections(items, rect(0, 0, 40000, 20)).length, MAX_SELECTION);
  });

  await test('select similar matches tag and class list, or the same structural role', () => {
    const profile = (tag, classes = [], role = '') => ({ tag, classes, role });
    const reference = profile('a', ['cta', 'primary'], 'link');
    const items = [
      { selection: item('a', 'body > a:nth-of-type(1)', rect(0, 0), 'a'), profile: reference },
      { selection: item('b', 'body > a:nth-of-type(2)', rect(0, 30), 'a'), profile: profile('a', ['primary', 'cta'], 'link') },
      { selection: item('c', 'body > a:nth-of-type(3)', rect(0, 60), 'a'), profile: profile('a', ['cta'], 'link') },
      { selection: item('d', 'body > a:nth-of-type(4)', rect(0, 90), 'a'), profile: profile('a', ['cta', 'primary'], '') },
      { selection: item('e', 'body > p:nth-of-type(1)', rect(0, 120), 'p'), profile: profile('p', ['cta', 'primary'], '') },
    ];
    assert.ok(sameProfile(reference, profile('a', ['cta', 'primary'], '')));
    assert.ok(sameProfile(reference, profile('a', ['cta'], 'link')), 'the same structural role is enough');
    assert.ok(!sameProfile(reference, profile('button', ['cta', 'primary'], '')), 'another tag and role is not similar');
    assert.deepEqual(similarSelections(reference, items, 10).map(value => value.element.id), ['a', 'b', 'c', 'd']);
    const roleReference = profile('button', [], 'button');
    const buttons = [
      { selection: item('x', 'body > button:nth-of-type(1)', rect(0, 0), 'button'), profile: roleReference },
      { selection: item('y', 'body > button:nth-of-type(2)', rect(0, 40), 'button'), profile: profile('button', ['big'], 'button') },
      { selection: item('z', 'body > input:nth-of-type(1)', rect(0, 80), 'input'), profile: profile('input', [], 'field') },
    ];
    assert.deepEqual(similarSelections(roleReference, buttons, 10).map(value => value.element.id), ['x', 'y']);
  });

  await test('select similar drops nested peers and bounded results', () => {
    const reference = { tag: 'div', classes: ['card'], role: '' };
    const outer = { selection: item('outer', 'body > div:nth-of-type(1)', rect(0, 0), 'div'), profile: reference };
    const inner = { selection: item('inner', 'body > div:nth-of-type(1) > div:nth-of-type(1)', rect(4, 4), 'div'), profile: reference };
    assert.deepEqual(similarSelections(reference, [inner, outer]).map(value => value.element.id), ['outer']);
    const many = Array.from({ length: MAX_SELECTION + 5 }, (_, index) => ({ selection: item(`m${index}`, `body > div:nth-of-type(${index + 1})`, rect(0, index * 40), 'div'), profile: reference }));
    assert.equal(similarSelections(reference, many).length, MAX_SELECTION);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
