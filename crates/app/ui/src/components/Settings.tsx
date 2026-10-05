import { getVersion } from '@tauri-apps/api/app';
import { createResource, createSignal, For, type JSX, Match, onMount, type ParentProps, Show, Switch } from 'solid-js';
import X from 'lucide-solid/icons/x';

import { api, type DeviceKind, errorMessage, fileManagerName, type SourceId, type SourceStatus } from '../api';
import {
  chooseLibrary,
  forgetRecentLibrary,
  library,
  openLibrary,
  recentLibraries,
  refresh,
  renameLibrary,
  setEditRevision,
} from '../library';
import { startImport } from '../importer';
import { addonReport, addThemeFile, loadThemeAddons, setTheme, themeAttrs, themeChoice, themes } from '../theme';
import { AccentPicker } from './AccentPicker';
import { toast } from '../toast';
import {
  columnOrder,
  isColumnVisible,
  resetColumnOrder,
  resetColumnSizing,
  setColumnVisible,
  setViewMode,
  setZebraRows,
  viewMode,
  zebraRows,
} from '../view';
import { DEVICE_FORMATS, DEVICE_KIND_NAMES, preferredFormat, setPreferredFormat } from '../devices';
import { describeWrite, plural, setWriteOnSave, writeOnSave } from '../writeback';
import { TABLE_COLUMNS } from './BookTable';
import { CoverSizeSlider } from './CoverSizeSlider';
import { resetZoom, tryZoom, ZOOM_STEPS, zoom, zoomLabel } from '../zoom';
import { Icon } from './Icon';
import { smallButton } from '../buttons';
import { backdropDismiss } from '../backdrop';
import { keys, SHORTCUTS } from '../keys';

type Section = 'appearance' | 'library' | 'list' | 'devices' | 'sources' | 'shortcuts' | 'about';

const SECTIONS: { id: Section; label: string }[] = [
  { id: 'appearance', label: 'Appearance' },
  { id: 'library', label: 'Library' },
  { id: 'list', label: 'List View' },
  { id: 'devices', label: 'Devices' },
  { id: 'sources', label: 'Metadata Sources' },
  { id: 'shortcuts', label: 'Keyboard Shortcuts' },
  { id: 'about', label: 'About' },
];

const button = smallButton;

/** The settings window (⌘,). Mount it to open; `onClose` unmounts it. */
export function SettingsDialog(props: { onClose(): void }) {
  let dialog!: HTMLDialogElement;
  const [section, setSection] = createSignal<Section>('appearance');

  onMount(() => dialog.showModal());

  return (
    <dialog
      ref={dialog}
      aria-label="Settings"
      class="m-auto h-[min(560px,calc(100vh-80px))] w-[min(760px,calc(100vw-48px))] animate-menu-in overflow-hidden rounded-dialog border border-line bg-popover p-0 text-fg shadow-lift"
      onCancel={(e) => {
        // Esc: let Solid unmount the dialog instead of the browser closing it.
        e.preventDefault();
        props.onClose();
      }}
      // Settings apply as they're made, so clicking outside always closes it.
      {...backdropDismiss(() => dialog, { close: () => props.onClose() })}
    >
      <div class="flex h-full">
        <nav
          class="flex w-48 flex-none flex-col gap-0.5 border-r border-line bg-bg/50 p-3"
          aria-label="Settings sections"
        >
          <h2 class="px-2.5 pt-1 pb-3 font-serif text-lg font-semibold">Settings</h2>
          <For each={SECTIONS}>
            {(s) => (
              <button
                class="rounded-control px-2.5 py-1.5 text-left hover:bg-hover aria-[current=page]:bg-highlight aria-[current=page]:text-accent"
                aria-current={section() === s.id ? 'page' : undefined}
                onClick={() => setSection(s.id)}
              >
                {s.label}
              </button>
            )}
          </For>
        </nav>

        <div class="min-w-0 flex-1 overflow-y-auto px-7 py-6 select-text">
          <button
            class="float-right -mt-1 -mr-2 grid size-7 place-items-center rounded-full hover:bg-hover"
            title="Close (Esc)"
            aria-label="Close settings"
            onClick={() => props.onClose()}
          >
            <Icon of={X} class="text-muted" />
          </button>
          <Switch>
            <Match when={section() === 'appearance'}>
              <Appearance />
            </Match>
            <Match when={section() === 'library'}>
              <LibrarySettings />
            </Match>
            <Match when={section() === 'list'}>
              <ListSettings />
            </Match>
            <Match when={section() === 'devices'}>
              <DeviceSettings />
            </Match>
            <Match when={section() === 'sources'}>
              <SourceSettings />
            </Match>
            <Match when={section() === 'shortcuts'}>
              <ShortcutSettings />
            </Match>
            <Match when={section() === 'about'}>
              <About />
            </Match>
          </Switch>
        </div>
      </div>
    </dialog>
  );
}

