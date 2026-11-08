import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
import { basename, join } from 'node:path';
import test from 'node:test';
import { root } from './typescript.mjs';

test('docs/PROJECT_MAP.md names every source file', async () => {
  const map = await readFile(join(root, 'docs/PROJECT_MAP.md'), 'utf8');
  const files = (await readdir(join(root, 'src'), { recursive: true }))
    .filter(path => /\.tsx?$/.test(path) && !path.startsWith('generated'));
  const missing = files.filter(path => !map.includes(basename(path)));
  assert.deepEqual(missing, [], 'add a row to docs/PROJECT_MAP.md for each of these files');
});
