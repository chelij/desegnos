// The static canvas renders the page in a same-origin srcdoc document so elements can be
// measured and picked. Frames inside it resolve against the session that served the page,
// so a relative iframe reaches the same site files the live preview uses.

/// Sources for the preview session's own files in every URL form the app serves, plus https
/// for pages that embed an external site.
const sessionSources = 'desegni-preview: http://*.desegni-preview.localhost http://desegni-preview.localhost';

/// Static canvas documents never run scripts. Frames may still render: the session's own
/// files, an https embed, and nothing else.
export const staticPolicy = `default-src 'none'; script-src 'none'; script-src-attr 'none'; style-src 'unsafe-inline' https: http: data: desegni-preview:; img-src data: https: http: desegni-preview:; connect-src 'none'; font-src data: https: http: desegni-preview:; frame-src ${sessionSources} https:; child-src ${sessionSources} https:; object-src 'none'; base-uri ${sessionSources}; form-action 'none'; media-src 'none'; worker-src 'none'`;

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

type FrameElement = { getAttribute(name: string): string | null; setAttribute(name: string, value: string): void };

/// The canvas frame is sandboxed without allow-scripts, so every document inside it inherits
/// that promise, and a framed site file is served with `script-src 'none'`. Author
/// `allow-scripts` is narrowed too, so a framed page cannot claim scripting back. Frames that
/// ask for nothing keep their attributes: rewriting them would reload the frame the canvas is
/// about to show.
export function hardenStaticFrames(doc: { querySelectorAll(selector: string): ArrayLike<FrameElement> }) {
  for (const frame of Array.from(doc.querySelectorAll('iframe'))) {
    if (frame.getAttribute('sandbox')?.includes('allow-scripts')) frame.setAttribute('sandbox', 'allow-same-origin');
  }
}
