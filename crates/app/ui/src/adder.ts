// Adding books to the open library: drag and drop, and "Add Books…".

import { batch } from 'solid-js';

import { type AddReport, api, errorMessage, onAddProgress } from './api';
import { library, refresh, setDetailsOpen, setLibrary, setSelectedId } from './library';
import { toast } from './toast';

const READ_ONLY = 'This calibre library is read-only. Import it into a Magnitide library (library menu) to add books.';

let busy = false;

/** Add files and folders to the open library, with progress and a summary. */
export async function addPaths(paths: string[]): Promise<void> {
  const lib = library();
  if (!lib || paths.length === 0) return;
  if (lib.kind !== 'magnitide') {
    toast(READ_ONLY, { error: true });
    return;
  }
  if (busy) {
    toast('Still adding the previous books…');
    return;
  }
  busy = true;
  toast('Adding books…', { duration: Infinity });
  const unlisten = await onAddProgress((p) => {
    if (p.total > 1 && p.done < p.total) toast(`Adding ${p.done + 1} of ${p.total}…`, { duration: Infinity });
  });
  try {
    const report = await api.addBooks(paths);
    await refresh();
    const first = report.added[0];
    batch(() => {
      setLibrary({ ...lib, bookCount: lib.bookCount + report.added.length });
      if (first !== undefined) {
        setSelectedId(first);
        // One new book: show it. Several: just select the first.
        if (report.added.length === 1) setDetailsOpen(true);
      }
    });
    summarize(report);
  } catch (err) {
    toast(errorMessage(err), { error: true });
  } finally {
    unlisten();
    busy = false;
  }
}

export async function chooseAndAddBooks(): Promise<void> {
  const files = await api.chooseBookFiles();
  if (files.length) await addPaths(files);
}

function summarize(r: AddReport): void {
  const n = r.added.length;
  const parts = [n === 1 ? 'Added 1 book' : `Added ${n} books`];
  if (r.duplicates.length) parts.push(`${r.duplicates.length} already in the library`);
  if (r.skipped.length) parts.push(`${r.skipped.length} skipped`);
  if (r.failed.length) parts.push(`${r.failed.length} failed`);
  const problems = r.failed.length > 0 || (n === 0 && r.duplicates.length + r.skipped.length > 0);
  for (const line of [...r.failed, ...r.duplicates, ...r.skipped, ...r.warnings]) console.info(line);
  toast(parts.join(' · '), { error: problems, duration: problems ? 6000 : 3000 });
}
