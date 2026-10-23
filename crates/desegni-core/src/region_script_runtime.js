;(() => {
  const block = document.querySelector(__REGION_SELECTOR__);
  const regionRoot = block && block.parentElement;
  if (!regionRoot) return;
  try { (function(root, window, document, setTimeout, requestAnimationFrame) { 'use strict';
__REGION_CODE__
  })(regionRoot, window, document, window.setTimeout.bind(window), window.requestAnimationFrame.bind(window)); } catch (error) { console.error('Region script failed', error); }
})();
