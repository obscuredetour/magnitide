// The open library's custom fields (Settings › Library › Custom Fields),
// loaded with it, and showing their values by kind.

import { createSignal } from 'solid-js';

import { api, type CustomField, type FieldKind, type SortDir } from './api';

const [customFields, setCustomFields] = createSignal<CustomField[]>([]);
export { customFields };

/** Load the open library's fields (after opening it, or changing them). */
export async function loadCustomFields(): Promise<void> {
  try {
    setCustomFields(await api.customFields());
  } catch {
    setCustomFields([]);
  }
}

/** The comic fields' group, and the field in it that marks a book as a comic. Mirrors core's `comics`. */
export const COMIC_GROUP = 'comic';
export const COMIC_KEY = 'comic';

/** The fields of the user's own (not in a group). */
export const ownFields = () => customFields().filter((f) => !f.group);
/** The comic fields in the order they're shown, whatever order a library made them in. Mirrors core's `COMIC_FIELDS`. */
const COMIC_ORDER = [
  COMIC_KEY,
  'issue',
  'format',
  'volume',
  'artists',
  'inkers',
  'colorists',
  'letterers',
  'cover_artists',
  'editors',
  'story_arcs',
  'characters',
  'teams',
  'locations',
  'age_rating',
  'right_to_left',
];
/** Sort by `COMIC_ORDER`; keys it doesn't know go last. */
export const byComicOrder = (a: string, b: string) => {
  const rank = (k: string) => (COMIC_ORDER.indexOf(k) + COMIC_ORDER.length + 1) % (COMIC_ORDER.length + 1);
  return rank(a) - rank(b);
};
/** The comic fields shown for a comic, without the field that marks it as one. */
export const comicFields = () =>
  customFields()
    .filter((f) => f.group === COMIC_GROUP && f.key !== COMIC_KEY)
    .sort((a, b) => byComicOrder(a.key, b.key));
/** The field that marks a book as a comic, once the library has it. */
export const comicMarker = () => customFields().find((f) => f.group === COMIC_GROUP && f.key === COMIC_KEY);

/** Comic archive formats: a book in one is taken for a comic until said otherwise. */
export const isComicFormat = (format: string) => ['CBZ', 'CBR', 'CB7'].includes(format.toUpperCase());

/** Make the comic fields if the library hasn't got them, and load them. */
export async function ensureComicFields(): Promise<void> {
  if (comicMarker()) return;
  await api.ensureComicFields();
  await loadCustomFields();
}

export const FIELD_KINDS: Record<FieldKind, string> = {
  text: 'Text',
  longText: 'Long text',
  integer: 'Whole number',
  number: 'Number',
  bool: 'Yes or no',
  date: 'Date',
  rating: 'Rating',
  choice: 'Choice',
  series: 'Series',
};

/** Text sorts A→Z first; numbers, dates, ratings and yes/no highest first. */
export function naturalFieldDir(kind: FieldKind): SortDir {
  return kind === 'text' || kind === 'longText' || kind === 'choice' || kind === 'series' ? 'asc' : 'desc';
}

/** Numbers line up on the right in the table. */
export function isNumeric(kind: FieldKind): boolean {
  return kind === 'integer' || kind === 'number';
}

const dateFormat = new Intl.DateTimeFormat(undefined, { year: 'numeric', month: 'short', day: 'numeric' });

/**
 * A value as Rust sends it (`BookRow.custom`) for display: dates in the
 * user's locale, yes/no, "Name #2" for series. Ratings are drawn as stars
 * by the caller; long text is HTML.
 */
export function formatCustom(kind: FieldKind, raw: string | undefined): string {
  if (raw === undefined || raw === '') return '';
  switch (kind) {
    case 'bool':
      return raw === 'true' ? 'Yes' : 'No';
    case 'date':
      return dateFormat.format(new Date(`${raw}T00:00:00`));
    case 'series': {
      const m = /^(.*) \[(.+)\]$/.exec(raw);
      return m ? `${m[1]} #${m[2]}` : raw;
    }
    case 'integer':
      return Number(raw).toLocaleString();
    default:
      return raw;
  }
}
