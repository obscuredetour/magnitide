import { createEffect, createSignal, Match, on, onCleanup, onMount, Show, Switch } from 'solid-js';

import { BookMenuHost, openBookMenu } from './components/BookMenu';
import { BookTable } from './components/BookTable';
import { CoverGrid, type ViewHandle } from './components/CoverGrid';
import { DetailsPanel } from './components/DetailsPanel';
import { DropOverlay } from './components/DropOverlay';
import { BulkEditDialog } from './components/BulkEditDialog';
import { EditDialog } from './components/EditDialog';
import { EmptyState } from './components/EmptyState';
import { ImportDialog } from './components/ImportDialog';
import { NetworkWarning } from './components/NetworkWarning';
import { SelectionBar } from './components/SelectionBar';
import { SettingsDialog } from './components/Settings';
import { ConfirmHost } from './confirm';
import { TopBar } from './components/TopBar';
import { selectedInOrder, shownItems, watchDevices } from './devices';
import {
  detailsOpen,
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
import { onOpenSettings } from './api';
import { bulkIds, editBooks, editingBook } from './editor';
import { importState } from './importer';
import { Toast } from './toast';
import { deleteBooks } from './transfer';
import { coverSize, setViewMode, viewMode } from './view';

export function App() {
  let view: ViewHandle | undefined;
  let searchInput: HTMLInputElement | undefined;
  const [settingsOpen, setSettingsOpen] = createSignal(false);

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
      setSettingsOpen(true);
      return;
    }
    if (target.closest('dialog')) return;

    if ((e.key === '/' && !inField) || (e.key === 'f' && cmd)) {
      e.preventDefault();
      searchInput?.focus();
      searchInput?.select();
      return;
    }
    // ⌘1 / ⌘2 switch views, as in Finder.
    if (cmd && (e.key === '1' || e.key === '2')) {
      e.preventDefault();
      setViewMode(e.key === '1' ? 'grid' : 'table');
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
      const cols = view?.columns() ?? 1;
      const horizontal = view?.horizontal ? 1 : 0;
      const step = { ArrowLeft: -horizontal, ArrowRight: horizontal, ArrowUp: -cols, ArrowDown: cols }[e.key];
      if (!step) return;
      e.preventDefault();
      const cur = list.findIndex((it) => it.id === id);
      const next = cur < 0 ? 0 : Math.min(list.length - 1, Math.max(0, cur + step));
      const nextItem = list[next];
      if (nextItem) select(nextItem.id, { open: detailsOpen(), reveal: true });
    }
  };

  onMount(() => {
    document.addEventListener('keydown', onKeyDown);
    // On macOS the menu bar item owns ⌘, (so onKeyDown's ⌘, branch only runs on other platforms).
    const unlisten = onOpenSettings(() => setSettingsOpen(true));
    onCleanup(() => void unlisten.then((off) => off()));
    void openInitialLibrary();
    watchDevices();
  });
  onCleanup(() => document.removeEventListener('keydown', onKeyDown));

  const emptyMessage = () =>
    query().trim() ? `No books match “${query().trim()}”.` : 'This library has no books yet.';

  return (
    <>
      <TopBar
        searchRef={(el) => (searchInput = el)}
        onLeaveSearch={leaveSearch}
        onOpenSettings={() => setSettingsOpen(true)}
      />
      <Switch>
        <Match when={status().kind === 'ready'}>
          <main class="fixed inset-x-0 top-[52px] bottom-0 flex flex-col">
            <NetworkWarning />
            <div class="relative flex min-h-0 flex-1">
              <Show
                when={viewMode() === 'table'}
                fallback={
                  <CoverGrid
                    ref={(h) => (view = h)}
                    items={shownItems()}
                    tileSize={coverSize()}
                    selectedId={selectedId()}
                    selectedIds={selectedIds()}
                    emptyMessage={emptyMessage()}
                    onSelect={click}
                    onContextMenu={contextMenu}
                    onOpen={(id) => void openBook(id)}
                  />
                }
              >
                <BookTable
                  ref={(h) => (view = h)}
                  rows={shownItems()}
                  selectedId={selectedId()}
                  selectedIds={selectedIds()}
                  emptyMessage={emptyMessage()}
                  onSelect={click}
                  onContextMenu={contextMenu}
                  onOpen={(id) => void openBook(id)}
                />
              </Show>
              <Show when={detailsOpen() && selectedId()}>
                {(id) => <DetailsPanel id={id()} onClose={() => setDetailsOpen(false)} />}
              </Show>
              <Show when={selectedIds().size > 1}>
                <SelectionBar />
              </Show>
            </div>
          </main>
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
        <SettingsDialog onClose={() => setSettingsOpen(false)} />
      </Show>
      <BookMenuHost />
      <ConfirmHost />
      <Toast />
    </>
  );
}
