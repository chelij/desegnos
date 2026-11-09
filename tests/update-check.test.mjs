import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/updateCheck.ts']);
try {
  const { updateSummary } = await modules.load('updateCheck.js');
  const release = { version: '0.2.0', notes: 'Update notes', url: 'https://github.com/chelij/desegnos/releases/latest' };

  await test('a supported update offers installation', () => {
    assert.deepEqual(updateSummary({ current: '0.1.0', latest: release, newer: true, installSupported: true }),
      { variant: 'success', message: 'Desegnos 0.2.0 is available.', release, installSupported: true });
  });
  await test('an unsupported install keeps the release for a browser download', () => {
    assert.deepEqual(updateSummary({ current: '0.1.0', latest: release, newer: true, installSupported: false }),
      { variant: 'success', message: 'Desegnos 0.2.0 is available.', release, installSupported: false });
  });
  await test('a current release offers no installation', () => {
    assert.deepEqual(updateSummary({ current: '0.1.0', latest: { ...release, version: '0.1.0' }, newer: false, installSupported: false }),
      { variant: 'info', message: 'Desegnos is up to date (0.1.0).', release: null, installSupported: false });
  });
  await test('an endpoint with no update says the app is current', () => {
    assert.deepEqual(updateSummary({ current: '0.1.0', latest: null, newer: false, installSupported: false }),
      { variant: 'info', message: 'Desegnos is up to date (0.1.0).', release: null, installSupported: false });
  });
} finally {
  await modules.cleanup();
}
