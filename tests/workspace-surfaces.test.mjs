import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.workspace-surfaces-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const compiled = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--jsx', 'react-jsx', '--types', 'vite/client', '--skipLibCheck', '--outDir', temporary, 'src/WorkspaceSurface.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(compiled.status, 0, compiled.stderr || compiled.stdout || String(compiled.error));
  const source = (await readFile(join(temporary, 'WorkspaceSurface.js'), 'utf8'))
    .replace(/^import .*;$/gm, '')
    .replace(/^export /gm, '');
  let setups = 0;
  async function setup() {
    // Each setup gets its own escape stack, so surfaces left open by one test cannot answer the next.
    const { onEscape, dismissNewestOverlay } = await import(`${pathToFileURL(join(temporary, 'escape.js')).href}?${++setups}`);
    const effects = [];
    const state = { context: null, active: null, menu: false };
    class Element {
      isConnected = true;
      disabled = false;
      insideSurface = false;
      focus() { state.active = this; }
      matches() { return this.disabled; }
      closest() { return this.insideSurface ? {} : null; }
      contains(element) { return element === this; }
      querySelector() { return null; }
    }
    const hooks = {
      createContext: () => ({ Provider: 'provider' }), useContext: () => state.context,
      useRef: value => ({ current: value }), useCallback: callback => callback,
      useState: initial => [initial, () => {}], useLayoutEffect: callback => effects.push(callback),
    };
    const document = { get activeElement() { return state.active; }, querySelector: () => state.menu ? {} : null };
    const jsx = (type, props) => ({ type, props });
    const components = new Function('hooks', 'document', 'onEscape', 'HTMLElement', 'Icon', '_jsx', '_jsxs', 'createPortal', `const { createContext, useContext, useRef, useState, useLayoutEffect, useCallback } = hooks;\n${source}\nreturn { WorkspaceSurfaces, WorkspaceSurface };`)(hooks, document, onEscape, Element, 'icon', jsx, jsx, (node, target) => ({ node, target }));
    state.context = components.WorkspaceSurfaces({ children: null }).props.value;
    // The editor's key handler: an open menu answers Escape itself, otherwise the newest surface closes.
    const escape = () => ({ stopped: !state.menu && dismissNewestOverlay() });
    return { ...components, state, effects, Element, escape };
  }
  await test('switching side-panel views closes the previous view and restores its persistent trigger', async () => {
    const { state, Element } = await setup();
    const trigger = new Element(), internal = new Element(); internal.insideSurface = true;
    let firstClosed = 0, secondClosed = 0;
    let leaveFirst;
    leaveFirst = state.context.enter({ place: 'panel', opener: trigger, close: () => { firstClosed++; leaveFirst(); } });
    const leaveSecond = state.context.enter({ place: 'panel', opener: internal, close: () => secondClosed++ });
    assert.equal(firstClosed, 1); assert.equal(secondClosed, 0);
    leaveSecond(); await Promise.resolve();
    assert.equal(state.active, trigger);
  });
  await test('save-conflict banners retain the pending save choice and Escape closes only the newest surface', async () => {
    const { state, escape, Element } = await setup();
    const closed = [], trigger = new Element();
    let panel, replacement, conflict;
    panel = state.context.enter({ place: 'panel', opener: trigger, close: () => { closed.push('panel'); panel(); } });
    replacement = state.context.enter({ place: 'banner', opener: trigger, close: () => { closed.push('replacement'); replacement(); } });
    conflict = state.context.enter({ place: 'banner', opener: trigger, close: () => { closed.push('conflict'); conflict(); } });
    assert.deepEqual(closed, []);
    assert.equal(escape().stopped, true); assert.deepEqual(closed, ['conflict']);
    assert.equal(escape().stopped, true); assert.deepEqual(closed, ['conflict', 'replacement']);
    state.menu = true; assert.equal(escape().stopped, false);
    state.menu = false; escape(); assert.deepEqual(closed, ['conflict', 'replacement', 'panel']);
    await Promise.resolve(); assert.equal(state.active, trigger);
  });
  await test('dismissing a save conflict returns focus to the retained save banner', async () => {
    const { state, Element } = await setup();
    const saveButton = new Element(); saveButton.insideSurface = true;
    state.context.enter({ place: 'banner', opener: new Element(), close: () => {} });
    const leave = state.context.enter({ place: 'banner', opener: saveButton, close: () => {} });
    leave(); await Promise.resolve(); assert.equal(state.active, saveButton);
  });
  await test('draft preview and side panel are independent; replaced previews cannot restore focus over the new preview', async () => {
    const { state, Element } = await setup();
    const oldTrigger = new Element(), newTrigger = new Element();
    let panelClosed = false, preview;
    state.context.enter({ place: 'panel', opener: oldTrigger, close: () => { panelClosed = true; } });
    preview = state.context.enter({ place: 'canvas', opener: oldTrigger, close: () => preview() });
    const leave = state.context.enter({ place: 'canvas', opener: newTrigger, close: () => {} });
    await Promise.resolve(); assert.equal(state.active, null); assert.equal(panelClosed, false);
    leave(); await Promise.resolve(); assert.equal(state.active, newTrigger);
  });
  await test('views render as non-modal sections in their designated host, without focus traps or canvas disabling', async () => {
    const { state, WorkspaceSurface, effects, Element } = await setup();
    const trigger = new Element(), field = new Element(), host = new Element();
    state.active = trigger; state.context.hosts.panel = host;
    const result = WorkspaceSurface({ title: 'Grid and rulers', close: () => {}, children: 'form' });
    assert.equal(result.target, host); assert.equal(result.node.type, 'section');
    assert.equal(result.node.props['aria-label'], 'Grid and rulers');
    assert.equal(result.node.props['aria-modal'], undefined); assert.equal(result.node.props.inert, undefined);
    const section = new Element(); section.querySelector = () => field;
    result.node.props.ref.current = section;
    const leave = effects.shift()(); assert.equal(state.active, field);
    leave(); await Promise.resolve(); assert.equal(state.active, trigger);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
