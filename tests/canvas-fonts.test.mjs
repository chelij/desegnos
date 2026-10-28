import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/projectCanvas/fonts.ts', 'src/projectCanvas/elementGeometry.ts']);
try {
  await modules.rewrite('projectCanvas/fonts.js', source => source.replace(/import \{ native \} from '[^']+';/, 'const native = (...args) => globalThis.fontCommand(...args);'));
  const commands = [];
  let release;
  const loading = new Promise(resolve => { release = resolve; });
  globalThis.fontCommand = async (command, args) => {
    commands.push([command, args]);
    return command === 'resolve_canvas_font'
      ? { id: 'same-face', bundled: false, fellBack: args.family === 'Missing', italic: false }
      : 'data:font/ttf;base64,AAAA';
  };
  const registered = [];
  globalThis.FontFace = class {
    constructor(family, source) { this.family = family; this.source = source; }
    async load() { await loading; return this; }
  };
  let measuredFont;
  globalThis.document = {
    fonts: { add: face => registered.push(face) },
    createElement: () => ({ getContext: () => ({ set font(value) { measuredFont = value; }, measureText: () => ({ width: 42 }) }) }),
  };
  const fonts = await modules.load('projectCanvas/fonts.js');
  const geometry = await modules.load('projectCanvas/elementGeometry.js');
  await test('canvas waits for the resolved face, shares loads across aliases, and measures privately', async () => {
    const style = { fontFamily: 'Installed', weight: 400, italic: false, size: 16, lineHeight: 1.4, letterSpacing: 0 };
    const first = fonts.loadCanvasFont(style);
    assert.equal(fonts.canvasFont(style), undefined);
    assert.deepEqual(geometry.measureText('Hello', style), { width: 0, height: 0 });
    const alias = fonts.loadCanvasFont({ ...style, fontFamily: 'Missing' });
    release();
    const [resolved, fallback] = await Promise.all([first, alias]);
    assert.equal(resolved.family, 'dsg-same-face');
    assert.equal(fallback.fellBack, true);
    assert.equal(registered.length, 1);
    assert.equal(commands.filter(([command]) => command === 'load_canvas_font').length, 1);
    assert.equal(await fonts.loadCanvasFont(style), resolved);
    assert.equal(geometry.measureText('Hello', style).width, 43);
    assert.match(measuredFont, /"dsg-same-face"$/);
    assert.doesNotMatch(measuredFont, /Installed/);
  });
} finally {
  delete globalThis.fontCommand;
  delete globalThis.FontFace;
  delete globalThis.document;
  await modules.cleanup();
}
