import { Notice, ProblemBanner } from './ProblemBanner.js';
import { userError, runtimeError } from './userErrors.js';
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import { native } from './api';
import { siteLayerTree, siteLayerStyles, siteCopyStyles, type SiteLayer } from './siteLayers';
import { translatedStyle } from './elementEditing';
import { measurableDocument, needsReferenceMeasure, pendingFrameHeight, promotionHeight } from './previewMeasurement';
import { hardenStaticFrames, staticBase, staticDocument, staticFrameDocument, staticPolicy } from './previewDocument';
import { liveCommand, liveLocation, type LiveHistoryAction } from './liveBar';
import { collectFrame, frameKey, resolveFrameSource, type CollectedFrame } from './previewFrames';
import { frameAt, frameContentRect, frameDocumentBase, frameDocumentSource, frameMapping, innerRectToPage, pageRectToInnerViewport, type EditableFrame, type FrameMapping } from './frameDocuments';
import { documentKey } from './elementTargets';
import { similarSelections, type ElementProfile } from './elementSelection';
import { resizedElementSize, type ResizeCorner } from './canvasGeometry';
import type { DocumentTarget, ElementSelection, ElementTarget, ElementStyleEdit, FrameDocumentView, LayerDirection, ElementVisual, Page, PreviewDescriptor, Rect, SiteContent } from './types';

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
export type ConstrainedFrame = { node: HTMLIFrameElement; document: Document; target: DocumentTarget; hash: string; selector: string; liveSrc: string | null };
export type CanvasPreviewHandle = { liveHistory: (action: LiveHistoryAction) => void; frameOverlays: () => FrameView; hideStaticFrames: (keys: readonly string[]) => void; frames: () => EditableFrame[]; measuredElements: () => ElementSelection[]; previewImage: (target: ElementTarget, src: string) => boolean; layers: () => SiteLayer[]; layerStyles: (target: ElementTarget, direction: LayerDirection) => ElementStyleEdit[] | null; copyStyles: (target: ElementTarget) => ElementStyleEdit[]; source: () => string | null; sourceOf: (target: ElementTarget) => string | null; find: (target: ElementTarget) => ElementSelection | null; contains: (target: ElementTarget) => boolean; visual: (target: ElementTarget) => ElementVisual | null; elementsWithin: (rect: Rect) => ElementSelection[]; similar: (target: ElementTarget) => ElementSelection[]; textAt: (x: number, y: number) => InlineText | null; occupied: (target: ElementTarget) => Rect[]; beginMove: (target: ElementTarget) => ElementMover | null; beginResize: (target: ElementTarget, corner: ResizeCorner) => ElementResizer | null; pick: (x: number, y: number) => ElementSelection | null; imageAt: (x: number, y: number) => ElementSelection | null; foundSource: (target: ElementTarget) => string | null; found: () => ElementSelection[]; placeholders: () => ElementSelection[]; next: (parent?: boolean, target?: ElementTarget) => ElementSelection | null };
const identityMapping: FrameMapping = { frame: { x: 0, y: 0, width: 0, height: 0 }, scaleX: 1, scaleY: 1, insetX: 0, insetY: 0, scrollX: 0, scrollY: 0 };
function delay(ms: number) { return new Promise<void>(resolve => setTimeout(resolve, ms)); }
function paints(count: number) { return new Promise<void>(resolve => { let left = count; const step = () => { if (--left <= 0) resolve(); else requestAnimationFrame(step); }; requestAnimationFrame(step); }); }

