// App-wide library state. Small enough for module-level signals; move to a
// context/store if views start needing independent copies.

import { batch, createSignal } from 'solid-js';

import {
  api,
  type BookRow,
  coverLibraryChanged,
  errorMessage,
  type LibraryInfo,
  type LibraryKind,
  type SortDir,
  type SortKey,
} from './api';
import { confirm } from './confirm';
import { customFields, loadCustomFields, naturalFieldDir } from './fields';
import { storage } from './storage';
import { toast } from './toast';

/** Each key's default direction. Mirrors `SortKey::natural_dir` in Rust. */
export const NATURAL_DIR: Record<SortKey, SortDir> = {
  title: 'asc',
  author: 'asc',
  series: 'asc',
  publisher: 'asc',
  rating: 'desc',
  added: 'desc',
  published: 'desc',
  size: 'desc',
};
const isSortKey = (s: string | null): s is SortKey => s !== null && s in NATURAL_DIR;
const isSortDir = (s: string | null): s is SortDir => s === 'asc' || s === 'desc';

export type Status = { kind: 'loading' } | { kind: 'ready' } | { kind: 'no-library'; message: string };

export const [status, setStatus] = createSignal<Status>({ kind: 'loading' });
export const [library, setLibrary] = createSignal<LibraryInfo | null>(null);
export const [items, setItems] = createSignal<BookRow[]>([]);
export const [query, setQuery] = createSignal('');
export const [queryError, setQueryError] = createSignal<string | null>(null);
/** The book shown in the details panel and moved by the arrow keys. */
const [selectedId, setPrimary] = createSignal<number | null>(null);
/** Every selected book (⌘-click, Shift-click, ⌘A); always includes `selectedId`. */
const [selectedIds, setSelection] = createSignal<ReadonlySet<number>>(new Set());
export { selectedId, selectedIds };
/** Where a Shift-click range starts. */
let anchor: number | null = null;

/** Select one book (or none), replacing any multiple selection. */
export function setSelectedId(id: number | null): void {
  batch(() => {
    setPrimary(id);
    setSelection(new Set(id === null ? [] : [id]));
  });
  anchor = id;
}

/** ⌘-click: add or remove one book. */
export function toggleSelected(id: number): void {
  const next = new Set(selectedIds());
  if (next.delete(id)) {
    batch(() => {
      setSelection(next);
      if (selectedId() === id) setPrimary([...next].at(-1) ?? null);
    });
  } else {
    next.add(id);
    batch(() => {
      setSelection(next);
      setPrimary(id);
    });
    anchor = id;
  }
}

/** Shift-click: everything from the last clicked book to this one, in `list` order. */
export function selectRange(id: number, list: readonly BookRow[]): void {
  const from = list.findIndex((b) => b.id === (anchor ?? id));
  const to = list.findIndex((b) => b.id === id);
  if (from < 0 || to < 0) return setSelectedId(id);
  const [a, b] = from < to ? [from, to] : [to, from];
  batch(() => {
    setSelection(new Set(list.slice(a, b + 1).map((r) => r.id)));
    setPrimary(id);
  });
}

/** Select exactly these books (the first becomes the primary one). */
export function selectBooks(ids: readonly number[]): void {
  batch(() => {
    setSelection(new Set(ids));
    setPrimary(ids[0] ?? null);
  });
  anchor = ids[0] ?? null;
}

export function selectAll(list: readonly BookRow[]): void {
  if (!list.length) return;
  batch(() => {
    setSelection(new Set(list.map((b) => b.id)));
    if (selectedId() === null) setPrimary(list[0]!.id);
  });
}
export const [detailsOpen, setDetailsOpen] = createSignal(false);
/** Bumped after any edit, so views showing a book's details fetch them again. */
export const [editRevision, setEditRevision] = createSignal(0);

const storedSort = storage.get('sort');
export const [sort, setSortSignal] = createSignal<SortKey>(isSortKey(storedSort) ? storedSort : 'title');
/** Sorted by this custom field (its key) instead of `sort`, when set. */
export const [sortField, setSortFieldSignal] = createSignal<string | null>(storage.get('sortField') || null);
const storedDir = storage.get('sortDir');
export const [sortDir, setSortDirSignal] = createSignal<SortDir>(
  isSortDir(storedDir) ? storedDir : NATURAL_DIR[sort()],
);

export type RecentLibrary = { path: string; name: string; kind?: LibraryKind };
const MAX_RECENT = 6;

function loadRecent(): RecentLibrary[] {
  try {
    const parsed: unknown = JSON.parse(storage.get('recentLibraries') ?? '[]');
    return Array.isArray(parsed)
      ? parsed
          .filter((r): r is RecentLibrary => typeof r?.path === 'string' && typeof r?.name === 'string')
          // Entries saved before the kind was kept have none.
          .map((r) => ({ ...r, kind: r.kind === 'magnitide' || r.kind === 'calibre' ? r.kind : undefined }))
      : [];
  } catch {
    return [];
  }
}

/** Most recent first, including the open library. */
export const [recentLibraries, setRecentLibraries] = createSignal<RecentLibrary[]>(loadRecent());

/** Remove a library from the Recent list (doesn't touch the library itself). */
export function forgetRecentLibrary(path: string): void {
  const next = recentLibraries().filter((r) => r.path !== path);
  setRecentLibraries(next);
  storage.set('recentLibraries', JSON.stringify(next));
}

