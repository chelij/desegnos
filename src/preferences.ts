export function readPreference(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}

export function writePreference(key: string, value: string | null): void {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch { /* Preferences are optional when browser storage is unavailable. */ }
}

export function readJsonPreference(key: string, fallback: unknown): unknown {
  try { return JSON.parse(readPreference(key) ?? 'null') ?? fallback; } catch { return fallback; }
}
