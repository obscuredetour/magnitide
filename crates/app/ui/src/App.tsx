import { createEffect, createSignal, Match, on, onCleanup, onMount, Show, Switch } from 'solid-js';

import { BookMenuHost, openBookMenu } from './components/BookMenu';
import { BookTable } from './components/BookTable';
import { type ArrowKey, CoverGrid, type ViewHandle } from './components/CoverGrid';
import { ShelfView } from './components/ShelfView';
import { DetailsPanel } from './components/DetailsPanel';
import { DropOverlay } from './components/DropOverlay';
import { BulkEditDialog } from './components/BulkEditDialog';
import { EditDialog } from './components/EditDialog';
import { EmptyState } from './components/EmptyState';
import { ImportDialog } from './components/ImportDialog';
import { NetworkWarning } from './components/NetworkWarning';
import { SelectionBar } from './components/SelectionBar';
import { SettingsDialog, type SettingsSection } from './components/Settings';
import { StatusBar } from './components/StatusBar';
import { QuestionHost } from './question';
import { ConfirmHost } from './confirm';
import { TitleBar } from './components/TitleBar';
import { TopBar } from './components/TopBar';
import { selectedInOrder, shownItems, watchDevices } from './devices';
import {
  detailsOpen,
  items,
  library,
  openBook,
  openInitialLibrary,
  query,
  selectedId,
  setDetailsOpen,
  selectAll,
  selectedIds,
  selectRange,
  setSelectedId,
  status,
  toggleSelected,
} from './library';
import { onOpenSettings, onZoomMenu } from './api';
import { bulkIds, editBooks, editingBook } from './editor';
import { LookupHost } from './lookup';
import { importState } from './importer';
import { Toast } from './toast';
import { deleteBooks } from './transfer';
import { loadThemeAddons } from './theme';
import { coverSize, setViewMode, sidebarOpen, viewMode } from './view';
import { resetZoom, stepZoom } from './zoom';
import { drag, dragJustEnded, startBookDrag } from './bookDrag';
import { addToShelf, newShelfFor, refreshShelves, shelves } from './shelves';
import { initPlugins } from './plugins';
import { Sidebar } from './components/Sidebar';

