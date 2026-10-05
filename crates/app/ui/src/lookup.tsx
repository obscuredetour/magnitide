// Find Metadata for several books: the Find Metadata dialog, one book after
// another ("2 / 4"), saving each book's chosen fields as it goes. The next
// book's search runs while the current one is reviewed.

import { createResource, createSignal, Show } from 'solid-js';

import { api, type BookEdit, coverChanged, errorMessage, type SearchResult } from './api';
import { type Applied, type Current, MatchDialog } from './components/MatchDialog';
import { items, library, refresh, setEditRevision } from './library';
import { recordJob } from './jobs';
import { toast } from './toast';
import { plural, writeOnSave } from './writeback';

type Run = { queue: number[]; index: number };

const [run, setRun] = createSignal<Run | null>(null);
let updated = 0;
/** What happened to each book, for the job history. */
let outcomes: string[] = [];
/** A dialog has been shown in this run (later books open without the animation). */
let shown = false;

/** A book loaded for review: its fields, and its search (started on load). */
type Loaded = { id: number; edit: BookEdit; current: Current; result: Promise<SearchResult> };
const loads = new Map<number, Promise<Loaded>>();

function load(id: number): Promise<Loaded> {
  let p = loads.get(id);
  if (!p) {
    p = (async () => {
      const edit = await api.bookEdit(id);
      const current: Current = {
        title: edit.title,
        authors: edit.authors,
        publisher: edit.publisher,
        published: edit.published,
        languages: edit.languages,
        tags: edit.tags,
        identifiers: edit.identifiers,
        descriptionHtml: edit.descriptionHtml,
        hasCover: items().find((b) => b.id === id)?.hasCover ?? false,
        isbn: edit.identifiers.isbn ?? null,
      };
      const result = api.findMetadata({ title: edit.title, authors: edit.authors.slice(0, 1), isbn: current.isbn });
      result.catch(() => {}); // reported when the dialog awaits it
      return { id, edit, current, result };
    })();
    p.catch(() => loads.delete(id));
    loads.set(id, p);
  }
  return p;
}

/** Look up these books one after another (Magnitide libraries only). */
export function findMetadataFor(ids: number[]): void {
  if (library()?.kind !== 'magnitide') {
    toast('This calibre library is read-only. Import it into a Magnitide library to change its books.', {
      error: true,
    });
    return;
  }
  if (!ids.length) return;
  loads.clear();
  updated = 0;
  outcomes = [];
  shown = false;
  setRun({ queue: ids, index: 0 });
}

function finish(): void {
  const r = run();
  setRun(null);
  loads.clear();
  if (!r || !outcomes.length) return;
  const summary =
    r.queue.length === 1
      ? updated
        ? 'Saved'
        : 'Nothing changed'
      : `Updated ${updated} of ${plural(r.queue.length, 'book', 'books')}`;
  recordJob('Find Metadata', summary, { details: outcomes });
}

function go(index: number): void {
  const r = run();
  if (!r) return;
  if (index >= r.queue.length) finish();
  else setRun({ ...r, index: Math.max(0, index) });
}

/** Put the chosen fields into the book's edit and save it (and the cover). */
async function save(book: Loaded, a: Applied): Promise<void> {
  const edit: BookEdit = { ...book.edit };
  if (a.title !== undefined) {
    edit.title = a.title;
    edit.titleSort = ''; // derived again from the new title
  }
  if (a.authors) edit.authors = a.authors;
  if (a.publisher !== undefined) edit.publisher = a.publisher;
  if (a.published !== undefined) edit.published = a.published;
  if (a.languages) edit.languages = a.languages;
  if (a.tags) edit.tags = a.tags;
  if (a.identifiers) edit.identifiers = a.identifiers;
  if (a.descriptionHtml !== undefined) edit.descriptionHtml = a.descriptionHtml;
  const fieldsChanged = Object.keys(a).some((k) => k !== 'coverUrl');
  if (fieldsChanged) {
    await api.saveBookEdit(book.id, edit);
    if (writeOnSave()) await api.writeMetadataToFiles(book.id).catch(() => null);
  }
  if (a.coverUrl) {
    await api.setCoverFromUrl(book.id, a.coverUrl);
    coverChanged(book.id);
  }
}

/** Mount once (in App): the dialog for the book being looked up. */
export function LookupHost() {
  const id = () => {
    const r = run();
    return r ? r.queue[r.index] : undefined;
  };
  const [book] = createResource(id, async (bookId) => {
    const loaded = await load(bookId).catch((err) => {
      toast(errorMessage(err), { error: true });
      return null;
    });
    // Start the next book's search while this one is reviewed.
    const r = run();
    const next = r?.queue[r.index + 1];
    if (next !== undefined) void load(next).catch(() => {});
    return loaded;
  });
  // The loaded book, if it belongs to this run (not one left from the last).
  const current = () => {
    const b = book.latest;
    return b && run()?.queue.includes(b.id) ? b : undefined;
  };

  return (
    // Keyed on the loaded book, so each book gets a fresh dialog; the
    // previous one stays up while the next loads.
    <Show when={current()} keyed>
      {(b) => {
        const r = run()!;
        const quiet = shown;
        shown = true;
        return (
          <MatchDialog
            id={b.id}
            current={b.current}
            quiet={quiet}
            onApply={() => {}}
            onClose={finish}
            step={{
              position: r.index,
              total: r.queue.length,
              prefetched: b.result,
              onPrevious: r.index > 0 ? () => go(r.index - 1) : undefined,
              onSkip: () => {
                outcomes.push(`Skipped “${b.edit.title}”`);
                go(r.index + 1);
              },
              onUse: async (applied) => {
                try {
                  await save(b, applied);
                  updated++;
                  outcomes.push(`Updated “${applied.title ?? b.edit.title}”`);
                  // Going back to this book shows what was saved.
                  loads.delete(b.id);
                  await refresh();
                  setEditRevision((n) => n + 1);
                  go(r.index + 1);
                  return null;
                } catch (err) {
                  return errorMessage(err);
                }
              },
            }}
          />
        );
      }}
    </Show>
  );
}
