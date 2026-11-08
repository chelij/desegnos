import { readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { runCommand } from './run.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
// Browser suites have their own npm commands; native tests live under tests/native/.
const browserTests = new Set(['ai-models-browser.test.mjs', 'canvas-ux-browser.test.mjs', 'notice-system.test.mjs']);
const files = (await readdir(new URL('../tests/', import.meta.url)))
  .filter(name => name.endsWith('.test.mjs') && !browserTests.has(name)).sort();
if (!files.length) throw new Error('No editor tests found.');
for (const file of files) await runCommand(process.execPath, [`tests/${file}`], { cwd: root });
