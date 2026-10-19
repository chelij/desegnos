import { spawn } from 'node:child_process';
import { toolchainEnvironment } from './toolchain.mjs';

const child = spawn('cargo', process.argv.slice(2), { env: toolchainEnvironment(), stdio: 'inherit' });
child.on('error', error => { console.error(error.message); process.exitCode = 1; });
child.on('exit', (code, signal) => { process.exitCode = code ?? (signal ? 1 : 0); });

