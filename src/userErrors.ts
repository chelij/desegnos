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

export function generationError(reason: string): string {
  reason = reason.split(/\bExcerpt:/i)[0];
  if (/changed|revision|stale/i.test(reason)) return 'The page has changed since this draft was made. Try again.';
  if (/dynamic|executable|script|JavaScript|shared behavior/i.test(reason)) return 'The AI added a script that affects the wrong part of the page. Try again.';
  if (/CSS outside|CSS selector|stylesheet|sharedCss/i.test(reason)) return 'The AI added styles that affect the wrong part of the page. Try again.';
  if (/target|selector|missing.*(?:element|region)|not found/i.test(reason)) return "The AI couldn't find the part of the page to change. Try again.";
  if (/outside|scope|another region|overlap/i.test(reason)) return 'The AI changed the wrong part of the page. Try again.';
  if (/JSON|schema|parse|reply|response|format/i.test(reason)) return "The AI sent a reply Desegnos couldn't use. Try again.";
  return "The AI's changes couldn't be added to the page. Try again.";
}

function isPlain(value: string): boolean {
  return value.length <= 160 && !/[\n\r<>`{}\[\]\\/#:;]|\b[\da-f]{8}-[\da-f-]{27,}\b|\b\w*(?:Error|Exception)\b|\b(?:scopes?|dossiers?|candidates?|schemas?|salvaged|revisions?|regions?|stack|root|DOM|CSS|HTML|JavaScript|finite|MiB|identity|analysis|selector|attribute|validation|checkpoint|journal|persist|mutex|poisoned|executable|stacking|painting|packet|manifest|digest|provenance|credential|vault|keyring)\b|\b\w+\.\w+|(?:^|\s)[.#][\w-]+|\w+\(/i.test(value) && /^(?:This |That |The |Your |Choose |Select |Enter |Open |Save |Add |Remove |Wait |Stop |Sign |Allow |Keep |Could|Can|We |No |Only |There |A |An |Desegnos |Please |Too )/.test(value) && (value.match(/[.!?](?:\s|$)/g)?.length ?? 0) <= 2;
}

export function userError(error: unknown): string {
  const value = diagnostic(error).replace(/^Error:\s*/, '').trim();
  if (!value) return '';
  // Preserve short plain instructions; translate technical diagnostics below.
  const messages: [RegExp, string][] = [
    [/SyntaxError|unexpected (?:token|identifier|end)|unclosed|unterminated|missing closing/i, 'This code has a writing mistake. Check the highlighted line.'],
    [/stacking|painting rules|move part of its layout/i, 'This item cannot be arranged on its own. Select its containing group.'],
    [/finite.*(?:position|offset|number)/i, 'Enter a valid number.'],
    [/(?:ZIP|file).*32 MiB/i, 'This file is too large. Choose a file smaller than 32 MB.'],
    [/complete preview.*limit/i, 'This page is too large to preview. Try a smaller page.'],
    [/identity analysis|analyzing identity/i, "Desegnos couldn't read your brand. Try again."],
    [/cancell?ed.*(?:record|receipt)|cancellation.*(?:save|storage|attention)/i, "The request stopped, but Desegnos couldn't save its status. Check that you can save files."],
    [/applied.*(?:history|job-history)|(?:history|job-history).*applied/i, "Your changes were added, but their history couldn't be saved."],
    [/copying.*nothing was removed|cut.*(?:failed|could not)/i, "The item couldn't be cut. Copy it again before deleting it."],
    [/clipboard.*(?:limit|large)|layer exceeds.*clipboard/i, 'This item is too large to copy. Copy a smaller group.'],
    [/clipboard.*(?:read|unavailable)|could not be read.*clipboard/i, "Desegnos couldn't read the clipboard. Copy the item again."],
    [/clipboard|could not be copied/i, "Desegnos couldn't copy this item. Try again."],
    [/recovery.*(?:disagree|identity|revision|pointer)|safety checkpoint.*differs/i, "This saved backup doesn't match the project. Open the project again."],
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
    [/candidate|region|selector|data-desegnos-|schema|salvaged|validation|excerpt|<\/?[a-z][^>]*>/i, generationError(value)],
    [/stale|revision|project changed|page changed/i, 'The page has changed. Try again.'],
    [/save|persist|journal|checkpoint/i, "Your changes couldn't be saved. Check that you can save files and try again."],
    [/parse|deserialize|UTF-8|JSON|corrupt|invalid.*(?:data|format)/i, "Desegnos couldn't read this file. Choose another file."],
    [/poison|mutex|panic/i, 'Desegnos ran into a problem. Restart the app.'],
  ];
  if (isPlain(value)) return value;
  for (const [pattern, message] of messages) if (pattern.test(value)) return message;
  return 'Something went wrong. Try again.';
}