function Heading(props: ParentProps) {
  return <h3 class="mb-4 text-base font-semibold">{props.children}</h3>;
}

/** A labelled setting: name and help text on the left, control on the right. */
function Row(props: ParentProps<{ label: string; description?: JSX.Element }>) {
  return (
    <div class="flex items-center justify-between gap-6 border-b border-line py-3.5 last:border-b-0">
      <div class="min-w-0">
        <div class="font-medium">{props.label}</div>
        <Show when={props.description}>
          <div class="mt-0.5 text-xs text-muted">{props.description}</div>
        </Show>
      </div>
      <div class="flex flex-none items-center gap-2">{props.children}</div>
    </div>
  );
}

/** Miniature of a theme: page, a card, an accent. Scoped via data-theme. */
function ThemePreview(props: { theme: string }) {
  return (
    <div {...themeAttrs(props.theme)} class="flex h-full w-full flex-col gap-1.5 bg-bg p-2">
      <div class="h-1.5 w-1/2 rounded-full bg-fg/70" />
      <div class="flex flex-1 gap-1.5">
        <div class="flex-1 rounded bg-panel shadow-card" />
        <div class="flex-1 rounded bg-element" />
      </div>
      <div class="h-1.5 w-1/3 rounded-full bg-accent" />
    </div>
  );
}

function ThemeCard(props: ParentProps<{ selected: boolean; label: string; onSelect(): void }>) {
  return (
    <button
      class="group flex cursor-pointer flex-col gap-1.5 text-left"
      role="radio"
      aria-checked={props.selected}
      onClick={() => props.onSelect()}
    >
      <div class="h-[68px] w-full overflow-hidden rounded-card border border-line group-aria-checked:outline-2 group-aria-checked:outline-offset-2 group-aria-checked:outline-accent">
        {props.children}
      </div>
      <span class="text-xs group-aria-checked:font-semibold">{props.label}</span>
    </button>
  );
}

/** Theme files in the themes folder: add one, open the folder, reload, and what couldn't be used. */
function ThemeAddonsSection() {
  const reload = () => loadThemeAddons().catch((err) => toast(errorMessage(err), { error: true }));
  onMount(() => void reload());
  return (
    <div class="mb-6">
      <div class="mb-1 font-medium">Theme add-ons</div>
      <p class="text-xs text-muted">
        Theme files in the themes folder show up with the themes above. A Catppuccin-style <code>palette.json</code>{' '}
        works too, one theme per flavour. The format is in <code>docs/themes.md</code>.
      </p>
      <Show when={addonReport()?.problems.length}>
        <ul class="mt-2 space-y-1 text-xs text-danger select-text">
          <For each={addonReport()!.problems}>
            {(p) => (
              <li>
                <span class="font-medium">{p.file}</span>: {p.message}
              </li>
            )}
          </For>
        </ul>
      </Show>
      <div class="mt-2.5 flex flex-wrap gap-2">
        <button
          class={button}
          onClick={async () => {
            const error = await addThemeFile();
            if (error) toast(error, { error: true });
          }}
        >
          Add Theme File…
        </button>
        <button
          class={button}
          onClick={() => api.openThemesFolder().catch((err) => toast(errorMessage(err), { error: true }))}
        >
          Open Themes Folder
        </button>
        <button class={button} title="After editing theme files" onClick={() => void reload()}>
          Reload
        </button>
      </div>
    </div>
  );
}

