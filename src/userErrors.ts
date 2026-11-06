// Presentation only: retain original errors in state, job records and AI requests.
function diagnostic(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) return String(error.message);
  return String(error ?? '');
}

export function runtimeError(error: string): string {
  if (/\bnull\b|\bundefined\b.*(?:object|propert|evaluating)|(?:propert|evaluating).*\bundefined\b/i.test(error)) return "A script on this page looks for something that isn't there.";
  if (/not a function|not callable|is not a constructor/i.test(error)) return "A script on this page tries to do something that isn't available.";
  if (/ReferenceError|is not defined|can't find variable|cannot access .+ before initialization/i.test(error)) return "A script on this page uses a name it doesn't recognise.";
  if (/SyntaxError|unexpected (?:token|identifier|end)|invalid or unexpected token|missing [)\]}]/i.test(error)) return 'A script on this page has a writing mistake.';
  if (/failed to (?:load|fetch)|load failed|loading .+ failed|networkerror|network error|could not load|error loading|failed load/i.test(error)) return "Something this page needs couldn't load.";
  return 'A script on this page stopped working.';
}

export function generationError(reason: string, request = '', affected?: string): string {
  reason = reason.split(/\bExcerpt:/i)[0];
  // A missing request reads as plain words; only real request text belongs in quotes.
  const text = request.trim().replace(/[\n\r]+/g, ' ').slice(0, 100).replace(/[.!?]+$/, '');
  const subject = text ? `“${text}”` : 'this request';
  // Desegnos prepares the frozen request locally, so a preparation failure is not the AI's.
  if (/prepare (?:a |the |this )?(?:new )?(?:generation )?request|preparing (?:a |the |this )?(?:new )?(?:generation )?request/i.test(reason)) return "Desegnos couldn't prepare this request. Try again.";
  if (/cannot be located|original source is unavailable|same styles or behavior|cannot be reconstructed|could not be decoded/i.test(reason)) return `The saved result for ${subject} cannot be merged into this boundary.`;
  if (/\b(?:page|project|file|source) (?:was |has |had )?(?:been )?(?:changed|edited) (?:after|while|since|before)|\brevision\b|\bstale\b|edited after/i.test(reason)) return 'The boundary was edited after the AI made this draft.';
  if (/\barea ids?\b/i.test(reason)) return "The AI's reply was incomplete. Try again.";
  if (/duplicate|(?:ids?|names?).*unique|unique.*(?:ids?|names?)|same.*(?:id|name)/i.test(reason)) return 'Two items ended up with the same name, so editing one could change the other.';
  if (/JSON|schema|parse|reply|response|format|writing mistake/i.test(reason)) return `The AI did not produce a usable boundary for ${subject}.`;
  if (/dynamic|executable|script|JavaScript|shared behavior|CSS outside|CSS selector|stylesheet|sharedCss|outside|scope|another layer|overlap/i.test(reason)) return `${subject} would also change ${affected ?? 'content outside this area'}.`;
  if (/target|selector|missing.*(?:element|layer)|not found/i.test(reason)) return `The item needed for ${subject} could not be found.`;
  return `The AI could not complete ${subject} as requested.`;
}

