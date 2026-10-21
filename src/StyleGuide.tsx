import { useMemo, useState } from 'react';
import type { Command, Project } from './types';

// A visual view of the brand: the colour and font rules the AI follows, values found in the pages,
// and real samples rendered with the site's own CSS. Choosing a value saves it as a project brief rule.
const COLOUR_ROLES = ['Primary colour', 'Accent colour', 'Text colour', 'Background colour'] as const;
const FONT_ROLES = ['Heading font', 'Body font'] as const;
const FONT_CHOICES = [
  'system-ui, sans-serif', 'Georgia, serif', '"Helvetica Neue", Arial, sans-serif', '"Trebuchet MS", sans-serif',
  '"Palatino Linotype", Palatino, serif', '"Courier New", monospace', 'Verdana, sans-serif',
];

function normal(colour: string) {
  const value = colour.trim().toLowerCase();
  const short = value.match(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/);
  if (short) return `#${short[1]}${short[1]}${short[2]}${short[2]}${short[3]}${short[3]}`;
  if (/^#[0-9a-f]{6}$/.test(value)) return value;
  const rgb = value.match(/^rgba?\(\s*(\d+)[,\s]+(\d+)[,\s]+(\d+)/);
  return rgb ? `#${rgb.slice(1, 4).map(part => Math.min(255, Number(part)).toString(16).padStart(2, '0')).join('')}` : null;
}

// Most-used colours and fonts across every page's CSS and inline styles.
export function siteStyles(project: Project) {
  const colours = new Map<string, number>(); const fonts = new Map<string, number>();
  for (const page of project.pages) {
    if (!page.content) continue;
    const text = `${page.content.css}\n${page.content.html}`;
    for (const match of text.matchAll(/#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b|rgba?\([^)]*\)/g)) {
      const value = normal(match[0]); if (value) colours.set(value, (colours.get(value) ?? 0) + 1);
    }
    for (const match of text.matchAll(/font-family\s*:\s*([^;}"]+(?:"[^"]*"[^;}"]*)*)/gi)) {
      const value = match[1].replace(/!important/i, '').trim(); if (value && !/^(inherit|initial|unset)$/i.test(value)) fonts.set(value, (fonts.get(value) ?? 0) + 1);
    }
  }
  const top = (map: Map<string, number>, count: number) => [...map].sort((a, b) => b[1] - a[1]).slice(0, count).map(([value]) => value);
  return { colours: top(colours, 10), fonts: top(fonts, 5) };
}

function samples(project: Project) {
  const page = project.pages.find(item => item.id === project.homePageId && item.content) ?? project.pages.find(item => item.content);
  const css = page?.content?.css ?? '';
  const styles = Array.from(new DOMParser().parseFromString(page?.content?.html ?? '', 'text/html').querySelectorAll('style')).map(node => node.textContent ?? '').join('\n');
  // Scripts never run here: the frame is sandboxed without allow-scripts.
  return `<!doctype html><html><head><meta charset="utf-8"><style>${css}\n${styles}\nbody{margin:0;padding:20px}.sg-row{display:flex;gap:12px;flex-wrap:wrap;align-items:center;margin-top:14px}</style></head><body>
<h1>Main heading</h1><h2>Section heading</h2><h3>Small heading</h3>
<p>Body text with <a href="#">a link</a>.</p>
<div class="sg-row"><button type="button">Button</button><a href="#">Text link</a></div></body></html>`;
}

export function StyleGuide({ project, apply, openBrief }: { project: Project; apply: (command: Command) => Promise<void>; openBrief: () => void }) {
  const found = useMemo(() => siteStyles(project), [project]);
  const preview = useMemo(() => samples(project), [project]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const rule = (topic: string) => project.decisions.find(decision => decision.scope.kind === 'project' && decision.topic.toLowerCase() === topic.toLowerCase());
  async function save(topic: string, value: string) {
    setBusy(true); setError('');
    try { await apply({ type: 'set_decision', topic, value, scope: { kind: 'project' }, protected: rule(topic)?.protected ?? false }); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  }
  async function clear(topic: string) {
    const existing = rule(topic); if (!existing) return;
    setBusy(true); setError('');
    try { await apply({ type: 'remove_decision', decision_id: existing.id }); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  }
  return <div className="dialog-body style-guide">
    {error && <p className="message error" role="alert">{error}</p>}
    <section aria-label="Brand colours"><h3>Colours</h3><div className="sg-roles">
      {COLOUR_ROLES.map(topic => { const value = normal(rule(topic)?.value ?? '') ?? null; return <div key={topic} className="sg-role">
        <label className="sg-swatch" style={{ background: value ?? undefined }} title={value ? `Change ${topic.toLowerCase()}` : `Choose ${topic.toLowerCase()}`}>
          {!value && <span aria-hidden="true">+</span>}<input type="color" aria-label={topic} value={value ?? '#888888'} disabled={busy} onChange={event => { void save(topic, event.target.value); }} />
        </label>
        <div><strong>{topic}</strong><span>{value ?? 'Not set'}</span>{value && <button type="button" className="link-button" disabled={busy} onClick={() => { void clear(topic); }}>Clear</button>}</div>
      </div>; })}
    </div>
      {found.colours.length > 0 && <><h4>Used on your pages</h4><div className="sg-found">{found.colours.map(colour => <details key={colour} className="sg-found-colour">
        <summary title={colour}><span style={{ background: colour }} />{colour}</summary>
        <div className="sg-found-menu">{COLOUR_ROLES.map(topic => <button key={topic} type="button" disabled={busy} onClick={event => { (event.currentTarget.closest('details') as HTMLDetailsElement).open = false; void save(topic, colour); }}>Use as {topic.toLowerCase()}</button>)}</div>
      </details>)}</div></>}
    </section>
    <section aria-label="Brand fonts"><h3>Fonts</h3><div className="sg-roles">
      {FONT_ROLES.map(topic => { const value = rule(topic)?.value ?? ''; const choices = [...new Set([...(value ? [value] : []), ...found.fonts, ...FONT_CHOICES])]; return <label key={topic} className="sg-font">
        <strong>{topic}</strong>
        <select value={value} disabled={busy} onChange={event => { if (event.target.value) void save(topic, event.target.value); else void clear(topic); }}>
          <option value="">Not set: let the AI choose</option>{choices.map(choice => <option key={choice} value={choice}>{choice.split(',')[0].replace(/"/g, '')}{found.fonts.includes(choice) ? ' · used on your pages' : ''}</option>)}
        </select>
        <span className="sg-font-sample" style={{ fontFamily: value || undefined }}>{topic === 'Heading font' ? 'Heading text' : 'Body text'}</span>
      </label>; })}
    </div></section>
    <section aria-label="How your site looks now"><h3>How your site looks now</h3>
      {project.pages.some(page => page.content) ? <iframe className="sg-samples" title="Text and button samples using your site’s styles" sandbox="" srcDoc={preview} /> : <p className="field-help">No generated pages.</p>}
    </section>
    <div className="form-actions"><button type="button" onClick={openBrief}>Open project brief</button></div>
  </div>;
}
