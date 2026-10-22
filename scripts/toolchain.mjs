import { existsSync, readFileSync } from 'node:fs';
import { resolve, delimiter } from 'node:path';
import { homedir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));

export function toolchainEnvironment() {
  const cargoHome = resolve(root, '.tools/cargo');
  const systemBin = resolve(homedir(), '.cargo/bin');
  const rustc = resolve(systemBin, process.platform === 'win32' ? 'rustc.exe' : 'rustc');
  const pinned = readFileSync(resolve(root, 'rust-toolchain.toml'), 'utf8').match(/channel\s*=\s*"([^"]+)"/)?.[1];
  const environment = { ...process.env };
  if (existsSync(cargoHome)) environment.CARGO_HOME = cargoHome;
  if (existsSync(rustc)) {
    const pathKey = Object.keys(environment).find(key => key.toLowerCase() === 'path') ?? 'PATH';
    environment[pathKey] = systemBin + delimiter + (environment[pathKey] ?? '');
    const candidate = { ...environment, RUSTUP_TOOLCHAIN: 'stable' };
    const version = spawnSync(rustc, ['--version'], { env: candidate, encoding: 'utf8' });
    if (version.status === 0 && version.stdout.split(' ')[1] === pinned) {
      return candidate;
    }
  }
  return environment;
}

