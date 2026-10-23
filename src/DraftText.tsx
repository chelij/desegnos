import { useEffect, useRef, useState, type ChangeEvent } from 'react';

// commit also gets the newest draft, for saves that run after later keystrokes were typed.
// A delay batches keystrokes into one save; the pending text is flushed on blur or unmount.
export function useDraftText(value: string, commit: (value: string, latest: () => string) => Promise<string>, delayMs = 0) {
  const [draft, setDraft] = useState(value);
  const newest = useRef(value);
  const latest = useRef(value);
  const sequence = useRef(0);
  // Saves that started or are scheduled; the workspace value must not overwrite a newer draft.
  const pending = useRef(0);
  const timer = useRef<number | null>(null);
  const waiting = useRef<string | null>(null);
  const commitRef = useRef(commit); commitRef.current = commit;
  useEffect(() => { latest.current = value; if (pending.current === 0) setDraft(value); }, [value]);
  function save(next: string, counted: boolean) {
    const ticket = ++sequence.current;
    if (!counted) pending.current += 1;
    void commitRef.current(next, () => newest.current).then(accepted => {
      if (ticket === sequence.current && waiting.current === null) setDraft(accepted);
    }, () => {
      if (ticket === sequence.current && waiting.current === null) setDraft(latest.current);
    }).finally(() => { pending.current -= 1; });
  }
  function flush() {
    if (timer.current !== null) { window.clearTimeout(timer.current); timer.current = null; }
    const next = waiting.current; waiting.current = null;
    if (next !== null) save(next, true);
  }
  function change(next: string) {
    setDraft(next); newest.current = next;
    if (delayMs <= 0) { save(next, false); return; }
    if (waiting.current === null) pending.current += 1;
    waiting.current = next;
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      const value = waiting.current; waiting.current = null;
      if (value !== null) save(value, true);
    }, delayMs);
  }
  useEffect(() => () => flush(), []);
  return [draft, change, flush] as const;
}

export function DraftText({ label, value, commit, multiline = false, autoFocus = false, maxLength = 16384 }: {
  label: string; value: string; commit: (value: string) => Promise<string>; multiline?: boolean; autoFocus?: boolean; maxLength?: number;
}) {
  const [draft, change] = useDraftText(value, commit);
  const props = { value: draft, onChange: (event: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => change(event.target.value), maxLength, autoFocus };
  return <label>{label}{multiline ? <textarea {...props} rows={5} /> : <input {...props} />}</label>;
}
