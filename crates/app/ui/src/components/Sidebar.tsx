import { createSignal, For, Show } from 'solid-js';
import LibraryBig from 'lucide-solid/icons/library-big';
import Plus from 'lucide-solid/icons/plus';
import SquareLibrary from 'lucide-solid/icons/square-library';

import type { Shelf } from '../api';
import { drag } from '../bookDrag';
import { library, query } from '../library';
import { activeShelf, createShelf, deleteShelf, renameShelf, shelves, showShelf } from '../shelves';
import { Icon } from './Icon';
import { ContextMenu, MenuItem } from './Menu';

const row =
  'group flex h-7 w-full cursor-default items-center gap-2 rounded-control px-2 text-left text-sm outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-focus aria-[current=true]:bg-highlight aria-[current=true]:text-accent';
const label = 'px-2 pt-4 pb-1 font-serif text-[13px] tracking-[.04em] text-muted small-caps';

/** A name field for creating or renaming a shelf: Enter saves, Esc or leaving cancels. */
function NameField(props: { value?: string; onDone(name: string | null): void }) {
  return (
    <input
      class="h-7 w-full rounded-control border border-focus bg-field px-2 text-sm outline-none select-text"
      value={props.value ?? ''}
      placeholder="Shelf name"
      aria-label="Shelf name"
      ref={(el) => queueMicrotask(() => (el.focus(), el.select()))}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Enter') props.onDone(e.currentTarget.value);
        else if (e.key === 'Escape') props.onDone(null);
      }}
      onBlur={(e) => props.onDone(e.currentTarget.value || null)}
    />
  );
}

/** The library's shelves, beside the books. Drop books on a shelf to add them. */
export function Sidebar() {
  const [creating, setCreating] = createSignal(false);
  const [renaming, setRenaming] = createSignal<number | null>(null);
  const [menu, setMenu] = createSignal<{ x: number; y: number; shelf: Shelf } | null>(null);
  const editable = () => library()?.kind === 'magnitide';
  const over = (target: number | 'new') => drag()?.over === target;

  return (
    <nav class="flex w-52 flex-none flex-col overflow-y-auto border-r border-line bg-panel p-2" aria-label="Library">
      <button class={row} aria-current={!query().trim()} onClick={() => showShelf(null)}>
        <Icon of={LibraryBig} class="text-muted" />
        <span class="min-w-0 flex-1 truncate">All Books</span>
        <span class="text-xs text-faint tabular-nums">{library()?.bookCount.toLocaleString()}</span>
      </button>

      <Show
        when={editable()}
        fallback={<p class="px-2 pt-4 text-xs text-faint">Shelves are for Magnitide libraries.</p>}
      >
        <div class="flex items-center">
          <div class={`${label} flex-1`}>Shelves</div>
          <button
            class="mt-3 grid size-6 cursor-pointer place-items-center rounded-control text-muted hover:bg-hover hover:text-fg"
            title="New shelf"
            aria-label="New shelf"
            onClick={() => setCreating(true)}
          >
            <Icon of={Plus} />
          </button>
        </div>

        <For each={shelves()}>
          {(shelf) => (
            <Show
              when={renaming() !== shelf.id}
              fallback={
                <NameField
                  value={shelf.name}
                  onDone={(name) => {
                    setRenaming(null);
                    if (name) void renameShelf(shelf, name);
                  }}
                />
              }
            >
              <button
                class={row}
                classList={{ 'bg-highlight! text-accent ring-2 ring-accent': over(shelf.id) }}
                aria-current={activeShelf()?.id === shelf.id}
                data-shelf-drop={shelf.id}
                title={`${shelf.name} · double-click to rename`}
                onClick={() => showShelf(shelf)}
                onDblClick={() => setRenaming(shelf.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, shelf });
                }}
              >
                <Icon of={SquareLibrary} class="text-muted group-aria-[current=true]:text-accent" />
                <span class="min-w-0 flex-1 truncate">{shelf.name}</span>
                <span class="text-xs text-faint tabular-nums">{shelf.count.toLocaleString()}</span>
              </button>
            </Show>
          )}
        </For>

        <Show when={creating()}>
          <NameField
            onDone={(name) => {
              setCreating(false);
              if (name) void createShelf(name);
            }}
          />
        </Show>

        {/* While dragging books: drop here to make a shelf for them. */}
        <Show when={drag()}>
          <div
            class="mt-1 flex h-7 items-center gap-2 rounded-control border border-dashed border-line px-2 text-sm text-muted"
            classList={{ 'border-accent bg-highlight text-accent': over('new') }}
            data-shelf-drop="new"
          >
            <Icon of={Plus} />
            New shelf with {drag()!.ids.length === 1 ? 'this book' : `${drag()!.ids.length} books`}
          </div>
        </Show>

        <Show when={!shelves().length && !creating() && !drag()}>
          <p class="px-2 pt-1 text-xs text-faint">Make a shelf with +, then drag books onto it.</p>
        </Show>
      </Show>

      <Show when={menu()}>
        {(m) => (
          <ContextMenu label={`Shelf ${m().shelf.name}`} x={m().x} y={m().y} onClose={() => setMenu(null)}>
            <MenuItem onSelect={() => showShelf(m().shelf)}>Show Books</MenuItem>
            <MenuItem onSelect={() => setRenaming(m().shelf.id)}>Rename…</MenuItem>
            <MenuItem danger onSelect={() => void deleteShelf(m().shelf)}>
              Delete Shelf…
            </MenuItem>
          </ContextMenu>
        )}
      </Show>
    </nav>
  );
}
