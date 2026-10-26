import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, writeFile, readdir, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.overlay-lifecycle-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--jsx', 'react-jsx', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/FrameOverlays.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const modulePath = join(temporary, 'FrameOverlays.js');
  for (const name of await readdir(temporary)) {
    if (!name.endsWith('.js')) continue;
    const path = join(temporary, name);
    await writeFile(path, (await readFile(path, 'utf8')).replace(/from '(\.\/[^']+)'/g, (_, value) => `from '${value.endsWith('.js') ? value : value + '.js'}'`));
  }
  const { FrameOverlay } = await import(pathToFileURL(modulePath).href);
  const props = {
    frame: { key: 'frame', rect: { x: 0, y: 0, width: 320, height: 200 }, src: 'desegnos-preview://still.localhost/game.html', sandbox: 'allow-scripts', allow: null, allowFullscreen: false, selector: null },
    live: { sessionId: 'live', url: 'desegnos-preview://live.localhost/index.html' }, pageId: 'page', staticPageUrl: 'desegnos-preview://still.localhost/index.html',
    zoom: 1, placement: {}, title: 'Game', stop() {},
  };
  await test('passive overlays mount no running iframe, including external and unmapped embeds', () => {
    for (const src of [props.frame.src, 'https://example.com/game']) {
      assert.equal(renderToStaticMarkup(createElement(FrameOverlay, { ...props, frame: { ...props.frame, src }, interactive: false })), '');
    }
  });
  await test('explicit Interact mounts only an isolated preview iframe', () => {
    const html = renderToStaticMarkup(createElement(FrameOverlay, { ...props, interactive: true }));
    assert.match(html, /<iframe/);
    assert.match(html, /src="desegnos-preview:\/\/live.localhost\/game.html"/);
    assert.match(html, /sandbox="allow-scripts"/);
    assert.doesNotMatch(html, /allow-same-origin/);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
