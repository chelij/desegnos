import { useEffect, useRef } from 'react';
import { useDraftText } from '../DraftText';
import type { CanvasTarget } from '../projectCanvasGeometry';
import type { ReferenceItem, ReferenceKind, Project, Rect } from '../types';
import { ElementView } from './elements';
import { resolveElement } from './links';
import { applyOverrides, resolveSharedStyles } from './sizes';
import { hostname } from './surface';
import { MAX_PROMPT_CHARS } from '../generated/limits';

export const shownReference = (item: ReferenceItem, movingItems: Record<string, Rect>): Rect => movingItems[item.id] ?? item.rect;

export function pictureSource(project: Project, picture: string): string {
  return project.pictures?.[picture]
    ?? project.importedIdentity?.assets.find(asset => asset.id === picture)?.dataUri
    ?? project.importedIdentity?.photos?.find(photo => photo.id === picture)?.dataUri
    ?? '';
}

export function ReferenceLayer({ project, movingItems, selection, draftNote }: {
  project: Project; movingItems: Record<string, Rect>; selection: CanvasTarget[]; draftNote: Rect | null;
}) {
  return <div className="reference-layer">
    {(project.reference ?? []).map(item => {
      const rect = shownReference(item, movingItems);
      const isSelected = selection.some(target => target.kind === 'reference' && target.itemId === item.id);
      const element = item.kind.kind === 'element' ? structuredClone(resolveElement(project, item.kind.element)) : null;
      if (element) {
        if (item.kind.kind === 'element' && item.kind.size) applyOverrides([element], item.kind.size);
        element.rect = { ...rect, x: 0, y: 0 };
        resolveSharedStyles(project, [element]);
      }
      return <div key={item.id} data-reference={item.id} className={`reference-item reference-${item.kind.kind}${isSelected ? ' selected' : ''}`}
        style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
        title={item.kind.kind === 'element' ? item.kind.element.name || item.kind.element.kind : item.kind.kind === 'note' ? item.kind.text : item.kind.kind === 'link' ? item.kind.url : item.kind.kind === 'image' ? item.kind.name : item.kind.color}>
        {element && <div style={{ pointerEvents: 'none' }}><ElementView project={project} pageId="" element={element} flow={false} editingId={null} /></div>}
        {item.kind.kind === 'note' && <span className="reference-note-text">{item.kind.text}</span>}
        {item.kind.kind === 'link' && <><span className="reference-link-host">{hostname(item.kind.url)}</span><span className="reference-link-title">{item.kind.title || hostname(item.kind.url)}</span></>}
        {item.kind.kind === 'image' && <><img src={pictureSource(project, item.kind.picture)} alt={item.kind.name || 'Picture'} draggable={false} /><span className="reference-image-name">{item.kind.name || 'Picture'}</span></>}
        {item.kind.kind === 'swatch' && <><span className="reference-swatch" style={{ background: item.kind.color }} /><span className="reference-swatch-label">{item.kind.color}</span></>}
      </div>;
    })}
    {draftNote && <div className="reference-draft" style={{ left: draftNote.x, top: draftNote.y, width: draftNote.width, height: draftNote.height }} />}
  </div>;
}

// The active note editor is native screen pixels, never inside the transformed layer.
export function ReferenceNoteEditor({ item, kind, screen, autoFocus, change, close }: {
  item: ReferenceItem; kind: Extract<ReferenceKind, { kind: 'note' }>; screen: { left: number; top: number; width: number; height: number }; autoFocus: boolean;
  change: (text: string) => void; close: () => void;
}) {
  const text = useRef<HTMLTextAreaElement>(null);
  const [draft, changeDraft, flushDraft] = useDraftText(kind.text, (_value, latest) => Promise.resolve(change(latest())).then(() => latest()), 400);
  useEffect(() => { if (autoFocus) text.current?.focus({ preventScroll: true }); }, [autoFocus, item.id]);
  return <div className="reference-note-editor" style={{ left: screen.left, top: screen.top, width: Math.max(160, screen.width), height: Math.min(Math.max(64, screen.height), 160) }} onPointerDown={event => event.stopPropagation()}>
    <textarea ref={text} aria-label="Reference note" maxLength={MAX_PROMPT_CHARS} value={draft} placeholder="Reference note" spellCheck
      style={{ fontSize: 12 }}
      onChange={event => changeDraft(event.target.value)} onBlur={flushDraft}
      onKeyDown={event => {
        if (event.key !== 'Escape') return;
        event.preventDefault(); event.stopPropagation();
        flushDraft(); text.current?.blur(); close();
      }} />
  </div>;
}
