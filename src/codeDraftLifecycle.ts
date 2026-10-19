export type PendingCodeDraft = { language: string; status: 'dirty' | 'saving' | 'invalid' | 'conflict' | 'error' };
const blocked = new Set(['invalid', 'conflict', 'error']);

// Navigation never waits forever or drops a draft that changed during a save.
// A timeout does not cancel an accepted native edit; it only keeps this view open.
export async function synchronizeCodeDrafts(adapter: { clearTimers: () => void; flush: () => Promise<void>; pending: () => PendingCodeDraft[] }, timeoutMs = 15_000): Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error('Code is still saving. Keep this page open and try again.')), timeoutMs); });
  const work = async () => {
    for (let pass = 0; pass < 3; pass++) {
      adapter.clearTimers();
      await adapter.flush();
      const pending = adapter.pending();
      if (!pending.length) return;
      const issue = pending.find(draft => blocked.has(draft.status));
      if (issue) throw new Error(`Your ${issue.language} code has unsaved changes. Fix the highlighted problem or choose Discard code changes before continuing.`);
    }
    throw new Error('Code changed while saving. Keep this page open and try again.');
  };
  try { await Promise.race([work(), timeout]); }
  finally { if (timer) clearTimeout(timer); }
}
