import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from 'solid-js';
import Check from 'lucide-solid/icons/check';

import type { BookRow } from '../api';
import { onDevice } from '../devices';
import { seriesBadges } from '../view';
import { Cover } from './Cover';
import { EmptyNote } from './Mark';
import { Icon } from './Icon';
import { credited } from '../fields';

// Layout constants (CSS px).
const PAD = 28;
const GAP_X = 24;
const GAP_Y = 28;
const TEXT_H = 38; // title + author lines under the cover
const OVERSCAN = 3; // rows rendered above/below the viewport

/** What the app needs from either view (grid or table) for keyboard navigation. */
export type ViewHandle = {
  /** Scroll so the item is fully visible. */
  reveal(id: number): void;
  /** Items per row, so ↑/↓ can move by a whole row. */
  columns(): number;
  /** Whether ←/→ move between items (grid) or do nothing (table). */
  horizontal: boolean;
  focus(): void;
};

type Props = {
  items: BookRow[];
  /** Target cover width (CSS px); tiles stretch to fill each row. */
  tileSize: number;
  selectedId: number | null;
  /** Every selected book (highlighted). */
  selectedIds: ReadonlySet<number>;
  emptyMessage: string;
  /** Show the mark with the message (the library is empty, rather than a search with no matches). */
  emptyMark: boolean;
  /** A click; modifier keys (⌘, Shift) extend the selection. */
  onSelect(id: number, e: MouseEvent): void;
  /** Right-click (or the keyboard's menu key) on a book. */
  onContextMenu(id: number, e: MouseEvent): void;
  onOpen(id: number): void;
  ref?: (handle: ViewHandle) => void;
};

/**
 * Virtualized cover grid: only rows near the viewport exist in the DOM, so
 * libraries of any size scroll smoothly.
 */
