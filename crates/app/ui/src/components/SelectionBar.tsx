import { For, onCleanup, onMount, Show } from 'solid-js';
import X from 'lucide-solid/icons/x';

import { booksOn, devices } from '../devices';
import { items, library, selectedId, selectedIds, setSelectedId } from '../library';
import { liftToasts } from '../toast';
import { mergeBooks, removeFromDevice, sendToDevice } from '../transfer';
import { selectedInOrder } from '../devices';
import { editBooks } from '../editor';
import { ConvertMenu } from './ConvertMenu';
import { Icon } from './Icon';

const action =
  'cursor-pointer rounded-md px-2.5 py-1 text-xs font-medium whitespace-nowrap hover:bg-on-inverse/15 disabled:cursor-default disabled:opacity-50';

/** What can be done with several selected books. Mount while more than one is selected. */
export function SelectionBar() {
  onMount(() => liftToasts(true));
  onCleanup(() => liftToasts(false));

  const ids = () => [...selectedIds()];
  const books = () => items().filter((b) => selectedIds().has(b.id));

  return (
    <div
      class="fixed bottom-6 left-1/2 z-15 flex max-w-[calc(100%-32px)] -translate-x-1/2 animate-toast-in items-center gap-1 overflow-x-auto rounded-xl bg-inverse/95 py-1.5 pr-1.5 pl-3.5 text-on-inverse shadow-lift"
      role="toolbar"
      aria-label="Selected books"
    >
      <span class="mr-1.5 text-xs whitespace-nowrap tabular-nums">{selectedIds().size.toLocaleString()} selected</span>
      <Show when={library()?.kind === 'magnitide'}>
        <button class={action} title="Edit these books together (⌘E)" onClick={() => editBooks(selectedInOrder())}>
          Edit…
        </button>
      </Show>
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
        class="ml-1 grid size-6 cursor-pointer place-items-center rounded-md hover:bg-on-inverse/15"
        title="Clear selection (Esc)"
        aria-label="Clear selection"
        onClick={() => setSelectedId(selectedId())}
      >
        <Icon of={X} size={12} />
      </button>
    </div>
  );
}
