// The right-click menu for books: what can be done with the books it was
// opened on (the selection, or the one book right-clicked outside it).

import { createResource, createSignal, For, Show } from 'solid-js';

import { api, errorMessage, fileManagerName, trashName } from '../api';
import { booksOn, devices } from '../devices';
import { editBooks, openEditor } from '../editor';
import { items, library, openBook, setDetailsOpen } from '../library';
import { toast } from '../toast';
import { convertBooks, deleteBooks, mergeBooks, removeFormat, removeFromDevice, sendToDevice } from '../transfer';
import { writeBookFiles } from '../writeback';
import { ContextMenu, MenuItem, MenuSeparator, SubMenu } from './Menu';

type Request = { x: number; y: number; ids: number[] };

const [request, setRequest] = createSignal<Request | null>(null);

/** Open the book menu at a point (viewport pixels) for these books, in list order. */
export function openBookMenu(x: number, y: number, ids: number[]): void {
  if (ids.length) setRequest({ x, y, ids });
}

/** Mount once (in App). */
export function BookMenuHost() {
  return <Show when={request()}>{(r) => <BookMenu request={r()} onClose={() => setRequest(null)} />}</Show>;
}

/** Delete with ⌘⌫ on macOS, Delete elsewhere. */
export const deleteShortcut = /Mac/.test(navigator.userAgent) ? '⌘⌫' : 'Del';

function BookMenu(props: { request: Request; onClose(): void }) {
  const ids = props.request.ids;
  const books = () => {
    const chosen = new Set(ids);
    return items().filter((b) => chosen.has(b.id));
  };
  const one = () => (ids.length === 1 ? books()[0] : undefined);
  const editable = () => library()?.kind === 'magnitide';
  const [targets] = createResource(() =>
    editable() ? api.convertTargets(ids).catch(() => [] as string[]) : Promise.resolve([] as string[]),
  );

  return (
    <ContextMenu
      label={one() ? `Actions for ${one()!.title}` : `Actions for ${ids.length} books`}
      x={props.request.x}
      y={props.request.y}
      onClose={props.onClose}
    >
      <Show when={one()}>
        {(b) => (
          <>
            <MenuItem shortcut="⌘O" disabled={!b().formats.length} onSelect={() => void openBook(b().id)}>
              Open
            </MenuItem>
            <MenuItem onSelect={() => setDetailsOpen(true)}>Show Details</MenuItem>
          </>
        )}
      </Show>
      <Show when={editable()}>
        <MenuItem shortcut="⌘E" onSelect={() => (one() ? void openEditor(ids[0]!) : editBooks(ids))}>
          {one() ? 'Edit…' : `Edit ${ids.length} Books…`}
        </MenuItem>
      </Show>

      <Show when={(one() || editable()) && (devices().length || editable())}>
        <MenuSeparator />
      </Show>
      <Show when={!one() && !editable() && !devices().length}>
        <MenuItem disabled onSelect={() => {}}>
          Nothing to do with these books here
        </MenuItem>
      </Show>
      <For each={devices()}>
        {(d) => {
          const on = () => ids.filter((id) => booksOn(d).has(id)).length;
          return (
            <>
              <Show when={on() < ids.length}>
                <MenuItem onSelect={() => void sendToDevice(d, ids)}>
                  Send to {d.name}
                  {on() && ids.length > 1 ? ` (${ids.length - on()})` : ''}
                </MenuItem>
              </Show>
              <Show when={on()}>
                <MenuItem onSelect={() => void removeFromDevice(d, ids)}>
                  Remove from {d.name}
                  {ids.length > 1 ? ` (${on()})` : ''}
                </MenuItem>
              </Show>
            </>
          );
        }}
      </For>
      <Show when={editable()}>
        <SubMenu label="Convert to" disabled={!targets()?.length}>
          <For each={targets()}>{(f) => <MenuItem onSelect={() => void convertBooks(ids, f)}>{f}</MenuItem>}</For>
        </SubMenu>
        <Show when={one()}>
          {(b) => (
            <>
              <SubMenu label="Remove Format" disabled={!b().formats.length}>
                <For each={b().formats}>
                  {(f) => <MenuItem onSelect={() => void removeFormat(b(), f)}>{f}</MenuItem>}
                </For>
              </SubMenu>
              <MenuItem onSelect={() => void writeBookFiles(b().id)}>Write Metadata to Files</MenuItem>
            </>
          )}
        </Show>
      </Show>

      <Show when={one() || editable()}>
        <MenuSeparator />
      </Show>
      <Show when={one()}>
        {(b) => (
          <MenuItem onSelect={() => api.revealBook(b().id).catch((err) => toast(errorMessage(err), { error: true }))}>
            Show in {fileManagerName}
          </MenuItem>
        )}
      </Show>
      <Show when={editable() && books().length === 2}>
        <MenuItem onSelect={() => void mergeBooks(books()[0]!, books()[1]!)}>Merge…</MenuItem>
      </Show>
      <Show when={editable()}>
        <MenuItem danger shortcut={deleteShortcut} onSelect={() => void deleteBooks(ids)}>
          Move to {trashName}…
        </MenuItem>
      </Show>
    </ContextMenu>
  );
}
