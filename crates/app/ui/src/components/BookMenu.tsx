// The right-click menu for books: what can be done with the books it was
// opened on (the selection, or the one book right-clicked outside it).

import { createResource, createSignal, For, Show } from 'solid-js';

import { api, errorMessage, fileManagerName, trashName } from '../api';
import { booksOn, devices, preferredFormat } from '../devices';
import type { DeviceInfo } from '../api';
import { editBooks, openEditor } from '../editor';
import { findMetadataFor } from '../lookup';
import { items, library, openBook, setDetailsOpen } from '../library';
import { isComicFormat } from '../fields';
import { readComic } from '../viewer';
import { toast } from '../toast';
import {
  convertBooks,
  deleteBooks,
  mergeBooks,
  removeFormat,
  removeFromDevice,
  sendOptions,
  sendToDevice,
} from '../transfer';
import { activeShelf, addToShelf, newShelfFor, removeFromShelf, shelves } from '../shelves';
import { writeBookFiles } from '../writeback';
import { hasPluginMenus } from '../plugins';
import { ContextMenu, MenuItem, MenuSeparator, SubMenu } from './Menu';
import { PluginItems } from './PluginMenus';
import { deleteKeys, keys } from '../keys';

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
            <Show when={b().formats.some(isComicFormat)}>
              <MenuItem onSelect={() => readComic(b().id)}>Read</MenuItem>
            </Show>
            <MenuItem shortcut={keys('mod+O')} disabled={!b().formats.length} onSelect={() => void openBook(b().id)}>
              Open
            </MenuItem>
            <MenuItem onSelect={() => setDetailsOpen(true)}>Show Details</MenuItem>
          </>
        )}
      </Show>
      <Show when={editable()}>
        <MenuItem shortcut={keys('mod+E')} onSelect={() => (one() ? void openEditor(ids[0]!) : editBooks(ids))}>
          {one() ? 'Edit…' : `Edit ${ids.length} Books…`}
        </MenuItem>
        <MenuItem onSelect={() => findMetadataFor(ids)}>
          {one() ? 'Find Metadata…' : `Find Metadata for ${ids.length} Books…`}
        </MenuItem>
        <Show when={hasPluginMenus()}>
          <SubMenu label="Plugins">
            <PluginItems ids={() => ids} />
          </SubMenu>
        </Show>
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
                <SendItem device={d} ids={ids} count={on() && ids.length > 1 ? ids.length - on() : null} />
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
        <SubMenu label="Add to Shelf">
          <For each={shelves()}>{(s) => <MenuItem onSelect={() => void addToShelf(s, ids)}>{s.name}</MenuItem>}</For>
          <Show when={shelves().length}>
            <MenuSeparator />
          </Show>
          <MenuItem onSelect={() => void newShelfFor(ids)}>New Shelf…</MenuItem>
        </SubMenu>
        <Show when={activeShelf()}>
          {(s) => <MenuItem onSelect={() => void removeFromShelf(s(), ids)}>Remove from “{s().name}”</MenuItem>}
        </Show>
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
        <MenuItem danger shortcut={keys(deleteKeys)} onSelect={() => void deleteBooks(ids)}>
          Move to {trashName}…
        </MenuItem>
      </Show>
    </ContextMenu>
  );
}

/**
 * "Send to <device>": a plain item when there's only one format to send, else
 * a submenu of them (the default first) that sends the one picked.
 */
function SendItem(props: { device: DeviceInfo; ids: number[]; count: number | null }) {
  /** How many would be sent (the rest are on the device already). */
  const sending = () => props.count ?? props.ids.length;
  const label = () => `Send to ${props.device.name}${props.count ? ` (${props.count})` : ''}`;
  const options = () => {
    const all = sendOptions(props.device, props.ids);
    const preferred = preferredFormat(props.device.kind);
    // The default first, then best first.
    return [...all.filter((o) => o.format === preferred), ...all.filter((o) => o.format !== preferred)];
  };
  return (
    <Show
      when={options().length > 1}
      fallback={<MenuItem onSelect={() => void sendToDevice(props.device, props.ids)}>{label()}</MenuItem>}
    >
      <SubMenu label={label()}>
        <For each={options()}>
          {(o) => (
            <MenuItem onSelect={() => void sendToDevice(props.device, props.ids, o.format)}>
              {o.format}
              <span class="ml-auto pl-4 text-xs text-faint">
                {[
                  o.format === preferredFormat(props.device.kind) && 'default',
                  o.converted && 'converted',
                  !o.converted && sending() > 1 && o.have < sending() && `${o.have} of ${sending()} have it`,
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </span>
            </MenuItem>
          )}
        </For>
      </SubMenu>
    </Show>
  );
}
