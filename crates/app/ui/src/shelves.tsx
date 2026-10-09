// Shelves: the list, filtering the library by one, and putting books on and
// taking them off (by menu or by dragging covers onto the sidebar).

import { createSignal } from 'solid-js';

import { api, errorMessage, type Shelf, type ShelfSource } from './api';
import { confirm } from './confirm';
import { library, query, refresh, search, setEditRevision } from './library';
import { toast } from './toast';
import { plural } from './writeback';

const [shelves, setShelves] = createSignal<Shelf[]>([]);
export { shelves };

/** Reload the list (counts change as books move). */
export async function refreshShelves(): Promise<void> {
  try {
    setShelves(library()?.kind === 'magnitide' ? await api.listShelves() : []);
  } catch (err) {
    console.warn('shelves:', errorMessage(err));
  }
}

const quoted = (name: string) => `shelf:"${name.replace(/"/g, '\\"')}"`;

/** The shelf the library is filtered to, if the search is exactly one shelf. */
export const activeShelf = (): Shelf | undefined => shelves().find((s) => quoted(s.name) === query().trim());

/** Show one shelf's books, or everything (null). */
export function showShelf(shelf: Shelf | null): void {
  search(shelf ? quoted(shelf.name) : '');
}

async function afterChange(): Promise<void> {
  await Promise.all([refresh(), refreshShelves()]);
  setEditRevision((n) => n + 1);
}

export async function addToShelf(shelf: Shelf, ids: number[]): Promise<void> {
  try {
    const added = await api.addToShelf(shelf.id, ids);
    await afterChange();
    toast(
      added
        ? `Added ${plural(added, 'book', 'books')} to “${shelf.name}”`
        : `${ids.length === 1 ? 'Already' : 'All already'} on “${shelf.name}”`,
    );
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}

export async function removeFromShelf(shelf: Shelf, ids: number[]): Promise<void> {
  try {
    const removed = await api.removeFromShelf(shelf.id, ids);
    await afterChange();
    toast(`Removed ${plural(removed, 'book', 'books')} from “${shelf.name}”`);
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}

/** Create a shelf; with `ids`, put those books on it too. */
export async function createShelf(name: string, ids: number[] = []): Promise<Shelf | null> {
  try {
    const shelf = await api.createShelf(name);
    if (ids.length) await api.addToShelf(shelf.id, ids);
    await afterChange();
    if (ids.length) toast(`Added ${plural(ids.length, 'book', 'books')} to “${shelf.name}”`);
    return shelf;
  } catch (err) {
    toast(errorMessage(err), { error: true });
    return null;
  }
}

/** Ask for a name, then create the shelf (with `ids` on it). */
export async function newShelfFor(ids: number[]): Promise<void> {
  const [name, setName] = createSignal('');
  const ok = await confirm({
    title: 'New Shelf',
    message: () => (
      <>
        <input
          class="h-8 w-full rounded-control border border-line bg-field px-2.5 text-fg outline-none select-text focus:border-focus focus:ring-3 focus:ring-focus/20"
          placeholder="Shelf name"
          aria-label="Shelf name"
          autofocus
          onInput={(e) => setName(e.currentTarget.value)}
          onKeyDown={(e) => {
            // Enter confirms (the dialog's confirm button is focused otherwise).
            if (e.key === 'Enter') {
              e.preventDefault();
              e.currentTarget.closest('dialog')?.querySelector<HTMLButtonElement>('[data-confirm]')?.click();
            }
          }}
        />
        <p class="mt-2 text-xs">
          {ids.length ? `${plural(ids.length, 'book goes', 'books go')} on it.` : 'Drag books onto it in the sidebar.'}
        </p>
      </>
    ),
    confirm: 'Create',
  });
  if (ok && name().trim()) await createShelf(name(), ids);
}

/** Ask, then make a shelf per series or per author (with at least two books). */
export async function makeShelvesFrom(source: ShelfSource): Promise<void> {
  const what = source === 'series' ? 'series' : 'author';
  const ok = await confirm({
    title: source === 'series' ? 'Shelves from Series' : 'Shelves from Authors',
    message: () => (
      <p>
        A shelf for each {what} with at least two books, named after it. A shelf already called that gets the books
        instead.
      </p>
    ),
    confirm: 'Make Shelves',
  });
  if (!ok) return;
  try {
    const made = await api.makeShelves(source, 2);
    await afterChange();
    toast(
      made.shelves || made.books
        ? `Made ${plural(made.shelves, 'shelf', 'shelves')}, shelving ${plural(made.books, 'book', 'books')}`
        : `Every ${what} with two or more books already has its shelf`,
    );
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}

export async function renameShelf(shelf: Shelf, name: string): Promise<void> {
  if (!name.trim() || name.trim() === shelf.name) return;
  const filtered = activeShelf()?.id === shelf.id;
  try {
    await api.renameShelf(shelf.id, name);
    await refreshShelves();
    // Keep showing it under its new name.
    if (filtered) showShelf({ ...shelf, name: name.trim() });
    else await refresh();
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}

export async function deleteShelf(shelf: Shelf): Promise<void> {
  const ok = await confirm({
    title: `Delete the shelf “${shelf.name}”?`,
    message: () => (
      <p>
        {shelf.count
          ? `Its ${plural(shelf.count, 'book stays', 'books stay')} in your library; only the shelf goes.`
          : 'It has no books.'}
      </p>
    ),
    confirm: 'Delete Shelf',
    danger: true,
  });
  if (!ok) return;
  const filtered = activeShelf()?.id === shelf.id;
  try {
    await api.deleteShelf(shelf.id);
    if (filtered) showShelf(null);
    await afterChange();
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}
