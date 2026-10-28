import { useCallback, useRef, useState } from 'react';
import { edit } from '../api';
import { errorMessage } from '../editorShared';
import type { Command, Project, WorkspaceView } from '../types';

/** One owner for native workspace publication and serialized project mutations. */
export function useProjectSession(initialView: WorkspaceView | undefined, setError: (message: string) => void) {
  const [view, setView] = useState<WorkspaceView | null>(initialView ?? null);
  const current = useRef<WorkspaceView | null>(initialView ?? null);
  const tail = useRef<Promise<void>>(Promise.resolve());
  const [pending, setPending] = useState(0);
  const accept = useCallback((next: WorkspaceView) => { current.current = next; setView(next); }, []);
  const queue = useCallback(<T,>(operation: (workspace: WorkspaceView | null) => Promise<T>, reportErrors = true, quiet = false): Promise<T> => {
    if (!quiet) setPending(count => count + 1);
    const result = tail.current.then(() => operation(current.current));
    tail.current = result.then(() => undefined, () => undefined);
    return result.catch(error => { if (reportErrors) setError(errorMessage(error)); throw error; }).finally(() => { if (!quiet) setPending(count => count - 1); });
  }, []);
  const mutate = useCallback((command: Command | ((project: Project) => Command | null), quiet = false) => queue(async workspace => {
    const project = workspace?.project;
    if (!project) throw new Error('Create or open a project first.');
    const resolved = typeof command === 'function' ? command(project) : command;
    if (!resolved) return workspace!;
    const next = await edit(project.revision, resolved);
    accept(next); return next;
  }, true, quiet), [queue, accept]);
  const settle = useCallback(async () => { await tail.current; }, []);
  const synchronize = useCallback(async () => { await settle(); }, [settle]);
  return { view, current, pending, accept, queue, mutate, settle, synchronize };
}
