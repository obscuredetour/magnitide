// Dragging books (cover tiles or table rows) onto a shelf in the sidebar.
// Pointer events, not HTML5 drag and drop, which the window's file drop
// (adding books) intercepts on some platforms.

import { createSignal } from 'solid-js';

export type BookDrag = {
  ids: number[];
  /** Pointer position, for the drag image. */
  x: number;
  y: number;
  /** The drop target under the pointer: a shelf id, 'new' (make a shelf), or null. */
  over: number | 'new' | null;
};

const [drag, setDrag] = createSignal<BookDrag | null>(null);
export { drag };

/** A drag ends in a click on the book; this keeps it from changing the selection. */
let justDragged = false;
export const dragJustEnded = () => justDragged;

/** Where the pointer is: an element marked data-shelf-drop="<id>" or "new". */
function dropTarget(x: number, y: number): BookDrag['over'] {
  const el = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-shelf-drop]');
  const v = el?.dataset.shelfDrop;
  if (v === undefined) return null;
  return v === 'new' ? 'new' : Number(v);
}

/**
 * Start tracking a press on a book; it becomes a drag after a few pixels of
 * movement. `ids` gives the books to drag (the selection, if the book is in
 * it). `onDrop` gets the target when released over one.
 */
export function startBookDrag(
  e: PointerEvent,
  ids: () => number[],
  onDrop: (target: number | 'new', ids: number[]) => void,
): void {
  // Modifier clicks are for selecting.
  if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  const startX = e.clientX;
  const startY = e.clientY;
  let dragged: number[] | null = null;

  const move = (ev: PointerEvent) => {
    if (!dragged) {
      if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < 6) return;
      dragged = ids();
      document.documentElement.dataset.draggingBooks = '';
    }
    setDrag({ ids: dragged, x: ev.clientX, y: ev.clientY, over: dropTarget(ev.clientX, ev.clientY) });
  };
  const finish = (drop: boolean) => {
    removeEventListener('pointermove', move);
    removeEventListener('pointerup', up);
    removeEventListener('pointercancel', cancel);
    removeEventListener('keydown', key, true);
    delete document.documentElement.dataset.draggingBooks;
    const d = drag();
    setDrag(null);
    if (!dragged) return;
    justDragged = true;
    setTimeout(() => (justDragged = false));
    if (drop && d?.over != null) onDrop(d.over, d.ids);
  };
  const up = () => finish(true);
  const cancel = () => finish(false);
  const key = (ev: KeyboardEvent) => {
    if (ev.key === 'Escape' && dragged) {
      ev.preventDefault();
      ev.stopPropagation();
      finish(false);
    }
  };
  addEventListener('pointermove', move);
  addEventListener('pointerup', up);
  addEventListener('pointercancel', cancel);
  addEventListener('keydown', key, true);
}
