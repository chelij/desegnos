import { useLayoutEffect, useRef } from 'react';
import { Modal } from './Modal';
import type { WorkspaceView } from './types';

export type FileStamp = { projectId: string; revision: number; manifestSha256: string };
export type SaveConflict = { token: string; kind: 'changed' | 'differentProject' | 'unreadable'; path: string; fileName: string; message: string; defaultChoice: 'copy' | 'cancel'; canReplace: boolean; details: { expected: FileStamp | null; actual: FileStamp | null } };
export type SaveOutcome = { status: 'saved'; workspace: WorkspaceView } | { status: 'cancelled' } | { status: 'conflict'; conflict: SaveConflict };
export type SaveChoice = 'copy' | 'replace' | 'cancel';

export function SaveConflictDialog({ conflict, choose }: { conflict: SaveConflict; choose: (choice: SaveChoice) => void }) {
  const defaultButton = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { defaultButton.current?.focus(); }, [conflict.token]);
  const different = conflict.kind === 'differentProject';
  const title = conflict.kind === 'changed' ? 'This file changed since you opened it.' : different ? conflict.canReplace ? `${conflict.fileName} is a different project. Replace it?` : 'Save your project with a new name' : 'Choose another file name';
  return <Modal title={title} close={() => choose('cancel')}>
    <div className="dialog-body save-conflict">
      <p>{conflict.kind === 'changed' ? 'Save a copy to keep both versions, or replace the file with your current work.' : different ? conflict.canReplace ? 'Replacing it keeps a backup of the other project.' : 'Your project needs its own file. Save a copy with another name.' : 'This file can’t be replaced. Choose a name ending in .desegnos.'}</p>
      <p className="muted">{conflict.fileName}</p>
      <details><summary>Show details</summary><p className="wrap-path">{conflict.path}</p><p>{conflict.message}</p>
        {(['expected', 'actual'] as const).map(key => { const stamp = conflict.details[key]; return <div key={key}><strong>{key === 'expected' ? 'When opened or last saved' : 'File on disk'}</strong>{stamp ? <dl><dt>Project</dt><dd>{stamp.projectId}</dd><dt>Revision</dt><dd>{stamp.revision}</dd><dt>File hash</dt><dd className="wrap-path">{stamp.manifestSha256}</dd></dl> : <p>Unavailable</p>}</div>; })}
      </details>
      <div className="form-actions">
        <button type="button" className={conflict.defaultChoice === 'copy' ? 'primary' : undefined} ref={conflict.defaultChoice === 'copy' ? defaultButton : undefined} autoFocus={conflict.defaultChoice === 'copy'} onClick={() => choose('copy')}>Save as copy</button>
        {conflict.canReplace && <button type="button" onClick={() => choose('replace')}>Replace it</button>}
        <button type="button" className={conflict.defaultChoice === 'cancel' ? 'primary' : undefined} ref={conflict.defaultChoice === 'cancel' ? defaultButton : undefined} autoFocus={conflict.defaultChoice === 'cancel'} onClick={() => choose('cancel')}>Cancel</button>
      </div>
    </div>
  </Modal>;
}
