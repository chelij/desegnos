import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react';
import type { Element, ElementKind, Project, Rect } from '../types';
import type { Boundary, CanvasCamera, ElementTarget } from '../projectCanvasGeometry';
import type { ElementCorner } from './elementGeometry';
import { elementPadding, elementPath, intrinsicElementSize, isTextKind, measureText, pictureUri } from './elementGeometry';
import { elementOrigin, resolveForSize, sizeAtWidth } from './sizes';
import { canvasFont, useCanvasFont, useCanvasFontRevision } from './fonts';
import { shortcutFor, type ShortcutAction } from '../shortcuts';
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
 * box, group or line, which let content overflow. The renderer in desegnos-core does the same.
 */
const clipsContent = (kind: ElementKind) => kind !== 'box' && kind !== 'group' && kind !== 'line';

/**
 * Blur is a CSS blur radius, so the shadow's Gaussian standard deviation is half of it, which is
 * what the exported SVG filter uses. Spread stays at zero.
 */
function shadowCss(shadow: Element['style']['shadow']): string | undefined {
  return shadow.color ? `${shadow.x}px ${shadow.y}px ${shadow.blur}px 0 ${shadow.color}` : undefined;
}

export function wrapperStyle(element: Element, flow: boolean, rect: Rect, parentLayout?: Element['layout']): CSSProperties {
  const style = element.style;
  const line = element.kind === 'line';
  const plain = element.kind === 'group';
  const autoText = (element.kind === 'text' || element.kind === 'button' || element.kind === 'link') && rect.width === 0;
  const sizing = style.sizing;
  const fitText = sizing?.width === 'fit' && isTextKind(element.kind);
  const availableWidth = flow ? '100%' : `max(0px, calc(100% - ${rect.x}px))`;
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
    minWidth: fitText ? sizing?.minWidth == null ? 0 : `min(${sizing.minWidth}px, ${availableWidth})` : sizing?.minWidth ?? (sizing?.width === 'fill' ? 0 : undefined),
    maxWidth: fitText ? sizing?.maxWidth == null ? availableWidth : `min(${sizing.maxWidth}px, ${availableWidth})` : sizing?.maxWidth ?? undefined,
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
    transform: element.rotation || element.style.flipX || element.style.flipY
      ? `rotate(${element.rotation}deg) scale(${element.style.flipX ? -1 : 1}, ${element.style.flipY ? -1 : 1})` : undefined,
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

function childrenStyle(element: Element): CSSProperties | null {
  const layout = element.layout;
  if (layout.kind === 'free') return null;
  const padding = elementPadding(element);
  const inset = `${padding.top}px ${padding.right}px ${padding.bottom}px ${padding.left}px`;
  const sizing = element.style.sizing;
  if (layout.kind === 'stack') {
    return {
      display: 'flex',
      flexDirection: layout.direction,
      gap: layout.gap,
      padding: inset,
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
    padding: inset,
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

const EMBED_DEPTH = 3;

/** An Embed frames one of this project's pages live, scaled and clipped to its box. */
function EmbedBody({ project, element, stack }: { project: Project; element: Element; stack: string[] }) {
  const page = element.url ? project.pages.find(item => item.path === element.url) : undefined;
  // At most two nested frames, and never a page already on the stack; the card marks the limit.
  if (!page || stack.length >= EMBED_DEPTH || stack.includes(page.id)) return <MediaCard element={element} />;
  const size = sizeAtWidth(page, element.rect.width);
  const elements = resolveForSize(project, page, size.id);
  const scale = size.width > 0 ? element.rect.width / size.width : 1;
  const next = [...stack, page.id];
  return <div className="canvas-embed-page" style={{ position: 'absolute', inset: 0, overflow: 'hidden', pointerEvents: 'none', userSelect: 'none', background: page.background }}>
    <div style={{ position: 'absolute', left: 0, top: 0, width: size.width, height: size.height, transform: `scale(${scale})`, transformOrigin: 'top left' }}>
      {elements.map(child => <ElementView key={child.id} project={project} pageId={page.id} element={child} flow={false} editingId={null} embedStack={next} embedded />)}
    </div>
  </div>;
}

function ElementBody({ project, pageId, element, flow, editingId, movingRects, embedStack, embedded }: { project: Project; pageId: string; element: Element; flow: boolean; editingId: string | null; movingRects?: Record<string, Rect>; embedStack: string[]; embedded?: boolean }) {
  const children = element.children.length > 0 && element.kind !== 'group'
    ? <ElementChildren project={project} pageId={pageId} element={element} editingId={editingId} movingRects={movingRects} embedStack={embedStack} embedded={embedded} />
    : element.children.length > 0
      ? <>{element.children.map(child => <ElementView key={child.id} project={project} pageId={pageId} element={child} flow={false} editingId={editingId} movingRects={movingRects} embedStack={embedStack} embedded={embedded} />)}</>
      : null;
  switch (element.kind) {
    case 'box':
      return children;
    case 'group':
      return children;
    case 'text':
      return <div style={{ ...textStyle(element), visibility: editingId === element.id ? 'hidden' : textStyle(element).visibility, whiteSpace: 'pre-wrap', overflowWrap: 'break-word' }}>{element.text}</div>;
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
      return <MediaCard element={element} />;
    case 'embed':
      return <EmbedBody project={project} element={element} stack={embedStack} />;
    case 'code':
      return <CodeCard element={element} />;
  }
}

function ElementChildren({ project, pageId, element, editingId, movingRects, embedStack, embedded }: { project: Project; pageId: string; element: Element; editingId: string | null; movingRects?: Record<string, Rect>; embedStack: string[]; embedded?: boolean }) {
  const style = childrenStyle(element);
  if (!style) return <>{element.children.map(child => <ElementView key={child.id} project={project} pageId={pageId} element={child} flow={false} editingId={editingId} movingRects={movingRects} embedStack={embedStack} embedded={embedded} />)}</>;
  return <div className="canvas-element-layout" style={style}>{element.children.map(child => <ElementView key={child.id} project={project} pageId={pageId} element={child} flow parentLayout={element.layout} editingId={editingId} movingRects={movingRects} embedStack={embedStack} embedded={embedded} />)}</div>;
}

export function ElementView({ project, pageId, element, flow, editingId, root, movingRects, parentLayout, embedStack, embedded }: {
  project: Project; pageId: string; element: Element; flow: boolean; editingId: string | null; root?: boolean; movingRects?: Record<string, Rect>; parentLayout?: Element['layout']; embedStack?: string[]; embedded?: boolean;
}) {
  useCanvasFont(isTextKind(element.kind) ? element.style.textStyle : undefined);
  if (!element.visible) return null;
  const stack = embedStack ?? [pageId];
  // Frames inside a frame draw the same way, but they never answer clicks or measurements.
  const data = embedded ? {} : { 'data-element': element.id, 'data-element-kind': element.kind, 'data-element-group': element.kind === 'group' ? 'true' : undefined, 'data-element-root': root ? 'true' : undefined };
  return <div {...data} style={wrapperStyle(element, flow, movingRects?.[element.id] ?? element.rect, parentLayout)} className={`canvas-element canvas-element-${element.kind}${element.locked ? ' locked' : ''}`}>
    <ElementBody project={project} pageId={pageId} element={element} flow={flow} editingId={editingId} movingRects={movingRects} embedStack={stack} embedded={embedded} />
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
export function measureElementSelection(surface: HTMLElement, selection: ElementTarget[]): MeasuredSelection[] {
  const host = surface.getBoundingClientRect();
  const rows: MeasuredSelection[] = [];
  const selected = new Set(selection.map(target => target.elementId));
  const nodes = new Map<string, HTMLElement[]>();
  for (const node of surface.querySelectorAll<HTMLElement>('[data-element]')) {
    const id = node.dataset.element!;
    if (!selected.has(id)) continue;
    const instances = nodes.get(id) ?? [];
    instances.push(node);
    nodes.set(id, instances);
  }
  for (const target of selection) {
    for (const node of nodes.get(target.elementId) ?? []) {
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

/** A camera change transforms measured bounds without another DOM/layout read. */
export function projectMeasuredSelection(rows: MeasuredSelection[], from: CanvasCamera, to: CanvasCamera): MeasuredSelection[] {
  const scale = to.zoom / from.zoom;
  return rows.map(row => ({ ...row, rect: {
    left: (row.rect.left - from.x) * scale + to.x,
    top: (row.rect.top - from.y) * scale + to.y,
    width: row.rect.width * scale, height: row.rect.height * scale,
  } }));
}

/** Selection lives in screen pixels above the transformed layer. */
export function ElementSelectionOverlay({ surface, project, selection, camera, version, onResizeStart }: {
  surface: { current: HTMLDivElement | null }; project: Project; selection: ElementTarget[]; camera: CanvasCamera; version: string;
  onResizeStart: (event: ReactPointerEvent<HTMLButtonElement>, elementId: string, corner: ElementCorner, sizeId: string | null) => void;
}) {
  const [measured, setMeasured] = useState<{ rows: MeasuredSelection[]; camera: CanvasCamera } | null>(null);
  const latest = useRef({ selection, camera });
  latest.current = { selection, camera };
  const fontRevision = useCanvasFontRevision();
  // Pan and zoom only transform the canvas layer; its measured element geometry stays valid.
  const key = `${project.revision}:${fontRevision}:${selection.map(target => `${target.elementId}:${target.sizeId ?? 'all'}`).join(',')}:${version}`;
  useLayoutEffect(() => {
    const node = surface.current;
    if (!node || !selection.length) { setMeasured(null); return; }
    setMeasured({ rows: measureElementSelection(node, selection), camera });
  }, [key]);
  useEffect(() => {
    const node = surface.current;
    if (!node) return;
    const observer = new ResizeObserver(() => {
      const { selection, camera } = latest.current;
      setMeasured(selection.length ? { rows: measureElementSelection(node, selection), camera } : null);
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [surface]);
  const rects = measured ? projectMeasuredSelection(measured.rows, measured.camera, camera) : [];
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
export function ElementTextEditor({ element, screen, zoom, autoWidth, autoFocus, onAction, onCommit }: {
  element: Element; screen: ScreenRect; zoom: number; autoWidth: boolean; autoFocus: boolean;
  onAction: (action: ShortcutAction) => boolean;
  onCommit: (text: string) => void;
}) {
  const [draft, setDraft] = useState(element.text);
  const done = useRef(false);
  const area = useRef<HTMLTextAreaElement>(null);
  const mirror = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: screen.width, height: screen.height });
  const style = element.style.textStyle;
  const font = useCanvasFont(style);
  useEffect(() => { if (autoFocus && font) { area.current?.focus({ preventScroll: true }); area.current?.select(); } }, [autoFocus, element.id, font?.family]);
  useLayoutEffect(() => {
    const box = mirror.current?.getBoundingClientRect();
    if (box) setSize({ width: box.width, height: box.height });
  }, [draft, font, zoom, screen.width, autoWidth, style, element.style.sizing]);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !area.current?.contains(event.target)) area.current?.blur();
    };
    document.addEventListener('pointerdown', outside, true);
    return () => document.removeEventListener('pointerdown', outside, true);
  }, []);
  function settle() {
    if (done.current) return;
    done.current = true;
    onCommit(area.current?.value ?? draft);
  }
  const typography = { ...textStyle(element), fontSize: style.size * zoom, letterSpacing: style.letterSpacing * zoom };
  const sizing = element.style.sizing;
  const constraints = { minWidth: sizing?.minWidth == null ? undefined : sizing.minWidth * zoom,
    maxWidth: sizing?.maxWidth == null ? undefined : sizing.maxWidth * zoom,
    minHeight: sizing?.minHeight == null ? undefined : sizing.minHeight * zoom,
    maxHeight: sizing?.maxHeight == null ? undefined : sizing.maxHeight * zoom };
  const width = autoWidth ? Math.max(1, size.width) : Math.max(1, screen.width);
  const height = element.style.sizing?.height === 'fit' || autoWidth ? Math.max(style.size * style.lineHeight * zoom, size.height) : screen.height;
  return <>
    <div ref={mirror} aria-hidden style={{ ...typography, ...constraints, position: 'absolute', visibility: 'hidden', pointerEvents: 'none',
      width: autoWidth ? 'max-content' : width, whiteSpace: 'pre-wrap', overflowWrap: 'break-word' }}>{draft + (draft.endsWith('\n') ? '\u200b' : '') || '\u200b'}</div>
    <textarea ref={area} className="element-text-editor" aria-label="Edit text" spellCheck value={draft}
      style={{ ...typography, ...constraints, left: screen.left, top: screen.top, width, height }}
      onChange={event => setDraft(event.target.value)}
      onPointerDown={event => event.stopPropagation()}
      onBlur={settle}
      onKeyDown={event => {
        event.stopPropagation();
        if (event.nativeEvent.isComposing) return;
        const action = shortcutFor(event, true);
        if ((action === 'bold' || action === 'italic') && onAction(action)) { event.preventDefault(); return; }
        if (event.key === 'Escape' || event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
          event.preventDefault(); settle();
        }
      }} />
  </>;
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
