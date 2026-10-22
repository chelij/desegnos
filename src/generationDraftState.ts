import type { GenerationJobView } from './types';

// Repair rounds can retain the same reply more than once. Keep every saved record, but
// show identical page drafts only once and count the same entries in the canvas summary.
export function pendingDrafts(job: GenerationJobView | null) {
  const drafts = job?.drafts?.filter(draft => !draft.dismissed && !draft.applied) ?? [];
  return drafts.filter((draft, index) => drafts.findIndex(item => item.id === draft.id || item.pageId === draft.pageId && item.code === draft.code) === index);
}
