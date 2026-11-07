import { useEffect, useState } from 'react';
import { readPreference, writePreference } from '../preferences';
import type { Theme } from '../editorShared';
const THEME_KEY = 'desegnos.editor-theme';
export function useEditorTheme() {
  const [theme, setTheme] = useState<Theme>(() => { const value = readPreference(THEME_KEY); return value === 'light' || value === 'dark' ? value : 'system'; });
  const [resolvedTheme, setResolvedTheme] = useState<'light' | 'dark'>('light');
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const update = () => { const value = theme === 'system' ? (media.matches ? 'dark' : 'light') : theme; document.documentElement.dataset.theme = value; setResolvedTheme(value); };
    update(); media.addEventListener('change', update);
    writePreference(THEME_KEY, theme);
    return () => media.removeEventListener('change', update);
  }, [theme]);
  return { theme, setTheme, resolvedTheme };
}
