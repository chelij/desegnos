import type { BoundaryKind, OutputType, Page, Project } from '../types';
import type { IconName } from '../icons';
import { TEMPLATE_DEFAULTS } from '../generated/limits';
import { BOUNDARY_GAP } from './boundaryLayout';

export type BoundaryToolKind = 'page' | 'slide' | 'image';
export const boundaryKindLabel = (kind: BoundaryToolKind) => ({ page: 'Page', slide: 'Slide', image: 'Image' })[kind];
export const boundaryIcon = (kind: BoundaryToolKind): IconName => ({ page: 'canvas', slide: 'rectangle', image: 'image' } as const)[kind];
// The saved page/screen values describe height behaviour, not separate output types.
export const layoutKind = (kind: BoundaryToolKind, project: Project): BoundaryKind => kind !== 'page' ? 'screen'
  : project.pages.find(page => pageKind(page) === 'page')?.sizes[0].kind ?? TEMPLATE_DEFAULTS[project.template ?? 'blank'].page.kind;
export const outputKind = (kind: BoundaryToolKind): OutputType => ({ page: 'webpage', slide: 'slide', image: 'image' } as const)[kind];
export const pageKind = (page: Page): BoundaryToolKind => page.output === 'slide' || page.output === 'image' ? page.output : 'page';
export function projectKind(project: Project): BoundaryToolKind {
  return TEMPLATE_DEFAULTS[project.template ?? 'blank'].output === 'webpage' ? 'page' : TEMPLATE_DEFAULTS[project.template ?? 'blank'].output as BoundaryToolKind;
}
export function boundaryDefaults(project: Project, kind: BoundaryToolKind) {
  const pages = project.pages.filter(page => pageKind(page) === kind);
  const first = pages[0]?.sizes[0];
  const preset = kind === 'page' ? TEMPLATE_DEFAULTS[project.template ?? 'blank'].page : TEMPLATE_DEFAULTS[kind === 'slide' ? 'slideDeck' : 'socialImage'];
  const width = (kind === 'page' ? first?.width : undefined) ?? preset.width;
  const height = (kind === 'page' ? first?.height : undefined) ?? preset.height;
  const last = [...pages].sort((a, b) => Math.max(...a.sizes.map(size => size.x + size.width)) - Math.max(...b.sizes.map(size => size.x + size.width))).at(-1);
  return { x: last ? Math.max(...last.sizes.map(size => size.x + size.width)) + BOUNDARY_GAP : 0, y: last?.sizes[0].y ?? 0, width, height };
}
export function nextBoundaryName(project: Project, kind: BoundaryToolKind) {
  const prefix = boundaryKindLabel(kind);
  const numbers = project.pages.map(page => page.name.startsWith(`${prefix} `) ? Number(page.name.slice(prefix.length + 1)) : 0).filter(Number.isInteger);
  return `${prefix} ${Math.max(0, ...numbers) + 1}`;
}
