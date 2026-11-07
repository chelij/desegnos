import { invoke, isTauri } from '@tauri-apps/api/core';
import type { CommandName } from './generated/commands';
import type { Command, WorkspaceView } from './types';
import { recordCommand } from './usage';

export function native<T>(command: CommandName, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error('Open Desegnos as a desktop application to access your projects.'));
  const started = performance.now();
  return invoke<T>(command, args).then(
    value => { recordCommand(command, args, started, true); return value; },
    error => { recordCommand(command, args, started, false, error); throw error; });
}

export function edit(revision: number, command: Command) {
  return native<WorkspaceView>('edit_project', { revision, command });
}
