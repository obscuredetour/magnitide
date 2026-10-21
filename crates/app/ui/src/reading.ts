// How far through each book you are: from the app's own reader, and from
// e-readers syncing positions through the library server (KOReader's
// progress sync), matched to books in Rust (see reading.rs).

import { createEffect, createRoot, createSignal, on } from 'solid-js';

import { api, type BookReading, errorMessage, onReadingChanged } from './api';
import { editRevision, library } from './library';

const [states, setStates] = createSignal<Map<number, BookReading>>(new Map());

/** Book id → its reading state, for every book that has one. */
export const readingOf = states;

let seq = 0;

export async function refreshReading(): Promise<void> {
  const mine = ++seq;
  try {
    const list = await api.readingStates();
    if (mine !== seq) return;
    setStates(new Map(list.map((r) => [r.id, r])));
  } catch (err) {
    console.warn('reading states:', errorMessage(err));
  }
}

/** Some books' states just moved (the viewer saved a page, a reader synced a position). */
export function noteReading(...moved: BookReading[]): void {
  const next = new Map(states());
  for (const r of moved) next.set(r.id, r);
  setStates(next);
}

/** Follow reading states: fetched when the library changes, updated as readers sync. */
export function watchReading(): void {
  void onReadingChanged((moved) => noteReading(...moved));
  createRoot(() => {
    createEffect(on([library, editRevision], () => void refreshReading()));
  });
}

/** How far through a book being read (0–1), or null when it isn't, or there's no position. */
export function progressOf(r: BookReading | undefined): number | null {
  return r?.status === 'reading' && r.progress != null ? r.progress : null;
}

export const percent = (progress: number): string => `${Math.round(progress * 100)}%`;

/** Where and when the book was last read: "on Kobo Libra 2, 3 days ago". */
export function readingWhere(r: BookReading): string {
  const parts = [];
  if (r.device) parts.push(`on ${r.device}`);
  if (r.lastReadAt) parts.push(ago(r.lastReadAt));
  return parts.join(', ');
}

/** "just now", "3 hours ago", "yesterday", "5 days ago", or the date once it's old. */
export function ago(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return '';
  const minutes = Math.round((Date.now() - then) / 60_000);
  if (minutes < 2) return 'just now';
  if (minutes < 60) return `${minutes} minutes ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return hours === 1 ? 'an hour ago' : `${hours} hours ago`;
  const days = Math.round(hours / 24);
  if (days === 1) return 'yesterday';
  if (days < 30) return `${days} days ago`;
  return new Date(iso).toLocaleDateString(undefined, { day: 'numeric', month: 'long', year: 'numeric' });
}
