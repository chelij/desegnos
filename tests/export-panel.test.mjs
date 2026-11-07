import assert from 'node:assert/strict';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/projectCanvas/exportPanel.tsx']);
try {
  const { ExportPanel } = await modules.load('projectCanvas/exportPanel.js');
  const noop = () => {};
  const action = async () => {};
  const render = (patch) => renderToStaticMarkup(React.createElement(ExportPanel, {
    project: { pages: [] }, busy: false, selectBoundary: noop, setIncluded: noop, setOutput: noop,
    close: noop, openSettings: noop,
    exports: {
      destination: '/Documents/Desegnos', sending: false, error: '', setDestination: noop, openFolder: noop,
      chooseDestination: action, run: action, send: action, cancel: action, openInBrowser: action,
      job: { status: 'ready', kind: 'code', message: 'Exported 3 boundaries.', summary: '',
        pages: Array.from({ length: 3 }, (_, i) => ({ pageId: String(i), name: `Page ${i}`, file: `${i}.html` })),
        sizeCount: 9, pointerCount: 3, reusedBehaviourCount: 3, warnings: [], attempts: [], ...patch },
    },
  }));

  await test('export counts behaviour code reused after Preview', () => {
    const html = render({});
    assert.match(html, /Export 3 boundaries · 9 sizes · 3 behaviours · 3 behaviours reused/);
    assert.doesNotMatch(html, /0 pointers|Rendered directly from the design/);
  });
  await test('mixed and singular reuse stays explicit', () => {
    assert.match(render({ pointerCount: 4, reusedBehaviourCount: 1 }), /4 behaviours · 1 behaviour reused/);
    assert.match(render({ pointerCount: 1, reusedBehaviourCount: 0 }), /1 behaviour<\/p>/);
    assert.doesNotMatch(render({ pointerCount: 0, reusedBehaviourCount: 0 }), /behaviours reused/);
  });
} finally {
  await modules.cleanup();
}
