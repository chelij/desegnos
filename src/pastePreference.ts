import { useEffect, useState } from 'react';

// How Ctrl/Cmd+V and Ctrl/Cmd+D paste. Unset means the inline choice is always shown.
export type PasteMode = 'linked' | 'unlinked';
const KEY = 'desegnos.paste-mode';
const EVENT = 'desegnos-paste-mode';

export function pasteDefault(): PasteMode | null {
  try {
    const saved = localStorage.getItem(KEY);
    return saved === 'linked' || saved === 'unlinked' ? saved : null;
  } catch { return null; }
}

export function setPasteDefault(value: PasteMode | null) {
  try {
    if (value) localStorage.setItem(KEY, value);
    else localStorage.removeItem(KEY);
  } catch { /* Preferences can be unavailable. */ }
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
