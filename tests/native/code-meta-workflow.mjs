import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { codeMetaFixture } from './code-meta-fixture.mjs';
import { waitFor } from './webdriver.mjs';

export async function codeMetaWorkflow(driver, physical, step, evidence) {
  let project;
  const metrics = { toggles: [], memory: [] };
  await step('Create a 17 KB site with six 270 KB stored pictures and a page script', async () => {
    if ((await driver.invoke('get_workspace')).project) await driver.button('New project');
    await driver.button('Blank website'); await driver.fill('Project name', 'Metadata toggle'); await driver.button('Create project');
    project = (await driver.invoke('get_workspace')).project;
    const { pictures, ...source } = codeMetaFixture();
    // Keep each WebDriver request below its body limit, just as separate imports do.
    source.html = source.html.replace(/data:image\/png;base64,[^"]+/g, (_, index) => `https://example.org/picture-${index}`);
    for (const picture of pictures) {
      source.html = source.html.replace(/https:\/\/example.org\/picture-\d+/, picture);
      project = (await driver.invoke('edit_page_source', { projectId: project.id, pageId: project.homePageId, revision: project.revision, ...source })).project;
      source.html = project.pages[0].content.html;
    }
    assert.equal(Object.keys(project.pictures).length, 6);
    const html = project.pages[0].content.html;
    assert.ok(html.length < 22000); assert.doesNotMatch(html, /data:image\//);
    await driver.button('Save');
    await driver.execute(() => localStorage.setItem('desegnos.code-show-meta', 'yes'));
    await driver.button('Split');
    await physical.initialize(evidence);
    await driver.ready('[contenteditable="true"][aria-label="HTML code"]');
    assert.equal(await driver.execute(() => document.querySelector('.code-meta-toggle input').checked), true);
  });
  await step('Mounted HTML editor toggles attributes both ways within one second without growing its document or decorations', async () => {
    const memory = async () => {
      const children = async pid => {
        const ids = (await readFile(`/proc/${pid}/task/${pid}/children`, 'utf8').catch(() => '')).trim().split(/\s+/).filter(Boolean);
        return [pid, ...(await Promise.all(ids.map(children))).flat()];
      };
      return Promise.all((await children(physical.pid)).map(async pid => {
        const status = await readFile(`/proc/${pid}/status`, 'utf8').catch(() => '');
        return { pid, name: status.match(/^Name:\s+(.+)/m)?.[1], rssKB: Number(status.match(/^VmRSS:\s+(\d+)/m)?.[1] ?? 0) };
      }));
    };
    metrics.memory.push(await memory());
    for (let i = 0; i < 20; i++) {
      // Exercise native focus/selection and scrolling, then use a real OS click.
      await driver.execute(() => {
        const content = document.querySelector('[aria-label="HTML code"]');
        content.focus();
        const selection = getSelection(); selection.selectAllChildren(content); selection.collapseToEnd();
        const scroll = content.closest('.cm-scroller'); scroll.scrollTop = scroll.scrollHeight;
        window.__codeMetaMetric = null;
        document.querySelector('.code-meta-toggle input').addEventListener('click', event => {
          const start = performance.now();
          requestAnimationFrame(() => requestAnimationFrame(() => {
            const editor = document.querySelector('[aria-label="HTML code"]');
            window.__codeMetaMetric = { ms: performance.now() - start, trusted: event.isTrusted,
              shown: document.querySelector('.code-meta-toggle input').checked, text: editor.cmTile.root.view.state.doc.toString(),
              decorations: document.querySelectorAll('.code-line').length, nodes: document.querySelectorAll('.cm-editor *').length };
          }));
        }, { once: true });
        return true;
      });
      await physical.click(await physical.center('.code-meta-toggle input'));
      const result = await waitFor(() => driver.execute(() => window.__codeMetaMetric), 'The toggle did not paint');
      metrics.toggles.push({ ...result, text: result.text.length });
      assert.equal(result.trusted, true);
      assert.ok(result.ms < 1000, `Toggle took ${result.ms} ms`);
      assert.equal(result.shown, i % 2 === 1);
      assert.doesNotMatch(result.text, /data:image\//); assert.ok(result.text.length < 22000);
      assert.ok(result.decorations <= 2000); assert.ok(result.nodes < 12000);
      if (i === 9 || i === 19) metrics.memory.push(await memory());
    }
    const total = sample => sample.reduce((sum, process) => sum + process.rssKB, 0);
    assert.ok(total(metrics.memory.at(-1)) - total(metrics.memory[0]) < 64 * 1024, 'Native process memory grew by more than 64 MB');
    await writeFile(join(evidence, 'code-meta-metrics.json'), JSON.stringify(metrics, null, 2));
    assert.deepEqual((await driver.invoke('get_workspace')).project.pages[0].content, project.pages[0].content);
  });
  return project;
}
