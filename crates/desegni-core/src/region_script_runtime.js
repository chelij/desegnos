;(() => {
  const block = document.querySelector(__REGION_SELECTOR__);
  const regionRoot = block && block.parentElement;
  if (!regionRoot) return;
  const readEvents = __GLOBAL_READ_EVENTS__;
  const created = new Set();
  const proxies = new WeakMap();
  const originals = new WeakMap();
  const listeners = new WeakMap();
  const unwrap = value => originals.get(value) || value;
  const owns = node => node === regionRoot || regionRoot.contains(node) ||
    [...created].some(parent => parent === node || parent.contains(node));
  const fail = () => { throw new Error('This script changes things outside its box.'); };
  const reads = new Set([
    'querySelector', 'querySelectorAll', 'getElementById', 'getElementsByTagName',
    'getElementsByClassName', 'getBoundingClientRect', 'getClientRects', 'closest',
    'matches', 'contains', 'getAttribute', 'hasAttribute', 'item', 'entries', 'keys',
    'values', 'forEach', 'map', 'filter', 'find', 'some', 'every', 'slice', 'includes', 'indexOf', 'toString', 'next',
    'getPropertyValue', 'getPropertyPriority'
  ]);
  function eventView(event) {
    return new Proxy(event, {
      get(target, key) {
        if (['preventDefault', 'stopPropagation', 'stopImmediatePropagation'].includes(key)) {
          return () => { if (event.target instanceof Node && owns(event.target)) event[key](); };
        }
        const value = Reflect.get(target, key, target);
        return typeof value === 'function' ? undefined : protect(value, false);
      },
      set: fail, defineProperty: fail, deleteProperty: fail, setPrototypeOf: fail
    });
  }
  function listen(target, type, callback, options) {
    if (!readEvents.includes(type) && !(target instanceof Node && owns(target)) &&
        !(target instanceof MediaQueryList && type === 'change')) return;
    if (typeof callback !== 'function') return;
    let callbacks = listeners.get(target);
    if (!callbacks) listeners.set(target, callbacks = new Map());
    let records = callbacks.get(callback);
    if (!records) callbacks.set(callback, records = []);
    const capture = typeof options === 'boolean' ? options : !!options?.capture;
    if (records.some(record => record.type === type && record.capture === capture)) return;
    const handler = event => {
      if (options?.once) records.splice(records.findIndex(record => record.handler === handler), 1);
      try { callback(eventView(event)); } catch (error) { console.error('Region script event failed', error); }
    };
    records.push({ type, capture, handler });
    target.addEventListener(type, handler, options);
  }
  function unlisten(target, type, callback, options) {
    const records = listeners.get(target)?.get(callback);
    const capture = typeof options === 'boolean' ? options : !!options?.capture;
    const index = records?.findIndex(record => record.type === type && record.capture === capture);
    if (index >= 0) {
      target.removeEventListener(type, records[index].handler, capture);
      records.splice(index, 1);
    }
  }
  // Keep references useful for reading, but check ownership again at every write. This also
  // covers aliases, collection callbacks, closest() results, and nodes moved since a lookup.
  function protect(value, writable = false) {
    if (!value || (typeof value !== 'object' && typeof value !== 'function')) return value;
    if (originals.has(value)) return value;
    if (proxies.has(value)) return proxies.get(value);
    const owner = value instanceof Node ? value : null;
    const canWrite = () => owner ? owns(owner) : typeof writable === 'function' ? writable() : writable;
    const proxy = new Proxy(value, {
      get(target, key) {
        if (['constructor', 'prototype', '__proto__', 'ownerDocument', 'defaultView'].includes(key)) return undefined;
        if (target === window && ['top', 'parent', 'opener'].includes(key)) return undefined;
        if (key === 'addEventListener') return (type, callback, options) => listen(target, type, callback, options);
        if (key === 'removeEventListener') return (type, callback, options) => unlisten(target, type, callback, options);
        const result = Reflect.get(target, key, target);
        if (typeof result !== 'function') return protect(result, canWrite);
        return (...args) => {
          if (!canWrite() && !reads.has(key) && key !== Symbol.iterator) {
            throw new Error('This script cannot use that action outside its box.');
          }
          if (['append', 'appendChild', 'prepend', 'insertBefore', 'replaceChild', 'replaceChildren'].includes(key)) {
            for (const arg of args) if (unwrap(arg) instanceof Node && !owns(unwrap(arg))) return fail();
          }
          const safeArgs = args.map(arg => typeof arg === 'function'
            ? (...items) => arg(...items.map(item => protect(item, canWrite))) : unwrap(arg));
          return protect(Reflect.apply(result, target, safeArgs), canWrite);
        };
      },
      set(target, key, next) { if (!canWrite()) return fail(); return Reflect.set(target, key, unwrap(next), target); },
      deleteProperty(target, key) { if (!canWrite()) return fail(); return Reflect.deleteProperty(target, key); },
      defineProperty: fail, setPrototypeOf: fail
    });
    proxies.set(value, proxy);
    originals.set(proxy, value);
    return proxy;
  }
  const root = protect(regionRoot);
  const scopedWindow = Object.freeze({
    get scrollX() { return window.scrollX; },
    get scrollY() { return window.scrollY; },
    get innerWidth() { return window.innerWidth; },
    get innerHeight() { return window.innerHeight; },
    matchMedia(query) { return protect(window.matchMedia(query)); },
    setTimeout(callback, delay) { return typeof callback === 'function' ? setTimeout(callback, delay) : undefined; },
    clearTimeout(id) { clearTimeout(id); },
    requestAnimationFrame(callback) { return requestAnimationFrame(callback); },
    cancelAnimationFrame(id) { cancelAnimationFrame(id); },
    addEventListener(type, callback, options) { listen(window, type, callback, options); },
    removeEventListener(type, callback, options) { unlisten(window, type, callback, options); }
  });
  const scopedDocument = Object.freeze({
    get body() { return protect(document.body); },
    get documentElement() { return protect(document.documentElement); },
    get hidden() { return document.hidden; },
    get visibilityState() { return document.visibilityState; },
    querySelector(selector) { return protect(document.querySelector(selector)); },
    querySelectorAll(selector) { return protect(document.querySelectorAll(selector)); },
    getElementById(id) { return protect(document.getElementById(id)); },
    getElementsByTagName(tag) { return protect(document.getElementsByTagName(tag)); },
    getElementsByClassName(name) { return protect(document.getElementsByClassName(name)); },
    addEventListener(type, callback, options) { listen(document, type, callback, options); },
    removeEventListener(type, callback, options) { unlisten(document, type, callback, options); },
    createTextNode(text) { const node = document.createTextNode(text); created.add(node); return protect(node); },
    createElement(tag) {
      if (typeof tag !== 'string' || ['script', 'iframe', 'object', 'embed', 'link', 'base', 'style', 'meta'].includes(tag.toLowerCase())) throw new Error('Unsafe region element');
      const node = document.createElement(tag);
      created.add(node);
      return protect(node);
    }
  });
  try { (function(root, window, document, setTimeout, requestAnimationFrame) { 'use strict';
__REGION_CODE__
  })(root, scopedWindow, scopedDocument, scopedWindow.setTimeout, scopedWindow.requestAnimationFrame); } catch (error) { console.error('Region script failed', error); }
})();
