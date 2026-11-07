import assert from 'node:assert/strict';
import test from 'node:test';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/exportJobs/exportFolder.ts']);
try {
  await writeFile(join(modules.out, 'path-stub.js'), `export const documentDir = async () => '/home/user/Documents';
export const join = async (...parts) => parts.join('/');\n`);
  // A single compiled entry is emitted at the output root.
  await modules.rewrite('exportFolder.js', source => source.replace("'@tauri-apps/api/path'", "'./path-stub.js'"));
  const { defaultExportFolder } = await modules.load('exportFolder.js');
  await test('default export selects a parent for exactly one project folder', async () => {
    assert.equal(await defaultExportFolder('Untitled website'), '/home/user/Documents/Desegnos');
  });
  await test('legacy automatic destinations migrate while custom choices survive', async () => {
    assert.equal(await defaultExportFolder('Untitled website', '/home/user/Documents/Desegnos/Untitled website'), '/home/user/Documents/Desegnos');
    assert.equal(await defaultExportFolder('Site', '/exports/custom'), '/exports/custom');
    assert.equal(await defaultExportFolder('Site', '/home/user/Documents/Desegnos'), '/home/user/Documents/Desegnos');
    assert.equal(await defaultExportFolder(' :? ', '/home/user/Documents/Desegnos/Untitled project'), '/home/user/Documents/Desegnos');
  });
} finally {
  await modules.cleanup();
}
