import { useLayoutEffect, useRef } from 'react';
import { WorkspaceSurface } from './WorkspaceSurface';
import type { WorkspaceView } from './types';

export type FileStamp = { projectId: string; revision: number; manifestSha256: string };
export type SaveConflict = { token: string; kind: 'changed' | 'differentProject' | 'unreadable'; path: string; fileName: string; message: string; defaultChoice: 'copy' | 'cancel'; canReplace: boolean; details: { expected: FileStamp | null; actual: FileStamp | null } };
export type SaveOutcome = { status: 'saved'; workspace: WorkspaceView } | { status: 'cancelled' } | { status: 'conflict'; conflict: SaveConflict };
export type SaveChoice = 'copy' | 'replace' | 'cancel';

export function SaveConflictDialog({ conflict, choose }: { conflict: SaveConflict; choose: (choice: SaveChoice) => void }) {
  const defaultButton = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { defaultButton.current?.focus(); }, [conflict.token]);
  const different = conflict.kind === 'differentProject';
  const sentence = conflict.kind === 'changed' ? `“${conflict.fileName}” changed on disk. Save a copy or replace it?` : different ? `“${conflict.fileName}” belongs to another project.` : 'This file cannot be replaced. Save a copy with a new name.';
  return <WorkspaceSurface place="banner" title="Save conflict" showHeader={false} close={() => choose('cancel')}>
    <div className="inline-notice save-conflict" role="group" aria-label="Resolve save conflict">
      <span>{sentence}</span>
      <button type="button" className={conflict.defaultChoice === 'copy' ? 'primary' : undefined} ref={conflict.defaultChoice === 'copy' ? defaultButton : undefined} autoFocus={conflict.defaultChoice === 'copy'} onClick={() => choose('copy')}>Save as copy</button>
      {conflict.canReplace && <button type="button" onClick={() => choose('replace')}>Replace it</button>}
      <button type="button" className={conflict.defaultChoice === 'cancel' ? 'primary' : undefined} ref={conflict.defaultChoice === 'cancel' ? defaultButton : undefined} autoFocus={conflict.defaultChoice === 'cancel'} onClick={() => choose('cancel')}>Cancel</button>
      <details><summary>Details</summary><p className="wrap-path">{conflict.path}</p><p>{conflict.canReplace ? 'Replacing overwrites the file with your current work.' : 'Choose another file name.'}</p></details>
    </div>
  </WorkspaceSurface>;
}
