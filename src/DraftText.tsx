import { useEffect, useRef, useState, type ChangeEvent } from 'react';

// commit also gets the newest draft, for saves that run after later keystrokes were typed.
export function useDraftText(value: string, commit: (value: string, latest: () => string) => Promise<string>) {
  const [draft, setDraft] = useState(value);
  const newest = useRef(value);
  const latest = useRef(value);
  const sequence = useRef(0);
  const pending = useRef(0);
  useEffect(() => { latest.current = value; if (pending.current === 0) setDraft(value); }, [value]);
  function change(value: string) {
    setDraft(value); newest.current = value;
    const ticket = ++sequence.current;
    pending.current += 1;
    void commit(value, () => newest.current).then(accepted => {
      if (ticket === sequence.current) setDraft(accepted);
    }, () => {
      if (ticket === sequence.current) setDraft(latest.current);
    }).finally(() => { pending.current -= 1; });
  }
  return [draft, change] as const;
}

export function DraftText({ label, value, commit, multiline = false, autoFocus = false, maxLength = 16384 }: {
  label: string; value: string; commit: (value: string) => Promise<string>; multiline?: boolean; autoFocus?: boolean; maxLength?: number;
}) {
  const [draft, change] = useDraftText(value, commit);
  const props = { value: draft, onChange: (event: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => change(event.target.value), maxLength, autoFocus };
  return <label>{label}{multiline ? <textarea {...props} rows={5} /> : <input {...props} />}</label>;
}
