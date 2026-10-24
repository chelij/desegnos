import { useEffect, useRef, useState } from 'react';
import type { ElementVisual } from './types';
import { Icon } from './icons';

// Quick, AI-free styling for the selected element: colour, fill, text size, spacing and alignment.
// Each change is one ordinary element edit, so Undo works as usual.
export type StyleChange = Record<string, string>;

function hex(value: string | undefined) {
  const rgb = value?.match(/[\d.]+/g)?.slice(0, 3);
  return rgb?.length === 3 ? `#${rgb.map(part => Math.max(0, Math.min(255, Math.round(Number(part)))).toString(16).padStart(2, '0')).join('')}` : '#000000';
}
const px = (value: string | undefined, fallback: number) => { const number = Number.parseFloat(value ?? ''); return Number.isFinite(number) ? number : fallback; };
const clear = (value: string | undefined) => !value || value === 'transparent' || /rgba\([^)]*,\s*0\)$/.test(value);

export function StyleBar({ visual, mixed = {}, busy, apply }: { visual: ElementVisual | null; mixed?: Partial<Record<keyof ElementVisual, boolean>>; busy: boolean; apply: (change: StyleChange) => void }) {
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
    <label className={`style-swatch${mixed.color ? ' mixed' : ''}`} title={mixed.color ? 'Text colour · mixed' : 'Text colour'}><span className="style-swatch-icon" style={{ color }} aria-hidden="true"><Icon name="text-color" /></span><input type="color" aria-label="Text colour" value={color} disabled={busy} onChange={event => { setColor(event.target.value); later({ color: event.target.value }); }} /></label>
    <label className={`style-swatch fill${clear(visual?.backgroundColor) && !mixed.backgroundColor ? ' empty' : ''}${mixed.backgroundColor ? ' mixed' : ''}`} title={mixed.backgroundColor ? 'Fill colour · mixed' : 'Fill colour'}><span style={{ background: clear(visual?.backgroundColor) ? undefined : fill }} aria-hidden="true" /><input type="color" aria-label="Fill colour" value={fill} disabled={busy} onChange={event => { setFill(event.target.value); later({ 'background-color': event.target.value }); }} /></label>
    <span className="style-divider" aria-hidden="true" />
    <button type="button" aria-label="Smaller text" title="Smaller text" disabled={busy || size <= 8} onClick={() => apply({ 'font-size': `${Math.max(8, Math.round(size - 2))}px` })}><Icon name="font-smaller" /></button>
    <span className={`style-value${mixed.fontSize ? ' mixed' : ''}`} title={mixed.fontSize ? 'Text size · mixed' : 'Text size'}>{mixed.fontSize ? '–' : Math.round(size)}</span>
    <button type="button" aria-label="Larger text" title="Larger text" disabled={busy || size >= 160} onClick={() => apply({ 'font-size': `${Math.min(160, Math.round(size + 2))}px` })}><Icon name="font-larger" /></button>
    <span className="style-divider" aria-hidden="true" />
    <button type="button" className={mixed.padding ? 'mixed' : undefined} aria-label="Less space inside" title={mixed.padding ? 'Space inside · mixed' : 'Less space inside'} disabled={busy || padding <= 0} onClick={() => apply({ padding: `${Math.max(0, padding - 4)}px` })}><Icon name="padding-less" /></button>
    <button type="button" className={mixed.padding ? 'mixed' : undefined} aria-label="More space inside" title={mixed.padding ? 'Space inside · mixed' : 'More space inside'} disabled={busy} onClick={() => apply({ padding: `${padding + 4}px` })}><Icon name="padding-more" /></button>
    <span className="style-divider" aria-hidden="true" />
    {(['left', 'center', 'right'] as const).map(value => <button key={value} type="button" className={mixed.textAlign ? 'mixed' : undefined} aria-label={`Align ${value}`} title={mixed.textAlign ? `Align ${value} · mixed` : `Align ${value}`} aria-pressed={!mixed.textAlign && align === value} disabled={busy} onClick={() => apply({ 'text-align': value })}>
      <Icon name={value === 'left' ? 'align-left' : value === 'right' ? 'align-right' : 'align-center'} />
    </button>)}
  </div>;
}
