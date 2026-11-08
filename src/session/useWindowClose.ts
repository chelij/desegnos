import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect, useRef, useState } from 'react';
import { native } from '../api';
import { errorValue, type ErrorValue } from '../errorMessage.js';
import type { GenerationJobView, IdentityAnalysis, WorkspaceView } from '../types';

/** Closing the window asks first when work is unsaved or a generation or analysis is still running. */
export function useWindowClose({ settle, current, generation, aiAuthentication, setError }: {
  settle: () => Promise<void>;
  current: { current: WorkspaceView | null };
  generation: { current: GenerationJobView | null };
  aiAuthentication: { current: { authentication: string } | null };
  setError: (message: ErrorValue) => void;
}) {
  const [closeRequest, setCloseRequest] = useState<{ message: string; resolve: (answer: boolean) => void } | null>(null);
  const closing = useRef(false);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow().onCloseRequested(async event => {
      event.preventDefault();
      if (closing.current) return;
      closing.current = true;
      try {
        await settle();
        const identity = await native<IdentityAnalysis | null>('get_identity_analysis');
        const generating = generation.current?.status === 'generating' || identity?.status === 'analyzing';
        const plan = identity?.status === 'analyzing' ? (identity.authentication ?? aiAuthentication.current?.authentication) === 'chatgpt' : generation.current?.authentication === 'chatgpt';
        if ((current.current?.anyDirty || current.current?.dirty || generating) && !await new Promise<boolean>(resolve => setCloseRequest({ resolve, message: `${generating ? `Generation is still running. Closing may interrupt the request, and ${plan ? 'submitted work may still use your ChatGPT plan or credits' : 'OpenAI may still charge for submitted work'}. Its outcome will be shown as uncertain when you return. ` : ''}${current.current?.dirty ? 'This project has changes that have not been saved to its file. ' : ''}Close Desegnos?` }))) return;
        await getCurrentWindow().destroy();
      } catch (error) { setError(errorValue(error)); }
      finally { closing.current = false; }
    }).then(stop => { if (disposed) stop(); else unlisten = stop; }, error => setError(errorValue(error)));
    return () => { disposed = true; unlisten?.(); };
  }, [settle]);
  return { closeRequest, setCloseRequest };
}
