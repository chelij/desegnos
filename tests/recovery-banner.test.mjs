import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.recovery-banner-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const compiled = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--jsx', 'react-jsx', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--types', 'vite/client', '--outDir', temporary, 'src/RecoveryBanner.tsx', 'src/App.tsx', 'src/NewProject.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(compiled.status, 0, compiled.stderr || compiled.stdout || String(compiled.error));
  // Node ESM needs extensions; preserve the app's bundler-style source imports and stub stylesheets.
  await writeFile(join(temporary, 'style-stub.js'), 'export {};\n');
  for (const [file, replacements] of [
    ['NewProject.js', [["'./api'", "'./api.js'"], ["'./ProjectLocation'", "'./ProjectLocation.js'"], ["'./newProject.css'", "'./style-stub.js'"]]],
    ['api.js', [["'./usage'", "'./usage.js'"]]],
  ]) {
    const path = join(temporary, file);
    const source = await readFile(path, 'utf8');
    await writeFile(path, replacements.reduce((text, [from, to]) => text.replaceAll(from, to), source));
  }
  const { RecoveryChoice, isPendingRecovery } = await import(pathToFileURL(join(temporary, 'RecoveryBanner.js')).href);
  const { NewProject } = await import(pathToFileURL(join(temporary, 'NewProject.js')).href);
  const pending = { code: 'pendingRecovery', projectId: 'project-id', path: '/chosen/site.desegnos' };
  const choices = (overrides = {}) => ({ path: pending.path, busy: false, recover() {}, openSaved() {}, dismiss() {}, ...overrides });

  await test('recovery errors are identified by code and typed fields, never by prose', () => {
    assert.equal(isPendingRecovery(pending), true);
    for (const error of [null, pending.code, new Error('This project has unsaved changes from an earlier session.'), { ...pending, code: 'other' }, { ...pending, path: null }, { ...pending, projectId: 3 }, { missing: true, message: 'Missing project' }]) {
      assert.equal(isPendingRecovery(error), false);
    }
  });
  await test('the choice has only the sentence, two actions, and dismiss', () => {
    const html = renderToStaticMarkup(React.createElement(RecoveryChoice, choices()));
    assert.equal(html.replace(/<[^>]+>/g, ''), 'Unsaved changes from an earlier sessionRecoverOpen saved version×');
    assert.match(html, /class="recovery-choice recovery-choice-row" role="alert"/);
    assert.match(html, /class="primary">Recover</);
    assert.match(html, />Open saved version</);
    assert.match(html, /aria-label="Dismiss recovery choice"/);
    assert.doesNotMatch(html, /role="dialog"|<dialog|modal|helper/i);
    const busy = renderToStaticMarkup(React.createElement(RecoveryChoice, choices({ busy: true })));
    assert.equal((busy.match(/<button[^>]*disabled=""/g) ?? []).length, 2, 'both actions wait while busy');
  });

  await test('the choice sits on the matching recent project card and other cards stay normal', async () => {
    const recents = [{ path: '/other/one.desegnos', name: 'One' }, { path: pending.path, name: 'Site' }, { path: '/other/two.desegnos', name: 'Two' }];
    const html = renderToStaticMarkup(React.createElement(NewProject, {
      importBrand() {}, busy: false, async create() {}, async open() {}, async openFolder() {}, location: null, async changeLocation() {},
      recentProjects: recents, async openRecent() {}, async forgetRecent() {}, settings: null, openSettings() {}, recovery: choices(),
    }));
    assert.equal((html.match(/recovery-choice-card/g) ?? []).length, 1, 'only the clicked card changes');
    assert.doesNotMatch(html, /recovery-choice-row|Remove Site from recent projects/);
    const card = html.indexOf('recovery-choice-card');
    assert.ok(card > html.indexOf('<ul'), 'the choice is inside the card grid');
    assert.ok(card > html.indexOf('title="/other/one.desegnos"') && card < html.indexOf('title="/other/two.desegnos"'), 'the choice is on the clicked card, in place');
    assert.equal((html.match(/class="recent-open"/g) ?? []).length, 2, 'the other cards are unchanged');
    assert.match(html, />Unsaved changes from an earlier session</);
    assert.match(html, />Open saved version</);
    const css = await readFile(join(root, 'src/newProject.css'), 'utf8');
    assert.match(css, /\.recent-projects ul \{[^}]*grid-auto-rows: 62px/, 'grid rows keep one height');
    assert.match(css, /\.recovery-choice-card \{[^}]*height: 100%/, 'the recovery card fills its row without resizing it');
    assert.match(css, /background: var\(--notice-warning-bg\)/, 'warning colours come from the notice tokens');
  });

  await test('a project outside the recent list puts the choice in the first row above the cards', () => {
    const html = renderToStaticMarkup(React.createElement(NewProject, {
      importBrand() {}, busy: false, async create() {}, async open() {}, async openFolder() {}, location: null, async changeLocation() {},
      recentProjects: [{ path: '/other/one.desegnos', name: 'One' }], async openRecent() {}, async forgetRecent() {}, settings: null, openSettings() {},
      recovery: choices({ path: '/elsewhere/folder', busy: true }),
    }));
    assert.equal((html.match(/recovery-choice-row/g) ?? []).length, 1);
    assert.doesNotMatch(html, /recovery-choice-card/);
    const row = html.indexOf('recovery-choice-row');
    assert.ok(row > html.indexOf('Recent projects') && row < html.indexOf('<ul'), 'the row precedes the cards');
    assert.equal((html.match(/class="recent-open"/g) ?? []).length, 1, 'the cards stay normal');
    assert.equal((html.match(/<button[^>]*disabled=""/g) ?? []).length, 2, 'busy disables the two actions');
  });

  // Exercise App's actual handlers with a native/queue boundary stub, without a GUI.
  const app = await readFile(join(temporary, 'App.js'), 'utf8');
  function handler(name, environment) {
    const start = app.indexOf(`    async function ${name}(`);
    assert.ok(start >= 0, `App handler ${name} exists`);
    const end = app.indexOf('\n    }', start) + '\n    }'.length;
    const source = app.slice(start, end);
    return new Function(...Object.keys(environment), `${source}; return ${name};`)(...Object.values(environment));
  }
  function setup() {
    const state = { pending: null, error: '', calls: [], accepted: [], showNew: false };
    const env = {
      isPendingRecovery,
      current: { current: null },
      setPendingRecovery: value => { state.pending = value; }, setError: value => { state.error = value; },
      openAlongside: operation => operation(false), synchronize: async () => {}, settle: async () => {},
      queue: operation => operation(),
      native: async (command, args) => { state.calls.push({ command, args }); if (state.failure) throw state.failure; if (!state.result) throw pending; return state.result; },
      accept: workspace => state.accepted.push(workspace), setSelected() {}, setNotice() {},
      setShowNew: value => { state.showNew = value; }, setShowRecovery() {},
      setImportFolder() {}, setImportAsNew() {}, setShowImport() {},
      setZoom() {}, initialZoom: () => 1, refreshRecent: async () => {},
    };
    env.requestProjectOpen = handler('requestProjectOpen', env);
    return { state, env, open: handler('open', env), openRecent: handler('openRecent', env), editWebsite: handler('editWebsite', env), recover: handler('recover', env) };
  }
  for (const source of ['dialog', 'recent', 'website']) {
    await test(`${source} opening offers recovery inline and retries the exact saved file`, async () => {
      const { state, open, openRecent, editWebsite } = setup();
      await (source === 'dialog' ? open() : source === 'recent' ? openRecent(pending.path) : editWebsite('folder-id'));
      assert.deepEqual(state.pending, pending);
      assert.equal(state.error, '');
      assert.deepEqual(state.accepted, []);
      assert.equal(state.showNew, false, 'no panel is forced while the start screen is visible');
      state.result = { project: { id: pending.projectId, viewport: { width: 100, height: 100 } } };
      await open(false, state.pending.path, true);
      assert.deepEqual(state.calls.at(-1), { command: 'open_project', args: { discard: false, folder: false, path: pending.path, discardRecovery: true } });
      assert.equal(state.pending, null);
      assert.deepEqual(state.accepted, [state.result]);
      assert.equal(state.showNew, false);
    });
  }
  await test('an already open project brings up the start/open panel for the choice', async () => {
    const { state, env, open } = setup();
    state.showNew = false; env.current.current = { project: { id: 'open-id' } };
    await open();
    assert.deepEqual(state.pending, pending);
    assert.equal(state.showNew, true);
  });
  await test('App keeps the choice in the start/open flow, never in the global top bar', async () => {
    const source = await readFile(join(root, 'src/App.tsx'), 'utf8');
    assert.doesNotMatch(source, /<RecoveryBanner/);
    assert.equal((source.match(/recovery=\{recovery\}/g) ?? []).length, 2, 'the start screen and the New project panel both receive it');
  });
  await test('unrelated native errors retain their existing reporting path', async () => {
    const { state, openRecent } = setup();
    state.failure = { missing: true, message: 'Could not open this recent project' };
    await assert.rejects(() => openRecent(pending.path), error => error === state.failure);
    assert.equal(state.pending, null);
  });
  await test('Recover uses the existing recover_project flow and clears the choice on success', async () => {
    const { state, open, recover } = setup();
    await open();
    state.result = { project: { id: pending.projectId, viewport: { width: 100, height: 100 } } };
    await recover(state.pending.projectId);
    assert.deepEqual(state.calls.at(-1), { command: 'recover_project', args: { projectId: pending.projectId, discard: false } });
    assert.equal(state.pending, null);
    assert.equal(state.showNew, false);
    assert.deepEqual(state.accepted, [state.result]);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
