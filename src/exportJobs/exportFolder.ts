import { documentDir, join } from '@tauri-apps/api/path';

export async function defaultExportFolder(current?: string): Promise<string> {
  return current ?? await join(await documentDir(), 'Desegnos');
}
