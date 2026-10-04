// View preferences (per device).

import { createSignal } from 'solid-js';

import { storage } from './storage';

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
  setColumnVisibilitySignal(visibility);
  storage.set('tableColumnVisibility', JSON.stringify(visibility));
}