export function App() {
  let view: ViewHandle | undefined;
  let searchInput: HTMLInputElement | undefined;
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [settingsSection, setSettingsSection] = createSignal<SettingsSection>();
  const openSettings = (section?: SettingsSection) => {
    setSettingsSection(section);
    setSettingsOpen(true);
  };

  const noLibrary = () => {
    const s = status();
    return s.kind === 'no-library' ? s : null;
  };

  const select = (id: number, { open = true, reveal = false } = {}) => {
    setSelectedId(id);
    if (open) setDetailsOpen(true);
    if (reveal) view?.reveal(id);
  };

  /** A click in a view: ⌘ adds or removes, Shift selects a range, else select just this one. */
  const click = (id: number, e: MouseEvent) => {
    if (dragJustEnded()) return;
    if (e.metaKey || e.ctrlKey) toggleSelected(id);
    else if (e.shiftKey) selectRange(id, shownItems());
    else select(id);
  };

  /** Right-click on a book: its menu, for the selection if it's in it, else for it alone (as in Finder). */
  const contextMenu = (id: number, e: MouseEvent) => {
    e.preventDefault();
    if (!selectedIds().has(id)) select(id, { open: false });
    openBookMenu(e.clientX, e.clientY, selectedInOrder());
  };

  /** The selected books, in list order (at least the primary one). */
  const chosen = (id: number) => (selectedInOrder().length ? selectedInOrder() : [id]);

  // Opening/closing the panel or switching views changes the layout; keep the selection in view.
  createEffect(
    on(
      [detailsOpen, viewMode],
      () => {
        const id = selectedId();
        if (id !== null) requestAnimationFrame(() => view?.reveal(id));
      },
      { defer: true },
    ),
  );

  const leaveSearch = () => {
    view?.focus();
    const list = shownItems();
    const first = list[0];
    if (first && !list.some((it) => it.id === selectedId())) select(first.id, { open: false, reveal: true });
  };

  const onKeyDown = (e: KeyboardEvent) => {
    // Key events normally target an element; fall back to <body> for anything else.
    const target = e.target instanceof HTMLElement ? e.target : document.body;
    const inField = target instanceof HTMLInputElement || target instanceof HTMLSelectElement;
    const cmd = e.metaKey || e.ctrlKey;

    // ⌘, opens settings (macOS convention). While it's open, its own controls own the keyboard.
    if (cmd && e.key === ',') {
      e.preventDefault();
      openSettings();
      return;
    }
    // ⌘+ / ⌘− / ⌘0: interface size, anywhere. On macOS the View menu owns ⌘=, ⌘− and ⌘0; ⌘⇧= (⌘+) comes here.
    const mac = document.documentElement.classList.contains('is-mac');
    if (cmd && !e.altKey && (e.key === '+' || (!mac && (e.key === '=' || e.key === '-' || e.key === '0')))) {
      e.preventDefault();
      if (e.key === '0') resetZoom();
      else stepZoom(e.key === '-' ? -1 : 1);
      return;
    }
    if (target.closest('dialog')) return;

    if ((e.key === '/' && !inField) || (e.key === 'f' && cmd)) {
      e.preventDefault();
      searchInput?.focus();
      searchInput?.select();
      return;
    }
    // ⌘1 / ⌘2 / ⌘3 switch views, as in Finder.
    const mode = cmd && ({ '1': 'grid', '2': 'table', '3': 'shelf' } as const)[e.key];
    if (mode) {
      e.preventDefault();
      setViewMode(mode);
      return;
    }
    if (inField) return;

    const id = selectedId();
    if (e.key === 'a' && cmd) {
      e.preventDefault();
      selectAll(shownItems());
    } else if (e.key === 'Escape') {
      // First back to one book, then close the panel.
      if (selectedIds().size > 1) setSelectedId(id);
      else setDetailsOpen(false);
    } else if (e.key === 'e' && cmd && id !== null) {
      e.preventDefault();
      editBooks(chosen(id));
    } else if (((e.key === 'Backspace' && cmd) || e.key === 'Delete') && id !== null) {
      e.preventDefault();
      void deleteBooks(chosen(id));
    } else if ((e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) && id !== null) {
      // The keyboard's menu key: open it beside the selected book.
      e.preventDefault();
      view?.reveal(id);
      const r = document.querySelector(`[data-book-id="${id}"]`)?.getBoundingClientRect();
      if (r) openBookMenu(r.left + Math.min(32, r.width / 2), r.top + Math.min(r.height, 28), chosen(id));
    } else if (e.key === 'o' && cmd && id !== null) {
      e.preventDefault();
      void openBook(id);
    } else if ((e.key === 'Enter' || e.key === ' ') && id !== null) {
      // Let focused buttons and links handle their own activation.
      if (target.closest('button, a')) return;
      e.preventDefault();
      setDetailsOpen(true);
    } else if (e.key.startsWith('Arrow')) {
      const list = shownItems();
      if (!list.length) return;
      const key = e.key as ArrowKey;
      let nextId: number | null | undefined;
      if (view?.neighbour) nextId = view.neighbour(id, key);
      else {
        // Views in fixed columns: step by index.
        const cols = view?.columns() ?? 1;
        const horizontal = view?.horizontal ? 1 : 0;
        const step = { ArrowLeft: -horizontal, ArrowRight: horizontal, ArrowUp: -cols, ArrowDown: cols }[key];
        if (!step) return;
        const cur = list.findIndex((it) => it.id === id);
        nextId = list[cur < 0 ? 0 : Math.min(list.length - 1, Math.max(0, cur + step))]?.id;
      }
      if (nextId === null || nextId === undefined) return;
      e.preventDefault();
      select(nextId, { open: detailsOpen(), reveal: true });
    }
  };

  // Drag books (a cover or row; the selection if it's in it) onto a shelf in the sidebar.
  const pressBook = (e: PointerEvent) => {
    const el = (e.target as Element | null)?.closest<HTMLElement>('[data-book-id]');
    if (!el || library()?.kind !== 'magnitide') return;
    const id = Number(el.dataset.bookId);
    startBookDrag(
      e,
      () => (selectedIds().has(id) ? selectedInOrder() : [id]),
      (target, ids) => {
        if (target === 'new') void newShelfFor(ids);
        else {
          const shelf = shelves().find((s) => s.id === target);
          if (shelf) void addToShelf(shelf, ids);
        }
      },
    );
  };

  // Shelf counts follow the books (deleting, merging, adding).
  createEffect(on([items, library], () => void refreshShelves()));

  onMount(() => {
    document.addEventListener('pointerdown', pressBook);
    onCleanup(() => document.removeEventListener('pointerdown', pressBook));
    document.addEventListener('keydown', onKeyDown);
    // On macOS the menu bar item owns ⌘, (so onKeyDown's ⌘, branch only runs on other platforms).
    const unlisten = onOpenSettings(() => openSettings());
    onCleanup(() => void unlisten.then((off) => off()));
    const unlistenZoom = onZoomMenu((action) =>
      action === 'reset' ? resetZoom() : stepZoom(action === 'in' ? 1 : -1),
    );
    onCleanup(() => void unlistenZoom.then((off) => off()));
    void openInitialLibrary();
    watchDevices();
    initPlugins();
    void loadThemeAddons().catch(() => {}); // the cached copy already applies
  });
  onCleanup(() => document.removeEventListener('keydown', onKeyDown));

  const emptyMessage = () =>
    query().trim() ? `No books match “${query().trim()}”.` : 'This library has no books yet.';

  return (
    <>
      <TitleBar />
      <TopBar searchRef={(el) => (searchInput = el)} onLeaveSearch={leaveSearch} onOpenSettings={openSettings} />
      <Switch>
        <Match when={status().kind === 'ready'}>
          <main class="fixed inset-x-0 top-(--chrome-h) bottom-(--statusbar-h) flex flex-col">
            <NetworkWarning />
            <div class="relative flex min-h-0 flex-1">
              <Show when={sidebarOpen()}>
                <Sidebar />
              </Show>
              <Switch
                fallback={
                  <CoverGrid
                    ref={(h) => (view = h)}
                    items={shownItems()}
                    tileSize={coverSize()}
                    selectedId={selectedId()}
                    selectedIds={selectedIds()}
                    emptyMessage={emptyMessage()}
                    emptyMark={!query().trim()}
                    onSelect={click}
                    onContextMenu={contextMenu}
                    onOpen={(id) => void openBook(id)}
                  />
                }
              >
                <Match when={viewMode() === 'table'}>
                  <BookTable
                    ref={(h) => (view = h)}
                    rows={shownItems()}
                    selectedId={selectedId()}
                    selectedIds={selectedIds()}
                    emptyMessage={emptyMessage()}
                    emptyMark={!query().trim()}
                    onSelect={click}
                    onContextMenu={contextMenu}
                    onOpen={(id) => void openBook(id)}
                  />
                </Match>
                <Match when={viewMode() === 'shelf'}>
                  <ShelfView
                    ref={(h) => (view = h)}
                    items={shownItems()}
                    tileSize={coverSize()}
                    selectedId={selectedId()}
                    selectedIds={selectedIds()}
                    emptyMessage={emptyMessage()}
                    emptyMark={!query().trim()}
                    onSelect={click}
                    onContextMenu={contextMenu}
                    onOpen={(id) => void openBook(id)}
                  />
                </Match>
              </Switch>
              <Show when={detailsOpen() && selectedId()}>
                {(id) => <DetailsPanel id={id()} onClose={() => setDetailsOpen(false)} />}
              </Show>
              <Show when={selectedIds().size > 1}>
                <SelectionBar />
              </Show>
            </div>
          </main>
          <StatusBar />
        </Match>
        <Match when={noLibrary()}>{(s) => <EmptyState message={s().message} />}</Match>
      </Switch>
      {/* Keyed: stepping to another book (Edit One by One) gets a fresh form. */}
      <Show when={editingBook()} keyed>
        {(e) => <EditDialog editing={e} />}
      </Show>
      <Show when={bulkIds()}>{(ids) => <BulkEditDialog ids={ids()} />}</Show>
      <DropOverlay />
      <Show when={importState().phase !== 'idle'}>
        <ImportDialog />
      </Show>
      <Show when={settingsOpen()}>
        <SettingsDialog section={settingsSection()} onClose={() => setSettingsOpen(false)} />
      </Show>
      <BookMenuHost />
      <LookupHost />
      <ConfirmHost />
      <QuestionHost />
      <Toast />
      {/* What's being dragged: follows the pointer. */}
      <Show when={drag()}>
        {(d) => (
          <div
            class="pointer-events-none fixed z-50 rounded-chip bg-accent px-2.5 py-1 text-xs font-semibold text-on-accent shadow-lift"
            style={{ left: `${d().x + 14}px`, top: `${d().y + 10}px` }}
          >
            {d().ids.length === 1
              ? (items().find((b) => b.id === d().ids[0])?.title ?? '1 book')
              : `${d().ids.length} books`}
          </div>
        )}
      </Show>
    </>
  );
}
