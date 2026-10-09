import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from 'solid-js';

import type { BookRow } from '../api';
import { drag } from '../bookDrag';
import { coverTint, requestCoverColors } from '../coverColors';
import { onDevice } from '../devices';
import { pagesField } from '../fields';
import { Cover } from './Cover';
import type { ArrowKey, ViewHandle } from './CoverGrid';
import { EmptyNote } from './Mark';

// Layout constants (CSS px).
const PAD = 28;
const GAP_Y = 36; // air between a board and the next shelf's spines
const BOARD_H = 10; // the drawn shelf board
const SPINE_GAP = 2; // books nearly touch
const OVERSCAN = 2; // shelves rendered above/below the viewport
const TITLE_MIN_W = 18; // narrower: colour only, the tooltip carries the title
const AUTHOR_MIN_W = 34; // room for a second rotated line
const HOVER_DELAY = 180; // ms before a hovered spine shows its cover

type Props = {
  items: BookRow[];
  /** The cover-size slider value; the shelf height is what a grid cover's would be. */
  tileSize: number;
  selectedId: number | null;
  selectedIds: ReadonlySet<number>;
  emptyMessage: string;
  emptyMark: boolean;
  onSelect(id: number, e: MouseEvent): void;
  onContextMenu(id: number, e: MouseEvent): void;
  onOpen(id: number): void;
  ref?: (handle: ViewHandle) => void;
};

const clamp01 = (x: number) => Math.min(1, Math.max(0, x));

/** 0–1 thickness: pages when the library has them, else file size on a log scale, else middling. */
function thickness(b: BookRow, pagesKey: string | undefined): number {
  if (pagesKey) {
    const pages = Number(b.custom[pagesKey]);
    if (Number.isFinite(pages) && pages > 0) return clamp01((pages - 60) / 940); // 60 → thin, 1000 → fat
  }
  if (b.size && b.size > 0) return clamp01(Math.log10(b.size / 150e3) / Math.log10(200)); // 150 KB → 0, 30 MB → 1
  return 0.35;
}

/** A stable 0–1 per book, for a little height variation. */
const hash01 = (id: number) => ((Math.imul(id, 2654435761) >>> 0) % 1000) / 999;

type Layout = {
  x: Float64Array;
  w: Float64Array;
  h: Float64Array;
  shelfOf: Int32Array;
  /** The item index that starts each shelf; `starts[shelves]` is the item count. */
  starts: number[];
  shelves: number;
};

/**
 * Books as spines standing on shelves: width from thickness, colour from the
 * cover, the title along the spine. Only shelves near the viewport exist in
 * the DOM. The result set's order is kept, so a sort by series shelves a
 * series together.
 */
