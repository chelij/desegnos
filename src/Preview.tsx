import { Notice, ProblemBanner } from './ProblemBanner.js';
import { userError, runtimeError } from './userErrors.js';
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import { native } from './api';
import { siteLayerTree, siteLayerStyles, siteCopyStyles, type SiteLayer } from './siteLayers';
import { translatedStyle } from './elementEditing';
import { measurableDocument, needsReferenceMeasure, pendingFrameHeight, promotionHeight } from './previewMeasurement';
import { hardenStaticFrames, staticBase, staticDocument, staticPolicy } from './previewDocument';
import { collectFrame, frameKey, resolveFrameSource, type CollectedFrame } from './previewFrames';
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
export type FrameView = { pageUrl: string | null; frames: CollectedFrame[] };
export type CanvasPreviewHandle = { frameOverlays: () => FrameView; hideStaticFrames: (keys: readonly string[]) => void; measuredElements: () => ElementSelection[]; previewImage: (target: ElementTarget, src: string) => boolean; layers: () => SiteLayer[]; layerStyles: (target: ElementTarget, direction: LayerDirection) => ElementStyleEdit[] | null; copyStyles: (target: ElementTarget) => ElementStyleEdit[]; source: () => string | null; find: (target: ElementTarget) => ElementSelection | null; contains: (target: ElementTarget) => boolean; visual: (target: ElementTarget) => ElementVisual | null; textAt: (x: number, y: number) => InlineText | null; occupied: (target: ElementTarget) => Rect[]; beginMove: (target: ElementTarget) => ElementMover | null; beginResize: (target: ElementTarget, corner: ResizeCorner) => ElementResizer | null; pick: (x: number, y: number) => ElementSelection | null; imageAt: (x: number, y: number) => ElementSelection | null; foundSource: (target: ElementTarget) => string | null; found: () => ElementSelection[]; placeholders: () => ElementSelection[]; next: (parent?: boolean, target?: ElementTarget) => ElementSelection | null };

