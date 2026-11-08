import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { toolchainEnvironment } from './toolchain.mjs';

export async function runCommand(program, args, { env = toolchainEnvironment(), capture = false, cwd } = {}) {
  const child = spawn(program, args, { cwd, env, stdio: ['inherit', capture ? 'pipe' : 'inherit', 'inherit'] });
  let output = '';
  if (capture) child.stdout.setEncoding('utf8').on('data', chunk => { output += chunk; });
  await new Promise((accept, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => code === 0 ? accept() : reject(Object.assign(new Error(`${program} failed (${signal ?? code}).`), { exitCode: code ?? 1 })));
  });
  return output;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [program, ...args] = process.argv.slice(2);
  const env = toolchainEnvironment();
  // Clap expects a boolean string, while CI providers commonly export CI=1.
  if (program === 'tauri' && env.CI === '1') env.CI = 'true';
  try {
    if (!program) throw new Error('Usage: node scripts/run.mjs <cargo|tauri|program> [...args]');
    await runCommand(program === 'tauri' ? process.execPath : program,
      program === 'tauri' ? ['node_modules/@tauri-apps/cli/tauri.js', ...args] : args, { env });
  } catch (error) { console.error(error.message); process.exitCode = error.exitCode ?? 1; }
}
