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

  const view = project => ({
    project, path: null, dirty: false, untitled: true, canUndo: false, canRedo: false,
    history: null, recovery: null, recoveries: [], recoveryError: null,
    openProjects: [], anyDirty: false, documentsKey: '', pointerStates: [],
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
