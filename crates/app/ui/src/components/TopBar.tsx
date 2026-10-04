import { onCleanup } from 'solid-js';

import type { SortKey } from '../api';
import { query, queryError, search, setSort, sort } from '../library';
import { CoverSizeSlider } from './CoverSizeSlider';
import { LibraryMenu } from './LibraryMenu';
import { ThemeMenu } from './ThemeMenu';

const SORT_OPTIONS: [SortKey, string][] = [
  ['title', 'Title'],
  ['author', 'Author'],
  ['added', 'Recently added'],
  ['published', 'Recently published'],
  ['series', 'Series'],
  ['rating', 'Rating'],
];

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
        <div class="max-[960px]:hidden">
          <CoverSizeSlider />
        </div>
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
        <ThemeMenu />
      </div>
    </header>
  );
}
