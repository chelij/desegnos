import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.live-bar-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/liveBar.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { liveCommand, liveLocation, pathPage, sitePath } = await import(pathToFileURL(join(temporary, 'liveBar.js')).href);

  await test('history controls name the session that owns the live page', () => {
    assert.deepEqual(liveCommand('session-1', 'back'), { type: 'desegni-preview-command', sessionId: 'session-1', action: 'back' });
    assert.equal(liveCommand('session-1', 'reload').action, 'reload');
    assert.equal(liveCommand('session-1', 'forward').sessionId, 'session-1');
  });

  await test('only a location message from this session updates the bar', () => {
    const message = { type: 'desegni-preview-location', sessionId: 'session-1', pageId: 'page-1', path: '/projects/papan/' };
    assert.equal(liveLocation(message, 'session-1'), '/projects/papan/');
    assert.equal(liveLocation({ ...message, sessionId: 'session-2' }, 'session-1'), null);
    assert.equal(liveLocation({ ...message, type: 'desegni-preview-error' }, 'session-1'), null);
    assert.equal(liveLocation({ ...message, path: 'projects/papan/' }, 'session-1'), null);
    assert.equal(liveLocation({ ...message, path: `/${'a'.repeat(2048)}` }, 'session-1'), null);
    assert.equal(liveLocation(null, 'session-1'), null);
    assert.equal(liveLocation([message], 'session-1'), null);
    assert.equal(liveLocation('location', 'session-1'), null);
  });

  await test('a directory index and its route name the same site path', () => {
    assert.equal(sitePath('/'), '/');
    assert.equal(sitePath('/projects/papan'), '/projects/papan');
    assert.equal(sitePath('/projects/papan/'), '/projects/papan');
    assert.equal(sitePath('/projects/papan/index.html'), '/projects/papan');
    assert.equal(sitePath('/projects/papan/index.htm'), '/projects/papan');
    assert.equal(sitePath('/index.html'), '/');
    assert.equal(sitePath('/projects/papan/?ref=home#top'), '/projects/papan');
    assert.equal(sitePath('/projects/papan%20two/'), '/projects/papan two');
    assert.equal(sitePath('projects/papan'), null);
    assert.equal(sitePath('projects/papan/?ref=home'), null);
  });

  await test('only a path that maps to a project page offers to edit that page', () => {
    const pages = [
      { id: 'home', name: 'Home', path: '/' },
      { id: 'papan', name: 'Papan', path: '/projects/papan' },
      { id: 'about', name: 'About', path: '/about' },
    ];
    assert.equal(pathPage('/', pages)?.id, 'home');
    assert.equal(pathPage('/index.html', pages)?.id, 'home');
    assert.equal(pathPage('/projects/papan', pages)?.id, 'papan');
    assert.equal(pathPage('/projects/papan/', pages)?.id, 'papan');
    assert.equal(pathPage('/projects/papan/index.html', pages)?.id, 'papan');
    assert.equal(pathPage('/Projects/Papan/?ref=home#top', pages)?.id, 'papan');
    assert.equal(pathPage('/about.html', pages)?.id, 'about');
    // A path the project does not own stays inside Live; it never selects a page.
    assert.equal(pathPage('/notes/2026', pages), null);
    assert.equal(pathPage('/about/team', pages), null);
    assert.equal(pathPage('', pages), null);
    assert.equal(pathPage('/', []), null);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
