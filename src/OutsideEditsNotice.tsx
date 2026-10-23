import { Notice } from './ProblemBanner';
import type { OutsideEdits } from './types';

export function OutsideEditsNotice({ edits, busy, importChanges, overwrite, dismiss }: {
  edits: OutsideEdits; busy: boolean; importChanges: () => void; overwrite: () => void; dismiss: () => void;
}) {
  const count = new Set([...edits.changedPaths, ...edits.keptStalePaths]).size;
  if (!count) return null;
  return <Notice variant="warning" message={`${count} website files were changed outside Desegnos.`}
    action={{ label: 'Import changes', onClick: importChanges, disabled: busy }}
    secondaryAction={{ label: 'Overwrite', onClick: overwrite, disabled: busy }} dismiss={dismiss} />;
}
