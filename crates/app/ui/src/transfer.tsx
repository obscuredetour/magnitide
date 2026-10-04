// Moving books between the library and devices, and merging duplicate books.

import { createSignal, For, Show } from 'solid-js';

import { api, type BookRow, type DeviceInfo, errorMessage, onConvertProgress, onTransferProgress } from './api';
import { confirm } from './confirm';
import { booksOn, DEVICE_KIND_NAMES, preferredFormat, setPreferredFormat } from './devices';
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

/**
 * Which format to send: the device kind's default, or (when a book has more
 * than one the device reads) the user's pick. Undefined: cancelled.
 */
async function chooseFormat(device: DeviceInfo, ids: number[]): Promise<string | null | undefined> {
  const preferred = preferredFormat(device.kind);
  if (preferred) return preferred;
  const books = items().filter((b) => ids.includes(b.id));
  const readable = (b: BookRow) => device.formats.filter((f) => b.formats.includes(f));
  // KEPUB can always be made from EPUB, so on a Kobo an EPUB is a choice too.
  const makeable = (b: BookRow) =>
    device.kind === 'kobo' && b.formats.includes('EPUB') && !b.formats.includes('KEPUB') ? ['KEPUB'] : [];
  const choices = (b: BookRow) => [...readable(b), ...makeable(b)];
  if (!books.some((b) => choices(b).length > 1)) return null;
  // Every format the device reads that some selected book has (or can get), best first.
  const options = device.formats.filter((f) => books.some((b) => choices(b).includes(f)));
  const converted = (f: string) => books.every((b) => !b.formats.includes(f));
  const [pick, setPick] = createSignal(options[0]!);
  const [remember, setRemember] = createSignal(false);
  const one = books.length === 1 ? books[0] : undefined;
  const ok = await confirm({
    title: one ? `Send “${one.title}” as…` : `Send ${books.length} books as…`,
    message: () => (
      <>
        <div class="space-y-1.5" role="radiogroup" aria-label="Format">
          <For each={options}>
            {(f, i) => (
              <label class="flex cursor-pointer items-center gap-2 text-fg">
                <input
                  type="radio"
                  name="send-format"
                  class="accent-accent"
                  checked={pick() === f}
                  onChange={() => setPick(f)}
                />
                <span class="font-medium">{f}</span>
                <span class="text-xs text-muted">
                  {[
                    i() === 0 && `reads best on ${device.name}`,
                    converted(f) && 'converted from EPUB for the device',
                    !one &&
                      !converted(f) &&
                      `${books.filter((b) => b.formats.includes(f)).length} of ${books.length} have it`,
                  ]
                    .filter(Boolean)
                    .join(' · ')}
                </span>
              </label>
            )}
          </For>
        </div>
        <Show when={!one}>
          <p class="mt-2 text-xs">Books without it are converted when possible, else get the best format they have.</p>
        </Show>
        <label class="mt-3 flex cursor-pointer items-center gap-2 text-xs text-fg">
          <input
            type="checkbox"
            class="accent-accent"
            checked={remember()}
            onChange={(e) => setRemember(e.currentTarget.checked)}
          />
          Always send this format to {DEVICE_KIND_NAMES[device.kind]} (change it in Settings → Devices)
        </label>
      </>
    ),
    confirm: 'Send',
  });
  if (!ok) return undefined;
  if (remember()) setPreferredFormat(device.kind, pick());
  return pick();
}

/** Copy books to a device; ones already there are skipped. */
export async function sendToDevice(device: DeviceInfo, ids: number[]): Promise<void> {
  const already = booksOn(device);
  const todo = ids.filter((id) => !already.has(id));
  if (!todo.length) {
    toast(ids.length === 1 ? `Already on ${device.name}` : `All of these are already on ${device.name}`);
    return;
  }
  if (busy) {
    toast('Still working on the last transfer…');
    return;
  }
  const format = await chooseFormat(device, todo);
  if (format === undefined) return;
  return exclusive(`Sending to ${device.name}…`, async () => {
    const unlisten = await onTransferProgress((p) => {
      if (p.total > 1 && p.done < p.total)
        toast(`Sending ${p.done + 1} of ${p.total} to ${device.name}…`, { duration: Infinity });
    });
    try {
      const r = await api.sendToDevice(device.id, todo, format);
      for (const line of [...r.failed, ...r.unsupported, ...r.warnings]) console.info(line);
      const parts = [`Sent ${plural(r.sent, 'book', 'books')} to ${device.name}`];
      if (r.converted) parts.push(`${r.converted} converted`);
      if (r.unsupported.length)
        parts.push(
          `${plural(r.unsupported.length, 'book has', 'books have')} no format it reads` +
            (r.needsCalibre ? ' (install calibre to convert them)' : ''),
        );
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

/** Convert books to a format, adding it to each. */
export function convertBooks(ids: number[], to: string): Promise<void> {
  if (library()?.kind !== 'magnitide') {
    toast('This calibre library is read-only. Import it into a Magnitide library to convert books.', { error: true });
    return Promise.resolve();
  }
  return exclusive(`Converting to ${to}…`, async () => {
    const unlisten = await onConvertProgress((p) => {
      if (p.total > 1 && p.done < p.total)
        toast(`Converting ${p.done + 1} of ${p.total} to ${to}…`, { duration: Infinity });
    });
    try {
      const r = await api.convertBooks(ids, to);
      await refresh();
      setEditRevision((n) => n + 1);
      for (const line of r.failed) console.info(line);
      const parts = [`Converted ${plural(r.converted, 'book', 'books')} to ${to}`];
      if (r.already) parts.push(`${r.already} already had it`);
      if (r.failed.length) parts.push(r.failed.length === 1 ? r.failed[0]! : `${r.failed.length} failed`);
      toast(parts.join(' · '), { error: r.failed.length > 0, duration: r.failed.length ? 7000 : 3500 });
    } finally {
      unlisten();
    }
  });
}
