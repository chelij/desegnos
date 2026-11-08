import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/session/projectCommands.ts']);
try {
  await modules.rewrite('api.js', () => 'export let native; export const setNative = call => { native = call; };');
  const { setNative } = await modules.load('api.js');
  const { createProjectCommands } = await modules.load('session/projectCommands.js');
  const pending = { code: 'pendingRecovery', projectId: 'project-id', path: '/chosen/site.desegnos' };
  function setup() {
    const state = { pending: null, error: '', calls: [], accepted: [], showRecovery: false, conflicts: [] };
    let commands;
    const env = {
      current: { current: null },
      setPendingRecovery: value => { state.pending = value; }, setError: value => { state.error = value; },
      settle: async () => {}, queue: operation => operation(),
      accept: workspace => state.accepted.push(workspace), setNotice() {},
      closePanel: kind => { if (kind === 'recovery') state.showRecovery = false; },
      setShowStartPage() {}, refreshRecent: async () => {},
      panels: { hidden: false, collapsed: false },
      saveState: { current: { kind: 'idle' } },
      transitionSave: next => {
        env.saveState.current = next;
        if (next.kind !== 'conflict') return;
        state.conflicts.push(next.conflict);
        queueMicrotask(() => commands.chooseSaveConflict(state.choice ?? 'copy'));
      },
    };
    setNative(async (command, args) => {
      state.calls.push({ command, args });
      if (state.respond) return state.respond(command, args);
      if (state.failure) throw state.failure;
      if (!state.result) throw pending;
      return state.result;
    });
    commands = createProjectCommands(env);
    return { state, env, ...commands };
  }
  for (const source of ['dialog', 'recent']) {
    await test(`${source} opening offers recovery inline and retries the exact saved file`, async () => {
      const { state, open, openRecent } = setup();
      await (source === 'dialog' ? open() : openRecent(pending.path));
      assert.deepEqual(state.pending, pending);
      assert.equal(state.error, '');
      assert.deepEqual(state.accepted, []);
      state.result = { project: { id: pending.projectId, viewport: { width: 100, height: 100 } } };
      await open(false, state.pending.path, true);
      assert.deepEqual(state.calls.at(-1), { command: 'open_project', args: { folder: false, path: pending.path, discardRecovery: true } });
      assert.equal(state.pending, null);
      assert.deepEqual(state.accepted, [state.result]);
    });
  }
  await test('a pending choice stays beside an already open project instead of replacing it', async () => {
    const { state, env, open } = setup();
    env.current.current = { project: { id: 'open-id' } };
    await open();
    assert.deepEqual(state.pending, pending);
    assert.deepEqual(state.accepted, []);
  });
  await test('unrelated native errors retain their existing reporting path', async () => {
    const { state, openRecent } = setup();
    state.failure = { missing: true, message: 'Could not open this recent project' };
    await assert.rejects(() => openRecent(pending.path), error => error === state.failure);
    assert.equal(state.pending, null);
  });
  await test('Recover uses the existing recover_project flow and clears the choice on success', async () => {
    const { state, open, recover } = setup();
    await open();
    state.result = { project: { id: pending.projectId, viewport: { width: 100, height: 100 } } };
    await recover(state.pending.projectId);
    assert.deepEqual(state.calls.at(-1), { command: 'recover_project', args: { projectId: pending.projectId } });
    assert.equal(state.pending, null);
    assert.equal(state.showRecovery, false);
    assert.deepEqual(state.accepted, [state.result]);
  });
  for (const untitled of [false, true]) {
    await test(`${untitled ? 'untitled' : 'named'} saves resolve repeated disk conflicts before publishing`, async () => {
      const { state, env, save, saveUntitledProject } = setup();
      const workspace = { project: { id: 'project' } };
      state.choice = untitled ? 'replace' : 'copy';
      const conflicts = [{ token: 'first' }, { token: 'second' }];
      const outcomes = [...conflicts.map(conflict => ({ status: 'conflict', conflict })), { status: 'saved', workspace }];
      state.respond = () => outcomes.shift();
      let completion;
      if (untitled) {
        env.current.current = { untitled: true, project: { id: 'project', name: 'Example' } };
        completion = save();
        await saveUntitledProject('Example', '/example.desegnos');
      } else assert.equal(await save(), workspace);
      assert.deepEqual(state.calls, [
        untitled
          ? { command: 'save_untitled_project', args: { name: 'Example', path: '/example.desegnos', label: null } }
          : { command: 'save_project_checked', args: { saveAs: false, label: null } },
        ...conflicts.map(conflict => ({ command: 'resolve_save_conflict', args: { token: conflict.token, choice: state.choice } })),
      ]);
      assert.deepEqual(state.conflicts, conflicts);
      assert.deepEqual(state.accepted, [workspace]);
      assert.equal(env.saveState.current.kind, 'idle');
      if (untitled) assert.equal(await completion, workspace);
    });

    await test(`${untitled ? 'untitled' : 'named'} save cancellation publishes no workspace`, async () => {
      const { state, env, save, saveUntitledProject } = setup();
      state.choice = 'cancel';
      state.respond = command => command === 'resolve_save_conflict'
        ? { status: 'cancelled' } : { status: 'conflict', conflict: { token: 'conflict' } };
      let completed = false;
      if (untitled) {
        env.current.current = { untitled: true, project: { id: 'project', name: 'Example' } };
        void save().then(() => { completed = true; });
        await assert.rejects(saveUntitledProject('Example', null), /Save cancelled/);
      } else assert.equal(await save(), null);
      assert.deepEqual(state.accepted, []);
      assert.equal(completed, false, 'cancelling the disk conflict leaves the untitled save open');
      assert.equal(env.saveState.current.kind, untitled ? 'naming' : 'idle');
    });
  }

  await test('concurrent named saves share one request and a failed save can be retried', async () => {
    const { state, save } = setup();
    state.failure = new Error('Disk unavailable');
    const first = save();
    assert.equal(save(), first);
    await assert.rejects(first, /Disk unavailable/);
    assert.equal(state.calls.length, 1);
    state.failure = null;
    const workspace = { project: { id: 'project' } };
    state.result = { status: 'saved', workspace };
    assert.equal(await save(), workspace);
    assert.equal(state.calls.length, 2);
    assert.deepEqual(state.accepted, [workspace]);
  });
} finally { await modules.cleanup(); }
