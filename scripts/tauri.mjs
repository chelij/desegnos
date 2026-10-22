import { spawn, spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { toolchainEnvironment } from './toolchain.mjs';
import { completeLaunchPaths, prepareLaunchPaths, restoreLaunchPaths } from './launch-paths.mjs';

const args = process.argv.slice(2);
const env = toolchainEnvironment();
// Clap expects a boolean string, while CI providers commonly export CI=1.
if (env.CI === '1') env.CI = 'true';
const builds = ['build', 'dev'].includes(args[0]) && !args.some(argument => ['--help', '-h'].includes(argument));
let plan;
try {
  if (builds) {
    const metadata = spawnSync('cargo', ['metadata', '--offline', '--no-deps', '--format-version', '1'], { env, encoding: 'utf8' });
    if (metadata.status !== 0) throw new Error(metadata.stderr || 'Cannot locate the desktop build directory.');
    const value = name => { const index = args.indexOf(name); return index < 0 ? args.find(arg => arg.startsWith(`${name}=`))?.slice(name.length + 1) : args[index + 1]; };
    const target = value('--target') || env.CARGO_BUILD_TARGET;
    const profile = value('--profile') || (args.includes('--debug') || args[0] === 'dev' && !args.includes('--release') ? 'debug' : 'release');
    const directory = join(JSON.parse(metadata.stdout).target_directory, ...(target ? [target] : []), profile === 'dev' ? 'debug' : profile);
    plan = await prepareLaunchPaths(directory, target ? target.includes('windows') : process.platform === 'win32');
  }
  const child = spawn(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js', ...args], { env, stdio: 'inherit' });
  const code = await new Promise((accept, reject) => { child.once('error', reject); child.once('exit', (code, signal) => accept(code ?? (signal ? 1 : 0))); });
  process.exitCode = code;
  if (plan) {
    if (code) await restoreLaunchPaths(plan);
    else {
      const result = await completeLaunchPaths(plan);
      console.log(`Desegnos executable: ${result.path}`);
      if (result.backup) console.log(`Previous executable kept at: ${result.backup}`);
    }
  }
} catch (error) {
  if (plan) try { await restoreLaunchPaths(plan); } catch (restoreError) { console.error(`Previous launch link: ${restoreError.message}`); }
  console.error(`Desegnos build: ${error.message}`); process.exitCode = 1;
}
