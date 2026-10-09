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

/** The custom field holding a page count, if the library has one (by key, then by name). */
export const pagesField = (): CustomField | undefined =>
  customFields().find((f) => isNumeric(f.kind) && /^(pages?|page_?count|num_?pages)$/i.test(f.key)) ??
  customFields().find((f) => isNumeric(f.kind) && /\bpages?\b/i.test(f.name));

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
