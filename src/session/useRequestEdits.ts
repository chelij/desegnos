import { useCallback, type Dispatch, type RefObject, type SetStateAction } from 'react';
import type { CanvasTarget } from '../projectCanvas/canvasTargets';
import { findElementIn } from '../projectCanvas/elementFacts';
import { baseSizeId } from '../projectCanvas/sizes';
import { readRequestAttachments } from '../requestAttachments';
import type { Command, PendingRequest, Project, RequestAttachment, WorkspaceView } from '../types';
import type { ProjectEdits } from './projectEdits';

function ownerPageId(project: Project, elementId: string): string | null {
  return project.pages.find(page => !!findElementIn(page.elements, elementId))?.id ?? null;
}

const requestTargetingElement = (project: Project, elementId: string): PendingRequest | null =>
  (project.requests ?? []).find(request => request.targets.some(target => target.kind === 'element' && target.id === elementId)) ?? null;

const elementRequest = (page: Project['pages'][number] | undefined, pageId: string, elementId: string, text: string, attachments: RequestAttachment[]): PendingRequest => ({
  id: crypto.randomUUID(), text, attachments, requestedAt: new Date().toISOString(),
  targets: [{ kind: 'element', page: pageId, size: page && page.sizes.length > 1 ? baseSizeId(page) : null, id: elementId }], area: null });

/** Requests attached to single layers: their text, their attachments and the Feedback reply. */
export function useRequestEdits({ current, mutate, edits, setError, setNotice, setCanvasSelection }: {
  current: RefObject<WorkspaceView | null>;
  mutate: (command: Command) => Promise<WorkspaceView | null>;
  edits: ProjectEdits;
  setError: (message: string) => void;
  setNotice: (message: string, undo?: () => void) => void;
  setCanvasSelection: Dispatch<SetStateAction<CanvasTarget[]>>;
}) {
  const removeCanvasRequests = useCallback((ids: string[]) => {
    if (!ids.length) return;
    setCanvasSelection(selection => selection.filter(target => target.kind !== 'request' || !ids.includes(target.requestId)));
    void mutate({ type: 'remove_requests', ids });
  }, [mutate]);

  // The layer's request box in Properties edits the one request that targets only that layer.
  const setCanvasElementRequest = useCallback((elementId: string, data: { text: string; attachments: RequestAttachment[] } | null) => {
    const project = current.current?.project;
    if (!project) return;
    const existing = (project.requests ?? []).find(request => request.targets.length === 1
      && request.targets[0].kind === 'element' && request.targets[0].id === elementId) ?? null;
    if (data === null || (!data.text.trim() && !data.attachments.length)) {
      if (existing) removeCanvasRequests([existing.id]);
      return;
    }
    const pageId = ownerPageId(project, elementId);
    const request = existing
      ? { ...existing, text: data.text, attachments: data.attachments }
      : elementRequest(project.pages.find(item => item.id === pageId), pageId ?? '', elementId, data.text, data.attachments);
    if (existing) void edits.updateRequest(request);
    else void edits.addRequest(request);
  }, [edits, removeCanvasRequests]);

  async function setElementPrompt(pageId: string, elementId: string, prompt: string) {
    const project = current.current?.project;
    const page = project?.pages.find(item => item.id === pageId);
    if (!project || !page || !findElementIn(page.elements, elementId)) throw new Error('This layer is no longer in the project.');
    const existing = requestTargetingElement(project, elementId);
    const request = existing ? { ...existing, text: prompt } : elementRequest(page, pageId, elementId, prompt, []);
    await mutate(existing ? { type: 'update_request', request } : { type: 'add_request', request });
  }

  const attachToRequest = useCallback(async (requestId: string, files: File[], links: string[]) => {
    const request = (current.current?.project?.requests ?? []).find(item => item.id === requestId);
    if (!request) return;
    const { attachments, errors: problems } = await readRequestAttachments(request.attachments, files, links);
    if (!await mutate({ type: 'update_request', request: { ...request, attachments } })) return;
    if (problems.length) setError(problems.join(' ')); else setNotice('Attached to the request.');
  }, [mutate]);

  /** Appends the user's feedback to the prompt of each reply target and returns the layers that were updated. */
  async function addFeedback(targets: { pageId: string; elementId: string }[], text: string): Promise<string[]> {
    const elementIds: string[] = [];
    for (const target of targets) {
      const project = current.current?.project;
      const page = project?.pages.find(page => page.id === target.pageId);
      const element = page && findElementIn(page.elements, target.elementId);
      if (!project || !element) continue;
      const prompt = requestTargetingElement(project, element.id)?.text.trim() ?? '';
      await setElementPrompt(target.pageId, target.elementId, prompt ? `${prompt}\n\nFeedback: ${text}` : `Feedback: ${text}`);
      elementIds.push(target.elementId);
    }
    return elementIds;
  }

  return { setCanvasElementRequest, attachToRequest, addFeedback };
}
