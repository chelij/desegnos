process.env.UPDATE_GENERATED = '1';
process.argv.splice(2, process.argv.length, 'test', '-p', 'desegnos-desktop', '--bin', 'desegnos', 'generated_typescript_is_current');
await import('./cargo.mjs');
