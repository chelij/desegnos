import type { Element, ElementKind, Pointer, Project } from '../types';
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

/** Playing a project made only of slides fills the canvas as a presentation. */
export const isPresenting = (project: Project, play: PlayState) =>
  playIsActive(play) && project.pages.length > 0 && project.pages.every(page => page.output === 'slide');

/** The pointer a click on this element runs in Play, or null when it runs nothing. */
function playPointerAt(project: Project, pageId: string, elementId: string): Pointer | null {
  for (const pointer of project.pointers ?? []) {
    if (pointer.from.kind !== 'element' || pointer.from.pageId !== pageId || pointer.from.elementId !== elementId) continue;
    if (pointer.play) return pointer;
  }
  return null;
}

/** The layer kinds a page URL turns into a hyperlink, matching the web export. */
const LINK_KINDS: ElementKind[] = ['box', 'text', 'button', 'link'];

/** The page a linked layer opens, when its url exactly names a boundary path. */
function linkTargetPage(project: Project, element: Element): string | null {
  if (!element.url || !LINK_KINDS.includes(element.kind)) return null;
  return project.pages.find(page => page.path === element.url)?.id ?? null;
}

/** The clicked layer and its ancestors as Play draws them, innermost first. */
function playChain(project: Project, pageId: string, elementId: string): Element[] {
  const page = project.pages.find(page => page.id === pageId);
  if (!page) return [];
  const elements = page.sizes[0] ? resolveForSize(project, page, page.sizes[0].id) : page.elements;
  const chain: Element[] = [];
  const walk = (list: Element[]): boolean => {
    for (const element of list) {
      chain.push(element);
      if (element.id === elementId) return true;
      if (walk(element.children)) return true;
      chain.pop();
    }
    return false;
  };
  return walk(elements) ? chain.reverse() : [];
}

/** One click's outcome: the next transient state and a boundary to move the camera to. */
export type PlayStep = { play: PlayState; goTo: string | null };

export function playClick(project: Project, play: PlayState, pageId: string, elementId: string): PlayStep {
  const chain = playChain(project, pageId, elementId);
  // The innermost layer carrying a Play pointer runs it, whichever layer was clicked.
  const pointer = chain.length
    ? chain.map(element => playPointerAt(project, pageId, element.id)).find((found): found is Pointer => !!found)
    : playPointerAt(project, pageId, elementId);
  if (!pointer?.play) {
    // With no Play pointer in the chain, the nearest page link opens its boundary.
    const target = chain.map(element => linkTargetPage(project, element)).find(id => id !== null) ?? null;
    return target ? { play: { ...play, boundary: target }, goTo: target } : { play, goTo: null };
  }
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
    } as Element;
  });
  return walk(elements);
}
