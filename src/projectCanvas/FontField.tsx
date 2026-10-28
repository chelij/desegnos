import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { Icon } from '../icons';
import { useFontFamilies } from './fonts';

/** A searchable font family field: bundled fonts first, then installed faces, and free text. */
export function FontField({ label, value, commit, disabled }: {
  label: string; value: string; commit: (value: string) => void; disabled?: boolean;
}) {
  const families = useFontFamilies();
  const id = useId();
  const [draft, setDraft] = useState(value);
  const [open, setOpen] = useState(false);
  // Opening lists every font; only typing filters, so the current family never hides the list.
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(-1);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  // Choosing a row commits once; the blur that follows must not commit it again.
  const choosing = useRef(false);
  useEffect(() => { setDraft(value); }, [value]);
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return needle ? families.filter(name => name.toLowerCase().includes(needle)) : families;
  }, [families, query]);
  useEffect(() => {
    if (!open) return;
    setActive(current => Math.min(current, filtered.length - 1));
    list.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [open, filtered.length, active]);

  function settle() {
    if (choosing.current) { choosing.current = false; return; }
    const next = draft.trim();
    if (!next) { setDraft(value); return; }
    if (next !== value) commit(next);
  }
  function choose(name: string) {
    choosing.current = true;
    setDraft(name);
    setOpen(false);
    setActive(-1);
    if (name !== value) commit(name);
    input.current?.blur();
  }

  return <div className="dock-field">
    <span className="dock-field-label">{label}</span>
    <div className="font-field">
      <input ref={input} className="props-input" type="text" role="combobox" autoComplete="off" spellCheck={false}
        aria-label={label} aria-expanded={open} aria-controls={`${id}-list`} aria-autocomplete="list"
        aria-activedescendant={open && active >= 0 ? `${id}-${active}` : undefined}
        disabled={disabled} value={draft}
        onFocus={event => { setQuery(''); setOpen(true); event.currentTarget.select(); }}
        onChange={event => { setDraft(event.target.value); setQuery(event.target.value); setOpen(true); setActive(-1); }}
        onBlur={() => { setOpen(false); settle(); }}
        onKeyDown={event => {
          if (event.key === 'ArrowDown') {
            event.preventDefault(); setOpen(true);
            setActive(current => filtered.length ? (current + 1) % filtered.length : -1);
          } else if (event.key === 'ArrowUp') {
            event.preventDefault(); setOpen(true);
            setActive(current => filtered.length ? (current <= 0 ? filtered.length - 1 : current - 1) : -1);
          } else if (event.key === 'Enter') {
            event.preventDefault();
            if (open && active >= 0 && filtered[active]) choose(filtered[active]);
            else { setOpen(false); settle(); event.currentTarget.blur(); }
          } else if (event.key === 'Escape') {
            event.preventDefault();
            setOpen(false); setDraft(value); event.currentTarget.blur();
          } else if (event.key === 'Tab') {
            setOpen(false);
          }
        }} />
      <button type="button" className="font-field-toggle" tabIndex={-1} aria-label={open ? 'Hide fonts' : 'Show fonts'}
        title={open ? 'Hide fonts' : 'Show fonts'} disabled={disabled}
        onPointerDown={event => event.preventDefault()}
        onClick={() => { setQuery(''); setOpen(current => !current); input.current?.focus({ preventScroll: true }); }}>
        <Icon name="chevron-down" size={12} />
      </button>
      {open && <ul ref={list} id={`${id}-list`} className="font-field-list" role="listbox" aria-label={`${label} options`}>
        {filtered.map((name, index) => <li key={name} id={`${id}-${index}`} role="option" aria-selected={index === active}
          className={`font-field-option${index === active ? ' is-active' : ''}`}
          onPointerDown={event => { event.preventDefault(); choose(name); }}>{name}</li>)}
        {filtered.length === 0 && <li className="font-field-empty">No matching fonts</li>}
      </ul>}
    </div>
  </div>;
}
