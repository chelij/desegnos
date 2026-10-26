#!/usr/bin/env node
// GPL-3.0-only. Installed browsers only; no package downloads or application hooks.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {once} from 'node:events';
import {createHash} from 'node:crypto';
import {mkdir, mkdtemp, readFile, writeFile, rm, realpath, stat, readdir} from 'node:fs/promises';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';

// This fixture is a real two-page export made by compile_website +
// write_website_export. Its source/provenance stays beside the evidence.
const options = Object.fromEntries(
    process.argv.slice(2)
        .map((value, index, args) => value.startsWith('--') ? [value.slice(2), args[index + 1]] : [])
        .filter(entry => entry.length));
if (!options.site || !options.output)
  throw new Error(
      'Usage: xvfb-run -d node scripts/export-browser-test.mjs --site /absolute/export --output /absolute/evidence');
if (!/^:(?:9[0-9]|[1-9][0-9]{2,})(?:\.\d+)?$/.test(process.env.DISPLAY ?? ''))
  throw new Error('A guarded private X11 display numbered 90 or higher is required.');
const site = await realpath(options.site);
assert((await stat(path.join(site, 'index.html'))).isFile());
const output = path.resolve(options.output);
await mkdir(output, {recursive: true});
const privateRoot = await mkdtemp(path.join(output, 'browser-profiles-'));
const children = [];
const listeners = [];
const hostProcess = process;
const report = {
  site,
  display: process.env.DISPLAY,
  started: new Date().toISOString(),
  browsers: [],
  requests: [],
  passed: false
};
const environment = {
  ...process.env,
  HOME: privateRoot,
  XDG_CONFIG_HOME: path.join(privateRoot, 'config'),
  XDG_CACHE_HOME: path.join(privateRoot, 'cache'),
  XDG_DATA_HOME: path.join(privateRoot, 'data'),
  DBUS_SESSION_BUS_ADDRESS: `unix:path=${privateRoot}/unused-bus`,
  MOZ_ENABLE_WAYLAND: '0',
  MOZ_CRASHREPORTER_DISABLE: '1',
  MOZ_CRASHREPORTER_NO_REPORT: '1'
};
async function wait(read, message, timeout = 20000) {
  const end = Date.now() + timeout;
  let last;
  do {
    try {
      last = await read();
      if (last) return last;
    } catch (error) {
      last = error.message;
    }
    await delay(80);
  } while (Date.now() < end);
  throw new Error(`${message}: ${last}`);
}
async function listen(server) {
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  listeners.push(server);
  return server.address().port;
}
async function freePort() {
  const server = createServer();
  const port = await listen(server);
  await new Promise(resolve => server.close(resolve));
  listeners.splice(listeners.indexOf(server), 1);
  return port;
}
async function fingerprint(directory, prefix = '') {
  let files = {};
  for (const item of await readdir(directory, {withFileTypes: true})) {
    const relative = path.posix.join(prefix, item.name);
    assert(!item.isSymbolicLink(), 'Export fixture must have no symlinks');
    if (item.isDirectory())
      Object.assign(files, await fingerprint(path.join(directory, item.name), relative));
    else
      files[relative] =
          createHash('sha256').update(await readFile(path.join(directory, item.name))).digest('hex');
  }
  return files;
}
const sourceBefore = await fingerprint(site);
report.fileHashes = sourceBefore;
function child(binary, args, label) {
  const process = spawn(binary, args, {env: environment, detached: true, stdio: ['ignore', 'pipe', 'pipe']});
  children.push(process);
  let log = '';
  process.stdout.on('data', chunk => log += chunk);
  process.stderr.on('data', chunk => log += chunk);
  process.once('error', error => log += error.stack);
  process.log = () => log;
  process.label = label;
  return process;
}
async function stop(process) {
  if (process.exitCode === null && process.signalCode === null) {
    process.kill('SIGTERM');
    await Promise.race([once(process, 'exit'), delay(4000)]);
    if (process.exitCode === null && process.signalCode === null) {
      process.kill('SIGKILL');
      await Promise.race([once(process, 'exit'), delay(4000)]);
    }
  }
  if (process.pid) try {
      hostProcess.kill(-process.pid, 'SIGKILL');
    } catch {
    }
  await writeFile(path.join(output, `${process.label}.log`), process.log());
}
async function request(base, method, route, body) {
  const response = await fetch(base + route, {
    method,
    headers: {'content-type': 'application/json'},
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(20000)
  });
  const data = await response.json();
  if (!response.ok || data.value?.error) throw new Error(`${method} ${route}: ${JSON.stringify(data)}`);
  return data.value;
}
async function chromium(proxy) {
  const driverPort = await freePort();
  const process =
      child('/usr/bin/chromedriver', [`--port=${driverPort}`, '--allowed-ips=127.0.0.1'], 'chromedriver');
  const base = `http://127.0.0.1:${driverPort}`;
  await wait(async () => {
    try {
      return (await request(base, 'GET', '/status'))?.ready;
    } catch {
      return false;
    }
  }, 'Chromedriver did not become ready');
  const profile = path.join(privateRoot, 'chromium');
  await mkdir(profile);
  const session = await request(base, 'POST', '/session', {
    capabilities: {
      alwaysMatch: {
        browserName: 'chrome',
        'goog:chromeOptions': {
          binary: '/usr/bin/chromium',
          args: [
            `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check',
            '--disable-background-networking', '--disable-component-update', '--disable-sync',
            '--disable-default-apps', '--metrics-recording-only', '--password-store=basic',
            '--use-mock-keychain', '--window-size=1100,760', `--proxy-server=http://127.0.0.1:${proxy}`,
            '--proxy-bypass-list=127.0.0.1;localhost;[::1]'
          ]
        }
      }
    }
  });
  const command = (method, route, body) =>
      request(base, method, `/session/${session.sessionId}${route}`, body);
  return {
    name: 'chromium',
    version: session.capabilities.browserVersion,
    async navigate(url) {
      await command('POST', '/url', {url});
    },
    async evaluate(expression) {
      return command('POST', '/execute/sync', {script: `return (${expression});`, args: []});
    },
    async screenshot(file) {
      await writeFile(file, Buffer.from(await command('GET', '/screenshot'), 'base64'));
    },
    async close() {
      await command('DELETE', '');
      await stop(process);
    }
  };
}
class BiDi {
  constructor(socket) {
    this.socket = socket;
    this.next = 0;
    this.pending = new Map();
    socket.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      if (!message.id) return;
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      clearTimeout(pending.timer);
      if (message.type === 'error')
        pending.reject(new Error(JSON.stringify(message)));
      else
        pending.resolve(message.result);
    });
  }
  async send(method, params = {}) {
    const id = ++this.next;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Firefox timed out: ${method}`));
      }, 20000);
      this.pending.set(id, {resolve, reject, timer});
      this.socket.send(JSON.stringify({id, method, params}));
    });
  }
}
async function firefox(proxy) {
  const port = await freePort();
  const profile = path.join(privateRoot, 'firefox');
  await mkdir(profile);
  const prefs = {
    'browser.shell.checkDefaultBrowser': false,
    'browser.startup.homepage': 'about:blank',
    'browser.startup.page': 0,
    'browser.newtabpage.enabled': false,
    'browser.newtabpage.activity-stream.feeds.telemetry': false,
    'browser.newtabpage.activity-stream.telemetry': false,
    'datareporting.healthreport.uploadEnabled': false,
    'datareporting.policy.dataSubmissionEnabled': false,
    'toolkit.telemetry.enabled': false,
    'toolkit.telemetry.unified': false,
    'app.update.auto': false,
    'app.update.enabled': false,
    'browser.safebrowsing.downloads.enabled': false,
    'browser.safebrowsing.malware.enabled': false,
    'browser.safebrowsing.phishing.enabled': false,
    'network.dns.disablePrefetch': true,
    'network.prefetch-next': false,
    'network.predictor.enabled': false,
    'network.trr.mode': 5,
    'network.proxy.type': 1,
    'network.proxy.http': '127.0.0.1',
    'network.proxy.http_port': proxy,
    'network.proxy.ssl': '127.0.0.1',
    'network.proxy.ssl_port': proxy,
    'network.proxy.no_proxies_on': 'localhost, 127.0.0.1, ::1',
    'network.captive-portal-service.enabled': false,
    'network.connectivity-service.enabled': false,
    'services.settings.server': 'http://127.0.0.1:1/',
    'browser.search.update': false,
    'browser.region.update.enabled': false,
    'browser.region.network.url': '',
    'browser.urlbar.quicksuggest.enabled': false,
    'browser.urlbar.merino.enabled': false,
    'extensions.update.enabled': false,
    'extensions.systemAddon.update.enabled': false,
    'media.gmp-manager.updateEnabled': false,
    'media.gmp-gmpopenh264.enabled': false,
    'media.gmp-widevinecdm.enabled': false
  };
  await writeFile(
      path.join(profile, 'user.js'),
      Object.entries(prefs)
          .map(([key, value]) => `user_pref(${JSON.stringify(key)}, ${JSON.stringify(value)});`)
          .join('\n'));
  const process = child(
      '/usr/bin/firefox',
      [
        '--no-remote', '--new-instance', '--profile', profile, '--remote-debugging-port', String(port),
        'about:blank'
      ],
      'firefox');
  const socket = await wait(async () => {
    if (process.exitCode !== null) throw new Error(`Firefox exited: ${process.log()}`);
    const candidate = new WebSocket(`ws://127.0.0.1:${port}/session`);
    try {
      await Promise.race([
        once(candidate, 'open'), delay(1000).then(() => {
          throw new Error('Not ready');
        })
      ]);
      return candidate;
    } catch {
      candidate.close();
      return false;
    }
  }, 'Firefox remote protocol did not become ready');
  const bidi = new BiDi(socket);
  const session = await bidi.send('session.new', {capabilities: {alwaysMatch: {acceptInsecureCerts: false}}});
  const context = (await bidi.send('browsingContext.create', {type: 'tab'})).context;
  return {
    name: 'firefox',
    version: session.capabilities.browserVersion,
    async navigate(url) {
      await bidi.send('browsingContext.navigate', {context, url, wait: 'complete'});
    },
    async evaluate(expression) {
      const result = await bidi.send(
          'script.evaluate',
          {expression: `JSON.stringify(${expression})`, target: {context}, awaitPromise: true});
      if (result.type !== 'success') throw new Error(JSON.stringify(result));
      return JSON.parse(result.result.value);
    },
    async screenshot(file) {
      const result = await bidi.send('browsingContext.captureScreenshot', {context});
      await writeFile(file, Buffer.from(result.data, 'base64'));
    },
    async close() {
      await bidi.send('session.end');
      socket.close();
      await stop(process);
    }
  };
}
const homeExpression =
    `(()=>{document.getElementById('change').click();return{title:document.title,text:document.getElementById('heading').textContent,script:document.body.dataset.scriptReady,color:getComputedStyle(document.body).color,imageWidth:document.getElementById('photo').naturalWidth,image:document.getElementById('photo').currentSrc,background:getComputedStyle(document.getElementById('background')).backgroundImage,href:document.getElementById('team').href,url:location.href}})()`;
