import type { BoundaryKind, OutputType, Page, Project } from '../types';
import type { IconName } from '../icons';
import { BOUNDARY_GAP } from '../projectCanvasGeometry';

export type BoundaryToolKind = 'page' | 'screen' | 'slide' | 'image';
export const boundaryName = (kind: BoundaryToolKind) => ({ page: 'Page', screen: 'Screen', slide: 'Slide', image: 'Image' })[kind];
export const boundaryIcon = (kind: BoundaryToolKind): IconName => ({ page: 'canvas', screen: 'phone', slide: 'rectangle', image: 'image' } as const)[kind];
export const layoutKind = (kind: BoundaryToolKind): BoundaryKind => kind === 'page' ? 'page' : 'screen';
export const outputKind = (kind: BoundaryToolKind): OutputType => ({ page: 'webpage', screen: 'appScreen', slide: 'slide', image: 'image' } as const)[kind];
export const pageKind = (page: Page): BoundaryToolKind => page.output === 'slide' || page.output === 'image' ? page.output : page.output === 'appScreen' || page.sizes[0].kind === 'screen' ? 'screen' : 'page';
export function projectKind(project: Project): BoundaryToolKind {
  return ({ blank: 'page', website: 'page', appScreen: 'screen', slideDeck: 'slide', socialImage: 'image' } as const)[project.template ?? 'blank'];
}
export function boundaryDefaults(project: Project, kind: BoundaryToolKind) {
  const pages = project.pages.filter(page => pageKind(page) === kind);
  const first = pages[0]?.sizes[0];
  const width = kind === 'slide' ? 1920 : kind === 'image' ? 1080 : first?.width ?? (kind === 'screen' ? 390 : 1440);
  const height = kind === 'slide' ? 1080 : kind === 'image' ? 1080 : first?.height ?? (kind === 'screen' ? 844 : 900);
  const last = [...pages].sort((a, b) => Math.max(...a.sizes.map(size => size.x + size.width)) - Math.max(...b.sizes.map(size => size.x + size.width))).at(-1);
  return { x: last ? Math.max(...last.sizes.map(size => size.x + size.width)) + BOUNDARY_GAP : 0, y: last?.sizes[0].y ?? 0, width, height };
}
export function nextBoundaryName(project: Project, kind: BoundaryToolKind) {
  const prefix = boundaryName(kind);
  const numbers = project.pages.map(page => page.name.startsWith(`${prefix} `) ? Number(page.name.slice(prefix.length + 1)) : 0).filter(Number.isInteger);
  return `${prefix} ${Math.max(0, ...numbers) + 1}`;
}
