import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const temporary = await mkdtemp(new URL('../.user-errors-', import.meta.url));
try {
  const compiler = new URL('bin/tsc', import.meta.resolve('typescript/package.json'));
  const result = spawnSync(process.execPath, [compiler.pathname, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--outDir', temporary, 'src/userErrors.ts'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const { browserOpenUrl, userError, runtimeError, generationError, outcomeReason, technicalReason } = await import(pathToFileURL(join(temporary, 'userErrors.js')));
  await test('browser error shapes have plain descriptions without echoing code', () => {
    const cases = [
      ["TypeError: null is not an object (evaluating pause.addEventListener)", "A script on this page looks for something that isn't there."],
      ["Cannot read properties of undefined (reading 'style')", "A script on this page looks for something that isn't there."],
      ['undefined is not an object (evaluating x.y)', "A script on this page looks for something that isn't there."],
      ['TypeError: pause.play is not a function', "A script on this page tries to do something that isn't available."],
      ['ReferenceError: pause is not defined', "A script on this page uses a name it doesn't recognise."],
      ["Can't find variable: pause", "A script on this page uses a name it doesn't recognise."],
      ["SyntaxError: Unexpected token '<'", 'A script on this page has a writing mistake.'],
      ['TypeError: Failed to fetch /private/page.js', "Something this page needs couldn't load."],
      ['NetworkError: Load failed', "Something this page needs couldn't load."],
      ['Something arbitrary\nat /private/page.js:2', 'A script on this page stopped working.'],
    ];
    for (const [raw, plain] of cases) assert.equal(runtimeError(raw), plain);
  });
  await test('generation diagnostics stay intact while their display is plain', () => {
    const diagnostic = 'Area 561acc85-1234-1234-1234-123456789012 ([data-desegnos-region=secret]): Region script root contains another region. Excerpt: <section id="private">';
    const job = { message: diagnostic };
    assert.equal(generationError(job.message), '“this request” would also change content outside this area.');
    assert.equal(job.message, diagnostic);
    assert.equal(generationError('Region script contains another region', 'Add scrolling chat', 'Box 20'), '“Add scrolling chat” would also change Box 20.');
    assert.equal(generationError('Editor IDs must be unique', 'Add chat'), 'Two items ended up with the same name, so editing one could change the other.');
    assert.equal(generationError('CSS selector reaches outside its box'), '“this request” would also change content outside this area.');
    assert.equal(generationError('Region script cannot be parsed', 'Add chat'), 'The AI did not produce a usable page for “Add chat”.');
    assert.equal(generationError('Invalid candidate schema'), 'The AI did not produce a usable page for “this request”.');
    assert.equal(generationError('Source revision changed'), 'The page was edited after the AI made this draft.');
  });
  await test('declared outcomes keep one plain sentence and drop app prefixes', () => {
    assert.equal(outcomeReason(`Area 561acc85-1234-1234-1234-123456789012: Which booking service should this use?`, 'Add booking'), 'Which booking service should this use?');
    assert.equal(outcomeReason('The logo file is missing', 'Add a logo'), 'The logo file is missing.');
    assert.equal(outcomeReason('Which colour? It can match the header. Use a strong accent.'), 'Which colour?');
    assert.equal(outcomeReason('', 'Add booking'), 'The AI could not complete “Add booking” as requested.');
    assert.equal(technicalReason('Which booking service should this use?'), false);
    assert.equal(technicalReason(`Area 561acc85-1234-1234-1234-123456789012: Region script root contains another region.`), true);
  });
  await test('native and caught errors hide internal details and keep plain instructions', () => {
    assert.equal(userError(''), '');
    assert.equal(userError('Select an item first.'), 'Select an item first.');
    assert.equal(userError("That's more than the AI can read at once. Untick a few files, or click 'Pick for me'."), "That's more than the AI can read at once. Untick a few files, or click 'Pick for me'.");
    assert.equal(userError('Confirm the intended audience before adding it to the brief.'), 'Confirm the intended audience before adding it to the brief.');
    assert.equal(userError(new Error('Choose an image first.')), 'Choose an image first.');
    assert.equal(userError({ code: 'unchanged', message: 'Permission denied: /home/private/site.desegnos' }), "Desegnos can't save here. Choose another folder.");
    assert.equal(userError('No space left on device: /private/file'), 'Your storage is full. Free some space and try again.');
    assert.equal(userError('The system clipboard could not be read: native error'), "Desegnos couldn't read the clipboard. Copy the item again.");
    assert.equal(userError('The result is preserved, but its project dossier could not be saved: /private'), "Your result is saved, but the project brief couldn't be updated.");
    assert.equal(userError('portfolio/index.html is too large to preview (90 MB).'), 'portfolio/index.html is too large to preview (90 MB).');
    assert.equal(userError("This page's code is too large to preview (17 MB)."), "This page's code is too large to preview (17 MB).");
    assert.equal(userError('The complete preview exceeds the 64 MiB local limit.'), 'This page is too large to preview. Try a smaller page.');
    assert.equal(userError('../private/index.html is too large to preview (90 MB).'), 'This page is too large to preview. Try a smaller page.');
    const portal = 'File dialogs need the desktop portal. Install xdg-desktop-portal and a backend such as xdg-desktop-portal-gtk, then try again.';
    assert.equal(userError(portal), portal);
    for (const raw of ['Error: bad\n at script.js:12', 'Unknown /private/file', 'Could not match #secret', 'Could not match .secret', 'Could not read private.json', 'Unknown 561acc85-1234-1234-1234-123456789012']) {
      const plain = userError(raw);
      assert.doesNotMatch(plain, /private|secret|script\.js|561acc85|Error:|\n/);
      assert.ok(plain.length <= 160);
    }
  });
  await test('update-check messages stay plain', () => {
    for (const message of ["Couldn't reach GitHub. Check your connection and try again.", 'GitHub is limiting requests. Try again later.', "Couldn't check for updates."]) assert.equal(userError(message), message);
  });
  await test('open-in-browser failures keep their local address for a copy action', async () => {
    const url = 'http://127.0.0.1:49152/8d4f0c2e/';
    assert.equal(browserOpenUrl({ message: `The site is running at ${url}`, url }), url);
    assert.equal(browserOpenUrl({ message: 'Only the Desegnos editor can open a website.' }), null);
    assert.equal(browserOpenUrl(new Error(url)), null);
    assert.equal(browserOpenUrl({ url: 'not a web address' }), null);
    const app = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');
    assert.match(app, /const url = browserOpenUrl\(error\);/);
    assert.match(app, /label: 'Copy link'/);
  });
  await test('runtime repair still sends the original observations', async () => {
    const source = await readFile(new URL('../src/Preview.tsx', import.meta.url), 'utf8');
    assert.match(source, /onClick: \(\) => fixRuntimeErrors\(observations\)/);
    assert.match(source, /message=\{runtimeError\(observations\[0\]\)\}/);
    const app = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');
    assert.match(app, /errors: errors\.slice\(0, 10\)\.map\(error => error\.slice\(0, 4096\)\)/);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
