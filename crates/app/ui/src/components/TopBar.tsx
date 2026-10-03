import { onCleanup, Show } from 'solid-js';

import type { SortKey } from '../api';
import { chooseLibrary, items, library, query, queryError, search, setSort, sort } from '../library';

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

  const count = () => {
    const total = library()?.bookCount ?? 0;
    const shown = items().length;
    return shown === total ? `${total.toLocaleString()} books` : `${shown.toLocaleString()} of ${total.toLocaleString()}`;
  };

  const libraryTitle = () => {
    const l = library();
    return l
      ? `${l.path}\nLoaded ${l.bookCount.toLocaleString()} books in ${l.loadMs} ms\nClick to open another library…`
      : 'Open a library…';
  };

  return (
    <header class="topbar" data-tauri-drag-region>
      <button class="library-button" title={libraryTitle()} onClick={() => void chooseLibrary()}>
        <span class="library-name">{library()?.name ?? 'Magnitide'}</span>
        <Show when={library()}>
          <span class="library-count">{count()}</span>
        </Show>
      </button>

      <div class="search" data-tauri-drag-region>
        <svg class="search-icon" viewBox="0 0 20 20" aria-hidden="true">
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
          classList={{ invalid: queryError() !== null }}
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
        <kbd class="search-hint">/</kbd>
      </div>

      <label class="sort">
        <span class="visually-hidden">Sort by</span>
        <select value={sort()} onChange={(e) => setSort(e.currentTarget.value as SortKey)}>
          {SORT_OPTIONS.map(([key, label]) => (
            <option value={key}>{label}</option>
          ))}
        </select>
      </label>
    </header>
  );
}
