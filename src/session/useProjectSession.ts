import { useCallback, useRef, useState } from 'react';
import { edit } from '../api';
import { errorMessage } from '../editorShared';
import type { Command, Project, WorkspaceView } from '../types';

/** One owner for native workspace publication and serialized project mutations. */
export function useProjectSession(initialView: WorkspaceView | undefined, setError: (message: string) => void) {
  const [view, setView] = useState<WorkspaceView | null>(initialView ?? null);
  const current = useRef<WorkspaceView | null>(initialView ?? null);
  const pendingNudge = useRef<{ key: string; command: Extract<Command, { type: 'nudge_elements' }>; promise: Promise<WorkspaceView | null> } | null>(null);
  const tail = useRef<Promise<void>>(Promise.resolve());
  const [pending, setPending] = useState(0);
  const accept = useCallback((next: WorkspaceView) => { current.current = next; setView(next); }, []);
  const queue = useCallback(<T,>(operation: (workspace: WorkspaceView | null) => Promise<T>, reportErrors = true): Promise<T> => {
    pendingNudge.current = null;
    setPending(count => count + 1);
    const result = tail.current.then(() => operation(current.current));
    tail.current = result.then(() => undefined, () => undefined);
    return result.catch(error => { if (reportErrors) setError(errorMessage(error)); throw error; }).finally(() => { setPending(count => count - 1); });
  }, []);
  const mutate = useCallback((command: Command | ((project: Project) => Command | null)) => {
    const projectId = current.current?.project?.id;
    const nudge = typeof command !== 'function' && command.type === 'nudge_elements' ? command : null;
    const key = nudge ? JSON.stringify([projectId, nudge.moves.map(move => [move.id, move.size_id, move.all_sizes]).sort()]) : null;
    if (nudge && pendingNudge.current?.key === key) {
      pendingNudge.current.command.dx += nudge.dx;
      pendingNudge.current.command.dy += nudge.dy;
      return pendingNudge.current.promise;
    }
    const promise = queue(async workspace => {
      if (nudge && pendingNudge.current?.command === nudge) pendingNudge.current = null;
      const project = workspace?.project;
      if (!project || project.id !== projectId) throw new Error('The active project changed before this edit.');
      const resolved = typeof command === 'function' ? command(project) : command;
      if (!resolved) return workspace!;
      const next = await edit(project.revision, resolved);
      accept(next); return next;
    }, false).catch(error => { setError(errorMessage(error)); return null; });
    if (nudge && key) pendingNudge.current = { key, command: nudge, promise };
    return promise;
  }, [queue, accept]);
  const settle = useCallback(async () => { await tail.current; }, []);
  return { view, current, pending, accept, queue, mutate, settle };
}
