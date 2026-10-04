// Adding books to the open library: drag and drop, and "Add Books…".

import { batch } from 'solid-js';

import { type AddReport, api, errorMessage, onAddProgress } from './api';
import { library, refresh, setDetailsOpen, setEditRevision, setLibrary, setSelectedId } from './library';
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
    const first = report.added[0] ?? report.formatsAddedTo[0];
    const touched = report.added.length + report.formatsAddedTo.length;
    batch(() => {
      setLibrary({ ...lib, bookCount: lib.bookCount + report.added.length });
      // Books that gained formats show them.
      if (report.formatsAddedTo.length) setEditRevision((n) => n + 1);
      if (first !== undefined) {
        setSelectedId(first);
        // One book: show it. Several: just select the first.
        if (touched === 1) setDetailsOpen(true);
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
  const joined = r.formatsAddedTo.length;
  const parts = n || !joined ? [n === 1 ? 'Added 1 book' : `Added ${n} books`] : [];
  if (joined) parts.push(`Added new formats to ${joined === 1 ? '1 book' : `${joined} books`} already in the library`);
  if (r.duplicates.length) parts.push(`${r.duplicates.length} already in the library`);
  if (r.skipped.length) parts.push(`${r.skipped.length} skipped`);
  if (r.failed.length) parts.push(`${r.failed.length} failed`);
  const problems = r.failed.length > 0 || (n + joined === 0 && r.duplicates.length + r.skipped.length > 0);
  for (const line of [...r.failed, ...r.duplicates, ...r.skipped, ...r.warnings]) console.info(line);
  toast(parts.join(' · '), { error: problems, duration: problems ? 6000 : 3000 });
}