export const PreviewFrame = forwardRef<CanvasPreviewHandle, {
  pageId: string; draftId?: string | null; jobId?: string | null; runScripts?: boolean; title: string; staticView?: boolean; revision?: number | string;
  onRuntimeProblems?: (errors: string[]) => void; fixRuntimeErrors?: (errors: string[]) => void; aiDisabled?: boolean; observationsVersion?: number;
  content?: SiteContent; onReady?: (settled: boolean, revision: number | string) => void; referenceHeight?: number; onHeight?: (height: number) => void;
}>(function PreviewFrame({ pageId, jobId = null, draftId = null, runScripts = false, title, staticView = false, revision = 0, content, referenceHeight = 900, onHeight, onReady, onRuntimeProblems, fixRuntimeErrors, aiDisabled, observationsVersion }, ref) {
  const frame = useRef<HTMLIFrameElement>(null);
  const stagingFrame = useRef<HTMLIFrameElement>(null);
  const [extent, setExtent] = useState(referenceHeight);
  type PreparedFrame = { descriptor: PreviewDescriptor; source: SiteContent | null; pageId: string; revision: number | string; runScripts: boolean };
  const [visible, setVisible] = useState<PreparedFrame | null>(null);
  const descriptor = visible?.descriptor ?? null;
  const [pending, setPending] = useState<PreparedFrame | null>(null);
  const pendingRef = useRef(pending); pendingRef.current = pending;
  const visibleSession = useRef<string | null>(null); visibleSession.current = descriptor?.sessionId ?? null;
  const readyDocument = useRef<{ document: Document; session: string; height: number } | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const [observations, setObservations] = useState<string[]>([]);
  useEffect(() => { onRuntimeProblems?.(observations); }, [observations, onRuntimeProblems]);
  // A web link clicked in the preview, waiting for the user to open it in their browser.
  const [outsideLink, setOutsideLink] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const picked = useRef<Element | null>(null);
  const mounted = useRef<{ document: Document; session: string } | null>(null);
  // The frame the canvas shows, recorded when it is promoted. Every measurement reads
  // this document and nothing else: the hidden pending frame and a replaced frame hold
  // layouts that are not on screen.
  const shown = useRef<{ pageId: string; revision: number | string; session: string; document: Document } | null>(null);
  const invalidated = useRef(false);
  // The inline opacity we replaced while a static frame sits under its live overlay.
  const previousOpacity = useRef(new WeakMap<Element, string>()).current;
  const registeredVersion = useRef<{ pageId: string; revision: number | string } | null>(null);
  // Canvas documents share DOM access only while all scripts remain forbidden by sandbox.
  // Interactive previews keep their separate opaque-origin, explicitly enabled sandbox.
  function measurable(): Document | null {
    return measurableDocument(shown.current, { pageId, revision, session: descriptor?.sessionId ?? '', document: frame.current?.contentDocument ?? null, invalidated: invalidated.current });
  }
  function identify(node: Element | null, exact = true): { node: Element; target: ElementTarget } | null {
    const doc = measurable();
    if (!doc || doc.URL !== 'about:srcdoc' || !node || !descriptor?.sourceSha256 || !doc.body.contains(node)) return null;
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
    const doc = measurable();
    if (!doc || doc.URL !== 'about:srcdoc' || (!target.id && target.sourceSha256 !== descriptor?.sourceSha256)) return null;
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
    const doc = measurable();
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
    const doc = measurable(); const node = findNode(target); if (!doc || !node) return [];
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
    // The frames the canvas can show live, read from the promoted document. A frame the
    // canvas hides under its overlay keeps reporting itself so the overlay stays put.
    frameOverlays(): FrameView {
      const doc = measurable();
      if (!doc || doc.URL !== 'about:srcdoc') return { pageUrl: null, frames: [] };
      const pageUrl = descriptor?.url ?? null;
      const frames: CollectedFrame[] = [];
      Array.from(doc.querySelectorAll('iframe')).forEach((node, index) => {
        const box = node.getBoundingClientRect();
        const style = getComputedStyle(node);
        const frame = collectFrame({
          rect: { x: box.x, y: box.y, width: box.width, height: box.height },
          display: style.display,
          visibility: style.visibility,
          src: node.getAttribute('src'), sandbox: node.getAttribute('sandbox'), allow: node.getAttribute('allow'),
          allowFullscreen: node.hasAttribute('allowfullscreen'),
        }, pageUrl, index);
        if (frame) frames.push({ ...frame, selector: identify(node, true)?.target.selector ?? null });
      });
      return { pageUrl, frames };
    },
    hideStaticFrames(keys) {
      const doc = measurable();
      if (!doc || doc.URL !== 'about:srcdoc') return;
      const wanted = new Set(keys);
      const pageUrl = descriptor?.url ?? null;
      Array.from(doc.querySelectorAll('iframe')).forEach((node, index) => {
        const key = frameKey(index, resolveFrameSource(node.getAttribute('src'), pageUrl));
        const hidden = node.hasAttribute('data-desegnos-frame-overlay');
        if (wanted.has(key) && !hidden) {
          // Hide the still render without touching its box or hit testing: the canvas still
          // picks the frame element with elementFromPoint. `visibility: hidden` would make
          // the click fall through to whatever is behind the frame.
          previousOpacity.set(node, node.style.getPropertyValue('opacity'));
          node.setAttribute('data-desegnos-frame-overlay', '');
          node.style.setProperty('opacity', '0', 'important');
        } else if (!wanted.has(key) && hidden) {
          node.removeAttribute('data-desegnos-frame-overlay');
          node.style.removeProperty('opacity');
          const previous = previousOpacity.get(node);
          if (previous) node.style.setProperty('opacity', previous);
          previousOpacity.delete(node);
        }
      });
    },
    previewImage(target, src) {
      const node = findNode(target);
      if (!node || node.localName !== 'img') return false;
      node.removeAttribute('srcset'); node.removeAttribute('sizes'); node.setAttribute('src', src);
      return true;
    },
    source() { return measurable() ? descriptor?.sourceSha256 ?? null : null; },
    find(target) { return selected(findNode(target), false, true); },
    contains(target) { return !!findNode(target); },
    layers() { const doc = measurable(); return doc && identify(doc.body) ? siteLayerTree(doc, node => selected(node, false, true)) : []; },
    measuredElements() {
      const doc = measurable();
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
    pick(x, y) { try { return selected(measurable()?.elementFromPoint(x, y) ?? null, true); } catch { return null; } },
    // The picture under a point, even inside a link or button, for dropping image files.
    imageAt(x, y) {
      try {
        const node = measurable()?.elementFromPoint(x, y) ?? null;
        const image = node?.closest('img') ?? (node && node.querySelectorAll('img').length === 1 ? node.querySelector('img') : null);
        return image ? selected(image, false, true) : null;
      } catch { return null; }
    },
    // Content the AI found online carries its source URL (editor metadata, removed on export).
    foundSource(target) { return findNode(target)?.getAttribute('data-desegnos-found') ?? null; },
    found() {
      const doc = measurable(); if (!doc) return [];
      return Array.from(doc.querySelectorAll('[data-desegnos-found]')).flatMap(node => { const hit = selected(node, false, true); return hit ? [hit] : []; });
    },
    placeholders() {
      const doc = measurable(); if (!doc) return [];
      return Array.from(doc.querySelectorAll('img[data-desegnos-placeholder]')).flatMap(node => { const found = selected(node, false, true); return found ? [found] : []; });
    },
    next(parent = false, target) {
      try {
        const doc = measurable();
        if (!doc) return null;
        const current = target ? findNode(target) : picked.current;
        if (parent && current) return selected(current.parentElement, true);
        const elements = [...new Set([...doc.body.querySelectorAll('*')].filter(node => !['SCRIPT', 'STYLE', 'META', 'LINK'].includes(node.tagName) && node.getBoundingClientRect().width >= 1 && node.getBoundingClientRect().height >= 1).map(node => Element.prototype.closest.call(node, 'button, a, input, select, textarea, label, summary') ?? node))];
        return selected(elements[(elements.indexOf(current!) + 1) % elements.length] ?? doc.body, true);
      } catch { return null; }
    },
  }));
  useEffect(() => { setObservations([]); }, [observationsVersion]);
  const rate = useRef({ started: 0, count: 0 });
  // Errors reported by the current load; null while a reload is pending.
  const reported = useRef<string[] | null>(null);
  const stagedProblems = useRef<{ session: string; errors: string[] } | null>(null);
  useEffect(() => {
    let active = true;
    let registered: string | null = null;
    setPending(null);
    // Keep the last errors until the reloaded page reports again, so the banner doesn't blink on every edit.
    setError(''); setLoading(true); reported.current = null;
    if (registeredVersion.current?.pageId !== pageId) setObservations([]);
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
      if (active) {
        stagedProblems.current = { session: value.sessionId, errors: [] };
        setPending({ descriptor: value, source: value.content ?? content ?? null, pageId, revision, runScripts });
      }
    }).catch(error => { if (registered && registered !== visibleSession.current) void native('release_preview', { sessionId: registered }).catch(() => {}); if (active) { setError(error instanceof Error ? error.message : String(error)); setLoading(false); } });
    return () => { active = false; if (registered && registered !== visibleSession.current) void native('release_preview', { sessionId: registered }).catch(() => {}); };
  }, [pageId, jobId, draftId, runScripts, revision, retry]);
  // The old session remains alive until its replacement is painted and committed.
  useEffect(() => () => { if (descriptor) void native('release_preview', { sessionId: descriptor.sessionId }).catch(() => {}); }, [descriptor]);
  useLayoutEffect(() => {
    if (!visible) return;
    registeredVersion.current = { pageId: visible.pageId, revision: visible.revision };
    mounted.current = readyDocument.current;
    picked.current = null; invalidated.current = false;
    // Record the frame the canvas now shows before anything measures it. Measuring
    // after the promotion (not before it) keeps the box, handles and toolbar on the
    // element rather than on the pending frame's layout.
    shown.current = readyDocument.current && readyDocument.current.session === visible.descriptor.sessionId ? { pageId: visible.pageId, revision: visible.revision, session: visible.descriptor.sessionId, document: readyDocument.current.document } : null;
    if (staticView && readyDocument.current) { onHeight?.(readyDocument.current.height); }
    onReady?.(false, visible.revision);
    // Viewport-relative layout (for example 100vh) can settle one paint after the
    // promoted frame gets its own size, so measure again once that layout is final.
    // Skip it when a newer revision is already loading: that frame owns the selection.
    let active = true;
    const settled = requestAnimationFrame(() => requestAnimationFrame(() => { if (active && measurable()) onReady?.(true, visible.revision); }));
    return () => { active = false; cancelAnimationFrame(settled); };
  }, [visible]);
  useEffect(() => {
    if (!descriptor || staticView) return;
    reported.current = stagedProblems.current?.session === descriptor.sessionId ? stagedProblems.current.errors : [];
    if (reported.current.length) setObservations(reported.current);
    const settle = setTimeout(() => { if (reported.current?.length === 0) setObservations([]); }, 2000);
    return () => clearTimeout(settle);
  }, [descriptor, staticView]);
  useEffect(() => {
    if (staticView) return;
    function receive(event: MessageEvent) {
      const staging = !!pending && event.source === stagingFrame.current?.contentWindow;
      const source = staging ? pending?.descriptor : descriptor;
      if (!source || (!staging && event.source !== frame.current?.contentWindow) || !event.data || typeof event.data !== 'object' || Array.isArray(event.data)) return;
      const now = performance.now();
      if (now - rate.current.started > 1000) rate.current = { started: now, count: 0 };
      if (++rate.current.count > 10) return;
      const value = event.data as Record<string, unknown>;
      if (value.type === 'desegni-preview-link' && value.sessionId === source.sessionId && value.pageId === pageId &&
        typeof value.url === 'string' && /^https?:\/\//.test(value.url) && value.url.length <= 2048 && Object.keys(value).length === 4) { if (!staging) setOutsideLink(value.url); return; }
      if (value.type !== 'desegni-preview-error' || value.sessionId !== source.sessionId || value.pageId !== pageId ||
        typeof value.message !== 'string' || value.message.length > 4096 || Object.keys(value).length !== 4) return;
      const message = value.message;
      const fresh = (staging ? stagedProblems.current?.errors : reported.current) ?? [];
      const errors = fresh.includes(message) ? fresh : [...fresh.slice(-9), message];
      if (staging) stagedProblems.current = { session: source.sessionId, errors };
      else { reported.current = errors; setObservations(errors); }
    }
    window.addEventListener('message', receive);
    return () => window.removeEventListener('message', receive);
  }, [descriptor, pending, pageId, staticView]);
  useLayoutEffect(() => {
    if (!staticView || !mounted.current || invalidated.current) return;
    Document.prototype.getAnimations?.call(mounted.current.document).forEach(animation => animation.pause());
  }, [staticView, extent]);
  const frames = [visible, pending].filter((item): item is PreparedFrame => !!item);
  const visibleExtent = visible ? { pageId: visible.pageId, extent } : null;
  return <div style={staticView ? undefined : { position: 'relative' }} data-source-hash={descriptor?.sourceSha256 ?? undefined} data-preview-ready={staticView && !loading && !error && visible?.pageId === pageId && visible.revision === revision && readyDocument.current?.session === descriptor?.sessionId ? 'true' : undefined} className={staticView ? 'preview-frame static-preview' : 'preview-frame'}>
    {loading && !staticView && !visible && <p role="status" className="preview-status">Preparing preview…</p>}
    {error && <ProblemBanner message={userError(error)} dismiss={() => setError('')} action={!staticView ? { label: 'Retry preview', onClick: () => setRetry(value => value + 1) } : undefined} />}
    {frames.map(candidate => {
      const staging = candidate === pending;
      const sourceDocument = staticView && candidate.source ? staticDocument(candidate.source.html, staticBase(candidate.descriptor)) : undefined;
      // The hidden frame is laid out at the extent the visible frame already has, so a
      // viewport-relative page inside it keeps the layout the canvas is showing.
      const frameHeight = staging ? pendingFrameHeight(visibleExtent, candidate.pageId, referenceHeight) : extent;
      return <iframe key={candidate.descriptor.sessionId} ref={node => { if (staging) stagingFrame.current = node; else frame.current = node; }} inert={staging || staticView || !!draftId || undefined}
        src={sourceDocument ? undefined : candidate.descriptor.url} srcDoc={sourceDocument} sandbox={sourceDocument ? 'allow-same-origin' : candidate.runScripts ? 'allow-scripts' : ''} referrerPolicy="no-referrer" title={title}
        style={{ ...(sourceDocument ? { height: frameHeight } : {}), ...(staging ? { position: 'absolute', inset: 0, visibility: 'hidden', pointerEvents: 'none' } as CSSProperties : {}) }}
        tabIndex={staging || staticView || draftId ? -1 : undefined} aria-hidden={staging || staticView || undefined} onLoad={async event => {
          const node = event.currentTarget;
          // A later navigation of the editable document must never regain DOM access.
          if (!staging) {
            if (staticView && node.contentDocument !== mounted.current?.document) { invalidated.current = true; mounted.current = null; setError('This page left its editable preview. Reopen the page to select elements.'); }
            return;
          }
          if (pendingRef.current !== candidate) return;
          try {
            let prepared: typeof readyDocument.current = null;
            if (sourceDocument && candidate.source) {
              const doc = node.contentDocument;
              if (!doc?.head || !doc.body || doc.URL !== 'about:srcdoc' || doc.querySelector('meta[http-equiv="Content-Security-Policy"]')?.getAttribute('content') !== staticPolicy) throw new Error('This page left its editable preview. Reopen the page to select elements.');
              // Frames render in the still canvas but never gain scripting.
              hardenStaticFrames(doc);
              const style = Document.prototype.createElement.call(doc, 'style'); style.textContent = candidate.source.css; doc.head.append(style);
              const still = Document.prototype.createElement.call(doc, 'style'); still.setAttribute('data-desegnos-editor-paused', ''); still.textContent = '*,*::before,*::after{animation-play-state:paused!important;transition:none!important}'; doc.head.append(still);
              await doc.fonts.ready;
              // Styles can introduce images and fonts after the srcdoc load event.
              await Promise.allSettled(Array.from(doc.images, image => image.decode()));
              Document.prototype.getAnimations?.call(doc).forEach(animation => animation.pause());
              const content = () => Math.max(doc.body.scrollHeight, doc.documentElement.scrollHeight);
              const staged = pendingFrameHeight(visibleExtent, candidate.pageId, referenceHeight);
              let height = promotionHeight(referenceHeight, content());
              if (needsReferenceMeasure(staged, referenceHeight, content())) {
                // The frame is taller than the reference viewport and still overflows, so its
                // own height feeds the content. Decide the height where it cannot feed itself.
                node.style.height = `${referenceHeight}px`;
                await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
                height = promotionHeight(referenceHeight, content());
              }
              prepared = { document: doc, session: candidate.descriptor.sessionId, height };
              // The pending frame keeps this height through promotion, so the frame measured
              // here is the frame the canvas shows.
              node.style.height = `${height}px`;
            }
            // Keep both documents mounted across a paint opportunity. Promote in one
            // React commit; onReady then measures the visible document in layout effect.
            await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
            if (pendingRef.current !== candidate) return;
            readyDocument.current = prepared;
            // The promoted frame must keep its own height from the promotion commit on,
            // otherwise it is measured in a layout the canvas is not showing.
            if (prepared) setExtent(prepared.height);
            setVisible(candidate); setPending(null); setLoading(false);
          } catch (error) { if (pendingRef.current === candidate) { setError(error instanceof Error ? error.message : String(error)); setLoading(false); } }
        }} onError={() => { if (pendingRef.current === candidate) { setError('The preview document could not be loaded.'); setLoading(false); } }} />;
    })}
    {!staticView && outsideLink && <Notice variant="info" message={`This link leaves your site: ${outsideLink}`} action={{ label: 'Open in browser', onClick: () => { const url = outsideLink; setOutsideLink(null); void native('open_preview_link', { url }).catch(error => setError(String(error))); } }} dismiss={() => setOutsideLink(null)} />}
    {!onRuntimeProblems && !staticView && !error && observations.length > 0 && <ProblemBanner message={runtimeError(observations[0])}
      action={!draftId && !jobId && fixRuntimeErrors ? { label: 'Fix with AI', disabled: aiDisabled, onClick: () => fixRuntimeErrors(observations) } : undefined}
      dismiss={() => setObservations([])} />}
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
