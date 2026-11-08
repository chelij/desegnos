import { stat } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { toolchainEnvironment } from './toolchain.mjs';
import { runCommand } from './run.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));

// Local rebuilds skip link-time optimization and use parallel, incremental codegen: about a
// minute instead of four after a change. --dist keeps the full release profile for publishing.
export const localProfile = { CARGO_PROFILE_RELEASE_LTO: 'false', CARGO_PROFILE_RELEASE_CODEGEN_UNITS: '16', CARGO_PROFILE_RELEASE_INCREMENTAL: 'true' };

export async function buildRelease({ run = (program, args, options) => runCommand(program, args, { ...options, cwd: root }), inspect = stat, env = toolchainEnvironment(), platform = process.platform, dist = false } = {}) {
  await run(process.execPath, [join(root, 'scripts/run.mjs'), 'tauri', 'build', '--no-bundle', '--features', 'custom-protocol'], { env: dist ? env : { ...env, ...localProfile } });
  const metadata = JSON.parse(await run('cargo', ['metadata', '--offline', '--no-deps', '--format-version', '1'], { env, capture: true }));
  const target = env.CARGO_BUILD_TARGET;
  const windows = target ? target.includes('windows') : platform === 'win32';
  const binary = join(metadata.target_directory, ...(target ? [target] : []), 'release', windows ? 'desegnos.exe' : 'desegnos');
  const information = await inspect(binary);
  console.log(`${binary}: ${information.size} bytes; modified ${information.mtime.toISOString()}`);
  if (platform === 'linux' && (!target || target.includes('linux'))) {
    await run(process.execPath, [join(root, 'scripts/startup-test.mjs'), '--binary', binary], { env });
  } else console.log('Skipping guarded Xvfb startup check: it requires a Linux host and Linux executable.');
}

const entry = process.argv[1] && resolve(process.argv[1]);
const script = fileURLToPath(import.meta.url);
if (entry && (process.platform === 'win32' ? entry.toLowerCase() === script.toLowerCase() : entry === script)) {
  try { await buildRelease({ dist: process.argv.includes('--dist') }); }
  catch (error) { console.error(`Desegnos release: ${error.message}`); process.exitCode = 1; }
}
