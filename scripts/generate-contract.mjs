import { runCommand } from './run.mjs';
import { toolchainEnvironment } from './toolchain.mjs';
const env = { ...toolchainEnvironment(), UPDATE_GENERATED: '1' };
await runCommand('cargo', ['test', '-p', 'desegnos-desktop', '--bin', 'desegnos', 'generated_typescript_is_current'], { env });
await runCommand('cargo', ['test', '-p', 'desegnos-core', '--test', 'resolution_fixture'], { env });
