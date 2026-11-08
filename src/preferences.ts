export function readPreference(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}

export function writePreference(key: string, value: string | null): void {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch { /* Preferences are optional when browser storage is unavailable. */ }
}

/** Reads a stored JSON value; `parse` turns whatever was stored (or null) into the caller's shape. */
export function readJsonPreference<T>(key: string, parse: (raw: unknown) => T): T {
  try { return parse(JSON.parse(readPreference(key) ?? 'null')); } catch { return parse(null); }
}

export function writeJsonPreference(key: string, value: unknown): void {
  writePreference(key, JSON.stringify(value));
}

export const stringList = (raw: unknown): string[] => Array.isArray(raw) ? raw.filter((item): item is string => typeof item === 'string') : [];