export const PreviewFrame = forwardRef<CanvasPreviewHandle, {
  pageId: string; draftId?: string | null; jobId?: string | null; runScripts?: boolean; title: string; staticView?: boolean; revision?: number | string;
  onRuntimeProblems?: (errors: string[]) => void; fixRuntimeErrors?: (errors: string[]) => void; aiDisabled?: boolean; observationsVersion?: number;
  content?: SiteContent; onReady?: (settled: boolean, revision: number | string) => void; referenceHeight?: number; onHeight?: (height: number) => void;
  onLiveLocation?: (path: string) => void; frameDocuments?: FrameDocumentView[];
}>(function PreviewFrame({ pageId, jobId = null, draftId = null, runScripts = false, title, staticView = false, revision = 0, content, referenceHeight = 900, onHeight, onReady, onRuntimeProblems, fixRuntimeErrors, aiDisabled, observationsVersion, onLiveLocation, frameDocuments = [] }, ref) {
  const frame = useRef<HTMLIFrameElement>(null);
  const stagingFrame = useRef<HTMLIFrameElement>(null);
  const [extent, setExtent] = useState(referenceHeight);
  type PreparedFrame = { descriptor: PreviewDescriptor; source: SiteContent | null; pageId: string; revision: number | string; runScripts: boolean; frames: ConstrainedFrame[] };
  const [visible, setVisible] = useState<PreparedFrame | null>(null);
  const descriptor = visible?.descriptor ?? null;
  const [pending, setPending] = useState<PreparedFrame | null>(null);
  const pendingRef = useRef(pending); pendingRef.current = pending;
  const visibleSession = useRef<string | null>(null); visibleSession.current = descriptor?.sessionId ?? null;
  const readyDocument = useRef<{ document: Document; session: string; height: number; frames: ConstrainedFrame[] } | null>(null);
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
  // Editable framed documents living in the shown page document.
  const shownFrames = useRef<{ outer: Document; frames: ConstrainedFrame[] } | null>(null);
  const frameDocumentsKey = frameDocuments.map(item => `${item.iframeId}:${item.sourceSha256}:${item.src}`).join('|');
  const frameDocumentsRef = useRef(frameDocuments); frameDocumentsRef.current = frameDocuments;
  // Only documents fetched for this page may render here; a page switch must not reuse the last page's.
  const usableFrames = (documents: FrameDocumentView[]) => documents.filter(spec => !spec.pageId || spec.pageId === pageId);
  const frameTicket = useRef(0);
  const onReadyRef = useRef(onReady); onReadyRef.current = onReady;
  const invalidated = useRef(false);
  // The inline opacity we replaced while a static frame sits under its live overlay.
  const previousOpacity = useRef(new WeakMap<Element, string>()).current;
  const registeredVersion = useRef<{ pageId: string; revision: number | string } | null>(null);
  // Canvas documents share DOM access only while all scripts remain forbidden by sandbox.
  // Interactive previews keep their separate opaque-origin, explicitly enabled sandbox.
  function measurable(): Document | null {
    return measurableDocument(shown.current, { pageId, revision, session: descriptor?.sessionId ?? '', document: frame.current?.contentDocument ?? null, invalidated: invalidated.current });
  }
  // The framed documents the canvas can measure right now. A framed document only counts
  // while its iframe still belongs to the promoted page and still holds that document.
  function frameEntries(): ConstrainedFrame[] {
    const holder = shownFrames.current;
    const outer = measurable();
    if (!holder || !outer || holder.outer !== outer || invalidated.current) return [];
    return holder.frames.filter(entry => entry.node.isConnected && entry.node.ownerDocument === outer && entry.node.contentDocument === entry.document);
  }
  function frameForNode(node: Element): ConstrainedFrame | null {
    return frameEntries().find(entry => entry.document === node.ownerDocument) ?? null;
  }
  // A frame the page hides neither shows its content nor takes clicks.
  function frameShown(entry: ConstrainedFrame): boolean {
    const style = computed(entry.node);
    return style.display !== 'none' && style.visibility !== 'hidden' && style.visibility !== 'collapse' && entry.node.getClientRects().length > 0;
  }
  function mappingFor(entry: ConstrainedFrame): FrameMapping {
    const node = entry.node;
    const box = Element.prototype.getBoundingClientRect.call(node);
    const style = computed(node);
    const px = (value: string) => Number.parseFloat(value) || 0;
    const view = entry.document.defaultView;
    return frameMapping({
      frame: { x: box.x, y: box.y, width: box.width, height: box.height },
      layoutWidth: node.offsetWidth, layoutHeight: node.offsetHeight,
      borderLeft: px(style.borderLeftWidth), borderTop: px(style.borderTopWidth),
      paddingLeft: px(style.paddingLeft), paddingTop: px(style.paddingTop),
      scrollX: view?.scrollX ?? 0, scrollY: view?.scrollY ?? 0,
    });
  }
  function computed(node: Element, pseudo?: string | null): CSSStyleDeclaration {
    const view = node.ownerDocument.defaultView;
    return view ? view.getComputedStyle(node, pseudo ?? null) : getComputedStyle(node, pseudo ?? null);
  }
  function structuralSelector(node: Element, body: Element): string | null {
    const parts: string[] = [];
    for (let current: Element | null = node; current && current !== body; current = current.parentElement) {
      if (!/^[A-Za-z][A-Za-z0-9-]{0,79}$/.test(current.localName) || parts.length >= 63) return null;
      const siblings: Element[] = Array.from(current.parentElement?.children ?? []);
      parts.unshift(`${current.localName}:nth-of-type(${siblings.filter(sibling => sibling.localName === current!.localName).indexOf(current) + 1})`);
    }
    return ['body', ...parts].join(' > ');
  }
  // Peers of a similar control are matched by tag and class list, or by structural role.
  function profileOf(node: Element): ElementProfile {
    const role = /^h[1-6]$/.test(node.localName) ? node.localName : node.localName === 'a' ? 'link'
      : ['button', 'summary'].includes(node.localName) ? node.localName : ['input', 'select', 'textarea'].includes(node.localName) ? 'field' : '';
    return { tag: node.localName, classes: Array.from(node.classList).sort(), role };
  }
  function elementCandidates(container: Document): Element[] {
    if (!container.body) return [];
    return Array.from(container.body.querySelectorAll('*')).filter(node => !['script', 'style', 'link', 'meta', 'template', 'noscript', 'source', 'track'].includes(node.localName) && node.getClientRects().length > 0);
  }
  function identify(node: Element | null, exact = true, withLabel = true): { node: Element; target: ElementTarget } | null {
    const doc = measurable();
    if (!doc || doc.URL !== 'about:srcdoc' || !node) return null;
    const framed = frameForNode(node);
    const container = framed ? framed.document : doc;
    const hash = framed ? framed.hash : descriptor?.sourceSha256;
    if (!container.body?.contains(node) || !hash) return null;
    if (!exact) node = Element.prototype.closest.call(node, 'button, a, input, select, textarea, label, summary') ?? node;
    if (!container.body.contains(node) || !/^[A-Za-z][A-Za-z0-9-]{0,79}$/.test(node.localName)) return null;
    const selector = structuralSelector(node, container.body); if (!selector) return null;
    let label = '';
    if (withLabel) {
      const walker = Document.prototype.createTreeWalker.call(container, node, NodeFilter.SHOW_TEXT);
      let visits = 0;
      while (label.length < 240 && visits++ < 1024) {
        const text = walker.nextNode(); if (!text) break;
        if (!['SCRIPT', 'STYLE'].includes(text.parentElement?.tagName ?? '')) label += ` ${text.nodeValue?.slice(0, 240) ?? ''}`;
      }
      label = node.getAttribute('aria-label') || label.trim() || node.getAttribute('alt') || node.getAttribute('title') || '';
    }
    const id = node.getAttribute('data-desegnos-id');
    return { node, target: { selector, tag: node.localName,
      label: Array.from(label.replace(/\s+/g, ' ').trim()).slice(0, 240).join(''), ...(id ? { id } : { sourceSha256: hash }), ...(framed ? { document: framed.target } : {}) } };
  }
  function selected(node: Element | null, remember = false, exact = false, withLabel = true): ElementSelection | null {
    const found = identify(node, exact, withLabel); if (!found) return null;
    const box = Element.prototype.getBoundingClientRect.call(found.node);
    if (![box.x, box.y, box.width, box.height, box.right, box.bottom].every(Number.isFinite) || box.width < 1 || box.height < 1) return null;
    const framed = frameForNode(found.node);
    const rect = framed ? innerRectToPage(mappingFor(framed), { x: box.x, y: box.y, width: box.width, height: box.height }) : { x: box.x, y: box.y, width: box.width, height: box.height };
    if (remember) picked.current = found.node;
    return { element: found.target, rect };
  }

  function findNode(target: ElementTarget): Element | null {
    const doc = measurable();
    if (!doc || doc.URL !== 'about:srcdoc') return null;
    let container: Document = doc; let hash = descriptor?.sourceSha256;
    if (target.document) {
      const key = documentKey(target.document);
      const entry = frameEntries().find(item => documentKey(item.target) === key);
      if (!entry) return null;
      container = entry.document; hash = entry.hash;
    }
    if (!target.id && target.sourceSha256 !== hash) return null;
    try { const node = Document.prototype.querySelector.call(container, target.id ? `[data-desegnos-id="${CSS.escape(target.id)}"]` : target.selector); return node && (target.id || node.localName === target.tag) ? node : null; } catch { return null; }
  }
  function visual(target: ElementTarget): ElementVisual | null {
    const node = findNode(target); if (!node) return null;
    const style = computed(node);
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
  function hitAt(x: number, y: number): { frame: ConstrainedFrame | null; node: Element | null; point: { x: number; y: number } } {
    const doc = measurable();
    const frames = frameEntries().filter(frameShown);
    const hit = frameAt(frames.map(entry => ({ mapping: mappingFor(entry), viewport: frameViewport(entry) })), { x, y });
    if (hit) {
      const entry = frames[hit.index];
      try { return { frame: entry, node: entry.document.elementFromPoint(hit.point.x, hit.point.y), point: hit.point }; } catch { return { frame: entry, node: null, point: hit.point }; }
    }
    return { frame: null, node: doc?.elementFromPoint(x, y) ?? null, point: { x, y } };
  }
  function frameViewport(entry: ConstrainedFrame): { width: number; height: number } {
    const root = entry.document.documentElement;
    return { width: root?.clientWidth ?? 0, height: root?.clientHeight ?? 0 };
  }
  function textAt(x: number, y: number): InlineText | null {
    const hit = hitAt(x, y);
    const selection = selected(hit.node, true);
    if (!hit.node || !selection || ['input', 'textarea', 'select', 'svg', 'body'].includes(selection.element.tag)) return null;
    const node = findNode(selection.element); if (!node) return null;
    const doc = node.ownerDocument; const point = hit.frame ? hit.point : { x, y };
    const nodes = textNodes(node);
    for (let index = 0; index < nodes.length; index++) {
      const text = nodes[index]; if (!text.data.trim()) continue;
      const range = doc.createRange(); range.selectNodeContents(text);
      if (!Array.from(range.getClientRects()).some(rect => point.x >= rect.left && point.x <= rect.right && point.y >= rect.top && point.y <= rect.bottom)) continue;
      const box = range.getBoundingClientRect(); const style = computed(text.parentElement!);
      let backgroundColor = '#ffffff';
      for (let parent = text.parentElement; parent; parent = parent.parentElement) {
        const value = computed(parent).backgroundColor;
        if (value !== 'transparent' && !/^rgba\([^)]*,\s*0(?:\.0+)?\)$/.test(value)) { backgroundColor = value; break; }
      }
      const rect = hit.frame ? innerRectToPage(mappingFor(hit.frame), { x: box.x, y: box.y, width: box.width, height: box.height }) : { x: box.x, y: box.y, width: box.width, height: box.height };
      return { selection, index, text: text.data, rect, style: {
        fontFamily: style.fontFamily, fontSize: style.fontSize, fontWeight: style.fontWeight, fontStyle: style.fontStyle,
        lineHeight: style.lineHeight, letterSpacing: style.letterSpacing, textAlign: style.textAlign as CSSProperties['textAlign'],
        textTransform: style.textTransform, textDecoration: style.textDecoration, direction: style.direction as 'ltr' | 'rtl', color: style.color, backgroundColor,
      } };
    }
    return null;
  }
  function occupied(target: ElementTarget): Rect[] {
    const node = findNode(target); if (!node) return [];
    const doc = node.ownerDocument; const framed = frameForNode(node);
    const map = (rect: { x: number; y: number; width: number; height: number }): Rect => framed ? innerRectToPage(mappingFor(framed), rect) : rect;
    const result: Rect[] = [];
    // Text and replaced controls represent content, rather than their large
    // layout containers. Ignore the selected subtree and its own ancestor text.
    const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
    let current: Node | null; let visited = 0;
    while ((current = walker.nextNode()) && visited++ < 8192) {
      if (node.contains(current)) continue;
      if (current.nodeType === Node.TEXT_NODE && current.textContent?.trim() && !current.parentElement?.closest('script,style,noscript,template,svg')) {
        const range = doc.createRange(); range.selectNodeContents(current);
        for (const box of range.getClientRects()) if (box.width && box.height) result.push(map({ x: box.x, y: box.y, width: box.width, height: box.height }));
      } else if (current instanceof doc.defaultView!.Element && current.matches('img,input,textarea,select,video,canvas,svg')) {
        const box = current.getBoundingClientRect(); if (box.width && box.height) result.push(map({ x: box.x, y: box.y, width: box.width, height: box.height }));
      }
    }
    return result;
  }
  function transformEditor(target: ElementTarget) {
    const node = findNode(target); const values = visual(target);
    if (!node || !values || target.selector === 'body') return null;
    for (let ancestor: Element | null = node; ancestor; ancestor = ancestor.parentElement) {
      const css = computed(ancestor);
      if (css.perspective !== 'none' || css.transform.startsWith('matrix3d(')) return null;
    }
    const owner = node.ownerDocument;
    const original = Element.prototype.getAttribute.call(node, 'style');
    const restore = () => { if (original === null) Element.prototype.removeAttribute.call(node, 'style'); else Element.prototype.setAttribute.call(node, 'style', original); };
    const set = (css: string) => { const style = owner.createElement('div').style; style.cssText = css; style.setProperty('transition', 'none', 'important'); Element.prototype.setAttribute.call(node, 'style', style.cssText); };
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
    // The Live bar's history controls reach the bridge in this session's document; a
    // command for another session is ignored there.
    liveHistory(action) {
      const target = frame.current?.contentWindow;
      if (!runScripts || !descriptor || !target) return;
      target.postMessage(liveCommand(descriptor.sessionId, action), '*');
    },
    // The frames the canvas can show live, read from the promoted document. A frame the
    // canvas hides under its overlay keeps reporting itself so the overlay stays put.
    frameOverlays(): FrameView {
      const doc = measurable();
      if (!doc || doc.URL !== 'about:srcdoc') return { pageUrl: null, frames: [] };
      const pageUrl = descriptor?.url ?? null;
      const entries = frameEntries();
      const frames: CollectedFrame[] = [];
      Array.from(doc.querySelectorAll('iframe')).forEach((node, index) => {
        const box = node.getBoundingClientRect();
        const style = computed(node);
        const entry = entries.find(item => item.node === node);
        const frame = collectFrame({
          rect: { x: box.x, y: box.y, width: box.width, height: box.height },
          display: style.display,
          visibility: style.visibility,
          src: entry?.liveSrc ?? node.getAttribute('data-desegnos-frame-src') ?? node.getAttribute('src'), sandbox: node.getAttribute('sandbox'), allow: node.getAttribute('allow'),
          allowFullscreen: node.hasAttribute('allowfullscreen'),
        }, pageUrl, index);
        if (frame) frames.push({ ...frame, selector: identify(node, true)?.target.selector ?? null, editable: !!entry });
      });
      return { pageUrl, frames };
    },
    hideStaticFrames(keys) {
      const doc = measurable();
      if (!doc || doc.URL !== 'about:srcdoc') return;
      const wanted = new Set(keys);
      const pageUrl = descriptor?.url ?? null;
      const entries = frameEntries();
      Array.from(doc.querySelectorAll('iframe')).forEach((node, index) => {
        const entry = entries.find(item => item.node === node);
        const key = frameKey(index, resolveFrameSource(entry?.liveSrc ?? node.getAttribute('data-desegnos-frame-src') ?? node.getAttribute('src'), pageUrl));
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
    // The editable framed documents, with their page geometry, for selection and requests.
    frames() {
      return frameEntries().filter(frameShown).flatMap(entry => {
        const found = identify(entry.node, true);
        if (!found) return [];
        const mapping = mappingFor(entry);
        const viewport = frameViewport(entry);
        return [{ iframe: found.target, selector: found.target.selector, document: entry.target, sourceSha256: entry.hash, mapping, viewport, contentRect: frameContentRect({ mapping, viewport }) }];
      });
    },
    previewImage(target, src) {
      const node = findNode(target);
      if (!node || node.localName !== 'img') return false;
      node.removeAttribute('srcset'); node.removeAttribute('sizes'); node.setAttribute('src', src);
      return true;
    },
    source() { return measurable() ? descriptor?.sourceSha256 ?? null : null; },
    // The fingerprint of the document a target lives in: edits re-render only that frame.
    sourceOf(target) {
      if (!measurable()) return null;
      if (target.document) {
        const entry = frameEntries().find(item => documentKey(item.target) === documentKey(target.document));
        return entry?.hash ?? null;
      }
      return descriptor?.sourceSha256 ?? null;
    },
    find(target) { return selected(findNode(target), false, true); },
    contains(target) { return !!findNode(target); },
    layers() {
      const doc = measurable();
      if (!doc || !identify(doc.body)) return [];
      const entries = frameEntries();
      const treeFor = (target: Document): SiteLayer[] => siteLayerTree(target, node => selected(node, false, true));
      const framed = (target: Document, items: SiteLayer[]): SiteLayer[] => items.map(item => {
        const children = framed(target, item.children);
        if (item.element.tag !== 'iframe') return { ...item, children };
        const entry = entries.find(value => value.selector === item.element.selector && value.node.ownerDocument === target);
        return entry ? { ...item, children: framed(entry.document, treeFor(entry.document)) } : { ...item, children };
      });
      return framed(doc, treeFor(doc));
    },
    measuredElements() {
      const doc = measurable();
      if (!doc || !identify(doc.body)) return [];
      const nodes = Array.from(doc.querySelectorAll('[data-desegnos-id]'));
      if (nodes.length > 10_000) throw new Error('This page has more than 10,000 elements. Select individual elements for this request.');
      return nodes.map(node => selected(node, false, true)).filter((item): item is ElementSelection => !!item);
    },
    // Every measurable element the marquee can reach, including editable framed documents.
    elementsWithin(rect) {
      const doc = measurable();
      if (!doc || !identify(doc.body)) return [];
      const nodes = [doc, ...frameEntries().filter(frameShown).map(entry => entry.document)].flatMap(elementCandidates)
        // A marquee over a frame reaches the document inside it, not the wrapper and its contents.
        .filter(node => node.localName !== 'iframe');
      if (nodes.length > 20_000) throw new Error('This page has more than 20,000 elements. Select a smaller area.');
      return nodes.flatMap(node => {
        const found = selected(node, false, true, false);
        return found && found.rect.x < rect.x + rect.width && found.rect.x + found.rect.width > rect.x && found.rect.y < rect.y + rect.height && found.rect.y + found.rect.height > rect.y ? [found] : [];
      });
    },
    // Elements matching the reference's tag and class list, or its structural role.
    similar(target) {
      const node = findNode(target);
      const container = node?.ownerDocument;
      if (!node || !container?.body) return [];
      const nodes = elementCandidates(container);
      if (nodes.length > 20_000) throw new Error('This page has more than 20,000 elements. Select a smaller group.');
      const profile = profileOf(node);
      const items = nodes.flatMap(candidate => {
        const selection = selected(candidate, false, true, false);
        return selection ? [{ selection, profile: profileOf(candidate) }] : [];
      });
      return similarSelections(profile, items);
    },
    layerStyles(target, direction) { const node = findNode(target); if (!node) throw new Error('The page changed. Select this layer again.'); return siteLayerStyles(node, direction, value => identify(value)?.target ?? null); },
    copyStyles(target) { const node = findNode(target); if (!node) throw new Error('The page changed. Select this layer again.'); return siteCopyStyles(node, value => identify(value)?.target ?? null); },
    visual,
    textAt,
    occupied,
    beginMove(target) {
      const edit = transformEditor(target); if (!edit) return null;
      // Canvas deltas are page pixels; the style lands in the frame's own pixels.
      const framed = frameForNode(edit.node);
      const scale = framed ? mappingFor(framed) : { scaleX: 1, scaleY: 1 };
      const style = (x: number, y: number) => edit.offset(edit.original, x / scale.scaleX, y / scale.scaleY);
      return { style, cancel: edit.restore, preview(x, y) { if (!findNode(target)) return null; edit.set(style(x, y)); return selected(edit.node, false, true); } };
    },
    beginResize(target, corner) {
      const edit = transformEditor(target); if (!edit) return null;
      const { node, original, origin, restore, set, offset } = edit;
      if (node.namespaceURI !== 'http://www.w3.org/1999/xhtml' && node.localName !== 'svg') return null;
      const owner = node.ownerDocument;
      const elementStyle = computed(node);
      const number = (value: string) => Number.parseFloat(value) || 0;
      const paddingX = number(elementStyle.paddingLeft) + number(elementStyle.paddingRight) + number(elementStyle.borderLeftWidth) + number(elementStyle.borderRightWidth);
      const paddingY = number(elementStyle.paddingTop) + number(elementStyle.paddingBottom) + number(elementStyle.borderTopWidth) + number(elementStyle.borderBottomWidth);
      const localWidth = elementStyle.width === 'auto' ? Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetWidth')?.get?.call(node) ?? 0 : number(elementStyle.width) + (elementStyle.boxSizing === 'border-box' ? 0 : paddingX);
      const localHeight = elementStyle.height === 'auto' ? Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight')?.get?.call(node) ?? 0 : number(elementStyle.height) + (elementStyle.boxSizing === 'border-box' ? 0 : paddingY);
      if (localWidth <= 0 || localHeight <= 0) return null;
      const base = owner.createElement('div').style; base.cssText = original;
      if (elementStyle.display === 'inline') base.setProperty('display', 'inline-block', 'important');
      for (const [name, value] of Object.entries({ 'box-sizing':'border-box', 'min-width':'0px', 'min-height':'0px', 'max-width':'none', 'max-height':'none', 'flex-grow':'0', 'flex-shrink':'0', 'flex-basis':'auto' })) base.setProperty(name,value,'important');
      const sized = (width: number, height: number) => {
        const css = owner.createElement('div').style; css.cssText = base.cssText;
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
      const framed = frameForNode(node); const mapping = framed ? mappingFor(framed) : null;
      const style = (rect: Rect) => {
        const inner = mapping ? pageRectToInnerViewport(mapping, rect) : rect;
        const dw=inner.width-initial.width; const dh=inner.height-initial.height;
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
    pick(x, y) { try { const hit = hitAt(x, y); const found = selected(hit.node, true); if (found && !(hit.frame && found.element.tag === 'body')) return found; return selected(measurable()?.elementFromPoint(x, y) ?? null, true); } catch { return null; } },
    // The picture under a point, even inside a link or button, for dropping image files.
    imageAt(x, y) {
      try {
        const hit = hitAt(x, y);
        const node = hit.node;
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
        if (parent && current) {
          const up = current.parentElement;
          if (up && up.localName !== 'html') { const hit = selected(up, true); if (hit) return hit; }
          // The parent of a framed document's root is the iframe that shows it.
          const framed = frameForNode(current);
          return framed ? selected(framed.node, true) : null;
        }
        const scope = current?.ownerDocument ?? doc;
        const elements = [...new Set([...scope.body.querySelectorAll('*')].filter(node => !['SCRIPT', 'STYLE', 'META', 'LINK'].includes(node.tagName) && node.getBoundingClientRect().width >= 1 && node.getBoundingClientRect().height >= 1).map(node => Element.prototype.closest.call(node, 'button, a, input, select, textarea, label, summary') ?? node))];
        return selected(elements[(elements.indexOf(current!) + 1) % elements.length] ?? scope.body, true);
      } catch { return null; }
    },
  }));
  type FrameMatch = { node: HTMLIFrameElement; spec: FrameDocumentView };
  function matchFrames(nodes: HTMLIFrameElement[], documents: FrameDocumentView[]): FrameMatch[] {
    const used = new Set<number>();
    const matches: FrameMatch[] = [];
    const authored = (node: HTMLIFrameElement) => node.getAttribute('data-desegnos-frame-src') ?? node.getAttribute('src') ?? '';
    for (const spec of documents) {
      let index = spec.iframeId ? nodes.findIndex((node, at) => !used.has(at) && node.getAttribute('data-desegnos-id') === spec.iframeId) : -1;
      if (index < 0) index = nodes.findIndex((node, at) => !used.has(at) && authored(node) === spec.src);
      if (index < 0) continue;
      used.add(index);
      matches.push({ node: nodes[index], spec });
    }
    return matches;
  }
  async function settleInner(doc: Document) {
    try { await Promise.race([doc.fonts.ready, delay(2000)]); } catch { /* fonts may never settle */ }
    await Promise.race([Promise.allSettled(Array.from(doc.images, image => image.decode())), delay(2000)]);
    Document.prototype.getAnimations?.call(doc).forEach(animation => animation.pause());
    await paints(2);
  }
  function setLiveSource(node: HTMLIFrameElement, spec: FrameDocumentView, descriptor: PreviewDescriptor) {
    node.dataset.desegnosFrameLiveSrc = frameDocumentSource(descriptor, spec.document.kind === 'page' ? spec.document.pageId : null, spec.path ?? null) ?? '';
  }
  // Render framed documents as scriptless, same-origin srcdoc while the page is still hidden.
  async function prepareFrames(doc: Document, specs: FrameDocumentView[], descriptor: PreviewDescriptor): Promise<ConstrainedFrame[]> {
    const nodes = Array.from(doc.querySelectorAll('iframe')) as HTMLIFrameElement[];
    const matches = matchFrames(nodes, specs);
    await Promise.all(matches.map(async ({ node, spec }) => {
      node.setAttribute('data-desegnos-frame-src', node.getAttribute('data-desegnos-frame-src') ?? node.getAttribute('src') ?? '');
      node.removeAttribute('src');
      node.setAttribute('sandbox', 'allow-same-origin');
      setLiveSource(node, spec, descriptor);
      const loaded = new Promise<void>(resolve => node.addEventListener('load', () => resolve(), { once: true }));
      node.setAttribute('srcdoc', staticFrameDocument(spec.content, frameDocumentBase(descriptor, spec.path ?? null)));
      await Promise.race([loaded, delay(4000)]);
      const inner = node.contentDocument;
      if (!inner || inner.URL !== 'about:srcdoc') return;
      node.dataset.desegnosFrameHash = spec.sourceSha256;
      await settleInner(inner);
    }));
    return matches.flatMap(({ node, spec }) => {
      const inner = node.contentDocument;
      if (!inner || inner.URL !== 'about:srcdoc') return [];
      return [{ node, document: inner, target: spec.document, hash: spec.sourceSha256, selector: structuralSelector(node, doc.body) ?? '', liveSrc: node.dataset.desegnosFrameLiveSrc || null }];
    });
  }
  // Replace one framed document in place, double-buffered: a hidden fixed copy loads over the
  // frame, then the frame reloads behind it and the copy steps aside. The frame's box, and so
  // the canvas selection, never moves and nothing flashes.
  async function swapFrame(doc: Document, node: HTMLIFrameElement, spec: FrameDocumentView, descriptor: PreviewDescriptor, current: () => boolean): Promise<boolean> {
    const html = staticFrameDocument(spec.content, frameDocumentBase(descriptor, spec.path ?? null));
    const box = Element.prototype.getBoundingClientRect.call(node);
    const style = computed(node);
    const number = (value: string) => Number.parseFloat(value) || 0;
    const scaleX = node.offsetWidth ? box.width / node.offsetWidth : 1;
    const scaleY = node.offsetHeight ? box.height / node.offsetHeight : 1;
    // The copy must cover the frame's content box, not its border and padding.
    const insetX = number(style.borderLeftWidth) + number(style.paddingLeft);
    const insetY = number(style.borderTopWidth) + number(style.paddingTop);
    const width = Math.max(1, node.clientWidth - number(style.paddingLeft) - number(style.paddingRight));
    const height = Math.max(1, node.clientHeight - number(style.paddingTop) - number(style.paddingBottom));
    const staging = doc.createElement('iframe') as HTMLIFrameElement;
    staging.setAttribute('sandbox', 'allow-same-origin');
    staging.setAttribute('aria-hidden', 'true');
    staging.setAttribute('tabindex', '-1');
    staging.setAttribute('referrerpolicy', 'no-referrer');
    const css = staging.style;
    css.position = 'fixed'; css.left = `${box.left + insetX * scaleX}px`; css.top = `${box.top + insetY * scaleY}px`;
    css.width = `${width}px`; css.height = `${height}px`;
    if (Math.abs(scaleX - 1) > 1e-4 || Math.abs(scaleY - 1) > 1e-4) { css.transform = `scale(${scaleX}, ${scaleY})`; css.transformOrigin = 'top left'; }
    css.border = '0'; css.margin = '0'; css.padding = '0'; css.pointerEvents = 'none';
    css.visibility = 'hidden'; css.zIndex = '2147483647'; css.background = '#fff';
    const loaded = new Promise<void>(resolve => staging.addEventListener('load', () => resolve(), { once: true }));
    doc.documentElement.append(staging);
    staging.setAttribute('srcdoc', html);
    await Promise.race([loaded, delay(4000)]);
    const fresh = staging.contentDocument;
    if (fresh) await settleInner(fresh);
    await paints(2);
    if (!current()) { staging.remove(); return false; }
    const previousVisibility = node.style.getPropertyValue('visibility');
    const previousPriority = node.style.getPropertyPriority('visibility');
    staging.style.visibility = 'visible';
    node.style.setProperty('visibility', 'hidden');
    const reloaded = new Promise<void>(resolve => node.addEventListener('load', () => resolve(), { once: true }));
    node.setAttribute('srcdoc', html);
    await Promise.race([reloaded, delay(4000)]);
    if (node.contentDocument) await settleInner(node.contentDocument);
    staging.remove();
    if (previousVisibility) node.style.setProperty('visibility', previousVisibility, previousPriority); else node.style.removeProperty('visibility');
    const inner = node.contentDocument;
    return !!inner && inner.URL === 'about:srcdoc';
  }
  // A framed document that changed on disk re-renders alone; the page around it stays.
  useEffect(() => {
    if (!staticView || !frameDocumentsKey) return;
    const doc = measurable();
    if (!doc || !descriptor) return;
    const ticket = ++frameTicket.current;
    const current = () => ticket === frameTicket.current && measurable() === doc;
    void (async () => {
      const entries = frameEntries();
      for (const { node, spec } of matchFrames(Array.from(doc.querySelectorAll('iframe')) as HTMLIFrameElement[], usableFrames(frameDocumentsRef.current))) {
        if (!current()) return;
        if (node.dataset.desegnosFrameHash === spec.sourceSha256) continue;
        const swapped = await swapFrame(doc, node, spec, descriptor, current).catch(() => false);
        if (!swapped || !current()) continue;
        node.dataset.desegnosFrameHash = spec.sourceSha256;
        setLiveSource(node, spec, descriptor);
        const holder = shownFrames.current;
        if (holder && holder.outer === doc && node.contentDocument) {
          const existing = holder.frames.find(item => item.node === node);
          const selector = structuralSelector(node, doc.body) ?? existing?.selector ?? '';
          const entry: ConstrainedFrame = { node, document: node.contentDocument, target: spec.document, hash: spec.sourceSha256, selector, liveSrc: node.dataset.desegnosFrameLiveSrc || null };
          if (existing) Object.assign(existing, entry); else if (entries.length < 64) holder.frames.push(entry);
        }
      }
      if (current()) onReadyRef.current?.(true, revision);
    })().catch(() => {});
  }, [frameDocumentsKey, visible, staticView]);
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
        setPending({ descriptor: value, source: value.content ?? content ?? null, pageId, revision, runScripts, frames: [] });
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
    shownFrames.current = readyDocument.current && readyDocument.current.session === visible.descriptor.sessionId ? { outer: readyDocument.current.document, frames: readyDocument.current.frames } : null;
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
      const path = liveLocation(value, source.sessionId);
      if (path) { onLiveLocation?.(path); return; }
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
  }, [descriptor, pending, pageId, staticView, onLiveLocation]);
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
        src={sourceDocument ? undefined : candidate.descriptor.url} srcDoc={sourceDocument} sandbox={sourceDocument ? 'allow-same-origin' : candidate.runScripts ? 'allow-scripts allow-forms' : ''} referrerPolicy="no-referrer" title={title}
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
              // Editable framed documents replace their network frames before anything measures.
              const framedDocuments = await prepareFrames(doc, usableFrames(frameDocumentsRef.current), candidate.descriptor);
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
              prepared = { document: doc, session: candidate.descriptor.sessionId, height, frames: framedDocuments };
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