export function CoverGrid(props: Props) {
  let el!: HTMLElement;
  const [width, setWidth] = createSignal(0);
  const [height, setHeight] = createSignal(0);
  const [scrollTop, setScrollTop] = createSignal(0);

  const metrics = createMemo(() => {
    const avail = Math.max(0, width() - PAD * 2);
    const cols = Math.max(1, Math.floor((avail + GAP_X) / (props.tileSize + GAP_X)));
    const tileW = Math.max(60, (avail - GAP_X * (cols - 1)) / cols);
    const rowH = tileW * 1.5 + TEXT_H + GAP_Y;
    const rows = Math.ceil(props.items.length / cols);
    return { cols, tileW, rowH, rows };
  });

  const range = createMemo(() => {
    const { cols, rowH, rows } = metrics();
    const top = scrollTop() - PAD;
    const firstRow = Math.max(0, Math.floor(top / rowH) - OVERSCAN);
    const lastRow = Math.min(rows - 1, Math.ceil((top + height()) / rowH) + OVERSCAN);
    return { start: firstRow * cols, end: Math.min(props.items.length, (lastRow + 1) * cols) };
  });

  // Slicing the same array keeps item identity, so <For> reuses tiles while scrolling.
  const visible = createMemo(() => props.items.slice(range().start, range().end));

  const contentHeight = () => {
    const { rows, rowH } = metrics();
    return rows ? PAD * 2 + rows * rowH - GAP_Y : 0;
  };

  // When the layout changes (cover size, window or panel width), keep the
  // selected cover where it was on screen if it was showing, else the row that
  // was at the top, instead of jumping somewhere unrelated.
  createEffect(
    on(metrics, (m, prev) => {
      if (!prev || (prev.cols === m.cols && prev.rowH === m.rowH)) return;
      const rowTop = (index: number, g: { cols: number; rowH: number }) => PAD + Math.floor(index / g.cols) * g.rowH;
      const selected = props.items.findIndex((it) => it.id === props.selectedId);
      const before = selected >= 0 ? rowTop(selected, prev) - el.scrollTop : -1;
      if (selected >= 0 && before >= 0 && before + prev.rowH - GAP_Y <= el.clientHeight) {
        el.scrollTop = rowTop(selected, m) - before;
      } else {
        const firstIndex = Math.max(0, Math.floor((el.scrollTop - PAD) / prev.rowH)) * prev.cols;
        el.scrollTop = Math.floor(firstIndex / m.cols) * m.rowH;
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
        if (i < 0) return;
        const { cols, rowH } = metrics();
        const rowTop = PAD + Math.floor(i / cols) * rowH;
        const rowBottom = rowTop + rowH - GAP_Y;
        if (rowTop < el.scrollTop + 8) el.scrollTop = rowTop - 8;
        else if (rowBottom > el.scrollTop + el.clientHeight - 8) el.scrollTop = rowBottom - el.clientHeight + 8;
      },
      columns: () => metrics().cols,
      horizontal: true,
      focus: () => el.focus(),
    });
  });

  let frame = 0;
  const onScroll = () => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      setScrollTop(el.scrollTop);
    });
  };
  onCleanup(() => cancelAnimationFrame(frame));

  return (
    <section
      class="relative min-w-0 flex-1 overflow-x-hidden overflow-y-auto outline-none"
      ref={el}
      tabIndex={0}
      aria-label="Books"
      onScroll={onScroll}
    >
      <div class="w-px" style={{ height: `${contentHeight()}px` }} />
      <Show when={props.items.length === 0}>
        <EmptyNote message={props.emptyMessage} mark={props.emptyMark} />
      </Show>
      <For each={visible()}>
        {(item, i) => {
          const index = () => range().start + i();
          const position = () => {
            const { cols, tileW, rowH } = metrics();
            const x = PAD + (index() % cols) * (tileW + GAP_X);
            const y = PAD + Math.floor(index() / cols) * rowH;
            return `translate(${x}px, ${y}px)`;
          };
          return (
            <div
              class="group absolute top-0 left-0 flex cursor-default flex-col gap-2 [contain:layout_style]"
              data-selected={props.selectedIds.has(item.id) || undefined}
              data-book-id={item.id}
              style={{
                width: `${metrics().tileW}px`,
                '--cover-w': `${metrics().tileW}px`,
                '--cover-h': `${metrics().tileW * 1.5}px`,
                transform: position(),
              }}
              title={[item.title, credited(item), item.series && `${item.series} #${item.seriesIndex}`]
                .filter(Boolean)
                .join('\n')}
              onClick={(e) => props.onSelect(item.id, e)}
              onContextMenu={(e) => props.onContextMenu(item.id, e)}
              onDblClick={() => props.onOpen(item.id)}
            >
              {/* A fixed cell, so rows keep their height; the cover stands on its
                  floor at its own proportions, like a book on a shelf. The inner
                  box hugs the image, so corners, shadow, lift, outline and badges
                  follow the cover, not the cell. */}
              <div class="flex h-(--cover-h) items-end justify-center">
                <div class="relative flex overflow-hidden rounded-md bg-element shadow-card transition duration-[180ms] ease-out group-hover:-translate-y-[3px] group-hover:shadow-lift group-data-selected:outline-3 group-data-selected:outline-offset-3 group-data-selected:outline-accent">
                  <Cover
                    id={item.id}
                    title={item.title}
                    authors={item.authors}
                    hasCover={item.hasCover}
                    size="thumb"
                    fit="contain"
                  />
                  <Show when={onDevice().get(item.id)}>
                    {(where) => (
                      <span
                        class="absolute top-1.5 left-1.5 grid size-5 place-items-center rounded-full bg-success text-on-status shadow-card ring-2 ring-black/20"
                        title={where()
                          .map((o) => `On ${o.device.name}`)
                          .join('\n')}
                        aria-label="On device"
                      >
                        <Icon of={Check} size={12} />
                      </span>
                    )}
                  </Show>
                  {/* The series as a library sticker on the bottom edge: the number, and the name when there's room. */}
                  <Show when={seriesBadges() && item.seriesIndex}>
                    {(n) => (
                      <span class="absolute bottom-0 left-2 flex max-w-[72%] rounded-t-[3px] bg-paper px-1.5 pt-px text-[10px]/[15px] font-semibold text-paper-ink tabular-nums shadow-[0_-1px_0_rgb(0_0_0/0.12)]">
                        <Show when={metrics().tileW >= 150}>
                          <span class="truncate">{item.series}</span>
                          <span class="flex-none">&nbsp;·&nbsp;</span>
                        </Show>
                        <span class="flex-none">#{n()}</span>
                      </span>
                    )}
                  </Show>
                </div>
              </div>
              <p class="truncate text-[12.5px] font-semibold">{item.title}</p>
              <p class="-mt-1.5 truncate text-xs text-muted">{credited(item)}</p>
            </div>
          );
        }}
      </For>
    </section>
  );
}
