import { readPreference, writePreference } from './preferences';
import { useEffect, useState } from 'react';

// How Ctrl/Cmd+V and Ctrl/Cmd+D paste. Unset means Unlinked; the explicit choice
// shortcut (Ctrl/Cmd+Shift+V) always asks.
export type PasteMode = 'linked' | 'unlinked';
const KEY = 'desegnos.paste-mode';
const EVENT = 'desegnos-paste-mode';

export function pasteDefault(): PasteMode | null {
  const saved = readPreference(KEY);
  return saved === 'linked' || saved === 'unlinked' ? saved : null;
}

export function setPasteDefault(value: PasteMode | null) {
  writePreference(KEY, value);
  window.dispatchEvent(new Event(EVENT));
}

export function usePasteDefault(): PasteMode | null {
  const [value, setValue] = useState(pasteDefault);
  useEffect(() => {
    const update = () => setValue(pasteDefault());
    window.addEventListener(EVENT, update);
    window.addEventListener('storage', update);
    return () => { window.removeEventListener(EVENT, update); window.removeEventListener('storage', update); };
  }, []);
  return value;
}