// Diagnostics name implementation details; a model's own explanation is already prose.
const technical = /[\n\r<>`{}\[\]\\/#:;]|\b[\da-f]{8}-[\da-f-]{27,}\b|\b\w*(?:Error|Exception)\b|\b(?:scopes?|dossiers?|candidates?|schemas?|salvaged|revisions?|layers?|stack|root|DOM|CSS|HTML|JavaScript|finite|MiB|identity|analysis|selector|attribute|validation|checkpoint|journal|persist|mutex|poisoned|executable|stacking|painting|packet|manifest|digest|provenance|credential|vault|keyring)\b|\b\w+\.\w+|(?:^|\s)[.#][\w-]+|\w+\(/i;

function isPlain(value: string): boolean {
  return value.length <= 160 && !technical.test(value) && /^(?:This |That |That's |The |Your |Choose |Select |Enter |Open |Save |Add |Remove |Wait |Stop |Sign |Allow |Keep |Could|Can|We |No |Only |There |A |An |Desegnos |GitHub |Please |Too |Confirm )/.test(value) && (value.match(/[.!?](?:\s|$)/g)?.length ?? 0) <= 2;
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

/// One plain sentence for an outcome the AI itself declared, falling back to translated diagnostics.
export function outcomeReason(reason: string, request = 'this request', affected?: string): string {
  const value = reasonText(reason);
  if (!value) return generationError(reason, request, affected);
  if (modelJargon.test(value) || camelCase.test(value)) return generationError(value, request, affected);
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
  const value = diagnostic(error).replace(/^Error:\s*/, '').trim();
  if (!value) return '';
  // A site-relative file name is the useful part of an oversized-preview message.
  if (/^[\w][\w .@+-]*(?:\/[\w .@+-]+)* is too large to preview \(\d+ MB\)\.$/.test(value) && !/\/\.\.?(?:\/| )/.test(value)) return value;
  // Preserve short plain instructions; translate technical diagnostics below.
  const messages: [RegExp, string][] = [
    [/desktop portal/i, 'File dialogs need the desktop portal. Install xdg-desktop-portal and a backend such as xdg-desktop-portal-gtk, then try again.'],
    [/SyntaxError|unexpected (?:token|identifier|end)|unclosed|unterminated|missing closing/i, 'This code has a writing mistake. Check the highlighted line.'],
    [/stacking|painting rules|move part of its layout/i, 'This item cannot be arranged on its own. Select its containing group.'],
    [/finite.*(?:position|offset|number)/i, 'Enter a valid number.'],
    [/(?:ZIP|file).*32 MiB/i, 'This file is too large. Choose a file smaller than 32 MB.'],
    [/too large to preview|complete preview.*limit/i, 'This page is too large to preview. Try a smaller page.'],
    [/identity analysis|analyzing identity/i, "Desegnos couldn't read your brand. Try again."],
    [/cancell?ed.*(?:record|receipt)|cancellation.*(?:save|storage|attention)/i, "The request stopped, but Desegnos couldn't save its status. Check that you can save files."],
    [/applied.*(?:history|job-history)|(?:history|job-history).*applied/i, "Your changes were added, but their history couldn't be saved."],
    [/copying.*nothing was removed|cut.*(?:failed|could not)/i, "The item couldn't be cut. Copy it again before deleting it."],
    [/clipboard.*(?:limit|large)|layer exceeds.*clipboard/i, 'This item is too large to copy. Copy a smaller group.'],
    [/clipboard.*(?:read|unavailable)|could not be read.*clipboard/i, "Desegnos couldn't read the clipboard. Copy the item again."],
    [/clipboard|could not be copied/i, "Desegnos couldn't copy this item. Try again."],
    [/recovery.*(?:disagree|identity|revision|pointer)|safety checkpoint.*differs/i, "This saved backup doesn't match the project. Open the project again."],
    [/unsaved work in this project is damaged/i, "Desegnos couldn't restore an unsaved project. Its original file is still on disk."],
    [/recovery.*(?:record|load|unavailable|attention)|recover.*(?:failed|error)/i, "Your unsaved work couldn't be restored. Open the saved project."],
    [/dossier|project brief.*(?:save|attention)/i, "Your result is saved, but the project brief couldn't be updated."],
    [/secure storage|vault|keyring|secret service/i, "Desegnos couldn't access your saved sign-in details. Check AI settings."],
    [/usage limit|rate limit|too many requests|quota/i, 'Your AI usage limit has been reached. Try again later.'],
    [/spending.*(?:limit|exceed|reach)|budget.*(?:exceed|reach)|payment|billing|credit/i, 'Your AI spending limit has been reached. Check AI settings.'],
    [/credential|API key|api_key|authentication|unauthori[sz]ed|sign.?in.*(?:expired|failed)|signed out/i, 'Your AI connection needs attention. Sign in again in AI settings.'],
    [/model.*(?:unavailable|not available|not supported|access)|(?:choose|select).*verified.*model/i, "This AI model isn't available. Choose another in AI settings."],
    [/context capacity|request estimate|input limit|context limit/i, 'There is too much for the AI to read. Choose fewer files.'],
    [/browser/i, "Desegnos couldn't open your browser. Check your default browser settings."],
    [/permission denied|not writable|writable storage|read.only|access denied/i, "Desegnos can't save here. Choose another folder."],
    [/no space|disk full|storage full/i, 'Your storage is full. Free some space and try again.'],
    [/network|connect|timed? ?out|fetch/i, "Desegnos couldn't connect. Check your internet connection and try again."],
    [/ENOENT|no such file|file.*(?:missing|not found|no longer exists)/i, "This file couldn't be found. Choose it again."],
    [/build.*(?:failed|error|unavailable)|unknown build action/i, "The website couldn't be prepared for editing. Try again."],
    [/preview.*(?:invalid|location|document|limit)|invalid preview|source.*preview/i, "This page couldn't be previewed. Try opening it again."],
    [/candidate|layer|element|selector|data-desegnos-|schema|salvaged|validation|excerpt|<\/?[a-z][^>]*>/i, generationError(value)],
    [/stale|revision|project changed|page changed/i, 'The boundary has changed. Try again.'],
    [/save|persist|journal|checkpoint/i, "Your changes couldn't be saved. Check that you can save files and try again."],
    [/parse|deserialize|UTF-8|JSON|corrupt|invalid.*(?:data|format)/i, "Desegnos couldn't read this file. Choose another file."],
    [/poison|mutex|panic/i, 'Desegnos ran into a problem. Restart the app.'],
  ];
  if (isPlain(value)) return value;
  for (const [pattern, message] of messages) if (pattern.test(value)) return message;
  return 'Something went wrong. Try again.';
}

export function generationPageError(reason: string): string {
  return reason ? userError(reason) : 'The AI could not finish this request.';
}
