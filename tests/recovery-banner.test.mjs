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
  const compiled = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--jsx', 'react-jsx', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--types', 'vite/client', '--outDir', temporary, 'src/RecoveryBanner.tsx', 'src/App.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(compiled.status, 0, compiled.stderr || compiled.stdout || String(compiled.error));
  // Node ESM needs extensions; preserve the app's bundler-style source imports and stub stylesheets.
  await writeFile(join(temporary, 'style-stub.js'), 'export {};\n');
  for (const [file, replacements] of [
    ['RecoveryBanner.js', [["'./icons'", "'./icons.js'"]]],
    ['ProblemBanner.js', [["'./icons'", "'./icons.js'"]]],
    ['api.js', [["'./usage'", "'./usage.js'"]]],
  ]) {
    const path = join(temporary, file);
    const source = await readFile(path, 'utf8');
    await writeFile(path, replacements.reduce((text, [from, to]) => text.replaceAll(from, to), source));
  }
  const { RecoveryChoice, isPendingRecovery } = await import(pathToFileURL(join(temporary, 'RecoveryBanner.js')).href);
  const { userError } = await import(pathToFileURL(join(temporary, 'userErrors.js')).href);
  await test('startup recovery failures do not become AI failures when diagnostics name elements', () => {
    const reason = 'de94f600-bf86-4db4-84e1-8af71e5f1522: The unsaved work in this project is damaged: unknown field `appearance`, expected one of `id`, `name`, `path`, `included`, `output`, `sizes`, `background`, `elements`';
    assert.equal(userError(reason), "Desegnos couldn't restore an unsaved project. Its original file is still on disk.");
  });
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
    assert.equal(html.replace(/<[^>]+>/g, ''), 'Unsaved changes from an earlier sessionRecoverOpen saved version');
    assert.match(html, /class="recovery-choice recovery-choice-row" role="alert"/);
    assert.match(html, /class="primary">Recover</);
    assert.match(html, />Open saved version</);
    assert.match(html, /aria-label="Dismiss recovery choice"/);
    assert.doesNotMatch(html, /role="dialog"|<dialog|modal|helper/i);
    const busy = renderToStaticMarkup(React.createElement(RecoveryChoice, choices({ busy: true })));
    assert.equal((busy.match(/<button[^>]*disabled=""/g) ?? []).length, 2, 'both actions wait while busy');
  });
  await test('the card variant keeps the same sentence and actions', () => {
    const html = renderToStaticMarkup(React.createElement(RecoveryChoice, { ...choices(), variant: 'card' }));
    assert.match(html, /class="recovery-choice recovery-choice-card" role="alert"/);
    assert.match(html, />Unsaved changes from an earlier session</);
    assert.match(html, />Recover</);
  });

  // Exercise App's actual handlers with a native/queue boundary stub, without a GUI.
  const app = await readFile(join(temporary, 'session/projectCommands.js'), 'utf8');
  function handler(name, environment) {
    const start = app.indexOf(`    async function ${name}(`);
    assert.ok(start >= 0, `App handler ${name} exists`);
    const end = app.indexOf('\n    }', start) + '\n    }'.length;
    const source = app.slice(start, end);
    return new Function('context', ...Object.keys(environment), `${source}; return ${name};`)(() => environment, ...Object.values(environment));
  }
  function setup() {
    const state = { pending: null, error: '', calls: [], accepted: [], showRecovery: false };
    const env = {
      isPendingRecovery,
      current: { current: null },
      setPendingRecovery: value => { state.pending = value; }, setError: value => { state.error = value; },
      openAlongside: operation => operation(false), synchronize: async () => {}, settle: async () => {},
      queue: operation => operation(),
      native: async (command, args) => { state.calls.push({ command, args }); if (state.failure) throw state.failure; if (!state.result) throw pending; return state.result; },
      accept: workspace => state.accepted.push(workspace), setSelected() {}, setNotice() {},
      setShowRecovery: value => { state.showRecovery = value; },
      setShowStartPage() {},
      setZoom() {}, initialZoom: () => 1, refreshRecent: async () => {},
    };
    env.requestProjectOpen = handler('requestProjectOpen', env);
    return { state, env, open: handler('open', env), openRecent: handler('openRecent', env), recover: handler('recover', env) };
  }
  for (const source of ['dialog', 'recent']) {
    await test(`${source} opening offers recovery inline and retries the exact saved file`, async () => {
      const { state, open, openRecent } = setup();
      await (source === 'dialog' ? open() : openRecent(pending.path));
      assert.deepEqual(state.pending, pending);
      assert.equal(state.error, '');
      assert.deepEqual(state.accepted, []);
      state.result = { project: { id: pending.projectId, viewport: { width: 100, height: 100 } } };
      await open(false, state.pending.path, true);
      assert.deepEqual(state.calls.at(-1), { command: 'open_project', args: { discard: false, folder: false, path: pending.path, discardRecovery: true } });
      assert.equal(state.pending, null);
      assert.deepEqual(state.accepted, [state.result]);
    });
  }
  await test('a pending choice stays beside an already open project instead of replacing it', async () => {
    const { state, env, open } = setup();
    env.current.current = { project: { id: 'open-id' } };
    await open();
    assert.deepEqual(state.pending, pending);
    assert.deepEqual(state.accepted, []);
  });
  await test('App keeps the choice in the recovery surface, never in the global top bar', async () => {
    const source = await readFile(join(root, 'src/App.tsx'), 'utf8');
    assert.doesNotMatch(source, /<RecoveryBanner/);
    assert.equal((source.match(/<RecoveryChoice \{\.\.\.recovery\} \/>/g) ?? []).length, 1);
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
    assert.equal(state.showRecovery, false);
    assert.deepEqual(state.accepted, [state.result]);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
