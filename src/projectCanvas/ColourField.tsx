import { useEffect, useRef, useState } from 'react';
import { fieldKeys, useFieldDraft } from '../CompactField';
import { colourHex } from '../colourValue';

const INVALID_HOLD_MS = 1500;

/** Colour swatch plus hex text in one row; an empty value means no colour unless it is required. */
export function ColourField({ label, value, commit, fallback = '#ffffff', disabled, optional = true, valid, mixed = false }: {
  label: string; value: string; commit: (value: string) => void; fallback?: string; disabled?: boolean;
  /** An optional colour clears to no colour; a required one rejects an empty draft. */
  optional?: boolean;
  /** A multi-selection with different colours reads as "Mixed" rather than "None". */
  mixed?: boolean;
  /** A final check on the normalised hex value, for models with a narrower colour format. */
  valid?: (value: string) => boolean;
}) {
  const [invalid, setInvalid] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => { if (timer.current !== null) clearTimeout(timer.current); }, []);
  const clearInvalid = () => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    setInvalid(false);
  };
  const showInvalid = () => {
    if (timer.current !== null) clearTimeout(timer.current);
    setInvalid(true);
    timer.current = setTimeout(() => { timer.current = null; setInvalid(false); }, INVALID_HOLD_MS);
  };
  const state = useFieldDraft(value, draft => commit(colourHex(draft.trim()) ?? (draft.trim() ? draft : '')), draft => {
    const trimmed = draft.trim();
    if (!trimmed) {
      if (!optional) showInvalid();
      return optional;
    }
    const hex = colourHex(trimmed);
    if (!hex || (valid && !valid(hex))) { showInvalid(); return false; }
    clearInvalid();
    return true;
  });
  const current = colourHex(value.trim()) ?? value.trim();
  const picker = /^#[0-9a-f]{6}$/i.test(current) ? current : fallback;
  return <div className="props-colour">
    <label className={`props-swatch${current ? '' : ' is-empty'}`} title={`${label} colour`} style={current ? { background: current } : undefined}>
      <input type="color" value={picker} disabled={disabled} aria-label={`${label} colour`} onChange={event => commit(event.target.value)} />
    </label>
    <input className={`props-hex${invalid ? ' is-invalid' : ''}`} type="text" aria-label={label} placeholder={mixed ? 'Mixed' : 'None'} spellCheck={false}
      aria-invalid={invalid || undefined}
      value={state.draft} disabled={disabled} onChange={event => state.setDraft(event.target.value)}
      onBlur={state.settle} onKeyDown={fieldKeys(state.settle, state.cancel)} />
  </div>;
}
