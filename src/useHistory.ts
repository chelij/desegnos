import type { HistoryEntry, WorkspaceView } from './types';
export type { HistoryEntry } from './types';

/** Native history is authoritative, including coalesced edits, undo and redo. */
export function useHistory(workspace: WorkspaceView | null): { entries: HistoryEntry[] } {
  return { entries: workspace?.history?.entries ?? [] };
}
