import { errorMessage, nativeErrorKind } from './errorMessage.js';
import { NEEDS_AI_SETTINGS_ERROR_KINDS } from './generated/limits';
import type { ProviderError } from './types';

export type ProviderProblem = { message: string; settings: boolean; action: 'AI settings' | 'Try again' };
const settingsKinds = new Set(NEEDS_AI_SETTINGS_ERROR_KINDS);

export function providerProblem(error: ProviderError | null | undefined, message = ''): ProviderProblem | null {
  if (!error) return null;
  const settings = settingsKinds.has(error.kind);
  return { message, settings, action: settings ? 'AI settings' : 'Try again' };
}

export function providerFailureMessage(error: ProviderError | null | undefined, message = ''): string | null {
  return error ? message || 'The AI could not finish this request.' : null;
}

export function generationError(request = ''): string {
  const text = request.trim().replace(/[\n\r]+/g, ' ').slice(0, 100).replace(/[.!?]+$/, '');
  const subject = text ? `“${text}”` : 'this request';
  return `The AI could not complete ${subject} as requested.`;
}

// Model explanations may use punctuation and questions freely but must not leak internals.
// App-generated fallback text is also recognised so a rejected reply never looks like a question.
const modelJargon = /[\n\r<>`{}\[\]\\]|\b[\da-f]{8}-[\da-f-]{27,}\b|\barea [0-9a-f]{8,}[^\s]*|data-desegnos|\bgeneration result\b|\brequired format\b|\brequirements format\b|\bgenerated source\b|\bconflicted page\b|\bomitted a requested layer\b|\b(?:regions?|candidates?|schemas?|validat(?:e|ed|es|ion|or)|selectors?|dossiers?|revisions?|excerpts?|DOM|CSS|HTML|JavaScript|stack|finite)\b/i;
// Case-sensitive: camelCase identifiers such as pageId leak implementation names.
const camelCase = /\b[a-z]+[A-Z]\w*\b/;
// App-issued conflict text names the area (and sometimes the page) before its own reason.
const appPrefix = /^Area\s+[0-9a-f][0-9a-f-]{7,}(?:\s*,\s*page\s+[0-9a-f][0-9a-f-]{7,})?\s*[:,]\s*/i;
const pagePrefix = /^Area\s+[0-9a-f][0-9a-f-]{7,}\s*,\s*page\s+/i;

function reasonText(reason: string): string {
  return String(reason ?? '').split(/\bExcerpt:/i)[0].replace(appPrefix, '').replace(/\s+/g, ' ').trim();
}

/// A validation explanation names implementation details; a model explanation is plain prose.
export function technicalReason(reason: string): boolean {
  const raw = String(reason ?? '');
  const text = reasonText(reason);
  return pagePrefix.test(raw) || camelCase.test(text) || modelJargon.test(text);
}

export function outcomeReason(reason: string, request = 'this request'): string {
  const value = reasonText(reason);
  if (!value) return generationError(request);
  if (modelJargon.test(value) || camelCase.test(value)) return generationError(request);
  const first = value.match(/^.*?[.!?](?=\s|$)/)?.[0] ?? value;
  const sentence = first.length > 200 ? `${first.slice(0, 197).trimEnd()}…` : first;
  return /[.!?…]$/.test(sentence) ? sentence : `${sentence}.`;
}

/// Open-in-browser failures carry the already-running local address separately, so the
/// editor can offer to copy it instead of turning the message into a generic notice.
export function browserOpenUrl(error: unknown): string | null {
  if (!error || typeof error !== 'object' || !('url' in error)) return null;
  const url = (error as { url?: unknown }).url;
  return typeof url === 'string' && /^https?:\/\/[^\s]+$/.test(url) ? url : null;
}

export function userError(error: unknown): string {
  if (error && typeof error === 'object' && 'providerError' in error) {
    const structured = error as { providerError?: ProviderError };
    const message = providerFailureMessage(structured.providerError, 'message' in error ? errorMessage(error) : '');
    if (message) return message;
  }
  switch (nativeErrorKind(error)) {
    case 'saveLocation': return "Desegnos couldn't save here. Choose another folder or check your storage.";
    case 'clipboard': return "Desegnos couldn't access the clipboard. Try copying the item again.";
    case 'browser': return "Desegnos couldn't open your browser. Check your default browser settings.";
    case 'aiSettings': return errorMessage(error) || 'Check your AI settings and try again.';
    case 'command': return errorMessage(error) || 'The operation could not finish. Try again.';
    default: return errorMessage(error);
  }
}

export function needsAiSettings(error: unknown, structured?: ProviderError | null): boolean {
  const problem = providerProblem(structured);
  if (problem) return problem.settings;
  return nativeErrorKind(error) === 'aiSettings';
}
