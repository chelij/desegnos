import { staticFrameDocument } from './previewDocument.js';

export type RenderedSnapshot = { url?: string; html: string; base: string; frames: Array<{ index: number; snapshot: RenderedSnapshot }> };

// Only the isolated preview executes author code. Its DOM is untrusted input to a
// scriptless srcdoc, never an editor document or a source update.
export function freezeSnapshot(snapshot: RenderedSnapshot, depth = 0): string {
  if (depth > 16 || snapshot.html.length > 16 * 1024 * 1024) throw new Error('The rendered page is too large to edit.');
  const doc = new DOMParser().parseFromString(snapshot.html, 'text/html');
  doc.querySelectorAll('script,base,meta[http-equiv],object,embed').forEach(node => node.remove());
  doc.documentElement.setAttribute('data-desegnos-snapshot-url', snapshot.url ?? '');
  const frames = Array.from(doc.querySelectorAll('iframe'));
  for (const node of doc.querySelectorAll('*')) {
    for (const name of node.getAttributeNames()) {
      if (/^on/i.test(name) || name === 'autofocus') node.removeAttribute(name);
    }
  }
  frames.forEach((node, index) => {
    node.setAttribute('data-desegnos-frame-src', node.getAttribute('src') ?? '');
    const originalSource = node.getAttribute('src');
    node.removeAttribute('src'); node.removeAttribute('srcdoc');
    node.setAttribute('sandbox', 'allow-same-origin');
    const child = snapshot.frames.find(frame => frame.index === index);
    if (child) node.setAttribute('srcdoc', freezeSnapshot(child.snapshot, depth + 1));
    else {
      node.setAttribute('data-desegnos-snapshot-unavailable', '');
      if (originalSource) node.setAttribute('src', originalSource);
    }
  });
  const style = doc.createElement('style');
  style.setAttribute('data-desegnos-editor-paused', '');
  style.textContent = '*,*::before,*::after{animation-play-state:paused!important;transition:none!important;caret-color:transparent!important}';
  doc.head.append(style);
  return staticFrameDocument({ html: doc.documentElement.outerHTML, css: '' }, snapshot.base);
}

export function capturePreview(url: string, sessionId: string, host: HTMLElement, signal?: AbortSignal): Promise<RenderedSnapshot> {
  return new Promise((resolve, reject) => {
    const runner = document.createElement('iframe');
    runner.sandbox.add('allow-scripts', 'allow-forms');
    runner.referrerPolicy = 'no-referrer'; runner.tabIndex = -1;
    // The host is the page viewport, so viewport-relative styles and frame sizes match it.
    runner.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;border:0;visibility:hidden;pointer-events:none';
    const token = crypto.randomUUID();
    const cancel = () => { cleanup(); reject(new Error('Preview preparation cancelled.')); };
    const cleanup = () => { signal?.removeEventListener('abort', cancel); clearTimeout(timeout); window.removeEventListener('message', receive); runner.remove(); };
    const receive = (event: MessageEvent) => {
      if (event.source !== runner.contentWindow || event.data?.type !== 'desegnos-snapshot' || event.data?.sessionId !== sessionId || event.data?.token !== token) return;
      cleanup(); resolve(event.data.snapshot as RenderedSnapshot);
    };
    const timeout = setTimeout(() => { cleanup(); reject(new Error('The page did not finish preparing its frozen preview.')); }, 8000);
    window.addEventListener('message', receive);
    if (signal?.aborted) { cancel(); return; }
    signal?.addEventListener('abort', cancel, { once: true });
    runner.onload = () => runner.contentWindow?.postMessage({ type: 'desegnos-capture', sessionId, token }, '*');
    runner.src = url; host.append(runner);
  });
}

// Runtime insertions have no source identity. The bridge records ownership before
// serializing; a cloned ID does not inherit the original node's ownership.
export class SourceMapping {
  private sources = new WeakMap<Element, Element>();
  private targets = new WeakMap<Document, Map<string, Element>>();
  bind(rendered: Document, html: string, expectedUrl?: string) {
    for (const node of this.targets.get(rendered)?.values() ?? []) this.sources.delete(node);
    const targets = new Map<string, Element>(); this.targets.set(rendered, targets);
    if (expectedUrl) {
      try {
        const actual = new URL(rendered.documentElement.getAttribute('data-desegnos-snapshot-url') ?? '');
        const expected = new URL(expectedUrl);
        if (actual.protocol !== expected.protocol || actual.host !== expected.host || actual.pathname.replace(/\/index\.html?$/i, '/') !== expected.pathname.replace(/\/index\.html?$/i, '/')) return;
      } catch { return; }
    }
    const source = new DOMParser().parseFromString(html, 'text/html');
    const byId = new Map(Array.from(source.querySelectorAll('[data-desegnos-id]'), node => [node.getAttribute('data-desegnos-id'), node]));
    const seen = new Set<string>();
    for (const node of Array.from(rendered.querySelectorAll('[data-desegnos-snapshot-source]'))) {
      const key = node.getAttribute('data-desegnos-snapshot-source')!;
      let original: Element | null | undefined;
      try { original = key.startsWith('id:') ? byId.get(key.slice(3)) : key.startsWith('selector:') ? source.querySelector(key.slice(9)) : null; } catch { continue; }
      if (!original || original.localName !== node.localName || seen.has(key)) continue;
      seen.add(key);
      targets.set(key, node);
      this.sources.set(node, original);
    }
  }
  find(doc: Document, key: string): Element | null { return this.targets.get(doc)?.get(key) ?? null; }
  source(node: Element) { return this.sources.get(node) ?? null; }
  // Style commands historically return a complete inline style. Persist only the
  // command's delta on top of SOURCE style, never the runtime inline declarations.
  style(node: Element, before: string, after: string): string {
    const doc = node.ownerDocument;
    const old = doc.createElement('div').style; old.cssText = before;
    const next = doc.createElement('div').style; next.cssText = after;
    const result = doc.createElement('div').style; result.cssText = this.source(node)?.getAttribute('style') ?? '';
    for (const name of new Set([...Array.from(old), ...Array.from(next)])) {
      if (old.getPropertyValue(name) === next.getPropertyValue(name) && old.getPropertyPriority(name) === next.getPropertyPriority(name)) continue;
      if (next.getPropertyValue(name)) result.setProperty(name, next.getPropertyValue(name), next.getPropertyPriority(name));
      else result.removeProperty(name);
    }
    return result.cssText;
  }
  textEditable(node: Element) {
    const source = this.source(node);
    // Text indexes are defined by the source subtree. A script replacing or
    // rearranging that subtree makes inline editing ambiguous, so fail closed.
    const shape = (element: Element): unknown[] => Array.from(element.childNodes, child => child.nodeType === 3 ? ['text', child.textContent] : child.nodeType === 1 ? [(child as Element).localName, (child as Element).getAttribute('data-desegnos-id'), shape(child as Element)] : ['other']);
    return !!source && JSON.stringify(shape(source)) === JSON.stringify(shape(node));
  }
}