/**
 * Interface size: a slider over the zoom steps (by index, so each notch is one
 * step). Dragging only previews the number; letting go tries the size, which
 * then asks to be kept (see tryZoom), so the window doesn't rescale under the
 * pointer mid-drag.
 */
function InterfaceSize() {
  const [dragging, setDragging] = createSignal<number | null>(null);
  const shown = () => dragging() ?? zoom();
  const index = () => ZOOM_STEPS.indexOf(zoom() as (typeof ZOOM_STEPS)[number]);
  return (
    <Row
      label="Interface size"
      description={`Everything in the window, larger or smaller. Also ${keys('mod++')}, ${keys('mod+-')} and ${keys('mod+0')}.`}
    >
      <input
        type="range"
        class="w-32 accent-accent"
        aria-label="Interface size"
        aria-valuetext={zoomLabel(shown())}
        min={0}
        max={ZOOM_STEPS.length - 1}
        step={1}
        list="zoom-steps"
        value={index()}
        onInput={(e) => setDragging(ZOOM_STEPS[Number(e.currentTarget.value)] ?? null)}
        onChange={(e) => {
          const z = ZOOM_STEPS[Number(e.currentTarget.value)] ?? 1;
          setDragging(null);
          const input = e.currentTarget;
          // Back to the size in use if it isn't kept.
          void tryZoom(z).then(() => (input.value = String(index())));
        }}
      />
      <datalist id="zoom-steps">
        <For each={ZOOM_STEPS}>{(_, i) => <option value={i()} />}</For>
      </datalist>
      <span class="w-10 text-right text-xs text-muted tabular-nums">{zoomLabel(shown())}</span>
      <button class={button} disabled={zoom() === 1} onClick={resetZoom}>
        Reset
      </button>
    </Row>
  );
}

function Appearance() {
  return (
    <>
      <Heading>Appearance</Heading>
      <div class="mb-2 font-medium">Theme</div>
      <div class="mb-6 grid grid-cols-[repeat(auto-fill,minmax(112px,1fr))] gap-4" role="radiogroup" aria-label="Theme">
        <ThemeCard selected={themeChoice() === 'system'} label="Match System" onSelect={() => setTheme('system')}>
          <div class="flex h-full">
            <div class="w-1/2 overflow-hidden">
              <ThemePreview theme="light" />
            </div>
            <div class="w-1/2 overflow-hidden">
              <ThemePreview theme="dark" />
            </div>
          </div>
        </ThemeCard>
        <For each={themes()}>
          {(t) => (
            <ThemeCard selected={themeChoice() === t.id} label={t.label} onSelect={() => setTheme(t.id)}>
              <ThemePreview theme={t.id} />
            </ThemeCard>
          )}
        </For>
      </div>

      <div class="mb-2 font-medium">Accent</div>
      <div class="mb-6">
        <AccentPicker />
        <p class="mt-2 text-xs text-muted">Remembered for each theme.</p>
      </div>

      <ThemeAddonsSection />

      <Row label="View" description={`Also ${keys('mod+1')} and ${keys('mod+2')}.`}>
        <div class="flex rounded-control bg-element p-0.5" role="group" aria-label="View">
          <For
            each={
              [
                ['grid', 'Covers'],
                ['table', 'List'],
              ] as const
            }
          >
            {([mode, label]) => (
              <button
                class="rounded-inset px-3 py-1 text-xs font-medium text-muted aria-pressed:bg-popover aria-pressed:text-fg aria-pressed:shadow-card"
                aria-pressed={viewMode() === mode}
                onClick={() => setViewMode(mode)}
              >
                {label}
              </button>
            )}
          </For>
        </div>
      </Row>
      <Row label="Cover size" description="How large covers appear in the cover view.">
        <CoverSizeSlider />
      </Row>
      <InterfaceSize />
    </>
  );
}

function formatBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  if (n >= 1e3) return `${Math.round(n / 1e3)} KB`;
  return `${n} B`;
}

function LibrarySettings() {
  const [cache, { refetch }] = createResource(() => api.thumbnailCacheInfo());
  const others = () => recentLibraries().filter((r) => r.path !== library()?.path);

  const run = async (action: () => Promise<void>, done?: string) => {
    try {
      await action();
      if (done) toast(done);
    } catch (err) {
      toast(errorMessage(err), { error: true });
    }
  };

  return (
    <>
      <Heading>Library</Heading>
      <Show when={library()}>
        {(lib) => (
          <Row
            label={lib().name}
            description={
              <>
                <span class="break-all">{lib().path}</span>
                <br />
                {lib().bookCount.toLocaleString()} books ·{' '}
                {lib().kind === 'magnitide' ? 'Magnitide library' : 'calibre library, opened read-only'}
              </>
            }
          >
            <button class={button} onClick={() => void run(api.showLibraryFolder)}>
              Show in {fileManagerName}
            </button>
          </Row>
        )}
      </Show>
      <Show when={library()?.kind === 'magnitide'}>
        <LibraryName />
        <TidyNames />
        <WriteBack />
      </Show>
      <Show when={library()?.kind === 'calibre'}>
        <Row
          label="Import into a Magnitide library"
          description="Copies books into a new library you can edit. Files are cloned where the disk supports it, using no extra space. This calibre library stays exactly as it is."
        >
          <button class={button} onClick={() => void startImport()}>
            Import…
          </button>
        </Row>
      </Show>
      <Row label="Open another library" description="A Magnitide library, or a calibre library (opened read-only).">
        <button class={button} onClick={() => void chooseLibrary()}>
          Choose…
        </button>
      </Row>

      <Show when={others().length}>
        <div class="mt-6 mb-1 font-medium">Recent libraries</div>
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

      <div class="mt-6 mb-1 font-medium">Storage</div>
      <Row
        label="Cover thumbnails"
        description={
          <Show when={cache()} fallback="Measuring…">
            {/* JSX (not a template string) so it updates when the cache is cleared. */}
            {(c) => (
              <>
                {c().files.toLocaleString()} cached · {formatBytes(c().bytes)}. Rebuilt automatically as covers are
                shown.
              </>
            )}
          </Show>
        }
      >
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
      </Row>
    </>
  );
}

function ListSettings() {
  // In display order: saved ones first, the rest in their default places.
  const ordered = () => {
    const order = columnOrder();
    const known = order.filter((id) => TABLE_COLUMNS.some((c) => c.id === id));
    return [
      ...known.map((id) => TABLE_COLUMNS.find((c) => c.id === id)!),
      ...TABLE_COLUMNS.filter((c) => !known.includes(c.id)),
    ];
  };
  return (
    <>
      <Heading>List View</Heading>
      <div class="mb-1 font-medium">Columns</div>
      <div class="mb-4 text-xs text-muted">Also available from the ⋯ button at the end of the list header.</div>
      <div class="mb-6 grid grid-cols-2 gap-x-6 gap-y-2">
        <For each={ordered()}>
          {(c) => (
            <label class="flex items-center gap-2" classList={{ 'opacity-60': c.locked }}>
              <input
                type="checkbox"
                class="size-4 accent-accent"
                checked={isColumnVisible(c.id)}
                disabled={c.locked}
                onChange={(e) => setColumnVisible(c.id, e.currentTarget.checked)}
              />
              {c.label}
            </label>
          )}
        </For>
      </div>
      <Row label="Alternating row colours" description="Shade every other row to make long lists easier to follow.">
        <input
          type="checkbox"
          class="size-4 accent-accent"
          aria-label="Alternating row colours"
          checked={zebraRows()}
          onChange={(e) => setZebraRows(e.currentTarget.checked)}
        />
      </Row>
      <Row label="Column widths" description="Drag a column's edge to resize it; double-click to reset one.">
        <button class={button} onClick={() => resetColumnSizing()}>
          Reset All
        </button>
      </Row>
      <Row
        label="Column order"
        description={`Drag a column's header sideways to move it, or focus it and press ${keys('alt+ArrowLeft')} / ${keys('alt+ArrowRight')}.`}
      >
        <button class={button} disabled={!columnOrder().length} onClick={() => resetColumnOrder()}>
          Reset
        </button>
      </Row>
    </>
  );
}

