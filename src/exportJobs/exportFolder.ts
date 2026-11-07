import { documentDir, join } from '@tauri-apps/api/path';

/** Keep custom choices; migrate the old automatic project folder to its parent. */
export async function defaultExportFolder(name: string, current?: string): Promise<string> {
  const parent = await join(await documentDir(), 'Desegnos');
  const clean = name.trim().replace(/[\\/:*?"<>|\u0000-\u001f]/g, ' ').replace(/\s+/g, ' ').replace(/^[.\s]+|[.\s]+$/g, '').slice(0, 80);
  const legacyDefault = await join(parent, clean || 'Untitled project');
  return !current || current === legacyDefault ? parent : current;
}
