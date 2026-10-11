import { createResource, createSignal, For, Show } from 'solid-js';
import ChevronRight from 'lucide-solid/icons/chevron-right';
import HardDrive from 'lucide-solid/icons/hard-drive';

import { api, errorMessage, fileManagerName } from '../../api';
import { startImport } from '../../importer';
import {
  chooseLibrary,
  createLibrary,
  editRevision,
  forgetOpenLibrary,
  forgetRecentLibrary,
  library,
  openLibrary,
  recentLibraries,
  refresh,
  renameLibrary,
  setEditRevision,
} from '../../library';
import { formatSize } from '../../sizes';
import { toast } from '../../toast';
import { describeWrite, plural, setWriteOnSave, writeOnSave } from '../../writeback';
import { Icon } from '../Icon';
import { Mark } from '../Mark';
import { CustomFieldsSettings } from './CustomFields';
import { button, Callout, Group, Heading, help, Row, Toggle } from './parts';

const run = async (action: () => Promise<void>, done?: string) => {
  try {
    await action();
    if (done) toast(done);
  } catch (err) {
    toast(errorMessage(err), { error: true });
  }
};

/**
 * The open library: its name (editable in a Magnitide library) and where it
 * is, then the others recently open, folded away until asked for.
 */
function CurrentLibrary() {
  const others = () => recentLibraries().filter((r) => r.path !== library()?.path);
  const editable = () => library()?.kind === 'magnitide';
  const [showRecent, setShowRecent] = createSignal(false);
  return (
    <Show when={library()}>
      {(lib) => (
        <section class="mb-7 rounded-card border border-line bg-bg/40">
          <div class="flex items-start gap-3.5 p-4">
            <span class="grid size-11 flex-none place-items-center rounded-card border border-line bg-popover text-fg shadow-card">
              <Mark size={28} colour />
            </span>
            <div class="min-w-0 flex-1">
              <Show when={editable()} fallback={<div class="text-base font-semibold">{lib().name}</div>}>
                <LibraryName />
              </Show>
              <div class={`mt-1 break-all ${help}`}>{lib().path}</div>
              <div class={help}>
                {lib().bookCount.toLocaleString()} books ·{' '}
                {lib().kind === 'magnitide' ? 'Magnitide library' : 'calibre library, opened read-only'}
              </div>
            </div>
            <div class="flex flex-none gap-2">
              <button class={button} onClick={() => void run(api.showLibraryFolder)}>
                Show in {fileManagerName}
              </button>
              <button
                class={button}
                title="Close this library and remove it from the list. Its folder and books aren't touched."
                onClick={() => void forgetOpenLibrary()}
              >
                Forget
              </button>
            </div>
          </div>

          <div class="flex items-center gap-2 border-t border-line px-4 py-2">
            <button
              class="group -ml-1 flex items-center gap-2 rounded-control px-1 py-0.5 font-medium hover:bg-hover"
              aria-expanded={showRecent()}
              onClick={() => setShowRecent(!showRecent())}
            >
              <Icon of={ChevronRight} size={12} class="text-faint transition-transform group-aria-expanded:rotate-90" />
              Recent libraries
              <Show when={others().length}>
                <span class="text-xs font-normal text-faint tabular-nums">{others().length}</span>
              </Show>
            </button>
            <button class={`${button} ml-auto`} onClick={() => void chooseLibrary()}>
              Open Another…
            </button>
            <button class={button} onClick={() => void createLibrary()}>
              New…
            </button>
          </div>
          <Show when={showRecent()}>
            <div class="border-t border-line px-4">
              <Show
                when={others().length}
                fallback={
                  <p class={`py-3 ${help}`}>
                    Libraries you open show up here: Magnitide libraries, and calibre libraries (read-only).
                  </p>
                }
              >
                <For each={others()}>
                  {(r) => (
                    <Row label={r.name} description={<span class="break-all">{r.path}</span>}>
                      <button class={button} onClick={() => void openLibrary(r.path)}>
                        Open
                      </button>
                      <button class={button} title="Remove from this list" onClick={() => forgetRecentLibrary(r.path)}>
                        Forget
                      </button>
                    </Row>
                  )}
                </For>
              </Show>
            </div>
          </Show>
        </section>
      )}
    </Show>
  );
}

/** The open Magnitide library's name, edited where it's shown; Enter or Save renames it. */
function LibraryName() {
  const [draft, setDraft] = createSignal(library()?.name ?? '');
  const changed = () => draft().trim() !== '' && draft().trim() !== library()?.name;
  const save = async () => {
    if (changed() && (await renameLibrary(draft()))) toast('Library renamed');
  };
  return (
    <div class="flex items-center gap-2">
      <input
        type="text"
        class="-mx-1.5 h-8 min-w-0 flex-1 rounded-control border border-transparent bg-transparent px-1.5 text-base font-semibold outline-none select-text hover:border-line focus:border-focus focus:bg-field focus:ring-3 focus:ring-focus/20"
        aria-label="Library name"
        title="The name in the library menu and recent libraries. The folder keeps its name."
        value={draft()}
        onInput={(e) => setDraft(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') void save();
          else if (e.key === 'Escape' && changed()) {
            e.stopPropagation();
            setDraft(library()?.name ?? '');
          }
        }}
      />
      <Show when={changed()}>
        <button class={button} onClick={() => void save()}>
          Save
        </button>
      </Show>
    </div>
  );
}

