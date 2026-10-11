// Asking first about a big or mixed drop: a parent folder (Documents,
// Downloads) full of files that aren't really books, or a calibre library,
// which is better imported, with what calibre knows, than added file by file.

import { createSignal, Show } from 'solid-js';

import type { DropScan } from './api';
import { confirm } from './confirm';
import { baseName } from './library';
import { plural } from './writeback';

/** A dropped folder with more book files than this is asked about first. */
const ASK_ABOVE = 50;

export type DropPlan = {
  /** Leave out TXT, HTML, RTF, DOCX and ODT files. */
  skipDocuments: boolean;
  /** calibre libraries to import, left out of the files added. */
  importCalibre: string[];
};

/**
 * How to add a drop: as it is when it's no surprise (files, or a folder of a
 * few books), else as the user says. Null when they cancel.
 */
export async function planDrop(paths: string[], scan: DropScan): Promise<DropPlan | null> {
  const calibre = scan.calibreLibraries;
  if (!scan.hasFolder || (scan.files <= ASK_ABOVE && scan.documents === 0 && calibre.length === 0)) {
    return { skipDocuments: false, importCalibre: [] };
  }

  // Ticked when documents are a good share of the drop: then it's a folder of papers, not of books.
  const [skipDocuments, setSkipDocuments] = createSignal(scan.documents * 4 >= scan.files);
  const [importCalibre, setImportCalibre] = createSignal(true);
  const one = calibre.length === 1;
  const names = calibre.map((p) => `“${baseName(p)}”`).join(', ');
  const calibreNoun = one ? 'a calibre library' : `${calibre.length} calibre libraries`;
  const from = paths.length === 1 ? `“${baseName(paths[0]!)}”` : plural(paths.length, 'item', 'items');
  const title =
    scan.files === 0
      ? `Add ${one ? `the calibre library ${names}` : `${calibre.length} calibre libraries`}?`
      : `Add ${plural(scan.files, 'file', 'files')}${calibre.length ? ` and ${calibreNoun}` : ''} from ${from}?`;
  const filesLeft = () => scan.files - (skipDocuments() ? scan.documents : 0);

  /** What pressing the button does, in a sentence: both parts of a drop at once are easy to miss. */
  const outcome = () => {
    const parts: string[] = [];
    if (scan.files) {
      parts.push(
        filesLeft()
          ? `${plural(filesLeft(), 'file', 'files')} ${filesLeft() === 1 ? 'is' : 'are'} added`
          : 'the documents are skipped',
      );
    }
    parts.push(
      importCalibre()
        ? `${names} ${one ? 'is' : 'are'} imported with ${one ? 'its' : 'their'} tags, series and ratings`
        : `the book files in ${names} are added one by one`,
    );
    const sentence = parts.join(', and ');
    return `${sentence[0]!.toUpperCase()}${sentence.slice(1)}.`;
  };

  const ok = await confirm({
    title,
    message: () => (
      <div class="space-y-3">
        <Show when={scan.files}>
          <div class="space-y-2">
            <p>
              <span class="font-medium text-fg">{plural(scan.files, 'book file', 'book files')}:</span>{' '}
              {formatLine(scan)}
            </p>
            <Show when={scan.documents}>
              <label class="flex cursor-pointer items-start gap-2 text-fg">
                <input
                  type="checkbox"
                  class="mt-0.5 accent-accent"
                  checked={skipDocuments()}
                  onChange={(e) => setSkipDocuments(e.currentTarget.checked)}
                />
                <span>
                  Skip the {plural(scan.documents, 'document', 'documents')} (TXT, HTML, RTF, DOCX, ODT)
                  <span class="block text-xs text-muted">Files like READMEs, letters and saved web pages.</span>
                </span>
              </label>
            </Show>
          </div>
        </Show>
        <Show when={calibre.length}>
          <div class="space-y-1.5" role="radiogroup" aria-label="calibre libraries">
            <p>
              <span class="font-medium text-fg">
                {one ? 'A calibre library' : `${calibre.length} calibre libraries`}:
              </span>{' '}
              {names}
            </p>
            <label class="flex cursor-pointer items-start gap-2 text-fg">
              <input
                type="radio"
                name="drop-calibre"
                class="mt-0.5 accent-accent"
                checked={importCalibre()}
                onChange={() => setImportCalibre(true)}
              />
              <span>
                Import {one ? 'it' : 'them'}
                <span class="block text-xs text-muted">
                  With the tags, series, ratings and custom columns calibre keeps. calibre’s copy isn’t changed.
                </span>
              </span>
            </label>
            <label class="flex cursor-pointer items-start gap-2 text-fg">
              <input
                type="radio"
                name="drop-calibre"
                class="mt-0.5 accent-accent"
                checked={!importCalibre()}
                onChange={() => setImportCalibre(false)}
              />
              <span>
                Add {one ? 'its' : 'their'} book files one by one
                <span class="block text-xs text-muted">Only what’s inside each file: no tags, series or ratings.</span>
              </span>
            </label>
          </div>
          <p class="border-t border-line pt-3 text-fg" aria-live="polite">
            {outcome()}
          </p>
        </Show>
      </div>
    ),
    confirm: () => (calibre.length && importCalibre() ? (filesLeft() > 0 ? 'Add and Import' : 'Import') : 'Add Books'),
  });
  if (!ok) return null;
  return { skipDocuments: skipDocuments(), importCalibre: importCalibre() ? calibre : [] };
}

/** "1,200 EPUB · 900 PDF · 300 TXT · and 4 more formats". */
function formatLine(scan: DropScan): string {
  const shown = scan.formats.slice(0, 5).map(([format, n]) => `${n.toLocaleString()} ${format}`);
  const more = scan.formats.length - shown.length;
  return [...shown, ...(more ? [`and ${plural(more, 'more format', 'more formats')}`] : [])].join(' · ');
}
