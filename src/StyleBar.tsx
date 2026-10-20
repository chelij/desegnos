import { useEffect, useRef, useState } from 'react';
import type { ElementVisual } from './types';

// Quick, AI-free styling for the selected element: colour, fill, text size, spacing and alignment.
// Each change is one ordinary element edit, so Undo works as usual.
export type StyleChange = Record<string, string>;

function hex(value: string | undefined) {
  const rgb = value?.match(/[\d.]+/g)?.slice(0, 3);
  return rgb?.length === 3 ? `#${rgb.map(part => Math.max(0, Math.min(255, Math.round(Number(part)))).toString(16).padStart(2, '0')).join('')}` : '#000000';
}
const px = (value: string | undefined, fallback: number) => { const number = Number.parseFloat(value ?? ''); return Number.isFinite(number) ? number : fallback; };
const clear = (value: string | undefined) => !value || value === 'transparent' || /rgba\([^)]*,\s*0\)$/.test(value);

export function StyleBar({ visual, busy, apply }: { visual: ElementVisual | null; busy: boolean; apply: (change: StyleChange) => void }) {
  // Colour pickers fire continuously while dragging; send one edit after the user settles.
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [color, setColor] = useState(hex(visual?.color));
  const [fill, setFill] = useState(hex(visual?.backgroundColor));
  useEffect(() => { setColor(hex(visual?.color)); setFill(hex(visual?.backgroundColor)); }, [visual?.color, visual?.backgroundColor]);
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, []);
  const later = (change: StyleChange) => { if (timer.current) clearTimeout(timer.current); timer.current = setTimeout(() => apply(change), 450); };
  const size = px(visual?.fontSize, 16);
  const padding = px(visual?.padding?.split(' ')[0], 0);
  const align = visual?.textAlign === 'start' ? 'left' : visual?.textAlign === 'end' ? 'right' : visual?.textAlign;
  return <div className="style-bar" role="group" aria-label="Quick style">
    <label className="style-swatch" title="Text colour"><span style={{ boxShadow: `inset 0 -4px 0 ${color}` }} aria-hidden="true">A</span><input type="color" aria-label="Text colour" value={color} disabled={busy} onChange={event => { setColor(event.target.value); later({ color: event.target.value }); }} /></label>
    <label className={`style-swatch fill${clear(visual?.backgroundColor) ? ' empty' : ''}`} title="Fill colour"><span style={{ background: clear(visual?.backgroundColor) ? undefined : fill }} aria-hidden="true" /><input type="color" aria-label="Fill colour" value={fill} disabled={busy} onChange={event => { setFill(event.target.value); later({ 'background-color': event.target.value }); }} /></label>
    <span className="style-divider" aria-hidden="true" />
    <button type="button" aria-label="Smaller text" title="Smaller text" disabled={busy || size <= 8} onClick={() => apply({ 'font-size': `${Math.max(8, Math.round(size - 2))}px` })}>A−</button>
    <span className="style-value" title="Text size">{Math.round(size)}</span>
    <button type="button" aria-label="Larger text" title="Larger text" disabled={busy || size >= 160} onClick={() => apply({ 'font-size': `${Math.min(160, Math.round(size + 2))}px` })}>A+</button>
    <span className="style-divider" aria-hidden="true" />
    <button type="button" aria-label="Less space inside" title="Less space inside" disabled={busy || padding <= 0} onClick={() => apply({ padding: `${Math.max(0, padding - 4)}px` })}>▣−</button>
    <button type="button" aria-label="More space inside" title="More space inside" disabled={busy} onClick={() => apply({ padding: `${padding + 4}px` })}>▣+</button>
    <span className="style-divider" aria-hidden="true" />
    {(['left', 'center', 'right'] as const).map(value => <button key={value} type="button" aria-label={`Align ${value}`} title={`Align ${value}`} aria-pressed={align === value} disabled={busy} onClick={() => apply({ 'text-align': value })}>
      <svg viewBox="0 0 16 16" aria-hidden="true">{[3, 7, 11].map((y, index) => { const width = index === 1 ? 8 : 12; const x = value === 'left' ? 2 : value === 'right' ? 14 - width : 8 - width / 2; return <line key={y} x1={x} y1={y} x2={x + width} y2={y} />; })}</svg>
    </button>)}
  </div>;
}
