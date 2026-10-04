import {
  type ColumnDef,
  columnResizingFeature,
  columnSizingFeature,
  columnVisibilityFeature,
  createColumnHelper,
  createTable,
  FlexRender,
  type Header,
  tableFeatures,
  type Updater,
} from '@tanstack/solid-table';
import { createVirtualizer } from '@tanstack/solid-virtual';
import { createEffect, For, Index, on, onMount, Show } from 'solid-js';

import type { BookRow, SortKey } from '../api';
import { sort, sortDir, toggleSort } from '../library';
import { columnSizing, effectiveColumnVisibility, setColumnSizing, setColumnVisibility } from '../view';
import { Cover } from './Cover';
import { Menu, MenuItem, MenuLabel } from './Menu';
import type { ViewHandle } from './CoverGrid';

const ROW_H = 40;
/** Always shown: every row needs a title. */
const LOCKED_COLUMN = 'title';

// Sorting happens in Rust (so the grid and table agree), not in TanStack;
// TanStack handles column sizing, resizing and visibility.
const features = tableFeatures({ columnSizingFeature, columnResizingFeature, columnVisibilityFeature });

type Meta = {
  /** The Rust sort key for this column; absent when it isn't sortable. */
  sortKey?: SortKey;
  align?: 'end';
  /** Shown in the column picker. */
  label: string;
};

const col = createColumnHelper<typeof features, BookRow>();

