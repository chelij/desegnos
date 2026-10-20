import { useEffect, useState } from 'react';

// "Don't ask again" for deleting elements and requests. Every deletion can still be undone.
// Removing a whole page always asks.
const KEY = 'desegnos.ask-before-delete';
const EVENT = 'desegnos-ask-before-delete';

export function askBeforeDelete() {
  try { return localStorage.getItem(KEY) !== 'no'; } catch { return true; }
}
export function setAskBeforeDelete(value: boolean) {
  try { localStorage.setItem(KEY, value ? 'yes' : 'no'); } catch {}
  window.dispatchEvent(new Event(EVENT));
}
export function useAskBeforeDelete() {
  const [value, setValue] = useState(askBeforeDelete);
  useEffect(() => {
    const update = () => setValue(askBeforeDelete());
    window.addEventListener(EVENT, update); window.addEventListener('storage', update);
    return () => { window.removeEventListener(EVENT, update); window.removeEventListener('storage', update); };
  }, []);
  return value;
}
