import { useCallback, useState } from 'react';
import type { DossierSection } from '../Notes';
import type { IdentitySource } from '../types';

export type EditorPanel =
  | { kind: 'closed' }
  | { kind: 'notes'; section: DossierSection }
  | { kind: 'import'; source: IdentitySource | null }
  | { kind: 'log'; focus: number }
  | { kind: 'gridSettings' | 'styleGuide' | 'shortcuts' | 'recovery' | 'rename' | 'projectLocation' | 'history' | 'export' };

export type OpenPanel = Exclude<EditorPanel, { kind: 'closed' }>;

/** The one surface shown beside the canvas (or on the start page) at a time. */
export function useEditorPanel() {
  const [panel, setPanel] = useState<EditorPanel>({ kind: 'closed' });
  const openPanel = useCallback((next: OpenPanel) => {
    setPanel(previous => next.kind === 'log' && next.focus === 0 && previous.kind === 'log' ? previous : next);
  }, []);
  const closePanel = useCallback((kind: EditorPanel['kind']) => {
    setPanel(previous => previous.kind === kind ? { kind: 'closed' } : previous);
  }, []);
  const togglePanel = useCallback((next: OpenPanel) => {
    setPanel(previous => previous.kind === next.kind ? { kind: 'closed' } : next);
  }, []);
  return { panel, openPanel, closePanel, togglePanel };
}
