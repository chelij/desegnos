import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCallback, useEffect, useRef, useState } from 'react';
import { native } from '../api';
import { errorValue, type ErrorValue } from '../errorMessage.js';
import type { GenerationJobView, IdentityAnalysis, WorkspaceView } from '../types';

/** Closing or restarting asks first when work is unsaved or a generation or analysis is still running. */
export function useWindowClose({ settle, current, generation, aiAuthentication, setError }: {
  settle: () => Promise<void>;
  current: { current: WorkspaceView | null };
  generation: { current: GenerationJobView | null };
  aiAuthentication: { current: { authentication: string } | null };
  setError: (message: ErrorValue) => void;
}) {
  const [closeRequest, setCloseRequest] = useState<{ action: string; message: string; resolve: (answer: boolean) => void } | null>(null);
  const closing = useRef(false);
  const confirmExit = useCallback(async (action = 'Close Desegnos') => {
    if (closing.current) return false;
    closing.current = true;
    try {
      await settle();
      let identity: IdentityAnalysis | null = null;
      let identityUnavailable = false;
      try { identity = await native<IdentityAnalysis | null>('get_identity_analysis'); }
      catch (error) {
        // A damaged record must not trap the user in the app. Ask before closing
        // because we could not establish whether an analysis is still running.
        identityUnavailable = true;
        setError(errorValue(error));
      }
      const generating = generation.current?.status === 'generating' || identity?.status === 'analyzing';
      const plan = identity?.status === 'analyzing' ? (identity.authentication ?? aiAuthentication.current?.authentication) === 'chatgpt' : generation.current?.authentication === 'chatgpt';
      const leaving = action === 'Close Desegnos' ? 'Closing' : 'Restarting';
      if ((current.current?.anyDirty || current.current?.dirty || generating || identityUnavailable) && !await new Promise<boolean>(resolve => setCloseRequest({ action, resolve, message: `${identityUnavailable ? `Identity analysis status could not be checked. ${leaving} may interrupt submitted work, which may still use your plan or credits. ` : ''}${generating ? `Generation is still running. ${leaving} may interrupt the request, and ${plan ? 'submitted work may still use your ChatGPT plan or credits' : 'OpenAI may still charge for submitted work'}. Its outcome will be shown as uncertain when you return. ` : ''}${current.current?.anyDirty || current.current?.dirty ? 'There are changes that have not been saved to their files. ' : ''}${action}?` }))) return false;
      return true;
    } catch (error) { setError(errorValue(error)); return false; }
    finally { closing.current = false; }
  }, [settle, current, generation, aiAuthentication, setError]);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow().onCloseRequested(async event => {
      event.preventDefault();
      if (await confirmExit()) {
        try { await getCurrentWindow().destroy(); }
        catch (error) { setError(errorValue(error)); }
      }
    }).then(stop => { if (disposed) stop(); else unlisten = stop; }, error => setError(errorValue(error)));
    return () => { disposed = true; unlisten?.(); };
  }, [confirmExit, setError]);
  return { closeRequest, setCloseRequest, confirmExit };
}
