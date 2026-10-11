// Adding books to the open library: drag and drop, and "Add Books…".

import { batch } from 'solid-js';

import { api, type DropScan, errorMessage, onAddProgress, onImportProgress } from './api';
import { type DropPlan, planDrop } from './dropCheck';
import { loadCustomFields } from './fields';
import { type JobHandle, startJob } from './jobs';
import {
  baseName,
  createLibrary,
  library,
  refresh,
  setDetailsOpen,
  setEditRevision,
  setLibrary,
  setSelectedId,
  status,
} from './library';
import { toast } from './toast';
import { plural } from './writeback';

const READ_ONLY = 'This calibre library is read-only. Import it into a Magnitide library (library menu) to add books.';

let busy = false;

/**
 * Add files and folders to the open library, with progress and a summary. A
 * big or mixed drop is asked about first (see planDrop), and calibre
 * libraries in it can be imported instead. With no library open (the empty
 * screen), one is created for them.
 */
export async function addPaths(paths: string[]): Promise<void> {
  if (paths.length === 0) return;
  if (library()?.kind === 'calibre') {
    toast(READ_ONLY, { error: true });
    return;
  }
  if (busy) {
    toast('Still adding the previous books…');
    return;
  }
  busy = true;
  try {
    let scan: DropScan;
    try {
      scan = await api.scanDrop(paths);
    } catch (err) {
      toast(errorMessage(err), { error: true });
      return;
    }
    const plan = await planDrop(paths, scan);
    if (!plan) return;
    if (!library() && !(status().kind === 'no-library' && (await createLibrary()))) return;

    // Nothing else in the drop once its calibre libraries are imported.
    const left = scan.files - (plan.skipDocuments ? scan.documents : 0);
    const addsFiles = !(plan.importCalibre.length && left === 0);
    // One job for the whole drop, so its summary covers the import and the files together.
    const job = startJob(addsFiles ? 'Add books' : 'Import calibre library', 'Adding books…');
    const outcome: Outcome = { parts: [], details: [], added: 0, already: 0, skipped: 0, failed: 0 };
    for (const path of plan.importCalibre) await importCalibre(path, job, outcome);
    if (addsFiles) await addFiles(paths, plan, job, outcome);
    // Books that were all there already is no problem; a drop with no books in it at all is.
    const problems = outcome.failed > 0 || (outcome.added === 0 && outcome.already === 0 && outcome.skipped > 0);
    const lead = outcome.added || outcome.failed ? [] : [outcome.already ? 'Nothing new to add' : 'No books found'];
    job.finish([...lead, ...outcome.parts].join(' · '), {
      error: problems,
      details: outcome.details,
      duration: problems ? 6000 : 3000,
    });
  } finally {
    busy = false;
  }
}

/** What a drop did, gathered for its one summary. */
type Outcome = {
  parts: string[];
  details: string[];
  /** Books added or imported, or given new formats. */
  added: number;
  /** Books already in the library: duplicates, and books imported from the same calibre library before. */
  already: number;
  /** Files that aren't books, or couldn't be read. */
  skipped: number;
  failed: number;
};

async function addFiles(paths: string[], plan: DropPlan, job: JobHandle, outcome: Outcome): Promise<void> {
  const lib = library();
  if (!lib) return;
  job.progress('Adding books…');
  const unlisten = await onAddProgress((p) => {
    if (p.total > 1 && p.done < p.total) job.progress(`Adding ${p.done + 1} of ${p.total}…`);
  });
  try {
    const r = await api.addBooks(paths, { skipDocuments: plan.skipDocuments, exclude: plan.importCalibre });
    await refresh();
    const first = r.added[0] ?? r.formatsAddedTo[0];
    const touched = r.added.length + r.formatsAddedTo.length;
    batch(() => {
      setLibrary({ ...lib, bookCount: lib.bookCount + r.added.length });
      // Books that gained formats show them.
      if (r.formatsAddedTo.length) setEditRevision((n) => n + 1);
      if (first !== undefined) {
        setSelectedId(first);
        // One book: show it. Several: just select the first.
        if (touched === 1) setDetailsOpen(true);
      }
    });
    const n = r.added.length;
    const joined = r.formatsAddedTo.length;
    if (n) outcome.parts.push(n === 1 ? 'Added 1 book' : `Added ${n} books`);
    if (joined) {
      outcome.parts.push(`Added new formats to ${joined === 1 ? '1 book' : `${joined} books`} already in the library`);
    }
    if (r.duplicates.length) outcome.parts.push(`${r.duplicates.length} already in the library`);
    if (r.skipped.length) outcome.parts.push(`${r.skipped.length} skipped`);
    if (r.failed.length) outcome.parts.push(`${r.failed.length} failed`);
    outcome.details.push(...r.failed, ...r.duplicates, ...r.skipped, ...r.warnings);
    outcome.added += touched;
    outcome.already += r.duplicates.length;
    outcome.skipped += r.skipped.length;
    outcome.failed += r.failed.length;
  } catch (err) {
    outcome.parts.push('Couldn’t add the files');
    outcome.details.push(errorMessage(err));
    outcome.failed += 1;
  } finally {
    unlisten();
  }
}

/** Import a calibre library found in a drop into the open library. */
async function importCalibre(path: string, job: JobHandle, outcome: Outcome): Promise<void> {
  const name = baseName(path);
  job.progress(`Importing “${name}”…`);
  const unlisten = await onImportProgress((p) => {
    if (p.total > 1 && p.done < p.total) job.progress(`Importing “${name}”: ${p.done + 1} of ${p.total}…`);
  });
  try {
    const r = await api.importCalibreIntoLibrary(path);
    const lib = library();
    if (lib) setLibrary({ ...lib, bookCount: lib.bookCount + r.booksImported });
    // Its custom columns are new fields here.
    await loadCustomFields();
    await refresh();
    // "Imported 150 books from “X” · 20 already imported", or just "20 already imported from “X”".
    if (r.booksImported) outcome.parts.push(`Imported ${plural(r.booksImported, 'book', 'books')} from “${name}”`);
    if (r.booksSkipped) {
      outcome.parts.push(
        `${r.booksSkipped.toLocaleString()} already imported${r.booksImported ? '' : ` from “${name}”`}`,
      );
    }
    if (r.failed.length) outcome.parts.push(`${r.failed.length} failed to import from “${name}”`);
    if (!r.booksImported && !r.booksSkipped && !r.failed.length) outcome.parts.push(`“${name}” has no books`);
    outcome.details.push(...r.failed.map((f) => `${name}: ${f}`), ...r.warnings.map((w) => `${name}: ${w}`));
    outcome.added += r.booksImported;
    outcome.already += r.booksSkipped;
    outcome.failed += r.failed.length;
  } catch (err) {
    outcome.parts.push(`Couldn’t import “${name}”`);
    outcome.details.push(`${name}: ${errorMessage(err)}`);
    outcome.failed += 1;
  } finally {
    unlisten();
  }
}

export async function chooseAndAddBooks(): Promise<void> {
  const files = await api.chooseBookFiles();
  if (files.length) await addPaths(files);
}
