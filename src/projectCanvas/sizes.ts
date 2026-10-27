import type { Element, ElementOverride, Page, PageSize, Project, SizePreset } from '../types';
import { resolveElements } from './links';

// Linked sizes: one element tree per page drawn as one boundary per size. The first size is the
// base size; its values are what every other size inherits. The core resolves the same way in
// `crates/desegnos-core/src/elements.rs`; both sides must agree.

export const SIZE_PRESETS: { preset: SizePreset; label: string; width: number; height: number }[] = [
  { preset: 'phone', label: 'Add Phone', width: 390, height: 844 },
  { preset: 'tablet', label: 'Add Tablet', width: 820, height: 1180 },
  { preset: 'desktop', label: 'Add Desktop', width: 1440, height: 900 },
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

function applyOverrides(elements: Element[], sizeId: string) {
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

/** The design one linked size shows: linked-element resolution first, then size overrides. */
export function resolveForSize(project: Project, page: Page, sizeId: string): Element[] {
  // A copy, so drawing one size never writes an override back into the project state.
  const elements = structuredClone(resolveElements(project, page.elements));
  applyOverrides(elements, sizeId);
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
