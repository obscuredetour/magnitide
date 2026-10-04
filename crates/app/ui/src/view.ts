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
