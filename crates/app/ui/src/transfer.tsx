// Moving books between the library and devices, and merging duplicate books.

import { createSignal, For, Show } from 'solid-js';

import {
  api,
  type BookRow,
  type CollectionsReport,
  coverChanged,
  type DeviceInfo,
  errorMessage,
  onConvertProgress,
  onTransferProgress,
  trashName,
} from './api';
import { confirm } from './confirm';
import { booksOn, DEVICE_KIND_NAMES, preferredFormat, setPreferredFormat, shownItems } from './devices';
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

/** A format these books could be sent to a device in. */
export type SendOption = {
  format: string;
  /** No book has it: it's made (natively) on the way. */
  converted: boolean;
  /** How many of the books have it. */
  have: number;
};

/**
 * The formats `ids` could go to `device` in, best first: ones the device
 * reads that a book has, plus the ones made natively on the way (KEPUB for a
 * Kobo and AZW3 for a Kindle, from an EPUB). Books already on it don't count.
 */
export function sendOptions(device: DeviceInfo, ids: number[]): SendOption[] {
  const on = booksOn(device);
  const books = items().filter((b) => ids.includes(b.id) && !on.has(b.id));
  const made = device.kind === 'kobo' ? 'KEPUB' : device.kind === 'kindle' ? 'AZW3' : null;
  const fromEpub = (b: BookRow) => b.formats.includes('EPUB') || (made === 'AZW3' && b.formats.includes('KEPUB'));
  const choices = (b: BookRow) => [
    ...device.formats.filter((f) => b.formats.includes(f)),
    ...(made && fromEpub(b) && !b.formats.includes(made) ? [made] : []),
  ];
  return device.formats
    .filter((f) => books.some((b) => choices(b).includes(f)))
    .map((format) => {
      const have = books.filter((b) => b.formats.includes(format)).length;
      return { format, converted: have === 0, have };
    });
}

/**
 * Which format to send: the device kind's default, or (when a book has more
 * than one the device reads) the user's pick. Undefined: cancelled.
 */
