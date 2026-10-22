import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import { native } from './api';
import { siteLayerTree, siteLayerStyles, siteCopyStyles, type SiteLayer } from './siteLayers';
import { translatedStyle } from './elementEditing';
import { resizedElementSize, type ResizeCorner } from './canvasGeometry';
import type { ElementSelection, ElementTarget, ElementStyleEdit, LayerDirection, ElementVisual, Page, PreviewDescriptor, Rect, SiteContent } from './types';

function validLocation(descriptor: PreviewDescriptor, pageId: string) {
  try {
    const url = new URL(descriptor.url);
    if (descriptor.website) {
      return ((url.protocol === 'desegni-preview:' && url.hostname === `${descriptor.sessionId}.localhost`) ||
        (url.protocol === 'http:' && url.hostname === `${descriptor.sessionId}.desegni-preview.localhost`)) && !url.username && !url.password && !url.port && !url.search && !url.hash;
    }
    const supported = (url.protocol === 'desegni-preview:' && url.hostname === 'localhost') ||
      ((url.protocol === 'http:' || url.protocol === 'https:') && url.hostname === 'desegni-preview.localhost');
    return supported && !url.username && !url.password && !url.port && !url.search && !url.hash &&
      decodeURIComponent(url.pathname).replace(/^\/+/, '') === `${descriptor.sessionId}/${pageId}/index.html`;
  } catch { return false; }
}

export type ElementMover = { preview: (x: number, y: number) => ElementSelection | null; style: (x: number, y: number) => string; cancel: () => void };
export type ElementResizer = { preview: (rect: Rect) => ElementSelection | null; style: (rect: Rect) => string; cancel: () => void };
export type InlineText = { selection: ElementSelection; index: number; text: string; rect: Rect; style: CSSProperties };
export type CanvasPreviewHandle = { measuredElements: () => ElementSelection[]; previewImage: (target: ElementTarget, src: string) => boolean; layers: () => SiteLayer[]; layerStyles: (target: ElementTarget, direction: LayerDirection) => ElementStyleEdit[] | null; copyStyles: (target: ElementTarget) => ElementStyleEdit[]; source: () => string | null; find: (target: ElementTarget) => ElementSelection | null; contains: (target: ElementTarget) => boolean; visual: (target: ElementTarget) => ElementVisual | null; textAt: (x: number, y: number) => InlineText | null; occupied: (target: ElementTarget) => Rect[]; beginMove: (target: ElementTarget) => ElementMover | null; beginResize: (target: ElementTarget, corner: ResizeCorner) => ElementResizer | null; pick: (x: number, y: number) => ElementSelection | null; imageAt: (x: number, y: number) => ElementSelection | null; foundSource: (target: ElementTarget) => string | null; found: () => ElementSelection[]; placeholders: () => ElementSelection[]; next: (parent?: boolean, target?: ElementTarget) => ElementSelection | null };
const staticPolicy = "default-src 'none'; script-src 'none'; script-src-attr 'none'; style-src 'unsafe-inline' https: http: data:; img-src data: https: http:; connect-src 'none'; font-src data: https: http:; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; media-src 'none'; worker-src 'none'";

