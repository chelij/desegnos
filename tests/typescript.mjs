import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const root = fileURLToPath(new URL('../', import.meta.url));

/** Compile browser TypeScript entry points into an importable Node module tree.
 * Node needs the file extensions the browser build leaves out, so relative imports are
 * rewritten after TypeScript emits them. */
export async function compileModules(entries) {
  const out = await mkdtemp(join(root, '.ts-test-'));
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--jsx', 'react-jsx', '--types', 'vite/client', '--skipLibCheck', '--outDir', out, ...entries], { cwd: root, encoding: 'utf8' });
  if (result.status !== 0) {
    await rm(out, { recursive: true, force: true });
    throw new Error(result.stderr || result.stdout || String(result.error));
  }
  await writeFile(join(out, 'style-stub.js'), 'export {};\n');
  const rewrite = async directory => {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) { await rewrite(path); continue; }
      if (!entry.name.endsWith('.js')) continue;
      let source = await readFile(path, 'utf8');
      source = source.replace(/(from\s+['"])(\.{1,2}\/[^'"]+\.css)(['"])/g, (_, before, _value, after) => `${before}./style-stub.js${after}`)
        .replace(/(import\s+['"])(\.{1,2}\/[^'"]+\.css)(['"])/g, (_, before, _value, after) => `${before}./style-stub.js${after}`);
      await writeFile(path, source.replace(/(from\s+['"])(\.{1,2}\/[^'"]+)(['"])/g,
        (_, before, value, after) => `${before}${value.endsWith('.js') ? value : `${value}.js`}${after}`));
    }
  };
  await rewrite(out);
  return {
    out,
    async load(name) {
      return import(pathToFileURL(join(out, name)).href);
    },
    async rewrite(name, transform) {
      const path = join(out, name);
      await writeFile(path, transform(await readFile(path, 'utf8')));
    },
    async cleanup() {
      await rm(out, { recursive: true, force: true });
    },
  };
}
