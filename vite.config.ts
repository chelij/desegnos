import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import { readFileSync } from 'node:fs';

const { build: { devUrl } } = JSON.parse(readFileSync(new URL('./src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const development = new URL(devUrl);

// WebKitGTK 2.54 aborts the page (AccessibilityObjectAtspi::selectionChanged → UTF16OffsetToUTF8,
// WebKit bug 279313) when the caret sits next to a replaced element such as <img> inside a
// contenteditable while an accessibility client listens — the desktop shell always does.
// CodeMirror draws an empty <img class="cm-widgetBuffer"> around inline widgets, so the code
// editor uses an empty <span> instead (styled in styles.css).
function codeMirrorWidgetBufferSpan(): Plugin {
  const original = 'let img = document.createElement("img");\n        img.className = "cm-widgetBuffer";';
  return {
    name: 'codemirror-widget-buffer-span', enforce: 'pre',
    transform(code, id) {
      if (!id.includes('@codemirror/view') || !code.includes('cm-widgetBuffer')) return null;
      if (!code.includes(original)) this.error('@codemirror/view changed: re-check the widget buffer patch for the WebKitGTK accessibility crash.');
      return { code: code.replace(original, 'let img = document.createElement("span");\n        img.className = "cm-widgetBuffer";'), map: null };
    },
  };
}

export default defineConfig({ plugins: [codeMirrorWidgetBufferSpan(), react()], clearScreen: false,
  server: { host: development.hostname, port: Number(development.port), strictPort: true },
  // Pre-bundling would skip the transform above in development.
  optimizeDeps: { exclude: ['@codemirror/view'] },
  envPrefix: ['VITE_'], build: { target: 'es2022' } });
