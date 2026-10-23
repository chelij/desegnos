(() => {
  const script = document.currentScript;
  const sessionId = script.dataset.session;
  const pageId = script.dataset.page;
  const routes = JSON.parse(script.dataset.routes);
  const currentPath = script.dataset.path;
  let reported = 0;
  function report(message) {
    if (reported++ >= 20) return;
    parent.postMessage({ type: 'desegni-preview-error', sessionId, pageId, message: String(message).slice(0, 4096) }, '*');
  }
  addEventListener('error', event => report(event.message || 'A preview resource could not be loaded.'));
  addEventListener('unhandledrejection', event => report(event.reason?.message || event.reason || 'A preview operation failed.'));
  // Escape leaves an interactive frame overlay. The editor cannot see key events inside
  // this document, so the frame reports the one key that belongs to the editor.
  addEventListener('keydown', event => {
    if (event.key !== 'Escape') return;
    parent.postMessage({ type: 'desegni-preview-escape', sessionId, pageId }, '*');
  }, true);
  document.addEventListener('submit', event => {
    event.preventDefault();
    report('Form submission is paused. This preview has no enabled external form binding.');
  }, true);
  document.addEventListener('click', event => {
    const link = event.target instanceof Element ? event.target.closest('a[href]') : null;
    if (!link) return;
    const href = link.getAttribute('href');
    if (href.startsWith('#')) return;
    event.preventDefault();
    try {
      if (script.dataset.fileMode === 'true') {
        const destination = new URL(href, document.baseURI);
        if (destination.origin === location.origin && destination.host === location.host) { location.href = destination.href; return; }
        if (/^https?:$/.test(destination.protocol)) parent.postMessage({ type: 'desegni-preview-link', sessionId, pageId, url: destination.href.slice(0, 2048) }, '*');
        return;
      }
      const destination = new URL(href, 'https://desegni.invalid' + currentPath);
      if (destination.origin !== 'https://desegni.invalid') {
        // The editor offers to open web links in the browser; the preview itself never leaves.
        if (/^https?:$/.test(destination.protocol)) parent.postMessage({ type: 'desegni-preview-link', sessionId, pageId, url: destination.href.slice(0, 2048) }, '*');
        else report(`${destination.protocol.replace(':', '')} links work on the published site, not in the preview.`);
        return;
      }
      const path = decodeURIComponent(destination.pathname).replace(/\/$/, '') || '/';
      const target = routes[path.toLowerCase()];
      if (!target) {
        report('This link has no page in the frozen preview.');
        return;
      }
      location.href = new URL('../' + target + '/index.html' + destination.hash, location.href).href;
    } catch {
      report('This preview link has an invalid destination.');
    }
  }, true);
})();
