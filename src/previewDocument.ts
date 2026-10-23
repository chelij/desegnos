// The static canvas renders the page in a same-origin srcdoc document so elements can be
// measured and picked. Frames inside it resolve against the session that served the page,
// so a relative iframe reaches the same site files the live preview uses.

/// Sources for the preview session's own files in every URL form the app serves, plus https
/// for pages that embed an external site.
const sessionSources = 'desegni-preview: http://*.desegni-preview.localhost http://desegni-preview.localhost';
/// Nested frames of an editable document render from `srcdoc`, which has no network URL.
const frameSources = `${sessionSources} https: about:`;

/// Static canvas documents never run scripts. Frames may still render: the session's own
/// files, an https embed, a same-origin srcdoc document, and nothing else.
export const staticPolicy = `default-src 'none'; script-src 'none'; script-src-attr 'none'; style-src 'unsafe-inline' https: http: data: desegni-preview:; img-src data: https: http: desegni-preview:; connect-src 'none'; font-src data: https: http: desegni-preview:; frame-src ${frameSources}; child-src ${frameSources}; object-src 'none'; base-uri ${sessionSources}; form-action 'none'; media-src 'none'; worker-src 'none'`;

/// The page's own session URL. A relative frame src in the imported page resolves against it,
/// so `../llama-wrap-preview.html` reaches the file that sits there in the imported site.
export function staticBase(descriptor: { website?: boolean; url: string } | null): string | null {
  return descriptor?.website ? descriptor.url : null;
}

function attribute(value: string) {
  return value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

export function staticDocument(html: string, base: string | null): string {
  const href = base ? `<base href="${attribute(base)}">` : '';
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${staticPolicy}"><meta name="viewport" content="width=device-width, initial-scale=1">${href}</head><body>${html}</body></html>`;
}

/// A framed document rendered inside the static canvas: scriptless, same-origin and with the
/// same policy as the page that frames it. An imported site file is a complete authored
/// document, so the policy, base and any separate CSS go into its own head instead of a
/// wrapper that would nest one document inside another.
export function staticFrameDocument(content: { html: string; css: string }, base: string | null): string {
  const href = base ? `<base href="${attribute(base)}">` : '';
  const head = `<meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${staticPolicy}"><meta name="viewport" content="width=device-width, initial-scale=1">${href}${content.css ? `<style data-desegnos-framed-css>${content.css}</style>` : ''}`;
  if (!/<html[\s>]/i.test(content.html) && !/<!doctype/i.test(content.html)) {
    return `<!doctype html><html><head>${head}</head><body>${content.html}</body></html>`;
  }
  const existing = /<head(\s[^>]*)?>/i.exec(content.html);
  if (existing) {
    const at = existing.index + existing[0].length;
    return content.html.slice(0, at) + head + content.html.slice(at);
  }
  const root = /<html(\s[^>]*)?>/i.exec(content.html);
  if (root) {
    const at = root.index + root[0].length;
    return `${content.html.slice(0, at)}<head>${head}</head>${content.html.slice(at)}`;
  }
  const doctype = /<!doctype[^>]*>/i.exec(content.html);
  if (doctype) {
    const at = doctype.index + doctype[0].length;
    return `${content.html.slice(0, at)}<head>${head}</head>${content.html.slice(at)}`;
  }
  return `<head>${head}</head>${content.html}`;
}

type FrameElement = { getAttribute(name: string): string | null; setAttribute(name: string, value: string): void };

/// The canvas frame is sandboxed without allow-scripts, so every document inside it inherits
/// that promise, and a framed site file is served with `script-src 'none'`. Author
/// `allow-scripts` is narrowed too, so a framed page cannot claim scripting back. Other
/// author restrictions stay, because the live overlay reads them to keep the same narrowing
/// when it runs the frame. Frames that ask for nothing keep their attributes: rewriting them
/// would reload the frame the canvas is about to show.
export function hardenStaticFrames(doc: { querySelectorAll(selector: string): ArrayLike<FrameElement> }) {
  for (const frame of Array.from(doc.querySelectorAll('iframe'))) {
    const sandbox = frame.getAttribute('sandbox');
    if (!sandbox?.split(/\s+/).includes('allow-scripts')) continue;
    const kept = sandbox.split(/\s+/).filter(token => token && token !== 'allow-scripts' && token !== 'allow-same-origin' &&
      token !== 'allow-popups-to-escape-sandbox' && !token.startsWith('allow-top-navigation'));
    frame.setAttribute('sandbox', ['allow-same-origin', ...new Set(kept)].join(' '));
  }
}
