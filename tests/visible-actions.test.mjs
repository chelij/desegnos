import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

// A control is left out of a surface when, in the current state, it cannot change what the
// user sees. These checks cover the rule itself and each surface that applies it.
const root = fileURLToPath(new URL('../', import.meta.url));
const read = async name => name === 'App.tsx'
  ? (await Promise.all(['App.tsx', 'session/useEditor.tsx', 'session/projectCommands.ts', 'editorShortcuts.ts'].map(file => readFile(join(root, 'src', file), 'utf8')))).join('\n')
  : readFile(join(root, 'src', name), 'utf8');
const temporary = await mkdtemp(join(root, '.visible-actions-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/visibleActions.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { requestEditorOpen, showsOpener } = await import(pathToFileURL(join(temporary, 'visibleActions.js')).href);

  await test('a request editor is open only for the selected layer that holds focus', () => {
    const open = { selectedElementId: 'box-1', focusedElementId: 'box-1' };
    assert.equal(requestEditorOpen('box-1', open), true);
    assert.equal(requestEditorOpen('box-2', open), false, 'another selected layer is not being edited');
    assert.equal(requestEditorOpen('box-1', { selectedElementId: 'box-1', focusedElementId: null }), false);
    assert.equal(requestEditorOpen('box-1', { selectedElementId: 'box-2', focusedElementId: 'box-1' }), false);
    assert.equal(requestEditorOpen(null, open), false);
    assert.equal(requestEditorOpen(undefined, { selectedElementId: null, focusedElementId: null }), false);
  });

  await test('an opener is shown only while the surface it opens is closed', () => {
    assert.equal(showsOpener({ available: true, open: false }), true);
    assert.equal(showsOpener({ available: true, open: true }), false, 'the open surface has its own Close');
    assert.equal(showsOpener({ available: false, open: false }), false);
    assert.equal(showsOpener({ available: false, open: true }), false);
  });

  await test('the app bar leaves out what is already on screen', async () => {
    const app = await read('App.tsx');
    assert.match(app, /import \{ showsOpener \} from '\.\/visibleActions';/);
    assert.match(app, /showsOpener\(\{ available: true, open: showImport \}\)/);
    assert.match(app, /showsOpener\(\{ available: !aiConnected, open: showAiSettings \}\)/);
    assert.match(app, /showsOpener\(\{ available: !onStartPage && !!project && !!\(generation\.job \|\| generation\.operation\), open: showLog \}\)/);
    assert.match(app, /showsOpener\(\{ available: true, open: showGridSettings \}\)/);
    assert.ok((app.match(/showsOpener\(\{ available: true, open: showLog \}\)/g) ?? []).length >= 2, 'Details stands down while the log is open');
    assert.doesNotMatch(app, /aria-pressed=\{showLog\}/);
  });

  await test('Undo and Redo appear only when they would change the canvas', async () => {
    const app = await read('App.tsx');
    assert.match(app, /\{view\?\.canUndo && <button aria-label="Undo last change"/);
    assert.match(app, /\{view\?\.canRedo && <button aria-label="Redo last change"/);
    assert.doesNotMatch(app, /disabled=\{!view\?\.can/);
    assert.match(app, /action === 'undo' \? current\.current\?\.canUndo : current\.current\?\.canRedo/);
  });

  await test('the AI log and status bar leave out Change request for the open request', async () => {
    const log = await read('GenerationLog.tsx');
    assert.match(log, /requestEditorOpen\?: \(elementId: string\) => boolean/);
    assert.match(log, /!requestEditorOpen\?\.\(item\.elementId\)/);
    assert.match(log, />Change request<\/button>/);
    const generation = await read('Generation.tsx');
    assert.match(generation, /!requestEditorOpen\?\.\(blocked\.elementId\)/);
    assert.match(generation, />Change request<\/button>/);
    assert.match(await read('App.tsx'), /requestEditorOpen=\{requestEditorIsOpen\}/);
  });

  await test('a notice that closes itself has no second close beside it', async () => {
    const banner = await read('ProblemBanner.tsx');
    assert.match(banner, /export function showsDismissButton\(notice: NoticeKind\): boolean \{/);
    assert.match(banner, /return !isToast\(notice\);/);
    assert.match(banner, /\{showDismiss && showsDismissButton\(\{ variant, action, secondaryAction \}\) && <button type="button" className="close-button notice-dismiss icon-button sm"/);
  });

  await test('a save conflict keeps the close button as its only cancel', async () => {
    const conflict = await read('SaveConflict.tsx');
    assert.doesNotMatch(conflict, /label: 'Cancel'/);
    assert.match(conflict, /secondaryAction=\{conflict\.canReplace \? \{ label: 'Replace it'/);
  });

  await test('settings and imports offer a save or a pick only while there is something to do', async () => {
    assert.match(await read('AiSettings.tsx'), /\{dirty && <button className="primary" disabled=\{busy \|\| pending \|\| \(!model && !view\.configurationError\)\}>Save AI settings<\/button>\}/);
    const importing = await read('ImportContext.tsx');
    assert.match(importing, /\{!automatic && <button type="button" disabled=\{locked \|\| !!candidate\}/);
    assert.match(importing, /\{source && !connected && <button type="button" onClick=\{openSettings\}>Connect AI<\/button>\}/);
    assert.match(importing, /disabled=\{locked \|\| !selection\?\.fits \|\| !connected\}/);
  });

  await test('panels leave out actions they have already applied', async () => {
    assert.match(await read('Notes.tsx'), /\{editing\?\.id !== decision\.id && <button onClick=\{\(\) => editDecision\(decision\)\}>Edit rule<\/button>\}/);
    const style = await read('StyleGuide.tsx');
    assert.match(style, /COLOUR_ROLES\.filter\(topic => normal\(rule\(topic\)\?\.value \?\? ''\) !== colour\)/);
    assert.match(style, /className="sg-found-colour sg-found-set"/);
  });

  await test('no first-run tip tour covers the canvas and the website import keeps its single entry', async () => {
    const app = await read('App.tsx');
    assert.doesNotMatch(app, /FirstRunTips|Onboarding/);
    assert.equal((app.match(/label: 'Import context…'/g) ?? []).length, 1);
    assert.equal((app.match(/label: 'Import website…'/g) ?? []).length, 1);
    assert.doesNotMatch(app, /label: 'Import…'/);
  });

  await test('the editor opens untitled and New opens another tab', async () => {
    const app = await read('App.tsx');
    assert.doesNotMatch(app, /NewProject|showNew/);
    assert.match(app, /case 'new': event\.preventDefault\(\); void newUntitled\(\)/);
    assert.match(app, /id: 'open-recent', label: 'Open recent'/);
    assert.match(app, /save_untitled_project/);
    assert.match(app, /id: 'import-website', label: 'Import website…'/);
  });

  await test('Home is a page and project controls leave the bar while it is shown', async () => {
    const app = await read('App.tsx');
    assert.match(app, /const onStartPage = !project \|\| showStartPage;/);
    assert.match(app, /id: 'home', label: 'Home'/);
    // The project name lives in its tab; the bar never repeats it beside the tabs.
    assert.doesNotMatch(app, /className="project-title"/);
    assert.match(app, /aria-label="Open projects"/);
    assert.match(app, /id: 'show-start-page', label: 'Show start page on launch'/);
    assert.match(app, /id: 'preferences', label: 'Preferences'/);
  });

  await test('hidden controls keep their menu entries and keyboard shortcuts', async () => {
    const app = await read('App.tsx');
    assert.match(app, /\{ id: 'settings', label: 'AI settings'/);
    assert.match(app, /\{ id: 'import', label: 'Import context…'/);
    assert.match(app, /\{ id: 'import-website', label: 'Import website…'/);
    assert.match(app, /\{ id: 'dossier', label: 'Project brief'/);
    const shortcuts = await read('shortcuts.ts');
    for (const action of ['insert', 'play', 'preview', 'export', 'link', 'unlink', 'group', 'ungroup']) {
      assert.match(shortcuts, new RegExp(`action: '${action}'`));
    }
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
