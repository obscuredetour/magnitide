import { createEffect, Match, on, onCleanup, onMount, Show, Switch } from 'solid-js';

import { CoverGrid, type GridHandle } from './components/CoverGrid';
import { DetailsPanel } from './components/DetailsPanel';
import { EmptyState } from './components/EmptyState';
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
import { Toast } from './toast';
import { coverSize } from './view';

export function App() {
  let grid: GridHandle | undefined;
  let searchInput: HTMLInputElement | undefined;

  const noLibrary = () => {
    const s = status();
    return s.kind === 'no-library' ? s : null;
  };

  const select = (id: number, { open = true, reveal = false } = {}) => {
    setSelectedId(id);
    if (open) setDetailsOpen(true);
    if (reveal) grid?.reveal(id);
  };

  // Opening or closing the panel changes the grid's width; keep the selection in view.
  createEffect(
    on(
      detailsOpen,
      () => {
        const id = selectedId();
        if (id !== null) requestAnimationFrame(() => grid?.reveal(id));
      },
      { defer: true },
    ),
  );

  const leaveSearch = () => {
    grid?.focus();
    const list = items();
    const first = list[0];
    if (first && !list.some((it) => it.id === selectedId())) select(first.id, { open: false, reveal: true });
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const target = e.target as HTMLElement;
    const inField = target instanceof HTMLInputElement || target instanceof HTMLSelectElement;

    if ((e.key === '/' && !inField) || (e.key === 'f' && (e.metaKey || e.ctrlKey))) {
      e.preventDefault();
      searchInput?.focus();
      searchInput?.select();
      return;
    }
    if (inField) return;

    const id = selectedId();
    if (e.key === 'Escape') {
      setDetailsOpen(false);
    } else if (e.key === 'o' && (e.metaKey || e.ctrlKey) && id !== null) {
      e.preventDefault();
      void openBook(id);
    } else if ((e.key === 'Enter' || e.key === ' ') && id !== null) {
      e.preventDefault();
      setDetailsOpen(true);
    } else if (e.key.startsWith('Arrow')) {
      const list = items();
      if (!list.length) return;
      const cols = grid?.columns() ?? 1;
      const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -cols, ArrowDown: cols }[e.key];
      if (step === undefined) return;
      e.preventDefault();
      const cur = list.findIndex((it) => it.id === id);
      const next = cur < 0 ? 0 : Math.min(list.length - 1, Math.max(0, cur + step));
      const nextItem = list[next];
      if (nextItem) select(nextItem.id, { open: detailsOpen(), reveal: true });
    }
  };

  onMount(() => {
    document.addEventListener('keydown', onKeyDown);
    void openInitialLibrary();
  });
  onCleanup(() => document.removeEventListener('keydown', onKeyDown));

  return (
    <>
      <TopBar searchRef={(el) => (searchInput = el)} onLeaveSearch={leaveSearch} />
      <Switch>
        <Match when={status().kind === 'ready'}>
          <main class="fixed inset-x-0 top-[52px] bottom-0 flex">
            <CoverGrid
              ref={(h) => (grid = h)}
              items={items()}
              tileSize={coverSize()}
              selectedId={selectedId()}
              emptyMessage={query().trim() ? `No books match “${query().trim()}”.` : 'This library has no books yet.'}
              onSelect={(id) => select(id)}
              onOpen={(id) => void openBook(id)}
            />
            <Show when={detailsOpen() && selectedId()}>
              {(id) => <DetailsPanel id={id()} onClose={() => setDetailsOpen(false)} />}
            </Show>
          </main>
        </Match>
        <Match when={noLibrary()}>{(s) => <EmptyState message={s().message} />}</Match>
      </Switch>
      <Toast />
    </>
  );
}