const dateFormat = new Intl.DateTimeFormat(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
/** `YYYY-MM-DD` (from Rust) in the user's locale, without timezone shifts. */
const formatDate = (iso: string | null) => (iso ? dateFormat.format(new Date(`${iso}T00:00:00`)) : '');

function formatSize(bytes: number | null): string {
  if (bytes === null) return '';
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
}

function stars(rating: number | null) {
  if (!rating) return null;
  const full = Math.floor(rating / 2);
  return (
    <span class="tracking-[1px] text-star" title={`${rating / 2} of 5`}>
      {'★'.repeat(full)}
      {rating % 2 ? '½' : ''}
    </span>
  );
}

const muted = (text: string | null) => <span class="text-muted">{text}</span>;

const columns: ColumnDef<typeof features, BookRow, any>[] = [
  col.accessor('title', {
    size: 320,
    minSize: 160,
    meta: { sortKey: 'title', label: 'Title' } satisfies Meta,
    header: 'Title',
    cell: (info) => {
      const b = info.row.original;
      return (
        <span class="flex min-w-0 items-center gap-2.5">
          <span class="relative h-[30px] w-5 flex-none overflow-hidden rounded-[3px] bg-surface-2 shadow-card">
            <Cover id={b.id} title={b.title} authors={b.authors} hasCover={b.hasCover} size="thumb" compact />
          </span>
          <span class="truncate font-medium">{b.title}</span>
        </span>
      );
    },
  }),
  col.accessor('authors', {
    id: 'author',
    size: 200,
    meta: { sortKey: 'author', label: 'Author' } satisfies Meta,
    header: 'Author',
  }),
  col.accessor('series', {
    size: 200,
    meta: { sortKey: 'series', label: 'Series' } satisfies Meta,
    header: 'Series',
    cell: (info) => {
      const b = info.row.original;
      return b.series ? (
        <span>
          {b.series} <span class="text-muted tabular-nums">#{b.seriesIndex}</span>
        </span>
      ) : null;
    },
  }),
  col.accessor('rating', {
    size: 100,
    meta: { sortKey: 'rating', label: 'Rating' } satisfies Meta,
    header: 'Rating',
    cell: (info) => stars(info.getValue()),
  }),
  col.accessor('published', {
    size: 120,
    meta: { sortKey: 'published', label: 'Published' } satisfies Meta,
    header: 'Published',
    cell: (info) => muted(formatDate(info.getValue())),
  }),
  col.accessor('added', {
    size: 120,
    meta: { sortKey: 'added', label: 'Date Added' } satisfies Meta,
    header: 'Added',
    cell: (info) => muted(formatDate(info.getValue())),
  }),
  col.accessor('publisher', {
    size: 170,
    meta: { sortKey: 'publisher', label: 'Publisher' } satisfies Meta,
    header: 'Publisher',
  }),
  col.accessor('tags', {
    size: 220,
    meta: { label: 'Tags' } satisfies Meta,
    header: 'Tags',
    cell: (info) => muted(info.getValue()),
  }),
  col.accessor('formats', {
    size: 150,
    meta: { label: 'Formats' } satisfies Meta,
    header: 'Formats',
    cell: (info) => (
      <span class="flex gap-1">
        <For each={info.getValue()}>
          {(f) => <span class="rounded bg-surface-2 px-1.5 text-[11px] font-medium text-muted">{f}</span>}
        </For>
      </span>
    ),
  }),
  col.accessor('size', {
    size: 90,
    meta: { sortKey: 'size', align: 'end', label: 'Size' } satisfies Meta,
    header: 'Size',
    cell: (info) => muted(formatSize(info.getValue())),
  }),
];

const meta = (h: { column: { columnDef: { meta?: unknown } } }) => h.column.columnDef.meta as Meta;
const apply = <T,>(updater: Updater<T>, prev: T): T =>
  typeof updater === 'function' ? (updater as (old: T) => T)(prev) : updater;

type Props = {
  rows: BookRow[];
  selectedId: number | null;
  emptyMessage: string;
  onSelect(id: number): void;
  onOpen(id: number): void;
  ref?: (handle: ViewHandle) => void;
};

/** Virtualized, sortable, resizable book table. */
export function BookTable(props: Props) {
  let scroller!: HTMLDivElement;

  const table = createTable({
    features,
    columns,
    get data() {
      return props.rows;
    },
    getRowId: (row) => String(row.id),
    columnResizeMode: 'onChange',
    state: {
      get columnSizing() {
        return columnSizing();
      },
      get columnVisibility() {
        return effectiveColumnVisibility();
      },
    },
    onColumnSizingChange: (u) => setColumnSizing(apply(u, columnSizing())),
    onColumnVisibilityChange: (u) => setColumnVisibility(apply(u, effectiveColumnVisibility())),
  });

  const virtualizer = createVirtualizer({
    get count() {
      return props.rows.length;
    },
    getScrollElement: () => scroller,
    estimateSize: () => ROW_H,
    overscan: 12,
  });

  // A new result set starts at the top.
  createEffect(
    on(
      () => props.rows,
      () => (scroller.scrollTop = 0),
      { defer: true },
    ),
  );

  onMount(() =>
    props.ref?.({
      reveal(id) {
        const i = props.rows.findIndex((r) => r.id === id);
        if (i >= 0) virtualizer.scrollToIndex(i, { align: 'auto' });
      },
      columns: () => 1,
      horizontal: false,
      focus: () => scroller.focus(),
    }),
  );

  const rows = () => table.getRowModel().rows;
  const totalWidth = () => table.getTotalSize();

  return (
    <div
      ref={scroller}
      class="relative min-w-0 flex-1 overflow-auto outline-none"
      tabIndex={0}
      role="table"
      aria-label="Books"
      aria-rowcount={props.rows.length}
    >
      <div
        class="sticky top-0 z-2 flex border-b border-line bg-bg/90 backdrop-blur"
        style={{ width: `${totalWidth()}px`, 'min-width': '100%' }}
        role="row"
      >
        {/* Index (not For): header objects are rebuilt on every resize, so reuse the DOM by position. */}
        <Index each={table.getHeaderGroups()[0]?.headers}>{(h) => <HeaderCell header={h()} />}</Index>
        <div class="sticky right-0 ml-auto flex items-center bg-[linear-gradient(to_right,transparent,var(--bg)_45%)] pr-2 pl-6">
          <ColumnPicker table={table} />
        </div>
      </div>

      <Show when={props.rows.length === 0}>
        <p class="absolute inset-x-0 top-[30%] text-center text-sm text-muted">{props.emptyMessage}</p>
      </Show>

      <div
        class="relative"
        style={{ height: `${virtualizer.getTotalSize()}px`, width: `${totalWidth()}px`, 'min-width': '100%' }}
      >
        <For each={virtualizer.getVirtualItems()}>
          {(v) => {
            const row = () => rows()[v.index];
            return (
              <Show when={row()}>
                {(r) => (
                  <div
                    role="row"
                    aria-rowindex={v.index + 2}
                    class="absolute top-0 left-0 flex w-full cursor-default items-center border-b border-line/50 hover:bg-line data-selected:bg-accent/20"
                    data-selected={r().original.id === props.selectedId || undefined}
                    style={{ height: `${ROW_H}px`, transform: `translateY(${v.start}px)` }}
                    onClick={() => props.onSelect(r().original.id)}
                    onDblClick={() => props.onOpen(r().original.id)}
                  >
                    <For each={r().getVisibleCells()}>
                      {(cell) => (
                        <div
                          role="cell"
                          class="flex-none truncate px-3"
                          classList={{ 'text-right tabular-nums': meta(cell).align === 'end' }}
                          style={{ width: `${cell.column.getSize()}px` }}
                        >
                          <FlexRender cell={cell} />
                        </div>
                      )}
                    </For>
                  </div>
                )}
              </Show>
            );
          }}
        </For>
      </div>
    </div>
  );
}

function HeaderCell(props: { header: Header<typeof features, BookRow, unknown> }) {
  const h = () => props.header;
  const key = () => meta(h()).sortKey;
  const active = () => key() !== undefined && key() === sort();
  return (
    <div
      role="columnheader"
      aria-sort={active() ? (sortDir() === 'asc' ? 'ascending' : 'descending') : undefined}
      class="group/h relative flex h-8 flex-none items-center text-xs font-semibold text-muted"
      style={{ width: `${h().getSize()}px` }}
    >
      <button
        class="flex h-full w-full min-w-0 items-center gap-1 px-3 text-left enabled:hover:text-fg disabled:cursor-default"
        classList={{ 'justify-end': meta(h()).align === 'end', 'text-fg': active() }}
        disabled={key() === undefined}
        onClick={() => {
          const k = key();
          if (k) toggleSort(k);
        }}
      >
        <span class="truncate">
          <FlexRender header={h()} />
        </span>
        <Show when={active()}>
          <svg class="size-2.5 flex-none fill-none stroke-current stroke-2" viewBox="0 0 10 10" aria-hidden="true">
            <path d={sortDir() === 'asc' ? 'm2 6.5 3-3 3 3' : 'm2 3.5 3 3 3-3'} />
          </svg>
        </Show>
      </button>
      {/* Drag to resize; double-click to reset. */}
      <div
        class="absolute top-1.5 right-0 bottom-1.5 w-[5px] cursor-col-resize border-r border-line group-hover/h:border-faint data-resizing:border-accent"
        data-resizing={h().column.getIsResizing() || undefined}
        onMouseDown={(e) => h().getResizeHandler()(e)}
        onTouchStart={(e) => h().getResizeHandler()(e)}
        onDblClick={() => h().column.resetSize()}
        onClick={(e) => e.stopPropagation()}
      />
    </div>
  );
}

function ColumnPicker(props: { table: ReturnType<typeof createTable<typeof features, BookRow>> }) {
  return (
    <Menu
      label="Columns"
      iconOnly
      align="end"
      buttonClass="grid size-6 place-items-center rounded-md hover:bg-line aria-expanded:bg-line"
      button={
        <svg class="size-3.5 fill-muted" viewBox="0 0 16 16" aria-hidden="true">
          <circle cx="3" cy="8" r="1.5" />
          <circle cx="8" cy="8" r="1.5" />
          <circle cx="13" cy="8" r="1.5" />
        </svg>
      }
    >
      <MenuLabel>Columns</MenuLabel>
      <For each={props.table.getAllLeafColumns()}>
        {(column) => (
          <MenuItem
            checkbox
            keepOpen
            checked={column.getIsVisible()}
            disabled={column.id === LOCKED_COLUMN}
            onSelect={() => column.toggleVisibility()}
          >
            {(column.columnDef.meta as Meta).label}
          </MenuItem>
        )}
      </For>
    </Menu>
  );
}

/** Table columns for the settings panel: id, label, and whether it can be hidden. */
export const TABLE_COLUMNS = columns.map((c) => {
  const id = c.id ?? ('accessorKey' in c ? String(c.accessorKey) : '');
  return { id, label: (c.meta as Meta).label, locked: id === LOCKED_COLUMN };
});
