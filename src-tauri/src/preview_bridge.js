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
      const destination = new URL(href, 'https://desegni.invalid' + currentPath);
      if (destination.origin !== 'https://desegni.invalid') {
        report('External navigation is blocked in the isolated preview.');
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
