import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readdir, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
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
  function setup() {
    const effects = [], listeners = new Map();
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
    const window = { addEventListener: (name, callback) => listeners.set(name, callback), removeEventListener: name => listeners.delete(name) };
    const jsx = (type, props) => ({ type, props });
    const components = new Function('hooks', 'document', 'window', 'HTMLElement', 'Icon', '_jsx', '_jsxs', 'createPortal', `const { createContext, useContext, useRef, useState, useLayoutEffect, useCallback } = hooks;\n${source}\nreturn { WorkspaceSurfaces, WorkspaceSurface };`)(hooks, document, window, Element, 'icon', jsx, jsx, (node, target) => ({ node, target }));
    state.context = components.WorkspaceSurfaces({ children: null }).props.value;
    const stop = effects.shift()();
    const escape = () => {
      const event = { key: 'Escape', defaultPrevented: false, stopped: false, preventDefault() { this.defaultPrevented = true; }, stopImmediatePropagation() { this.stopped = true; } };
      listeners.get('keydown')(event); return event;
    };
    return { ...components, state, effects, Element, escape, stop };
  }
  await test('editor components do not create modal dialogs or popup windows', async () => {
    for (const name of (await readdir(join(root, 'src'))).filter(name => name.endsWith('.tsx'))) {
      const source = await readFile(join(root, 'src', name), 'utf8');
      assert.doesNotMatch(source, /<Modal\b|<dialog\b|\.showModal\s*\(|window\.open\s*\(|\bconfirm\s*\(/, name);
    }
  });
  await test('switching side-panel views closes the previous view and restores its persistent trigger', async () => {
    const { state, Element } = setup();
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
    const { state, escape, Element } = setup();
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
    const { state, Element } = setup();
    const saveButton = new Element(); saveButton.insideSurface = true;
    state.context.enter({ place: 'banner', opener: new Element(), close: () => {} });
    const leave = state.context.enter({ place: 'banner', opener: saveButton, close: () => {} });
    leave(); await Promise.resolve(); assert.equal(state.active, saveButton);
  });
  await test('draft preview and side panel are independent; replaced previews cannot restore focus over the new preview', async () => {
    const { state, Element } = setup();
    const oldTrigger = new Element(), newTrigger = new Element();
    let panelClosed = false, preview;
    state.context.enter({ place: 'panel', opener: oldTrigger, close: () => { panelClosed = true; } });
    preview = state.context.enter({ place: 'canvas', opener: oldTrigger, close: () => preview() });
    const leave = state.context.enter({ place: 'canvas', opener: newTrigger, close: () => {} });
    await Promise.resolve(); assert.equal(state.active, null); assert.equal(panelClosed, false);
    leave(); await Promise.resolve(); assert.equal(state.active, newTrigger);
  });
  await test('views render as non-modal sections in their designated host, without focus traps or canvas disabling', async () => {
    const { state, WorkspaceSurface, effects, Element } = setup();
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
