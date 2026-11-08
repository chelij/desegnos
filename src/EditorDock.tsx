import { Fragment, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { Icon, type IconName } from './icons';

import { clampDockWidth, MIN_PANEL_HEIGHT, type PanelState, type PanelActions } from './dock/panelState';
import { readJsonPreference, stringList, writeJsonPreference } from './preferences';
const PANEL_WEIGHTS_KEY = 'desegnos.dock-panel-weights';
const PANEL_COLLAPSED_KEY = 'desegnos.dock-panel-collapsed';
function DockSection({ name, open, toggle, children, grow, register, fitContent }: {
  name: string; open: boolean; toggle: () => void; children: ReactNode;
  grow: number; register: (node: HTMLElement | null) => void; fitContent?: boolean;
}) {
  return <section ref={register} className={`dock-panel${open ? '' : ' is-collapsed'}${fitContent ? ' is-fit-content' : ''}`} aria-label={name} style={open && !fitContent ? { flexGrow: grow } : undefined}>
    <header className="dock-panel-header">
      <h2>{name}</h2>
      <button type="button" className="icon-button sm" aria-expanded={open} aria-label={`${open ? 'Collapse' : 'Expand'} ${name.toLowerCase()}`} title={open ? `Collapse ${name}` : `Expand ${name}`} onClick={toggle}><Icon name="chevron-right" className={open ? 'expanded' : undefined} /></button>
    </header>
    {open && <div className="dock-panel-body">{children}</div>}
  </section>;
}

const readWeights = (): Record<string, number> => readJsonPreference(PANEL_WEIGHTS_KEY, raw => !raw || typeof raw !== 'object' ? {}
  : Object.fromEntries(Object.entries(raw).filter(([, value]) => typeof value === 'number' && Number.isFinite(value) && value > 0)));
const readCollapsedPanels = (): ReadonlySet<string> => new Set(readJsonPreference(PANEL_COLLAPSED_KEY, stringList));

const DOCK_ICONS: Record<string, IconName> = { history: 'history', export: 'external', properties: 'properties', layers: 'layers' };
/** Properties carries the most fields, so it starts taller than History and Layers. */
const DEFAULT_PANEL_WEIGHTS: Record<string, number> = { history: 0.8, export: 0.9, properties: 1.6, layers: 1.1 };

export function EditorDock({ state, actions, properties, layers, history, exportPanel }: {
  state: PanelState; actions: PanelActions; properties: ReactNode; layers: ReactNode; history?: ReactNode; exportPanel?: ReactNode;
}) {
  const panels = [
    ...(history ? [{ key: 'history', name: 'History', content: history }] : []),
    ...(exportPanel ? [{ key: 'export', name: 'Export', content: exportPanel, fitContent: true }] : []),
    { key: 'properties', name: 'Properties', content: properties },
    { key: 'layers', name: 'Layers', content: layers },
  ];
  const [weights, setWeights] = useState<Record<string, number>>(readWeights);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(readCollapsedPanels);
  const nodes = useRef(new Map<string, HTMLElement>());
  useEffect(() => { writeJsonPreference(PANEL_WEIGHTS_KEY, weights); }, [weights]);
  useEffect(() => { writeJsonPreference(PANEL_COLLAPSED_KEY, [...collapsed]); }, [collapsed]);

  function startResize(event: ReactPointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const edge = event.currentTarget.parentElement?.getBoundingClientRect().right;
    if (edge === undefined) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    const move = (next: PointerEvent) => actions.setWidth(clampDockWidth(edge - next.clientX));
    const stop = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', stop); };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
  }

  // A splitter trades height between the nearest resizable panels on each side and stops at the
  // readable minimum. Fit-content panels keep their height, so they sit out of the trade.
  function startPanelResize(event: ReactPointerEvent<HTMLDivElement>, index: number) {
    event.preventDefault();
    const resizable = (panel: typeof panels[number]) => !panel.fitContent && !collapsed.has(panel.key);
    const aboveKey = panels.slice(0, index).reverse().find(resizable)?.key;
    const belowKey = panels.slice(index).find(resizable)?.key;
    if (!aboveKey || !belowKey) return;
    const aboveNode = nodes.current.get(aboveKey);
    const belowNode = nodes.current.get(belowKey);
    if (!aboveNode || !belowNode) return;
    const startY = event.clientY;
    const startAbove = aboveNode.getBoundingClientRect().height;
    const startBelow = belowNode.getBoundingClientRect().height;
    const pair = startAbove + startBelow;
    const move = (next: PointerEvent) => {
      const dy = Math.max(MIN_PANEL_HEIGHT - startAbove, Math.min(pair - MIN_PANEL_HEIGHT - startBelow, next.clientY - startY));
      const heights: Record<string, number> = {};
      for (const panel of panels) {
        const node = nodes.current.get(panel.key);
        if (node && resizable(panel)) heights[panel.key] = node.getBoundingClientRect().height;
      }
      heights[aboveKey] = startAbove + dy;
      heights[belowKey] = startBelow - dy;
      setWeights(heights);
    };
    const stop = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', stop); };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
  }

  function togglePanel(key: string) {
    const next = new Set(collapsed);
    if (next.has(key)) next.delete(key); else next.add(key);
    if (panels.every(panel => next.has(panel.key))) { setCollapsed(new Set()); actions.setCollapsed(true); }
    else setCollapsed(next);
  }

  function expandDock() {
    setCollapsed(new Set());
    actions.setCollapsed(false);
  }

  if (state.hidden) return null;
  if (state.collapsed) return <aside className="editor-dock collapsed" aria-label="Panels">
    {panels.map(panel => <button key={panel.key} type="button" className="dock-icon" aria-label={`Show ${panel.name}`} title={panel.name} onClick={expandDock}><Icon name={DOCK_ICONS[panel.key] ?? 'properties'} /></button>)}
  </aside>;
  return <aside className="editor-dock" style={{ width: state.width }} aria-label="Panels">
    <div className="dock-resizer" role="separator" aria-orientation="vertical" aria-label="Resize panels" onPointerDown={startResize} />
    <div className="dock-panels">
      {panels.map((panel, index) => {
        const open = !collapsed.has(panel.key);
        const previous = panels[index - 1];
        return <Fragment key={panel.key}>
          {previous && open && !collapsed.has(previous.key) && <div className="dock-splitter" role="separator" aria-orientation="horizontal"
            aria-label={`Resize ${previous.name} and ${panel.name}`} onPointerDown={event => startPanelResize(event, index)} />}
          <DockSection name={panel.name} open={open} toggle={() => togglePanel(panel.key)} grow={weights[panel.key] ?? DEFAULT_PANEL_WEIGHTS[panel.key] ?? 1} fitContent={panel.fitContent}
            register={node => { if (node) nodes.current.set(panel.key, node); else nodes.current.delete(panel.key); }}>{panel.content}</DockSection>
        </Fragment>;
      })}
    </div>
  </aside>;
}

