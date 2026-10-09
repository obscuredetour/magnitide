// Cover palettes from Rust, for the details panel to blend its background
// toward. Whoever shows a book asks for its colours; requests are batched
// and deduplicated. Reset when another library opens; a book's entry
// is dropped when its cover changes (coverChanged in api.ts).

import { createEffect, createRoot, createSignal, on } from 'solid-js';

import { api, type CoverColor, coverVersions, errorMessage } from './api';
import { library } from './library';

/** The generated cover's hue for a book without one. Mirrors `cover-placeholder` in styles.css. */
export const placeholderHue = (id: number): number => (id * 47) % 360;
/** The generated cover's main colour, usable wherever a cover colour is. */
export const placeholderColor = (id: number): string => `hsl(${placeholderHue(id)} 42% 42%)`;

/** Ids per command: a screenful. More would hold one blocking thread for seconds on a cold cache. */
const CHUNK = 64;

const [colors, setColors] = createSignal<ReadonlyMap<number, CoverColor | null>>(new Map());
const queued = new Set<number>();
const inFlight = new Set<number>();
let scheduled = false;
let generation = 0; // bumped when the library changes, so late answers are dropped

/** Ask for these books' colours; known and in-flight ids are skipped. */
export function requestCoverColors(ids: Iterable<number>): void {
  const known = colors();
  for (const id of ids) if (!known.has(id) && !inFlight.has(id)) queued.add(id);
  if (queued.size && !scheduled) {
    scheduled = true;
    queueMicrotask(() => void flush());
  }
}

async function flush(): Promise<void> {
  scheduled = false;
  while (queued.size) {
    const ids = [...queued].slice(0, CHUNK);
    for (const id of ids) {
      queued.delete(id);
      inFlight.add(id);
    }
    const gen = generation;
    try {
      const got = await api.coverColors(ids);
      if (gen !== generation) return;
      setColors((m) => {
        const next = new Map(m);
        for (const id of ids) next.set(id, null); // asked; no readable cover
        for (const c of got) next.set(c.id, c);
        return next;
      });
    } catch (err) {
      console.warn('cover colours:', errorMessage(err));
    } finally {
      for (const id of ids) inFlight.delete(id);
    }
  }
}

/** A book's cover palette, most common colour first: the generated cover's for a book without one; undefined while unknown. Reactive. */
export function coverPalette(id: number, hasCover: boolean): readonly string[] | undefined {
  return hasCover ? colors().get(id)?.colors : [placeholderColor(id)];
}

createRoot(() => {
  // Another library: its ids mean other books.
  createEffect(
    on(
      library,
      () => {
        generation++;
        queued.clear();
        inFlight.clear();
        setColors(new Map());
      },
      { defer: true },
    ),
  );
  // A changed cover: forget its colour; whoever shows it asks again.
  createEffect(
    on(coverVersions, (now, prev) => {
      if (!prev) return;
      const changed = [...now].filter(([id, v]) => prev.get(id) !== v).map(([id]) => id);
      if (changed.length) {
        setColors((m) => {
          const next = new Map(m);
          for (const id of changed) next.delete(id);
          return next;
        });
      }
    }),
  );
});
