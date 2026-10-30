import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/generationSession.ts']);
try {
  const { ownsJob, announcesStatus } = await modules.load('generationSession.js');
  const job = { jobId: 'job', status: 'blocked', candidate: { requests: [{ elementId: 'area', status: 'conflict', explanation: 'Which booking service?' }] } };
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
    const clean = { ...job, candidate: { requests: [{ elementId: 'area', status: 'implemented', explanation: '' }] } };
    assert.equal(announcesStatus({ ...clean, status: 'ready' }, owns), false);
    assert.equal(announcesStatus({ ...clean, status: 'applied' }, owns), false);
    assert.equal(announcesStatus({ ...clean, status: 'blocked' }, owns), true);
    assert.equal(announcesStatus(null, { ...owns, stopped: true }), true);
  });

} finally { await modules.cleanup(); }
