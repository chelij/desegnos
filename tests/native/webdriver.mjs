import { setTimeout as delay } from 'node:timers/promises';

const elementKey = 'element-6066-11e4-a52e-4f735466cecf';

export async function waitFor(read, message, timeout = 10_000) {
  const end = Date.now() + timeout;
  let last;
  do {
    last = await read();
    if (last) return last;
    await delay(50);
  } while (Date.now() < end);
  throw new Error(`${message}; last result: ${JSON.stringify(last)}`);
}

export function xpathString(value) {
  if (!value.includes("'")) return `'${value}'`;
  if (!value.includes('"')) return `"${value}"`;
  return `concat(${value.split("'").map(part => `'${part}'`).join(',"\'",')})`;
}

export class WebDriver {
  constructor(port) { this.base = `http://127.0.0.1:${port}`; }

  async request(method, path, body, timeout = this.requestTimeout ?? 15_000) {
    const response = await fetch(this.base + path, {
      method, headers: { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(timeout),
    });
    const result = await response.json();
    if (!response.ok || result.value?.error) {
      throw new Error(`${method} ${path}: ${result.value?.error ?? response.status}: ${result.value?.message ?? JSON.stringify(result)}`);
    }
    return result.value;
  }

  async start(isAlive) {
    await waitFor(async () => {
      if (!isAlive()) throw new Error('The native app exited before its WebDriver became ready. Read app-1.log.');
      try { return (await this.request('GET', '/status', undefined, 1500))?.ready; }
      catch (error) {
        if (error.cause?.code === 'ECONNREFUSED' || error.name === 'TimeoutError') return false;
        throw error;
      }
    }, 'The embedded native WebDriver did not become ready', 30_000);
    const session = await this.request('POST', '/session', {
      capabilities: { alwaysMatch: { browserName: 'wry', 'wdio:tauriServiceOptions': { windowLabel: 'editor' } } },
    });
    this.session = session.sessionId;
    this.capabilities = session.capabilities;
    await this.command('POST', '/timeouts', { script: 10_000, implicit: 0, pageLoad: 15_000 });
    return this;
  }

  command(method, path, body) { return this.request(method, `/session/${this.session}${path}`, body); }
  async stop() { if (this.session) await this.command('DELETE', ''); this.session = null; }

  execute(fn, ...args) {
    return this.command('POST', '/execute/sync', { script: `return (${fn.toString()})(...arguments);`, args });
  }

  async executeAsync(fn, ...args) {
    const result = await this.command('POST', '/execute/async', {
      script: `const done = arguments[arguments.length - 1];
        Promise.resolve((${fn.toString()})(...Array.prototype.slice.call(arguments, 0, -1)))
          .then(value => done({ ok: true, value }), error => done({ ok: false, error: String(error) }));`, args,
    });
    if (!result?.ok) throw new Error(`Native webview script failed: ${result?.error ?? JSON.stringify(result)}`);
    return result.value;
  }

  invoke(command, args = {}) {
    return this.executeAsync((name, parameters) => window.__TAURI__.core.invoke(name, parameters), command, args);
  }

  async reload() {
    const errors = await this.execute(() => window.__desegniTestErrors ?? []);
    await this.execute(() => { window.__beforeWorkflowReload = true; setTimeout(() => location.reload(), 0); });
    await waitFor(async () => {
      try { return await this.execute(() => !window.__beforeWorkflowReload && !!document.querySelector('[data-testid="canvas"]')); }
      catch { return false; }
    }, 'The native editor did not reload', 12_000);
    await this.execute(previous => {
      window.__desegniTestErrors = previous;
      window.addEventListener('error', event => window.__desegniTestErrors.push({ kind: 'error', message: event.message }));
      window.addEventListener('unhandledrejection', event => window.__desegniTestErrors.push({ kind: 'rejection', message: String(event.reason) }));
      const original = console.error;
      console.error = (...values) => { window.__desegniTestErrors.push({ kind: 'console', message: values.map(String).join(' ') }); original.apply(console, values); };
    }, errors);
  }

  async find(value, using = 'css selector') {
    return (await this.command('POST', '/element', { using, value }))[elementKey];
  }

  async all(value, using = 'css selector') { return this.command('POST', '/elements', { using, value }); }
  async ready(value, using = 'css selector') {
    await waitFor(() => this.execute((selector, strategy) => {
      const node = strategy === 'xpath'
        ? document.evaluate(selector, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue
        : document.querySelector(selector);
      if (!(node instanceof Element) || !node.getClientRects().length || getComputedStyle(node).visibility !== 'visible' || node.matches(':disabled') || node.closest('[inert], dialog:not([open])')) return false;
      if (node instanceof HTMLLabelElement && node.control?.matches(':disabled')) return false;
      const top = [...document.querySelectorAll('dialog[open]')].at(-1);
      return !top || top.contains(node);
    }, value, using), `The target did not become visible and enabled: ${value}`, 15_000);
    return this.find(value, using);
  }
  async click(value, using = 'css selector') {
    await this.command('POST', `/element/${await this.ready(value, using)}/click`, {});
  }
  async menu(path, rootAriaLabel = 'Editor menu') {
    if (!Array.isArray(path) || !path.length) throw new Error('A menu action needs its visible item path');
    await this.executeAsync(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
    const trigger = `//button[@aria-label=${xpathString(rootAriaLabel)} and @aria-haspopup="menu"]`;
    const expanded = await this.execute(selector => document.evaluate(selector, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue?.getAttribute('aria-expanded'), trigger);
    if (expanded !== 'true') await this.click(trigger, 'xpath');
    let menuId = await waitFor(() => this.execute(selector => document.evaluate(selector, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue?.getAttribute('aria-controls'), trigger), `${rootAriaLabel} did not open its menu`);
    for (let index = 0; index < path.length; index += 1) {
      const selector = `//ul[@role="menu" and @id=${xpathString(menuId)}]/li/button[@aria-label=${xpathString(path[index])} and (@role="menuitem" or @role="menuitemcheckbox" or @role="menuitemradio")]`;
      await this.click(selector, 'xpath');
      if (index + 1 < path.length) menuId = await waitFor(() => this.execute(value => document.evaluate(value, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue?.getAttribute('aria-controls'), selector), `${path[index]} did not open its submenu`);
    }
  }

  page(pageId) {
    return this.click(`//div[@role="tablist" and @aria-label="Project pages"]//button[@role="tab" and @id=${xpathString(`page-tab-${pageId}`)}]`, 'xpath');
  }

  async button(text) {
    const visible = await this.execute(label => {
      const top = [...document.querySelectorAll('dialog[open]')].at(-1);
      return [...document.querySelectorAll('button')].some(button => (button.getAttribute('aria-label') === label || button.textContent.trim() === label)
        && button.getClientRects().length && getComputedStyle(button).visibility === 'visible' && !button.matches(':disabled') && (!top || top.contains(button)));
    }, text);
    if (text === 'Fuzzy' || text === 'Strict') return this.menu(['AI style', text === 'Strict' ? 'Precise — follow descriptions and placement closely' : 'Flexible — interpret and coordinate']);
    const routes = {
      'Add page': ['Add page'], 'AI settings':['AI settings'],
      'New project': ['New project'], 'Open project': ['Open project'], Save: ['Save'], 'Save as': ['Save as'],
      'Project dossier': ['Project brief'], 'Recover projects': ['Recover unsaved projects'],

      'Page settings': ['Page settings'], 'Remove page': ['Remove page'],
    };
    if (!visible && routes[text]) return this.menu(routes[text]);
    // Keep the target tied to its accessible name while the UI changes around it.
    // A global button ordinal can point elsewhere when selection handles mount/unmount.
    const modal = await this.execute(() => !!document.querySelector('dialog[open]'));
    const label = xpathString(text);
    return this.click(`${modal ? '(//dialog[@open])[last()]//' : '//'}button[not(@disabled) and (@aria-label=${label} or normalize-space(.)=${label})]`, 'xpath');
  }

  field(label) {
    const name = xpathString(label);
    return `//label[normalize-space(text())=${name}]/*[self::input or self::textarea or self::select] | //*[@aria-label=${name}][self::input or self::textarea or self::select or @contenteditable="true"]`;
  }

  async fill(label, text) {
    const id = await this.ready(this.field(label), 'xpath');
    const editable = await this.execute(name => {
      const node = document.querySelector(`[contenteditable="true"][aria-label="${CSS.escape(name)}"]`);
      return !!node;
    }, label);
    if (editable) {
      await this.command('POST', `/element/${id}/click`, {});
      await this.command('POST', '/actions', { actions: [{ type: 'key', id: 'keyboard', actions: [
        { type: 'keyDown', value: '\uE009' }, { type: 'keyDown', value: 'a' }, { type: 'keyUp', value: 'a' }, { type: 'keyUp', value: '\uE009' },
        { type: 'keyDown', value: '\uE003' }, { type: 'keyUp', value: '\uE003' },
      ] }] });
      await this.command('DELETE', '/actions');
    } else await this.command('POST', `/element/${id}/clear`, {});
    await this.command('POST', `/element/${id}/value`, { text: String(text) });
  }

  async press(selector, text, using = 'css selector') {
    await this.click(selector, using);
    await this.command('POST', '/actions', { actions: [{ type: 'key', id: 'keyboard', actions: [
      { type: 'keyDown', value: text }, { type: 'keyUp', value: text },
    ] }] });
    await this.command('DELETE', '/actions');
  }

  async select(label, value) {
    if (label === 'Page appearance') {
      await this.menu(['Page settings']); await this.select('Website appearance', value); return this.button('Save page');
    }
    if (label === 'Zoom') return this.menu([`${Number(value) * 100}%`], 'Canvas zoom');
    if (label === 'Editor theme') return this.menu(['Editor theme', value === 'system' ? 'System' : value === 'light' ? 'Light' : 'Dark']);
    if (label === 'Page') return this.page(value);
    await this.ready(this.field(label), 'xpath');
    return this.execute((name, choice) => {
      const input = [...document.querySelectorAll('select')].find(item => item.closest('label')?.firstChild?.textContent.trim() === name || item.getAttribute('aria-label') === name);
      if (!input || ![...input.options].some(option => option.value === choice)) throw new Error(`No ${name} option ${choice}`);
      input.focus(); input.value = choice;
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.dispatchEvent(new Event('change', { bubbles: true }));
      return input.value;
    }, label, value);
  }

  async toggleGrid() {
    await this.button('Grid and rulers');
    await this.click('//label[normalize-space(.)="Show grid"]/input', 'xpath');
    await this.button('Save grid settings');
  }
  async revealSelection() {
    await this.button('Layers');
    await this.click('section[aria-label="Layers"] .layer-row[data-selected="true"] button.layer-select');
    await this.button('Close Layers');
  }
  async escape() {
    await this.command('POST','/actions',{actions:[{type:'key',id:'keyboard',actions:[{type:'keyDown',value:'\uE00C'},{type:'keyUp',value:'\uE00C'},{type:'keyDown',value:'\uE00C'},{type:'keyUp',value:'\uE00C'}]}]});
    await this.command('DELETE','/actions');
  }
  source(label) { return this.execute(name => document.querySelector(`pre[aria-label="${name}"]`)?.textContent ?? null, label); }
  screenshot() { return this.command('GET', '/screenshot'); }
}
