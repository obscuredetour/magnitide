// App-wide library state. Small enough for module-level signals; move to a
// context/store if views start needing independent copies.

import { batch, createSignal } from 'solid-js';

import { api, errorMessage, type GridItem, type LibraryInfo, type SortKey } from './api';
import { storage } from './storage';
import { toast } from './toast';

const SORT_KEYS: readonly SortKey[] = ['title', 'author', 'added', 'published', 'series', 'rating'];
const isSortKey = (s: string | null): s is SortKey => SORT_KEYS.includes(s as SortKey);

export type Status = { kind: 'loading' } | { kind: 'ready' } | { kind: 'no-library'; message: string };

export const [status, setStatus] = createSignal<Status>({ kind: 'loading' });
export const [library, setLibrary] = createSignal<LibraryInfo | null>(null);
export const [items, setItems] = createSignal<GridItem[]>([]);
export const [query, setQuery] = createSignal('');
export const [queryError, setQueryError] = createSignal<string | null>(null);
export const [selectedId, setSelectedId] = createSignal<number | null>(null);
export const [detailsOpen, setDetailsOpen] = createSignal(false);

const storedSort = storage.get('sort');
export const [sort, setSortSignal] = createSignal<SortKey>(isSortKey(storedSort) ? storedSort : 'title');

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
    const result = await api.listBooks(query(), sort());
    if (seq !== refreshSeq) return;
    batch(() => {
      setItems(result);
      setQueryError(null);
    });
  } catch (err) {
    if (seq === refreshSeq) setQueryError(errorMessage(err));
  }
}

export function setSort(key: SortKey): void {
  setSortSignal(key);
  storage.set('sort', key);
  void refresh();
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
    setStatus({ kind: 'no-library', message: errorMessage(err) });
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
