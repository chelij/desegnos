import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef, useState } from 'react';
import { native } from '../api';
import { errorMessage } from '../errorMessage.js';
import { UPDATE_PROGRESS_EVENT } from '../generated/commands';
import type { UpdateProgress } from '../types';
import { updateSummary, type UpdateCheck, type UpdateSummary } from '../updateCheck';

type UpdateState =
  | { kind: 'idle' | 'checking' }
  | { kind: 'installing'; message: string }
  | { kind: 'done'; result: UpdateSummary }
  | { kind: 'failed'; message: string; download?: string };

export function useUpdateCheck(setOpenFailure: (failure: { message: string; url: string }) => void, confirmExit: (action: string) => Promise<boolean>) {
  const [updateState, setUpdateState] = useState<UpdateState>({ kind: 'idle' });
  const busy = useRef(false);
  const checked = useRef(false);
  const checkForUpdates = useCallback(async (quiet = false) => {
    if (busy.current) return;
    busy.current = true;
    checked.current = true;
    setUpdateState({ kind: 'checking' });
    try {
      const check = await native<UpdateCheck>('check_for_updates');
      setUpdateState(!quiet || check.newer ? { kind: 'done', result: updateSummary(check) } : { kind: 'idle' });
    } catch (error) {
      setUpdateState(quiet ? { kind: 'idle' } : { kind: 'failed', message: errorMessage(error) });
    } finally { busy.current = false; }
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    const timer = setTimeout(() => { if (!checked.current) void checkForUpdates(true); }, 5000);
    return () => clearTimeout(timer);
  }, [checkForUpdates]);
  async function installUpdate() {
    if (busy.current || updateState.kind !== 'done' || !updateState.result.installSupported || !updateState.result.release) return;
    busy.current = true;
    let unlisten: (() => void) | undefined;
    try {
      if (!await confirmExit('Install and restart Desegnos')) return;
      // Subscribe first so even the first download chunk is reflected in the notice.
      unlisten = await listen<UpdateProgress>(UPDATE_PROGRESS_EVENT, ({ payload }) => {
        setUpdateState({ kind: 'installing', message: payload.installing ? 'Installing update…' : `Downloading update…${payload.percent === null ? '' : ` ${payload.percent}%`}` });
      });
      setUpdateState({ kind: 'installing', message: 'Downloading update…' });
      await native('install_update', { version: updateState.result.release.version });
    } catch (error) { setUpdateState({ kind: 'failed', message: errorMessage(error), download: updateState.result.release.url }); }
    finally { unlisten?.(); busy.current = false; }
  }
  async function openRelease(url: string) {
    try { await native('open_preview_link', { url }); }
    catch { setOpenFailure({ message: url, url }); }
  }
  return { updateState, dismissUpdate: () => { if (!busy.current) setUpdateState({ kind: 'idle' }); }, checkForUpdates, installUpdate, openRelease };
}
