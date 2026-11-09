// Builds the updater's latest.json from a release's .sig files, once, after every platform
// has uploaded. Build jobs writing it in parallel raced and dropped platforms.
// Usage: node scripts/updater-manifest.mjs <sig-dir> <owner/repo> <tag>  (prints JSON)
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const platforms = [
  [/_amd64\.AppImage\.sig$/, ['linux-x86_64', 'linux-x86_64-appimage']],
  [/_amd64\.deb\.sig$/, ['linux-x86_64-deb']],
  [/\.x86_64\.rpm\.sig$/, ['linux-x86_64-rpm']],
  [/_x64\.app\.tar\.gz\.sig$/, ['darwin-x86_64', 'darwin-x86_64-app']],
  [/_aarch64\.app\.tar\.gz\.sig$/, ['darwin-aarch64', 'darwin-aarch64-app']],
  [/_x64_en-US\.msi\.sig$/, ['windows-x86_64', 'windows-x86_64-msi']],
  [/_x64-setup\.exe\.sig$/, ['windows-x86_64-nsis']],
];

export function updaterManifest(signatures, repository, tag, date = new Date()) {
  const manifest = { version: tag.replace(/^v/, ''), notes: '', pub_date: date.toISOString(), platforms: {} };
  for (const [pattern, keys] of platforms) {
    const matches = Object.keys(signatures).filter(name => pattern.test(name));
    if (matches.length !== 1) throw new Error(`Expected one ${pattern} signature, found ${matches.length}`);
    const asset = matches[0].slice(0, -'.sig'.length);
    const entry = { signature: signatures[matches[0]].trim(), url: `https://github.com/${repository}/releases/download/${tag}/${encodeURIComponent(asset)}` };
    for (const key of keys) manifest.platforms[key] = entry;
  }
  return manifest;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [directory, repository, tag] = process.argv.slice(2);
  if (!directory || !repository || !tag) throw new Error('Usage: updater-manifest.mjs <sig-dir> <owner/repo> <tag>');
  const signatures = Object.fromEntries(readdirSync(directory).filter(name => name.endsWith('.sig')).map(name => [name, readFileSync(join(directory, name), 'utf8')]));
  console.log(JSON.stringify(updaterManifest(signatures, repository, tag), null, 2));
}
