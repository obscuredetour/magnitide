import { getCurrentWindow } from '@tauri-apps/api/window';
import { onCleanup, onMount, Show } from 'solid-js';
import ArrowDownWideNarrow from 'lucide-solid/icons/arrow-down-wide-narrow';
import ArrowUpNarrowWide from 'lucide-solid/icons/arrow-up-narrow-wide';
import LayoutGrid from 'lucide-solid/icons/layout-grid';
import List from 'lucide-solid/icons/list';
import PanelLeft from 'lucide-solid/icons/panel-left';
import Search from 'lucide-solid/icons/search';
import Settings from 'lucide-solid/icons/settings';
import X from 'lucide-solid/icons/x';

import type { SortKey } from '../api';
import type { SettingsSection } from './settings/Settings';
import { query, queryError, search, setSort, sort, sortDir } from '../library';
import { setSidebarOpen, setViewMode, sidebarOpen, toolbarLayout, viewMode } from '../view';
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

function SidebarToggle() {
  return (
    <button
      class="grid size-7 flex-none place-items-center rounded-control text-muted hover:bg-hover hover:text-fg aria-pressed:text-fg"
      aria-pressed={sidebarOpen()}
      title={sidebarOpen() ? 'Hide shelves' : 'Show shelves'}
      aria-label="Shelves sidebar"
      onClick={() => setSidebarOpen(!sidebarOpen())}
    >
      <Icon of={PanelLeft} size={16} />
    </button>
  );
}

function SearchField(props: Pick<Props, 'searchRef' | 'onLeaveSearch'>) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(timer));
  return (
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
  );
}

/** How books show: cover size (covers only, and only where there's room), covers or list, and their order. */
function ViewControls(props: { slider: string }) {
  return (
    <div class="flex items-center gap-3 justify-self-end">
      <Show when={viewMode() === 'grid'}>
        <div class={props.slider}>
          <CoverSizeSlider />
        </div>
      </Show>
      <ViewSwitcher />
      <div class="flex items-center gap-1">
        <label>
          <span class="sr-only">Sort by</span>
          <Select class="h-[30px] pl-2.5" value={sort()} onChange={(e) => setSort(e.currentTarget.value as SortKey)}>
            {SORT_OPTIONS.map(([key, label]) => (
              <option value={key}>{label}</option>
            ))}
          </Select>
        </label>
        <button
          class="grid size-7 place-items-center rounded-control text-muted hover:bg-hover hover:text-fg"
          title={sortDir() === 'asc' ? 'Ascending (click to reverse)' : 'Descending (click to reverse)'}
          aria-label={sortDir() === 'asc' ? 'Sort ascending' : 'Sort descending'}
          onClick={() => setSort(sort(), sortDir() === 'asc' ? 'desc' : 'asc')}
        >
          <Icon of={sortDir() === 'asc' ? ArrowUpNarrowWide : ArrowDownWideNarrow} />
        </button>
      </div>
    </div>
  );
}

/** The app's own controls: plugin menus, theme, settings. */
function AppControls(props: Pick<Props, 'onOpenSettings'>) {
  return (
    <div class="flex flex-none items-center gap-1">
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
  );
}

/** Whether a pointer event is on an open dialog's backdrop, over the toolbar (`bar`). */
function onBackdropOverBar(e: MouseEvent, bar: HTMLElement): boolean {
  if (!(e.target instanceof HTMLDialogElement) || !e.target.open) return false;
  // The backdrop's clicks target the dialog itself; make sure it's outside the dialog's box.
  const box = e.target.getBoundingClientRect();
  const inside = e.clientX >= box.left && e.clientX <= box.right && e.clientY >= box.top && e.clientY <= box.bottom;
  return !inside && e.clientY < bar.getBoundingClientRect().bottom;
}

/**
 * macOS: the toolbar is the window's title bar (dragging or double-clicking
 * its empty space moves or zooms the window), so a modal dialog's backdrop
 * over it does the same, and macOS hides the traffic lights in full screen,
 * so their room goes too (data-fullscreen, styles.css).
 */
