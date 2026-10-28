import { useEffect, useSyncExternalStore } from 'react';
import { native } from '../api';
import type { Element } from '../types';

type TextStyle = Element['style']['textStyle'];
type Face = { id: string; bundled: boolean; fellBack: boolean; italic: boolean };
type LoadedFont = Face & { family: string };
const requests = new Map<string, Promise<LoadedFont>>();
const loaded = new Map<string, LoadedFont>();
const faces = new Map<string, Promise<string>>();
const listeners = new Set<() => void>();
let revision = 0;
const key = (style: TextStyle) => JSON.stringify([style.fontFamily, style.weight, style.italic]);
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };

export function canvasFont(style: TextStyle): LoadedFont | undefined {
  return loaded.get(key(style));
}

export function loadCanvasFont(style: TextStyle): Promise<LoadedFont> {
  const requestKey = key(style);
  let request = requests.get(requestKey);
  if (!request) {
    request = native<Face>('resolve_canvas_font', { family: style.fontFamily, weight: style.weight, italic: style.italic }).then(async face => {
      let loading = faces.get(face.id);
      if (!loading) {
        loading = (async () => {
          const family = `dsg-${face.id}`;
          const source = face.bundled
            ? `url("${new URL('../assets/brand/archivo-variable.woff2', import.meta.url).href}")`
            : `url("${await native<string>('load_canvas_font', { id: face.id })}")`;
          const font = new FontFace(family, source, { weight: '1 1000' });
          document.fonts.add(await font.load());
          return family;
        })();
        faces.set(face.id, loading);
      }
      const font = { ...face, family: await loading };
      loaded.set(requestKey, font);
      revision++;
      listeners.forEach(listener => listener());
      return font;
    });
    requests.set(requestKey, request);
  }
  return request;
}

export function useCanvasFontRevision(): number {
  return useSyncExternalStore(subscribe, () => revision, () => 0);
}

export function useCanvasFont(style: TextStyle | undefined): LoadedFont | undefined {
  useCanvasFontRevision();
  const requestKey = style ? key(style) : '';
  useEffect(() => {
    if (style) void loadCanvasFont(style).catch(error => console.error('Could not load canvas font', error));
  }, [requestKey]);
  return style ? canvasFont(style) : undefined;
}
