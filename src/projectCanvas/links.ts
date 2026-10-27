import type { Element, LinkedDefinition, Project } from '../types';

// Linked elements: one named definition every copy shares. A copy keeps its own id, position,
// parent, visibility and request history; the definition holds everything else. The core does
// the same work in `crates/desegnos-core/src/elements.rs`; both sides must agree on the ids.

/** FNV-1a over each path step, so a layer has the same id in both languages. */
function pathHash(path: number[]): number {
  let value = 0x811c9dc5;
  for (const step of path) {
    let rest = step >>> 0;
    for (let index = 0; index < 8; index += 1) {
      value = (value ^ (rest & 0xff)) >>> 0;
      value = Math.imul(value, 0x01000193) >>> 0;
      rest = Math.floor(rest / 256);
    }
  }
  return (value ^ 0xa5a5a5a5) >>> 0;
}

/** The id of a layer inside one copy, from its path in the definition. */
export function nestedElementId(root: string, path: number[]): string {
  const hex = root.replace(/-/g, '').padEnd(32, '0');
  const tail = pathHash(path).toString(16).padStart(8, '0');
  const body = `${hex.slice(0, 24)}${tail}`;
  return `${body.slice(0, 8)}-${body.slice(8, 12)}-${body.slice(12, 16)}-${body.slice(16, 20)}-${body.slice(20, 32)}`;
}

function assignNestedIds(root: string, element: Element, path: number[]) {
  element.children.forEach((child, index) => {
    path.push(index);
    child.id = nestedElementId(root, path);
    child.link = null;
    assignNestedIds(root, child, path);
    path.pop();
  });
}

export function linkedDefinition(project: Project, element: Element | null | undefined): LinkedDefinition | null {
  const id = element?.link;
  return id ? project.linked?.[id] ?? null : null;
}

/** A copy's concrete tree: its own id and position, the definition's content. */
export function resolveElement(project: Project, element: Element): Element {
  const definition = linkedDefinition(project, element);
  if (!definition) {
    if (!element.children.length) return element;
    const children = element.children.map(child => resolveElement(project, child));
    return children.every((child, index) => child === element.children[index]) ? element : { ...element, children };
  }
  const concrete: Element = structuredClone(definition.master);
  concrete.id = element.id;
  concrete.rect = { ...concrete.rect, x: element.rect.x, y: element.rect.y };
  concrete.visible = element.visible;
  concrete.locked = element.locked;
  concrete.meta = structuredClone(element.meta);
  // The copy's own size overrides apply after resolution, exactly like a plain layer's.
  concrete.overrides = element.overrides ? structuredClone(element.overrides) : {};
  concrete.link = element.link ?? null;
  concrete.name = definition.name;
  assignNestedIds(element.id, concrete, []);
  return concrete;
}

export function resolveElements(project: Project, elements: Element[]): Element[] {
  return elements.map(element => resolveElement(project, element));
}

/** The name Layers and Properties show for a copy. */
export function linkedName(project: Project, element: Element): string | null {
  return linkedDefinition(project, element)?.name ?? null;
}

/** Every layer id that comes from a definition; their order and parent are shared. */
export function nestedLayerIds(project: Project): Set<string> {
  const ids = new Set<string>();
  if (!project.linked) return ids;
  const walk = (root: string, elements: Element[], path: number[]) => {
    elements.forEach((child, index) => {
      path.push(index);
      ids.add(nestedElementId(root, path));
      walk(root, child.children, path);
      path.pop();
    });
  };
  const visit = (elements: Element[]) => {
    for (const element of elements) {
      const definition = linkedDefinition(project, element);
      if (definition) walk(element.id, definition.master.children, []);
      visit(element.children);
    }
  };
  for (const page of project.pages) visit(page.elements);
  return ids;
}

export const sameKind = (first: Element, second: Element) => first.kind === second.kind;

/** Same kind at every node and the same children structure; content may differ. */
export function sameStructure(first: Element, second: Element): boolean {
  return sameKind(first, second)
    && first.children.length === second.children.length
    && first.children.every((child, index) => sameStructure(child, second.children[index]));
}

function sameContent(first: Element, second: Element): boolean {
  return JSON.stringify(content(first)) === JSON.stringify(content(second))
    && first.children.every((child, index) => sameContent(child, second.children[index]));
}

function content(element: Element) {
  return {
    kind: element.kind, style: element.style, text: element.text, image: element.image, url: element.url,
    layout: element.layout, rotation: element.rotation, inputType: element.inputType,
    options: element.options, language: element.language, source: element.source,
  };
}

/** Copies that differ in what they show need the user to pick the copy that wins. */
export function linkNeedsChoice(elements: Element[]): boolean {
  return elements.some(element => !sameContent(element, elements[0]));
}

/** A short, thumbnail-free description of a copy for the link and paste choices. */
export function copySummary(element: Element): string {
  const name = element.name?.trim();
  if (name) return name.length > 48 ? `${name.slice(0, 47)}…` : name;
  const text = element.text.trim().split('\n')[0];
  if (text) return text.length > 48 ? `${text.slice(0, 47)}…` : text;
  if (element.kind === 'image') return 'Picture';
  if (element.kind === 'code') return `${element.language || 'Code'} code`;
  return element.kind.charAt(0).toUpperCase() + element.kind.slice(1);
}

/** True when a layer only exists inside a linked element's definition. */
export function isNestedLayer(project: Project, id: string): boolean {
  if (!project.linked) return false;
  const walk = (root: string, elements: Element[], path: number[]): boolean => elements.some((child, index) => {
    path.push(index);
    const found = nestedElementId(root, path) === id || walk(root, child.children, path);
    path.pop();
    return found;
  });
  const visit = (elements: Element[]): boolean => elements.some(element => {
    const definition = linkedDefinition(project, element);
    return (!!definition && walk(element.id, definition.master.children, [])) || visit(element.children);
  });
  return project.pages.some(page => visit(page.elements));
}
