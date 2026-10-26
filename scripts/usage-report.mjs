#!/usr/bin/env node
// Summarizes how Desegnos is actually used, from local data only:
// - usage/*.jsonl: clicks, shortcuts, typing counts and commands (src/usage.ts)
// - generation/<project>/<job>/: every request sent to the AI and its outcome
// The real-use native suite (tests/native/real-use-workflow.mjs) is built around these flows;
// rerun this after a while of normal use and update that suite when the patterns change.
//
//   node scripts/usage-report.mjs [--data <dir>] [--out <file.md>]
import { readFile, readdir, stat, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const option = name => { const index = args.indexOf(name); return index === -1 ? undefined : args[index + 1]; };
const data = option('--data') ?? join(process.env.XDG_DATA_HOME ?? join(homedir(), '.local/share'), 'io.github.chelij.desegnos');
const out = option('--out');

async function list(path) { try { return await readdir(path); } catch { return []; } }
const count = (map, key, by = 1) => map.set(key, (map.get(key) ?? 0) + by);
const top = (map, limit = 15) => [...map].sort((a, b) => b[1] - a[1]).slice(0, limit);
const table = (rows, head) => rows.length ? [`| ${head.join(' | ')} |`, `| ${head.map(() => '---').join(' | ')} |`, ...rows.map(row => `| ${row.join(' | ')} |`)].join('\n') : '_None yet._';

// Usage log.
const events = [];
for (const name of (await list(join(data, 'usage'))).filter(name => name.endsWith('.jsonl')).sort()) {
  for (const line of (await readFile(join(data, 'usage', name), 'utf8')).split('\n')) {
    if (!line.trim()) continue;
    try { events.push(JSON.parse(line)); } catch { /* a partial last line */ }
  }
}
events.sort((a, b) => a.t - b.t);
const clicks = new Map(), keys = new Map(), typing = new Map(), commands = new Map(), edits = new Map(), failures = new Map(), slow = new Map();
const steps = [];
for (const event of events) {
  if (event.kind === 'click') { count(clicks, `${event.name}${event.detail ? ` · ${event.detail}` : ''}`); steps.push(`click ${event.name}`); }
  if (event.kind === 'key') { count(keys, `${event.name}${event.detail ? ` · ${event.detail}` : ''}`); steps.push(`key ${event.name}`); }
  if (event.kind === 'typing') { count(typing, event.name, event.count ?? 1); steps.push(`type ${event.name}`); }
  if (event.kind === 'command') {
    const name = event.detail ? `${event.name}:${event.detail}` : event.name;
    count(commands, name, event.count ?? 1);
    if (event.name === 'edit_project' && event.detail) count(edits, event.detail, event.count ?? 1);
    if (event.ok === false) count(failures, name);
    if ((event.ms ?? 0) > 500) count(slow, name);
  }
}
// Three-step sequences show the flows people repeat.
const flows = new Map();
for (let index = 0; index + 2 < steps.length; index++) if (new Set(steps.slice(index, index + 3)).size > 1) count(flows, steps.slice(index, index + 3).join(' → '));
const sessions = events.filter(event => event.kind === 'session' && event.name === 'start').length;

// Generation history.
const jobs = [];
for (const project of await list(join(data, 'generation'))) {
  for (const job of await list(join(data, 'generation', project))) {
    const folder = join(data, 'generation', project, job);
    let record, markdown = '';
    try { record = JSON.parse(await readFile(join(folder, 'job.json'), 'utf8')); } catch { continue; }
    try { markdown = await readFile(join(folder, 'REQUIREMENTS.md'), 'utf8'); } catch { /* A failed write may leave the report unavailable. */ }
    const view = record.view ?? record;
    const prompts = [...markdown.matchAll(/"prompt": "((?:[^"\\]|\\.)*)"/g)].map(match => JSON.parse(`"${match[1]}"`));
    const kinds = [];
    if (markdown.includes('\n## Selected elements\n')) kinds.push('element fix');
    else if (markdown.includes('"elements"')) kinds.push('selected elements (full page)');
    if (markdown.includes('"mobileOnly": true')) kinds.push('phone fix');
    if (markdown.includes('"attachments"')) kinds.push('attachments');
    if (prompts.some(prompt => prompt.startsWith('Update this page to follow the project brand rules'))) kinds.push('brand restyle');
    if (!kinds.length) kinds.push('new areas');
    const files = await list(folder);
    jobs.push({ project: project.slice(0, 8), when: (await stat(join(folder, 'job.json'))).mtimeMs, status: view.status ?? 'unknown', kinds, prompts, format: markdown.match(/Requirements format: (\d+)/)?.[1] ?? '?', followUp: files.includes('CONTEXT.md'), tokens: view.usage?.inputTokens });
  }
}
jobs.sort((a, b) => a.when - b.when);
const byKind = new Map(), byStatus = new Map();
for (const job of jobs) { for (const kind of job.kinds) count(byKind, kind); count(byStatus, job.status); }
// The same requests sent again right after a discard: the result was not what was wanted.
let retries = 0;
for (let index = 1; index < jobs.length; index++) if (jobs[index - 1].status === 'discarded' && jobs[index].prompts[0]?.slice(0, 40) === jobs[index - 1].prompts[0]?.slice(0, 40)) retries++;

const report = `# Desegnos usage report

Data: \`${data}\` · ${new Date().toISOString().slice(0, 16).replace('T', ' ')}

## Generation history

${jobs.length} generations across ${new Set(jobs.map(job => job.project)).size} projects. ${retries} were retries straight after discarding a result.

${table(top(byStatus), ['Outcome', 'Jobs'])}

${table(top(byKind), ['Kind', 'Jobs'])}

${table(jobs.map(job => [new Date(job.when).toISOString().slice(5, 16).replace('T', ' '), job.project, job.status, job.kinds.join(', '), job.format, job.tokens ?? '', job.prompts.map(prompt => prompt.replace(/\s+/g, ' ').slice(0, 70).replace(/\|/g, '/')).join(' / ')]), ['When', 'Project', 'Outcome', 'Kind', 'Format', 'Input tokens', 'Requests'])}

## Editor usage

${events.length ? `${events.length} events over ${sessions} sessions (${new Date(events[0].t).toISOString().slice(0, 10)} – ${new Date(events.at(-1).t).toISOString().slice(0, 10)}).` : '_No usage log yet. It starts recording once this version of the app runs._'}

### Most used controls
${table(top(clicks, 25), ['Control · where', 'Clicks'])}

### Shortcuts
${table(top(keys), ['Keys · field', 'Uses'])}

### Typing (keystrokes per field, text is never recorded)
${table(top(typing), ['Field', 'Keystrokes'])}

### Project edits
${table(top(edits), ['Edit', 'Count'])}

### Repeated flows
${table(top(flows, 20), ['Three steps in a row', 'Times'])}

### Problems
${table([...top(failures).map(([name, value]) => [name, 'failed', value]), ...top(slow).map(([name, value]) => [name, 'over 500 ms', value])], ['Command', 'Issue', 'Times'])}
`;
if (out) { await writeFile(out, report); console.log(`Wrote ${out}`); } else console.log(report);
