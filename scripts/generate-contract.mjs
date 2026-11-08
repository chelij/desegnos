import { runCommand } from './run.mjs';
import { toolchainEnvironment } from './toolchain.mjs';
await runCommand('cargo', ['test', '-p', 'desegnos-desktop', '--bin', 'desegnos', 'generated_typescript_is_current'], { env: { ...toolchainEnvironment(), UPDATE_GENERATED: '1' } });
