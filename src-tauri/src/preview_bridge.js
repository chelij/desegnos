(() => {
  const script = document.currentScript;
  const sessionId = script.dataset.session;
  const pageId = script.dataset.page;
  const routes = JSON.parse(script.dataset.routes);
  const currentPath = script.dataset.path;
  const fileMode = script.dataset.fileMode === 'true';
  let reported = 0;
  function post(message) {
    parent.postMessage(message, '*');
  }
  function report(message) {
    if (reported++ >= 20) return;
    post({ type: 'desegnos-preview-error', sessionId, pageId, message: String(message).slice(0, 4096) });
  }
  addEventListener('error', event => report(event.message || 'A preview resource could not be loaded.'));
  addEventListener('unhandledrejection', event => report(event.reason?.message || event.reason || 'A preview operation failed.'));
  // Escape leaves an interactive frame overlay. The editor cannot see key events inside
  // this document, so the frame reports the one key that belongs to the editor.
  addEventListener('keydown', event => {
    if (event.key !== 'Escape') return;
    post({ type: 'desegnos-preview-escape', sessionId, pageId });
  }, true);
  // The Live bar shows where the page is. Imported sites report their real address;
  // a generated page reports the route it will have on the published site.
  function announce() {
    const path = (fileMode ? location.pathname : currentPath) + location.search + location.hash;
    post({ type: 'desegnos-preview-location', sessionId, pageId, path: path.slice(0, 2048) });
  }
  addEventListener('load', announce);
  addEventListener('hashchange', announce);
  addEventListener('popstate', announce);
  announce();
  // Back, Forward and Reload drive this session's own history. The editor is the only
  // sender that counts, and the command must name this session.
  addEventListener('message', event => {
    if (event.source !== parent) return;
    const value = event.data;
    if (!value || typeof value !== 'object' || Array.isArray(value)) return;
    if (value.type !== 'desegnos-preview-command' || value.sessionId !== sessionId) return;
    if (value.action === 'back') history.back();
    else if (value.action === 'forward') history.forward();
    else if (value.action === 'reload') location.reload();
  });
  function sameSite(url) {
    return url.protocol === location.protocol && url.host === location.host;
  }
  // A generated site keeps its routes as session files, so a same-site route link points
  // at the page this session serves. An imported site serves its files at their paths.
  function route(url) {
    if (fileMode) return null;
    let path;
    try { path = decodeURIComponent(url.pathname); } catch { path = url.pathname; }
    const target = routes[(path.replace(/\/$/, '') || '/').toLowerCase()];
    return target ? `../${target}/index.html` : null;
  }
  function offerLink(url) {
    post({ type: 'desegnos-preview-link', sessionId, pageId, url: String(url).slice(0, 2048) });
  }
  // Links belong to the browser. The editor only hears about destinations this session
  // cannot reach: a new window, another site, or a scheme with no preview.
  document.addEventListener('click', event => {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const link = event.target instanceof Element ? event.target.closest('a[href], area[href]') : null;
    if (!link) return;
    const href = link.getAttribute('href');
    if (!href || href.startsWith('#')) return;
    let destination;
    try {
      destination = new URL(href, document.baseURI);
    } catch {
      event.preventDefault();
      report('This preview link has an invalid destination.');
      return;
    }
    if (destination.protocol === 'javascript:') return;
    const blank = /^_blank$/i.test((link.getAttribute('target') || '').trim());
    if (sameSite(destination) && !blank) {
      const routed = route(destination);
      // Following a route still uses the browser; it just points at the served page.
      if (routed) link.setAttribute('href', routed + destination.search + destination.hash);
      return;
    }
    event.preventDefault();
    if (/^https?:$/.test(destination.protocol)) offerLink(destination.href);
    else if (blank) report('This link needs a new window, which the preview cannot open.');
    else report(`${destination.protocol.replace(':', '')} links work on the published site, not in the preview.`);
  }, true);
  const FORM_MESSAGE = 'Form submission is paused. This preview has no enabled external form binding.';
  // A GET form is an ordinary navigation to a session file. A form that needs a server,
  // another site or a new window has nowhere to go in the preview.
  document.addEventListener('submit', event => {
    const form = event.target;
    if (!(form instanceof HTMLFormElement)) return;
    const submitter = event.submitter;
    const method = (submitter?.getAttribute('formmethod') || form.method || 'get').toLowerCase();
    if (method === 'dialog') return;
    const target = (submitter?.getAttribute('formtarget') || form.getAttribute('target') || '').trim();
    if (method !== 'get' || /^_blank$/i.test(target)) {
      event.preventDefault();
      report(FORM_MESSAGE);
      return;
    }
    let action;
    try {
      action = new URL(submitter?.getAttribute('formaction') || form.getAttribute('action') || document.baseURI, document.baseURI);
    } catch {
      event.preventDefault();
      report(FORM_MESSAGE);
      return;
    }
    if (!sameSite(action)) {
      event.preventDefault();
      report(FORM_MESSAGE);
      return;
    }
    const routed = route(action);
    if (routed) form.setAttribute('action', routed + action.search);
  }, true);
})();
