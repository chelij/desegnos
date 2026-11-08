import { useState } from 'react';
import { native } from '../api';
import { errorMessage } from '../errorMessage.js';
import { updateSummary, type UpdateCheck, type UpdateSummary } from '../updateCheck';

type UpdateState =
  | { kind: 'idle' | 'checking' }
  | { kind: 'done'; result: UpdateSummary }
  | { kind: 'failed'; message: string };

export function useUpdateCheck(setOpenFailure: (failure: { message: string; url: string }) => void) {
  const [updateState, setUpdateState] = useState<UpdateState>({ kind: 'idle' });
  async function checkForUpdates() {
    setUpdateState({ kind: 'checking' });
    try { setUpdateState({ kind: 'done', result: updateSummary(await native<UpdateCheck>('check_for_updates')) }); }
    catch (error) { setUpdateState({ kind: 'failed', message: errorMessage(error) }); }
  }
  async function openRelease(url: string) {
    try { await native('open_preview_link', { url }); }
    catch { setOpenFailure({ message: url, url }); }
  }
  return { updateState, dismissUpdate: () => setUpdateState({ kind: 'idle' }), checkForUpdates, openRelease };
}
