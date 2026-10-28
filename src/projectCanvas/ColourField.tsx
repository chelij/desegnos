import { useFieldDraft } from '../CompactField';

const validColour = (value: string) => !value.trim() || /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(value.trim()) || value.trim().toLowerCase() === 'transparent';
function keyDown(settle: () => void, cancel: () => void) {
  return (event: React.KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !(event.currentTarget instanceof HTMLTextAreaElement && event.shiftKey)) {
      event.preventDefault(); settle(); event.currentTarget.blur();
    } else if (event.key === 'Escape') {
      event.preventDefault(); cancel(); event.currentTarget.blur();
    }
  };
}

/** Colour swatch plus hex text in one row; an empty value means no colour. */
export function ColourField({ label, value, commit, fallback = '#ffffff', disabled, valid = validColour }: {
  label: string; value: string; commit: (value: string) => void; fallback?: string; disabled?: boolean; valid?: (value: string) => boolean;
}) {
  const state = useFieldDraft(value, commit, valid);
  const trimmed = value.trim();
  const hex = /^#[0-9a-f]{3}$/i.test(trimmed) ? `#${trimmed.slice(1).split('').map(part => part + part).join('')}` : trimmed;
  const picker = /^#[0-9a-f]{6}$/i.test(hex) ? hex : fallback;
  return <div className="props-colour">
    <label className={`props-swatch${trimmed ? '' : ' is-empty'}`} title={`${label} colour`} style={trimmed ? { background: trimmed } : undefined}>
      <input type="color" value={picker} disabled={disabled} aria-label={`${label} colour`} onChange={event => commit(event.target.value)} />
    </label>
    <input className="props-hex" type="text" aria-label={label} placeholder="None" spellCheck={false}
      value={state.draft} disabled={disabled} onChange={event => state.setDraft(event.target.value)}
      onBlur={() => state.settle()} onKeyDown={keyDown(() => state.settle(), state.cancel)} />
  </div>;
}

