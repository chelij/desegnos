import type { Element, ElementOverride, Page, PageSize, Project, SizePreset } from '../types';
import { resolveElements } from './links';

// Linked sizes: one element tree per page drawn as one boundary per size. The first size is the
// base size; its values are what every other size inherits. The core resolves the same way in
// `crates/desegnos-core/src/elements.rs`; both sides must agree.

export const SIZE_PRESETS: { preset: SizePreset; label: string; width: number; height: number }[] = [
  { preset: 'phone', label: 'Phone', width: 390, height: 844 },
  { preset: 'tablet', label: 'Tablet', width: 820, height: 1180 },
  { preset: 'desktop', label: 'Desktop', width: 1440, height: 900 },
];

export const baseSize = (page: Page): PageSize => page.sizes[0];
export const baseSizeId = (page: Page): string => baseSize(page).id;
export const hasManySizes = (page: Page): boolean => page.sizes.length > 1;
export const findSize = (page: Page, sizeId: string | null | undefined): PageSize | null =>
  page.sizes.find(size => size.id === sizeId) ?? null;
export const isBaseSize = (page: Page, sizeId: string | null | undefined): boolean => !!sizeId && baseSizeId(page) === sizeId;

export const presetLabel = (preset: SizePreset, width = 0, height = 0): string =>
  preset === 'phone' ? 'Phone' : preset === 'tablet' ? 'Tablet' : preset === 'desktop' ? 'Desktop' : `${Math.round(width)} × ${Math.round(height)}`;

const PRESET_NAMES: Record<SizePreset, string> = { phone: 'Phone', tablet: 'Tablet', desktop: 'Desktop', custom: 'Custom' };

/**
 * A size only claims a preset name while its width still matches that preset; an edited width
 * reads as Custom, so a stored label can never describe a size the user no longer has.
 */
export function sizeLabel(width: number): string {
  const preset = SIZE_PRESETS.find(item => item.width === Math.round(width));
  return preset ? PRESET_NAMES[preset.preset] : 'Custom';
}

/** The name a boundary shows: a page with one size reads as the page, more sizes add the size. */
export function boundaryName(page: Page, size: PageSize): string {
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

/** A freshly serialized native snapshot with the same id/revision has the same design.
 * Share its immutable size views without retaining the previous full project snapshot. */
export function reuseResolvedTrees(source: Project, target: Project): void {
  const cache = resolvedTrees.get(source);
  if (!cache || cache.revision !== target.revision) return;
  const byId = new Map([...cache.pages].map(([page, sizes]) => [page.id, sizes]));
  resolvedTrees.set(target, { revision: target.revision, pages: new Map(target.pages.flatMap(page => {
    const sizes = byId.get(page.id);
    return sizes ? [[page, sizes] as const] : [];
  })) });
}

/** The design one linked size shows: linked-element resolution first, then size overrides. */
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

/** The page-local top-left of a layer inside one resolved tree. */
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

/** A layer hidden in this size that has not been dismissed draws a faint placeholder. */
export const overrideFor = (element: Element, sizeId: string): ElementOverride | null => element.overrides?.[sizeId] ?? null;
export const hiddenInSize = (element: Element, sizeId: string): boolean => !!element.overrides?.[sizeId]?.hidden;
export const placeholderInSize = (element: Element, sizeId: string): boolean => {
  const over = element.overrides?.[sizeId];
  return !!over?.hidden && !over.dismissed;
};

export const sameSizeRect = (first: PageSize, second: PageSize): boolean =>
  first.id === second.id && first.label === second.label && first.width === second.width && first.height === second.height && first.kind === second.kind;

/** CSS min-width inheritance: the nearest smaller named size supplies overrides. */
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
