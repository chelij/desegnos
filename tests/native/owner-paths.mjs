import { stat } from 'node:fs/promises';

// Owner-only fixtures are opt-in. A missing variable or folder skips the step
// with a plain log line instead of failing on an owner-specific path.
export async function ownerFolder(variable, label) {
  const value = process.env[variable];
  if (!value) { console.log(`${label}: skipped: set ${variable}`); return null; }
  const information = await stat(value).catch(() => null);
  if (information?.isDirectory()) return value;
  console.log(`${label}: skipped: ${variable} is not a folder (${value})`);
  return null;
}

export async function ownerFile(path, variable, label) {
  const information = await stat(path).catch(() => null);
  if (information?.isFile()) return path;
  console.log(`${label}: skipped: ${variable} has no ${path}`);
  return null;
}
