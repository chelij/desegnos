import { runInBrowser } from './browserFixture.mjs';

// Real WorkspaceSurface components in a real DOM: focus return, the shared side panel and Escape order.
const script = `
import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { flushSync } from 'react-dom';
import { WorkspaceSurface, WorkspaceSurfaces, SurfaceHost } from '/src/WorkspaceSurface.tsx';
import { dismissNewestOverlay } from '/src/escape.ts';

const check = (ok, message) => { if (!ok) throw Error(message); };
const settle = () => new Promise(resolve => setTimeout(resolve, 30));
const closed = [];
let setItems;

function Harness() {
  const [items, set] = useState([]);
  setItems = set;
  return <WorkspaceSurfaces>
    <button id="trigger-a">A</button><button id="trigger-b">B</button>
    <SurfaceHost place="panel" /><SurfaceHost place="banner" /><SurfaceHost place="canvas" />
    {items.map(item => <WorkspaceSurface key={item.id} title={item.id} place={item.place} close={item.close}><input aria-label={item.id + ' field'} /></WorkspaceSurface>)}
  </WorkspaceSurfaces>;
}

const field = id => document.querySelector('[aria-label="' + id + ' field"]');
const section = id => document.querySelector('section[aria-label="' + id + '"]');
const trigger = id => document.getElementById('trigger-' + id);
function open(id, place, from) {
  from.focus();
  const close = () => { closed.push(id); setItems(items => items.filter(item => item.id !== id)); };
  flushSync(() => setItems(items => [...items, { id, place, close }]));
}

const tests = {
  async 'switching side-panel views closes the previous view and restores its persistent trigger'() {
    open('first', 'panel', trigger('a'));
    check(section('first').contains(document.activeElement), 'focus enters the view');
    open('second', 'panel', field('first'));
    await settle();
    check(closed.join() === 'first' && !section('first') && section('second'), 'the second view replaces the first');
    dismissNewestOverlay(); await settle();
    check(document.activeElement === trigger('a'), 'focus returns to the first trigger, not to the closed view');
  },
  async 'Escape closes only the newest surface and focus returns to the trigger'() {
    open('panel', 'panel', trigger('a'));
    open('replacement', 'banner', trigger('a'));
    open('conflict', 'banner', trigger('a'));
    check(closed.length === 0, 'opening banners does not close the panel');
    for (const expected of ['conflict', 'replacement', 'panel']) {
      check(dismissNewestOverlay() === true, 'Escape finds a surface');
      check(closed.at(-1) === expected, 'Escape closed ' + closed.at(-1) + ', expected ' + expected);
      await settle();
    }
    check(dismissNewestOverlay() === false, 'nothing left to dismiss');
    check(document.activeElement === trigger('a'), 'focus returns to the trigger');
  },
  async 'dismissing a banner returns focus to the control that opened it'() {
    open('other', 'banner', trigger('b'));
    open('save', 'banner', trigger('a'));
    dismissNewestOverlay(); await settle();
    check(document.activeElement === trigger('a'), 'focus is on the save control');
  },
  async 'draft preview and side panel are independent; a replaced preview cannot take focus back'() {
    open('panel', 'panel', trigger('a'));
    open('old preview', 'canvas', trigger('a'));
    open('new preview', 'canvas', trigger('b'));
    await settle();
    check(closed.join() === 'old preview', 'only the replaced preview closed');
    check(section('panel') && section('new preview') && !section('old preview'), 'panel and new preview stay open');
    check(section('new preview').contains(document.activeElement), 'focus stays in the new preview');
    dismissNewestOverlay(); await settle();
    check(document.activeElement === trigger('b'), 'closing the new preview restores its own trigger');
  },
  async 'views render as non-modal sections in their host without focus traps or canvas disabling'() {
    open('Grid and rulers', 'panel', trigger('a'));
    const view = section('Grid and rulers');
    check(view.tagName === 'SECTION' && view.parentElement.classList.contains('surface-host-panel'), 'rendered in the panel host');
    check(!view.hasAttribute('aria-modal') && !view.hasAttribute('inert'), 'not modal');
    check(view.contains(document.activeElement), 'focus moves into the view');
    dismissNewestOverlay(); await settle();
    check(document.activeElement === trigger('a'), 'focus returns to the trigger');
  },
};

try {
  for (const [name, run] of Object.entries(tests)) {
    closed.length = 0;
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    flushSync(() => root.render(<Harness />));
    await settle();
    try { await run(); } catch (error) { throw Error(name + ': ' + error.message); }
    root.unmount(); await settle(); container.remove();
  }
  document.querySelector('#result').textContent = 'PASS: ' + Object.keys(tests).length + ' workspace surface behaviours';
} catch (error) { document.querySelector('#result').textContent = 'FAIL: ' + error.message; }
`;
console.log(await runInBrowser({ name: 'workspace-surfaces', script }));
