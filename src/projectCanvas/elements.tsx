import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react';
import type { Element, ElementKind, Project, Rect } from '../types';
import type { Boundary, CanvasCamera, ElementTarget } from '../projectCanvasGeometry';
import type { ElementCorner } from './elementGeometry';
import { elementPath, intrinsicElementSize, isTextKind, measureText, pictureUri } from './elementGeometry';
import { elementOrigin, resolveForSize } from './sizes';
import { canvasFont, loadCanvasFont, useCanvasFont, useCanvasFontRevision } from './fonts';
import type { ScreenRect } from './surface';

const FALLBACK_TEXT = '#1a1a1a';

function textStyle(element: Element): CSSProperties {
  const style = element.style.textStyle;
  const font = canvasFont(style);
  return {
    fontFamily: font?.family,
    visibility: font ? undefined : 'hidden',
    fontSize: style.size,
    fontWeight: style.weight,
    fontStyle: style.italic && !font?.italic ? 'italic' : 'normal',
    lineHeight: style.lineHeight,
    letterSpacing: style.letterSpacing,
    color: style.color || FALLBACK_TEXT,
    textAlign: style.align,
  };
}

/**
 * The rendering contract, canvas half: a layer clips its own content to its box unless it is a
 * box or group, which let children overflow. The renderer in desegnos-core does the same.
 */
const clipsContent = (kind: ElementKind) => kind !== 'box' && kind !== 'group';

/**
 * Blur is a CSS blur radius, so the shadow's Gaussian standard deviation is half of it, which is
 * what the exported SVG filter uses. Spread stays at zero.
 */
function shadowCss(shadow: Element['style']['shadow']): string | undefined {
  return shadow.color ? `${shadow.x}px ${shadow.y}px ${shadow.blur}px 0 ${shadow.color}` : undefined;
}

function wrapperStyle(element: Element, flow: boolean, rect: Rect, parentLayout?: Element['layout']): CSSProperties {
  const style = element.style;
  const line = element.kind === 'line';
  const plain = element.kind === 'group';
  const autoText = (element.kind === 'text' || element.kind === 'button' || element.kind === 'link') && rect.width === 0;
  const sizing = style.sizing;
  const row = parentLayout?.kind === 'stack' && parentLayout.direction === 'row';
  const mainFill = parentLayout?.kind === 'stack' && (row ? sizing?.width : sizing?.height) === 'fill';
  const axis = (mode: string | undefined, fixed: number, offset: number, horizontal: boolean): CSSProperties['width'] => {
    if (mode === 'fit') {
      if (element.layout.kind === 'free' && element.children.length) {
        const content = intrinsicElementSize(element, child => typeof document === 'undefined' ? child.rect : measureText(child.text, child.style.textStyle));
        return horizontal ? content.width : content.height;
      }
      return horizontal ? 'max-content' : 'auto';
    }
    if (mode === 'fill') return !flow ? `calc(100% - ${offset}px)` : mainFill && horizontal === row ? 0 : '100%';
    return fixed > 0 ? fixed : horizontal && autoText ? 'max-content' : undefined;
  };
  const size = {
    width: line && (sizing?.width ?? 'fixed') === 'fixed' ? Math.max(1, rect.width) : axis(sizing?.width, rect.width, rect.x, true),
    height: line && (sizing?.height ?? 'fixed') === 'fixed' ? Math.max(1, rect.height) : axis(sizing?.height, rect.height, rect.y, false),
    minWidth: sizing?.minWidth ?? (sizing?.width === 'fill' ? 0 : undefined), maxWidth: sizing?.maxWidth ?? undefined,
    minHeight: sizing?.minHeight ?? (sizing?.height === 'fill' ? 0 : undefined), maxHeight: sizing?.maxHeight ?? undefined,
    background: line || plain ? undefined : style.fill ?? undefined,
    borderColor: line || plain ? undefined : style.stroke.color ?? undefined,
    borderStyle: line || plain ? undefined : style.stroke.color && style.stroke.width > 0 ? 'solid' : undefined,
    borderWidth: line || plain ? undefined : style.stroke.color ? style.stroke.width : undefined,
    borderRadius: element.kind === 'ellipse' ? '50%' : style.radius,
    boxShadow: line || plain ? undefined : shadowCss(style.shadow),
    opacity: style.opacity,
    // Turning about the layer's centre is part of the contract; the renderer rotates about the
    // centre of the same rect, so children turn with it.
    transform: element.rotation ? `rotate(${element.rotation}deg)` : undefined,
    transformOrigin: 'center center',
  };
  if (flow) {
    return { ...size, position: 'relative', boxSizing: 'border-box', flex: mainFill ? '1 1 0' : '0 0 auto', overflow: clipsContent(element.kind) ? 'hidden' : undefined };
  }
  return {
    ...size,
    position: 'absolute',
    left: rect.x,
    top: rect.y,
    boxSizing: 'border-box',
    overflow: clipsContent(element.kind) ? 'hidden' : undefined,
  };
}