/** What the library holds on disk, and the thumbnail cache beside it. */
function Storage() {
  const [cache, { refetch }] = createResource(() => api.thumbnailCacheInfo());
  // Follows opening, adding and deleting (library) and edits (editRevision), as the sidebar's do.
  const [stats] = createResource(
    () => [library(), editRevision()] as const,
    ([lib]) => (lib ? api.libraryStats().catch(() => null) : null),
  );
  return (
    <Callout icon={HardDrive} title="Storage" class="mb-7">
      <Show when={stats()}>
        {(s) => (
          <p>
            <span class="text-fg">Books: {formatSize(s().totalBytes)}</span>
            <Show when={s().formats.length}>
              {' · '}
              {s()
                .formats.map((f) => `${f.format} ${f.count.toLocaleString()}`)
                .join(' · ')}
            </Show>
          </p>
        )}
      </Show>
      <div class="flex items-center justify-between gap-4">
        <p>
          <span class="text-fg">Cover thumbnails: </span>
          <Show when={cache()} fallback="measuring…">
            {/* JSX (not a template string) so it updates when the cache is cleared. */}
            {(c) => (
              <>
                {c().files.toLocaleString()} cached, {formatSize(c().bytes)}. Rebuilt as covers are shown.
              </>
            )}
          </Show>
        </p>
        <button
          class={button}
          disabled={!cache()?.files}
          onClick={() =>
            void run(async () => {
              await api.clearThumbnailCache();
              await refetch();
            }, 'Thumbnail cache cleared')
          }
        >
          Clear
        </button>
      </div>
    </Callout>
  );
}

/** "Tidy file names": preview, run, and list files missing from disk. */
function TidyNames() {
  const [preview, { refetch }] = createResource(() => api.tidyPreview().catch(() => null));
  const [running, setRunning] = createSignal(false);
  const [missing, setMissing] = createSignal<string[]>([]);

  const tidy = async () => {
    setRunning(true);
    try {
      const r = await api.tidyFileNames();
      await refresh();
      setEditRevision((n) => n + 1);
      setMissing(r.missing);
      const n = (count: number, one: string, many: string) => `${count.toLocaleString()} ${count === 1 ? one : many}`;
      const parts = [
        r.booksMoved || r.filesRenamed
          ? `Renamed ${n(r.filesRenamed, 'file', 'files')}, moved ${n(r.booksMoved, 'folder', 'folders')}`
          : 'Nothing to rename',
      ];
      if (r.missing.length) parts.push(`${n(r.missing.length, 'file', 'files')} missing from disk`);
      if (r.failed.length) parts.push(`${n(r.failed.length, 'book', 'books')} couldn't be renamed`);
      for (const f of r.failed) console.warn(f);
      toast(parts.join(' · '), { error: r.failed.length > 0, duration: 5000 });
      await refetch();
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setRunning(false);
    }
  };

  const description = () => {
    const p = preview();
    if (p === undefined) return 'Checking…';
    if (p === null) return 'Couldn’t check the library.';
    return p.books
      ? `${p.books.toLocaleString()} ${p.books === 1 ? 'book has' : 'books have'} folder or file names that don’t match their title and author (${p.files.toLocaleString()} files).`
      : 'Every folder and file name matches its book’s title and author. Checking also looks for missing files.';
  };

  return (
    <>
      <Row label="Tidy file names" description={description()}>
        <button class={button} disabled={running()} onClick={() => void tidy()}>
          {running() ? 'Tidying…' : preview()?.books ? 'Tidy' : 'Check'}
        </button>
      </Row>
      <Show when={missing().length}>
        <div class="mb-3 rounded-control bg-bg p-3 text-xs">
          <div class="mb-1 font-medium text-danger">Missing from disk</div>
          <ul class="max-h-32 space-y-0.5 overflow-y-auto text-muted select-text">
            <For each={missing()}>{(m) => <li class="break-all">{m}</li>}</For>
          </ul>
        </div>
      </Show>
    </>
  );
}

/**
 * EPUB files carry their own copy of the metadata. Two ways to keep it in
 * step: as each edit is saved (a setting), or every file at once (an action).
 */