async function chooseFormat(device: DeviceInfo, ids: number[]): Promise<string | null | undefined> {
  const preferred = preferredFormat(device.kind);
  if (preferred) return preferred;
  const books = items().filter((b) => ids.includes(b.id));
  const sendable = sendOptions(device, ids);
  const readable = (b: BookRow) => device.formats.filter((f) => b.formats.includes(f));
  const madeFor = sendable.filter((o) => o.converted).map((o) => o.format);
  if (!books.some((b) => readable(b).length + madeFor.length > 1)) return null;
  const options = sendable.map((o) => o.format);
  const converted = (f: string) => sendable.some((o) => o.format === f && o.converted);
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

/**
 * Copy books to a device; ones already there are skipped. `as` sends that
 * format (converting where a book lacks it); otherwise the device's default,
 * or asking when there's a choice.
 */
export async function sendToDevice(device: DeviceInfo, ids: number[], as?: string): Promise<void> {
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
  const format = as ?? (await chooseFormat(device, todo));
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
      if (r.collections?.books) parts.push('shelves updated');
      for (const w of r.collections?.warnings ?? []) console.warn(w);
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

/** What a shelf sync did, as a sentence. */
export function describeCollections(r: CollectionsReport, device: DeviceInfo): string {
  const apps = [r.kobo !== null && 'Kobo', r.koreader !== null && 'KOReader'].filter(Boolean).join(' and ');
  if (!apps) return r.warnings[0] ?? `Nothing on ${device.name} takes shelves`;
  const what = r.books
    ? `${plural(r.books, 'book', 'books')} on ${plural(Math.max(r.kobo ?? 0, r.koreader ?? 0), 'shelf', 'shelves')}`
    : 'no books from your shelves are on it yet';
  return `Shelves written to ${device.name} for ${apps}: ${what}` + (r.warnings.length ? ` · ${r.warnings[0]}` : '');
}

/** Write the library's shelves to a Kobo as collections, now. */
export async function syncDeviceShelves(device: DeviceInfo): Promise<void> {
  try {
    const r = await api.syncDeviceShelves(device.id);
    for (const w of r.warnings) console.warn(w);
    toast(describeCollections(r, device), { error: r.kobo === null && r.koreader === null, duration: 6000 });
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}

/** Remove one format from a book, after asking; its file goes to the Trash. */
export async function removeFormat(book: { id: number; title: string; formats: readonly unknown[] }, format: string) {
  const last = book.formats.length === 1;
  const ok = await confirm({
    title: `Remove the ${format}?`,
    message: () => (
      <>
        <p>
          The {format} file of <span class="font-medium text-fg">{book.title}</span> moves to the {trashName}.
        </p>
        <p class="mt-2 text-xs">
          {last
            ? 'It is the only file, so the book stays in your library with no files.'
            : 'The book and its other formats stay.'}
        </p>
      </>
    ),
    confirm: `Remove ${format}`,
    danger: true,
  });
  if (!ok) return;
  try {
    await api.removeFormat(book.id, format);
    await refresh();
    setEditRevision((n) => n + 1);
    toast(`Removed the ${format} · it's in the ${trashName}`);
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}

/** Delete books, after asking; their folders go to the Trash. */
export async function deleteBooks(ids: number[]): Promise<void> {
  if (library()?.kind !== 'magnitide' || !ids.length) return;
  const chosen = new Set(ids);
  const list = shownItems();
  const books = list.filter((b) => chosen.has(b.id));
  const one = books.length === 1 ? books[0] : undefined;
  const ok = await confirm({
    title: one
      ? `Move “${one.title}” to the ${trashName}?`
      : `Move ${plural(ids.length, 'book', 'books')} to the ${trashName}?`,
    message: () => (
      <>
        <Show when={!one}>
          <ul class="mb-3 max-h-40 list-inside list-disc overflow-y-auto text-fg">
            <For each={books.slice(0, 50)}>{(b) => <li class="truncate">{b.title}</li>}</For>
            <Show when={books.length > 50}>
              <li class="list-none text-muted">and {(books.length - 50).toLocaleString()} more</li>
            </Show>
          </ul>
        </Show>
        <p>
          {one ? 'Its folder' : 'Their folders'} (every format, the cover and anything else in {one ? 'it' : 'them'}){' '}
          {one ? 'moves' : 'move'} to the {trashName}, and {one ? 'it leaves' : 'they leave'} the library.
        </p>
        <p class="mt-2 text-xs">
          Putting files back from the {trashName} doesn't restore the details edited here; add them again to get the
          books back. Copies on devices aren't touched.
        </p>
      </>
    ),
    confirm: `Move to ${trashName}`,
    danger: true,
  });
  if (!ok) return;
  // Afterwards, select the book that followed the first one deleted.
  const first = list.findIndex((b) => chosen.has(b.id));
  const next =
    list.slice(first).find((b) => !chosen.has(b.id)) ??
    list
      .slice(0, first)
      .reverse()
      .find((b) => !chosen.has(b.id));
  try {
    const r = await api.deleteBooks(ids);
    // Ids can be reused by books added later; drop their cached covers.
    for (const id of r.deleted) coverChanged(id);
    await refresh();
    const lib = library();
    if (lib) setLibrary({ ...lib, bookCount: lib.bookCount - r.deleted.length });
    if (r.deleted.length) setSelectedId(next?.id ?? null);
    setEditRevision((n) => n + 1);
    for (const line of r.failed) console.info(line);
    if (r.failed.length) {
      toast(
        `Moved ${plural(r.deleted.length, 'book', 'books')} to the ${trashName} · ${r.failed.length} couldn't be: ${r.failed[0]}`,
        { error: true },
      );
    } else {
      toast(`Moved ${plural(r.deleted.length, 'book', 'books')} to the ${trashName}`);
    }
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
