import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { resolveNumberField, roundNumber } from './numberExpression';

/** Display numbers rounded to at most two decimals; the full value survives untouched until edited. */
function shownNumber(value: number): string {
  return String(Math.round(value * 100) / 100);
}

/** A field commits on Enter or blur and Escape puts the old value back. `resolve` turns the raw
 * draft (an expression, typed units) into the value to commit; returning null reverts. */
export function useFieldDraft(value: string | number, commit: (value: string) => void, valid?: (draft: string) => boolean, resolve?: (draft: string) => string | null) {
  const [draft, setDraft] = useState(String(value));
  const cancelled = useRef(false);
  useEffect(() => { setDraft(String(value)); }, [value]);
  function settle() {
    if (cancelled.current) { cancelled.current = false; setDraft(String(value)); return; }
    // An unchanged draft never commits, so an inherited value outside a field's range stays as it is.
    if (draft === String(value)) return;
    const resolved = resolve ? resolve(draft) : draft;
    if (resolved === null) { setDraft(String(value)); return; }
    if (resolved === String(value)) { if (resolved !== draft) setDraft(resolved); return; }
    // Show the result the field will keep, including any clamping, before the edit is committed.
    if (resolved !== draft) setDraft(resolved);
    if (!valid || valid(resolved)) commit(resolved); else setDraft(String(value));
  }
  return {
    draft, setDraft, settle,
    cancel: () => { cancelled.current = true; setDraft(String(value)); },
  };
}

function fieldKeys(settle: () => void, cancel: () => void) {
  return (event: React.KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !(event.currentTarget instanceof HTMLTextAreaElement && event.shiftKey)) {
      event.preventDefault(); settle(); event.currentTarget.blur();
    } else if (event.key === 'Escape') {
      event.preventDefault(); cancel(); event.currentTarget.blur();
    }
  };
}

/** A compact text input with no visible label; the placeholder names the field. */
export function CompactTextField({ label, placeholder, value, commit, valid, disabled, field, prefix }: {
  label: string; placeholder?: string; value: string; commit: (value: string) => void;
  valid?: (draft: string) => boolean; disabled?: boolean; field?: string; prefix?: ReactNode;
}) {
  const state = useFieldDraft(value, commit, valid);
  const input = <input className={prefix ? undefined : 'props-input'} type="text" aria-label={label} placeholder={placeholder ?? label}
    data-field={field} spellCheck={false} value={state.draft} disabled={disabled}
    onChange={event => state.setDraft(event.target.value)} onBlur={state.settle}
    onKeyDown={fieldKeys(state.settle, state.cancel)} />;
  if (!prefix) return input;
  return <label className="props-num" title={label}>
    <span className="props-num-prefix" aria-hidden="true">{prefix}</span>{input}
  </label>;
}

type Drag = { start: number; origin: number; value: number; moved: boolean };

/**
 * Compact number field: a tiny prefix lives inside the field, spinners are gone.
 * Dragging the prefix scrubs the value like a paint program: one unit per 2px,
 * ten per 2px while Shift is held, clamped to the field's range and committed
 * once on release so the whole drag is a single undo step.
 *
 * Typing also accepts arithmetic (`120*2`, `(10+5)*2`, `100/3`) and relative entries
 * (`+16`, `*2`, `/2`; subtraction is `+-8` because a leading `-` is an absolute number).
 * Units typed by habit (`24px`, `50%`) are ignored, results are clamped and rounded to two
 * decimals, and invalid input reverts without committing.
 */
export function CompactNumberField({ prefix, label, value, commit, valid, min, max, disabled, unit, field, optional, normalise }: {
  prefix: ReactNode; label: string; value: string | number; commit: (value: string) => void;
  valid?: (draft: string) => boolean; min?: number; max?: number; disabled?: boolean; unit?: string; field?: string;
  /** Optional fields may be cleared with an empty draft; every other field reverts. */
  optional?: boolean;
  /** Wrap a clamped result, for values such as an angle that has a natural range. */
  normalise?: (value: number) => number;
}) {
  const formatted = typeof value === 'number' ? shownNumber(value) : value;
  const state = useFieldDraft(formatted, commit, valid, draft => {
    if (!draft.trim()) return optional ? '' : null;
    const resolved = resolveNumberField(draft, formatted, min, max);
    return resolved === null || !normalise ? resolved : String(roundNumber(normalise(Number(resolved))));
  });
  const input = useRef<HTMLInputElement>(null);
  const drag = useRef<Drag | null>(null);
  const [scrubbed, setScrubbed] = useState<string | null>(null);

  const clamp = (next: number) => Math.min(max ?? Number.POSITIVE_INFINITY, Math.max(min ?? Number.NEGATIVE_INFINITY, next));

  function beginScrub(event: ReactPointerEvent<HTMLSpanElement>) {
    if (disabled || event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    const current = Number(state.draft);
    const origin = Number.isFinite(current) && state.draft.trim() !== '' ? current : min ?? 0;
    drag.current = { start: event.clientX, origin, value: clamp(origin), moved: false };
  }
  function scrub(event: ReactPointerEvent<HTMLSpanElement>) {
    const active = drag.current;
    if (!active) return;
    const distance = event.clientX - active.start;
    if (!active.moved && Math.abs(distance) < 1) return;
    active.moved = true;
    active.value = clamp(active.origin + distance / 2 * (event.shiftKey ? 10 : 1));
    setScrubbed(shownNumber(active.value));
  }
  function endScrub(event: ReactPointerEvent<HTMLSpanElement>) {
    const active = drag.current;
    drag.current = null;
    if (!active) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    setScrubbed(null);
    if (!active.moved) { input.current?.focus(); input.current?.select(); return; }
    const next = shownNumber(normalise ? normalise(active.value) : active.value);
    if (next === state.draft) return;
    state.setDraft(next);
    commit(next);
  }
  function cancelScrub() {
    drag.current = null;
    setScrubbed(null);
  }

  return <label className="props-num" title={label}>
    <span className="props-num-prefix is-scrubby" aria-hidden="true" data-scrub={label}
      onPointerDown={beginScrub} onPointerMove={scrub} onPointerUp={endScrub} onPointerCancel={cancelScrub}>{prefix}</span>
    <input ref={input} type="text" inputMode="decimal" aria-label={label} value={scrubbed ?? state.draft}
      spellCheck={false} autoComplete="off" disabled={disabled} data-field={field}
      style={{ fontVariantNumeric: 'tabular-nums' }}
      onChange={event => state.setDraft(event.target.value)}
      onBlur={state.settle} onKeyDown={fieldKeys(state.settle, state.cancel)} />
    {unit && <span className="props-num-unit" aria-hidden="true">{unit}</span>}
  </label>;
}
