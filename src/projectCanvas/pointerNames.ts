import { elementLabel } from './elementFacts';
import type { Pointer, PointerSource, PointerTarget, Project } from '../types';
import { findIn } from './pointerGeometry';

const referenceLabel = (item: { kind: { kind: string; text?: string; title?: string; url?: string; name?: string; color?: string; element?: { name: string | null; kind: string } } }): string => {
  switch (item.kind.kind) {
    case 'element': return item.kind.element?.name || item.kind.element?.kind || 'Layer';
    case 'note': return (item.kind.text ?? '').trim().split('\n')[0] || 'Note';
    case 'link': return (item.kind.title ?? '').trim() || item.kind.url || 'Link';
    case 'image': return item.kind.name || 'Picture';
    default: return item.kind.color || 'Colour';
  }
};

// Source and target names for Properties, Layers and a pointer's empty label.
export function pointerEndpointName(endpoint: PointerSource | PointerTarget, project: Project, role: 'source' | 'target'): string {
  const missing = role === 'source' ? 'Missing source' : 'Missing target';
  switch (endpoint.kind) {
    case 'element': {
      const page = project.pages.find(item => item.id === endpoint.pageId);
      const element = page ? findIn(project, endpoint.pageId, endpoint.elementId) : null;
      if (!page || !element) return missing;
      return elementLabel(element);
    }
    case 'boundary': return project.pages.find(item => item.id === endpoint.pageId)?.name ?? missing;
    case 'reference': {
      const item = (project.reference ?? []).find(entry => entry.id === endpoint.itemId);
      return item ? referenceLabel(item) : missing;
    }
    case 'url': return endpoint.url;
  }
}

export function pointerLabel(pointer: Pointer, project: Project): string {
  return pointer.comment.trim().split('\n')[0]
    || `${pointerEndpointName(pointer.from, project, 'source')} → ${pointerEndpointName(pointer.to, project, 'target')}`;
}