function childrenStyle(layout: Element['layout'], sizing?: Element['style']['sizing']): CSSProperties | null {
  if (layout.kind === 'free') return null;
  if (layout.kind === 'stack') {
    return {
      display: 'flex',
      flexDirection: layout.direction,
      gap: layout.gap,
      padding: layout.padding,
      alignItems: layout.align === 'start' ? 'flex-start' : layout.align === 'end' ? 'flex-end' : layout.align,
      justifyContent: layout.justify === 'start' ? 'flex-start' : layout.justify === 'end' ? 'flex-end' : layout.justify === 'spaceBetween' ? 'space-between' : 'center',
      flexWrap: layout.wrap ? 'wrap' : 'nowrap',
      width: sizing?.width === 'fit' ? 'max-content' : '100%',
      height: sizing?.height === 'fit' ? 'auto' : '100%',
      boxSizing: 'border-box',
    };
  }
  return {
    display: 'grid',
    gridTemplateColumns: `repeat(${layout.columns}, minmax(0, 1fr))`,
    gap: layout.gap,
    padding: layout.padding,
    width: '100%',
    height: '100%',
    boxSizing: 'border-box',
  };
}

const placeholderText = (element: Element): string => {
  if (element.url) return element.url;
  if (element.kind === 'video') return 'Video';
  return element.kind === 'embed' ? 'Embed' : 'Audio';
};

function ControlText({ element }: { element: Element }) {
  const style = textStyle(element);
  if (element.kind === 'checkbox' || element.kind === 'radio') {
    return <div style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '0 2px', ...style }}>
      <span aria-hidden="true" style={{
        width: 13, height: 13, flex: '0 0 auto', boxSizing: 'border-box',
        border: `1px solid ${element.style.stroke.color ?? '#8a8a8a'}`, borderRadius: element.kind === 'radio' ? '50%' : 2,
        background: element.kind === 'checkbox' ? element.style.fill ?? 'transparent' : 'transparent',
      }} />
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{element.text || (element.kind === 'checkbox' ? 'Checkbox' : 'Radio')}</span>
    </div>;
  }
  if (element.kind === 'select') {
    return <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, padding: '0 8px', height: '100%', ...style }}>
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{element.text || element.options?.[0] || ''}</span>
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true" style={{ flex: '0 0 auto', opacity: 0.7 }}><path d="M2 3.5 5 6.5 8 3.5" fill="none" stroke="currentColor" strokeWidth="1.2" /></svg>
    </div>;
  }
  if (element.kind === 'input' || element.kind === 'textarea') {
    return <div style={{
      ...style, padding: element.kind === 'textarea' ? '8px' : '0 8px', height: element.kind === 'textarea' ? '100%' : '100%',
      display: 'flex', alignItems: element.kind === 'textarea' ? 'flex-start' : 'center', color: element.text ? style.color : '#8a8a8a',
      whiteSpace: 'pre-wrap', overflow: 'hidden', wordBreak: 'break-word',
    }}>{element.text || (element.kind === 'input' ? element.inputType ?? 'text' : 'Text')}</div>;
  }
  if (element.kind === 'button') {
    return <div style={{
      ...style, display: 'flex', alignItems: 'center', justifyContent: 'center', width: '100%', height: '100%',
      padding: '0 12px', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', boxSizing: 'border-box',
    }}>{element.text}</div>;
  }
  return <div style={{ ...style, display: 'flex', alignItems: 'center', height: '100%', padding: '0 1px', textDecoration: element.kind === 'link' ? 'underline' : undefined, whiteSpace: 'nowrap', overflow: 'hidden' }}>{element.text}</div>;
}

