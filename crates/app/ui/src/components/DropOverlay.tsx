import { createSignal, onCleanup, onMount, Show } from 'solid-js';

import { onDragDrop } from '../api';
import { addPaths } from '../adder';
import { library } from '../library';

/** Covers the window while files are dragged over it; adds them on drop. */
export function DropOverlay() {
  const [count, setCount] = createSignal<number | null>(null);

  onMount(() => {
    const unlisten = onDragDrop((e) => {
      if (e.type === 'enter') setCount(e.paths.length);
      else if (e.type === 'leave') setCount(null);
      else if (e.type === 'drop') {
        setCount(null);
        void addPaths(e.paths);
      }
    });
    onCleanup(() => void unlisten.then((off) => off()));
  });

  const readOnly = () => library()?.kind !== 'magnitide';

  return (
    <Show when={count() !== null && library()}>
      <div class="pointer-events-none fixed inset-0 z-40 grid place-items-center bg-bg/70 p-8 backdrop-blur-sm">
        <div
          class="flex w-[min(420px,100%)] flex-col items-center gap-2 rounded-2xl border-2 border-dashed px-8 py-10 text-center shadow-lift"
          classList={{ 'border-accent bg-surface': !readOnly(), 'border-line bg-surface-2': readOnly() }}
        >
          <svg
            class="mb-1 size-10 fill-none stroke-[1.5]"
            classList={{ 'stroke-accent': !readOnly(), 'stroke-muted': readOnly() }}
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <path
              d="M12 15V4m0 0-4 4m4-4 4 4M5 14v4a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
          <Show
            when={!readOnly()}
            fallback={
              <>
                <p class="font-serif text-lg font-semibold">This calibre library is read-only</p>
                <p class="text-sm text-muted">Import it into a Magnitide library (library menu) to add books.</p>
              </>
            }
          >
            <p class="font-serif text-lg font-semibold">Drop to add to “{library()?.name}”</p>
            <p class="text-sm text-muted">
              {count() === 1 ? '1 item' : `${count()} items`}. Folders are searched for books; files are cloned or
              copied, never moved.
            </p>
          </Show>
        </div>
      </div>
    </Show>
  );
}