function rememberLibrary(info: LibraryInfo): void {
  const next = [
    { path: info.path, name: info.name, kind: info.kind },
    ...recentLibraries().filter((r) => r.path !== info.path),
  ].slice(0, MAX_RECENT);
  setRecentLibraries(next);
  storage.set('recentLibraries', JSON.stringify(next));
}

let refreshSeq = 0;

/** Re-run the current search and sort. Stale responses are dropped. */
export async function refresh(): Promise<void> {
  const seq = ++refreshSeq;
  try {
    const result = await api.listBooks(query(), sort(), sortDir(), sortField());
    if (seq !== refreshSeq) return;
    batch(() => {
      setItems(result);
      setQueryError(null);
    });
  } catch (err) {
    if (seq === refreshSeq) setQueryError(errorMessage(err));
  }
}

/** Sort by `key`; without `dir`, in that key's natural direction. */
export function setSort(key: SortKey, dir: SortDir = NATURAL_DIR[key]): void {
  batch(() => {
    setSortSignal(key);
    setSortDirSignal(dir);
    setSortFieldSignal(null);
  });
  storage.set('sort', key);
  storage.set('sortDir', dir);
  storage.set('sortField', '');
  void refresh();
}

/** Sort by a custom field (its key); without `dir`, in its natural direction. */
export function setFieldSort(key: string, dir?: SortDir): void {
  const field = customFields().find((f) => f.key === key);
  const d = dir ?? (field ? naturalFieldDir(field.kind) : 'asc');
  batch(() => {
    setSortFieldSignal(key);
    setSortDirSignal(d);
  });
  storage.set('sortField', key);
  storage.set('sortDir', d);
  void refresh();
}

/** Table header click: same column flips direction, a new column starts natural. */
export function toggleSort(key: SortKey): void {
  if (key === sort() && sortField() === null) setSort(key, sortDir() === 'asc' ? 'desc' : 'asc');
  else setSort(key);
}

/** The same for a custom field's column. */
export function toggleFieldSort(key: string): void {
  if (key === sortField()) setFieldSort(key, sortDir() === 'asc' ? 'desc' : 'asc');
  else setFieldSort(key);
}

export function search(text: string): void {
  setQuery(text);
  void refresh();
}

/** Open a library; `null` means calibre's current/default one. */
export async function openLibrary(path: string | null): Promise<boolean> {
  try {
    const info = await api.openLibrary(path);
    for (const w of info.warnings) console.warn(w);
    storage.set('libraryPath', info.path);
    rememberLibrary(info);
    await loadCustomFields();
    batch(() => {
      coverLibraryChanged(info.cacheKey);
      setLibrary(info);
      setSelectedId(null);
      setDetailsOpen(false);
      setStatus({ kind: 'ready' });
    });
    await refresh();
    return true;
  } catch (err) {
    // Keep showing the current library if there is one; otherwise explain on the empty screen.
    if (status().kind === 'ready') toast(errorMessage(err), { error: true });
    else setStatus({ kind: 'no-library', message: errorMessage(err) });
    return false;
  }
}

/** Rename the open Magnitide library (calibre libraries are read-only). */
export async function renameLibrary(name: string): Promise<boolean> {
  const current = library();
  if (!current) return false;
  try {
    const saved = await api.renameLibrary(name);
    const info = { ...current, name: saved };
    setLibrary(info);
    rememberLibrary(info);
    return true;
  } catch (err) {
    toast(errorMessage(err), { error: true });
    return false;
  }
}

/**
 * Close the open library and forget it: off the Recent list, and not reopened
 * at startup. The library itself isn't touched.
 */
export async function forgetOpenLibrary(): Promise<void> {
  const current = library();
  if (!current) return;
  try {
    await api.closeLibrary();
  } catch (err) {
    toast(errorMessage(err), { error: true });
    return;
  }
  forgetRecentLibrary(current.path);
  storage.set('libraryPath', '');
  batch(() => {
    setLibrary(null);
    setItems([]);
    setSelectedId(null);
    setDetailsOpen(false);
    setStatus({ kind: 'no-library', message: 'No library open.' });
  });
}

/** Ask where to create a new Magnitide library, then create and open it. */
export async function createLibrary(): Promise<boolean> {
  const path = await api.chooseNewLibraryLocation('Magnitide Library');
  if (!path) return false;
  try {
    await api.createLibrary(path);
  } catch (err) {
    toast(errorMessage(err), { error: true });
    return false;
  }
  return openLibrary(path);
}

/** Pick a library folder and open it; a folder that isn't one offers a new library instead. */
export async function chooseLibrary(): Promise<void> {
  const path = await api.chooseLibraryFolder();
  if (!path) return;
  if (await api.libraryKindAt(path)) {
    await openLibrary(path);
    return;
  }
  const folder = path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
  const create = await confirm({
    title: `“${folder}” isn’t a library`,
    message: () =>
      'Open a Magnitide library, or a calibre library (the folder containing metadata.db). To start one, create a new library, then add books by dropping them on the window or with Add Books.',
    confirm: 'Create New Library…',
  });
  if (create) await createLibrary();
}

/** Startup: the last library used here, else calibre's current one. */
export async function openInitialLibrary(): Promise<void> {
  const last = storage.get('libraryPath');
  if (!(last && (await openLibrary(last)))) await openLibrary(null);
}

export async function openBook(id: number, format: string | null = null): Promise<void> {
  try {
    const opened = await api.openBook(id, format);
    toast(`Opening ${opened}…`);
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}
