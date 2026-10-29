import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/projectCanvas/play.ts']);
try {
  const { idlePlay, playClick } = await modules.load('projectCanvas/play.js');

  const element = (id, patch = {}) => ({
    id, kind: 'box', name: null, link: null, rect: { x: 0, y: 0, width: 100, height: 40 }, rotation: 0,
    visible: true, locked: false, style: {}, text: '', image: null, url: null,
    layout: { kind: 'free' }, children: [], meta: { requests: [], context: [] }, ...patch,
  });
  const size = id => ({ id: `${id}-base`, label: 'Desktop', width: 800, height: 600, kind: 'page', x: 0, y: 0 });
  const page = (id, path, elements) => ({
    id, name: id, path, included: true, output: 'webpage', sizes: [size(id)], background: '#ffffff', elements,
  });
  const pointer = (id, pageId, elementId, play) => ({
    id, kind: 'change', from: { kind: 'element', pageId, elementId }, to: { kind: 'boundary', pageId },
    comment: '', wiring: null, play,
  });
  const project = (pages, pointers = []) => ({
    schemaVersion: 14, id: 'project', name: 'Play', revision: 1, viewport: { width: 800, height: 600 },
    appearance: 'light', showGrid: false, gridSettings: { spacing: 8, unit: 'pixels', snap: true, rulers: true },
    generationMode: 'fuzzy', homePageId: pages[0].id, pages, reference: [], pointers, linked: {},
    views: [], brief: { purpose: '', audience: '', brandName: '', tone: '' }, decisions: [], exceptions: [],
    appliedJobIds: [], extensions: {},
  });
  const play = { ...idlePlay, boundary: 'home' };

  await test('a layer whose url names a page path goes to that page', () => {
    const link = element('link', { kind: 'link', url: '/about' });
    const design = project([page('home', '/', [link]), page('about', '/about', [])]);
    const step = playClick(design, play, 'home', 'link');
    assert.equal(step.goTo, 'about');
    assert.equal(step.play.boundary, 'about');
  });

  await test('a play pointer wins over a page link on the same layer', () => {
    const link = element('link', { kind: 'button', url: '/about' });
    const design = project(
      [page('home', '/', [link]), page('about', '/about', []), page('settings', '/settings', [])],
      [pointer('go', 'home', 'link', { kind: 'goTo', boundary: 'settings' })],
    );
    assert.equal(playClick(design, play, 'home', 'link').goTo, 'settings');
  });

  await test('a play pointer on an ancestor beats a link on its child', () => {
    const link = element('link', { kind: 'text', url: '/about' });
    const box = element('box', { children: [link] });
    const design = project(
      [page('home', '/', [box]), page('about', '/about', []), page('settings', '/settings', [])],
      [pointer('go', 'home', 'box', { kind: 'goTo', boundary: 'settings' })],
    );
    assert.equal(playClick(design, play, 'home', 'link').goTo, 'settings');
  });

  await test('a child follows the nearest linked ancestor', () => {
    const deep = element('deep', { kind: 'text' });
    const inner = element('inner', { kind: 'box', url: '/three', children: [deep] });
    const outer = element('outer', { kind: 'box', url: '/two', children: [inner] });
    const design = project([
      page('home', '/', [outer]), page('two', '/two', []), page('three', '/three', []),
    ]);
    assert.equal(playClick(design, play, 'home', 'deep').goTo, 'three');
    assert.equal(playClick(design, play, 'home', 'outer').goTo, 'two');
  });

  await test('external and near-matching addresses do nothing in Play', () => {
    const external = element('external', { kind: 'link', url: 'https://example.com/about' });
    const near = element('near', { kind: 'link', url: '/about/' });
    const query = element('query', { kind: 'link', url: '/about?from=home' });
    const image = element('image', { kind: 'image', url: '/about' });
    const design = project([
      page('home', '/', [external, near, query, image]), page('about', '/about', []),
    ]);
    for (const id of ['external', 'near', 'query', 'image']) {
      const step = playClick(design, play, 'home', id);
      assert.equal(step.goTo, null, id);
      assert.equal(step.play, play, id);
    }
  });

  await test('following a link keeps the session visibility and swaps', () => {
    const link = element('link', { kind: 'box', url: '/about' });
    const design = project([page('home', '/', [link]), page('about', '/about', [])]);
    const session = { ...play, hidden: { menu: true }, swaps: { slot: 'other' } };
    const step = playClick(design, session, 'home', 'link');
    assert.deepEqual(step.play, { ...session, boundary: 'about' });
  });
} finally {
  await modules.cleanup();
}
