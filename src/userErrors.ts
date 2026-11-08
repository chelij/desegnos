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

// A model explanation may use punctuation and questions freely but must not leak internals.
const modelJargon = /[\n\r<>`{}\[\]\\]|\b[\da-f]{8}-[\da-f-]{27,}\b|data-desegnos|\b(?:regions?|candidates?|schemas?|validat(?:e|ed|es|ion|or)|selectors?|dossiers?|revisions?|excerpts?|DOM|CSS|HTML|JavaScript|stack|finite)\b/i;
// Case-sensitive: camelCase identifiers such as pageId leak implementation names.
const camelCase = /\b[a-z]+[A-Z]\w*\b/;

function reasonText(reason: string): string {
  return String(reason ?? '').replace(/\s+/g, ' ').trim();
}

/// A model explanation that names implementation details is not plain prose.
export function technicalReason(reason: string): boolean {
  const text = reasonText(reason);
  return camelCase.test(text) || modelJargon.test(text);
}

export function outcomeReason(reason: string, request = 'this request'): string {
  const value = reasonText(reason);
  if (!value) return generationError(request);
  if (technicalReason(value)) return generationError(request);
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
