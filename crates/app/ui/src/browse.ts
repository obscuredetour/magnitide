// The sidebar's Browse mode: the library's authors, series, tags and so on
// with counts, each a filter; and the browse panel, which opens a category
// wide beside the sidebar (taking the details panel's place) to see every
// value, and to merge the ones that should be one.

import { createResource, createRoot, createSignal } from 'solid-js';

import { api, type Facets, type ValueField } from './api';
import { languageName } from './languages';
import { editRevision, library, query, search, setDetailsOpen } from './library';
import { storage } from './storage';

export type FacetKey = keyof Facets;

export type Category = {
  key: FacetKey;
  label: string;
  /** For sentences: "a shelf per author". */
  one: string;
  /** The search field. */
  field: string;
  /** Which shared names these are, when they can be merged. Authors can't: renaming one moves folders, so that stays a per-book edit. */
  merge: ValueField | null;
  /** How a value shows (languages as names, ratings as stars). */
  show(value: string): string;
};

const stars = (v: string) => {
  const n = Number(v);
  return '★'.repeat(Math.floor(n)) + (n % 1 ? '½' : '');
};
const same = (v: string) => v;

export const CATEGORIES: readonly Category[] = [
  { key: 'authors', label: 'Authors', one: 'author', field: 'author', merge: null, show: same },
  { key: 'series', label: 'Series', one: 'series', field: 'series', merge: 'series', show: same },
  { key: 'tags', label: 'Tags', one: 'tag', field: 'tag', merge: 'tags', show: same },
  { key: 'publishers', label: 'Publishers', one: 'publisher', field: 'publisher', merge: 'publishers', show: same },
  { key: 'formats', label: 'Formats', one: 'format', field: 'format', merge: null, show: same },
  { key: 'languages', label: 'Languages', one: 'language', field: 'language', merge: null, show: languageName },
  { key: 'ratings', label: 'Ratings', one: 'rating', field: 'rating', merge: null, show: stars },
  {
    key: 'identifiers',
    label: 'Identifiers',
    one: 'identifier',
    field: 'identifier',
    merge: 'identifiers',
    show: (v) => v.toUpperCase(),
  },
];

export const category = (key: FacetKey): Category => CATEGORIES.find((c) => c.key === key)!;

// ---------- Queries ----------

/** Quoted when it needs to be. The grammar has no escapes, so a quote inside a name can't be searched for and is dropped. */
const quoted = (v: string) => (/[\s"]/.test(v) ? `"${v.replace(/"/g, '')}"` : v);

/** The search for books with exactly this value. */
export const valueQuery = (c: Category, value: string): string =>
  c.key === 'ratings' ? `rating:${value}` : `${c.field}:=${quoted(value)}`;
/** The search for books with any value at all. */
export const anyQuery = (c: Category): string => `${c.field}:`;
/** The search for books with none. */
export const noneQuery = (c: Category): string => `-${c.field}:`;

/** Whether this exact search is the one showing. */
export const isActive = (q: string): boolean => query().trim() === q;

/** Run a browse search, or clear it when it's already showing (so a second click toggles). */
export function toggleSearch(q: string): void {
  search(isActive(q) ? '' : q);
}

// ---------- State ----------

export type SidebarMode = 'shelves' | 'browse';
const [sidebarMode, setSidebarModeSignal] = createSignal<SidebarMode>(
  storage.get('sidebarMode') === 'browse' ? 'browse' : 'shelves',
);
export { sidebarMode };
export function setSidebarMode(mode: SidebarMode): void {
  setSidebarModeSignal(mode);
  storage.set('sidebarMode', mode);
}

function storedExpanded(): Set<FacetKey> {
  try {
    const parsed: unknown = JSON.parse(storage.get('browseExpanded') ?? '[]');
    return new Set(Array.isArray(parsed) ? parsed.filter((k) => CATEGORIES.some((c) => c.key === k)) : []);
  } catch {
    return new Set();
  }
}
/** Which categories are open in the sidebar. */
const [expanded, setExpanded] = createSignal<ReadonlySet<FacetKey>>(storedExpanded());
export { expanded };
export function toggleExpanded(key: FacetKey): void {
  const next = new Set(expanded());
  if (!next.delete(key)) next.add(key);
  setExpanded(next);
  storage.set('browseExpanded', JSON.stringify([...next]));
}

/** The category open in the browse panel, if any. */
const [browsePanel, setBrowsePanel] = createSignal<FacetKey | null>(null);
export { browsePanel };

/** Open a category wide. The panel takes the details panel's place. */
export function openBrowsePanel(key: FacetKey): void {
  setDetailsOpen(false);
  setBrowsePanel(key);
}
export function closeBrowsePanel(): void {
  setBrowsePanel(null);
}

/** The library's values with counts; follows the catalog like the sidebar's totals do. */
export const [facets] = createRoot(() =>
  createResource(
    () => [library(), editRevision()] as const,
    ([lib]) => (lib ? api.facets().catch(() => null) : null),
  ),
);
