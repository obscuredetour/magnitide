import { For, Show } from 'solid-js';

import { api, type DeviceInfo, errorMessage, fileManagerName } from '../api';
import { deviceFilter, devices, ejectDevice, rescanDevice, setDeviceFilter } from '../devices';
import { library } from '../library';
import { importFromDevice } from '../transfer';
import { toast } from '../toast';
import { Menu, MenuItem, MenuLabel, MenuSeparator } from './Menu';

function formatBytes(bytes: number): string {
  return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
}

function DeviceIcon() {
  return (
    <svg class="size-[15px] flex-none fill-none stroke-current stroke-[1.5]" viewBox="0 0 16 16" aria-hidden="true">
      <rect x="3" y="1.5" width="10" height="13" rx="1.8" />
      <path d="M6.5 12h3" stroke-linecap="round" />
    </svg>
  );
}

/** The connected e-readers: which books are on them, and a filter to show just those. */
export function DeviceMenu() {
  const label = () => {
    const list = devices();
    return list.length === 1 ? list[0]!.name : `${list.length} devices`;
  };
  const onCount = () => new Set(devices().flatMap((d) => d.hits.map((h) => h.bookId))).size;

  return (
    <Show when={devices().length}>
      <Menu
        label="Devices"
        title={`${label()}: ${onCount().toLocaleString()} books from this library`}
        class="min-w-0 flex-none"
        buttonClass="flex min-w-0 items-center gap-1.5 rounded-[7px] px-2 py-[5px] text-sm hover:bg-line aria-expanded:bg-line"
        panelClass="w-80"
        button={
          <>
            <span classList={{ 'text-success': deviceFilter() === 'all', 'text-accent': deviceFilter() !== 'all' }}>
              <DeviceIcon />
            </span>
            <span class="truncate font-medium max-[1080px]:hidden">{label()}</span>
            <span class="whitespace-nowrap text-muted tabular-nums max-[1080px]:hidden">
              {onCount().toLocaleString()}
            </span>
            <Show when={deviceFilter() !== 'all'}>
              <span class="rounded-full bg-accent/15 px-1.5 text-[11px] font-medium text-accent max-[1080px]:hidden">
                filtered
              </span>
            </Show>
          </>
        }
      >
        <For each={devices()}>{(d) => <DeviceSection device={d} />}</For>
        <MenuSeparator />
        <MenuLabel>Show</MenuLabel>
        <MenuItem checked={deviceFilter() === 'all'} onSelect={() => setDeviceFilter('all')}>
          All books
        </MenuItem>
        <MenuItem checked={deviceFilter() === 'on'} onSelect={() => setDeviceFilter('on')}>
          Books on a device
        </MenuItem>
        <MenuItem checked={deviceFilter() === 'off'} onSelect={() => setDeviceFilter('off')}>
          Books not on a device
        </MenuItem>
      </Menu>
    </Show>
  );
}

function DeviceSection(props: { device: DeviceInfo }) {
  const d = () => props.device;
  const books = () => new Set(d().hits.map((h) => h.bookId)).size;
  const reveal = async () => {
    try {
      await api.revealDevice(d().id);
    } catch (err) {
      toast(errorMessage(err), { error: true });
    }
  };
  return (
    <>
      <div class="px-2.5 pt-1.5 pb-1">
        <div class="font-semibold">{d().name}</div>
        <div class="text-xs text-muted">
          {books().toLocaleString()} {books() === 1 ? 'book' : 'books'} from this library
          <Show when={d().unmatched.length}>
            {' '}
            · {d().unmatched.length.toLocaleString()} other {d().unmatched.length === 1 ? 'file' : 'files'}
          </Show>
          <Show when={d().freeBytes !== null && d().totalBytes !== null}>
            <br />
            {formatBytes(d().freeBytes!)} free of {formatBytes(d().totalBytes!)}
          </Show>
        </div>
      </div>
      <Show when={d().unmatched.length && library()?.kind === 'magnitide'}>
        <MenuItem title={d().unmatched.slice(0, 20).join('\n')} onSelect={() => void importFromDevice(d())}>
          Add {d().unmatched.length.toLocaleString()} Other {d().unmatched.length === 1 ? 'File' : 'Files'} to Library
        </MenuItem>
      </Show>
      <MenuItem onSelect={() => void rescanDevice(d())}>Rescan</MenuItem>
      <MenuItem onSelect={() => void reveal()}>Show in {fileManagerName}</MenuItem>
      <MenuItem onSelect={() => void ejectDevice(d())}>Eject “{d().name}”</MenuItem>
    </>
  );
}
