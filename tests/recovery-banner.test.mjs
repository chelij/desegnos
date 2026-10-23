import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.recovery-banner-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const compiled = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--jsx', 'react-jsx', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--types', 'vite/client', '--outDir', temporary, 'src/RecoveryBanner.tsx', 'src/App.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(compiled.status, 0, compiled.stderr || compiled.stdout || String(compiled.error));
  const { RecoveryBanner, isPendingRecovery } = await import(pathToFileURL(join(temporary, 'RecoveryBanner.js')).href);
  const pending = { code: 'pendingRecovery', projectId: 'project-id', path: '/chosen/site.desegnos' };

  await test('recovery errors are identified by code and typed fields, never by prose', () => {
    assert.equal(isPendingRecovery(pending), true);
    for (const error of [null, pending.code, new Error('This project has unsaved changes from an earlier session.'), { ...pending, code: 'other' }, { ...pending, path: null }, { ...pending, projectId: 3 }, { missing: true, message: 'Missing project' }]) {
      assert.equal(isPendingRecovery(error), false);
    }
  });
  await test('the inline banner has only the sentence, two actions, and dismiss', () => {
    const actions = [];
    const props = { busy: false, recover: () => actions.push('recover'), openSaved: () => actions.push('saved'), dismiss: () => actions.push('dismiss') };
    const html = renderToStaticMarkup(React.createElement(RecoveryBanner, props));
    assert.equal(html.replace(/<[^>]+>/g, ''), 'This project has unsaved changes from an earlier session.RecoverOpen saved versionDismiss');
    assert.match(html, /class="problem-banner recovery-banner"/);
    assert.doesNotMatch(html, /role="dialog"|<dialog|modal|helper/i);
    const banner = RecoveryBanner(props).props;
    banner.action.onClick(); banner.children.props.onClick(); banner.dismiss();
    assert.deepEqual(actions, ['recover', 'saved', 'dismiss']);
    const busy = RecoveryBanner({ ...props, busy: true }).props;
    assert.equal(busy.action.disabled, true);
    assert.equal(busy.children.props.disabled, true);
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
    const state = { pending: null, error: '', calls: [], accepted: [], showNew: true };
    const env = {
      isPendingRecovery,
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
    return { state, open: handler('open', env), openRecent: handler('openRecent', env), editWebsite: handler('editWebsite', env), recover: handler('recover', env) };
  }
  for (const source of ['dialog', 'recent', 'website']) {
    await test(`${source} opening offers recovery inline and retries the exact saved file`, async () => {
      const { state, open, openRecent, editWebsite } = setup();
      await (source === 'dialog' ? open() : source === 'recent' ? openRecent(pending.path) : editWebsite('folder-id'));
      assert.deepEqual(state.pending, pending);
      assert.equal(state.error, '');
      assert.deepEqual(state.accepted, []);
      state.result = { project: { id: pending.projectId, viewport: { width: 100, height: 100 } } };
      await open(false, state.pending.path, true);
      assert.deepEqual(state.calls.at(-1), { command: 'open_project', args: { discard: false, folder: false, path: pending.path, discardRecovery: true } });
      assert.equal(state.pending, null);
      assert.deepEqual(state.accepted, [state.result]);
      assert.equal(state.showNew, false);
    });
  }
  await test('unrelated native errors retain their existing reporting path', async () => {
    const { state, openRecent } = setup();
    state.failure = { missing: true, message: 'Could not open this recent project' };
    await assert.rejects(() => openRecent(pending.path), error => error === state.failure);
    assert.equal(state.pending, null);
  });
  await test('Recover uses the existing recover_project flow and clears the banner on success', async () => {
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
