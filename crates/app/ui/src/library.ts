// App-wide library state. Small enough for module-level signals; move to a
// context/store if views start needing independent copies.

import { batch, createSignal } from 'solid-js';

import { api, type BookRow, errorMessage, type LibraryInfo, type SortDir, type SortKey } from './api';
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
export const [selectedId, setSelectedId] = createSignal<number | null>(null);
export const [detailsOpen, setDetailsOpen] = createSignal(false);
/** Bumped after any edit, so views showing a book's details fetch them again. */
export const [editRevision, setEditRevision] = createSignal(0);

const storedSort = storage.get('sort');
export const [sort, setSortSignal] = createSignal<SortKey>(isSortKey(storedSort) ? storedSort : 'title');
const storedDir = storage.get('sortDir');
export const [sortDir, setSortDirSignal] = createSignal<SortDir>(
  isSortDir(storedDir) ? storedDir : NATURAL_DIR[sort()],
);

export type RecentLibrary = { path: string; name: string };
const MAX_RECENT = 6;

function loadRecent(): RecentLibrary[] {
  try {
    const parsed: unknown = JSON.parse(storage.get('recentLibraries') ?? '[]');
    return Array.isArray(parsed)
      ? parsed.filter((r): r is RecentLibrary => typeof r?.path === 'string' && typeof r?.name === 'string')
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
  const next = [{ path: info.path, name: info.name }, ...recentLibraries().filter((r) => r.path !== info.path)].slice(
    0,
    MAX_RECENT,
  );
  setRecentLibraries(next);
  storage.set('recentLibraries', JSON.stringify(next));
}

let refreshSeq = 0;

/** Re-run the current search and sort. Stale responses are dropped. */
export async function refresh(): Promise<void> {
  const seq = ++refreshSeq;
  try {
    const result = await api.listBooks(query(), sort(), sortDir());
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
  });
  storage.set('sort', key);
  storage.set('sortDir', dir);
  void refresh();
}

/** Table header click: same column flips direction, a new column starts natural. */
export function toggleSort(key: SortKey): void {
  if (key === sort()) setSort(key, sortDir() === 'asc' ? 'desc' : 'asc');
  else setSort(key);
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
    batch(() => {
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

export async function chooseLibrary(): Promise<void> {
  const path = await api.chooseLibraryFolder();
  if (path) await openLibrary(path);
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