function MediaCard({ element }: { element: Element }) {
  const kind = element.kind;
  return <div className="canvas-media-card">
    <span className="canvas-media-kind">{kind === 'embed' ? 'Embed' : kind === 'video' ? 'Video' : 'Audio'}</span>
    {kind === 'audio'
      ? <span className="canvas-media-wave" aria-hidden="true">{Array.from({ length: 24 }, (_, index) => <i key={index} style={{ height: 3 + ((index * 7) % 13) }} />)}</span>
      : <span className="canvas-media-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M8 5.5 18 12 8 18.5Z" fill="currentColor" /></svg></span>}
    <span className="canvas-media-url">{placeholderText(element)}</span>
  </div>;
}

function CodeCard({ element }: { element: Element }) {
  const source = element.source ?? '';
  const shown = source.length > 4000 ? `${source.slice(0, 4000)}\n…` : source;
  return <div className="canvas-code-card">
    <span className="canvas-code-language">{element.language || 'code'}</span>
    <pre>{shown}</pre>
  </div>;
}

function ElementBody({ project, pageId, element, flow, editingId, movingRects }: { project: Project; pageId: string; element: Element; flow: boolean; editingId: string | null; movingRects?: Record<string, Rect> }) {
  const children = element.children.length > 0 && element.kind !== 'group'
    ? <ElementChildren project={project} pageId={pageId} element={element} editingId={editingId} movingRects={movingRects} />
    : element.children.length > 0
      ? <>{element.children.map(child => <ElementView key={child.id} project={project} pageId={pageId} element={child} flow={false} editingId={editingId} movingRects={movingRects} />)}</>
      : null;
  switch (element.kind) {
    case 'box':
      return children;
    case 'group':
      return children;
    case 'text':
      return editingId === element.id ? null : <div style={{ ...textStyle(element), whiteSpace: 'pre-wrap', overflowWrap: 'break-word' }}>{element.text}</div>;
    case 'ellipse':
      return null;
    case 'line': {
      const height = Math.max(1, element.rect.height);
      return <svg width="100%" height="100%" viewBox={`0 0 ${Math.max(1, element.rect.width)} ${height}`} preserveAspectRatio="none" aria-hidden="true" style={{ display: 'block', overflow: 'visible' }}>
        <line x1="0" y1={height / 2} x2={Math.max(1, element.rect.width)} y2={height / 2} stroke={element.style.stroke.color ?? element.style.textStyle.color} strokeWidth={element.style.stroke.width || 1} />
      </svg>;
    }
    case 'image': {
      const uri = pictureUri(project, element.image);
      return uri ? <img src={uri} alt="" draggable={false} style={{ display: 'block', width: '100%', height: '100%', objectFit: 'cover', borderRadius: 'inherit' }} />
        : <div className="canvas-image-missing">Picture</div>;
    }
    case 'button':
    case 'link':
    case 'input':
    case 'textarea':
    case 'select':
    case 'checkbox':
    case 'radio':
      return <ControlText element={element} />;
    case 'video':
    case 'audio':
    case 'embed':
      return <MediaCard element={element} />;
    case 'code':
      return <CodeCard element={element} />;
  }
}

function ElementChildren({ project, pageId, element, editingId, movingRects }: { project: Project; pageId: string; element: Element; editingId: string | null; movingRects?: Record<string, Rect> }) {
  const style = childrenStyle(element.layout, element.style.sizing);
  if (!style) return <>{element.children.map(child => <ElementView key={child.id} project={project} pageId={pageId} element={child} flow={false} editingId={editingId} movingRects={movingRects} />)}</>;
  return <div className="canvas-element-layout" style={style}>{element.children.map(child => <ElementView key={child.id} project={project} pageId={pageId} element={child} flow parentLayout={element.layout} editingId={editingId} movingRects={movingRects} />)}</div>;
}

