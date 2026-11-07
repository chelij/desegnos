import { usePoll } from '../usePoll';
import { errorMessage } from '../editorShared';
import { useCallback, useEffect, useRef, useState } from 'react';
import { native } from '../api';
import type { AiSettingsView } from '../types';

export function useAiSettings() {
  const [showAiSettings, setShowAiSettings] = useState(false);
  const [aiSettings, setAiSettings] = useState<AiSettingsView | null>(null);
  const aiCurrent = useRef<AiSettingsView | null>(null);
  const aiEpoch = useRef(0);
  const aiRead = useRef(0);
  const aiTail = useRef<Promise<void>>(Promise.resolve());
  const [aiPollingError, setAiPollingError] = useState('');

  const storeAiSettings = useCallback((value: AiSettingsView) => { aiCurrent.current = value; setAiSettings(value); setAiPollingError(''); }, []);
  const refreshAiSettings = useCallback(async (refreshModels = false) => {
    const epoch = aiEpoch.current; const read = ++aiRead.current;
    const value = await native<AiSettingsView>('get_ai_settings', { refreshModels });
    if (epoch === aiEpoch.current && read === aiRead.current) { storeAiSettings(value); return value; }
    return aiCurrent.current ?? value;
  }, [storeAiSettings]);
  const updateAiSettings = useCallback((command: string, args?: Record<string, unknown>) => {
    const result = aiTail.current.then(async () => {
      aiEpoch.current += 1; aiRead.current += 1;
      const value = await native<AiSettingsView>(command, args);
      storeAiSettings(value); return value;
    });
    aiTail.current = result.then(() => undefined, () => undefined);
    return result;
  }, [storeAiSettings]);
  const manageChatgptUsage = useCallback(() => native<void>('open_chatgpt_usage'), []);
  useEffect(() => { void refreshAiSettings().catch(error => setAiPollingError(errorMessage(error))); }, [refreshAiSettings]);
  usePoll(aiSettings?.chatgpt.signIn.status === 'awaitingBrowser', 'chatgpt-sign-in', 900,
    async () => { await refreshAiSettings(); },
    error => setAiPollingError(`ChatGPT sign-in status could not be refreshed: ${errorMessage(error)}`), 3000);

  const settleAiSettings = useCallback(() => aiTail.current, []);
  return { settleAiSettings, showAiSettings, setShowAiSettings, aiSettings, aiCurrent, aiPollingError, refreshAiSettings, updateAiSettings, manageChatgptUsage };
}
