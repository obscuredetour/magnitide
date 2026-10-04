// Editing a book's metadata (Magnitide libraries only).

import { createSignal } from 'solid-js';

import { api, type BookEdit, coverChanged, errorMessage } from './api';
import { library, refresh, setEditRevision } from './library';
import { toast } from './toast';

export type Editing = { id: number; edit: BookEdit };

const [editing, setEditing] = createSignal<Editing | null>(null);
export const editingBook = editing;

/** Open the edit dialog for a book. */
export async function openEditor(id: number): Promise<void> {
  if (library()?.kind !== 'magnitide') {
    toast('This calibre library is read-only. Import it into a Magnitide library (library menu) to edit books.', {
      error: true,
    });
    return;
  }
  try {
    setEditing({ id, edit: await api.bookEdit(id) });
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}

export function closeEditor(): void {
  setEditing(null);
}

/** Save; resolves to an error message, or null on success (and closes). */
export async function saveEdit(id: number, edit: BookEdit): Promise<string | null> {
  try {
    const outcome = await api.saveBookEdit(id, edit);
    await refresh();
    setEditRevision((n) => n + 1);
    setEditing(null);
    toast(outcome.moved || outcome.renamedFiles ? 'Saved · files renamed to match' : 'Saved');
    return null;
  } catch (err) {
    return errorMessage(err);
  }
}

/** Change or remove the cover now (not deferred to Save). */
export async function changeCover(id: number, action: 'choose' | 'remove'): Promise<void> {
  try {
    if (action === 'choose') {
      const path = await api.chooseImageFile();
      if (!path) return;
      await api.setCoverFromFile(id, path);
    } else {
      await api.removeCover(id);
    }
    coverChanged(id);
    await refresh();
    setEditRevision((n) => n + 1);
    toast(action === 'choose' ? 'Cover updated' : 'Cover removed');
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}
