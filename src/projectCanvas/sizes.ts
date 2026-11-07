import type { Element, Page, PageSize, Project, SizePreset } from '../types';
import { resolveElements } from './links';

// Linked sizes: one element tree per page drawn as one boundary per size. The first size is the
// base size; its values are what every other size inherits. The core resolves the same way in
// `crates/desegnos-core/src/elements.rs`; both sides must agree.

export const SIZE_PRESETS: { preset: SizePreset; label: string; width: number; height: number }[] = [
  { preset: 'phone', label: 'Phone', width: 390, height: 844 },
  { preset: 'tablet', label: 'Tablet', width: 820, height: 1180 },
  { preset: 'desktop', label: 'Desktop', width: 1440, height: 900 },
];

export const baseSizeId = (page: Page): string => page.sizes[0].id;
export const hasManySizes = (page: Page): boolean => page.sizes.length > 1;
export function sizeLabel(width: number): string {
  return width >= 1024 ? 'Desktop' : width >= 700 ? 'Tablet' : 'Phone';
}

export function boundaryLabel(page: Page, size: PageSize): string {
  return page.sizes.length > 1 ? `${page.name} · ${sizeLabel(size.width)}` : page.name;
}

export function applyOverrides(elements: Element[], sizeId: string) {
  for (const element of elements) {
    const over = element.overrides?.[sizeId];
    if (over) {
      if (over.rect) element.rect = { ...over.rect };
      if (over.style) element.style = structuredClone(over.style);
      if (over.layout) element.layout = structuredClone(over.layout);
      if (over.hidden) element.visible = false;
    }
    applyOverrides(element.children, sizeId);
  }
}

// Cached results are immutable views. The project object scopes lifetime; revision invalidates
// views even when a caller deliberately reuses an object while loading a new revision.
const resolvedTrees = new WeakMap<Project, { revision: number; pages: Map<Page, Map<string, Element[]>> }>();

export function reuseResolvedTrees(source: Project, target: Project): void {
  const cache = resolvedTrees.get(source);
  if (!cache || cache.revision !== target.revision) return;
  const byId = new Map([...cache.pages].map(([page, sizes]) => [page.id, sizes]));
  resolvedTrees.set(target, { revision: target.revision, pages: new Map(target.pages.flatMap(page => {
    const sizes = byId.get(page.id);
    return sizes ? [[page, sizes] as const] : [];
  })) });
}

export function resolveForSize(project: Project, page: Page, sizeId: string): Element[] {
  let cache = resolvedTrees.get(project);
  if (!cache || cache.revision !== project.revision) {
    cache = { revision: project.revision, pages: new Map() };
    resolvedTrees.set(project, cache);
  }
  let sizes = cache.pages.get(page);
  if (!sizes) { sizes = new Map(); cache.pages.set(page, sizes); }
  const cached = sizes.get(sizeId);
  if (cached) return cached;
  // A copy, so drawing one size never writes an override back into the project state.
  const elements = structuredClone(resolveElements(project, page.elements));
  applyOverrides(elements, sizeId);
  resolveSharedStyles(project, elements);
  sizes.set(sizeId, elements);
  return elements;
}

export function elementOrigin(elements: Element[], elementId: string): { x: number; y: number } | null {
  const walk = (children: Element[], base: { x: number; y: number }): { x: number; y: number } | null => {
    for (const child of children) {
      const origin = { x: base.x + child.rect.x, y: base.y + child.rect.y };
      if (child.id === elementId) return origin;
      const found = walk(child.children, origin);
      if (found) return found;
    }
    return null;
  };
  return walk(elements, { x: 0, y: 0 });
}

export function sizeAtWidth(page: Page, width: number): PageSize {
  const sizes = [...page.sizes].sort((a, b) => a.width - b.width || a.id.localeCompare(b.id));
  return sizes.filter(size => size.width <= width).at(-1) ?? sizes[0];
}
export function resolveSharedStyles(project: Project, elements: Element[]): void {
  if (!project.styles) return;
  for (const element of elements) {
    const style = element.style, refs = style.references;
    const colour = (id: string | null | undefined) => project.styles?.colours.find(item => item.id === id)?.value;
    const fill = colour(refs?.fill), stroke = colour(refs?.stroke), textColour = colour(refs?.textColour);
    if (fill) style.fill = fill;
    if (stroke) style.stroke.color = stroke;
    if (textColour) style.textStyle.color = textColour;
    const text = project.styles?.text.find(item => item.id === refs?.text);
    if (text) { const { id: _id, name: _name, ...properties } = text; Object.assign(style.textStyle, properties); }
    resolveSharedStyles(project, element.children);
  }
}
