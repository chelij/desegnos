import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { basename, join, resolve } from 'node:path';
import { chooseFile, chooseFolder, savedState, verifyArchive } from './file-dialog-workflow.mjs';
import { assertCanvasOnly } from './workflow.mjs';
import { waitFor, xpathString } from './webdriver.mjs';

const hash = value => createHash('sha256').update(value).digest('hex');
const workspace = driver => driver.invoke('get_workspace');
const analysis = driver => driver.invoke('get_identity_analysis');
const blocks = markdown => [...markdown.matchAll(/^(`{3,})json\n([\s\S]*?)\n\1\n/gm)].map(match => JSON.parse(match[2]));

async function snapshot(directory) {
  const files = {};
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    assert.equal(entry.isSymbolicLink(), false);
    if (entry.isDirectory()) for (const [name, digest] of Object.entries(await snapshot(path))) files[`${entry.name}/${name}`] = digest;
    else if (entry.isFile()) files[entry.name] = hash(await readFile(path));
  }
  return Object.fromEntries(Object.entries(files).sort(([a], [b]) => a.localeCompare(b)));
}

async function sourceFixtures(output) {
  const directory = join(output, 'Northstar website');
  await mkdir(join(directory, 'brand'), { recursive: true });
  await writeFile(join(directory, 'brand/IDENTITY.md'), '# Northstar Studio\n\nBrand name: Northstar Studio.\nTone: Clear, warm and confident.\nUse blue #225577, warm white #faf8f2 and ink #18232a. The existing star mark identifies the brand.\n');
  await writeFile(join(directory, 'brand/tokens.css'), ':root{--brand-blue:#225577;--brand-paper:#faf8f2;--brand-ink:#18232a;--font-body:Arial,sans-serif}\n');
  await writeFile(join(directory, 'brand/logo.svg'), '<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128"><title>Northstar Studio</title><path fill="#225577" d="M64 8L78 48H120L86 74L100 116L64 90L28 116L42 74L8 48H50Z"/></svg>');
  await writeFile(join(directory, 'index.html'), '<!doctype html><html lang="en"><head><title>Northstar Studio</title><link rel="stylesheet" href="brand/tokens.css"></head><body><img src="brand/logo.svg" alt="Northstar Studio"><h1>Ideas made clear.</h1><img src="work/photo-a.png" alt="Studio portrait"></body></html>');
  await writeFile(join(directory, 'projects.html'), '<title>Projects</title><h1>Projects</h1><img src="work/photo-b.png" alt="Robot build"><a href="/work/northstar">Northstar case study</a>');
  await writeFile(join(directory, '.env'), 'OPENAI_API_KEY=sk-synthetic-identity-private-do-not-submit-123456789\n');
  const archive = join(output, 'Northstar website.zip');
  // A real PNG and ZIP use only the platform's standard Python library.
  const fixture = spawnSync('python3', ['-c', String.raw`
import binascii, pathlib, struct, sys, zipfile, zlib
root, archive = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', binascii.crc32(kind + data) & 0xffffffff)
pixels = b''.join(b'\x00' + bytes([34,85,119,255]) * 128 for _ in range(128))
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB',128,128,8,6,0,0,0)) + chunk(b'IDAT',zlib.compress(pixels)) + chunk(b'IEND',b'')
(root/'brand/logo.png').write_bytes(png)
(root/'work').mkdir()
for name, color in [('photo-a.png', bytes([120,40,30,255])), ('photo-b.png', bytes([10,150,80,255]))]:
    pixels = b''.join(b'\x00' + color * 320 for _ in range(240))
    photo = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB',320,240,8,6,0,0,0)) + chunk(b'IDAT',zlib.compress(pixels)) + chunk(b'IEND',b'')
    (root/'work'/name).write_bytes(photo)

with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as output:
    for path in sorted(root.rglob('*')):
        if path.is_file(): output.write(path, path.relative_to(root).as_posix())
`, directory, archive], { encoding: 'utf8', timeout: 10_000 });
  if (fixture.error) throw fixture.error;
  assert.equal(fixture.status, 0, fixture.stderr);
  const astro = join(output, 'Astro brand');
  for (const subfolder of ['src/pages', 'src/layouts', 'src/styles', 'node_modules/example', 'dist', 'public/fonts', 'public/play/solopong']) await mkdir(join(astro, subfolder), { recursive: true });
  await writeFile(join(astro, 'src/styles/global.css'), ':root{--brand:#225577;font-family:Arial}');
  await writeFile(join(astro, 'src/layouts/Base.astro'), '<html><body><slot /></body></html>');
  for (let i = 0; i < 8; i++) await writeFile(join(astro, `src/pages/${i === 0 ? 'index' : `work-${i}`}.astro`), `<h1>Studio page ${i}</h1>\n${'<p>Our creative work and welcoming brand.</p>\n'.repeat(2200)}`);
  for (const name of ['node_modules/example/index.js', 'dist/index.html', 'public/fonts/Font-OFL.txt', 'public/play/solopong/game.js']) await writeFile(join(astro, name), 'Unrelated to the site brand');
  return { directory, archive, astro };
}

async function importedSource(driver, expected) {
  await waitFor(() => driver.execute(name => document.querySelector('.identity-source-name')?.textContent === name, expected), 'The actual native identity picker did not display its source');
  return driver.execute(() => [...document.querySelectorAll('.identity-files label')].map(label => label.textContent));
}

async function completedAnalysis(driver, statuses) {
  return waitFor(async () => { const value = await analysis(driver); return statuses.includes(value?.status) && value; }, `Identity analysis did not reach ${statuses.join('/')}`, 20_000);
}


export async function identityWorkflow(driver, physical, step, evidence, provider) {
  const output = join(resolve(evidence), 'identity'); await mkdir(output, { recursive: true });
  const fixtures = await sourceFixtures(output);
  const originals = { folder: await snapshot(fixtures.directory), zip: hash(await readFile(fixtures.archive)) };
  const originalProject = (await workspace(driver)).project;
  const sourceProjectPath = (await workspace(driver)).path;
  let sourceProjectHash, cancelled, completed, selectedPng, finalProject;
  const generationCount = provider.generationRequests().length;
  const identityCount = provider.identityRequests().length;
  await physical.initialize(output);

  let blankProject;
  await step('New projects always start blank; Use a site or brand opens from the main window', async () => {
    await driver.button('Save');
    await savedState(driver, sourceProjectPath);
    sourceProjectHash = hash(await readFile(sourceProjectPath));
    await driver.button('New project');
    assert.equal(await driver.execute(() => !!document.querySelector('dialog[open] input[name="projectName"]')), false);
    await driver.ready('//dialog[@open]//button[.//strong[normalize-space(.)="Use my existing brand"]]', 'xpath');
    await driver.click('//dialog[@open]//button[.//strong[normalize-space(.)="Blank website"]]', 'xpath');
    await driver.fill('Project name', 'Imported Northstar website');
    await driver.button('Web');
    assert.deepEqual(await driver.execute(() => [...document.querySelector('dialog[open] select').options].map(option => option.value)), ['1440x900', '1920x1080', '1536x864', '1366x768', '1280x720', '2560x1440', 'custom']);
    await driver.select('Resolution', '1536x864');
    assert.equal(await driver.execute(() => !!document.querySelector('dialog[open] .appearance-choice')), false);
    await driver.button('Create project');
    blankProject = await waitFor(async () => { const value = (await workspace(driver)).project; return value?.id !== originalProject.id && value; }, 'The blank project did not open');
    // The source project stays open in its own tab group.
    const openIds = (await workspace(driver)).openProjects.map(open => open.id); assert.ok(openIds.includes(originalProject.id) && openIds.at(-1) === blankProject.id, JSON.stringify(openIds));
    await driver.button('Use a site or brand…');
    await driver.ready('dialog[open][aria-label="Use a site or brand"] .import-context');
    assert.equal(await driver.execute(() => [...document.querySelectorAll('dialog[open] button')].find(button => button.textContent.trim() === 'Add to project')?.disabled), true);
    assert.match(await driver.execute(() => document.querySelector('.import-context').textContent), /1\. Where is your brand\?.*Read your brand first/s);
    await physical.captureWindow(join(output, 'new-project-choice-window.png'));
  });

  await step('Native identity pickers review website folders, ZIPs and Desegnos projects without writing or contacting AI', async () => {
    await chooseFile(physical, () => driver.button('Choose folder'), null, output, 'cancel-folder-picker', true);
    assert.equal(await driver.execute(() => document.querySelector('.identity-source-name')?.textContent ?? null), null);
    await chooseFolder(physical, () => driver.button('Choose folder'), fixtures.directory, output, 'website-folder-picker');
    await driver.button('Use it for my brand only');
    const folderNames = await importedSource(driver, 'Northstar website');
    for (const name of ['brand/IDENTITY.md', 'brand/tokens.css', 'brand/logo.png', 'brand/logo.svg', 'index.html']) assert.ok(folderNames.some(value => value.includes(name)), `The folder review omitted ${name}`);
    assert.equal(folderNames.some(value => value.includes('.env')), false);
    assert.equal(await driver.execute(() => document.querySelector('.identity-skipped')?.textContent.includes('.env')), true);
    await chooseFile(physical, () => driver.button('Choose project or ZIP'), fixtures.archive, output, 'website-zip-picker');
    const zipNames = await importedSource(driver, 'Northstar website.zip');
    assert.deepEqual(zipNames, folderNames);
    await chooseFile(physical, () => driver.button('Choose project or ZIP'), sourceProjectPath, output, 'desegnos-project-picker');
    const projectNames = await importedSource(driver, basename(sourceProjectPath));
    assert.ok(projectNames.some(value => value.includes('PROJECT.md')));
    assert.ok(projectNames.some(value => value.includes('styles.css')));
    assert.deepEqual((await workspace(driver)).project, blankProject);
    assert.equal(provider.generationRequests().length, generationCount);
    assert.equal(provider.identityRequests().length, identityCount);
    assert.deepEqual(await snapshot(fixtures.directory), originals.folder);
    assert.equal(hash(await readFile(fixtures.archive)), originals.zip);
    assert.equal(hash(await readFile(sourceProjectPath)), sourceProjectHash);
    await chooseFolder(physical, () => driver.button('Choose folder'), fixtures.directory, output, 'analysis-folder-picker');
    await driver.button('Use it for my brand only');
    await importedSource(driver, 'Northstar website');
  });

  await step('Astro sources start with a fitting brand selection; manual over-selection is blocked and Pick for me restores it', async () => {
    await chooseFolder(physical, () => driver.button('Choose folder'), fixtures.astro, output, 'astro-folder-picker');
    await driver.button('Use it for my brand only');
    const names = await importedSource(driver, 'Astro brand');
    assert.ok(names[0].includes('src/styles/global.css'));
    assert.ok(names[1].includes('src/layouts/Base.astro'));
    assert.ok(names.some(name => name.includes('src/pages/index.astro')));
    assert.ok(!names.some(name => /OFL|game\.js|node_modules|dist\//.test(name)));
    await driver.ready('//button[normalize-space(.)="Read my brand"]', 'xpath');
    const automatic = await driver.execute(() => [...document.querySelectorAll('.identity-files input')].map(input => input.checked));
    assert.ok(automatic[0] && automatic[1]); assert.ok(automatic.some(value => !value));
    assert.equal(await driver.execute(() => document.querySelector('.identity-file-review').open), false);
    assert.equal(await driver.execute(() => [...document.querySelectorAll('.import-context button')].some(button => /AI settings/.test(button.textContent))), false);
    assert.match(await driver.execute(() => document.querySelector('.identity-source-name').parentElement.textContent), /pages, 1 stylesheet/);
    const headingGap = await driver.execute(() => { const heading = document.querySelector('.identity-read-heading h2').getBoundingClientRect(); const summary = document.querySelector('.identity-source-name').parentElement.getBoundingClientRect(); return summary.top - heading.bottom; });
    assert.ok(headingGap >= 0 && headingGap <= 2, `Unexpected gap below Read it: ${headingGap}`);
    assert.doesNotMatch(await driver.execute(() => document.querySelector('.import-context').textContent), /We'll look at|most useful parts|Bring your brand/);
    assert.match(await driver.execute(() => document.querySelector('.import-context').textContent), /Sent to OpenAI \(API key\)/);
    await physical.captureWindow(join(output, 'astro-simple-import-window.png'));
    await driver.click('.identity-file-review > summary');
    for (let i = 0; i < automatic.length; i++) if (!automatic[i]) await driver.click(`.identity-files label:nth-child(${i + 1}) input`);
    const explanation = "That's more than the AI can read at once. Untick a few files, or click 'Pick for me'.";
    await waitFor(() => driver.execute(text => [...document.querySelectorAll('.identity-import [role="alert"]')].some(item => item.textContent === text), explanation), 'Too many files did not show the friendly recovery message');
    assert.equal(await driver.execute(() => [...document.querySelectorAll('.import-context button')].find(button => button.textContent === 'Read my brand').disabled), true);
    assert.equal(provider.identityRequests().length, identityCount);
    await driver.button('Pick for me');
    await driver.ready('//button[normalize-space(.)="Read my brand"]', 'xpath');
    assert.deepEqual(await driver.execute(() => [...document.querySelectorAll('.identity-files input')].map(input => input.checked)), automatic);
    await driver.button('Pick for me');
    await driver.ready('//button[normalize-space(.)="Read my brand"]', 'xpath');
    assert.equal(await driver.execute(() => !!document.querySelector('.identity-import [role="alert"]')), false);
    await chooseFolder(physical, () => driver.button('Choose folder'), fixtures.directory, output, 'restore-brand-folder-picker');
    await driver.button('Use it for my brand only');
    await importedSource(driver, 'Northstar website');
  });

  await step('Website address import keeps pages and styles, fits them locally and leaves file details closed', async () => {
    await driver.fill('Website address', `${provider.endpoint.replace(/\/v1$/, '')}/brand-site/`);
    await driver.button('Open website');
    const names = await importedSource(driver, '127.0.0.1');
    assert.ok(names.some(name => name.includes('index.html')));
    assert.ok(names.some(name => name.includes('about.html')));
    assert.ok(names.some(name => name.includes('.css')));
    await driver.ready('//button[normalize-space(.)="Read my brand"]', 'xpath');
    assert.equal(await driver.execute(() => document.querySelector('.identity-file-review').open), false);
    assert.equal(provider.identityRequests().length, identityCount);
    // A bare address is supported. Leaving it in the source field must not make
    // the browser block the later Add to project form submission.
    await driver.fill('Website address', 'example.com');
    await chooseFolder(physical, () => driver.button('Choose folder'), fixtures.directory, output, 'restore-folder-after-url');
    await driver.button('Use it for my brand only');
    await importedSource(driver, 'Northstar website');
  });

  await step('Identity analysis shows progress, rejects duplicate sending and cancels without creating a website', async () => {
    provider.setMode('waiting');
    await driver.button('Read my brand');
    cancelled = await waitFor(async () => { const value = await analysis(driver); return value?.status === 'analyzing' && provider.identityRequests().some(record => record.metadata.jobId === value.jobId) && value; }, 'The frozen identity request did not reach the controlled provider');
    await driver.ready('.identity-progress[role="status"]');
    assert.equal(await driver.execute(() => document.querySelector('.import-context').textContent.includes('Read your brand first')), false);
    assert.match(await driver.execute(() => document.querySelector('.identity-progress').textContent), /Reading your brand…/);
    assert.equal(await driver.execute(() => !!document.querySelector('dialog[open] button[disabled][type="submit"]')), true);
    const before = provider.identityRequests().length;
    await assert.rejects(() => driver.invoke('analyze_identity', { sourceId: cancelled.sourceId, fileIds: cancelled.fileIds }), /already|running|finish|analysis/i);
    assert.equal(provider.identityRequests().length, before);
    await physical.captureWindow(join(output, 'identity-progress-window.png'));
    await driver.button('Stop reading');
    const value = await completedAnalysis(driver, ['cancelled']);
    assert.equal(value.candidate ?? null, null);
    assert.deepEqual((await workspace(driver)).project, blankProject);
    provider.release(cancelled.jobId);
  });

  await step('An invalid identity reply leaves a visible failure and never creates or changes a project', async () => {
    provider.setMode('malformed'); await driver.button('Read my brand');
    const value = await completedAnalysis(driver, ['failed', 'unknown']);
    assert.equal(value.candidate ?? null, null); assert.ok(value.error || value.message);
    await waitFor(() => driver.execute(() => !!document.querySelector('.identity-import [role="alert"]')?.textContent), 'The identity failure was hidden from the user');
    assert.equal(await driver.execute(() => document.querySelector('dialog[open] button[type="submit"]')?.disabled), true);
    assert.deepEqual((await workspace(driver)).project, blankProject);
  });

  await step('Identity analysis freezes reviewed files and image provenance, then offers findings and portable logos to choose', async () => {
    provider.setMode('complete', { chunkDelayMs: 2, chunkSize: 512 }); await driver.button('Read my brand');
    completed = await completedAnalysis(driver, ['complete']);
    await driver.ready('.identity-result[aria-label="Brand review"]');
    assert.match(await driver.execute(() => document.querySelector('.identity-result').textContent), /3\. Choose what to keep/);
    const record = provider.identityRequests().find(record => record.metadata.jobId === completed.jobId);
    assert.ok(record?.authenticated);
    assert.equal(record.metadata.purpose, 'identityAnalysis');
    assert.equal(record.markdown.includes('sk-synthetic-identity-private-do-not-submit'), false);
    assert.equal(record.metadata.requirementsSha256, hash(record.markdown));
    const files = blocks(record.markdown).filter(value => value?.fileId);
    assert.deepEqual(files.map(file => file.fileId).sort(), [...completed.fileIds].sort());
    assert.ok(files.some(file => file.name === 'brand/tokens.css' && file.text.includes('#225577')));
    assert.ok(files.some(file => file.name === 'brand/IDENTITY.md' && file.text.includes('Northstar Studio')));
    assert.equal(record.body.input[0].content.filter(value => value.type === 'input_image').length, 3);
    assert.equal(files.find(file => file.asset?.mime === 'image/svg+xml').asset.analysisImage, null);
    assert.equal(files.find(file => file.asset?.mime === 'image/png').asset.analysisImage, 1);
    assert.deepEqual(completed.source.files.map(file => file.id).sort(), [...completed.fileIds].sort());
    assert.deepEqual(await driver.execute(() => [...document.querySelectorAll('.identity-section-heading h3')].map(node => node.textContent)), ['Colours', 'Fonts', 'Logos', 'Style & tone', 'Pages', 'Other']);
    assert.deepEqual(await driver.execute(() => [...document.querySelectorAll('.identity-swatch-card small')].map(node => node.textContent)), ['#225577', '#faf8f2', '#18232a']);
    assert.equal(await driver.execute(() => [...document.querySelectorAll('.identity-font-specimen')].every(node => /Arial|Georgia/.test(getComputedStyle(node).fontFamily))), true);
    assert.deepEqual(await driver.execute(() => [...document.querySelectorAll('section[aria-label="Fonts"] .identity-card > small')].map(node => node.textContent)), ['Headings', 'Body']);
    assert.equal(await driver.execute(() => !!document.querySelector('.identity-card-info[open]')), false);
    assert.equal(await driver.execute(() => document.querySelector('input[aria-label="Keep Possible audience"]').checked), false);
    assert.equal(await driver.execute(() => document.querySelector('input[aria-label="Keep Possible audience"]').closest('label').querySelector('.identity-unsure').textContent), 'Not sure');
    // Clicking anywhere on a swatch toggles its original finding, including sibling swatches.
    await driver.click('.identity-swatch');
    assert.equal(await driver.execute(() => document.querySelectorAll('.identity-swatch-card input:checked').length), 0);
    await driver.button('Keep all colours');
    assert.equal(await driver.execute(() => document.querySelectorAll('.identity-swatch-card input:checked').length), 3);
    await driver.button('Keep no fonts');
    assert.equal(await driver.execute(() => document.querySelectorAll('section[aria-label="Fonts"] input:checked').length), 0);
    await driver.button('Keep all fonts');
    assert.equal(await driver.execute(() => document.querySelectorAll('section[aria-label="Fonts"] input:checked').length), 2);
    // Real keyboard input toggles the focused native checkbox and draws a visible focus ring.
    await driver.execute(() => document.querySelector('.identity-swatch-card input').focus());
    await physical.keys('space');
    await waitFor(() => driver.execute(() => document.querySelectorAll('.identity-swatch-card input:checked').length === 0), 'Space did not toggle the focused swatch');
    assert.equal(await driver.execute(() => document.querySelectorAll('.identity-swatch-card input:checked').length), 0);
    assert.equal(await driver.execute(() => getComputedStyle(document.activeElement.closest('label')).outlineStyle), 'solid');
    await driver.button('Keep all colours');
    await driver.click('.identity-card-info > summary[aria-label="Details: Tone"]');
    assert.match(await driver.execute(() => document.querySelector('.identity-card-info[open]').textContent), /Clear, warm and confident.*Source.*IDENTITY.md/s);
    await driver.click('.identity-card-info > summary[aria-label="Details: Tone"]');
    // Keep the original fixture's three decisions for the persistence assertions below.
    await driver.button('Keep no fonts');
    await driver.click('input[aria-label="Keep Visual style"] + .identity-check');
    await driver.click('input[aria-label="Keep Writing voice"] + .identity-check');
    selectedPng = completed.source.files.find(file => file.asset?.mime === 'image/png').asset;
    for (const asset of completed.source.files.flatMap(file => file.asset ? [file.asset] : []).filter(asset => asset.id !== selectedPng.id)) {
      await driver.click(`//label[contains(@class,"identity-card") and .//img[@alt=${xpathString(asset.name)}]]`, 'xpath');
    }
    assert.deepEqual(completed.source.pages.map(page => page.path), ['/', '/projects']);
    assert.deepEqual(await driver.execute(() => [...document.querySelectorAll('.identity-page-path')].map(node => node.textContent)), ['/', '/projects']);
    assert.equal(await driver.execute(() => document.querySelectorAll('.identity-page-strip img').length), 2);
    assert.equal(await driver.execute(() => document.querySelectorAll('.identity-page-work > div').length), 1);
    assert.equal(await driver.execute(() => document.querySelectorAll('.identity-page-heading input:checked').length), 2);
    await driver.click('input[aria-label="Keep page: Projects"]');
    assert.equal(await driver.execute(() => document.querySelectorAll('.identity-page-heading input:checked').length), 1);
    await driver.click('input[aria-label="Keep page: Projects"]');
    await driver.click('.identity-page-card:nth-child(2) .identity-page-details > summary');
    await driver.click('input[aria-label="Keep photo: work/photo-b.png on Projects"]');
    await driver.click('input[aria-label="Keep work: Northstar case study on Projects"]');
    assert.equal(await driver.execute(() => document.querySelectorAll('.identity-page-details input:checked').length), 1);
    assert.equal(await driver.execute(() => document.querySelector('.import-context button[type="submit"]').disabled), false);
    await physical.captureWindow(join(output, 'identity-review-window.png'));
    await writeFile(join(output, 'identity-http-request.json'), JSON.stringify(record, null, 2), { mode: 0o600 });
    await writeFile(join(output, 'identity-analysis.json'), JSON.stringify(completed, null, 2), { mode: 0o600 });
  });

  await step('Visual brand review fits narrow dialogs in light and dark themes', async () => {
    const original = await driver.execute(() => ({ theme: document.documentElement.getAttribute('data-theme'), width: document.querySelector('dialog[open]').style.width }));
    try {
      for (const theme of ['light', 'dark']) {
        await driver.execute(value => {
          document.documentElement.setAttribute('data-theme', value);
          const dialog = document.querySelector('dialog[open]');
          dialog.style.width = '340px'; dialog.scrollTop = 0;
        }, theme);
        await driver.executeAsync(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
        const layout = await driver.execute(() => {
          const dialog = document.querySelector('dialog[open]');
          const review = document.querySelector('.identity-result');
          const bounds = review.getBoundingClientRect();
          const cards = [...review.querySelectorAll('.identity-card, .identity-page-card')];
          return { overflow: dialog.scrollWidth > dialog.clientWidth, outside: cards.some(card => card.getBoundingClientRect().right > bounds.right + 1), columns: getComputedStyle(document.querySelector('.identity-review-grid')).gridTemplateColumns.split(' ').length,
            background: getComputedStyle(cards[0]).backgroundColor, text: getComputedStyle(cards[0]).color, pageRows: [...review.querySelectorAll('.identity-page-heading, .identity-page-details label')].every(label => getComputedStyle(label).flexDirection === 'row') };
        });
        assert.equal(layout.overflow, false); assert.equal(layout.outside, false); assert.equal(layout.columns, 1); assert.equal(layout.pageRows, true);
        assert.notEqual(layout.background, layout.text);
        await physical.captureWindow(join(output, `identity-review-narrow-${theme}.png`));
      }
    } finally {
      await driver.execute(value => {
        if (value.theme === null) document.documentElement.removeAttribute('data-theme'); else document.documentElement.setAttribute('data-theme', value.theme);
        document.querySelector('dialog[open]').style.width = value.width;
      }, original);
    }
  });

  await step('Selected identity is added to the open blank website and preserves only chosen decisions and the actual logo', async () => {
    await driver.button('Add to project');
    finalProject = await waitFor(async () => { const value = (await workspace(driver)).project; return value?.id === blankProject.id && value.importedIdentity && value; }, 'The imported context was not added to the open project');
    assert.equal(finalProject.name, 'Imported Northstar website'); assert.equal(finalProject.schemaVersion, 8);
    assert.equal(finalProject.appearance, 'light'); assert.equal(finalProject.pages[0].appearance, 'light');
    assert.deepEqual(finalProject.viewport, { width: 1536, height: 864 });
    assert.equal(finalProject.pages.length, 2); assert.equal(finalProject.pages[1].path, '/projects'); assert.equal(finalProject.pages[1].name, 'Projects');
    assert.equal(finalProject.importedIdentity.pages[0].photoIds.length, 1); assert.equal(finalProject.importedIdentity.pages[1].photoIds.length, 0); assert.equal(finalProject.importedIdentity.pages[1].content.length, 0); assert.equal(finalProject.pages[0].content, null); assert.deepEqual(finalProject.pages[0].regions, []);
    assert.equal(finalProject.latestGeneration, null); assert.deepEqual(finalProject.appliedJobIds, []);
    assert.equal(finalProject.brief.brandName, 'Northstar Studio'); assert.equal(finalProject.brief.tone, 'Clear, warm and confident.');
    assert.deepEqual(finalProject.decisions.map(value => value.topic), ['Brand name', 'Brand palette', 'Tone']);
    assert.equal(finalProject.importedIdentity.sourceSha256, completed.source.sha256);
    assert.deepEqual(finalProject.importedIdentity.assets, [selectedPng]);
    assert.equal(finalProject.importedIdentity.photos.length, 1);
    assert.equal(finalProject.importedIdentity.photos[0].name, 'work/photo-a.png');
    assert.deepEqual(finalProject.importedIdentity.content ?? [], []);
    assert.equal(await driver.execute(() => document.querySelectorAll('[data-testid="canvas"] textarea, .static-preview iframe').length), 0);
    assert.equal(await driver.invoke('get_generation', { projectId: finalProject.id }), null);
    assert.equal(provider.generationRequests().length, generationCount);
    const notes = await driver.invoke('get_project_notes');
    for (const expected of ['Northstar Studio', '#225577', selectedPng.name]) assert.ok(notes.includes(expected));
    assert.equal(notes.includes('Possible audience'), false); assert.equal(notes.includes('Writing voice'), false);
    await assertCanvasOnly(driver);
    await physical.captureWindow(join(output, 'identity-blank-workspace-window.png'));
  });

  await step('Saved identity reopens exactly and future generation requirements include its approved identity and real image bytes', async () => {
    const destination = (await driver.invoke('get_workspace')).path;
    await driver.button('Save');
    await savedState(driver, destination);
    await verifyArchive(destination, finalProject, await driver.invoke('get_project_notes'));
    await chooseFile(physical, () => driver.button('Open project'), sourceProjectPath, output, 'identity-temporary-open');
    await waitFor(async () => (await workspace(driver)).project?.id === originalProject.id, 'The original saved project did not reopen');
    await chooseFile(physical, () => driver.button('Open project'), destination, output, 'identity-project-reopen');
    await waitFor(async () => (await workspace(driver)).project?.id === finalProject.id, 'The identity project did not reopen');
    assert.deepEqual((await workspace(driver)).project, finalProject);
    await driver.press('[data-testid="canvas"]', '\uE007');
    await driver.fill('Request 1', 'Create a welcoming homepage using the imported Northstar identity and its approved logo.');
    await waitFor(async () => (await workspace(driver)).project.pages[0].regions[0]?.prompt.includes('Northstar'), 'The future-generation identity request did not persist');
    const frozen = await driver.invoke('prepare_requirements');
    assert.match(frozen.markdown, /^Requirements format: 17$/m);
    const catalogue = blocks(frozen.markdown).find(block => block?.assets && block?.pages);
    assert.equal(catalogue.photos.length, 0); assert.equal(catalogue.pages[0].path, '/'); assert.equal(catalogue.pages[0].photos.length, 1); assert.equal(catalogue.pages[0].photos[0].name, 'work/photo-a.png');
    assert.ok(frozen.markdown.includes('#225577')); assert.ok(frozen.markdown.includes('Clear, warm and confident.'));
    assert.ok(!frozen.markdown.includes(selectedPng.dataUri)); assert.ok(frozen.markdown.includes(`desegnos-asset:${selectedPng.id}`)); assert.ok(frozen.markdown.includes(selectedPng.sha256));
    assert.equal(provider.generationRequests().length, generationCount);
    assert.deepEqual(await snapshot(fixtures.directory), originals.folder);
    assert.equal(hash(await readFile(fixtures.archive)), originals.zip);
    assert.equal(hash(await readFile(sourceProjectPath)), sourceProjectHash);
    assert.deepEqual(provider.errors(), []);
    await writeFile(join(output, 'identity-requirements.md'), frozen.markdown, { mode: 0o600 });
    await writeFile(join(output, 'read-only-source-proof.json'), JSON.stringify({ before: originals, after: { folder: await snapshot(fixtures.directory), zip: hash(await readFile(fixtures.archive)) }, sourceProjectSha256: sourceProjectHash }, null, 2));
    finalProject = (await workspace(driver)).project;
  });
  return finalProject;
}