function About() {
  const [version] = createResource(() => getVersion().catch(() => null));
  return (
    <div class="flex flex-col items-center pt-6 text-center">
      <img src="/icon.png" alt="" width="96" height="96" />
      <h3 class="mt-4 font-serif text-2xl font-semibold">Magnitide</h3>
      <Show when={version()}>{(v) => <div class="mt-1 text-xs text-muted">Version {v()}</div>}</Show>
      <p class="mt-4 max-w-sm text-sm text-muted">
        An AI-free ebook library manager. Opens calibre libraries read-only, and imports them into Magnitide libraries
        without changing them.
      </p>
      <p class="mt-4 text-xs text-faint">Licensed under the GNU GPL v3 or later.</p>
    </div>
  );
}

/** Rename the open Magnitide library. */
function LibraryName() {
  const [draft, setDraft] = createSignal(library()?.name ?? '');
  const changed = () => draft().trim() !== '' && draft().trim() !== library()?.name;
  const save = async () => {
    if (changed() && (await renameLibrary(draft()))) toast('Library renamed');
  };
  return (
    <Row label="Name" description="Shown in the library menu and recent libraries. The folder keeps its name.">
      <input
        type="text"
        class="h-7 w-48 rounded-control border border-line bg-field px-2 outline-none select-text focus:border-focus focus:ring-3 focus:ring-focus/20"
        aria-label="Library name"
        value={draft()}
        onInput={(e) => setDraft(e.currentTarget.value)}
        onKeyDown={(e) => e.key === 'Enter' && void save()}
      />
      <button class={button} disabled={!changed()} onClick={() => void save()}>
        Save
      </button>
    </Row>
  );
}

/** "Tidy file names": preview, run, and list files missing from disk. */
function TidyNames() {
  const [preview, { refetch }] = createResource(() => api.tidyPreview().catch(() => null));
  const [running, setRunning] = createSignal(false);
  const [missing, setMissing] = createSignal<string[]>([]);

  const run = async () => {
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
      ? `${p.books.toLocaleString()} ${p.books === 1 ? 'book has' : 'books have'} folder or file names that don’t match their metadata (${p.files.toLocaleString()} files).`
      : 'Every folder and file name matches its book’s title and author. Tidying also checks for missing files.';
  };

  return (
    <>
      <Row label="Tidy file names" description={description()}>
        <button class={button} disabled={running()} onClick={() => void run()}>
          {running() ? 'Tidying…' : preview()?.books ? 'Tidy' : 'Check'}
        </button>
      </Row>
      <Show when={missing().length}>
        <div class="mb-2 rounded-card bg-bg p-3 text-xs">
          <div class="mb-1 font-medium text-danger">Missing from disk</div>
          <ul class="max-h-32 space-y-0.5 overflow-y-auto text-muted select-text">
            <For each={missing()}>{(m) => <li class="break-all">{m}</li>}</For>
          </ul>
        </div>
      </Show>
    </>
  );
}