export function ElementView({ project, pageId, element, flow, editingId, root, movingRects, parentLayout }: {
  project: Project; pageId: string; element: Element; flow: boolean; editingId: string | null; root?: boolean; movingRects?: Record<string, Rect>; parentLayout?: Element['layout'];
}) {
  useCanvasFont(isTextKind(element.kind) ? element.style.textStyle : undefined);
  if (!element.visible) return null;
  const data = { 'data-element': element.id, 'data-element-kind': element.kind, 'data-element-group': element.kind === 'group' ? 'true' : undefined, 'data-element-root': root ? 'true' : undefined };
  return <div {...data} style={wrapperStyle(element, flow, movingRects?.[element.id] ?? element.rect, parentLayout)} className={`canvas-element canvas-element-${element.kind}${element.locked ? ' locked' : ''}`}>
    <ElementBody project={project} pageId={pageId} element={element} flow={flow} editingId={editingId} movingRects={movingRects} />
  </div>;
}

/** A boundary with elements draws the tree instead of a snapshot. */
export function ElementTree({ project, pageId, sizeId, elements, boundary, editingId, movingRects }: {
  project: Project; pageId: string; sizeId: string; elements: Element[]; boundary: Boundary; editingId?: string | null; movingRects?: Record<string, Rect>;
}) {
  void sizeId;
  return <div className="boundary-elements" style={{ left: 0, top: 0, width: boundary.rect.width, height: boundary.rect.height }}>
    {elements.map(element => <ElementView key={element.id} project={project} pageId={pageId} element={element} flow={false} editingId={editingId ?? null} movingRects={movingRects} root />)}
  </div>;
}

export const ELEMENT_CORNERS: ElementCorner[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];

export type MeasuredSelection = { elementId: string; sizeId: string; rect: ScreenRect; inherited: boolean };

// One layer draws in every linked size, so a selection measures each of its instances.
function measure(surface: HTMLElement, selection: ElementTarget[]): MeasuredSelection[] {
  const host = surface.getBoundingClientRect();
  const rows: MeasuredSelection[] = [];
  for (const target of selection) {
    for (const node of surface.querySelectorAll<HTMLElement>(`[data-element="${CSS.escape(target.elementId)}"]`)) {
      const sizeId = node.closest<HTMLElement>('[data-size]')?.dataset.size ?? '';
      const box = node.getBoundingClientRect();
      // Solid in the edited size (or every size for a double-click selection); dashed where
      // the layer is only inherited.
      rows.push({ elementId: target.elementId, sizeId, inherited: target.sizeId !== null && target.sizeId !== sizeId,
        rect: { left: box.left - host.left, top: box.top - host.top, width: box.width, height: box.height } });
    }
  }
  return rows;
}

/** Selection lives in screen pixels above the transformed layer. */
export function ElementSelectionOverlay({ surface, project, selection, camera, version, onResizeStart }: {
  surface: { current: HTMLDivElement | null }; project: Project; selection: ElementTarget[]; camera: CanvasCamera; version: string;
  onResizeStart: (event: ReactPointerEvent<HTMLButtonElement>, elementId: string, corner: ElementCorner, sizeId: string | null) => void;
}) {
  const [rects, setRects] = useState<MeasuredSelection[]>([]);
  const fontRevision = useCanvasFontRevision();
  const key = `${project.revision}:${fontRevision}:${camera.x}:${camera.y}:${camera.zoom}:${selection.map(target => `${target.elementId}:${target.sizeId ?? 'all'}`).join(',')}:${version}`;
  useLayoutEffect(() => {
    const node = surface.current;
    if (!node || !selection.length) { setRects(current => current.length ? [] : current); return; }
    setRects(measure(node, selection));
  }, [key]);
  useEffect(() => {
    const node = surface.current;
    if (!node) return;
    const observer = new ResizeObserver(() => setRects(measure(node, selection)));
    observer.observe(node);
    return () => observer.disconnect();
  }, [selection.map(target => `${target.elementId}:${target.sizeId ?? 'all'}`).join(',')]);
  if (!selection.length) return null;
  const editable = selection.length === 1 ? rects.find(row => !row.inherited) ?? rects[0] ?? null : null;
  return <>
    {rects.map(row => <div key={`${row.elementId}:${row.sizeId}`} className={`element-selection-box${row.inherited ? ' inherited' : ''}`}
      style={{ left: row.rect.left, top: row.rect.top, width: row.rect.width, height: row.rect.height }} />)}
    {editable && selection.length === 1 && ELEMENT_CORNERS.map(corner => {
      const rect = editable.rect;
      const x = corner === 'nw' || corner === 'w' || corner === 'sw' ? rect.left : corner === 'ne' || corner === 'e' || corner === 'se' ? rect.left + rect.width : rect.left + rect.width / 2;
      const y = corner === 'nw' || corner === 'n' || corner === 'ne' ? rect.top : corner === 'sw' || corner === 's' || corner === 'se' ? rect.top + rect.height : rect.top + rect.height / 2;
      return <button key={corner} type="button" className={`element-handle ${corner}`} data-element-handle={corner} aria-label={`Resize ${corner}`} title="Drag to resize"
        style={{ left: x - 4, top: y - 4 }} onPointerDown={event => onResizeStart(event, selection[0].elementId, corner, selection[0].sizeId)} />;
    })}
  </>;
}

