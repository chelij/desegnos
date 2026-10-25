// Which preview document the editor may measure.
//
// A box measured inside a preview document is already in page coordinates: the canvas
// scales and offsets the whole frame, never the box. It is therefore only valid when it
// comes from the frame the canvas is showing. The hidden pending frame and a frame from
// an earlier revision describe layouts that are not on screen, so a rectangle measured
// there draws the selection outline, handles and toolbar away from the element.
export type PreviewIdentity = { pageId: string; revision: string | number; session: string };
export type PromotedFrame<Document> = PreviewIdentity & { document: Document };
export type VisibleFrame<Document> = PreviewIdentity & { document: Document | null; invalidated: boolean };

export function measurableDocument<Document>(promoted: PromotedFrame<Document> | null, visible: VisibleFrame<Document>): Document | null {
  if (!promoted || !visible.document || visible.invalidated) return null;
  if (promoted.pageId !== visible.pageId || promoted.revision !== visible.revision || promoted.session !== visible.session) return null;
  return promoted.document === visible.document ? promoted.document : null;
}

// The size the hidden pending frame is laid out at, and the size it is promoted into.
//
// The pending frame has to be laid out at the extent the visible frame already has, so
// viewport-relative layout (100vh) inside it matches what the canvas is showing. If the
// promotion commit resizes the frame that was just measured, that reading still describes
// the pending frame's layout and draws the outline at a wrong offset until the next
// measurement corrects it.
export type ShownExtent = { pageId: string; extent: number };

export function pendingFrameHeight(shown: ShownExtent | null, pageId: string, referenceHeight: number): number {
  return shown && shown.pageId === pageId ? shown.extent : referenceHeight;
}

// The height a pending document is promoted into, from the content height measured at the
// size the pending frame was laid out at. Viewport-relative content reports its own frame
// back, so the frame keeps the size it was measured in; content that is taller or shorter
// than the reference viewport decides the height itself.
export function promotionHeight(referenceHeight: number, contentHeight: number): number {
  return Math.min(100000, Math.max(referenceHeight, contentHeight));
}

// Whether the content height has to be decided against the reference height instead. A
// frame taller than the reference viewport that still overflows is feeding its own layout
// (100vh content plus anything outside it), so measuring it there would grow the frame on
// every reload. One measurement at the reference height keeps that height stable.
export function needsReferenceMeasure(stagedHeight: number, referenceHeight: number, contentHeight: number): boolean {
  return stagedHeight !== referenceHeight && contentHeight > stagedHeight;
}

// The box to draw for a selected element.
//
// A promoted frame is measured twice: once when it is recorded as the shown frame and
// again once its layout has settled. Only the settled measurement describes the frame
// the user ends up looking at, because the first one can still belong to the layout the
// pending frame was built in. Until the settled box arrives the canvas keeps drawing the
// box its own edit aimed at (the drag target, or the previous box moved by a nudge), so
// the outline never jumps to an interim offset and never has to jump back.
export function shownSelectionRect<Rect>(measured: Rect, aimed: Rect, settled: boolean): Rect {
  return settled ? measured : aimed;
}

// The boxes to draw for a whole selection.
//
// Every element an edit moved or resized keeps the box that edit aimed at until the settled
// measurement arrives, and an element without an aimed box keeps the box already drawn. Once
// settled, only the measurements are drawn, so a multi-selection never shows one interim
// element while the others are final.
export function shownSelectionRects<Rect>(measured: Rect[], aimed: Array<Rect | null | undefined>, settled: boolean): Rect[] {
  return measured.map((rect, index) => shownSelectionRect(rect, aimed[index] ?? rect, settled));
}