function useMacTitleBar(bar: () => HTMLElement) {
  onMount(() => {
    if (!document.documentElement.classList.contains('is-mac')) return;
    // A modal dialog covers the page with its backdrop, so the drag region never
    // sees presses there; move or zoom the window from the backdrop instead, and
    // keep that press from counting as a click on the backdrop (which closes
    // some dialogs).
    const press = (e: MouseEvent) => {
      if (e.button !== 0 || !onBackdropOverBar(e, bar())) return;
      e.preventDefault();
      e.stopPropagation();
      const win = getCurrentWindow();
      void (e.detail === 2 ? win.toggleMaximize() : win.startDragging());
    };
    const click = (e: MouseEvent) => {
      if (onBackdropOverBar(e, bar())) e.stopPropagation();
    };
    addEventListener('mousedown', press, true);
    addEventListener('click', click, true);
    onCleanup(() => {
      removeEventListener('mousedown', press, true);
      removeEventListener('click', click, true);
    });

    try {
      const win = getCurrentWindow();
      const sync = () =>
        win.isFullscreen().then(
          (full) => document.documentElement.toggleAttribute('data-fullscreen', full),
          () => {},
        );
      void sync();
      const unlisten = win.onResized(() => void sync());
      onCleanup(() => void unlisten.then((off) => off()));
    } catch {
      // Not running in Tauri.
    }
  });
}

type Props = {
  searchRef?: (el: HTMLInputElement) => void;
  /** Leave the search box (Esc on empty, Enter, ↓). */
  onLeaveSearch(): void;
  /** Opens Settings, at `section` if given. */
  onOpenSettings(section?: SettingsSection): void;
};

/**
 * The toolbar. Two rows: the library, devices and the app's controls above
 * (beside the traffic lights on macOS); the sidebar, search and how books
 * show below. Or, set in Settings › Appearance, all in one row. Its empty
 * space drags the window (data-tauri-drag-region, on each element that's
 * empty space, since it doesn't pass to children).
 */
export function TopBar(props: Props) {
  let bar!: HTMLElement;
  useMacTitleBar(() => bar);
  // Room for the traffic lights, then the usual 16px.
  const start = 'pl-[calc(var(--lights-w)+16px)]';

  return (
    <header
      ref={bar}
      class="fixed inset-x-0 top-0 z-10 flex h-(--topbar-h) flex-col border-b border-line bg-bg/80 backdrop-blur-[18px] backdrop-saturate-[1.6]"
      data-tauri-drag-region
    >
      <Show
        when={toolbarLayout() === 'two-rows'}
        fallback={
          <div
            class={`grid h-(--toolbar-row) grid-cols-[minmax(0,1fr)_minmax(160px,2fr)_minmax(max-content,1fr)] items-center gap-4 pr-4 ${start}`}
            data-tauri-drag-region
          >
            <div class="flex min-w-0 items-center gap-1 justify-self-start" data-tauri-drag-region>
              <SidebarToggle />
              <LibraryMenu />
              <DeviceMenu narrow />
            </div>
            <SearchField searchRef={props.searchRef} onLeaveSearch={props.onLeaveSearch} />
            <div class="flex items-center gap-3 justify-self-end" data-tauri-drag-region>
              <ViewControls slider="max-[1080px]:hidden" />
              <AppControls onOpenSettings={props.onOpenSettings} />
            </div>
          </div>
        }
      >
        <div class={`flex h-(--toolbar-row) items-center gap-4 pr-3 ${start}`} data-tauri-drag-region>
          <div class="flex min-w-0 flex-1 items-center gap-1" data-tauri-drag-region>
            <LibraryMenu />
            <DeviceMenu />
          </div>
          <AppControls onOpenSettings={props.onOpenSettings} />
        </div>
        <div
          class="grid h-(--toolbar-row-2) grid-cols-[minmax(0,1fr)_minmax(160px,2fr)_minmax(max-content,1fr)] items-center gap-4 px-4 pb-1.5"
          data-tauri-drag-region
        >
          <div class="flex items-center justify-self-start" data-tauri-drag-region>
            <SidebarToggle />
          </div>
          <SearchField searchRef={props.searchRef} onLeaveSearch={props.onLeaveSearch} />
          <ViewControls slider="max-[880px]:hidden" />
        </div>
      </Show>
    </header>
  );
}
