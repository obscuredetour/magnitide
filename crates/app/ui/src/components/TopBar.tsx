import { onCleanup, Show } from 'solid-js';

import type { SortKey } from '../api';
import { query, queryError, search, setSort, sort, sortDir } from '../library';
import { setViewMode, viewMode } from '../view';
import { CoverSizeSlider } from './CoverSizeSlider';
import { LibraryMenu } from './LibraryMenu';
import { ThemeMenu } from './ThemeMenu';

const SORT_OPTIONS: [SortKey, string][] = [
  ['title', 'Title'],
  ['author', 'Author'],
  ['series', 'Series'],
  ['rating', 'Rating'],
  ['added', 'Date Added'],
  ['published', 'Published'],
  ['publisher', 'Publisher'],
  ['size', 'Size'],
];

const segment =
  'grid h-[26px] w-8 place-items-center rounded-md text-muted hover:text-fg aria-pressed:bg-surface aria-pressed:text-fg aria-pressed:shadow-card';

function ViewSwitcher() {
  return (
    <div class="flex rounded-lg bg-line p-0.5" role="group" aria-label="View">
      <button
        class={segment}
        aria-pressed={viewMode() === 'grid'}
        title="Covers (⌘1)"
        onClick={() => setViewMode('grid')}
      >
        <svg class="size-3.5 fill-current" viewBox="0 0 16 16" aria-hidden="true">
          <rect x="1" y="1" width="6" height="6" rx="1.2" />
          <rect x="9" y="1" width="6" height="6" rx="1.2" />
          <rect x="1" y="9" width="6" height="6" rx="1.2" />
          <rect x="9" y="9" width="6" height="6" rx="1.2" />
        </svg>
      </button>
      <button
        class={segment}
        aria-pressed={viewMode() === 'table'}
        title="List (⌘2)"
        onClick={() => setViewMode('table')}
      >
        <svg class="size-3.5 fill-current" viewBox="0 0 16 16" aria-hidden="true">
          <rect x="1" y="2" width="14" height="2.4" rx="1" />
          <rect x="1" y="6.8" width="14" height="2.4" rx="1" />
          <rect x="1" y="11.6" width="14" height="2.4" rx="1" />
        </svg>
      </button>
    </div>
  );
}

type Props = {
  searchRef?: (el: HTMLInputElement) => void;
  /** Leave the search box (Esc on empty, Enter, ↓). */
  onLeaveSearch(): void;
};

export function TopBar(props: Props) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(timer));

  return (
    <header
      class="fixed inset-x-0 top-0 z-10 grid h-[52px] grid-cols-[minmax(0,1fr)_minmax(160px,2fr)_minmax(max-content,1fr)] items-center gap-4 border-b border-line bg-bg/80 px-4 backdrop-blur-[18px] backdrop-saturate-[1.6] mac:pl-[84px]"
      data-tauri-drag-region
    >
      <LibraryMenu />

      <div class="relative flex w-full max-w-[520px] items-center justify-self-center" data-tauri-drag-region>
        <svg
          class="pointer-events-none absolute left-2.5 size-[15px] fill-none stroke-faint stroke-[1.8] [stroke-linecap:round]"
          viewBox="0 0 20 20"
          aria-hidden="true"
        >
          <circle cx="8.5" cy="8.5" r="5.5" />
          <path d="m13 13 4 4" />
        </svg>
        <input
          ref={props.searchRef}
          type="search"
          placeholder='Search title, author, tag:fantasy, series:"…"'
          autocomplete="off"
          spellcheck={false}
          value={query()}
          class="peer h-8 w-full rounded-[9px] border border-line bg-surface pr-[30px] pl-8 transition-[border-color,box-shadow] duration-150 outline-none select-text focus:border-accent focus:ring-3 focus:ring-accent/15 [&::-webkit-search-cancel-button]:hidden"
          classList={{ 'border-danger! ring-3 ring-danger/20!': queryError() !== null }}
          title={queryError() ?? ''}
          onInput={(e) => {
            const text = e.currentTarget.value;
            clearTimeout(timer);
            timer = setTimeout(() => search(text), 120);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.stopPropagation();
              if (e.currentTarget.value) {
                clearTimeout(timer);
                search('');
              } else props.onLeaveSearch();
            } else if (e.key === 'Enter' || e.key === 'ArrowDown') {
              e.preventDefault();
              props.onLeaveSearch();
            }
          }}
        />
        <kbd class="pointer-events-none absolute right-2 rounded-[5px] border border-line px-1.5 font-sans text-[11px]/4 text-faint peer-focus:hidden peer-[:not(:placeholder-shown)]:hidden">
          /
        </kbd>
      </div>

      <div class="flex items-center gap-3 justify-self-end">
        <ViewSwitcher />
        <Show when={viewMode() === 'grid'}>
          <div class="max-[1080px]:hidden">
            <CoverSizeSlider />
          </div>
        </Show>
        <label>
          <span class="sr-only">Sort by</span>
          <select
            class="h-[30px] rounded-lg border border-line bg-surface px-2.5"
            value={sort()}
            onChange={(e) => setSort(e.currentTarget.value as SortKey)}
          >
            {SORT_OPTIONS.map(([key, label]) => (
              <option value={key}>{label}</option>
            ))}
          </select>
        </label>
        <button
          class="-ml-2 grid size-7 place-items-center rounded-md text-muted hover:bg-line hover:text-fg"
          title={sortDir() === 'asc' ? 'Ascending (click to reverse)' : 'Descending (click to reverse)'}
          aria-label={sortDir() === 'asc' ? 'Sort ascending' : 'Sort descending'}
          onClick={() => setSort(sort(), sortDir() === 'asc' ? 'desc' : 'asc')}
        >
          <svg
            class="size-3 fill-none stroke-current stroke-[1.6] [stroke-linecap:round]"
            viewBox="0 0 12 12"
            aria-hidden="true"
          >
            <path d={sortDir() === 'asc' ? 'M6 10V2M3 5l3-3 3 3' : 'M6 2v8M3 7l3 3 3-3'} />
          </svg>
        </button>
        <ThemeMenu />
      </div>
    </header>
  );
}
