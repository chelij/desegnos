import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

// A control is left out of a surface when, in the current state, it cannot change what the
// user sees. These checks cover the rule itself and each surface that applies it.
const root = fileURLToPath(new URL('../', import.meta.url));
const read = name => readFile(join(root, 'src', name), 'utf8');
const temporary = await mkdtemp(join(root, '.visible-actions-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/visibleActions.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { requestEditorOpen, showsOpener } = await import(pathToFileURL(join(temporary, 'visibleActions.js')).href);

  await test('a request editor is open only for the selected request that holds focus', () => {
    const open = { selectedRegionId: 'box-1', focusedRegionId: 'box-1' };
    assert.equal(requestEditorOpen('box-1', open), true);
    assert.equal(requestEditorOpen('box-2', open), false, 'another selected box is not being edited');
    assert.equal(requestEditorOpen('box-1', { selectedRegionId: 'box-1', focusedRegionId: null }), false);
    assert.equal(requestEditorOpen('box-1', { selectedRegionId: 'box-2', focusedRegionId: 'box-1' }), false);
    assert.equal(requestEditorOpen(null, open), false);
    assert.equal(requestEditorOpen(undefined, { selectedRegionId: null, focusedRegionId: null }), false);
  });

  await test('an opener is shown only while the surface it opens is closed', () => {
    assert.equal(showsOpener({ available: true, open: false }), true);
    assert.equal(showsOpener({ available: true, open: true }), false, 'the open surface has its own Close');
    assert.equal(showsOpener({ available: false, open: false }), false);
    assert.equal(showsOpener({ available: false, open: true }), false);
  });

  await test('the app bar leaves out what is already on screen and what cannot change', async () => {
    const app = await read('App.tsx');
    assert.match(app, /import \{ requestEditorOpen, showsOpener \} from '\.\/visibleActions';/);
    // AI log while the log is open, and Details beside it.
    assert.match(app, /showsOpener\(\{ available: !!project && !!\(generation\.job \|\| generation\.operation\), open: showLog \}\)/);
    assert.equal((app.match(/showsOpener\(\{ available: true, open: showLog \}\)/g) ?? []).length, 3,
      'result Details, generation Details and the draft notice all stand down while the log is open');
    assert.doesNotMatch(app, /aria-pressed=\{showLog\}/);
    // Panels that are already open, and the project close choice that is already open.
    assert.match(app, /showsOpener\(\{ available: true, open: showImport \}\)/);
    assert.match(app, /showsOpener\(\{ available: !aiConnected, open: showAiSettings \}\)/);
    assert.match(app, /showsOpener\(\{ available: fixCount > 0, open: showCheck \}\)/);
    assert.match(app, /showsOpener\(\{ available: true, open: showGridSettings \}\)/);
    assert.match(app, /showsOpener\(\{ available: true, open: closeChoice === open\.id \}\)/);
  });

  await test('Undo and Redo appear only when they would change the page', async () => {
    const app = await read('App.tsx');
    assert.match(app, /\{view\?\.canUndo && <button aria-label="Undo last change"/);
    assert.match(app, /\{view\?\.canRedo && <button aria-label="Redo last change"/);
    assert.doesNotMatch(app, /className="compact"/, 'top-bar buttons share one control size');
    assert.doesNotMatch(app, /disabled=\{!view\?\.can/);
    // The keyboard shortcut still travels through the same history commands.
    assert.match(app, /shortcut === 'undo' \? current\.current\?\.canUndo : current\.current\?\.canRedo/);
  });

  await test('a result already on the page offers Undo and Details, never a retry', async () => {
    const app = await read('App.tsx');
    assert.doesNotMatch(app, /retryResult/);
    const start = app.indexOf('generation-notice status-applied');
    const resultBar = app.slice(start, start + 800);
    assert.match(resultBar, /title="Page updated"/);
    assert.match(resultBar, /label: 'Undo'/);
    assert.doesNotMatch(resultBar, /Try again|label: 'Cancel'/);
  });

  await test('canvas callouts leave out actions that are already done', async () => {
    const canvas = await read('Canvas.tsx');
    assert.match(canvas, /requestEditorOpen\(gap\.regionId, \{ selectedRegionId: selected, focusedRegionId: focusRequest \}\)/);
    assert.equal((canvas.match(/!requestEditorOpen\(problem\.regionId, \{/g) ?? []).length, 2);
    // Nothing to apply without a draft; the retry beside it stays.
    assert.match(canvas, /\{problem\.draftId && <button disabled=\{busy \|\| !!error\}/);
    assert.doesNotMatch(canvas, /disabled=\{busy \|\| !problem\.draftId \|\| !!error\}/);
    // The element toolbar leaves out Edit while the Edit dialog is already open; F2 and the
    // context menu still open it.
    assert.match(canvas, /\{elementDialog !== 'edit' && <button className="ask-ai" title=\{withShortcut\('Change text, links and style', 'editElement'\)\}/);
    assert.match(canvas, /event\.key === 'F2' && !nudge\.current\.promise && !editingElement\) \{ event\.preventDefault\(\); setElementDialog\('edit'\)/);
  });

  await test('the AI log and its status bar leave out Change request for the open request', async () => {
    const log = await read('GenerationLog.tsx');
    assert.match(log, /requestEditorOpen\?: \(regionId: string\) => boolean/);
    assert.match(log, /\{!requestEditorOpen\?\.\(item\.regionId\) && <button type="button" onClick=\{\(\) => editRequest\(item\.regionId\)\}>Change request<\/button>\}/);
    const generation = await read('Generation.tsx');
    assert.match(generation, /!requestEditorOpen\?\.\(blocked\.regionId\)/);
    assert.match(await read('App.tsx'), /requestEditorOpen=\{requestEditorIsOpen\}/);
  });

  await test('a notice that closes itself has no second close beside it', async () => {
    const banner = await read('ProblemBanner.tsx');
    assert.match(banner, /export function showsDismissButton\(notice: NoticeKind\): boolean \{/);
    assert.match(banner, /return !isToast\(notice\);/);
    assert.match(banner, /\{showsDismissButton\(\{ variant, action, secondaryAction \}\) && <button type="button" className="notice-dismiss icon-button sm"/);
  });

  await test('a save conflict keeps the close button as its only cancel', async () => {
    const conflict = await read('SaveConflict.tsx');
    assert.doesNotMatch(conflict, /label: 'Cancel'/);
    assert.match(conflict, /secondaryAction=\{conflict\.canReplace \? \{ label: 'Replace it', onClick: \(\) => choose\('replace'\) \} : undefined\}/);
  });

  await test('settings and imports offer a save or a pick only while there is something to do', async () => {
    assert.match(await read('AiSettings.tsx'), /\{dirty && <button className="primary" disabled=\{busy \|\| pending \|\| \(!model && !view\.configurationError\)\}>Save AI settings<\/button>\}/);
    const importing = await read('ImportContext.tsx');
    assert.match(importing, /\{!automatic && <button type="button" disabled=\{locked \|\| !!candidate\}/);
    // Read my brand directly below already retries; only the AI settings fix is its own action.
    assert.doesNotMatch(importing, /label: 'Try again'/);
    assert.match(importing, /needsAiSettings\(analysis\.error \|\| ''\) \? \{ label: 'AI settings'/);
  });

  await test('panels leave out actions they have already applied', async () => {
    assert.match(await read('Notes.tsx'), /\{editing\?\.id !== decision\.id && <button onClick=\{\(\) => editDecision\(decision\)\}>Edit rule<\/button>\}/);
    const style = await read('StyleGuide.tsx');
    assert.match(style, /COLOUR_ROLES\.filter\(topic => normal\(rule\(topic\)\?\.value \?\? ''\) !== colour\)/);
    assert.match(style, /className="sg-found-colour sg-found-set"/);
  });

  await test('the start screen finishes once and opens a website once', async () => {
    assert.match(await read('Onboarding.tsx'), /\{step < TIPS\.length - 1 && <button type="button" className="link-button" onClick=\{finish\}>Skip tips<\/button>\}/);
    const choice = await read('WebsiteChoice.tsx');
    assert.match(choice, /aria-pressed=\{!brand\}[^>]*onClick=\{chooseEdit\}>Edit this website<\/button>/);
    assert.match(choice, /className="primary"[^>]*onClick=\{\(\) => \{ void edit\(\)/);
    assert.match(choice, /chooseEdit: \(\) => void;/);
    // A failed build or open retries from the button that already offers it.
    assert.doesNotMatch(choice, /label: 'Try again'/);
  });

  await test('hidden controls keep their menu entries and shortcuts', async () => {
    const app = await read('App.tsx');
    assert.match(app, /\{ id: 'live', label: 'Live view'/);
    assert.match(app, /\{ id: 'settings', label: 'AI settings'/);
    assert.match(app, /\{ id: 'page-check', label: fixCount \? `Check page \(\$\{fixCount\}\)` : 'Check page'/);
    assert.match(app, /\{ id: 'import', label: 'Import…'/);
    assert.match(app, /\{ id: 'dossier', label: 'Project brief'/);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
