import { spawn } from 'node:child_process';
import { chmod, mkdir, mkdtemp, readFile, readlink, realpath, rm, stat, writeFile } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { toolchainEnvironment } from './toolchain.mjs';
import { startProviderServer, assertNoSyntheticKeys } from '../tests/native/provider-server.mjs';

const workspace = fileURLToPath(new URL('../', import.meta.url));
const script = fileURLToPath(import.meta.url);
const flag = 'private-secret-service-v1';
const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const children = new Map();
let interrupted = false;
for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, () => {
  interrupted = true;
  for (const [child, detached] of children) {
    try { if (detached && child.pid) process.kill(-child.pid, 'SIGTERM'); else child.kill('SIGTERM'); } catch {}
  }
});

async function run(command, args, { env = process.env, timeout = 30_000, input, detached = false } = {}) {
  if (interrupted) throw new Error('Vault test interrupted; owned resources are being removed.');
  const child = spawn(command, args, { env, cwd: workspace, detached, stdio: ['pipe', 'pipe', 'pipe'] });
  children.set(child, detached);
  let stdout = '', stderr = '', overflow = false;
  child.stdout.on('data', chunk => { stdout += chunk; if (stdout.length > 4_194_304) { overflow = true; child.kill('SIGKILL'); } });
  child.stderr.on('data', chunk => { stderr += chunk; if (stderr.length > 4_194_304) { overflow = true; child.kill('SIGKILL'); } });
  child.stdin.end(input);
  const timer = setTimeout(() => { if (detached && child.pid) { try { process.kill(-child.pid, 'SIGKILL'); } catch {} } else child.kill('SIGKILL'); }, timeout);
  try {
    const result = await new Promise((resolve, reject) => { child.on('error', reject); child.on('close', (code, signal) => resolve({ code, signal, stdout, stderr })); });
    if (overflow) throw new Error('A test subprocess exceeded the bounded output limit.');
    return result;
  } finally { clearTimeout(timer); children.delete(child); }
}

async function stopOwned(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  child.kill('SIGTERM');
  const deadline = Date.now() + 3000;
  while (child.exitCode === null && child.signalCode === null && Date.now() < deadline) await delay(30);
  if (child.exitCode === null && child.signalCode === null) {
    child.kill('SIGKILL');
    const killed = Date.now() + 2000;
    while (child.exitCode === null && child.signalCode === null && Date.now() < killed) await delay(30);
    if (child.exitCode === null && child.signalCode === null) throw new Error('The owned keyring process did not terminate.');
  }
}

