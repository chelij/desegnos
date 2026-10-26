(() => {
  const script = document.currentScript;
  const sessionId = script.dataset.session;
  const pageId = script.dataset.page;
  const routes = JSON.parse(script.dataset.routes);
  const currentPath = script.dataset.path;
  const fileMode = script.dataset.fileMode === 'true';
  const sourceUrl = location.href;
  // The bridge is the first parser script. Remember the first actual node for
  // each disposable source token, including nodes later removed or cloned by site scripts.
  // Mutation records retain parser-created nodes even when a script removes them.
  const owners = new Map();
  function remember(node) {
    if (node.nodeType !== 1) return;
    const key = node.getAttribute('data-desegnos-preview-key');
    const source = node.getAttribute('data-desegnos-preview-source');
    if (key && source && !owners.has(key)) owners.set(key, { node, source });
  }
  function records(changes) {
    for (const change of changes) for (const node of change.addedNodes) remember(node);
  }
  const ownership = new MutationObserver(records);
  ownership.observe(document, { childList: true, subtree: true });
  for (const node of document.querySelectorAll('*')) remember(node);
  const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
  // Each document settles independently, so nested captures share the same idle
  // window and frames inserted during startup are included in the final tree.
  const settled = (async () => {
    if (document.readyState !== 'complete') await new Promise(resolve => addEventListener('load', resolve, { once: true }));
    await Promise.race([document.fonts.ready.catch(() => {}), pause(1500)]);
    await pause(300);
  })();
  const childRequests = new Map();
  const bridgedChildren = new Set();
  parent.postMessage({ type: 'desegnos-capture-ready', sessionId }, '*');
  addEventListener('message', async event => {
    const value = event.data;
    if (!value || typeof value !== 'object') return;
    if (value.type === 'desegnos-capture-ready' && value.sessionId === sessionId) { bridgedChildren.add(event.source); return; }
    if (value.type === 'desegnos-snapshot') {
      const request = childRequests.get(value.token);
      if (request && event.source === request.window && value.sessionId === sessionId) request.resolve(value.snapshot);
      return;
    }
    if (event.source !== parent || value.type !== 'desegnos-capture' || value.sessionId !== sessionId || typeof value.token !== 'string') return;
    await settled;
    const frameNodes = Array.from(document.querySelectorAll('iframe'));
    const frames = await Promise.all(frameNodes.map(async (frame, index) => {
      let local = false;
      try { local = sameSite(new URL(frame.src, document.baseURI)) && !frame.hasAttribute('srcdoc') && (!frame.hasAttribute('sandbox') || frame.sandbox.contains('allow-scripts')); } catch { /* invalid frame source */ }
      if (!bridgedChildren.has(frame.contentWindow) && !local) {
        // Inline srcdoc documents can share this preview's opaque origin. They
        // have no source mapping, but their settled DOM can still be frozen.
        try {
          const child = frame.contentDocument;
          if (child?.documentElement && child.URL === 'about:srcdoc') return { index, snapshot: { html: child.documentElement.outerHTML, base: document.baseURI, frames: [] } };
        } catch { /* an external frame has its own origin */ }
        return null;
      }
      const token = `${value.token}:${index}`;
      const snapshot = await new Promise(resolve => {
        childRequests.set(token, { window: frame.contentWindow, resolve });
        frame.contentWindow?.postMessage({ type: 'desegnos-capture', sessionId, token }, '*');
        setTimeout(() => resolve(null), 2000);
      });
      childRequests.delete(token);
      return snapshot ? { index, snapshot } : null;
    }));
    records(ownership.takeRecords());
    const copy = document.documentElement.cloneNode(true);
    const originals = [document.documentElement, ...document.documentElement.querySelectorAll('*')];
    const clones = [copy, ...copy.querySelectorAll('*')];
    // Preserve property-only form state and pixels that HTML serialization omits.
    clones.forEach((node, index) => {
      const original = originals[index];
      if (original instanceof HTMLStyleElement && original.sheet) {
        try { node.textContent = Array.from(original.sheet.cssRules, rule => rule.cssText).join('\n'); } catch { /* retain authored stylesheet text */ }
      }
      if (original instanceof HTMLInputElement) {
        node.setAttribute('value', original.value);
        if (original.checked) node.setAttribute('checked', ''); else node.removeAttribute('checked');
      }
      if (original instanceof HTMLTextAreaElement) node.textContent = original.value;
      if (original instanceof HTMLOptionElement) {
        if (original.selected) node.setAttribute('selected', ''); else node.removeAttribute('selected');
      }
      if (original instanceof HTMLCanvasElement && original.width && original.height) {
        try { node.style.backgroundImage = `url("${original.toDataURL()}")`; node.style.backgroundSize = '100% 100%'; } catch { /* cross-origin pixels cannot be read */ }
      }

      node.removeAttribute('data-desegnos-snapshot-source');
      const key = original.getAttribute('data-desegnos-preview-key');
      const owner = owners.get(key);
      node.removeAttribute('data-desegnos-preview-key');
      node.removeAttribute('data-desegnos-preview-source');
      if (owner?.node === original) node.setAttribute('data-desegnos-snapshot-source', owner.source);
    });
    for (const animation of document.getAnimations()) {
      const target = animation.effect?.target;
      const index = originals.indexOf(target);
      if (index < 0) continue;
      const pseudo = animation.effect.pseudoElement;
      const computed = getComputedStyle(target, pseudo);
      const style = pseudo ? document.createElement('div').style : clones[index].style;
      for (const key of new Set(animation.effect.getKeyframes().flatMap(frame => Object.keys(frame)))) {
        if (['offset', 'computedOffset', 'easing', 'composite'].includes(key)) continue;
        const property = key.startsWith('--') ? key : key.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`);
        style.setProperty(property, computed.getPropertyValue(property), 'important');
      }
      style.setProperty('animation', 'none', 'important');
      style.setProperty('transition', 'none', 'important');
      if (pseudo) {
        clones[index].setAttribute('data-desegnos-frozen-animation', String(index));
        const rule = document.createElement('style');
        rule.textContent = `[data-desegnos-frozen-animation="${index}"]${pseudo}{${style.cssText}}`;
        copy.querySelector('head').append(rule);
      }
    }
    const finalFrames = Array.from(document.querySelectorAll('iframe'));
    const capturedFrames = frames.filter(Boolean).flatMap(frame => {
      const index = finalFrames.indexOf(frameNodes[frame.index]);
      return index < 0 ? [] : [{ index, snapshot: frame.snapshot }];
    });
    parent.postMessage({ type: 'desegnos-snapshot', sessionId, token: value.token, snapshot: { html: copy.outerHTML, base: document.baseURI, url: sourceUrl, frames: capturedFrames } }, '*');
  });
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
