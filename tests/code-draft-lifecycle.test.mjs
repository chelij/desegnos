import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.code-draft-lifecycle-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/codeDraftLifecycle.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { synchronizeCodeDrafts } = await import(pathToFileURL(join(temporary, 'codeDraftLifecycle.js')).href);
  await test('navigation clears delayed saves and waits for every code file to flush', async () => {
    let pending = [{ language: 'HTML', status: 'dirty' }, { language: 'CSS', status: 'dirty' }, { language: 'JavaScript', status: 'dirty' }];
    const events = [];
    await synchronizeCodeDrafts({ clearTimers() { events.push('timers cleared'); }, async flush() { for (const file of pending) events.push(`${file.language} saved`); pending = []; }, pending: () => pending });
    assert.deepEqual(events, ['timers cleared', 'HTML saved', 'CSS saved', 'JavaScript saved']);
  });
  await test('an in-flight source edit must finish before navigation can complete', async () => {
    let release;
    const sourceEdit = new Promise(resolve => { release = resolve; });
    let pending = [{ language: 'HTML', status: 'saving' }], complete = false;
    const synchronized = synchronizeCodeDrafts({ clearTimers() {}, async flush() { await sourceEdit; pending = []; }, pending: () => pending }).then(() => { complete = true; });
    await new Promise(resolve => setImmediate(resolve)); assert.equal(complete, false);
    release(); await synchronized; assert.equal(complete, true);
  });
  for (const status of ['invalid', 'conflict', 'error']) await test(`${status} source stays in place and blocks transition with a plain action`, async () => {
    const draft = { language: 'JavaScript', status, text: 'const bad = ;' };
    await assert.rejects(synchronizeCodeDrafts({ clearTimers() {}, async flush() {}, pending: () => [draft] }), /JavaScript code has unsaved changes.*Discard code changes/);
    assert.equal(draft.text, 'const bad = ;');
  });
  await test('continuing edits receive bounded retries and retain the newer draft', async () => {
    let calls = 0;
    const draft = { language: 'HTML', status: 'dirty', text: 'current text' };
    await assert.rejects(synchronizeCodeDrafts({ clearTimers() {}, async flush() { calls++; draft.text += '!'; }, pending: () => [draft] }), /Code changed while saving/);
    assert.equal(calls, 3); assert.equal(draft.text, 'current text!!!');
  });
  await test('a stalled native save times out without permitting navigation or cancelling accepted work', async () => {
    let accepted = false, release;
    const sourceEdit = new Promise(resolve => { release = resolve; });
    await assert.rejects(synchronizeCodeDrafts({ clearTimers() {}, async flush() { await sourceEdit; accepted = true; }, pending: () => [] }, 5), /Code is still saving.*Keep this page open/);
    assert.equal(accepted, false);
    release(); await new Promise(resolve => setImmediate(resolve)); assert.equal(accepted, true);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
