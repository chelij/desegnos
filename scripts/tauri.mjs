import { spawn } from 'node:child_process';
import { toolchainEnvironment } from './toolchain.mjs';

const env = toolchainEnvironment();
// Clap expects a boolean string, while CI providers commonly export CI=1.
if (env.CI === '1') env.CI = 'true';
const child = spawn(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js', ...process.argv.slice(2)], { env, stdio: 'inherit' });
child.on('error', error => { console.error(`Desegnos build: ${error.message}`); process.exitCode = 1; });
child.on('exit', (code, signal) => { process.exitCode = code ?? (signal ? 1 : 0); });
