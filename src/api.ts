import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Command, WorkspaceView } from './types';

export function native<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error('Open Desegnos as a desktop application to access your projects.'));
  return invoke<T>(command, args);
}

export function edit(revision: number, command: Command) {
  return native<WorkspaceView>('edit_project', { revision, command });
}
