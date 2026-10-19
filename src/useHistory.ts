import { useCallback, useRef, useState } from 'react';
import type { Project } from './types';

export type HistoryEntry = { label: string; at: number; fingerprint: string };

function hash(text: string) {
  let value = 0x811c9dc5;
  for (let index = 0; index < text.length; index += 1) { value ^= text.charCodeAt(index); value = Math.imul(value, 0x01000193); }
  return (value >>> 0).toString(36) + text.length.toString(36);
}

// What the user would call "the project": pages, boxes, content and the brief.
// Element bounds and bookkeeping (revision, job records) are left out so background refreshes don't count as changes.
export function fingerprint(project: Project) {
  return hash(JSON.stringify([project.name, project.brief, project.decisions, project.exceptions, project.pages.map(page => [
    page.id, page.name, page.path, page.included, page.viewport, page.appearance, page.content,
    page.regions.map(region => [region.id, region.prompt, region.completed, region.element ? region.element.selector : region.rect]),
  ])]));
}

function describe(before: Project, after: Project): string {
  if (after.appliedJobIds.some(id => !before.appliedJobIds.includes(id))) return 'AI built the page';
  if (after.name !== before.name) return 'Renamed project';
  if (after.pages.length > before.pages.length) return `Added page “${after.pages.at(-1)!.name}”`;
  if (after.pages.length < before.pages.length) return 'Removed a page';
  for (const page of after.pages) {
    const old = before.pages.find(item => item.id === page.id);
    if (!old) continue;
    if (page.regions.length > old.regions.length) return `Added a box on ${page.name}`;
    if (page.regions.length < old.regions.length) return `Removed a box on ${page.name}`;
    if (JSON.stringify(page.content) !== JSON.stringify(old.content)) return `Edited ${page.name}`;
    const changed = page.regions.find(region => { const previous = old.regions.find(item => item.id === region.id); return previous && previous.prompt !== region.prompt; });
    if (changed) return `Described a box on ${page.name}`;
    if (page.regions.some(region => { const previous = old.regions.find(item => item.id === region.id); return previous && !region.element && JSON.stringify(previous.rect) !== JSON.stringify(region.rect); })) return `Moved a box on ${page.name}`;
    if (page.name !== old.name || page.path !== old.path || page.appearance !== old.appearance || page.included !== old.included || JSON.stringify(page.viewport) !== JSON.stringify(old.viewport)) return `Changed ${page.name} settings`;
  }
  if (JSON.stringify([after.brief, after.decisions, after.exceptions]) !== JSON.stringify([before.brief, before.decisions, before.exceptions])) return 'Updated project brief';
  return 'Changed the project';
}

// A session list of changes mirroring Undo/Redo. Jumping matches project fingerprints, so it stays correct
// even if the native undo stack holds steps this list didn't see.
export function useHistory() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [position, setPosition] = useState(0);
  const state = useRef({ projectId: null as string | null, previous: null as Project | null, entries: [] as HistoryEntry[], position: 0, travelling: false });
  const commit = (entries: HistoryEntry[], position: number) => { state.current.entries = entries; state.current.position = position; setEntries(entries); setPosition(position); };
  const observe = useCallback((project: Project | null) => {
    const current = state.current;
    if (!project) { current.projectId = null; current.previous = null; commit([], 0); return; }
    const print = fingerprint(project);
    if (project.id !== current.projectId) {
      current.projectId = project.id; current.previous = project;
      commit([{ label: 'Opened project', at: Date.now(), fingerprint: print }], 1); return;
    }
    const last = current.entries[current.position - 1];
    if (last?.fingerprint === print) { current.previous = project; current.travelling = false; return; }
    if (current.travelling) {
      const index = current.entries.findIndex(entry => entry.fingerprint === print);
      if (index >= 0) commit(current.entries, index + 1);
      current.previous = project; current.travelling = false; return;
    }
    const entry = { label: current.previous ? describe(current.previous, project) : 'Changed the project', at: Date.now(), fingerprint: print };
    commit([...current.entries.slice(0, current.position), entry].slice(-200), Math.min(200, current.position + 1));
    current.previous = project;
  }, []);
  const travelling = useCallback((value: boolean) => { state.current.travelling = value; }, []);
  return { entries, position, observe, travelling, state };
}
