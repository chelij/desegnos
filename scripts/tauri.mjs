import { spawn, spawnSync } from 'node:child_process';
import { copyFile, lstat, readFile, readlink, symlink, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { toolchainEnvironment } from './toolchain.mjs';

const args = process.argv.slice(2);
const env = toolchainEnvironment();
const child = spawn(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js', ...args], { env, stdio: 'inherit' });
child.on('error', error => { console.error(error.message); process.exitCode = 1; });
child.on('exit', async (code, signal) => {
  process.exitCode = code ?? (signal ? 1 : 0);
  if (process.exitCode || args[0] !== 'build') return;
  try {
    const metadata = spawnSync('cargo', ['metadata', '--offline', '--no-deps', '--format-version', '1'], { env, encoding: 'utf8' });
    if (metadata.status !== 0) throw new Error(metadata.stderr || 'Cannot locate the desktop build directory.');
    const value = name => { const index = args.indexOf(name); return index < 0 ? args.find(arg => arg.startsWith(`${name}=`))?.slice(name.length + 1) : args[index + 1]; };
    const target = value('--target') || env.CARGO_BUILD_TARGET;
    const profile = value('--profile') || (args.includes('--debug') ? 'debug' : 'release');
    const directory = join(JSON.parse(metadata.stdout).target_directory, ...(target ? [target] : []), profile === 'dev' ? 'debug' : profile);
    const windowsTarget = target ? target.includes('windows') : process.platform === 'win32';
    const executable = windowsTarget ? 'desegni.exe' : 'desegni';
    const alias = join(directory, windowsTarget ? 'desegnos.exe' : 'desegnos');
    await lstat(join(directory, executable));
    const existing = await lstat(alias).catch(error => { if (error.code !== 'ENOENT') throw error; return null; });
    if (process.platform === 'win32') {
      const receipt = `${alias}.sha256`;
      const hash = bytes => createHash('sha256').update(bytes).digest('hex');
      if (existing && hash(await readFile(alias)) !== await readFile(receipt, 'utf8').catch(() => '')) throw new Error(`Refusing to replace an unrelated executable at ${alias}.`);
      await copyFile(join(directory, executable), alias);
      await writeFile(receipt, hash(await readFile(alias)));
    } else if (existing) {
      if (!existing.isSymbolicLink() || await readlink(alias) !== executable) throw new Error(`Refusing to replace an unrelated file at ${alias}.`);
    } else await symlink(executable, alias);
    console.log(`Desegnos launch path: ${alias}`);
  } catch (error) { console.error(`Desegnos launch alias: ${error.message}`); process.exitCode = 1; }
});
