import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { compileModules, root } from './typescript.mjs';

const templates = (() => {
  const run = spawnSync('cargo', ['run', '--quiet', '-p', 'desegnos-core', '--example', 'template_projects'], {
    cwd: root, encoding: 'utf8', timeout: 180_000,
  });
  assert.equal(run.status, 0, run.stderr || run.stdout || String(run.error));
  return JSON.parse(run.stdout);
})();

const modules = await compileModules(['src/App.tsx']);
try {
  await writeFile(join(modules.out, 'tauri-core-stub.mjs'), 'export const isTauri = () => false;\nexport const invoke = async () => { throw new Error("Not running under Tauri."); };\n');
  await writeFile(join(modules.out, 'tauri-window-stub.mjs'), 'export const getCurrentWindow = () => ({ onCloseRequested: async () => () => {}, destroy: async () => {} });\n');
  await writeFile(join(modules.out, 'asset-stub.mjs'), 'export default "stub.svg";\n');
  const core = pathToFileURL(join(modules.out, 'tauri-core-stub.mjs')).href;
  const windowStub = pathToFileURL(join(modules.out, 'tauri-window-stub.mjs')).href;
  const asset = pathToFileURL(join(modules.out, 'asset-stub.mjs')).href;
  async function rewrite(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) { await rewrite(path); continue; }
      if (!entry.name.endsWith('.js')) continue;
      const source = await readFile(path, 'utf8');
      await writeFile(path, source
        .replaceAll("'@tauri-apps/api/core'", `'${core}'`)
        .replaceAll('"@tauri-apps/api/core"', `"${core}"`)
        .replaceAll("'@tauri-apps/api/window'", `'${windowStub}'`)
        .replaceAll('"@tauri-apps/api/window"', `"${windowStub}"`)
        .replace(/(from\s+['"])[^'"]+\.svg(?:\.js)?(['"])/g, (_, before, after) => `${before}${asset}${after}`));
    }
  }
  await rewrite(modules.out);
  const { default: App } = await modules.load('App.js');

  const { ExportPanel } = await modules.load('projectCanvas/exportPanel.js');
  const { BoundaryProperties } = await modules.load('dock/properties.js');
  const noop = () => {};
  const { pageKind, projectKind, boundaryDefaults, layoutKind, outputKind } = await modules.load('projectCanvas/boundaryDefaults.js');
  await test('Page creation keeps website and app template sizes and height modes', () => {
    for (const [template, width, height, mode] of [['website', 1440, 900, 'page'], ['appScreen', 390, 844, 'screen']]) {
      const { project } = templates.find(item => item.template === template);
      assert.equal(projectKind(project), 'page');
      assert.equal(pageKind(project.pages[0]), 'page');
      assert.equal(project.pages[0].output, 'webpage');
      assert.equal(project.pages[0].name, 'Page 1');
      for (const current of [project, { ...project, pages: [] }]) {
        const defaults = boundaryDefaults(current, 'page');
        assert.equal(defaults.width, width);
        assert.equal(defaults.height, height);
        assert.equal(layoutKind('page', current), mode);
      }
    }
    assert.equal(outputKind('page'), 'webpage');
  });
  await test('app screens appear as Pages with fixed height and web export', () => {
    const { project } = templates.find(item => item.template === 'appScreen');
    const page = { ...project.pages[0], name: 'Existing screen' };
    assert.equal(pageKind(page), 'page');
    const properties = renderToStaticMarkup(React.createElement(BoundaryProperties, {
      page, busy: false, commit: noop, updateSize: noop, addSize: noop, removeSize: noop, remove: noop,
    }));
    assert.match(properties, /<strong>Page<\/strong>/);
    assert.match(properties, /aria-label="Height behaviour of Phone"/);
    assert.match(properties, /<option value="screen" selected="">Fixed<\/option>/);
    for (const label of ['Phone', 'Tablet', 'Desktop']) assert.match(properties, new RegExp(`>${label}</button>`));
    assert.doesNotMatch(properties, />Screen<\/option>|Delete screen/);
    const html = renderToStaticMarkup(React.createElement(ExportPanel, {
      project: { ...project, pages: [page] }, busy: false, exports: { destination: null, job: null, sending: false, error: '' },
      selectBoundary: noop, setIncluded: noop, setOutput: noop, close: noop,
    }));
    assert.match(html, /<option value="webpage" selected="">Page<\/option>/);
    assert.doesNotMatch(html, /<option value="appScreen"/);
    assert.match(html, />Preview<\/button>/);
    assert.equal(page.output, 'webpage');
  });
  await test('Export lists included and excluded boundaries with accessible controls above the folder', () => {
    const { project } = templates.find(item => item.template === 'website');
    const pages = [
      { ...project.pages[0], name: 'Home', included: true },
      { ...project.pages[0], id: 'excluded', name: 'Draft', included: false, output: 'image' },
    ];
    const html = renderToStaticMarkup(React.createElement(ExportPanel, {
      project: { ...project, pages }, busy: false, exports: { destination: null, job: null, sending: false, error: '' },
      selectBoundary: noop, setIncluded: noop, setOutput: noop, close: noop,
    }));
    assert.match(html, /aria-label="Include Home in export" checked=""/);
    assert.match(html, /aria-label="Include Draft in export"/);
    assert.match(html, /aria-label="Export Draft as"/);
    assert.equal((html.match(/>Preview<\/button>/g) ?? []).length, 1);
    assert.ok(html.indexOf('Export Draft as') < html.indexOf('Folder'));
    assert.match(html, /<button\b[^>]*>Draft<\/button>/);
  });
  await test('Boundary properties reflects the current background', () => {
    const { project } = templates.find(item => item.template === 'website');
    const html = renderToStaticMarkup(React.createElement(BoundaryProperties, {
      page: { ...project.pages[0], background: '#121212' }, busy: false,
      commit: noop, updateSize: noop, addSize: noop, removeSize: noop, remove: noop,
    }));
    assert.match(html, /aria-label="Background"[^>]+value="#121212"/);
  });

  const view = project => ({
    project, path: null, dirty: false, untitled: true, canUndo: false, canRedo: false,
    history: null, recovery: null, recoveries: [], recoveryError: null,
    openProjects: [], anyDirty: false, pointerStates: [],
  });

  for (const { template, project } of templates) {
    await test(`the editor shell renders the ${template} template without throwing`, () => {
      const html = renderToStaticMarkup(React.createElement(App, { initialView: view(project) }));
      assert.match(html, /id="project-canvas"/);
      assert.match(html, /aria-label="Panels"/);
      assert.match(html, /aria-label="Properties"/);
      assert.match(html, /aria-label="Layers"/);
      assert.match(html, /aria-label="Canvas tools"/);
      assert.doesNotMatch(html, /stopped working/);
    });
  }

  const buttons = html => [...html.matchAll(/<button\b([^>]*)>([\s\S]*?)<\/button>/g)]
    .map(([, attributes, content]) => ({ attributes, text: content.replace(/<[^>]*>/g, '') }));
  const namedButton = (html, name) => {
    const found = buttons(html).filter(button => button.attributes.includes(`aria-label="${name}"`) || button.text === name);
    assert.equal(found.length, 1, `${name} is offered once`);
    return found[0];
  };
  await test('Undo and Redo stay visible and reflect available history', () => {
    const { project } = templates.find(item => item.template === 'website');
    for (const [canUndo, canRedo] of [[false, false], [true, false], [false, true]]) {
      const html = renderToStaticMarkup(React.createElement(App, { initialView: { ...view(project), canUndo, canRedo } }));
      assert.equal(/\bdisabled(?:=|\s|$)/.test(namedButton(html, 'Undo last change').attributes), !canUndo);
      assert.equal(/\bdisabled(?:=|\s|$)/.test(namedButton(html, 'Redo last change').attributes), !canRedo);
    }
  });
  await test('the app bar offers import and export, and preview only for web output', () => {
    const { project } = templates.find(item => item.template === 'website');
    for (const output of ['webpage', 'image', 'slide']) {
      const pages = project.pages.map(page => ({ ...page, output }));
      const html = renderToStaticMarkup(React.createElement(App, { initialView: view({ ...project, pages }) }));
      namedButton(html, 'Import');
      namedButton(html, 'Export');
      assert.equal(buttons(html).some(button => button.attributes.includes('aria-label="Preview export"')), output === 'webpage');
    }
  });

  await test('the editor shell renders a project with a pointer that has no wiring yet', () => {
    const { project } = templates.find(item => item.template === 'website');
    project.pointers.push({
      id: 'pointer-1',
      from: { kind: 'boundary', pageId: project.homePageId, sizeId: null },
      to: { kind: 'reference', itemId: project.reference[0].id },
      comment: 'Explain this note',
      wiring: null,
      playComment: '',
    });
    const html = renderToStaticMarkup(React.createElement(App, { initialView: view(project) }));
    assert.match(html, /id="project-canvas"/);
    assert.doesNotMatch(html, /stopped working/);
  });
} finally {
  await modules.cleanup();
}
