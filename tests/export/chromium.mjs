// GPL-3.0-only. A small client for the installed Chromium/ChromeDriver pair.
// Native tests call this only inside their existing guarded virtual display.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {mkdir, mkdtemp, rm, writeFile} from 'node:fs/promises';
import {createServer} from 'node:http';
import path from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

async function waitFor(read, message, timeout = 20000) {
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
  return server.address().port;
}
async function request(base, method, route, body) {
  const response = await fetch(base + route, {
    method,
    headers: {'content-type': 'application/json'},
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(20000),
  });
  const result = await response.json();
  if (!response.ok || result.value?.error) throw new Error(`${method} ${route}: ${JSON.stringify(result)}`);
  return result.value;
}

/**
 * Load a real export and inspect its DOM through ChromeDriver. Every browser
 * has a disposable profile; all non-loopback web traffic reaches a rejecting
 * proxy. The caller's function runs only in the browser's ordinary page realm.
 * Call with page-specific wait/assert expressions, never substitute mocked DOM.
 */
export async function inspectChromiumPage({
  url,
  outputDirectory,
  expression,
  waitForExpression = 'document.readyState === \'complete\'',
  label = 'chromium'
}) {
  if (!/^:(?:9[0-9]|[1-9][0-9]{2,})(?:\.\d+)?$/.test(process.env.DISPLAY ?? '')) {
    throw new Error('Chromium verification requires a guarded private X11 display numbered 90 or higher.');
  }
  assert.match(label, /^[a-zA-Z0-9_-]+$/);
  const parsed = new URL(url);
  assert(
      parsed.protocol === 'file:' ||
          (parsed.protocol === 'http:' && ['127.0.0.1', '[::1]', 'localhost'].includes(parsed.hostname)),
      'Only local exports may be opened by this test helper.');
  await mkdir(outputDirectory, {recursive: true});
  const profile = await mkdtemp(path.join(outputDirectory, `${label}-profile-`));
  const blocked = [];
  const proxy = createServer((request, response) => {
    blocked.push(request.url);
    response.writeHead(502);
    response.end('External network disabled');
  });
  proxy.on('connect', (request, socket) => {
    blocked.push(request.url);
    socket.end('HTTP/1.1 502 Network disabled\r\n\r\n');
  });
  let driver, base, session, log = '', result;
  try {
    const proxyPort = await listen(proxy);
    const reservation = createServer();
    const port = await listen(reservation);
    await new Promise(resolve => reservation.close(resolve));
    base = `http://127.0.0.1:${port}`;
    driver = spawn('/usr/bin/chromedriver', [`--port=${port}`, '--allowed-ips=127.0.0.1'], {
      detached: true,
      stdio: ['ignore', 'pipe', 'pipe'],
      env: {
        ...process.env,
        HOME: profile,
        XDG_CONFIG_HOME: path.join(profile, 'config'),
        XDG_CACHE_HOME: path.join(profile, 'cache'),
        XDG_DATA_HOME: path.join(profile, 'data'),
        DBUS_SESSION_BUS_ADDRESS: `unix:path=${profile}/unused-bus`,
      },
    });
    driver.stdout.on('data', bytes => log += bytes);
    driver.stderr.on('data', bytes => log += bytes);
    await waitFor(async () => {
      if (driver.exitCode !== null || driver.signalCode !== null) throw new Error('Chromedriver exited');
      try {
        return (await request(base, 'GET', '/status'))?.ready;
      } catch {
        return false;
      }
    }, 'Chromedriver did not start');
    session = await request(base, 'POST', '/session', {
      capabilities: {
        alwaysMatch: {
          browserName: 'chrome',
          'goog:chromeOptions': {
            binary: '/usr/bin/chromium',
            args: [
              `--user-data-dir=${profile}`,
              '--no-first-run',
              '--no-default-browser-check',
              '--disable-background-networking',
              '--disable-component-update',
              '--disable-sync',
              '--disable-default-apps',
              '--metrics-recording-only',
              '--password-store=basic',
              '--use-mock-keychain',
              '--window-size=1100,760',
              `--proxy-server=http://127.0.0.1:${proxyPort}`,
              '--proxy-bypass-list=127.0.0.1;localhost;[::1]',
            ]
          },
        }
      }
    });
    const command = (method, route, body) =>
        request(base, method, `/session/${session.sessionId}${route}`, body);
    const evaluate = value => command('POST', '/execute/sync', {script: `return (${value});`, args: []});
    await command('POST', '/url', {url});
    await waitFor(() => evaluate(waitForExpression), 'The exported document did not become ready');
    result = {
      version: session.capabilities.browserVersion,
      value: await evaluate(expression),
      screenshot: path.join(outputDirectory, `${label}.png`),
      blockedExternal: blocked
    };
    await writeFile(result.screenshot, Buffer.from(await command('GET', '/screenshot'), 'base64'));
    return result;
  } finally {
    if (session) try {
        await request(base, 'DELETE', `/session/${session.sessionId}`);
      } catch (error) {
        log += `\nSession cleanup: ${error.message}`;
      }
    if (driver) {
      if (driver.exitCode === null && driver.signalCode === null) {
        driver.kill('SIGTERM');
        await Promise.race([once(driver, 'exit'), delay(4000)]);
      }
      if (driver.pid) try {
          process.kill(-driver.pid, 'SIGKILL');
        } catch {
        }
    }
    await new Promise(resolve => proxy.close(resolve));
    await writeFile(path.join(outputDirectory, `${label}.log`), log);
    await rm(profile, {recursive: true, force: true});
    if (result) result.profileRemoved = true;
  }
}
