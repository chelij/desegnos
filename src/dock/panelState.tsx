import { readPreference as readString, writePreference as store } from '../preferences';
import { useCallback, useEffect, useMemo, useState } from 'react';

const DOCK_WIDTH_KEY = 'desegnos.dock-width';
const DOCK_COLLAPSED_KEY = 'desegnos.dock-collapsed';
const PANELS_HIDDEN_KEY = 'desegnos.panels-hidden';
const MIN_DOCK_WIDTH = 224;
const MAX_DOCK_WIDTH = 480;
const DEFAULT_DOCK_WIDTH = 272;
export const MIN_PANEL_HEIGHT = 160;

export type PanelState = { width: number; collapsed: boolean; hidden: boolean };
export type PanelActions = { setWidth: (width: number) => void; setCollapsed: (collapsed: boolean) => void; toggleHidden: () => void };

export function clampDockWidth(width: number): number {
  return Number.isFinite(width) ? Math.min(MAX_DOCK_WIDTH, Math.max(MIN_DOCK_WIDTH, Math.round(width))) : DEFAULT_DOCK_WIDTH;
}
function readWidth(): number {
  const value = Number(readString(DOCK_WIDTH_KEY));
  return Number.isFinite(value) && value > 0 ? clampDockWidth(value) : DEFAULT_DOCK_WIDTH;
}

export function usePanelState(): [PanelState, PanelActions] {
  const [width, setWidth] = useState(readWidth);
  const [collapsed, setCollapsed] = useState(() => readString(DOCK_COLLAPSED_KEY) === '1');
  const [hidden, setHidden] = useState(() => readString(PANELS_HIDDEN_KEY) === '1');
  useEffect(() => { store(DOCK_WIDTH_KEY, String(width)); }, [width]);
  useEffect(() => { store(DOCK_COLLAPSED_KEY, collapsed ? '1' : '0'); }, [collapsed]);
  useEffect(() => { store(PANELS_HIDDEN_KEY, hidden ? '1' : '0'); }, [hidden]);
  const toggleHidden = useCallback(() => setHidden(value => !value), []);
  const actions = useMemo<PanelActions>(() => ({ setWidth, setCollapsed, toggleHidden }), [toggleHidden]);
  return [{ width, collapsed, hidden }, actions];
}

