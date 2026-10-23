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
