import type { BoundaryKind, OutputType, Page, Project } from '../types';
import type { IconName } from '../icons';
import { SIZE_PRESETS } from '../generated/limits';
import { BOUNDARY_GAP } from '../projectCanvasGeometry';

export type BoundaryToolKind = 'page' | 'slide' | 'image';
export const boundaryKindLabel = (kind: BoundaryToolKind) => ({ page: 'Page', slide: 'Slide', image: 'Image' })[kind];
export const boundaryName = boundaryKindLabel;
export const boundaryIcon = (kind: BoundaryToolKind): IconName => ({ page: 'canvas', slide: 'rectangle', image: 'image' } as const)[kind];
// The saved page/screen values describe height behaviour, not separate output types.
export const layoutKind = (kind: BoundaryToolKind, project: Project): BoundaryKind => kind !== 'page' ? 'screen'
  : project.pages.find(page => pageKind(page) === 'page')?.sizes[0].kind ?? (project.template === 'appScreen' ? 'screen' : 'page');
export const outputKind = (kind: BoundaryToolKind): OutputType => ({ page: 'webpage', slide: 'slide', image: 'image' } as const)[kind];
export const pageKind = (page: Page): BoundaryToolKind => page.output === 'slide' || page.output === 'image' ? page.output : 'page';
export function projectKind(project: Project): BoundaryToolKind {
  return ({ blank: 'page', website: 'page', appScreen: 'page', slideDeck: 'slide', socialImage: 'image' } as const)[project.template ?? 'blank'];
}
export function boundaryDefaults(project: Project, kind: BoundaryToolKind) {
  const pages = project.pages.filter(page => pageKind(page) === kind);
  const first = pages[0]?.sizes[0];
  const width = kind === 'slide' ? 1920 : kind === 'image' ? 1080 : first?.width ?? (project.template === 'appScreen' ? SIZE_PRESETS.phone.width : SIZE_PRESETS.desktop.width);
  const height = kind === 'slide' ? 1080 : kind === 'image' ? 1080 : first?.height ?? (project.template === 'appScreen' ? SIZE_PRESETS.phone.height : SIZE_PRESETS.desktop.height);
  const last = [...pages].sort((a, b) => Math.max(...a.sizes.map(size => size.x + size.width)) - Math.max(...b.sizes.map(size => size.x + size.width))).at(-1);
  return { x: last ? Math.max(...last.sizes.map(size => size.x + size.width)) + BOUNDARY_GAP : 0, y: last?.sizes[0].y ?? 0, width, height };
}
export function nextBoundaryName(project: Project, kind: BoundaryToolKind) {
  const prefix = boundaryKindLabel(kind);
  const numbers = project.pages.map(page => page.name.startsWith(`${prefix} `) ? Number(page.name.slice(prefix.length + 1)) : 0).filter(Number.isInteger);
  return `${prefix} ${Math.max(0, ...numbers) + 1}`;
}
