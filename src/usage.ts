import { errorMessage } from './errorMessage.js';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { CommandName } from './generated/commands';

// Local usage log: which controls, shortcuts and commands are used, never what is typed.
// Events are batched to the native `record_usage` command (see src-tauri/src/usage.rs).
type UsageEvent = { t: number; kind: 'click' | 'key' | 'typing' | 'command' | 'session'; name: string; detail?: string; count?: number; ms?: number; ok?: boolean };

const queue: UsageEvent[] = [];
// Reads and polling say nothing about what the user did.
const QUIET = /^(get_|list_|autosave_project$|record_usage$|test_ai_access$)/;
let typing: { name: string; detail?: string; count: number; started: number } | null = null;

function clean(value: string | null | undefined, limit: number) {
  // Numbers become # so "Request 3" and "Request 4" read as the same control.
  const text = (value ?? '').replace(/\s+/g, ' ').trim().replace(/\d+/g, '#');
  return text ? Array.from(text).slice(0, limit).join('') : undefined;
}

// Failure reasons stay literal: their wording and numbers are what makes them traceable.
function message(error: unknown, limit: number) {
  const text = errorMessage(error).replace(/\s+/g, ' ').trim();
  return text ? Array.from(text).slice(0, limit).join('') : undefined;
}

function recordUsage(event: Omit<UsageEvent, 't'>) {
  if (!isTauri()) return;
  queue.push({ t: Date.now(), ...event });
  if (queue.length >= 200) flush();
}

function flush() {
  finishTyping();
  if (!queue.length || !isTauri()) return;
  const events = queue.splice(0, queue.length);
  void invoke('record_usage' satisfies CommandName, { events }).catch(error => console.warn('Could not record editor activity', error));
}

function area(element: Element) {
  if (element.closest('.recovery-list')) return 'recovery';
  const owner = element.closest('[role="dialog"], [aria-label], [data-testid="canvas"], header, footer, aside');
  if (!owner || owner === element) return undefined;
  if (owner.matches('[data-testid="canvas"]')) return 'canvas';
  return clean(owner.getAttribute('aria-label') ?? owner.querySelector('h2')?.textContent ?? owner.tagName.toLowerCase(), 60);
}

// Names that may hold the user's own words (project and page names) are replaced by their role.
function controlName(element: Element) {
  if (element.closest('.recovery-list, .recent-projects')) return clean(element.textContent, 40);
  return clean(element.getAttribute('aria-label') ?? element.getAttribute('title') ?? element.textContent, 60);
}

function fieldName(element: HTMLElement) {
  const labelled = element.getAttribute('aria-label') ?? (element.id ? document.querySelector(`label[for="${CSS.escape(element.id)}"]`)?.textContent : null) ?? element.getAttribute('placeholder');
  if (labelled) return clean(labelled, 60);
  if (element.closest('.cm-editor')) return 'code editor';
  if (element.isContentEditable || element.matches('textarea.canvas-text-editor')) return 'canvas text';
  return element.tagName.toLowerCase();
}

function finishTyping() {
  if (!typing) return;
  queue.push({ t: typing.started, kind: 'typing', name: typing.name, detail: typing.detail, count: typing.count });
  typing = null;
}

export function installUsageLog() {
  if (!isTauri()) return () => {};
  recordUsage({ kind: 'session', name: 'start' });
  const click = (event: MouseEvent) => {
    const target = event.target instanceof Element ? event.target.closest('button, a, summary, [role="menuitem"], [role="tab"], [role="option"], input[type="checkbox"], input[type="radio"], select') : null;
    if (target) { const name = controlName(target); if (name) recordUsage({ kind: 'click', name, detail: area(target) }); return; }
    if (event.target instanceof Element && event.target.closest('[data-testid="canvas"]') && event.detail === 2) recordUsage({ kind: 'click', name: 'canvas double-click', detail: 'canvas' });
  };
  const key = (event: KeyboardEvent) => {
    const target = event.target instanceof HTMLElement ? event.target : null;
    const editing = !!target && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName) || !!target.closest('.cm-editor'));
    const modified = event.ctrlKey || event.metaKey || event.altKey;
    if (modified || /^(Escape|Delete|Backspace|Enter|Tab|F\d+)$/.test(event.key) && !(editing && /^(Backspace|Enter|Tab)$/.test(event.key))) {
      const combo = [event.ctrlKey && 'Ctrl', event.metaKey && 'Meta', event.altKey && 'Alt', event.shiftKey && 'Shift', event.key.length === 1 ? event.key.toUpperCase() : event.key].filter(Boolean).join('+');
      recordUsage({ kind: 'key', name: combo, detail: editing && target ? fieldName(target) : undefined });
      return;
    }
    // Plain typing is only counted per field, never recorded.
    if (editing && target && (event.key.length === 1 || event.key === 'Backspace' || event.key === 'Enter')) {
      const name = fieldName(target) ?? 'field';
      if (typing?.name !== name) { finishTyping(); typing = { name, detail: area(target), count: 0, started: Date.now() }; }
      typing.count += 1;
    }
  };
  const hidden = () => { if (document.visibilityState === 'hidden') flush(); };
  document.addEventListener('click', click, true);
  document.addEventListener('keydown', key, true);
  document.addEventListener('focusout', finishTyping, true);
  document.addEventListener('visibilitychange', hidden);
  window.addEventListener('beforeunload', flush);
  const timer = window.setInterval(flush, 5000);
  return () => {
    document.removeEventListener('click', click, true);
    document.removeEventListener('keydown', key, true);
    document.removeEventListener('focusout', finishTyping, true);
    document.removeEventListener('visibilitychange', hidden);
    window.removeEventListener('beforeunload', flush);
    window.clearInterval(timer); flush();
  };
}

export function recordCommand(command: string, args: Record<string, unknown> | undefined, started: number, ok: boolean, error?: unknown) {
  // Quiet reads are left out unless they fail: a failed read can still raise a visible error.
  if (ok && QUIET.test(command)) return;
  const edit = (args?.command as { type?: string } | undefined)?.type;
  // A failure records the native error message instead of the edit type, so it can be diagnosed later.
  const detail = ok ? (typeof edit === 'string' ? edit : undefined) : message(error, 200), ms = Math.round(performance.now() - started);
  // Typing in a request saves on every keystroke; repeats fold into one event with a count.
  const last = queue.at(-1);
  if (last?.kind === 'command' && last.name === command && last.detail === detail && last.ok === ok && Date.now() - last.t < 10_000) {
    last.count = (last.count ?? 1) + 1; last.ms = Math.max(last.ms ?? 0, ms); return;
  }
  recordUsage({ kind: 'command', name: command, detail, ms, ok });
}