export const PreviewFrame = forwardRef<CanvasPreviewHandle, {
  pageId: string; draftId?: string | null; jobId?: string | null; runScripts?: boolean; title: string; staticView?: boolean; revision?: number | string;
  content?: SiteContent; onReady?: () => void; referenceHeight?: number; onHeight?: (height: number) => void;
}>(function PreviewFrame({ pageId, jobId = null, draftId = null, runScripts = false, title, staticView = false, revision = 0, content, referenceHeight = 900, onHeight, onReady }, ref) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [extent, setExtent] = useState(referenceHeight);
  const [descriptor, setDescriptor] = useState<PreviewDescriptor | null>(null);
  const [previewSource, setPreviewSource] = useState<SiteContent | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const [observations, setObservations] = useState<string[]>([]);
  // A web link clicked in the preview, waiting for the user to open it in their browser.
  const [outsideLink, setOutsideLink] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const picked = useRef<Element | null>(null);
  const mounted = useRef<{ document: Document; session: string } | null>(null);
  const invalidated = useRef(false);
  const registeredVersion = useRef<{ pageId: string; revision: number | string } | null>(null);
  // Canvas documents share DOM access only while all scripts remain forbidden by sandbox.
  // Interactive previews keep their separate opaque-origin, explicitly enabled sandbox.
  function identify(node: Element | null, exact = true): { node: Element; target: ElementTarget } | null {
    const doc = frame.current?.contentDocument;
    if (!doc || invalidated.current || mounted.current?.document !== doc || mounted.current.session !== descriptor?.sessionId || doc.URL !== 'about:srcdoc' || !node || !descriptor?.sourceSha256 || !doc.body.contains(node)) return null;
    if (!exact) node = Element.prototype.closest.call(node, 'button, a, input, select, textarea, label, summary') ?? node;
    if (!/^[A-Za-z][A-Za-z0-9-]{0,79}$/.test(node.localName)) return null;
    const parts: string[] = [];
    for (let current: Element | null = node; current && current !== doc.body; current = current.parentElement) {
      if (!/^[A-Za-z][A-Za-z0-9-]{0,79}$/.test(current.localName) || parts.length >= 63) return null;
      const siblings: Element[] = Array.from(current.parentElement?.children ?? []);
      parts.unshift(`${current.localName}:nth-of-type(${siblings.filter(sibling => sibling.localName === current!.localName).indexOf(current) + 1})`);
    }
    let label = '';
    const walker = Document.prototype.createTreeWalker.call(doc, node, NodeFilter.SHOW_TEXT);
    let visits = 0;
    while (label.length < 240 && visits++ < 1024) {
      const text = walker.nextNode(); if (!text) break;
      if (!['SCRIPT', 'STYLE'].includes(text.parentElement?.tagName ?? '')) label += ` ${text.nodeValue?.slice(0, 240) ?? ''}`;
    }
    label = node.getAttribute('aria-label') || label.trim() || node.getAttribute('alt') || node.getAttribute('title') || '';
    return { node, target: { selector: ['body', ...parts].join(' > '), tag: node.localName,
      label: Array.from(label.replace(/\s+/g, ' ').trim()).slice(0, 240).join(''), ...(node.getAttribute('data-desegnos-id') ? { id: node.getAttribute('data-desegnos-id')! } : { sourceSha256: descriptor.sourceSha256 }) } };
  }
  function selected(node: Element | null, remember = false, exact = false): ElementSelection | null {
    const found = identify(node, exact); if (!found) return null;
    const box = Element.prototype.getBoundingClientRect.call(found.node);
    if (![box.x, box.y, box.width, box.height, box.right, box.bottom].every(Number.isFinite) || box.width < 1 || box.height < 1) return null;
    if (remember) picked.current = found.node;
    return { element: found.target, rect: { x: box.x, y: box.y, width: box.width, height: box.height } };
  }

  function findNode(target: ElementTarget): Element | null {
    const doc = frame.current?.contentDocument;
    if (!doc || invalidated.current || mounted.current?.document !== doc || mounted.current.session !== descriptor?.sessionId || doc.URL !== 'about:srcdoc' || (!target.id && target.sourceSha256 !== descriptor?.sourceSha256)) return null;
    try { const node = Document.prototype.querySelector.call(doc, target.id ? `[data-desegnos-id="${CSS.escape(target.id)}"]` : target.selector); return node && (target.id || node.localName === target.tag) ? node : null; } catch { return null; }
  }
  function visual(target: ElementTarget): ElementVisual | null {
    const node = findNode(target); if (!node) return null;
    const style = getComputedStyle(node);
    return { translate: style.translate, color: style.color, backgroundColor: style.backgroundColor, fontSize: style.fontSize, width: style.width, height: style.height, borderRadius: style.borderRadius, padding: style.padding, textAlign: style.textAlign };
  }
  function textNodes(node: Node): Text[] {
    const result: Text[] = []; const pending = [node];
    while (pending.length) {
      const current = pending.pop()!;
      if (['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'SVG'].includes(current.nodeName)) continue;
      if (current.nodeType === Node.TEXT_NODE) result.push(current as Text);
      else pending.push(...Array.from(current.childNodes).reverse());
    }
    return result;
  }
  function textAt(x: number, y: number): InlineText | null {
    const doc = frame.current?.contentDocument;
    const selection = selected(doc?.elementFromPoint(x, y) ?? null, true);
    if (!doc || !selection || ['input', 'textarea', 'select', 'svg', 'body'].includes(selection.element.tag)) return null;
    const node = findNode(selection.element); if (!node) return null;
    const nodes = textNodes(node);
    for (let index = 0; index < nodes.length; index++) {
      const text = nodes[index]; if (!text.data.trim()) continue;
      const range = doc.createRange(); range.selectNodeContents(text);
      if (!Array.from(range.getClientRects()).some(rect => x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom)) continue;
      const box = range.getBoundingClientRect(); const style = getComputedStyle(text.parentElement!);
      let backgroundColor = '#ffffff';
      for (let parent = text.parentElement; parent; parent = parent.parentElement) {
        const value = getComputedStyle(parent).backgroundColor;
        if (value !== 'transparent' && !/^rgba\([^)]*,\s*0(?:\.0+)?\)$/.test(value)) { backgroundColor = value; break; }
      }
      return { selection, index, text: text.data, rect: { x: box.x, y: box.y, width: box.width, height: box.height }, style: {
        fontFamily: style.fontFamily, fontSize: style.fontSize, fontWeight: style.fontWeight, fontStyle: style.fontStyle,
        lineHeight: style.lineHeight, letterSpacing: style.letterSpacing, textAlign: style.textAlign as CSSProperties['textAlign'],
        textTransform: style.textTransform, textDecoration: style.textDecoration, direction: style.direction as 'ltr' | 'rtl', color: style.color, backgroundColor,
      } };
    }
    return null;
  }
  function occupied(target: ElementTarget): Rect[] {
    const doc = frame.current?.contentDocument; const node = findNode(target); if (!doc || !node) return [];
    const result: Rect[] = [];
    // Text and replaced controls represent content, rather than their large
    // layout containers. Ignore the selected subtree and its own ancestor text.
    const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
    let current: Node | null; let visited = 0;
    while ((current = walker.nextNode()) && visited++ < 8192) {
      if (node.contains(current)) continue;
      if (current.nodeType === Node.TEXT_NODE && current.textContent?.trim() && !current.parentElement?.closest('script,style,noscript,template,svg')) {
        const range = doc.createRange(); range.selectNodeContents(current);
        for (const box of range.getClientRects()) if (box.width && box.height) result.push({ x: box.x, y: box.y, width: box.width, height: box.height });
      } else if (current instanceof doc.defaultView!.Element && current.matches('img,input,textarea,select,video,canvas,svg')) {
        const box = current.getBoundingClientRect(); if (box.width && box.height) result.push({ x: box.x, y: box.y, width: box.width, height: box.height });
      }
    }
    return result;
  }
  function transformEditor(target: ElementTarget) {
    const node = findNode(target); const values = visual(target);
    if (!node || !values || target.selector === 'body') return null;
    for (let ancestor: Element | null = node; ancestor; ancestor = ancestor.parentElement) {
      const css = getComputedStyle(ancestor);
      if (css.perspective !== 'none' || css.transform.startsWith('matrix3d(')) return null;
    }
    const original = Element.prototype.getAttribute.call(node, 'style');
    const restore = () => { if (original === null) Element.prototype.removeAttribute.call(node, 'style'); else Element.prototype.setAttribute.call(node, 'style', original); };
    const set = (css: string) => { const style = document.createElement('div').style; style.cssText = css; style.setProperty('transition', 'none', 'important'); Element.prototype.setAttribute.call(node, 'style', style.cssText); };
    const origin = Element.prototype.getBoundingClientRect.call(node);
    let horizontal: DOMRect; let vertical: DOMRect;
    try {
      set(translatedStyle(original ?? '', values.translate, 1, 0)); horizontal = Element.prototype.getBoundingClientRect.call(node); restore();
      set(translatedStyle(original ?? '', values.translate, 0, 1)); vertical = Element.prototype.getBoundingClientRect.call(node);
    } finally { restore(); }
    const a = horizontal.x - origin.x; const b = horizontal.y - origin.y; const c = vertical.x - origin.x; const d = vertical.y - origin.y;
    const determinant = a * d - b * c;
    if (!Number.isFinite(determinant) || Math.abs(determinant) < 1e-6) return null;
    const offset = (css: string, x: number, y: number) => translatedStyle(css, values.translate, (d * x - c * y) / determinant, (a * y - b * x) / determinant);
    return { node, original: original ?? '', origin, restore, set, offset };
  }
  useImperativeHandle(ref, () => ({
    previewImage(target, src) {
      const node = findNode(target);
      if (!node || node.localName !== 'img') return false;
      node.removeAttribute('srcset'); node.removeAttribute('sizes'); node.setAttribute('src', src);
      return true;
    },
    source() { return registeredVersion.current?.pageId === pageId && registeredVersion.current.revision === revision && mounted.current?.session === descriptor?.sessionId && mounted.current?.document === frame.current?.contentDocument && !invalidated.current ? descriptor?.sourceSha256 ?? null : null; },
    find(target) { return selected(findNode(target), false, true); },
    contains(target) { return !!findNode(target); },
    layers() { const doc = frame.current?.contentDocument; return doc && identify(doc.body) ? siteLayerTree(doc, node => selected(node, false, true)) : []; },
    measuredElements() {
      const doc = frame.current?.contentDocument;
      if (!doc || !identify(doc.body)) return [];
      const nodes = Array.from(doc.querySelectorAll('[data-desegnos-id]'));
      if (nodes.length > 10_000) throw new Error('This page has more than 10,000 elements. Select individual elements for this request.');
      return nodes.map(node => selected(node, false, true)).filter((item): item is ElementSelection => !!item);
    },
    layerStyles(target, direction) { const node = findNode(target); if (!node) throw new Error('The page changed. Select this layer again.'); return siteLayerStyles(node, direction, value => identify(value)?.target ?? null); },
    copyStyles(target) { const node = findNode(target); if (!node) throw new Error('The page changed. Select this layer again.'); return siteCopyStyles(node, value => identify(value)?.target ?? null); },
    visual,
    textAt,
    occupied,
    beginMove(target) {
      const edit = transformEditor(target); if (!edit) return null;
      const style = (x: number, y: number) => edit.offset(edit.original, x, y);
      return { style, cancel: edit.restore, preview(x, y) { if (!findNode(target)) return null; edit.set(style(x, y)); return selected(edit.node, false, true); } };
    },
    beginResize(target, corner) {
      const edit = transformEditor(target); if (!edit) return null;
      const { node, original, origin, restore, set, offset } = edit;
      if (node.namespaceURI !== 'http://www.w3.org/1999/xhtml' && node.localName !== 'svg') return null;
      const computed = getComputedStyle(node);
      const number = (value: string) => Number.parseFloat(value) || 0;
      const paddingX = number(computed.paddingLeft) + number(computed.paddingRight) + number(computed.borderLeftWidth) + number(computed.borderRightWidth);
      const paddingY = number(computed.paddingTop) + number(computed.paddingBottom) + number(computed.borderTopWidth) + number(computed.borderBottomWidth);
      const localWidth = computed.width === 'auto' ? Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetWidth')?.get?.call(node) ?? 0 : number(computed.width) + (computed.boxSizing === 'border-box' ? 0 : paddingX);
      const localHeight = computed.height === 'auto' ? Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight')?.get?.call(node) ?? 0 : number(computed.height) + (computed.boxSizing === 'border-box' ? 0 : paddingY);
      if (localWidth <= 0 || localHeight <= 0) return null;
      const base = document.createElement('div').style; base.cssText = original;
      if (computed.display === 'inline') base.setProperty('display', 'inline-block', 'important');
      for (const [name, value] of Object.entries({ 'box-sizing':'border-box', 'min-width':'0px', 'min-height':'0px', 'max-width':'none', 'max-height':'none', 'flex-grow':'0', 'flex-shrink':'0', 'flex-basis':'auto' })) base.setProperty(name,value,'important');
      const sized = (width: number, height: number) => {
        const css = document.createElement('div').style; css.cssText = base.cssText;
        css.setProperty('width',`${Math.max(paddingX + 1, width)}px`,'important'); css.setProperty('height',`${Math.max(paddingY + 1, height)}px`,'important'); return css.cssText;
      };
      let initial: DOMRect; let horizontal: DOMRect; let vertical: DOMRect;
      try {
        set(sized(localWidth,localHeight)); initial = Element.prototype.getBoundingClientRect.call(node);
        set(sized(localWidth+4,localHeight)); horizontal = Element.prototype.getBoundingClientRect.call(node);
        set(sized(localWidth,localHeight+4)); vertical = Element.prototype.getBoundingClientRect.call(node);
      } finally { restore(); }
      const a=(horizontal.width-initial.width)/4; const b=(horizontal.height-initial.height)/4;
      const c=(vertical.width-initial.width)/4; const d=(vertical.height-initial.height)/4;
      const dimensions = { x: localWidth, y: localHeight }, minimum = { x: paddingX + 1, y: paddingY + 1 };
      if (!resizedElementSize(dimensions, minimum, [a,b,c,d], { x: 0, y: 0 })) return null;
      const west=corner.includes('w'); const north=corner.includes('n');
      const style = (rect: Rect) => {
        const dw=rect.width-initial.width; const dh=rect.height-initial.height;
        const size=resizedElementSize(dimensions, minimum, [a,b,c,d], { x: dw, y: dh });
        if (!size) throw new Error('This item cannot be resized in its current layout.');
        const css=sized(size.x,size.y);
        // Size may change text flow, transform origins or centred layout. Measure before fixing the opposite corner.
        try {
          set(css); const box=Element.prototype.getBoundingClientRect.call(node);
          const x=(west ? origin.right-box.right : origin.x-box.x); const y=(north ? origin.bottom-box.bottom : origin.y-box.y);
          return offset(css,x,y);
        } finally { restore(); }
      };
      return { style, cancel:restore, preview(rect) { if (!findNode(target)) return null; set(style(rect)); return selected(node, false, true); } };
    },
    pick(x, y) { try { return selected(frame.current?.contentDocument?.elementFromPoint(x, y) ?? null, true); } catch { return null; } },
    // The picture under a point, even inside a link or button, for dropping image files.
    imageAt(x, y) {
      try {
        const node = frame.current?.contentDocument?.elementFromPoint(x, y) ?? null;
        const image = node?.closest('img') ?? (node && node.querySelectorAll('img').length === 1 ? node.querySelector('img') : null);
        return image ? selected(image, false, true) : null;
      } catch { return null; }
    },
    // Content the AI found online carries its source URL (editor metadata, removed on export).
    foundSource(target) { return findNode(target)?.getAttribute('data-desegnos-found') ?? null; },
    found() {
      const doc = frame.current?.contentDocument; if (!doc) return [];
      return Array.from(doc.querySelectorAll('[data-desegnos-found]')).flatMap(node => { const hit = selected(node, false, true); return hit ? [hit] : []; });
    },
    placeholders() {
      const doc = frame.current?.contentDocument; if (!doc) return [];
      return Array.from(doc.querySelectorAll('img[data-desegnos-placeholder]')).flatMap(node => { const found = selected(node, false, true); return found ? [found] : []; });
    },
    next(parent = false, target) {
      try {
        const doc = frame.current?.contentDocument;
        if (!doc) return null;
        const current = target ? findNode(target) : picked.current;
        if (parent && current) return selected(current.parentElement, true);
        const elements = [...new Set([...doc.body.querySelectorAll('*')].filter(node => !['SCRIPT', 'STYLE', 'META', 'LINK'].includes(node.tagName) && node.getBoundingClientRect().width >= 1 && node.getBoundingClientRect().height >= 1).map(node => Element.prototype.closest.call(node, 'button, a, input, select, textarea, label, summary') ?? node))];
        return selected(elements[(elements.indexOf(current!) + 1) % elements.length] ?? doc.body, true);
      } catch { return null; }
    },
  }));
  const rate = useRef({ started: 0, count: 0 });
  useEffect(() => {
    let active = true;
    let registered: string | null = null;
    picked.current = null; mounted.current = null; invalidated.current = false; setExtent(referenceHeight);
    setDescriptor(null); setError(''); setLoading(true); setObservations([]);
    void native<PreviewDescriptor>('create_preview', { pageId, jobId, draftId, runScripts }).then(async value => {
      if (!active) { void native('release_preview', { sessionId: value.sessionId }).catch(() => {}); return; }
      registered = value.sessionId;
      if (!validLocation(value, pageId)) {
        void native('release_preview', { sessionId: registered }).catch(() => {});
        registered = null;
        throw new Error('The application returned an invalid preview location.');
      }
      if (staticView && content) {
        const bytes = new TextEncoder().encode(JSON.stringify({ html: content.html, css: content.css, javascript: content.javascript }));
        const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))].map(byte => byte.toString(16).padStart(2, '0')).join('');
        if (digest !== value.sourceSha256) throw new Error('The page changed while opening its preview. Reopen the page.');
      }
      if (active) { registeredVersion.current = { pageId, revision }; setPreviewSource(value.content ?? content ?? null); setDescriptor(value); }
    }).catch(error => { if (active) { setError(error instanceof Error ? error.message : String(error)); setLoading(false); } });
    return () => { active = false; if (registered) void native('release_preview', { sessionId: registered }).catch(() => {}); };
  }, [pageId, jobId, draftId, runScripts, revision, retry]);
  useEffect(() => {
    if (!descriptor || staticView) return;
    function receive(event: MessageEvent) {
      if (event.source !== frame.current?.contentWindow || !event.data || typeof event.data !== 'object' || Array.isArray(event.data)) return;
      const now = performance.now();
      if (now - rate.current.started > 1000) rate.current = { started: now, count: 0 };
      if (++rate.current.count > 10) return;
      const value = event.data as Record<string, unknown>;
      if (value.type === 'desegni-preview-link' && value.sessionId === descriptor!.sessionId && value.pageId === pageId &&
        typeof value.url === 'string' && /^https?:\/\//.test(value.url) && value.url.length <= 2048 && Object.keys(value).length === 4) { setOutsideLink(value.url); return; }
      if (value.type !== 'desegni-preview-error' || value.sessionId !== descriptor!.sessionId || value.pageId !== pageId ||
        typeof value.message !== 'string' || value.message.length > 4096 || Object.keys(value).length !== 4) return;
      const message = value.message;
      setObservations(previous => previous.includes(message) ? previous : [...previous.slice(-9), message]);
    }
    window.addEventListener('message', receive);
    return () => window.removeEventListener('message', receive);
  }, [descriptor, pageId, staticView]);
  useLayoutEffect(() => {
    if (!staticView || !mounted.current || invalidated.current) return;
    Document.prototype.getAnimations?.call(mounted.current.document).forEach(animation => animation.pause());
  }, [staticView, extent]);
  const sourceDocument = staticView && previewSource ? `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${staticPolicy}"><meta name="viewport" content="width=device-width, initial-scale=1"></head><body>${previewSource.html}</body></html>` : undefined;
  return <div data-source-hash={descriptor?.sourceSha256 ?? undefined} data-preview-ready={staticView && !loading && !error && registeredVersion.current?.revision === revision && mounted.current?.session === descriptor?.sessionId ? 'true' : undefined} className={staticView ? 'preview-frame static-preview' : 'preview-frame'}>
    {loading && !staticView && <p role="status" className="preview-status">Preparing preview…</p>}
    {error && <div className="preview-error"><p role="alert">{error}</p>{!staticView && <button onClick={() => setRetry(value => value + 1)}>Retry preview</button>}</div>}
    {descriptor && <iframe key={`${descriptor.sessionId}:${runScripts}`} ref={frame} inert={staticView || undefined} src={sourceDocument ? undefined : descriptor.url} srcDoc={sourceDocument} sandbox={sourceDocument ? 'allow-same-origin' : runScripts ? 'allow-scripts' : ''} referrerPolicy="no-referrer" title={title}
      style={sourceDocument ? { height: extent } : undefined} tabIndex={staticView ? -1 : undefined} aria-hidden={staticView || undefined} onLoad={event => {
        if (event.currentTarget !== frame.current) return;
        setLoading(false);
        if (!sourceDocument || !previewSource) return;
        try {
          const doc = frame.current?.contentDocument;
          if (invalidated.current || !doc?.head || !doc.body || doc.URL !== 'about:srcdoc' || (mounted.current && mounted.current.document !== doc) || doc.querySelector('meta[http-equiv="Content-Security-Policy"]')?.getAttribute('content') !== staticPolicy) throw new Error('The page navigated away from its editable preview.');
          mounted.current = { document: doc, session: descriptor.sessionId };
          const style = Document.prototype.createElement.call(doc, 'style'); style.textContent = previewSource.css; doc.head.append(style);
          const still = Document.prototype.createElement.call(doc, 'style'); still.setAttribute('data-desegnos-editor-paused', ''); still.textContent = '*,*::before,*::after{animation-play-state:paused!important;transition:none!important}'; doc.head.append(still);
          // Measure once against the reference viewport. Observing our own iframe resize would create a vh/percentage-height feedback loop.
          const height = Math.min(100000, Math.max(referenceHeight, doc.body.scrollHeight, doc.documentElement.scrollHeight));
          Document.prototype.getAnimations?.call(doc).forEach(animation => animation.pause());
          setExtent(height); onHeight?.(height); onReady?.();
        } catch { invalidated.current = true; mounted.current = null; picked.current = null; setError('This page left its editable preview. Reopen the page to select elements.'); onReady?.(); }
      }}
      onError={() => { setError('The preview document could not be loaded.'); setLoading(false); }} />}
    {!staticView && outsideLink && <div className="message info preview-link" role="status"><span>This link leaves your site: <strong title={outsideLink}>{outsideLink.replace(/^https?:\/\//, '').slice(0, 80)}</strong></span>
      <button className="primary" onClick={() => { const url = outsideLink; setOutsideLink(null); void native('open_preview_link', { url }).catch(error => setObservations(previous => [...previous.slice(-9), error instanceof Error ? error.message : String(error)])); }}>Open in browser</button>
      <button aria-label="Dismiss link" onClick={() => setOutsideLink(null)}>×</button></div>}
    {!staticView && observations.length > 0 && <section className="preview-observations" aria-label="Preview runtime observations"><h3>Preview reported errors</h3>
      <ul>{observations.map((message, index) => <li key={index}>{message}</li>)}</ul><button onClick={() => setObservations([])}>Clear observations</button>
    </section>}
  </div>;
});

export function SourceView({ content, label = 'Site' }: { content: SiteContent; label?: string }) {
  const [tab, setTab] = useState<keyof SiteContent>('html');
  return <section className="site-source"><nav className="tabs" aria-label={`${label} source files`}>
    {(['html', 'css', 'javascript'] as const).map(kind => <button key={kind} aria-pressed={tab === kind} onClick={() => setTab(kind)}>{kind === 'javascript' ? 'JavaScript' : kind.toUpperCase()}</button>)}
    </nav>{!content[tab] && <p className="small muted source-empty">This source file is empty.</p>}<pre className="markdown-source" tabIndex={0} aria-label={`${label} ${tab === 'javascript' ? 'JavaScript' : tab.toUpperCase()} source`}>{content[tab]}</pre>
  </section>;
}

const DEVICES = [{ id: 'desktop', label: 'Desktop', width: 1440, height: 900 }, { id: 'tablet', label: 'Tablet', width: 768, height: 1024 }, { id: 'phone', label: 'Phone', width: 390, height: 844 }] as const;

export function PagePreview({ page, revision, jobId = null, draftId = null, initialDevice, fixOnPhones }: { page: Page; revision: number; jobId?: string | null; draftId?: string | null; initialDevice?: 'desktop' | 'tablet' | 'phone'; fixOnPhones?: () => void }) {
  const [runScripts, setRunScripts] = useState(false);
  const [deviceId, setDeviceId] = useState<typeof DEVICES[number]['id']>(initialDevice ?? (page.viewport.width < 600 ? 'phone' : 'desktop'));
  const stage = useRef<HTMLDivElement>(null);
  const [available, setAvailable] = useState(0);
  useLayoutEffect(() => {
    const node = stage.current; if (!node) return;
    const observer = new ResizeObserver(() => setAvailable(node.clientWidth)); observer.observe(node); setAvailable(node.clientWidth);
    return () => observer.disconnect();
  }, []);
  const device = DEVICES.find(item => item.id === deviceId)!;
  // Render at the real device width, then scale it down to fit, so layouts break exactly as on that device.
  const height = Math.min(device.height, 620);
  const scale = available ? Math.min(1, available / device.width) : 1;
  return <section className="page-preview">
    <div className="preview-toolbar">
      <div className="segmented device-switch" role="group" aria-label="Preview device">{DEVICES.map(item => <button key={item.id} type="button" aria-pressed={deviceId === item.id} onClick={() => setDeviceId(item.id)}>{item.label}</button>)}</div>
      <span className="field-help">{device.width} px wide{scale < 1 ? ` · shown at ${Math.round(scale * 100)}%` : ''}</span>
      {deviceId === 'phone' && fixOnPhones && <button type="button" className="primary" title="Fix on phones" onClick={fixOnPhones}>Fix on phones</button>}
      {!draftId && <label className="check-label"><input type="checkbox" checked={runScripts} onChange={event => setRunScripts(event.target.checked)} />Try buttons and links</label>}
    </div>
    <div ref={stage} className="device-stage" style={{ height: height * scale }}>
      <div className={`device-frame device-${device.id}`} style={{ width: device.width, '--device-height': `${height}px`, transform: `scale(${scale})`, left: Math.max(0, (available - device.width * scale) / 2) } as CSSProperties}>
        <PreviewFrame pageId={page.id} jobId={jobId} draftId={draftId} revision={revision} runScripts={runScripts} title={`${page.name} preview on ${device.label}`} />
      </div>
    </div>
  </section>;
}
