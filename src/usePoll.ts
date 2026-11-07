import { useEffect, useRef } from 'react';

type Poll = { read: (active: () => boolean) => Promise<void>; failed: (error: unknown) => void };

export function usePoll(enabled: boolean, key: string, intervalMs: number, read: Poll['read'], failed: Poll['failed'], retryMs = intervalMs, initialMs = intervalMs) {
  const latest = useRef({ read, failed });
  latest.current = { read, failed };
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      let delay = intervalMs;
      const operation = latest.current;
      try { await operation.read(() => active); }
      catch (error) { if (active) operation.failed(error); delay = retryMs; }
      if (active) timer = setTimeout(() => { void poll(); }, delay);
    }
    timer = setTimeout(() => { void poll(); }, initialMs);
    return () => { active = false; clearTimeout(timer); };
  }, [enabled, key, intervalMs, retryMs, initialMs]);
}
