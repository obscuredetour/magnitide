// Connected e-readers and which library books are on them.

import { createEffect, createMemo, createRoot, createSignal, on } from 'solid-js';

import {
  api,
  type BookRow,
  type DeviceHit,
  type DeviceInfo,
  type DeviceKind,
  errorMessage,
  onDevicesChanged,
} from './api';
import { editRevision, items, library } from './library';
import { storage } from './storage';
import { toast } from './toast';

const [devices, setDevices] = createSignal<DeviceInfo[]>([]);
export { devices };

export type OnDevice = { device: DeviceInfo; hit: DeviceHit };

/** Book id → where it is on connected devices. */
export const onDevice = createRoot(() =>
  createMemo(() => {
    const map = new Map<number, OnDevice[]>();
    for (const device of devices()) {
      for (const hit of device.hits) {
        const list = map.get(hit.bookId);
        if (list) list.push({ device, hit });
        else map.set(hit.bookId, [{ device, hit }]);
      }
    }
    return map;
  }),
);

/** The books on a device (or every connected device). */
export const booksOn = (device?: DeviceInfo) =>
  new Set((device ? [device] : devices()).flatMap((d) => d.hits.map((h) => h.bookId)));

/** Show every book, only those on a device, or only those not on one. */
export type DeviceFilter = 'all' | 'on' | 'off';
const [deviceFilter, setDeviceFilter] = createSignal<DeviceFilter>('all');
export { deviceFilter, setDeviceFilter };

/** The current search results, narrowed by the device filter. What the views show. */
export const shownItems = createRoot(() =>
  createMemo((): BookRow[] => {
    const filter = deviceFilter();
    if (filter === 'all' || !devices().length) return items();
    const on = onDevice();
    return items().filter((b) => on.has(b.id) === (filter === 'on'));
  }),
);

/** What each kind of device reads, best first (mirrors `readable` in devices/transfer.rs). */
export const DEVICE_FORMATS: Record<DeviceKind, string[]> = {
  kobo: ['KEPUB', 'EPUB', 'PDF', 'MOBI', 'CBZ', 'CBR', 'TXT', 'HTML', 'RTF'],
  kindle: ['AZW3', 'MOBI', 'AZW', 'PDF', 'TXT'],
  generic: ['EPUB', 'PDF', 'MOBI', 'AZW3', 'FB2', 'CBZ', 'CBR', 'TXT', 'KEPUB'],
};

export const DEVICE_KIND_NAMES: Record<DeviceKind, string> = {
  kobo: 'Kobo',
  kindle: 'Kindle',
  generic: 'Other e-readers',
};

function storedFormats(): Partial<Record<DeviceKind, string>> {
  try {
    const parsed: unknown = JSON.parse(storage.get('deviceFormats') ?? '{}');
    return parsed && typeof parsed === 'object' ? (parsed as Partial<Record<DeviceKind, string>>) : {};
  } catch {
    return {};
  }
}

const [deviceFormats, setDeviceFormats] = createSignal(storedFormats());

/** The format always sent to this kind of device, or null to ask when there's a choice. */
export const preferredFormat = (kind: DeviceKind): string | null => deviceFormats()[kind] ?? null;

export function setPreferredFormat(kind: DeviceKind, format: string | null): void {
  const next = { ...deviceFormats() };
  if (format) next[kind] = format;
  else delete next[kind];
  setDeviceFormats(next);
  storage.set('deviceFormats', JSON.stringify(next));
}

let seq = 0;

export async function refreshDevices(): Promise<void> {
  const mine = ++seq;
  try {
    const list = await api.listDevices();
    if (mine !== seq) return;
    setDevices(list);
    if (!list.length) setDeviceFilter('all');
  } catch (err) {
    console.warn('listing devices:', errorMessage(err));
  }
}

export async function rescanDevice(device: DeviceInfo): Promise<void> {
  try {
    await api.rescanDevice(device.id);
    toast(`Rescanned ${device.name}`);
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}

export async function ejectDevice(device: DeviceInfo): Promise<void> {
  toast(`Ejecting ${device.name}…`, { duration: Infinity });
  try {
    await api.ejectDevice(device.id);
    toast(`${device.name} can be unplugged`);
  } catch (err) {
    toast(`Couldn’t eject ${device.name}: ${errorMessage(err)}`, { error: true });
  }
}

/**
 * Start following devices: when one is plugged in or out, and when the
 * library changes (matches are made against the open library).
 */
export function watchDevices(): void {
  void onDevicesChanged(() => void refreshDevices());
  createRoot(() => {
    createEffect(on([library, editRevision], () => void refreshDevices()));
  });
}
