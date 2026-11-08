import { fieldKeys, useFieldDraft } from '../CompactField';
import { MAX_HTTP_URL_BYTES } from '../generated/limits';

/** The same rule as the core's `valid_http_url`: http(s), a host, no credentials, bounded length. */
export function validHttpUrl(value: string): boolean {
  const text = value.trim();
  if (new TextEncoder().encode(text).length > MAX_HTTP_URL_BYTES) return false;
  try { const url = new URL(text); return (url.protocol === 'http:' || url.protocol === 'https:') && !!url.hostname && !url.username && !url.password; } catch { return false; }
}

export function PropertyField({ label, ariaLabel, value, commit, valid, type = 'text', min, max, maxLength, disabled, field }: {
  label: string; ariaLabel?: string; value: string | number; commit: (value: string) => void;
  valid?: (draft: string) => boolean;
  type?: 'text' | 'number'; min?: number; max?: number; maxLength?: number; disabled?: boolean; field?: string;
}) {
  const state = useFieldDraft(value, commit, valid);
  return <label className="dock-field">{label}
    <input type={type} aria-label={ariaLabel} value={state.draft} disabled={disabled} min={min} max={max} maxLength={maxLength} spellCheck={false} data-field={field}
      onChange={event => state.setDraft(event.target.value)}
      onBlur={state.settle}
      onKeyDown={fieldKeys(state.settle, state.cancel)} />
  </label>;
}

export function ChoiceRow<T extends string>({ label, value, choices, choose, disabled, names, hideLabel }: {
  label: string; value: T; choices: [T, string][]; choose: (value: T) => void; disabled?: boolean;
  /** Full names for short labels, used as the accessible name and tooltip. */
  names?: Partial<Record<T, string>>; hideLabel?: boolean;
}) {
  const name = (id: T) => names?.[id];
  return <div className="dock-choice" role="group" aria-label={label}>
    {!hideLabel && <span className="dock-choice-label">{label}</span>}
    <div className="segmented">{choices.map(([id, text]) => <button key={id} type="button" aria-pressed={value === id} aria-label={name(id)} title={name(id)} disabled={disabled} onClick={() => choose(id)}>{text}</button>)}</div>
  </div>;
}