const teamExpression =
    `({title:document.title,color:getComputedStyle(document.body).color,search:location.search,hash:location.hash,script:document.body.dataset.scriptReady,background:getComputedStyle(document.body).backgroundImage,home:document.getElementById('home').href,url:location.href})`;
async function scenario(browser, label, url) {
  await browser.navigate(url);
  try {
    await wait(
        () => browser.evaluate(
            `!!(document.getElementById('photo')?.naturalWidth && document.body.dataset.scriptReady==='yes')`),
        'Home image/script did not load');
  } catch (error) {
    await writeFile(path.join(output, `${browser.name}-${label}-failure.json`), JSON.stringify(await browser.evaluate(`({ready:document.readyState,url:location.href,title:document.title,head:document.head.innerHTML,body:document.body.innerHTML,imageWidth:document.getElementById('photo')?.naturalWidth,script:document.body.dataset.scriptReady,resources:performance.getEntriesByType('resource').map(item=>item.toJSON())})`), null, 2));
    await browser.screenshot(path.join(output, `${browser.name}-${label}-failure.png`));
    throw error;
  }
  const home = await browser.evaluate(homeExpression);
  assert.equal(home.title, 'Portable home');
  assert.equal(home.text, 'Changed by exported JavaScript');
  assert.equal(home.script, 'yes');
  assert.equal(home.color, 'rgb(200, 0, 0)');
  assert.equal(home.imageWidth, 1);
  assert(home.background.includes('/assets/'));
  assert(home.href.endsWith('/about/team/index.html?view=full#lead'));
  assert(!home.href.includes('desegnos-preview'));
  await browser.screenshot(path.join(output, `${browser.name}-${label}-home.png`));
  await browser.evaluate(`(document.getElementById('team').click(), true)`);
  await wait(
      () => browser.evaluate(`document.title==='Portable team' && document.body.dataset.scriptReady==='yes'`),
      'Nested page did not finish navigation');
  const team = await browser.evaluate(teamExpression);
  assert.equal(team.color, 'rgb(0, 0, 200)');
  assert.equal(team.search, '?view=full');
  assert.equal(team.hash, '#lead');
  assert(team.background.includes('/assets/'));
  assert.equal(team.home, url);
  await browser.screenshot(path.join(output, `${browser.name}-${label}-nested.png`));
  await browser.evaluate(`(document.getElementById('home').click(), true)`);
  await wait(
      () => browser.evaluate(`document.title==='Portable home' && document.body.dataset.scriptReady==='yes'`),
      'Return navigation failed');
  const returned = await browser.evaluate(`({title:document.title,url:location.href})`);
  assert.equal(returned.url, url);
  return {label, home, team, returned};
}
try {
  const proxy = createServer((request, response) => {
    report.requests.push({blockedExternal: request.url});
    response.writeHead(502);
    response.end('External network disabled for export verification');
  });
  proxy.on('connect', (request, socket) => {
    report.requests.push({blockedExternal: request.url});
    socket.end('HTTP/1.1 502 Network disabled\r\n\r\n');
  });
  const proxyPort = await listen(proxy);
  const mime = {
    '.html': 'text/html; charset=utf-8',
    '.css': 'text/css; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.png': 'image/png',
    '.svg': 'image/svg+xml'
  };
  const server = createServer(async (request, response) => {
    try {
      const requested = new URL(request.url, 'http://localhost');
      let relative = decodeURIComponent(requested.pathname);
      if (relative.startsWith('/portfolio/')) relative = relative.slice('/portfolio'.length);
      relative = relative.replace(/^\//, '');
      let file = path.resolve(site, relative);
      assert(file === site || file.startsWith(site + path.sep));
      if ((await stat(file)).isDirectory()) file = path.join(file, 'index.html');
      report.requests.push({local: request.url, file: path.relative(site, file)});
      response.writeHead(200, {
        'content-type': mime[path.extname(file)] ?? 'application/octet-stream',
        'cache-control': 'no-store'
      });
      response.end(await readFile(file));
    } catch {
      response.writeHead(404);
      response.end('Not found');
    }
  });
  const serverPort = await listen(server);
  for (const create of [chromium, firefox]) {
    const browser = await create(proxyPort);
    const result = {name: browser.name, version: browser.version, scenarios: []};
    report.browsers.push(result);
    try {
      for (const [label, url] of [
               ['file', pathToFileURL(path.join(site, 'index.html')).href],
               ['host-root', `http://127.0.0.1:${serverPort}/index.html`],
               ['host-subdirectory', `http://127.0.0.1:${serverPort}/portfolio/index.html`]])
        result.scenarios.push(await scenario(browser, label, url));
    } finally {
      await browser.close();
    }
  }
  assert.deepEqual(await fingerprint(site), sourceBefore, 'Browser verification changed exported files');
  report.passed = true;
} catch (error) {
  report.error = error.stack;
  process.exitCode = 1;
} finally {
  for (const process of children) try {
      await stop(process);
    } catch {
    }
  for (const server of listeners) await new Promise(resolve => server.close(resolve));
  await rm(privateRoot, {recursive: true, force: true});
  report.profilesRemoved = true;
  report.processesStopped =
      children.every(process => process.exitCode !== null || process.signalCode !== null);
  report.finished = new Date().toISOString();
  await writeFile(path.join(output, 'result.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({
    passed: report.passed,
    error: report.error,
    browsers: report.browsers.map(browser => ({
                                    name: browser.name,
                                    version: browser.version,
                                    scenarios: browser.scenarios.map(result => result.label)
                                  })),
    output
  }));
}
