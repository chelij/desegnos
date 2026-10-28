import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import './palette.css';

export type PaletteCommand = {
  id: string;
  label: string;
  /** The shortcut shown on the right, already formatted for the platform. */
  keys?: string;
  group: string;
  /** Set when the current state leaves the command with nothing to change. */
  disabled?: boolean;
  run: () => void;
};

const RECENT_KEY = 'desegnos.recent-commands';
const RECENT_LIMIT = 6;
const MAX_ROWS = 80;

function readRecent(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]');
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string').slice(0, RECENT_LIMIT) : [];
  } catch { return []; }
}

function rememberRecent(id: string) {
  try { localStorage.setItem(RECENT_KEY, JSON.stringify([id, ...readRecent().filter(item => item !== id)].slice(0, RECENT_LIMIT))); }
  catch { /* The recent order is a convenience, never a requirement. */ }
}

/** Characters that appear in order win; nearby and word-start characters win more. */
function fuzzyScore(needle: string, haystack: string): number | null {
  let index = 0, score = 0, streak = 0;
  for (const character of needle) {
    let found = index;
    while (found < haystack.length && haystack[found] !== character) found += 1;
    if (found >= haystack.length) return null;
    const start = found === 0 || haystack[found - 1] === ' ' || haystack[found - 1] === '-';
    streak = found === index ? streak + 1 : 0;
    score += 6 + (start ? 8 : 0) + (streak > 1 ? 3 : 0) - Math.min(4, found - index);
    index = found + 1;
  }
  return score - haystack.length * 0.15;
}

/** Quick-action ranking: score first, then what was used last, then the list's own order. */
function rank(commands: PaletteCommand[], query: string, recents: string[]): PaletteCommand[] {
  const recentRank = new Map(recents.map((id, index) => [id, index]));
  const needle = query.trim().toLocaleLowerCase();
  const scored: { command: PaletteCommand; score: number; recent: number; order: number }[] = [];
  commands.forEach((command, order) => {
    const score = needle ? fuzzyScore(needle, command.label.toLocaleLowerCase()) : 0;
    if (score === null) return;
    scored.push({ command, score, recent: recentRank.get(command.id) ?? Number.POSITIVE_INFINITY, order });
  });
  scored.sort((first, second) => needle
    ? second.score - first.score || first.recent - second.recent || first.order - second.order
    : first.recent - second.recent || first.order - second.order);
  const seen = new Set<string>();
  const unique: PaletteCommand[] = [];
  for (const entry of scored) {
    if (seen.has(entry.command.id)) continue;
    seen.add(entry.command.id);
    unique.push(entry.command);
    if (unique.length === MAX_ROWS) break;
  }
  return unique;
}

/** A quick-actions overlay under the top bar: it never traps focus or blocks the canvas behind it. */
export function CommandPalette({ commands, close }: { commands: PaletteCommand[]; close: () => void }) {
  const [query, setQuery] = useState('');
  const [recents] = useState(readRecent);
  const [activeId, setActiveId] = useState<string | null>(null);
  const panel = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const closeNow = useRef(close); closeNow.current = close;
  const results = useMemo(() => rank(commands, query, recents), [commands, query, recents]);
  const runnable = results.filter(command => !command.disabled);
  const active = runnable.find(command => command.id === activeId) ?? runnable[0] ?? null;
  const activeIndex = active ? results.indexOf(active) : -1;

  useEffect(() => { input.current?.focus({ preventScroll: true }); }, []);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !panel.current?.contains(event.target)) closeNow.current();
    };
    document.addEventListener('pointerdown', outside, true);
    return () => document.removeEventListener('pointerdown', outside, true);
  }, []);

  function run(command: PaletteCommand | null) {
    if (!command || command.disabled) return;
    rememberRecent(command.id);
    closeNow.current();
    command.run();
  }
  function move(step: number) {
    if (!runnable.length) return;
    const index = runnable.findIndex(command => command.id === active?.id);
    const next = runnable[(index + step + runnable.length) % runnable.length];
    setActiveId(next.id);
    panel.current?.querySelector<HTMLElement>(`#command-palette-row-${results.indexOf(next)}`)?.scrollIntoView({ block: 'nearest' });
  }
  function keydown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeNow.current(); return; }
    if (event.key === 'ArrowDown') { event.preventDefault(); move(1); return; }
    if (event.key === 'ArrowUp') { event.preventDefault(); move(-1); return; }
    if (event.key === 'Home' && runnable.length) { event.preventDefault(); setActiveId(runnable[0].id); return; }
    if (event.key === 'End' && runnable.length) { event.preventDefault(); setActiveId(runnable[runnable.length - 1].id); return; }
    if (event.key === 'Enter') { event.preventDefault(); run(active); }
  }

  return <div className="palette-layer">
    <div className="command-palette" ref={panel} role="search" aria-label="Command palette" onKeyDown={keydown}>
      <input ref={input} className="palette-input" type="text" role="combobox" autoComplete="off" spellCheck={false}
        aria-label="Search commands" aria-expanded aria-controls="command-palette-list" aria-autocomplete="list"
        aria-activedescendant={activeIndex >= 0 ? `command-palette-row-${activeIndex}` : undefined}
        placeholder="Search commands" value={query} onChange={event => { setQuery(event.target.value); setActiveId(null); }} />
      <ul className="palette-list" id="command-palette-list" role="listbox" aria-label="Commands">
        {results.map((command, index) => <li key={command.id} id={`command-palette-row-${index}`} data-command={command.id} role="option"
          aria-selected={index === activeIndex} aria-disabled={command.disabled || undefined}
          className={`palette-row${index === activeIndex ? ' active' : ''}${command.disabled ? ' unavailable' : ''}`}
          onPointerDown={event => event.preventDefault()} onPointerEnter={() => { if (!command.disabled) setActiveId(command.id); }}
          onClick={() => run(command)}>
          <span className="palette-label">{command.label}</span>
          {command.keys && <span className="palette-key">{command.keys}</span>}
        </li>)}
        {!results.length && <li className="palette-empty" role="presentation">No commands match “{query.trim()}”</li>}
      </ul>
    </div>
  </div>;
}
