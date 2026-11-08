import type { GenerationCandidate, Project } from './types';

const join = (labels: string[]) => labels.length < 2 ? labels.join('') : `${labels.slice(0, -1).join(', ')} and ${labels.at(-1)}`;

/** Page properties belong in the result's changed-items summary alongside layer edits. */
export function summarizePageChanges(candidate: GenerationCandidate | null | undefined, project: Project): string {
  const changes = candidate?.requests.flatMap(request => request.status === 'implemented' ? request.operations.flatMap(operation => {
    if (operation.kind !== 'setPage') return [];
    const page = operation.name ?? project.pages.find(page => page.id === operation.pageId)?.name ?? 'Page';
    const fields = [operation.name !== null ? 'name' : '', operation.path !== null ? `path ${operation.path}` : '', operation.background !== null ? `background ${operation.background}` : ''].filter(Boolean);
    return fields.length ? [`${page}: ${join(fields)}`] : [];
  }) : []) ?? [];
  return changes.length ? `Updated ${join(changes)}` : '';
}