async function inside(binary, directory, evidencePath) {
  const address = process.env.DBUS_SESSION_BUS_ADDRESS;
  if (!address || process.env.DESEGNI_TEST_VAULT_OPT_IN !== flag || directory !== process.env.DESEGNI_TEST_VAULT_ROOT) throw new Error('Private bus isolation guard missing.');
  if (address === process.env.DESEGNI_TEST_PREVIOUS_BUS) throw new Error('The session bus was not replaced.');
  if (!/^\/tmp\/desegni-vault-[^/]+$/.test(directory) || await realpath(directory) !== directory) throw new Error('Private vault storage path is unsafe.');
  const rootInformation = await stat(directory);
  if (rootInformation.uid !== process.getuid() || (rootInformation.mode & 0o077) !== 0) throw new Error('Private vault storage owner or permissions are unsafe.');
  for (const [variable, name] of [['XDG_DATA_HOME', 'data'], ['XDG_CONFIG_HOME', 'config'], ['XDG_CACHE_HOME', 'cache'], ['XDG_RUNTIME_DIR', 'runtime']]) {
    const expected = join(directory, name);
    const information = await stat(expected);
    if (await realpath(process.env[variable]) !== expected || information.uid !== process.getuid() || (information.mode & 0o077) !== 0) throw new Error('Private XDG preflight failed.');
  }
  for (const variable of ['DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET']) if (process.env[variable]) throw new Error('Display access must be cleared before keyring initialization.');
  const parent = await readlink(`/proc/${process.ppid}/exe`);
  if (!parent.endsWith('/dbus-run-session')) throw new Error('The vault test must be a direct child of its private dbus-run-session.');
  const busOwner = await run('busctl', ['--address', address, '--timeout=2', 'call', 'org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'GetConnectionUnixProcessID', 's', 'org.freedesktop.DBus']);
  const busPid = /^u (\d+)\s*$/.exec(busOwner.stdout)?.[1];
  if (busOwner.code !== 0 || !busPid) throw new Error('Private bus daemon identity is unavailable.');
  const busProcess = (await readFile(`/proc/${busPid}/stat`)).toString().split(') ').at(-1).split(' ');
  if (Number(busProcess[1]) !== process.ppid || !(await readlink(`/proc/${busPid}/exe`)).endsWith('/dbus-daemon')) throw new Error('The bus is not owned by this isolated dbus-run-session.');
  const token = randomBytes(24).toString('hex');
  const password = randomBytes(32).toString('base64');
  const evidence = { version: 1, status: 'running', privateBusVerified: false, privateXdgVerified: false, unsafeInvocationRejected: true, phases: [], keyringRestarts: 0, liveOpenAiCall: false, secretsLogged: false };
  let daemon;
  const provider = await startProviderServer();
  provider.oauth.setMode('complete', 'private-vault', { expiresIn: 30 });
  async function startKeyring() {
    daemon = spawn('gnome-keyring-daemon', ['--foreground', '--unlock', '--components=secrets', `--control-directory=${join(directory, 'runtime/keyring')}`], { env: process.env, stdio: ['pipe', 'ignore', 'pipe'] });
    children.set(daemon, false);
    const ownedDaemon = daemon;
    daemon.on('close', () => children.delete(ownedDaemon));
    daemon.stdin.end(password);
    let daemonError = false;
    daemon.on('error', () => { daemonError = true; });
    daemon.stderr.resume();
    const deadline = Date.now() + 10_000;
    let owned = false;
    while (Date.now() < deadline && !daemonError && daemon.exitCode === null) {
      const response = await run('busctl', ['--address', address, '--timeout=1', 'call', 'org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'GetConnectionUnixProcessID', 's', 'org.freedesktop.secrets'], { timeout: 2000 });
      if (response.code === 0 && response.stdout.trim() === `u ${daemon.pid}`) { owned = true; break; }
      await delay(50);
    }
    if (!owned) throw new Error('Private keyring daemon did not acquire the private Secret Service.');
    const entries = (await readFile(`/proc/${daemon.pid}/environ`)).toString().split('\0');
    const executable = await readlink(`/proc/${daemon.pid}/exe`);
    if (!executable.endsWith('/gnome-keyring-daemon') || !entries.includes(`DBUS_SESSION_BUS_ADDRESS=${address}`)) throw new Error('Private keyring process identity mismatch.');
    for (const [variable, name] of [['XDG_DATA_HOME', 'data'], ['XDG_CONFIG_HOME', 'config'], ['XDG_CACHE_HOME', 'cache'], ['XDG_RUNTIME_DIR', 'runtime']]) {
      const expected = join(directory, name);
      const information = await stat(expected);
      if (await realpath(process.env[variable]) !== expected || !entries.includes(`${variable}=${expected}`) || information.uid !== process.getuid() || (information.mode & 0o077) !== 0) throw new Error('Private XDG isolation verification failed.');
    }
    await writeFile(join(directory, 'isolation.json'), JSON.stringify({ token, busAddress: address, keyringPid: daemon.pid }), { mode: 0o600 });
    evidence.privateBusVerified = true; evidence.privateXdgVerified = true;
  }
  async function phase(name) {
    const started = Date.now();
    const result = await run(binary, ['--ignored', '--exact', 'private_system_vault_lifecycle', '--test-threads=1'], { env: { ...process.env, DESEGNI_TEST_VAULT_TOKEN: token, DESEGNI_TEST_VAULT_PHASE: name, DESEGNI_TEST_CHATGPT_ENDPOINT: provider.endpoint.replace(/\/v1$/, '') }, timeout: 30_000 });
    evidence.phases.push({ name, passed: result.code === 0, durationMs: Date.now() - started });
    if (result.code !== 0) throw new Error(`Private vault phase ${name} failed; subprocess output was withheld to keep credentials out of diagnostics.`);
  }
  try {
    await startKeyring();
    await phase('initial');
    await phase('replace-session');
    await stopOwned(daemon); daemon = undefined;
    await startKeyring(); evidence.keyringRestarts += 1;
    await phase('restart-remove');
    await stopOwned(daemon); daemon = undefined;
    await startKeyring(); evidence.keyringRestarts += 1;
    await phase('empty');
    if (provider.errors().length) throw new Error('The private OAuth fixture rejected its protocol contract.');
    const oauth = provider.oauth.records();
    if (!oauth.some(record => record.grantType === 'authorization_code' && record.pkceValidated)
      || !oauth.some(record => record.grantType === 'refresh_token' && record.replacement)
      || !oauth.some(record => record.confirmed && record.path.endsWith('/revoke'))) throw new Error('The private OAuth lifecycle did not verify exchange, token rotation and revocation.');
    evidence.chatgpt = { signedOAuth: true, systemTokenBundle: true, persistedAcrossProcessesAndDaemons: true,
      rotatedRefresh: true, revokedAndRemoved: true, authenticatedModelReads: provider.modelRequests().filter(record => record.authentication === 'chatgpt').length, protocol: oauth };
    evidence.credentialScan = await assertNoSyntheticKeys([join(directory, 'data'), join(directory, 'chatgpt')]);
    evidence.providers = {separateServices:true,independentKeys:true,persistedAcrossProcessesAndDaemons:true,independentRemoval:true};
    evidence.status = 'passed';
  } catch (error) { evidence.status = 'failed'; evidence.error = error.message; throw error; }
  finally { await stopOwned(daemon); await provider.close(); await writeFile(evidencePath, JSON.stringify(evidence, null, 2) + '\n'); }
}

async function outer() {
  if (process.platform !== 'linux') throw new Error('This opt-in vault integration test requires Linux Secret Service.');
  const evidenceDirectory = resolve(workspace, 'test-results/system-vault', `${new Date().toISOString().replaceAll(':', '-')}-${process.pid}`);
  await mkdir(evidenceDirectory, { recursive: true });
  const evidencePath = join(evidenceDirectory, 'evidence.json');
  const directory = await mkdtemp('/tmp/desegni-vault-');
  await chmod(directory, 0o700);
  const environment = toolchainEnvironment();
  for (const variable of Object.keys(environment)) if (variable.startsWith('DBUS_') || variable.startsWith('GNOME_KEYRING_') || variable.startsWith('DESEGNI_TEST_')) delete environment[variable];
  for (const variable of ['DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'XAUTHORITY', 'SSH_AUTH_SOCK', 'OPENAI_API_KEY']) delete environment[variable];
  for (const [variable, name] of [['XDG_DATA_HOME', 'data'], ['XDG_CONFIG_HOME', 'config'], ['XDG_CACHE_HOME', 'cache'], ['XDG_RUNTIME_DIR', 'runtime']]) {
    environment[variable] = join(directory, name); await mkdir(environment[variable], { mode: 0o700 });
  }
  environment.CARGO_TARGET_DIR = join(directory, 'target');
  try {
    const build = await run('cargo', ['test', '--offline', '--locked', '-p', 'desegni-ai', '--test', 'system_vault', '--features', 'test-support', '--no-run', '--message-format=json'], { env: environment, timeout: 240_000 });
    if (build.code !== 0) throw new Error('The opt-in vault integration binary did not compile.');
    const artifact = build.stdout.split('\n').filter(Boolean).map(line => { try { return JSON.parse(line); } catch { return null; } }).find(value => value?.reason === 'compiler-artifact' && value.target?.name === 'system_vault' && value.executable);
    if (!artifact) throw new Error('The opt-in vault integration binary was not produced.');
    const rejection = await run(artifact.executable, ['--ignored', '--exact', 'private_system_vault_lifecycle', '--test-threads=1'], { env: environment });
    if (rejection.code === 0 || !(rejection.stdout + rejection.stderr).includes('refusing to access any system vault')) throw new Error('The vault test did not reject an unguarded invocation.');
    environment.DESEGNI_TEST_VAULT_OPT_IN = flag;
    environment.DESEGNI_TEST_VAULT_ROOT = directory;
    environment.DESEGNI_TEST_PREVIOUS_BUS = process.env.DBUS_SESSION_BUS_ADDRESS ?? '';
    const result = await run('dbus-run-session', ['--', process.execPath, script, '--inside', artifact.executable, directory, evidencePath], { env: environment, timeout: 150_000, detached: true });
    if (result.code !== 0) throw new Error('The isolated Secret Service lifecycle check failed. See its credential-free evidence report.');
    console.log(`Private system vault lifecycle passed. Evidence: ${evidencePath}`);
  } catch (error) {
    try { await stat(evidencePath); } catch { await writeFile(evidencePath, JSON.stringify({ version: 1, status: 'failed', error: error.message, liveOpenAiCall: false, secretsLogged: false }, null, 2) + '\n'); }
    throw error;
  } finally {
    await rm(directory, { recursive: true, force: true });
    const evidence = JSON.parse(await readFile(evidencePath, 'utf8'));
    evidence.privateStorageRemoved = true;
    await writeFile(evidencePath, JSON.stringify(evidence, null, 2) + '\n');
  }
}

try {
  if (process.argv[2] === '--inside') await inside(process.argv[3], process.argv[4], process.argv[5]);
  else await outer();
} catch (error) { console.error(error.message); process.exitCode = 1; }
