import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from 'solid-js';

import type { GridItem } from '../api';
import { Cover } from './Cover';

// Layout constants (CSS px).
const PAD = 28;
const GAP_X = 24;
const GAP_Y = 28;
const MIN_TILE = 128;
const MAX_TILE = 176;
const TEXT_H = 38; // title + author lines under the cover
const OVERSCAN = 3; // rows rendered above/below the viewport

export type GridHandle = {
  /** Scroll so the item is fully visible. */
  reveal(id: number): void;
  /** Columns in the current layout, for arrow-key navigation. */
  columns(): number;
  focus(): void;
};

type Props = {
  items: GridItem[];
  selectedId: number | null;
  emptyMessage: string;
  onSelect(id: number): void;
  onOpen(id: number): void;
  ref?: (handle: GridHandle) => void;
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
    let cols = Math.max(1, Math.floor((avail + GAP_X) / (MIN_TILE + GAP_X)));
    let tileW = (avail - GAP_X * (cols - 1)) / cols;
    while (tileW > MAX_TILE) {
      cols += 1;
      tileW = (avail - GAP_X * (cols - 1)) / cols;
    }
    tileW = Math.max(60, tileW);
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
        <p class="absolute inset-x-0 top-[30%] text-center text-sm text-muted">{props.emptyMessage}</p>
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
              data-selected={item.id === props.selectedId || undefined}
              style={{ width: `${metrics().tileW}px`, transform: position() }}
              title={[item.title, item.authors, item.series && `${item.series} #${item.seriesIndex}`]
                .filter(Boolean)
                .join('\n')}
              onClick={() => props.onSelect(item.id)}
              onDblClick={() => props.onOpen(item.id)}
            >
              <div class="relative aspect-[2/3] w-full overflow-hidden rounded-md bg-surface-2 shadow-card transition duration-[180ms] ease-out group-hover:-translate-y-[3px] group-hover:shadow-lift group-data-selected:outline-3 group-data-selected:outline-offset-3 group-data-selected:outline-accent">
                <Cover id={item.id} title={item.title} authors={item.authors} hasCover={item.hasCover} size="thumb" />
                <Show when={item.seriesIndex}>{(n) => <span class="absolute right-1.5 bottom-1.5 rounded-full bg-black/60 px-[7px] py-px text-[11px] font-semibold text-white tabular-nums backdrop-blur-sm">#{n()}</span>}</Show>
              </div>
              <p class="truncate text-[12.5px] font-semibold">{item.title}</p>
              <p class="-mt-1.5 truncate text-xs text-muted">{item.authors}</p>
            </div>
          );
        }}
      </For>
    </section>
  );
}
