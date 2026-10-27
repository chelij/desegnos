import type { Element, Pointer, Project } from '../types';
import { resolveForSize } from './sizes';

/** The transient state Play keeps for one session: nothing here is saved. */
export type PlayState = {
  /** The boundary being played, or null when Play is off. */
  boundary: string | null;
  /** Element ids Play hid or showed; only overrides live here. */
  hidden: Record<string, boolean>;
  /** Element ids whose content another element's content replaced for this session. */
  swaps: Record<string, string>;
};

export const idlePlay: PlayState = { boundary: null, hidden: {}, swaps: {} };

export const playIsActive = (play: PlayState) => play.boundary !== null;

/** The pointer a click on this element runs in Play, or null when it runs nothing. */
export function playPointerAt(project: Project, pageId: string, elementId: string): Pointer | null {
  for (const pointer of project.pointers ?? []) {
    if (pointer.from.kind !== 'element' || pointer.from.pageId !== pageId || pointer.from.elementId !== elementId) continue;
    if (pointer.play) return pointer;
  }
  return null;
}

export const playElementIdAt = (project: Project, pageId: string, elementId: string) => playPointerAt(project, pageId, elementId)?.id ?? null;

/** One click's outcome: the next transient state and a boundary to move the camera to. */
export type PlayStep = { play: PlayState; goTo: string | null };

export function playClick(project: Project, play: PlayState, pageId: string, elementId: string): PlayStep {
  const pointer = playPointerAt(project, pageId, elementId);
  if (!pointer?.play) return { play, goTo: null };
  const action = pointer.play;
  switch (action.kind) {
    case 'goTo':
      return { play: { ...play, boundary: action.boundary }, goTo: action.boundary };
    case 'show':
      return { play: { ...play, hidden: { ...play.hidden, [action.element]: false } }, goTo: null };
    case 'hide':
      return { play: { ...play, hidden: { ...play.hidden, [action.element]: true } }, goTo: null };
    case 'toggle':
      return { play: { ...play, hidden: { ...play.hidden, [action.element]: !play.hidden[action.element] } }, goTo: null };
    case 'swap':
      return { play: { ...play, swaps: { ...play.swaps, [action.element]: action.withElement } }, goTo: null };
  }
}

function findElement(elements: Element[], id: string): Element | null {
  for (const element of elements) {
    if (element.id === id) return element;
    const found = findElement(element.children, id);
    if (found) return found;
  }
  return null;
}

/** Elements as Play shows them: visibility overrides and swapped content, same positions. */
export function playElements(project: Project, elements: Element[], play: PlayState): Element[] {
  if (!playIsActive(play)) return elements;
  const all = project.pages.flatMap(page => resolveForSize(project, page, page.sizes[0].id));
  const swapSource = (id: string): Element | null => {
    const sourceId = play.swaps[id];
    return sourceId ? findElement(all, sourceId) : null;
  };
  // Swapped-in children get stable synthetic ids, so React keys stay unique and no pointer
  // accidentally matches the visiting content.
  const swappedChildren = (source: Element, targetId: string): Element[] =>
    source.children.map((child, index) => ({
      ...child,
      id: `${targetId}:play:${index}`,
      children: swappedChildren(child, `${targetId}:play:${index}`),
    }));
  const walk = (list: Element[]): Element[] => list.map(element => {
    const hidden = play.hidden[element.id];
    const source = swapSource(element.id);
    const content = source
      ? { text: source.text, image: source.image, url: source.url, kind: source.kind, style: source.style }
      : {};
    return {
      ...element,
      ...content,
      visible: hidden === undefined ? element.visible : !hidden,
      children: source && source.children.length ? swappedChildren(source, element.id) : walk(element.children),
    };
  });
  return walk(elements);
}
