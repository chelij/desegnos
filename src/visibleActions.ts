import type { Project } from './types';

// Rules for conditional controls. A control is left out of a surface when, in the current
// state, it cannot change what the user sees: it opens what is already open, repeats an
// action that is one step away on screen, or applies something already applied. Menus and
// keyboard shortcuts keep every command reachable.

export type RequestEditor = { selectedElementId: string | null; focusedElementId: string | null };

/// A request's editor is open when that request is selected and its prompt holds focus.
/// Asking to change that same request would only put the caret back where it already is.
export function requestEditorOpen(elementId: string | null | undefined, editor: RequestEditor): boolean {
  return !!elementId && elementId === editor.selectedElementId && elementId === editor.focusedElementId;
}

export type OpenerState = { available: boolean; open: boolean };

/// An opener is left out while the surface it opens is already on screen; that surface has
/// its own Close. The same rule hides a Details action while the AI log is open.
export function showsOpener({ available, open }: OpenerState): boolean {
  return available && !open;
}

/// Preview renders exported web output, so a project without any web page has nothing to show.
export function projectHasPreviewPages(project: Pick<Project, 'pages'> | null | undefined): boolean {
  return !!project?.pages.some(page => page.output === 'webpage' || page.output === 'appScreen');
}
