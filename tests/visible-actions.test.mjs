import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import test from 'node:test';
import { compileModules, root } from './typescript.mjs';

// A control is left out of a surface when, in the current state, it cannot change what the
// user sees. These checks cover the rule itself and each surface that applies it.
const read = async name => name === 'App.tsx'
  ? (await Promise.all(['App.tsx', 'session/useEditor.tsx', 'session/projectCommands.ts', 'editorShortcuts.ts'].map(file => readFile(join(root, 'src', file), 'utf8')))).join('\n')
  : readFile(join(root, 'src', name), 'utf8');
const modules = await compileModules(['src/visibleActions.ts']);
try {
  const { projectHasPreviewPages, showsOpener } = await modules.load('visibleActions.js');



  await test('an opener is shown only while the surface it opens is closed', () => {
    assert.equal(showsOpener({ available: true, open: false }), true);
    assert.equal(showsOpener({ available: true, open: true }), false, 'the open surface has its own Close');
    assert.equal(showsOpener({ available: false, open: false }), false);
    assert.equal(showsOpener({ available: false, open: true }), false);
  });

  await test('Preview is offered only while a project has web output', () => {
    const project = outputs => ({ pages: outputs.map((output, index) => ({ id: `page-${index}`, output })) });
    assert.equal(projectHasPreviewPages(null), false);
    assert.equal(projectHasPreviewPages({ pages: [] }), false);
    assert.equal(projectHasPreviewPages(project(['slide', 'image'])), false);
    assert.equal(projectHasPreviewPages(project(['image', 'webpage'])), true);
    assert.equal(projectHasPreviewPages(project(['appScreen'])), true);
  });

  await test('the app bar leaves out what is already on screen', async () => {
    const app = await read('App.tsx');
    assert.match(app, /import \{ projectHasPreviewPages, showsOpener \} from '\.\/visibleActions';/);
    // Import stays in place and shows as pressed, so the bar never shifts under the pointer.
    assert.match(app, /aria-pressed=\{panel\.kind === 'import'\} aria-label="Import"/);
    assert.match(app, /showsOpener\(\{ available: !aiConnected, open: showAiSettings \}\)/);
    assert.match(app, /showsOpener\(\{ available: !onStartPage && !!project && !!\(generation\.job \|\| generation\.operation\), open: panel\.kind === 'log' \}\)/);
    assert.match(app, /showsOpener\(\{ available: true, open: panel\.kind === 'gridSettings' \}\)/);
    assert.match(app, /showsOpener\(\{ available: true, open: panel\.kind === 'log' \}\)/, 'Details stands down while the log is open');
    assert.doesNotMatch(app, /aria-pressed=\{panel\.kind === 'log'\}/);
  });

  await test('Undo and Redo keep their place and disable when they would change nothing', async () => {
    const app = await read('App.tsx');
    assert.match(app, /aria-label="Undo last change" type="button" className="icon-button" disabled=\{!view\?\.canUndo\}/);
    assert.match(app, /aria-label="Redo last change" type="button" className="icon-button" disabled=\{!view\?\.canRedo\}/);
    assert.match(app, /command === 'undo_project' \? workspace\?\.canUndo : workspace\?\.canRedo/);
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
    assert.match(await read('AiSettings.tsx'), /\{dirty && <button className="primary" disabled=\{busy \|\| pending \|\| endpointDirty \|\| \(!model && !view\.configurationError\)\}>Save AI settings<\/button>\}/);
    const importing = await read('ImportContext.tsx');
    assert.match(importing, /\{!automatic && <button type="button" disabled=\{locked \|\| !!candidate\}/);
    assert.match(importing, /\{\(source \|\| pages\.source\) && !connected && <button type="button" onClick=\{openSettings\}>Connect AI<\/button>\}/);
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
    assert.equal((app.match(/label: 'Import…'/g) ?? []).length, 1);
    assert.doesNotMatch(app, /label: 'Import website…'/);
  });

  await test('the editor opens untitled and New opens another tab', async () => {
    const app = await read('App.tsx');
    assert.doesNotMatch(app, /NewProject|showNew/);
    assert.match(app, /case 'new': event\.preventDefault\(\); void newUntitled\(\)/);
    assert.match(app, /id: 'open-recent', label: 'Open recent'/);
    assert.match(app, /save_untitled_project/);
    assert.match(app, /id: 'import', label: 'Import…'/);
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

  await test('the editor menu hides what does not apply and keeps panel actions out of it', async () => {
    const app = await read('App.tsx');
    assert.match(app, /\.\.\.\(!onStartPage \? \[\{ id: 'home', label: 'Home'/);
    assert.match(app, /\.\.\.\(onStartPage && project \? \[\{ id: 'back-to-canvas'/);
    assert.match(app, /\.\.\.\(recentProjects\.length \? \[\{ id: 'open-recent'/);
    assert.match(app, /\.\.\.\(canEditProject && project\.lastExport \? \[\{ id: 'open-browser'/);
    assert.doesNotMatch(app, /Publish online/);
    assert.doesNotMatch(app, /id: 'compact-history'/);
    assert.match(await read('HistoryPanel.tsx'), /disabled=\{busy \|\| !data\} onClick=\{\(\) => compact\(\)\}>Compact history<\/button>/);
  });

  await test('Play names the boundary it runs and the bar reads Stop while it plays', async () => {
    const app = await read('App.tsx');
    assert.match(app, /className="play-pill" role="status"/);
    assert.match(app, /Playing \{playingLabel\}/);
    assert.match(app, /<button type="button" onClick=\{\(\) => setPlay\(idlePlay\)\}>Stop<\/button>/);
    assert.match(app, /playIsActive\(play\) \? 'stop' : 'play'/);
    assert.match(app, /playIsActive\(play\) \? 'Stop' : 'Play'/);
    assert.doesNotMatch(app, /Playing · Esc to stop/);
  });

  await test('the app bar labels its primary actions beside their icons', async () => {
    const app = await read('App.tsx');
    assert.match(app, /aria-label="Import" title="Import".*<span className="button-label">Import<\/span>/);
    assert.match(app, /className="icon-button labeled" aria-label="Preview export"/);
    assert.match(app, /<span className="button-label">Preview<\/span>/);
    assert.match(app, /<span className="button-label">Export<\/span>/);
    // The editor theme lives in Preferences; the start page keeps its own toggle.
    assert.match(app, /\(onStartPage \|\| !project\) && <button type="button" className="theme-toggle icon-button"/);
    assert.match(await read('shell.css'), /@media \(max-width: 1100px\) \{\n  \.shell-primary-actions > button\.icon-button\.labeled \{ width: var\(--control-md\); padding: 0; \}/);
  });

  await test('hidden controls keep their menu entries and keyboard shortcuts', async () => {
    const app = await read('App.tsx');
    assert.match(app, /\{ id: 'settings', label: 'AI settings'/);
    assert.match(app, /\{ id: 'import', label: 'Import…'/);
    assert.doesNotMatch(app, /id: 'import-website'/);
    assert.match(app, /\{ id: 'dossier', label: 'Project brief'/);
    const shortcuts = await read('shortcuts.ts');
    for (const action of ['insert', 'play', 'preview', 'export', 'link', 'unlink', 'group', 'ungroup']) {
      assert.match(shortcuts, new RegExp(`action: '${action}'`));
    }
  });
} finally {
  await modules.cleanup();
}
