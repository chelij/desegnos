import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.outside-edits-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--jsx', 'react-jsx', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/OutsideEditsNotice.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const source = (await readFile(join(temporary, 'OutsideEditsNotice.js'), 'utf8')).replace(/^import .*;$/gm, '').replace(/^export /gm, '');
  const Component = new Function('Notice', '_jsx', `${source}\nreturn OutsideEditsNotice;`)('shared-notice', (type, props) => ({ type, props }));
  const edits = { changedPaths: ['index.html', 'nested/page.html'], keptStalePaths: ['old.css', 'index.html'], backedUp: false };
  await test('one warning counts distinct outside paths with exactly the two requested actions', () => {
    const calls = [];
    const notice = Component({ edits, busy: false, importChanges: () => calls.push('import'), overwrite: () => calls.push('overwrite'), dismiss: () => calls.push('dismiss') });
    assert.equal(notice.type, 'shared-notice');
    assert.equal(notice.props.variant, 'warning');
    assert.equal(notice.props.message, '3 website files were changed outside Desegnos.');
    assert.equal(notice.props.title, undefined);
    assert.equal(notice.props.action.label, 'Import changes');
    assert.equal(notice.props.secondaryAction.label, 'Overwrite');
    notice.props.action.onClick(); notice.props.secondaryAction.onClick(); notice.props.dismiss();
    assert.deepEqual(calls, ['import', 'overwrite', 'dismiss']);
  });
  await test('actions are disabled while working and no notice remains after ownership or overwrite', () => {
    const notice = Component({ edits, busy: true });
    assert.equal(notice.props.action.disabled, true);
    assert.equal(notice.props.secondaryAction.disabled, true);
    assert.equal(Component({ edits: { changedPaths: [], keptStalePaths: [], backedUp: true }, busy: false }), null);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
