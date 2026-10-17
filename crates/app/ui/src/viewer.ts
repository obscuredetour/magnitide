// The comic viewer: which book is open in it (a full-window overlay, see
// components/ComicViewer.tsx), and opening a book there or in its own app.

import { createSignal } from 'solid-js';

import { isComicFormat } from './fields';
import { items, openBook } from './library';

const [reading, setReading] = createSignal<number | null>(null);
export { reading };

/** Read a comic in the viewer. */
export function readComic(id: number): void {
  setReading(id);
}

export function closeViewer(): void {
  setReading(null);
}

/** Whether a book has a file the viewer reads (CBZ, CB7, CBR). */
export function isReadable(id: number): boolean {
  return (
    items()
      .find((b) => b.id === id)
      ?.formats.some(isComicFormat) ?? false
  );
}

/** Opening a book: a comic in the viewer, anything else in its own app. */
export function openOrRead(id: number): void {
  if (isReadable(id)) readComic(id);
  else void openBook(id);
}
