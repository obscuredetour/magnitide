import { createEffect, createSignal, Match, on, onCleanup, onMount, Show, Switch } from 'solid-js';

import { BookTable } from './components/BookTable';
import { CoverGrid, type ViewHandle } from './components/CoverGrid';
import { DetailsPanel } from './components/DetailsPanel';
import { DropOverlay } from './components/DropOverlay';
import { EditDialog } from './components/EditDialog';
import { EmptyState } from './components/EmptyState';
import { ImportDialog } from './components/ImportDialog';
import { NetworkWarning } from './components/NetworkWarning';
import { SettingsDialog } from './components/Settings';
import { TopBar } from './components/TopBar';
import {
  detailsOpen,
  items,
  openBook,
  openInitialLibrary,
  query,
  selectedId,
  setDetailsOpen,
  setSelectedId,
  status,
} from './library';
import { onOpenSettings } from './api';
import { editingBook, openEditor } from './editor';
import { importState } from './importer';
import { Toast } from './toast';
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
    const list = items();
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
    if (e.key === 'Escape') {
      setDetailsOpen(false);
    } else if (e.key === 'e' && cmd && id !== null) {
      e.preventDefault();
      void openEditor(id);
    } else if (e.key === 'o' && cmd && id !== null) {
      e.preventDefault();
      void openBook(id);
    } else if ((e.key === 'Enter' || e.key === ' ') && id !== null) {
      // Let focused buttons and links handle their own activation.
      if (target.closest('button, a')) return;
      e.preventDefault();
      setDetailsOpen(true);
    } else if (e.key.startsWith('Arrow')) {
      const list = items();
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
                    items={items()}
                    tileSize={coverSize()}
                    selectedId={selectedId()}
                    emptyMessage={emptyMessage()}
                    onSelect={(id) => select(id)}
                    onOpen={(id) => void openBook(id)}
                  />
                }
              >
                <BookTable
                  ref={(h) => (view = h)}
                  rows={items()}
                  selectedId={selectedId()}
                  emptyMessage={emptyMessage()}
                  onSelect={(id) => select(id)}
                  onOpen={(id) => void openBook(id)}
                />
              </Show>
              <Show when={detailsOpen() && selectedId()}>
                {(id) => <DetailsPanel id={id()} onClose={() => setDetailsOpen(false)} />}
              </Show>
            </div>
          </main>
        </Match>
        <Match when={noLibrary()}>{(s) => <EmptyState message={s().message} />}</Match>
      </Switch>
      <Show when={editingBook()}>{(e) => <EditDialog editing={e()} />}</Show>
      <DropOverlay />
      <Show when={importState().phase !== 'idle'}>
        <ImportDialog />
      </Show>
      <Show when={settingsOpen()}>
        <SettingsDialog onClose={() => setSettingsOpen(false)} />
      </Show>
      <Toast />
    </>
  );
}
