import { onCleanup, Show } from 'solid-js';
import ArrowDownWideNarrow from 'lucide-solid/icons/arrow-down-wide-narrow';
import ArrowUpNarrowWide from 'lucide-solid/icons/arrow-up-narrow-wide';
import LayoutGrid from 'lucide-solid/icons/layout-grid';
import List from 'lucide-solid/icons/list';
import PanelLeft from 'lucide-solid/icons/panel-left';
import Search from 'lucide-solid/icons/search';
import Settings from 'lucide-solid/icons/settings';
import X from 'lucide-solid/icons/x';

import type { SortKey } from '../api';
import type { SettingsSection } from './Settings';
import { query, queryError, search, setSort, sort, sortDir } from '../library';
import { setSidebarOpen, setViewMode, sidebarOpen, viewMode } from '../view';
import { CoverSizeSlider } from './CoverSizeSlider';
import { DeviceMenu } from './DeviceMenu';
import { LibraryMenu } from './LibraryMenu';
import { Select } from './Select';
import { ThemeMenu } from './ThemeMenu';
import { PluginsMenu } from './PluginMenus';
import { Icon } from './Icon';
import { keys } from '../keys';

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
  'grid h-[26px] w-8 place-items-center rounded-inset text-muted hover:text-fg aria-pressed:bg-popover aria-pressed:text-fg aria-pressed:shadow-card';

function ViewSwitcher() {
  return (
    <div class="flex rounded-control bg-element p-0.5" role="group" aria-label="View">
      <button
        class={segment}
        aria-pressed={viewMode() === 'grid'}
        title={`Covers (${keys('mod+1')})`}
        onClick={() => setViewMode('grid')}
      >
        <Icon of={LayoutGrid} />
      </button>
      <button
        class={segment}
        aria-pressed={viewMode() === 'table'}
        title={`List (${keys('mod+2')})`}
        onClick={() => setViewMode('table')}
      >
        <Icon of={List} />
      </button>
    </div>
  );
}

type Props = {
  searchRef?: (el: HTMLInputElement) => void;
  /** Leave the search box (Esc on empty, Enter, ↓). */
  onLeaveSearch(): void;
  /** Opens Settings, at `section` if given. */
  onOpenSettings(section?: SettingsSection): void;
};

export function TopBar(props: Props) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(timer));

  return (
    <header
      class="fixed inset-x-0 top-(--titlebar-h) z-10 grid h-(--topbar-h) grid-cols-[minmax(0,1fr)_minmax(160px,2fr)_minmax(max-content,1fr)] items-center gap-4 border-b border-line bg-bg/80 px-4 backdrop-blur-[18px] backdrop-saturate-[1.6]"
      data-tauri-drag-region
    >
      <div class="flex min-w-0 items-center gap-1 justify-self-start" data-tauri-drag-region>
        <button
          class="grid size-7 flex-none place-items-center rounded-control text-muted hover:bg-hover hover:text-fg aria-pressed:text-fg"
          aria-pressed={sidebarOpen()}
          title={sidebarOpen() ? 'Hide shelves' : 'Show shelves'}
          aria-label="Shelves sidebar"
          onClick={() => setSidebarOpen(!sidebarOpen())}
        >
          <Icon of={PanelLeft} size={16} />
        </button>
        <LibraryMenu />
        <DeviceMenu />
      </div>

      <div class="relative flex w-full max-w-[520px] items-center justify-self-center" data-tauri-drag-region>
        <Icon of={Search} class="pointer-events-none absolute left-2.5 text-faint" />
        <input
          ref={props.searchRef}
          type="search"
          placeholder='Search title, author, tag:fantasy, series:"…"'
          autocomplete="off"
          spellcheck={false}
          value={query()}
          class="peer h-8 w-full rounded-control border border-line bg-field pr-[30px] pl-8 transition-[border-color,box-shadow] duration-150 outline-none select-text focus:border-focus focus:ring-3 focus:ring-focus/20 [&::-webkit-search-cancel-button]:hidden"
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
        <kbd class="pointer-events-none absolute right-2 rounded-inset border border-line px-1.5 font-sans text-[11px]/4 text-faint peer-focus:hidden peer-[:not(:placeholder-shown)]:hidden">
          /
        </kbd>
        {query() && (
          <button
            class="absolute right-2 grid size-5 place-items-center rounded-inset text-muted hover:bg-hover hover:text-fg"
            aria-label="Clear search"
            onClick={() => {
              clearTimeout(timer);
              search('');
            }}
          >
            <Icon of={X} size={12} />
          </button>
        )}
      </div>

      <div class="flex items-center gap-3 justify-self-end">
        <Show when={viewMode() === 'grid'}>
          <div class="max-[1080px]:hidden">
            <CoverSizeSlider />
          </div>
        </Show>
        <ViewSwitcher />
        <label>
          <span class="sr-only">Sort by</span>
          <Select class="h-[30px] pl-2.5" value={sort()} onChange={(e) => setSort(e.currentTarget.value as SortKey)}>
            {SORT_OPTIONS.map(([key, label]) => (
              <option value={key}>{label}</option>
            ))}
          </Select>
        </label>
        <button
          class="-ml-2 grid size-7 place-items-center rounded-control text-muted hover:bg-hover hover:text-fg"
          title={sortDir() === 'asc' ? 'Ascending (click to reverse)' : 'Descending (click to reverse)'}
          aria-label={sortDir() === 'asc' ? 'Sort ascending' : 'Sort descending'}
          onClick={() => setSort(sort(), sortDir() === 'asc' ? 'desc' : 'asc')}
        >
          <Icon of={sortDir() === 'asc' ? ArrowUpNarrowWide : ArrowDownWideNarrow} />
        </button>
        <PluginsMenu onManage={() => props.onOpenSettings('plugins')} />
        <ThemeMenu />
        <button
          class="grid size-8 place-items-center rounded-control text-muted hover:bg-hover hover:text-fg"
          title={`Settings (${keys('mod+,')})`}
          aria-label="Settings"
          onClick={() => props.onOpenSettings()}
        >
          <Icon of={Settings} size={16} />
        </button>
      </div>
    </header>
  );
}
