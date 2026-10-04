import { For, onCleanup, onMount, Show } from 'solid-js';

import { booksOn, devices } from '../devices';
import { items, library, selectedId, selectedIds, setSelectedId } from '../library';
import { liftToasts } from '../toast';
import { mergeBooks, removeFromDevice, sendToDevice } from '../transfer';
import { ConvertMenu } from './ConvertMenu';

const action =
  'cursor-pointer rounded-md px-2.5 py-1 text-xs font-medium whitespace-nowrap hover:bg-white/15 disabled:cursor-default disabled:opacity-50';

/** What can be done with several selected books. Mount while more than one is selected. */
export function SelectionBar() {
  onMount(() => liftToasts(true));
  onCleanup(() => liftToasts(false));

  const ids = () => [...selectedIds()];
  const books = () => items().filter((b) => selectedIds().has(b.id));

  return (
    <div
      class="fixed bottom-6 left-1/2 z-15 flex max-w-[calc(100%-32px)] -translate-x-1/2 animate-toast-in items-center gap-1 overflow-x-auto rounded-xl bg-[rgb(30_30_33/0.94)] py-1.5 pr-1.5 pl-3.5 text-white shadow-lift"
      role="toolbar"
      aria-label="Selected books"
    >
      <span class="mr-1.5 text-xs whitespace-nowrap tabular-nums">{selectedIds().size.toLocaleString()} selected</span>
      <For each={devices()}>
        {(d) => {
          const on = () => ids().filter((id) => booksOn(d).has(id)).length;
          return (
            <>
              <button class={action} disabled={on() === ids().length} onClick={() => void sendToDevice(d, ids())}>
                Send to {d.name}
              </button>
              <Show when={on()}>
                <button class={action} onClick={() => void removeFromDevice(d, ids())}>
                  Remove from {d.name} ({on()})
                </button>
              </Show>
            </>
          );
        }}
      </For>
      <Show when={library()?.kind === 'magnitide'}>
        <ConvertMenu ids={ids()} buttonClass={action} above />
      </Show>
      <Show when={library()?.kind === 'magnitide' && books().length === 2}>
        <button class={action} onClick={() => void mergeBooks(books()[0]!, books()[1]!)}>
          Merge…
        </button>
      </Show>
      <button
        class="ml-1 grid size-6 cursor-pointer place-items-center rounded-md hover:bg-white/15"
        title="Clear selection (Esc)"
        aria-label="Clear selection"
        onClick={() => setSelectedId(selectedId())}
      >
        <svg class="size-2.5 stroke-current stroke-2 [stroke-linecap:round]" viewBox="0 0 20 20" aria-hidden="true">
          <path d="m5 5 10 10M15 5 5 15" />
        </svg>
      </button>
    </div>
  );
}