/** Text is edited in fixed screen pixels, never inside the transformed layer. */
export function ElementTextEditor({ element, screen, zoom, autoWidth, autoFocus, onCommit, onCancel }: {
  element: Element; screen: ScreenRect; zoom: number; autoWidth: boolean; autoFocus: boolean;
  onCommit: (text: string, size?: { width: number; height: number }) => void;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState(element.text);
  const cancelled = useRef(false);
  const done = useRef(false);
  const area = useRef<HTMLTextAreaElement>(null);
  const style = element.style.textStyle;
  const font = useCanvasFont(style);
  useEffect(() => { if (autoFocus && font) { area.current?.focus({ preventScroll: true }); area.current?.select(); } }, [autoFocus, element.id, font?.family]);
  async function settle() {
    if (cancelled.current || done.current) return;
    done.current = true;
    if (autoWidth) await loadCanvasFont(style);
    if (cancelled.current) return;
    onCommit(draft, autoWidth ? measureText(draft, style) : undefined);
  }
  const lines = Math.max(1, draft.split('\n').length);
  const width = autoWidth ? Math.max(120, measureText(draft || 'Text', style).width * zoom + 24) : Math.max(40, screen.width);
  const height = autoWidth ? Math.max(28, lines * style.size * style.lineHeight * zoom + 12) : Math.max(28, screen.height);
  return <textarea ref={area} className="element-text-editor" aria-label="Edit text" spellCheck value={draft}
    style={{ left: screen.left, top: screen.top, width, height, fontSize: style.size * zoom, lineHeight: style.lineHeight, visibility: font ? undefined : 'hidden', fontFamily: font?.family, fontWeight: style.weight, fontStyle: style.italic && !font?.italic ? 'italic' : 'normal', letterSpacing: style.letterSpacing * zoom, color: style.color, textAlign: style.align }}
    onChange={event => setDraft(event.target.value)}
    onPointerDown={event => event.stopPropagation()}
    onBlur={settle}
    onKeyDown={event => {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); cancelled.current = true; done.current = true; onCancel(); return; }
      if (event.key === 'Enter' && (!event.shiftKey || element.kind !== 'text')) {
        event.preventDefault(); event.stopPropagation();
        settle();
        return;
      }
      // The textarea holds the keys; the editor's own shortcuts must not fire behind it.
      event.stopPropagation();
    }} />;
}

/** The world rect a text editor sits over, in screen pixels. */
export function textEditorScreen(project: Project, pageId: string, element: Element, boundary: Boundary, camera: CanvasCamera, sizeId?: string): ScreenRect {
  const page = project.pages.find(item => item.id === pageId);
  const resolved = page && sizeId ? resolveForSize(project, page, sizeId) : null;
  const path = elementPath(project, pageId, element.id);
  const origin = (resolved ? elementOrigin(resolved, element.id) : null) ?? path?.[path.length - 1]?.origin ?? { x: element.rect.x, y: element.rect.y };
  return {
    left: (boundary.rect.x + origin.x) * camera.zoom + camera.x,
    top: (boundary.rect.y + origin.y) * camera.zoom + camera.y,
    width: element.rect.width * camera.zoom,
    height: element.rect.height * camera.zoom,
  };
}

export const elementScreenRect = (rect: Rect, camera: CanvasCamera): ScreenRect => ({
  left: rect.x * camera.zoom + camera.x, top: rect.y * camera.zoom + camera.y,
  width: rect.width * camera.zoom, height: rect.height * camera.zoom,
});

export const textKind = isTextKind;

export type ElementDraft = { pageId: string; parentId: string | null; rect: Rect; kind: ElementKind; flow: boolean } | null;
