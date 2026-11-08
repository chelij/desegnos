import { readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { runCommand } from './run.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
// Tests that need Chromium end in `-browser.test.mjs`; `--browser` runs those instead of the rest.
// Native tests live under tests/native/.
const browser = process.argv.includes('--browser');
const files = (await readdir(new URL('../tests/', import.meta.url)))
  .filter(name => name.endsWith('.test.mjs') && name.endsWith('-browser.test.mjs') === browser).sort();
if (!files.length) throw new Error('No editor tests found.');
for (const file of files) await runCommand(process.execPath, [`tests/${file}`], { cwd: root });
