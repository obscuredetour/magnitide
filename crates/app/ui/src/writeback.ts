// Writing library metadata into book files (Magnitide libraries only).

import { createSignal } from 'solid-js';

import { api, errorMessage, type WriteReport } from './api';
import { refresh, setEditRevision } from './library';
import { storage } from './storage';
import { toast } from './toast';

/** "Also write to the book's EPUB and CBZ files when saving"; off unless turned on. */
const [writeOnSave, setWriteOnSaveSignal] = createSignal(storage.get('writeFilesOnSave') === 'on');
export { writeOnSave };

export function setWriteOnSave(on: boolean): void {
  setWriteOnSaveSignal(on);
  storage.set('writeFilesOnSave', on ? 'on' : 'off');
}

export const plural = (count: number, one: string, many: string) =>
  `${count.toLocaleString()} ${count === 1 ? one : many}`;

/** A one-line summary of a write. */
export function describeWrite(r: WriteReport): string {
  const parts = [];
  if (r.written) parts.push(`Wrote metadata into ${plural(r.written, 'file', 'files')}`);
  else if (r.upToDate) parts.push('Files already match the library');
  else if (r.skipped) parts.push('No EPUB or CBZ to write to; other formats aren’t supported yet');
  else parts.push('Nothing to write');
  if (r.failed.length) parts.push(`${plural(r.failed.length, 'file', 'files')} couldn’t be written`);
  return parts.join(' · ');
}

/** Write one book's metadata into its files, reporting the result in a toast. */
export async function writeBookFiles(id: number): Promise<void> {
  try {
    const r = await api.writeMetadataToFiles(id);
    await refresh();
    setEditRevision((n) => n + 1);
    for (const f of r.failed) console.warn(f);
    // One book: say why it failed.
    toast(r.failed.length ? r.failed.join('; ') : describeWrite(r), { error: r.failed.length > 0, duration: 5000 });
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
}