export function ShelfView(props: Props) {
  let el!: HTMLElement;
  const [width, setWidth] = createSignal(0);
  const [height, setHeight] = createSignal(0);
  const [scrollTop, setScrollTop] = createSignal(0);
  const [hovered, setHovered] = createSignal<number | null>(null);

  const geometry = createMemo(() => {
    const H = Math.round(props.tileSize * 1.5);
    const minW = Math.max(14, Math.round(H * 0.09));
    const maxW = Math.round(H * 0.32);
    return { H, minW, maxW, pitch: H + BOARD_H + GAP_Y };
  });

  const layout = createMemo((): Layout => {
    const items = props.items;
    const { H, minW, maxW } = geometry();
    const pagesKey = pagesField()?.key;
    const right = width() - PAD;
    const n = items.length;
    const x = new Float64Array(n);
    const w = new Float64Array(n);
    const h = new Float64Array(n);
    const shelfOf = new Int32Array(n);
    const starts: number[] = [];
    if (width() === 0 || n === 0) return { x, w, h, shelfOf, starts: [0], shelves: 0 };
    let cx = PAD;
    let shelf = -1;
    for (let i = 0; i < n; i++) {
      const b = items[i]!;
      const wi = Math.round(minW + thickness(b, pagesKey) * (maxW - minW));
      if (shelf < 0 || cx + wi > right) {
        shelf++;
        starts.push(i);
        cx = PAD;
      }
      x[i] = cx;
      w[i] = wi;
      h[i] = Math.round(H * (0.86 + 0.14 * hash01(b.id)));
      shelfOf[i] = shelf;
      cx += wi + SPINE_GAP;
    }
    starts.push(n);
    return { x, w, h, shelfOf, starts, shelves: shelf + 1 };
  });

  const shelfTop = (s: number) => PAD + s * geometry().pitch;
  const spineTop = (i: number) => {
    const l = layout();
    return shelfTop(l.shelfOf[i]!) + geometry().H - l.h[i]!;
  };

  const range = createMemo(() => {
    const { pitch } = geometry();
    const l = layout();
    const top = scrollTop() - PAD;
    const first = Math.max(0, Math.floor(top / pitch) - OVERSCAN);
    const last = Math.min(l.shelves - 1, Math.ceil((top + height()) / pitch) + OVERSCAN);
    if (last < first) return { firstShelf: 0, lastShelf: -1, start: 0, end: 0 };
    return { firstShelf: first, lastShelf: last, start: l.starts[first]!, end: l.starts[last + 1]! };
  });

  // Slicing the same array keeps item identity, so <For> reuses spines while scrolling.
  const visible = createMemo(() => props.items.slice(range().start, range().end));
  const visibleShelves = createMemo(() => {
    const { firstShelf, lastShelf } = range();
    return Array.from({ length: Math.max(0, lastShelf - firstShelf + 1) }, (_, i) => firstShelf + i);
  });

  // The spines on screen take their covers' colours.
  createEffect(() =>
    requestCoverColors(
      visible()
        .filter((b) => b.hasCover)
        .map((b) => b.id),
    ),
  );

  const contentHeight = () => {
    const l = layout();
    return l.shelves ? PAD * 2 + l.shelves * geometry().pitch - GAP_Y : 0;
  };

  // When the layout changes (slider, window or panel width), keep the selected
  // spine's shelf where it was on screen if it was showing, else the shelf at
  // the top, instead of jumping somewhere unrelated.
  createEffect(
    on([layout, geometry], ([l, g], prev) => {
      if (!prev) return;
      const [pl, pg] = prev;
      if (pl.shelves === l.shelves && pg.pitch === g.pitch) return;
      const top = (shelf: number, pitch: number) => PAD + shelf * pitch;
      const selected = props.items.findIndex((it) => it.id === props.selectedId);
      const before =
        selected >= 0 && selected < pl.shelfOf.length ? top(pl.shelfOf[selected]!, pg.pitch) - el.scrollTop : -1;
      if (selected >= 0 && before >= 0 && before + pg.pitch - GAP_Y <= el.clientHeight) {
        el.scrollTop = top(l.shelfOf[selected]!, g.pitch) - before;
      } else {
        const firstShelf = Math.max(0, Math.floor((el.scrollTop - PAD) / pg.pitch));
        const firstIndex = pl.starts[Math.min(firstShelf, Math.max(0, pl.shelves - 1))] ?? 0;
        el.scrollTop = firstIndex < l.shelfOf.length ? top(l.shelfOf[firstIndex]!, g.pitch) : 0;
      }
      setScrollTop(el.scrollTop);
    }),
  );

  // A new result set starts at the top.
  createEffect(
    on(
      () => props.items,
      () => {
        el.scrollTop = 0;
        setScrollTop(0);
      },
      { defer: true },
    ),
  );

  // A drag takes the pointer: no floating cover following it.
  createEffect(() => {
    if (drag()) setHovered(null);
  });

  onMount(() => {
    const ro = new ResizeObserver(() => {
      setWidth(el.clientWidth);
      setHeight(el.clientHeight);
    });
    ro.observe(el);
    onCleanup(() => ro.disconnect());

    props.ref?.({
      reveal(id) {
        const i = props.items.findIndex((it) => it.id === id);
        if (i < 0 || i >= layout().shelfOf.length) return;
        const top = shelfTop(layout().shelfOf[i]!);
        const bottom = top + geometry().pitch - GAP_Y;
        if (top < el.scrollTop + 8) el.scrollTop = top - 8;
        else if (bottom > el.scrollTop + el.clientHeight - 8) el.scrollTop = bottom - el.clientHeight + 8;
      },
      columns: () => 1,
      horizontal: true,
      focus: () => el.focus(),
      neighbour: (id, key) => neighbour(id, key),
    });
  });

  /** Where an arrow key goes: along the shelf by index, or to the spine under the same x on the shelf above/below. */
  const neighbour = (id: number | null, key: ArrowKey): number | null => {
    const list = props.items;
    const l = layout();
    if (!list.length || !l.shelves) return null;
    const cur = id === null ? -1 : list.findIndex((b) => b.id === id);
    if (cur < 0) return list[0]!.id;
    if (key === 'ArrowLeft' || key === 'ArrowRight') {
      const next = Math.min(list.length - 1, Math.max(0, cur + (key === 'ArrowLeft' ? -1 : 1)));
      return list[next]!.id;
    }
    const s = l.shelfOf[cur]! + (key === 'ArrowUp' ? -1 : 1);
    if (s < 0) return list[0]!.id;
    if (s >= l.shelves) return list[list.length - 1]!.id;
    const cx = l.x[cur]! + l.w[cur]! / 2;
    let best = l.starts[s]!;
    let bestD = Infinity;
    for (let i = l.starts[s]!; i < l.starts[s + 1]!; i++) {
      const d = Math.abs(l.x[i]! + l.w[i]! / 2 - cx);
      if (d < bestD) {
        best = i;
        bestD = d;
      } else break; // x ascends along the shelf
    }
    return list[best]!.id;
  };

  let frame = 0;
  const onScroll = () => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      setScrollTop(el.scrollTop);
    });
  };
  onCleanup(() => cancelAnimationFrame(frame));

  let hoverTimer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(hoverTimer));
  const hoverSoon = (id: number) => {
    clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => {
      if (!drag()) setHovered(id);
    }, HOVER_DELAY);
  };
  const hoverEnd = (id: number) => {
    clearTimeout(hoverTimer);
    if (hovered() === id) setHovered(null);
  };

  /** The book whose cover floats above its spine: the hovered one, else the selected one, if its spine is rendered. */
  const preview = createMemo(() => {
    const id = hovered() ?? props.selectedId;
    if (id === null) return null;
    const { start, end } = range();
    for (let i = start; i < end; i++) if (props.items[i]!.id === id) return i;
    return null;
  });

  return (
    <section
      class="relative min-w-0 flex-1 overflow-x-hidden overflow-y-auto outline-none"
      ref={el}
      tabIndex={0}
      aria-label="Books"
      onScroll={onScroll}
      onPointerLeave={() => {
        clearTimeout(hoverTimer);
        setHovered(null);
      }}
    >
      <div class="w-px" style={{ height: `${contentHeight()}px` }} />
      <Show when={props.items.length === 0}>
        <EmptyNote message={props.emptyMessage} mark={props.emptyMark} />
      </Show>
      <For each={visibleShelves()}>
        {(s) => (
          <div
            class="pointer-events-none absolute inset-x-4 top-0 rounded-[2px] border-t border-line bg-element shadow-card"
            style={{ height: `${BOARD_H}px`, transform: `translateY(${shelfTop(s) + geometry().H}px)` }}
          />
        )}
      </For>
      <For each={visible()}>
        {(item, i) => {
          const index = () => range().start + i();
          const w = () => layout().w[index()] ?? 0;
          const h = () => layout().h[index()] ?? 0;
          const position = () => `translate(${layout().x[index()] ?? 0}px, ${spineTop(index())}px)`;
          return (
            <div
              class="group absolute top-0 left-0 cursor-default rounded-[2px] spine ring-1 ring-black/15 transition-transform duration-[180ms] ease-out [contain:layout_style] ring-inset hover:z-10 hover:-translate-y-1.5 data-selected:z-10 data-selected:-translate-y-2.5 data-selected:shadow-lift data-selected:outline-2 data-selected:outline-offset-1 data-selected:outline-accent"
              data-selected={props.selectedIds.has(item.id) || undefined}
              data-book-id={item.id}
              style={{
                width: `${w()}px`,
                height: `${h()}px`,
                transform: position(),
                '--spine': coverTint(item.id, item.hasCover),
              }}
              title={[item.title, item.authors, item.series && `${item.series} #${item.seriesIndex}`]
                .filter(Boolean)
                .join('\n')}
              onClick={(e) => props.onSelect(item.id, e)}
              onContextMenu={(e) => props.onContextMenu(item.id, e)}
              onDblClick={() => props.onOpen(item.id)}
              onPointerEnter={() => hoverSoon(item.id)}
              onPointerLeave={() => hoverEnd(item.id)}
            >
              <Show when={w() >= TITLE_MIN_W}>
                {/* Reversed row: the title (first in the DOM) lands on the right, the top line once turned.
                    Each line is its own vertical box, so `truncate` clips along the spine with an ellipsis. */}
                <div class="flex h-full flex-row-reverse items-start justify-center gap-px overflow-hidden px-[3px] py-2">
                  <span class="max-h-full truncate text-[12px] leading-none font-semibold [writing-mode:vertical-rl]">
                    {item.title}
                  </span>
                  <Show when={w() >= AUTHOR_MIN_W}>
                    <span class="max-h-full truncate text-[10.5px] leading-none opacity-75 [writing-mode:vertical-rl]">
                      {item.authors}
                    </span>
                  </Show>
                </div>
              </Show>
              <Show when={onDevice().get(item.id)}>
                <span
                  class="absolute bottom-1.5 left-1/2 size-1.5 -translate-x-1/2 rounded-full bg-success ring-1 ring-black/20"
                  aria-label="On device"
                />
              </Show>
            </div>
          );
        }}
      </For>
      {/* The hovered or selected book's cover, floating above its spine. */}
      <Show when={preview()}>
        {(i) => {
          const item = () => props.items[i()]!;
          const pw = () => Math.round(Math.min(Math.max(props.tileSize * 0.7, 90), 170));
          const ph = () => Math.round(pw() * 1.5);
          const left = () => {
            const l = layout();
            return Math.min(Math.max(l.x[i()]! + l.w[i()]! / 2 - pw() / 2, PAD), Math.max(PAD, width() - PAD - pw()));
          };
          const top = () => Math.max(4, spineTop(i()) - 10 - 8 - ph());
          return (
            <div
              class="pointer-events-none absolute top-0 left-0 z-20 animate-menu-in overflow-hidden rounded-md bg-element shadow-lift"
              style={{ width: `${pw()}px`, height: `${ph()}px`, transform: `translate(${left()}px, ${top()}px)` }}
            >
              <Cover
                id={item().id}
                title={item().title}
                authors={item().authors}
                hasCover={item().hasCover}
                size="thumb"
              />
            </div>
          );
        }}
      </Show>
    </section>
  );
}
