import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/session/useWindowClose.ts']);
try {
  await modules.rewrite('session/useWindowClose.js', source => source
    .replace(/^import .* from '@tauri-apps\/api\/core';$/m, 'const isTauri = () => true;')
    .replace(/^import .* from '@tauri-apps\/api\/window';$/m, 'const getCurrentWindow = () => globalThis.__windowClose.window;')
    .replace(/^import .* from 'react';$/m, 'const { useEffect, useRef, useState } = globalThis.__windowClose.hooks;')
    .replace(/^import .* from '\.\.\/api\.js';$/m, 'const native = (...args) => globalThis.__windowClose.native(...args);'));
  let handler, request, destroyed, errors;
  globalThis.__windowClose = {
    hooks: {
      useEffect(effect) { effect(); },
      useRef(value) { return { current: value }; },
      useState(value) { return [value, next => { request = next; }]; },
    },
    window: {
      async onCloseRequested(callback) { handler = callback; return () => {}; },
      async destroy() { destroyed++; },
    },
    native: async () => null,
  };
  const { useWindowClose } = await modules.load('session/useWindowClose.js');
  function setup({ identity = null, failure, dirty = false, generation = null } = {}) {
    request = null; destroyed = 0; errors = [];
    globalThis.__windowClose.native = async command => {
      assert.equal(command, 'get_identity_analysis');
      if (failure) throw failure;
      return identity;
    };
    useWindowClose({ settle: async () => {}, current: { current: { dirty } },
      generation: { current: generation }, aiAuthentication: { current: null }, setError: error => errors.push(error) });
  }
  async function close() {
    let prevented = false;
    const result = handler({ preventDefault() { prevented = true; } });
    // The handler waits for project edits and the native status lookup before asking.
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(prevented, true);
    return { result };
  }
  await test('damaged identity allows confirmed exit and keeps unsaved-work protection', async () => {
    setup({ failure: 'A identity record is damaged or uses an unsupported format. The original was preserved.', dirty: true });
    const { result } = await close();
    assert.match(request.message, /status could not be checked/);
    assert.match(request.message, /not been saved/);
    assert.equal(errors.length, 1);
    assert.equal(destroyed, 0);
    request.resolve(true);
    await result;
    assert.equal(destroyed, 1);
  });
  await test('declining an unavailable-status close allows another close attempt', async () => {
    setup({ failure: 'Unreadable identity record' });
    const first = await close();
    request.resolve(false); await first.result;
    assert.equal(destroyed, 0);
    const second = await close();
    request.resolve(true); await second.result;
    assert.equal(destroyed, 1);
  });
  await test('clean idle window exits without confirmation', async () => {
    setup();
    const { result } = await close(); await result;
    assert.equal(request, null);
    assert.equal(destroyed, 1);
  });
  await test('active analysis still warns about submitted work and credits', async () => {
    setup({ identity: { status: 'analyzing', authentication: 'chatgpt' } });
    const { result } = await close();
    assert.match(request.message, /Generation is still running/);
    assert.match(request.message, /ChatGPT plan or credits/);
    request.resolve(false); await result;
    assert.equal(destroyed, 0);
  });
} finally {
  delete globalThis.__windowClose;
  await modules.cleanup();
}
