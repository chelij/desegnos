import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/updateCheck.ts']);
try {
  const { updateSummary } = await modules.load('updateCheck.js');

  const release = { version: '0.2.0', name: 'Desegnos 0.2.0', url: 'https://github.com/chelij/desegnos/releases/tag/v0.2.0', publishedAt: '2026-11-01T10:00:00Z' };

  await test('a newer release offers one download link', () => {
    assert.deepEqual(updateSummary({ current: '0.1.0', latest: release, newer: true }),
      { variant: 'success', message: 'Desegnos 0.2.0 is available.', download: release.url });
  });
  await test('the newest release being current says so with the version', () => {
    assert.deepEqual(updateSummary({ current: '0.1.0', latest: { ...release, version: '0.1.0' }, newer: false }),
      { variant: 'info', message: 'Desegnos is up to date (0.1.0).', download: null });
  });
  await test('a repository without releases says so', () => {
    assert.deepEqual(updateSummary({ current: '0.1.0', latest: null, newer: false }),
      { variant: 'info', message: 'No releases yet.', download: null });
  });
} finally {
  await modules.cleanup();
}
