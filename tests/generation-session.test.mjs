import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const temporary = await mkdtemp(new URL('../.generation-session-', import.meta.url));
try {
  const compiler = new URL('bin/tsc', import.meta.resolve('typescript/package.json')).pathname;
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--outDir', temporary, 'src/generationSession.ts'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const { ownsJob, announcesStatus, announcedDraftCount } = await import(pathToFileURL(join(temporary, 'generationSession.js')));
  const draft = { id: 'one', pageId: 'page', code: 'saved output' };
  const job = { jobId: 'job', status: 'blocked', drafts: [draft], candidate: { regions: [{ regionId: 'area', status: 'conflict', explanation: 'Which booking service?' }], componentRequests: [] } };
  const state = { owns: false, stopped: false, working: false };

  await test('a job restored on project open is not owned by this session', () => {
    assert.equal(ownsJob('project', job, null), false);
    assert.equal(ownsJob('project', job, { projectId: 'project', jobId: 'other' }), false);
    // Switching back to a project restores the job again; the earlier session action no longer applies.
    assert.equal(ownsJob('project', job, { projectId: 'other-project', jobId: 'job' }), false);
    assert.equal(ownsJob('project', null, { projectId: 'project', jobId: 'job' }), false);
    assert.equal(ownsJob(null, job, { projectId: 'project', jobId: 'job' }), false);
    assert.equal(ownsJob('project', job, { projectId: 'project', jobId: 'job' }), true);
  });

  await test('a restored job never shows an automatic status notice', () => {
    assert.equal(announcesStatus(job, state), false);
    assert.equal(announcesStatus(job, { ...state, working: true }), false);
    assert.equal(announcesStatus({ ...job, status: 'generating' }, { ...state, working: true }), false);
    assert.equal(announcesStatus(job, { ...state, stopped: true }), false);
    assert.equal(announcesStatus(job, { ...state, owns: true }), true);
  });

  await test('a generation started in this session keeps every notice', () => {
    const owns = { ...state, owns: true };
    assert.equal(announcesStatus({ ...job, status: 'prepared' }, owns), true);
    assert.equal(announcesStatus({ ...job, status: 'failed' }, owns), true);
    assert.equal(announcesStatus({ ...job, status: 'cancelled' }, owns), true);
    assert.equal(announcesStatus({ ...job, status: 'generating' }, { ...owns, working: true }), true);
    const clean = { ...job, candidate: { regions: [{ regionId: 'area', status: 'implemented', explanation: '' }], componentRequests: [] } };
    assert.equal(announcesStatus({ ...clean, status: 'ready' }, owns), false);
    assert.equal(announcesStatus({ ...clean, status: 'applied' }, owns), false);
    assert.equal(announcesStatus({ ...clean, status: 'blocked' }, owns), true);
    assert.equal(announcesStatus(null, { ...owns, stopped: true }), true);
  });

  await test('drafts of a restored job stay in the AI conversation', () => {
    assert.equal(announcedDraftCount(job, true), 1);
    assert.equal(announcedDraftCount(job, false), 0);
    assert.equal(announcedDraftCount(null, true), 0);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
