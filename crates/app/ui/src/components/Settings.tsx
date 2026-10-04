import { getVersion } from '@tauri-apps/api/app';
import { createResource, createSignal, For, type JSX, Match, onMount, type ParentProps, Show, Switch } from 'solid-js';

import { api, errorMessage, fileManagerName } from '../api';
import { chooseLibrary, forgetRecentLibrary, library, openLibrary, recentLibraries } from '../library';
import { setTheme, THEMES, themeChoice } from '../theme';
import { toast } from '../toast';
import { isColumnVisible, resetColumnSizing, setColumnVisible, setViewMode, viewMode } from '../view';
import { TABLE_COLUMNS } from './BookTable';
import { CoverSizeSlider } from './CoverSizeSlider';

type Section = 'appearance' | 'library' | 'list' | 'about';

const SECTIONS: { id: Section; label: string }[] = [
  { id: 'appearance', label: 'Appearance' },
  { id: 'library', label: 'Library' },
  { id: 'list', label: 'List View' },
  { id: 'about', label: 'About' },
];

const button =
  'cursor-pointer rounded-md border border-line bg-surface-2 px-3 py-1 text-xs font-medium hover:bg-line disabled:cursor-default disabled:opacity-50';

/** The settings window (⌘,). Mount it to open; `onClose` unmounts it. */
export function SettingsDialog(props: { onClose(): void }) {
  let dialog!: HTMLDialogElement;
  const [section, setSection] = createSignal<Section>('appearance');

  onMount(() => dialog.showModal());

  return (
    <dialog
      ref={dialog}
      aria-label="Settings"
      class="m-auto h-[min(560px,calc(100vh-80px))] w-[min(760px,calc(100vw-48px))] animate-menu-in overflow-hidden rounded-2xl border border-line bg-surface p-0 text-fg shadow-lift backdrop:bg-black/40"
      onCancel={(e) => {
        // Esc: let Solid unmount the dialog instead of the browser closing it.
        e.preventDefault();
        props.onClose();
      }}
      // A click on the dialog element itself (not its contents) is the backdrop.
      onClick={(e) => e.target === dialog && props.onClose()}
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
                class="rounded-md px-2.5 py-1.5 text-left hover:bg-line aria-[current=page]:bg-accent/15 aria-[current=page]:text-accent"
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
            class="float-right -mt-1 -mr-2 grid size-7 place-items-center rounded-full hover:bg-line"
            title="Close (Esc)"
            aria-label="Close settings"
            onClick={() => props.onClose()}
          >
            <svg class="size-3 stroke-muted stroke-2 [stroke-linecap:round]" viewBox="0 0 20 20" aria-hidden="true">
              <path d="m5 5 10 10M15 5 5 15" />
            </svg>
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
    <div data-theme={props.theme} class="flex h-full w-full flex-col gap-1.5 bg-bg p-2">
      <div class="h-1.5 w-1/2 rounded-full bg-fg/70" />
      <div class="flex flex-1 gap-1.5">
        <div class="flex-1 rounded bg-surface shadow-card" />
        <div class="flex-1 rounded bg-surface-2" />
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
      <div class="h-[68px] w-full overflow-hidden rounded-lg border border-line group-aria-checked:outline-2 group-aria-checked:outline-offset-2 group-aria-checked:outline-accent">
        {props.children}
      </div>
      <span class="text-xs group-aria-checked:font-semibold">{props.label}</span>
    </button>
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
        <For each={THEMES}>
          {(t) => (
            <ThemeCard selected={themeChoice() === t.id} label={t.label} onSelect={() => setTheme(t.id)}>
              <ThemePreview theme={t.id} />
            </ThemeCard>
          )}
        </For>
      </div>

      <Row label="View" description="Also ⌘1 and ⌘2.">
        <div class="flex rounded-lg bg-line p-0.5" role="group" aria-label="View">
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
                class="rounded-md px-3 py-1 text-xs font-medium text-muted aria-pressed:bg-surface aria-pressed:text-fg aria-pressed:shadow-card"
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
                {lib().bookCount.toLocaleString()} books · opened read-only
              </>
            }
          >
            <button class={button} onClick={() => void run(api.showLibraryFolder)}>
              Show in {fileManagerName}
            </button>
          </Row>
        )}
      </Show>
      <Row label="Open another library" description="Any folder containing a calibre metadata.db.">
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
  return (
    <>
      <Heading>List View</Heading>
      <div class="mb-1 font-medium">Columns</div>
      <div class="mb-4 text-xs text-muted">Also available from the ⋯ button at the end of the list header.</div>
      <div class="mb-6 grid grid-cols-2 gap-x-6 gap-y-2">
        <For each={TABLE_COLUMNS}>
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
      <Row label="Column widths" description="Drag a column's edge to resize it; double-click to reset one.">
        <button class={button} onClick={() => resetColumnSizing()}>
          Reset All
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
        An AI-free ebook library manager for calibre libraries. Your library is opened read-only and never changed.
      </p>
      <p class="mt-4 text-xs text-faint">Licensed under the GNU GPL v3 or later.</p>
    </div>
  );
}