/** Comics: CBRs repacked as CBZ when added, and what reads RAR here. */
function Comics() {
  const [on, { mutate }] = createResource(() => api.convertCbrOnAdd().catch(() => false));
  const [tool] = createResource(() => api.rarTool().catch(() => null));
  const set = async (value: boolean) => {
    mutate(value);
    try {
      await api.setConvertCbrOnAdd(value);
    } catch (err) {
      mutate(!value);
      toast(errorMessage(err), { error: true });
    }
  };
  return (
    <Group title="Comics">
      <Row
        label="Convert CBR to CBZ when adding"
        description="CBR comics are repacked as CBZ on the way in: the same pages, nothing recompressed, and the library keeps the CBZ. Metadata can be written into a CBZ; a RAR can't be written. The files you add are never changed."
      >
        <Toggle label="Convert CBR to CBZ when adding" checked={on() ?? false} onChange={(v) => void set(v)} />
      </Row>
      <p class={`pb-3 ${help}`}>
        <Show
          when={tool()}
          fallback={
            tool.loading
              ? 'Looking for a program that reads RAR…'
              : 'Most CBRs are RAR archives, which need bsdtar (libarchive), 7-Zip or unrar installed. None was found, so only CBRs that are really ZIPs can be read or converted.'
          }
        >
          {(name) => `RAR archives (most CBRs) are read with ${name()}.`}
        </Show>
      </p>
    </Group>
  );
}

function EpubMetadata() {
  const [preview, { refetch }] = createResource(() => api.writebackPreview().catch(() => null));
  const [confirming, setConfirming] = createSignal(false);
  const [running, setRunning] = createSignal(false);

  const write = async () => {
    setConfirming(false);
    setRunning(true);
    try {
      const r = await api.writeAllMetadata();
      await refresh();
      setEditRevision((n) => n + 1);
      for (const f of r.failed) console.warn(f);
      toast(describeWrite(r), { error: r.failed.length > 0, duration: 5000 });
      await refetch();
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setRunning(false);
    }
  };

  const status = () => {
    const p = preview();
    if (p === undefined) return 'Checking…';
    if (p === null) return 'Couldn’t check the library.';
    return p.files
      ? `${plural(p.books, 'book has', 'books have')} EPUB files that differ from the library (${plural(p.files, 'file', 'files')}).`
      : 'Every EPUB matches the library.';
  };

  return (
    <Group
      title="Metadata in EPUB files"
      description="An EPUB carries its own copy of the title, authors, cover and the rest, which e-readers and other apps read. Magnitide can keep that copy matching the library."
    >
      <Row
        label="Update a book’s EPUB when saving edits"
        description="Off by default, since every save rewrites the file."
      >
        <Toggle label="Update a book’s EPUB when saving edits" checked={writeOnSave()} onChange={setWriteOnSave} />
      </Row>
      <Row label="Update every EPUB now" description={status()}>
        <Show
          when={confirming()}
          fallback={
            <button class={button} disabled={running() || !preview()?.files} onClick={() => setConfirming(true)}>
              {running() ? 'Updating…' : 'Update…'}
            </button>
          }
        >
          <button class={button} onClick={() => setConfirming(false)}>
            Cancel
          </button>
          <button class={`${button} border-accent text-accent`} onClick={() => void write()}>
            Update {plural(preview()?.files ?? 0, 'file', 'files')}
          </button>
        </Show>
      </Row>
      <Show when={confirming()}>
        <p class={`mb-3 rounded-control bg-bg p-3 ${help}`}>
          Files are rewritten in place. Apps that track reading progress by a file’s fingerprint (KOReader sync, for
          one) may treat a rewritten book as new.
        </p>
      </Show>
    </Group>
  );
}

export function LibrarySettings() {
  const magnitide = () => library()?.kind === 'magnitide';
  return (
    <>
      <Heading title="Library" />
      <CurrentLibrary />
      <Show when={library()}>
        <Storage />
      </Show>
      <Show when={library()?.kind === 'calibre'}>
        <Group>
          <Row
            label="Import into a Magnitide library"
            description="Copies the books into a new library you can edit; this calibre library stays exactly as it is. Files are cloned where the disk supports it, so they take no extra space."
          >
            <button class={button} onClick={() => void startImport()}>
              Import…
            </button>
          </Row>
        </Group>
      </Show>
      <Show when={magnitide()}>
        <EpubMetadata />
        <Comics />
        <Group title="Files">
          <TidyNames />
        </Group>
      </Show>
      <Show when={library()}>
        <CustomFieldsSettings editable={magnitide()} />
      </Show>
      <Show when={!library()}>
        <Group>
          <Row label="Open a library" description="A Magnitide library, or a calibre library (opened read-only).">
            <button class={button} onClick={() => void chooseLibrary()}>
              Choose…
            </button>
          </Row>
          <Row label="Create a library" description="A new, empty Magnitide library to add your books to.">
            <button class={button} onClick={() => void createLibrary()}>
              Create…
            </button>
          </Row>
        </Group>
      </Show>
    </>
  );
}
