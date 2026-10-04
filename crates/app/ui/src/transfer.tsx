// Moving books between the library and devices, and merging duplicate books.

import { createSignal, For } from 'solid-js';

import { api, type BookRow, type DeviceInfo, errorMessage, onTransferProgress } from './api';
import { confirm } from './confirm';
import { booksOn } from './devices';
import { items, library, refresh, setEditRevision, setLibrary, setSelectedId } from './library';
import { toast } from './toast';
import { plural } from './writeback';

let busy = false;

/** Run one transfer at a time, with a persistent "working" toast. */
async function exclusive(label: string, work: () => Promise<void>): Promise<void> {
  if (busy) {
    toast('Still working on the last transfer…');
    return;
  }
  busy = true;
  toast(label, { duration: Infinity });
  try {
    await work();
  } catch (err) {
    toast(errorMessage(err), { error: true });
  } finally {
    busy = false;
  }
}

/** Copy books to a device; ones already there are skipped. */
export function sendToDevice(device: DeviceInfo, ids: number[]): Promise<void> {
  const already = booksOn(device);
  const todo = ids.filter((id) => !already.has(id));
  if (!todo.length) {
    toast(ids.length === 1 ? `Already on ${device.name}` : `All of these are already on ${device.name}`);
    return Promise.resolve();
  }
  return exclusive(`Sending to ${device.name}…`, async () => {
    const unlisten = await onTransferProgress((p) => {
      if (p.total > 1 && p.done < p.total)
        toast(`Sending ${p.done + 1} of ${p.total} to ${device.name}…`, { duration: Infinity });
    });
    try {
      const r = await api.sendToDevice(device.id, todo);
      for (const line of [...r.failed, ...r.unsupported, ...r.warnings]) console.info(line);
      const parts = [`Sent ${plural(r.sent, 'book', 'books')} to ${device.name}`];
      if (r.unsupported.length)
        parts.push(`${plural(r.unsupported.length, 'book has', 'books have')} no format it reads`);
      if (r.failed.length) parts.push(r.failed.length === 1 ? r.failed[0]! : `${r.failed.length} failed`);
      const problem = r.failed.length > 0 || (r.sent === 0 && r.unsupported.length > 0);
      toast(parts.join(' · '), { error: problem, duration: problem ? 7000 : 3500 });
    } finally {
      unlisten();
    }
  });
}

/** Delete books from a device, after asking. */
export async function removeFromDevice(device: DeviceInfo, ids: number[]): Promise<void> {
  const on = booksOn(device);
  const todo = ids.filter((id) => on.has(id));
  if (!todo.length) return;
  const one = todo.length === 1 ? items().find((b) => b.id === todo[0]) : undefined;
  const ok = await confirm({
    title: one ? `Remove “${one.title}” from ${device.name}?` : `Remove ${todo.length} books from ${device.name}?`,
    message: () => (
      <>
        The {todo.length === 1 ? 'file is' : 'files are'} deleted from the device, along with any KOReader notes and
        reading progress kept beside {todo.length === 1 ? 'it' : 'them'}. The library keeps its copies.
      </>
    ),
    confirm: todo.length === 1 ? 'Remove' : `Remove ${todo.length} Books`,
    danger: true,
  });
  if (!ok) return;
  await exclusive(`Removing from ${device.name}…`, async () => {
    const r = await api.removeFromDevice(device.id, todo);
    for (const line of r.failed) console.info(line);
    toast(
      r.failed.length
        ? `Removed ${r.removed}; ${r.failed.length} couldn't be removed`
        : `Removed ${plural(r.removed, 'book', 'books')} from ${device.name}`,
      { error: r.failed.length > 0 },
    );
  });
}

/** Add the device's files that aren't in the library. */
export function importFromDevice(device: DeviceInfo): Promise<void> {
  const lib = library();
  if (lib?.kind !== 'magnitide') {
    toast('This calibre library is read-only. Import it into a Magnitide library to add books.', { error: true });
    return Promise.resolve();
  }
  return exclusive(`Adding books from ${device.name}…`, async () => {
    const r = await api.importFromDevice(device.id, device.unmatched);
    await refresh();
    setLibrary({ ...lib, bookCount: lib.bookCount + r.added.length });
    if (r.formatsAddedTo.length) setEditRevision((n) => n + 1);
    const parts = [`Added ${plural(r.added.length, 'book', 'books')} from ${device.name}`];
    if (r.formatsAddedTo.length) parts.push(`new formats for ${plural(r.formatsAddedTo.length, 'book', 'books')}`);
    if (r.duplicates.length) parts.push(`${r.duplicates.length} already in the library`);
    if (r.failed.length) parts.push(`${r.failed.length} failed`);
    for (const line of [...r.failed, ...r.duplicates, ...r.skipped]) console.info(line);
    toast(parts.join(' · '), { error: r.failed.length > 0, duration: 4000 });
  });
}

/** Merge two books into one, after asking which one's details to keep. */
export async function mergeBooks(a: BookRow, b: BookRow): Promise<void> {
  const [older, newer] = a.id < b.id ? [a, b] : [b, a];
  const [keep, setKeep] = createSignal(older.id);
  const ok = await confirm({
    title: 'Merge these books?',
    message: () => (
      <>
        <p class="mb-3">Their files become one book. Keep the title, authors and other details of:</p>
        <div class="space-y-1.5" role="radiogroup">
          <For each={[older, newer]}>
            {(book) => (
              <label class="flex cursor-pointer items-start gap-2 text-fg">
                <input
                  type="radio"
                  name="merge-keep"
                  class="mt-1 accent-accent"
                  checked={keep() === book.id}
                  onChange={() => setKeep(book.id)}
                />
                <span>
                  <span class="font-medium">{book.title}</span>
                  <span class="text-muted">
                    {' '}
                    · {book.authors} · {book.formats.join(', ') || 'no files'}
                  </span>
                </span>
              </label>
            )}
          </For>
        </div>
        <p class="mt-3 text-xs">It also takes the other's cover (if it has none), tags and identifiers.</p>
      </>
    ),
    confirm: 'Merge',
  });
  if (!ok) return;
  const kept = keep();
  const other = kept === older.id ? newer.id : older.id;
  try {
    const r = await api.mergeBooks(kept, other);
    await refresh();
    const lib = library();
    if (lib) setLibrary({ ...lib, bookCount: lib.bookCount - 1 });
    setSelectedId(kept);
    setEditRevision((n) => n + 1);
    toast(`Merged · ${plural(r.formatsMoved, 'file', 'files')} moved${r.coverTaken ? ', cover taken' : ''}`);
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}
