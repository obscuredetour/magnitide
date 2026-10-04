// Editing a book's metadata (Magnitide libraries only).

import { createSignal } from 'solid-js';

import { api, type BookEdit, coverChanged, errorMessage } from './api';
import { library, refresh, setEditRevision } from './library';
import { toast } from './toast';
import { writeOnSave } from './writeback';

/** The book being edited; with `queue`, one of several stepped through in order. */
export type Editing = { id: number; edit: BookEdit; queue?: number[] };

const [editing, setEditing] = createSignal<Editing | null>(null);
export const editingBook = editing;

/** Books being edited together (the bulk edit window). */
const [bulkIds, setBulkIds] = createSignal<number[] | null>(null);
export { bulkIds };

const READ_ONLY_EDIT =
  'This calibre library is read-only. Import it into a Magnitide library (library menu) to edit books.';

/** Open the edit dialog for a book (within `queue`, to step through several). */
export async function openEditor(id: number, queue?: number[]): Promise<void> {
  if (library()?.kind !== 'magnitide') {
    toast(READ_ONLY_EDIT, { error: true });
    return;
  }
  try {
    setEditing({ id, edit: await api.bookEdit(id), queue: queue && queue.length > 1 ? queue : undefined });
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}

/** ⌘E: one book opens the editor; several open the bulk editor. */
export function editBooks(ids: number[]): void {
  if (ids.length === 1) void openEditor(ids[0]!);
  else if (ids.length > 1) {
    if (library()?.kind !== 'magnitide') toast(READ_ONLY_EDIT, { error: true });
    else setBulkIds(ids);
  }
}

/** From the bulk editor: step through the books in the normal editor. */
export function editOneByOne(ids: number[]): void {
  setBulkIds(null);
  if (ids.length) void openEditor(ids[0]!, ids);
}

export function closeBulkEditor(): void {
  setBulkIds(null);
}

export function closeEditor(): void {
  setEditing(null);
}

/**
 * Save; resolves to an error message, or null on success. Then closes, or
 * with `then`, opens that book of the queue instead.
 */
export async function saveEdit(id: number, edit: BookEdit | null, then?: number): Promise<string | null> {
  const queue = editing()?.queue;
  if (edit === null) {
    // Nothing changed: just move.
    if (then !== undefined) await openEditor(then, queue);
    return null;
  }
  try {
    const outcome = await api.saveBookEdit(id, edit);
    let message = outcome.moved || outcome.renamedFiles ? 'Saved · files renamed to match' : 'Saved';
    // The edit is saved either way; a failed file write is only a warning.
    let warning: string | null = null;
    if (writeOnSave()) {
      try {
        const r = await api.writeMetadataToFiles(id);
        if (r.failed.length) warning = r.failed.join('; ');
        else if (r.written) message += ' · EPUB updated';
      } catch (err) {
        warning = errorMessage(err);
      }
    }
    await refresh();
    setEditRevision((n) => n + 1);
    if (then !== undefined) await openEditor(then, queue);
    else setEditing(null);
    if (warning) toast(`Saved, but the file couldn’t be updated: ${warning}`, { error: true, duration: 6000 });
    else toast(message);
    return null;
  } catch (err) {
    return errorMessage(err);
  }
}

/** Change or remove the cover now (not deferred to Save): from a file, or a metadata source's cover URL. */
export async function changeCover(id: number, action: 'choose' | 'remove' | { url: string }): Promise<void> {
  try {
    if (action === 'choose') {
      const path = await api.chooseImageFile();
      if (!path) return;
      await api.setCoverFromFile(id, path);
    } else if (action === 'remove') {
      await api.removeCover(id);
    } else {
      await api.setCoverFromUrl(id, action.url);
    }
    coverChanged(id);
    await refresh();
    setEditRevision((n) => n + 1);
    toast(action === 'remove' ? 'Cover removed' : 'Cover updated');
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}
