import { useCallback, useRef, useState } from 'react';
import type { Project, WorkspaceView } from './types';

export type HistoryEntry = { label: string; at: number; position: number };

function describe(before: Project, after: Project): string {
  if (after.appliedJobIds.some(id => !before.appliedJobIds.includes(id))) return 'AI updated layers';
  if (after.name !== before.name) return 'Renamed project';
  if (after.pages.length > before.pages.length) return `Added boundary “${after.pages.at(-1)!.name}”`;
  if (after.pages.length < before.pages.length) return 'Removed a boundary';
  for (const page of after.pages) {
    const old = before.pages.find(item => item.id === page.id);
    if (!old) continue;
    if (JSON.stringify(page.elements) !== JSON.stringify(old.elements)) return `Changed something on ${page.name}`;
    if (page.name !== old.name || page.path !== old.path || page.appearance !== old.appearance || page.included !== old.included || JSON.stringify(page.sizes) !== JSON.stringify(old.sizes)) return `Changed ${page.name} settings`;
  }
  if (JSON.stringify([after.brief, after.decisions, after.exceptions]) !== JSON.stringify([before.brief, before.decisions, before.exceptions])) return 'Updated project brief';
  return 'Changed the project';
}

// Native positions distinguish identical edits and include steps the renderer did not observe.
export function useHistory() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const state = useRef<{ sessionId: string | null; previous: Project | null; version: number; entries: HistoryEntry[] }>({ sessionId: null, previous: null, version: 0, entries: [] });
  const observe = useCallback((workspace: WorkspaceView | null) => {
    const current = state.current;
    const project = workspace?.project, history = workspace?.history;
    if (!project || !history) { current.sessionId = null; current.previous = null; current.entries = []; setEntries([]); return; }
    if (history.sessionId !== current.sessionId) {
      current.entries = [{ label: 'Opened project', at: Date.now(), position: history.position }];
    } else {
      current.entries = current.entries.filter(entry => entry.position >= history.start && entry.position <= history.end);
      if (history.version !== current.version) {
        current.entries = current.entries.filter(entry => entry.position < history.position);
        current.entries.push({ label: current.previous ? describe(current.previous, project) : 'Changed the project', at: Date.now(), position: history.position });
      } else if (!current.entries.some(entry => entry.position === history.position)) {
        current.entries.push({ label: 'Earlier change', at: Date.now(), position: history.position });
        current.entries.sort((a, b) => a.position - b.position);
      }
    }
    current.sessionId = history.sessionId; current.previous = project; current.version = history.version;
    setEntries(current.entries);
  }, []);
  return { entries, observe };
}
