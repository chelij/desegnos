import { SIZE_PRESETS } from './generated/limits';
// The extraction wrapper runs inside the local static server's origin. The page itself is
// rendered in a nested, script-disabled frame; this script only reads its DOM and posts a
// compact tree back to the editor.
/** How long the extractor waits for fonts and pictures; the outer capture timer must outlast it. */
const IMAGE_WAIT_MS = 8000;
export const CAPTURE_TIMEOUT_MS = IMAGE_WAIT_MS * 4;

export const EXTRACTOR_SCRIPT = String.raw`
(function () {
  'use strict';
  var params = new URLSearchParams(location.search);
  var id = params.get('id') || '';
  var pagePath = params.get('url') || '/';
  var width = Math.max(320, Number(params.get('width')) || ${SIZE_PRESETS.desktop.width});
  var height = width >= 700 ? ${SIZE_PRESETS.desktop.height} : ${SIZE_PRESETS.phone.height};
  function post(type, payload) {
    var message = { type: type, id: id };
    if (payload) { for (var key in payload) { if (Object.prototype.hasOwnProperty.call(payload, key)) message[key] = payload[key]; } }
    parent.postMessage(message, '*');
  }
  function number(value) {
    var parsed = parseFloat(value);
    return isFinite(parsed) ? parsed : 0;
  }
  function colour(value) {
    if (!value || value === 'transparent' || value === 'rgba(0, 0, 0, 0)') return '';
    return value;
  }
  function level(value, fallback) {
    var v = String(value || '').toLowerCase();
    if (v === 'bold' || v === 'bolder') return 700;
    if (v === 'normal') return 400;
    var parsed = parseInt(v, 10);
    return isFinite(parsed) ? parsed : fallback;
  }
  function shadow(value) {
    var result = { shadowX: 0, shadowY: 0, shadowBlur: 0, shadowColor: '' };
    if (!value || value === 'none') return result;
    var match = value.match(/(rgba?\([^)]*\)|#[0-9a-fA-F]{3,8})\s+(-?[\d.]+)px\s+(-?[\d.]+)px\s+([\d.]+)px/);
    if (!match) return result;
    result.shadowColor = match[1];
    result.shadowX = number(match[2]);
    result.shadowY = number(match[3]);
    result.shadowBlur = number(match[4]);
    return result;
  }
  function radius(value, rect) {
    if (!value) return 0;
    if (value.indexOf('%') >= 0) return Math.max(0, Math.min(rect.width, rect.height) / 2);
    return Math.max(0, number(value));
  }
  function trackCount(value) {
    if (!value || value === 'none') return 1;
    var count = 0, depth = 0, inToken = false;
    for (var index = 0; index < value.length; index += 1) {
      var character = value.charAt(index);
      if (character === '(') depth += 1;
      else if (character === ')') depth = Math.max(0, depth - 1);
      else if (depth === 0 && /\s/.test(character)) {
        if (inToken) { count += 1; inToken = false; }
        continue;
      }
      if (!/\s/.test(character)) inToken = true;
    }
    if (inToken) count += 1;
    return Math.max(1, count);
  }
  function directText(element) {
    var text = '';
    for (var index = 0; index < element.childNodes.length; index += 1) {
      var child = element.childNodes[index];
      if (child.nodeType === 3 || child.nodeType === 4) text += ' ' + child.nodeValue;
    }
    return text.replace(/\s+/g, ' ').trim();
  }
  function rectOf(element) {
    var rect = element.getBoundingClientRect();
    return { x: rect.left + (window.scrollX || 0), y: rect.top + (window.scrollY || 0), width: rect.width, height: rect.height };
  }
  function visible(element, style, rect) {
    if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') return false;
    if (style.contentVisibility === 'hidden') return false;
    if (number(style.opacity) <= 0.001) return false;
    return rect.width >= 1 && rect.height >= 1;
  }
  function attributesOf(element) {
    var ariaExpanded = element.getAttribute('aria-expanded');
    var ariaControls = element.getAttribute('aria-controls');
    var toggle = element.getAttribute('data-toggle') || element.getAttribute('data-bs-toggle') || element.getAttribute('data-target');
    return {
      id: element.id || '',
      className: typeof element.className === 'string' ? element.className : (element.getAttribute('class') || ''),
      role: element.getAttribute('role') || '',
      ariaExpanded: ariaExpanded === null ? null : ariaExpanded,
      ariaControls: ariaControls === null ? null : ariaControls,
      dataToggle: toggle === null ? null : String(toggle),
      name: element.getAttribute('name')
    };
  }
  function stylesOf(element, rect) {
    var style = (element.ownerDocument.defaultView || window).getComputedStyle(element);
    var fontSize = number(style.fontSize) || 16;
    var lineHeight = style.lineHeight === 'normal' ? 1.4 : (style.lineHeight.indexOf('px') >= 0 ? number(style.lineHeight) / fontSize : number(style.lineHeight));
    var shadowValues = shadow(style.boxShadow);
    var padding = Math.max(number(style.paddingTop), number(style.paddingRight), number(style.paddingBottom), number(style.paddingLeft));
    return {
      background: colour(style.backgroundColor),
      color: style.color || '',
      borderColor: style.borderTopColor || '',
      borderWidth: number(style.borderTopWidth),
      radius: radius(style.borderTopLeftRadius, rect),
      opacity: number(style.opacity),
      shadowX: shadowValues.shadowX,
      shadowY: shadowValues.shadowY,
      shadowBlur: shadowValues.shadowBlur,
      shadowColor: shadowValues.shadowColor,
      fontFamily: style.fontFamily || '',
      fontSize: fontSize,
      fontWeight: level(style.fontWeight, 400),
      italic: style.fontStyle === 'italic' || style.fontStyle === 'oblique',
      lineHeight: lineHeight || 1.4,
      letterSpacing: style.letterSpacing === 'normal' ? 0 : number(style.letterSpacing),
      align: style.textAlign || '',
      display: style.display || '',
      direction: style.flexDirection || '',
      rowGap: style.rowGap === 'normal' ? 0 : number(style.rowGap),
      columnGap: style.columnGap === 'normal' ? 0 : number(style.columnGap),
      padding: padding,
      columns: trackCount(style.gridTemplateColumns),
      alignItems: style.alignItems || '',
      justifyContent: style.justifyContent || '',
      wrap: style.flexWrap === 'wrap' || style.flexWrap === 'wrap-reverse'
    };
  }
  var SKIP = { script: 1, style: 1, head: 1, title: 1, meta: 1, link: 1, noscript: 1, template: 1, br: 1, source: 1, track: 1, option: 1, optgroup: 1, col: 1, colgroup: 1, wbr: 1 };
  function nodeOf(element, depth, budget) {
    if (depth > 40 || budget.count > 6000) return null;
    var tag = String(element.tagName || element.nodeName || '').toLowerCase();
    if (SKIP[tag]) return null;
    var rect = rectOf(element);
    var style = (element.ownerDocument.defaultView || window).getComputedStyle(element);
    if ((tag === 'html' || tag === 'body') && (rect.width < 1 || rect.height < 1)) {
      var view = element.ownerDocument.defaultView || window;
      rect = { x: 0, y: 0, width: view.innerWidth || width, height: Math.max(view.innerHeight || height, element.ownerDocument.documentElement.scrollHeight || 0, element.ownerDocument.body.scrollHeight || 0) };
    }
    var contents = style.display === 'contents';
    if (!contents && !visible(element, style, rect)) return null;
    budget.count += 1;
    var node = {
      tag: tag,
      rect: rect,
      text: directText(element) || null,
      src: null,
      poster: null,
      href: null,
      inputType: null,
      placeholder: null,
      value: null,
      alt: null,
      options: [],
      attributes: attributesOf(element),
      styles: stylesOf(element, rect),
      children: []
    };
    if (tag === 'img') {
      node.src = element.currentSrc || element.src || null;
      node.alt = element.getAttribute('alt') || null;
    } else if (tag === 'a') {
      node.href = element.href || null;
    } else if (tag === 'video') {
      node.src = element.currentSrc || element.getAttribute('src') || null;
      node.poster = element.poster ? new URL(element.poster, element.ownerDocument.baseURI).href : null;
    } else if (tag === 'audio') {
      node.src = element.currentSrc || element.getAttribute('src') || null;
    } else if (tag === 'iframe' || tag === 'embed') {
      node.src = element.src || element.getAttribute('src') || null;
    } else if (tag === 'input') {
      node.inputType = (element.getAttribute('type') || 'text').toLowerCase();
      node.value = element.value || null;
      node.placeholder = element.getAttribute('placeholder');
      node.alt = element.getAttribute('alt');
    } else if (tag === 'textarea') {
      node.value = element.value || null;
      node.placeholder = element.getAttribute('placeholder');
    } else if (tag === 'select') {
      node.options = Array.prototype.map.call(element.options || [], function (option) { return option.text || option.value || ''; }).slice(0, 64);
      node.value = element.value || null;
    } else if (tag === 'button') {
      node.value = element.getAttribute('value');
      node.alt = element.getAttribute('aria-label');
    }
    for (var index = 0; index < element.children.length; index += 1) {
      var child = nodeOf(element.children[index], depth + 1, budget);
      if (child) node.children.push(child);
    }
    if (contents && (node.rect.width < 1 || node.rect.height < 1) && node.children.length) {
      var left = Infinity, top = Infinity, right = -Infinity, bottom = -Infinity;
      node.children.forEach(function (child) {
        left = Math.min(left, child.rect.x);
        top = Math.min(top, child.rect.y);
        right = Math.max(right, child.rect.x + child.rect.width);
        bottom = Math.max(bottom, child.rect.y + child.rect.height);
      });
      node.rect = { x: left, y: top, width: Math.max(1, right - left), height: Math.max(1, bottom - top) };
    }
    return node;
  }
  function waitForAssets(doc) {
    var waits = [];
    Array.prototype.forEach.call(doc.querySelectorAll ? doc.querySelectorAll('video, audio') : [], function (media) {
      try { media.muted = true; media.pause(); } catch (error) {}
    });
    var images = Array.prototype.slice.call(doc.images || []);
    images.forEach(function (image) {
      try { image.loading = 'eager'; } catch (error) {}
      if (!image.complete) {
        waits.push(new Promise(function (resolve) {
          var done = function () { resolve(); };
          image.addEventListener('load', done, { once: true });
          image.addEventListener('error', done, { once: true });
        }));
      }
    });
    if (doc.fonts && doc.fonts.ready) waits.push(doc.fonts.ready.catch(function () {}));
    return Promise.race([Promise.all(waits), new Promise(function (resolve) { setTimeout(resolve, ${IMAGE_WAIT_MS}); })]);
  }
  function extract(doc, pageUrl) {
    var budget = { count: 1 };
    var nodes = [];
    var root = doc.documentElement || doc.body;
    var rootNode = nodeOf(root, 0, budget);
    if (rootNode) nodes.push(rootNode);
    var scripts = Array.prototype.slice.call(doc.scripts || []).map(function (script) {
      return { src: script.src || null, text: (script.textContent || '').slice(0, 4000) };
    }).filter(function (script) { return !!script.src || !!script.text; }).slice(0, 60);
    return {
      url: pageUrl,
      title: doc.title || '',
      width: width,
      height: Math.max(doc.documentElement.scrollHeight || 0, doc.body.scrollHeight || 0, 1),
      nodes: nodes,
      scripts: scripts
    };
  }
  try {
    var stage = document.getElementById('stage');
    var frame = document.createElement('iframe');
    frame.setAttribute('sandbox', 'allow-same-origin');
    frame.setAttribute('title', 'Imported page');
    frame.style.cssText = 'display:block;border:0;width:' + width + 'px;height:' + height + 'px;background:#fff';
    frame.addEventListener('load', function () {
      try {
        var doc = frame.contentDocument;
        if (!doc) throw new Error('The imported page could not be read.');
        waitForAssets(doc).then(function () {
          var pageUrl = frame.contentWindow.location.href;
          post('desegnos-extract', { capture: extract(doc, pageUrl) });
        }, function (error) {
          post('desegnos-extract-error', { message: String((error && error.message) || error) });
        });
      } catch (error) {
        post('desegnos-extract-error', { message: String((error && error.message) || error) });
      }
    });
    frame.addEventListener('error', function () {
      post('desegnos-extract-error', { message: 'That page could not be loaded.' });
    });
    frame.src = pagePath;
    stage.appendChild(frame);
    setTimeout(function () {
      post('desegnos-extract-error', { message: 'That page took too long to render.' });
    }, 25000);
  } catch (error) {
    post('desegnos-extract-error', { message: String((error && error.message) || error) });
  }
})();
`;
