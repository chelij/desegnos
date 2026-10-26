import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { access, chmod, copyFile, mkdir, readFile, readdir, readlink, realpath, rm, stat, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { toolchainEnvironment } from './toolchain.mjs';
import { resolveDbusRunSession, resolveXvfbRun, xvfbEnvironment } from './xvfb.mjs';
import { WebDriver, waitFor } from '../tests/native/webdriver.mjs';
import { PhysicalInput } from '../tests/native/physical-workflow.mjs';
import { chooseFile } from '../tests/native/file-dialog-workflow.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const fixtureRoot = join(root, 'benchmarks/generation');
const digest = value => createHash('sha256').update(value).digest('hex');
const catalogue = JSON.parse(await readFile(join(fixtureRoot, 'scenarios.json'), 'utf8'));
const fixtures = JSON.parse(await readFile(join(fixtureRoot, 'fixtures.json'), 'utf8'));
// The 0.1.0 gate (docs/GENERATION_BENCHMARK.md); retired scenarios stay runnable but do not count.
const gateScenarioIds = ['01-hero', '02-pricing', '04-about', '05-mobile-home', '06-button-revision', '08-multi-precise', '09-imported-brand', '10-faq', '12-navigation-footer', '13-mobile-contact', '15-long-copy', '16-two-pages', '17-responsive-layout'];
const help = `Desegnos generation benchmark — OWNER RUN ONLY; quality remains UNVERIFIED until rated.

Offline commands (no app launch, credentials or provider request):
  node scripts/generation-benchmark.mjs --list
  node scripts/generation-benchmark.mjs --validate-fixtures
  node scripts/generation-benchmark.mjs --dry-run --scenario 01-hero
  node scripts/generation-benchmark.mjs --summarize <evidence-directory>

One explicitly authorized paid attempt, on a private virtual display:
  systemd-ask-password 'OpenAI API key' | node scripts/generation-benchmark.mjs \\
    --scenario 01-hero --binary /absolute/native-tests/debug/desegnos \\
    --budget-usd 1 --max-seconds 180 --key-stdin --allow-paid-generation

Build the native-tests DEBUG binary first with tauri.test.conf.json.
The runner accepts no API-key argument or environment credential, uses a fresh
private profile and unavailable system vault, and submits exactly once through
native prepare_generation → start_generation → apply_generation. No automatic
retry. Budget is a conservative estimate, not a guaranteed billing cap. Cancelling
or timing out submitted work can still incur charges. Evidence is private under
test-results/benchmark/<UTC-date>/<scenario>-<unique-run>/.
`;

function argumentsFor(values) {
  const result = {};
  const flags = new Set(['--help', '--list', '--validate-fixtures', '--dry-run', '--key-stdin', '--allow-paid-generation']);
  const options = new Set(['--scenario', '--binary', '--budget-usd', '--max-seconds', '--summarize']);
  for (let index = 0; index < values.length; index += 1) {
    const value = values[index];
    if (result[value] !== undefined) throw new Error(`Repeated option ${value}`);
    if (flags.has(value)) result[value] = true;
    else if (options.has(value) && values[index + 1] && !values[index + 1].startsWith('--')) result[value] = values[++index];
    else throw new Error('Unknown or incomplete benchmark option; use --help.');
  }
  return result;
}

async function validateFixtures() {
  assert.equal(catalogue.version, 1); assert.equal(catalogue.scenarios.length, 18);
  assert.equal(fixtures.scenarios.length, 18); assert.equal(new Set(catalogue.scenarios.map(item => item.id)).size, 18);
  for (const id of gateScenarioIds) assert.ok(catalogue.scenarios.some(item => item.id === id), `Unknown gate scenario ${id}`);
  for (const scenario of catalogue.scenarios) {
    assert.match(scenario.id, /^\d{2}-[a-z-]+$/);
    const fixture = fixtures.scenarios.find(item => item.id === scenario.id); assert.ok(fixture);
    assert.equal(fixture.path, `projects/${scenario.id}.desegnos`);
    assert.equal(digest(await readFile(join(fixtureRoot, fixture.path))), fixture.sha256, `Changed input fixture ${scenario.id}`);
    assert.ok(scenario.criteria.length >= 3 && scenario.pages.length > 0);
  }
  const checked = spawnSync(process.execPath, ['scripts/cargo.mjs', 'run', '--offline', '--locked', '--manifest-path', 'benchmarks/generation/fixture-builder/Cargo.toml', '--', '--check', fixtureRoot], { cwd: root, env: { ...toolchainEnvironment(), CARGO_TARGET_DIR: join(root, 'target/benchmark-fixtures') }, encoding: 'utf8', timeout: 120_000, maxBuffer: 1024 * 1024 });
  if (checked.error && checked.status !== 0) throw checked.error;
  assert.equal(checked.status, 0, `Core/storage fixture validation failed: ${checked.stderr}`);
  assert.deepEqual(JSON.parse(checked.stdout), fixtures);
  return { valid: true, scenarios: 18, providerCalls: 0, credentialsRead: false, coreAndStorageValidation: 'passed' };
}

async function summarize(directory) {
  const found = [];
  async function visit(path, depth) {
    if (depth > 4) return;
    let entries;
    try { entries = await readdir(path, { withFileTypes: true }); }
    catch (error) { if (error.code === 'ENOENT') return; throw error; }
    for (const entry of entries) {
      assert.equal(entry.isSymbolicLink(), false, 'Benchmark evidence traversal rejects symlinks');
      const file = join(path, entry.name);
      if (entry.isDirectory()) { if (!entry.name.startsWith('.')) await visit(file, depth + 1); }
      else if (entry.name === 'result.json') {
        const result = JSON.parse(await readFile(file, 'utf8'));
        let rating = null; try { rating = JSON.parse(await readFile(join(path, 'owner-rating.json'), 'utf8')); } catch (error) { if (error.code !== 'ENOENT') throw error; }
        found.push({ path, result, rating });
      }
    }
  }
  await visit(resolve(directory), 0);
  const scenarios = catalogue.scenarios.map(scenario => {
    // A newer failed/unrated run cannot silently reuse an older favorable rating.
    const run = found.filter(item => item.result.scenarioId === scenario.id).sort((a, b) => String(b.result.startedAt).localeCompare(String(a.result.startedAt)))[0];
    const rated = !!run?.rating?.reviewer?.trim() && Number.isFinite(Date.parse(run.rating.reviewedAt)) && typeof run.rating.usableWithoutManualFixes === 'boolean';
    const applied = run?.result.status === 'applied' && !run.result.error && !!run.result.rawOutput;
    const strict = scenario.mode !== 'strict' || applied && rated && run.rating.strictOrderAndGrouping === true;
    const costRecorded = Number.isFinite(run?.result.usageCostEstimateUsd) && run.result.usageCostEstimateUsd >= 0 || Number.isFinite(run?.rating?.actualBilledUsd) && run.rating.actualBilledUsd >= 0;
    const accounting = Number.isFinite(run?.result.durationMs) && run.result.durationMs > 0 && costRecorded;
    return { id: scenario.id, gate: gateScenarioIds.includes(scenario.id), placeholdersPerPage: run?.result.placeholdersPerPage ?? null, run: run?.path ?? null, attempted: (run?.result.providerAttempts ?? 0) > 0, applied, rated, usable: rated && run.rating.usableWithoutManualFixes, strictOrderAndGrouping: strict, costAndDurationRecorded: accounting };
  });
  const gate = scenarios.filter(item => item.gate);
  const usable = gate.filter(item => item.applied && item.rated && item.usable).length;
  const complete = gate.every(item => item.attempted && item.rated && item.strictOrderAndGrouping && item.costAndDurationRecorded);
  return { status: complete && usable === gate.length ? 'OWNER_RATED_GATE_PASSED' : 'UNVERIFIED', usable, denominator: gate.length, requiredUsable: gate.length, usablePercent: usable * 100 / gate.length, costBasis: 'Usage × frozen local catalogue standard token prices; conservative estimate, not confirmed billing.', scenarios };
}

async function stdinKey() {
  if (process.stdin.isTTY) throw new Error('Pipe a hidden password prompt to stdin; refusing visible terminal key entry.');
  const chunks = []; let size = 0;
  for await (const chunk of process.stdin) { size += chunk.length; if (size > 4096) throw new Error('The session key input is too long.'); chunks.push(chunk); }
  const buffer = Buffer.concat(chunks); chunks.forEach(chunk => chunk.fill(0));
  const key = buffer.toString('utf8').trim(); buffer.fill(0);
  if (!key || /[\r\n\0]/.test(key)) throw new Error('Provide exactly one nonempty API key through stdin.');
  return key;
}

async function freePort() {
  const server = createServer(); await new Promise((accept, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', accept); });
  const port = server.address().port; await new Promise((accept, reject) => server.close(error => error ? reject(error) : accept())); return port;
}
function exit(child) { return child.exitCode !== null ? Promise.resolve(child.exitCode) : new Promise((accept, reject) => { child.once('error', reject); child.once('exit', code => accept(code ?? 1)); }); }
async function stop(child) {
  if (!child?.pid) return;
  const signal = kind => { try { process.kill(-child.pid, kind); } catch (error) { if (error.code !== 'ESRCH') throw error; } };
  signal('SIGTERM'); await Promise.race([exit(child), delay(2000)]); signal('SIGKILL'); await exit(child);
}

async function desktopState() {
  const display = process.env.DISPLAY ?? null, number = display?.match(/^:(\d+)(?:\.\d+)?$/)?.[1];
  const resources = [];
  for (const path of number ? [`/tmp/.X11-unix/X${number}`, `/tmp/.X${number}-lock`] : []) {
    try { const item = await stat(path); resources.push({ path, dev: item.dev, ino: item.ino }); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  const probe = display ? spawnSync('xprop', ['-root', '_NET_SUPPORTING_WM_CHECK'], { timeout: 5000 }) : null;
  return { display, resources, connected: probe?.status === 0 };
}

async function isolated() {
  const requestPath = process.env.DESEGNOS_BENCHMARK_REQUEST;
  assert.ok(requestPath && process.env.DESEGNOS_BENCHMARK_GUARD, 'Missing private benchmark launch guard');
  const request = JSON.parse(await readFile(requestPath, 'utf8'));
  assert.equal(request.guard, process.env.DESEGNOS_BENCHMARK_GUARD); assert.equal(request.allowPaidGeneration, true);
  const display = process.env.DISPLAY?.match(/^:(\d+)(?:\.\d+)?$/);
  assert.ok(display && Number(display[1]) >= 90); assert.notEqual(process.env.DISPLAY, request.originalDisplay);
  assert.equal(process.env.GDK_BACKEND, 'x11'); assert.equal(process.env.WAYLAND_DISPLAY, undefined); assert.equal(process.env.WAYLAND_SOCKET, undefined); assert.ok(process.env.DBUS_SESSION_BUS_ADDRESS);
  assert.equal(process.env.DESEGNOS_NATIVE_TEST_OPENAI_ENDPOINT, undefined);
  const evidence = request.evidence, scenario = catalogue.scenarios.find(item => item.id === request.scenarioId);
  assert.ok(scenario); let key = await stdinKey(), app, driver, job;
  const redact = value => String(value).split(key).join('[credential redacted]');
  const result = { version: 1, status: 'notSubmitted', quality: 'UNVERIFIED', scenarioId: scenario.id, fixtureSha256: request.fixtureSha256, startedAt: new Date().toISOString(), authorizedByOwnerFlag: true, providerAttempts: 0, durationMs: null, usageCostEstimateUsd: null, rawOutput: null, limits: ['Owner-run live path has not been executed by Codex.', 'Only a validated complete candidate can apply.', 'Catalogue-based cost is not confirmed provider billing.'] };
  try {
    const port = await freePort();
    app = spawn(request.binary, [], { cwd: root, detached: true, stdio: 'ignore', env: { ...process.env, TAURI_WEBDRIVER_PORT: String(port), DESEGNOS_GENERATION_BENCHMARK: '1' } });
    await new Promise((accept, reject) => { app.once('spawn', accept); app.once('error', reject); });
    await writeFile(join(dirname(requestPath), 'app-pid.json'), JSON.stringify({pid:app.pid,binary:request.binary}), {mode:0o600});
    const environment = (await readFile(`/proc/${app.pid}/environ`, 'utf8')).split('\0'); assert.ok(environment.includes(`DISPLAY=${process.env.DISPLAY}`)); assert.ok(environment.includes('GDK_BACKEND=x11'));
    driver = await new WebDriver(port).start(() => app.exitCode === null && app.signalCode === null);
    await waitFor(async () => /class="application(?:\s|")/.test(await driver.command('GET', '/source')), 'The benchmark editor did not mount', 30_000);
    const physical = new PhysicalInput(driver, app.pid); await physical.initialize(evidence);
    await chooseFile(physical, () => driver.button('Open project'), join(evidence, 'working.desegnos'), evidence, 'benchmark-open');
    const project = await waitFor(async () => (await driver.invoke('get_workspace')).project, 'The benchmark input did not open');
    assert.equal(project.id, request.projectId);
    const connected = await driver.invoke('set_ai_key', { key, storage: 'session' }); assert.equal(connected.credential.storage, 'session');
    const settings = { ...connected.settings, spendingLimitUsd: request.budgetUsd };
    const configured = await driver.invoke('save_ai_settings', { settings }); assert.equal(configured.authentication, 'apiKey');
    job = await driver.invoke('prepare_generation'); result.jobId = job.jobId; result.settings = job.settings; result.preSubmissionEstimateUsd = job.estimateUsd;
    assert.equal(job.status, 'prepared'); assert.equal(job.requirements.sha256, digest(job.requirements.markdown)); assert.ok(job.estimateUsd <= request.budgetUsd);
    await writeFile(join(evidence, 'REQUIREMENTS.md'), job.requirements.markdown, { mode: 0o600 });
    await writeFile(join(evidence, 'prepared.json'), JSON.stringify(job, null, 2), { mode: 0o600 });
    const start = performance.now();
    result.submissionStarted = true; result.providerAttempts = 1;
    await writeFile(join(evidence, 'submission.json'), JSON.stringify({jobId:job.jobId,attempt:1,submittedAt:new Date().toISOString(),ownerAuthorized:true}), {mode:0o600});
    job = await driver.invoke('start_generation', { jobId: job.jobId });
    while (job.status === 'generating') {
      if (performance.now() - start > request.maxSeconds * 1000) { await driver.invoke('cancel_generation', { jobId: job.jobId }); result.timedOut = true; break; }
      await delay(350); job = await driver.invoke('get_generation', { projectId: project.id });
    }
    job = await driver.invoke('get_generation', { projectId: project.id }); result.durationMs = performance.now() - start;
    if (job.status === 'ready') await driver.invoke('apply_generation', { jobId: job.jobId, acceptedDecisionIndices: [] });
    job = await driver.invoke('get_generation', { projectId: project.id }); result.status = job.status; result.usage = job.usage;
    const model = configured.catalogue.find(item => item.id === job.settings.model); result.catalogue = model ?? null;
    if (job.usage && model) result.usageCostEstimateUsd = (job.usage.inputTokens * model.inputUsdPerMillion + job.usage.outputTokens * model.outputUsdPerMillion) / 1_000_000;
    result.attempts = job.attempts; result.providerAttempts = job.attempts.length;
    const identifier = JSON.parse(await readFile(join(root, 'src-tauri/tauri.conf.json'), 'utf8')).identifier;
    const raw = join(process.env.XDG_DATA_HOME, identifier, 'generation', project.id, job.jobId, 'provider-output-1.txt');
    try { const bytes = await readFile(raw); if (bytes.includes(Buffer.from(key))) throw new Error('Credential echo detected in benchmark output'); await writeFile(join(evidence, 'provider-output.txt'), bytes, { mode: 0o600 }); result.rawOutput = { path: 'provider-output.txt', sha256: digest(bytes), bytes: bytes.length, format: 'Exact successful transport output text, before structured candidate parsing; not reconstructed JSON and not a raw HTTP/SSE capture.' }; }
    catch (error) { if (error.code === 'ENOENT') result.rawOutputUnavailable = 'Transport did not return safe complete output; raw rejected bytes are deliberately not retained.'; else throw error; }
    if (job.status === 'applied') assert.ok(result.rawOutput, 'The debug benchmark capture is missing; rebuild the current native-tests binary');
    await writeFile(join(evidence, 'job.json'), redact(JSON.stringify(job, null, 2)), { mode: 0o600 });
    await writeFile(join(evidence, 'project.json'), redact(JSON.stringify((await driver.invoke('get_workspace')).project, null, 2)), { mode: 0o600 });
    await driver.invoke('save_project', { saveAs: false });
    await copyFile(join(evidence, 'working.desegnos'), join(evidence, 'result.desegnos')); await chmod(join(evidence, 'result.desegnos'), 0o600);
    // Reload the real editor so it reads the accepted native state; no state shim.
    try { await driver.execute(() => { location.reload(); return true; }); } catch { /* Navigation may discard the old script response. */ }
    await waitFor(async () => { try { return await driver.execute(() => !!document.querySelector('[data-testid="canvas"]')); } catch { return false; } }, 'The accepted benchmark result did not reopen on canvas', 30_000);
    await physical.initialize(evidence);
    result.placeholdersPerPage=[];
    for (const page of (await driver.invoke('get_workspace')).project.pages) {
      await driver.page(page.id);
      if (page.content) await waitFor(() => driver.execute(() => document.querySelector('.static-preview')?.dataset.previewReady === 'true'), 'Accepted benchmark source did not render');
      await driver.button('Fit'); await physical.captureWindow(join(evidence, `page-${page.id}.png`));
      const layout = await driver.execute(() => { const document = window.document.querySelector('.static-preview iframe')?.contentDocument; return document ? [...document.querySelectorAll('header,nav,main,section,h1,h2,h3,footer,button,a,input,textarea')].slice(0,300).map(element => { const rect = element.getBoundingClientRect(); return { tag: element.localName, text: element.textContent.trim().slice(0,160), x:rect.x,y:rect.y,width:rect.width,height:rect.height }; }) : []; });
      const placeholders=await driver.execute(()=>{
        const doc=document.querySelector('.static-preview iframe')?.contentDocument;
        if(!doc)return null;
        const slots=[...doc.querySelectorAll('[data-desegnos-placeholder]')].filter(node=>!node.parentElement?.closest('[data-desegnos-placeholder]')).length;
        const unmarked=[...doc.querySelectorAll('*')].filter(node=>!node.closest('[data-desegnos-placeholder]')&&[...node.childNodes].some(child=>child.nodeType===Node.TEXT_NODE&&/placeholder/i.test(child.textContent))).length;
        return {markedSlots:slots,unmarkedLabels:unmarked,total:slots+unmarked};
      });
      result.placeholdersPerPage.push({pageId:page.id,...(placeholders??{markedSlots:null,unmarkedLabels:null,total:null})});
      await writeFile(join(evidence, `layout-${page.id}.json`), JSON.stringify(layout, null, 2), { mode: 0o600 });
    }
    await writeFile(join(evidence, 'owner-rating.json'), JSON.stringify({ version:1, reviewer:null, reviewedAt:null, usableWithoutManualFixes:null, strictOrderAndGrouping:null, actualBilledUsd:null, notes:'' }, null, 2), { mode: 0o600 });
  } catch (error) {
    result.error = redact(error.stack ?? error);
    if (driver && job) {
      try {
        job = await driver.invoke('get_generation', { projectId: request.projectId }); result.status = job.status; result.attempts = job.attempts; result.providerAttempts = job.attempts.length; result.usage = job.usage;
        await writeFile(join(evidence, 'job.json'), redact(JSON.stringify(job, null, 2)), { mode:0o600 });
      } catch { if (result.submissionStarted) result.status = 'outcomeUncertain'; }
    } else if (result.submissionStarted) result.status = 'outcomeUncertain';
  }
  finally {
    if (driver) {
      if (job?.status === 'generating') try { await driver.invoke('cancel_generation', { jobId: job.jobId }); } catch {}
      try { await driver.invoke('remove_ai_key'); } catch { result.credentialCleanup = 'Session ends with the isolated app process; no system vault was used.'; }
      try { await driver.stop(); } catch {}
    }
    await stop(app); result.finishedAt = new Date().toISOString();
    await writeFile(join(evidence, 'result.json'), redact(JSON.stringify(result, null, 2)), { mode: 0o600 }); key = '';
  }
  process.exitCode = result.status === 'applied' && result.rawOutput && !result.error ? 0 : 1;
}

async function main() {
  const options = argumentsFor(process.argv.slice(2));
  if (options['--help']) { console.log(help); return; }
  if (options['--list']) { console.log(catalogue.scenarios.map(item => `${item.id}\t${item.mode}\t${item.title}`).join('\n')); return; }
  if (options['--summarize']) { console.log(JSON.stringify(await summarize(options['--summarize']), null, 2)); return; }
  if (options['--validate-fixtures']) { console.log(JSON.stringify(await validateFixtures(), null, 2)); return; }
  const scenario = catalogue.scenarios.find(item => item.id === options['--scenario']);
  if (!scenario) throw new Error('Choose exactly one fixed --scenario; see --list.');
  const fixture = fixtures.scenarios.find(item => item.id === scenario.id);
  await validateFixtures();
  if (options['--dry-run']) { console.log(JSON.stringify({ status:'UNVERIFIED', dryRun:true, credentialsRead:false, providerCalls:0, fixture, scenario }, null, 2)); return; }
  if (!options['--allow-paid-generation'] || !options['--key-stdin']) throw new Error('Owner execution requires both --allow-paid-generation and --key-stdin. Codex must not run paid benchmarks.');
  assert.equal(process.platform, 'linux', 'Only the guarded Linux benchmark route is implemented.');
  const budgetUsd = Number(options['--budget-usd']), maxSeconds = Number(options['--max-seconds']);
  assert.ok(Number.isFinite(budgetUsd) && budgetUsd > 0 && budgetUsd <= 100, 'Explicit --budget-usd must be > 0 and <= 100; this is not a billing cap.');
  assert.ok(Number.isInteger(maxSeconds) && maxSeconds >= 10 && maxSeconds <= 600, 'Explicit --max-seconds must be 10–600.');
  assert.ok(options['--binary'] && isAbsolute(options['--binary']), 'Choose an absolute native-tests DEBUG --binary.');
  const binary = await realpath(options['--binary']); await access(binary);
  const xvfbRun = resolveXvfbRun();
  const dbusRunSession = resolveDbusRunSession();
  const binaryBytes = await readFile(binary);
  assert.ok(binaryBytes.includes(Buffer.from('DESEGNOS_GENERATION_BENCHMARK')) && binaryBytes.includes(Buffer.from('provider-output-')), 'Rebuild the current debug native-tests binary with benchmark capture and isolated vault support before launching.');
  let key = await stdinKey();
  const evidence = join(root, 'test-results/benchmark', new Date().toISOString().slice(0,10), `${scenario.id}-${randomUUID()}`);
  await mkdir(evidence, { recursive:true, mode:0o700 }); await chmod(evidence, 0o700);
  const scratch = join(evidence, '.profile'); await mkdir(scratch, { mode:0o700 });
  const environment = Object.fromEntries(['PATH','LANG','LC_ALL','LC_CTYPE','USER','LOGNAME','SHELL','HOME'].filter(name => process.env[name]).map(name => [name,process.env[name]]));
  for (const [variable,name] of [['XDG_DATA_HOME','data'],['XDG_CONFIG_HOME','config'],['XDG_CACHE_HOME','cache'],['XDG_RUNTIME_DIR','runtime']]) { environment[variable] = join(scratch,name); await mkdir(environment[variable],{mode:0o700}); }
  const request = { guard:randomUUID(), allowPaidGeneration:true, scenarioId:scenario.id, fixtureSha256:fixture.sha256, projectId:fixture.projectId, binary, binarySha256:digest(await readFile(binary)), budgetUsd, maxSeconds, evidence, originalDisplay:process.env.DISPLAY ?? null };
  await copyFile(join(fixtureRoot, fixture.path), join(evidence,'input.desegnos')); await chmod(join(evidence,'input.desegnos'),0o600);
  await copyFile(join(evidence,'input.desegnos'), join(evidence,'working.desegnos')); await chmod(join(evidence,'working.desegnos'),0o600);
  const requestPath = join(scratch,'launch.json'); await writeFile(requestPath,JSON.stringify(request),{mode:0o600});
  const original = await desktopState(); let child;
  await writeFile(join(evidence,'authorization.json'), JSON.stringify({...request,guard:undefined},null,2),{mode:0o600});
  try {
    child = spawn(xvfbRun,['-d','-e',join(evidence,'xvfb.log'),'-f',join(scratch,'Xauthority'),'-s','-screen 0 1600x1100x24',dbusRunSession,'--',process.execPath,fileURLToPath(import.meta.url),'--isolated'],{cwd:root,detached:true,stdio:['pipe','ignore','ignore'],env:xvfbEnvironment({...environment,GDK_SCALE:'1',GDK_DPI_SCALE:'1',DESEGNOS_BENCHMARK_REQUEST:requestPath,DESEGNOS_BENCHMARK_GUARD:request.guard,DESEGNOS_NATIVE_TEST_EVIDENCE:evidence})});
    child.stdin.end(key); key = '';
    const interrupt = () => { void stop(child); }; process.once('SIGINT',interrupt); process.once('SIGTERM',interrupt);
    try { process.exitCode = await exit(child); } finally { process.removeListener('SIGINT',interrupt); process.removeListener('SIGTERM',interrupt); }
  } finally {
    await stop(child);
    // A hard interruption can bypass the inner finally. Kill only the recorded
    // benchmark app after checking both its executable and isolated data root.
    try {
      const owned = JSON.parse(await readFile(join(scratch,'app-pid.json'),'utf8'));
      try {
        const executable = await readlink(`/proc/${owned.pid}/exe`);
        const environment = (await readFile(`/proc/${owned.pid}/environ`,'utf8')).split('\0');
        assert.equal(executable,binary); assert.ok(environment.includes(`XDG_DATA_HOME=${join(scratch,'data')}`));
        try { process.kill(-owned.pid,'SIGKILL'); } catch(error) { if(error.code!=='ESRCH') throw error; }
      } catch(error) { if(error.code!=='ENOENT' && error.code!=='ESRCH') throw error; }
    } catch(error) { if(error.code!=='ENOENT') throw error; }
    try { await access(join(evidence,'result.json')); }
    catch(error) {
      if(error.code!=='ENOENT') throw error;
      let submitted=false; try { await access(join(evidence,'submission.json')); submitted=true; } catch(problem) { if(problem.code!=='ENOENT') throw problem; }
      await writeFile(join(evidence,'result.json'),JSON.stringify({version:1,scenarioId:scenario.id,startedAt:new Date().toISOString(),quality:'UNVERIFIED',status:submitted?'outcomeUncertain':'notSubmitted',providerAttempts:submitted?1:0,error:'Interrupted before evidence collection finished. Never automatically retry.'},null,2),{mode:0o600});
    }
    await rm(scratch,{recursive:true,force:true});
    const after = await desktopState(); assert.deepEqual(after.resources,original.resources); if(original.connected) assert.equal(after.connected,true);
    await writeFile(join(evidence,'desktop-cleanup.json'),JSON.stringify({before:original,after,resourceIdentity:'passed'},null,2),{mode:0o600});
    console.log(`Benchmark evidence: ${evidence}`);
  }
}

try { if (process.argv.length === 3 && process.argv[2] === '--isolated') await isolated(); else await main(); }
catch (error) { console.error(error.message); process.exitCode = 1; }