function WriteBack() {
  const [preview, { refetch }] = createResource(() => api.writebackPreview().catch(() => null));
  const [confirming, setConfirming] = createSignal(false);
  const [running, setRunning] = createSignal(false);

  const run = async () => {
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

  const description = () => {
    const p = preview();
    if (p === undefined) return 'Checking…';
    if (p === null) return 'Couldn’t check the library.';
    return p.files
      ? `${plural(p.books, 'book has', 'books have')} EPUB files whose built-in metadata or cover differs from the library (${plural(p.files, 'file', 'files')}). Writing updates them for e-readers and other apps.`
      : 'Every EPUB’s built-in metadata and cover match the library.';
  };

  return (
    <>
      <Row label="Write metadata to EPUB files" description={description()}>
        <Show
          when={confirming()}
          fallback={
            <button class={button} disabled={running() || !preview()?.files} onClick={() => setConfirming(true)}>
              {running() ? 'Writing…' : 'Write…'}
            </button>
          }
        >
          <button class={button} onClick={() => setConfirming(false)}>
            Cancel
          </button>
          <button class={button + ' border-accent text-accent'} onClick={() => void run()}>
            Write {plural(preview()?.files ?? 0, 'file', 'files')}
          </button>
        </Show>
      </Row>
      <Show when={confirming()}>
        <p class="mb-2 rounded-card bg-bg p-3 text-xs text-muted">
          Files are rewritten in place. Apps that track reading progress by a file’s fingerprint (KOReader sync, for
          one) may treat a rewritten book as new.
        </p>
      </Show>
      <Row
        label="Also write when saving edits"
        description="Keep EPUB files in step with every edit. Off by default, since each save rewrites the file."
      >
        <input
          type="checkbox"
          class="size-4 accent-accent"
          aria-label="Also write when saving edits"
          checked={writeOnSave()}
          onChange={(e) => setWriteOnSave(e.currentTarget.checked)}
        />
      </Row>
    </>
  );
}

const SOURCE_NOTES: Record<SourceId, string> = {
  openLibrary: 'Free and open, run by the Internet Archive. No account needed.',
  google:
    'Needs your own free API key: in the Google Cloud console, enable the Books API, then create an API key under Credentials.',
};

function SourceSettings() {
  const [sources, { mutate }] = createResource(() => api.metadataSources().catch(() => []));
  const [keyDraft, setKeyDraft] = createSignal('');

  const update = async (s: SourceStatus, enabled: boolean, apiKey: string | null = null) => {
    try {
      mutate(await api.setMetadataSource(s.id, enabled, apiKey));
      if (apiKey !== null) {
        setKeyDraft('');
        toast(apiKey ? 'API key saved' : 'API key removed');
      }
    } catch (err) {
      toast(errorMessage(err), { error: true });
    }
  };

  return (
    <>
      <Heading>Metadata Sources</Heading>
      <p class="mb-4 text-xs text-muted">
        “Find Metadata…” in the edit window asks these sources. Nothing is sent until you search, and then only what you
        searched for (title, author, ISBN).
      </p>
      <For each={sources()}>
        {(s) => (
          <>
            <Row
              label={s.name}
              description={
                <>
                  {SOURCE_NOTES[s.id]}
                  <Show when={s.needsKey && s.enabled && !s.hasKey}>
                    <span class="mt-1 block text-danger">Not used until a key is saved.</span>
                  </Show>
                </>
              }
            >
              <input
                type="checkbox"
                class="size-4 accent-accent"
                aria-label={`Use ${s.name}`}
                checked={s.enabled}
                onChange={(e) => void update(s, e.currentTarget.checked)}
              />
            </Row>
            <Show when={s.needsKey}>
              <div class="-mt-1.5 mb-2 flex items-center gap-2 border-b border-line pb-3.5">
                <input
                  type="password"
                  autocomplete="off"
                  spellcheck={false}
                  aria-label={`${s.name} API key`}
                  placeholder={s.hasKey ? 'Key saved · enter a new one to replace it' : 'API key'}
                  class="h-7 min-w-0 flex-1 rounded-control border border-line bg-field px-2 text-xs outline-none focus:border-focus"
                  value={keyDraft()}
                  onInput={(e) => setKeyDraft(e.currentTarget.value)}
                  onKeyDown={(e) => e.key === 'Enter' && keyDraft().trim() && void update(s, s.enabled, keyDraft())}
                />
                <button
                  class={button}
                  disabled={!keyDraft().trim()}
                  onClick={() => void update(s, s.enabled, keyDraft())}
                >
                  Save
                </button>
                <Show when={s.hasKey}>
                  <button class={button} onClick={() => void update(s, s.enabled, '')}>
                    Remove
                  </button>
                </Show>
              </div>
            </Show>
          </>
        )}
      </For>
    </>
  );
}

function DeviceSettings() {
  const kinds: DeviceKind[] = ['kobo', 'kindle', 'generic'];
  const [conversion] = createResource(() => api.conversionInfo().catch(() => ({ calibre: null })));
  return (
    <>
      <Heading>Devices</Heading>
      <p class="mb-4 text-xs text-muted">
        When a book has more than one format a device reads, Magnitide asks which to send, unless you pick one here.
        Books without it are converted on the way when possible, else get the best format they have. In a Magnitide
        library the converted file is added to the book too, so it's only converted once.
      </p>
      <For each={kinds}>
        {(kind) => (
          <Row
            label={DEVICE_KIND_NAMES[kind]}
            description={`Reads ${DEVICE_FORMATS[kind].slice(0, 4).join(', ')}${kind === 'kindle' ? ' (not EPUB over USB)' : ''}…`}
          >
            <select
              class="h-7 rounded-control border border-line bg-field px-2 text-xs"
              aria-label={`Format to send to ${DEVICE_KIND_NAMES[kind]}`}
              value={preferredFormat(kind) ?? ''}
              onChange={(e) => setPreferredFormat(kind, e.currentTarget.value || null)}
            >
              <option value="">Ask each time</option>
              <For each={DEVICE_FORMATS[kind]}>{(f) => <option value={f}>{f}</option>}</For>
            </select>
          </Row>
        )}
      </For>
      <Row
        label="Converting"
        description={
          <Show when={conversion()} fallback="Checking…">
            {(c) =>
              c().calibre ? (
                <>
                  EPUB → KEPUB (Kobo) and EPUB → AZW3 (Kindle) are built in; everything else uses calibre's converter at{' '}
                  <span class="break-all">{c().calibre}</span>.
                </>
              ) : (
                <>
                  EPUB → KEPUB (Kobo) and EPUB → AZW3 (Kindle) are built in. Install calibre to also convert to MOBI,
                  PDF and more, or from other formats; Magnitide finds it by itself.
                </>
              )
            }
          </Show>
        }
      />
    </>
  );
}

function ShortcutSettings() {
  return (
    <>
      <Heading>Keyboard Shortcuts</Heading>
      <For each={SHORTCUTS}>
        {(g) => (
          <section class="mb-6">
            <h4 class="mb-1 text-[11px] font-semibold tracking-[.06em] text-muted uppercase">{g.group}</h4>
            <For each={g.items}>
              {(s) => (
                <div class="flex items-center justify-between gap-6 border-b border-line py-2 last:border-b-0">
                  <span>{s.action}</span>
                  <span class="flex flex-none flex-wrap justify-end gap-1">
                    <For each={s.keys}>
                      {(k) => (
                        <kbd class="min-w-6 rounded-inset border border-line bg-element px-1.5 py-0.5 text-center font-sans text-xs text-fg shadow-[0_1px_0_var(--color-line)]">
                          {keys(k)}
                        </kbd>
                      )}
                    </For>
                  </span>
                </div>
              )}
            </For>
          </section>
        )}
      </For>
    </>
  );
}
