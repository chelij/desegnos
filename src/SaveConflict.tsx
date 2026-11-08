import { Notice } from './ProblemBanner.js';
import { WorkspaceSurface } from './WorkspaceSurface';
import type { FileStamp, SaveConflict, SaveOutcome } from './types';

export type { FileStamp, SaveConflict, SaveOutcome };
export type SaveChoice = 'copy' | 'replace' | 'cancel';

export function SaveConflictDialog({ conflict, choose }: { conflict: SaveConflict; choose: (choice: SaveChoice) => void }) {
  const different = conflict.kind === 'differentProject';
  const sentence = conflict.kind === 'changed' ? `“${conflict.fileName}” changed on disk.` : different ? `“${conflict.fileName}” belongs to another project.` : 'This file cannot be replaced.';
  return <WorkspaceSurface place="banner" title="Save conflict" showHeader={false} close={() => choose('cancel')}>
    <Notice className="save-conflict" dismissKey={conflict.token} autoFocus={conflict.defaultChoice === 'copy' ? 'primary' : 'dismiss'} variant="warning" message={sentence} dismiss={() => choose('cancel')}
      action={{ label: 'Save as copy', onClick: () => choose('copy') }}
      secondaryAction={conflict.canReplace ? { label: 'Replace it', onClick: () => choose('replace') } : undefined} />
  </WorkspaceSurface>;
}
