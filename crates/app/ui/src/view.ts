// View preferences (per device).

import { createSignal } from 'solid-js';

import { devices } from './devices';
import { storage } from './storage';
import { customFields } from './fields';

/** Target cover width in CSS px; the grid stretches tiles to fill each row. */
export const COVER_SIZE = { min: 100, max: 240, step: 10, initial: 150 } as const;

const clamp = (n: number) => Math.min(COVER_SIZE.max, Math.max(COVER_SIZE.min, Math.round(n)));
const stored = Number(storage.get('coverSize'));

const [coverSize, setCoverSizeSignal] = createSignal(stored ? clamp(stored) : COVER_SIZE.initial);
export { coverSize };

export function setCoverSize(n: number): void {
  const size = clamp(n);
  setCoverSizeSignal(size);
  storage.set('coverSize', String(size));
}

export type ViewMode = 'grid' | 'table';
const storedMode = storage.get('viewMode');
const [viewMode, setViewModeSignal] = createSignal<ViewMode>(storedMode === 'table' ? 'table' : 'grid');
export { viewMode };

export function setViewMode(mode: ViewMode): void {
  setViewModeSignal(mode);
  storage.set('viewMode', mode);
}

/** Table column widths (px) and visibility, keyed by column id. */
function storedRecord<T>(key: string): Record<string, T> {
  try {
    const parsed: unknown = JSON.parse(storage.get(key) ?? '{}');
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? (parsed as Record<string, T>) : {};
  } catch {
    return {};
  }
}

const [columnSizing, setColumnSizingSignal] = createSignal(storedRecord<number>('tableColumnSizing'));
const [columnVisibility, setColumnVisibilitySignal] = createSignal(storedRecord<boolean>('tableColumnVisibility'));
export { columnSizing, columnVisibility };

export function setColumnSizing(sizing: Record<string, number>): void {
  setColumnSizingSignal(sizing);
  storage.set('tableColumnSizing', JSON.stringify(sizing));
}

export function setColumnVisibility(visibility: Record<string, boolean>): void {
  // The device column is forced off while nothing is connected; don't save that.
  if (!devices().length) {
    const { device: _forced, ...rest } = visibility;
    const saved = columnVisibility().device;
    visibility = saved === undefined ? rest : { ...rest, device: saved };
  }
  setColumnVisibilitySignal(visibility);
  storage.set('tableColumnVisibility', JSON.stringify(visibility));
}

/** Table columns hidden until the user turns them on. */
const DEFAULT_HIDDEN: Record<string, boolean> = { publisher: false, tags: false, size: false };

/** Visibility of every table column that differs from "shown". "On Device" only shows while a device is connected. */
export const effectiveColumnVisibility = (): Record<string, boolean> => ({
  ...DEFAULT_HIDDEN,
  // Custom fields' columns are hidden until turned on.
  ...Object.fromEntries(customFields().map((f) => [`custom:${f.key}`, false])),
  ...columnVisibility(),
  ...(devices().length ? {} : { device: false }),
});

export function isColumnVisible(id: string): boolean {
  return effectiveColumnVisibility()[id] ?? true;
}

export function setColumnVisible(id: string, visible: boolean): void {
  setColumnVisibility({ ...effectiveColumnVisibility(), [id]: visible });
}

/** Back to each column's default width. */
export function resetColumnSizing(): void {
  setColumnSizing({});
}

function storedOrder(): string[] {
  try {
    const parsed: unknown = JSON.parse(storage.get('tableColumnOrder') ?? '[]');
    return Array.isArray(parsed) ? parsed.filter((x): x is string => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

/** Table column ids in display order; columns it doesn't name keep their default place after these. Empty: the default order. */
const [columnOrder, setColumnOrderSignal] = createSignal<string[]>(storedOrder());
export { columnOrder };

export function setColumnOrder(order: string[]): void {
  setColumnOrderSignal(order);
  storage.set('tableColumnOrder', JSON.stringify(order));
}

/** Back to the default column order. */
export function resetColumnOrder(): void {
  setColumnOrder([]);
}

/** The series label on covers in the cover view; on unless turned off. */
const [seriesBadges, setSeriesBadgesSignal] = createSignal(storage.get('seriesBadges') !== 'off');
export { seriesBadges };

export function setSeriesBadges(on: boolean): void {
  setSeriesBadgesSignal(on);
  storage.set('seriesBadges', on ? 'on' : 'off');
}

/** The cover's colours behind the details on every theme, not only Light and Dark; off unless turned on. */
const [blendAllThemes, setBlendAllThemesSignal] = createSignal(storage.get('blendAllThemes') === 'on');
export { blendAllThemes };

function applyBlendAll(on: boolean): void {
  if (on) document.documentElement.dataset.blendAll = '';
  else delete document.documentElement.dataset.blendAll;
}
applyBlendAll(blendAllThemes());

export function setBlendAllThemes(on: boolean): void {
  setBlendAllThemesSignal(on);
  applyBlendAll(on);
  storage.set('blendAllThemes', on ? 'on' : 'off');
}

/**
 * The toolbar: two rows (the library, devices and app controls above; the
 * sidebar, search and how books show below), or everything in one row.
 */
export type ToolbarLayout = 'two-rows' | 'one-row';
const [toolbarLayout, setToolbarLayoutSignal] = createSignal<ToolbarLayout>(
  storage.get('toolbar') === 'one-row' ? 'one-row' : 'two-rows',
);
export { toolbarLayout };

/** On <html>, for the toolbar's height (styles.css). */
function applyToolbar(layout: ToolbarLayout): void {
  document.documentElement.dataset.toolbar = layout;
}
applyToolbar(toolbarLayout());

export function setToolbarLayout(layout: ToolbarLayout): void {
  setToolbarLayoutSignal(layout);
  applyToolbar(layout);
  storage.set('toolbar', layout);
}

/** Alternating row colours in the list view; on unless turned off. */
const [zebraRows, setZebraRowsSignal] = createSignal(storage.get('tableZebra') !== 'off');
export { zebraRows };

export function setZebraRows(on: boolean): void {
  setZebraRowsSignal(on);
  storage.set('tableZebra', on ? 'on' : 'off');
}

/** Width of the details panel in CSS px (it also never squeezes the list below 320px). */
export const DETAILS_WIDTH = { min: 300, max: 680, initial: 380 } as const;

const clampDetails = (n: number) => Math.min(DETAILS_WIDTH.max, Math.max(DETAILS_WIDTH.min, Math.round(n)));
const storedDetails = Number(storage.get('detailsWidth'));
const [detailsWidth, setDetailsWidthSignal] = createSignal(
  storedDetails ? clampDetails(storedDetails) : DETAILS_WIDTH.initial,
);
export { detailsWidth };

/** Set the details panel's width; `save` stores it (at the end of a drag, not on every move). */
export function setDetailsWidth(n: number, save = true): void {
  const w = clampDetails(n);
  setDetailsWidthSignal(w);
  if (save) storage.set('detailsWidth', String(w));
}

/** The shelves sidebar; shown unless turned off. */
const [sidebarOpen, setSidebarOpenSignal] = createSignal(storage.get('sidebar') !== 'off');
export { sidebarOpen };

export function setSidebarOpen(open: boolean): void {
  setSidebarOpenSignal(open);
  storage.set('sidebar', open ? 'on' : 'off');
}
