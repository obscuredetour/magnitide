import { getVersion } from '@tauri-apps/api/app';
import { createResource, createSignal, For, type JSX, Match, onMount, type ParentProps, Show, Switch } from 'solid-js';
import X from 'lucide-solid/icons/x';

import {
  api,
  type CustomField,
  type DeviceKind,
  errorMessage,
  type FieldKind,
  fileManagerName,
  type InstalledPlugin,
  type NativeCapability,
  type NativeKind,
  type NativePlugin,
  onPluginRuntimeProgress,
  type PluginKind,
  type RuntimeStatus,
  type PluginHostChoice,
  type ServerStatus,
  type SourceId,
  type SourceStatus,
} from '../api';
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
import { customFields, FIELD_KINDS, loadCustomFields } from '../fields';
import { addonReport, addThemeFile, loadThemeAddons, setTheme, themeAttrs, themeChoice, themes } from '../theme';
import { AccentPicker } from './AccentPicker';
import { toast } from '../toast';
import { refreshGuiPlugins } from '../plugins';
import { startJob } from '../jobs';
import { confirm } from '../confirm';
import {
  columnOrder,
  isColumnVisible,
  resetColumnOrder,
  resetColumnSizing,
  setColumnVisible,
  setViewMode,
  blendAllThemes,
  seriesBadges,
  setBlendAllThemes,
  setSeriesBadges,
  setToolbarLayout,
  setZebraRows,
  toolbarLayout,
  viewMode,
  zebraRows,
} from '../view';
import { DEVICE_FORMATS, DEVICE_KIND_NAMES, preferredFormat, setPreferredFormat } from '../devices';
import { describeWrite, plural, setWriteOnSave, writeOnSave } from '../writeback';
import { tableColumns } from './BookTable';
import { CoverSizeSlider } from './CoverSizeSlider';
import { resetZoom, tryZoom, ZOOM_STEPS, zoom, zoomLabel } from '../zoom';
import { Icon } from './Icon';
import { Select } from './Select';
import { smallButton } from '../buttons';
import { backdropDismiss } from '../backdrop';
import { keys, SHORTCUTS } from '../keys';
import { formatSize } from '../sizes';

export type SettingsSection = Section;
type Section = 'appearance' | 'library' | 'list' | 'devices' | 'server' | 'sources' | 'plugins' | 'shortcuts' | 'about';

const SECTIONS: { id: Section; label: string }[] = [
  { id: 'appearance', label: 'Appearance' },
  { id: 'library', label: 'Library' },
  { id: 'list', label: 'List View' },
  { id: 'devices', label: 'Devices' },
  { id: 'server', label: 'Library Server' },
  { id: 'sources', label: 'Metadata Sources' },
  { id: 'plugins', label: 'Plugins' },
  { id: 'shortcuts', label: 'Keyboard Shortcuts' },
  { id: 'about', label: 'About' },
];

const button = smallButton;

/** The settings window (⌘,). Mount it to open; `onClose` unmounts it. */
export function SettingsDialog(props: { onClose(): void; section?: Section }) {
  let dialog!: HTMLDialogElement;
  const [section, setSection] = createSignal<Section>(props.section ?? 'appearance');

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
            <Match when={section() === 'server'}>
              <ServerSettings />
            </Match>
            <Match when={section() === 'plugins'}>
              <PluginSettings />
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

      <Row
        label="Cover colours behind the details"
        description="On every theme, not only Light and Dark. Those two are made for it; on a themed palette the colours may clash."
      >
        <input
          type="checkbox"
          class="size-4 accent-accent"
          aria-label="Cover colours behind the details on every theme"
          checked={blendAllThemes()}
          onChange={(e) => setBlendAllThemes(e.currentTarget.checked)}
        />
      </Row>

      <Row label="Toolbar" description="Two rows, or everything in one for more room below.">
        <div class="flex rounded-control bg-element p-0.5" role="group" aria-label="Toolbar">
          <For
            each={
              [
                ['two-rows', 'Two Rows'],
                ['one-row', 'One Row'],
              ] as const
            }
          >
            {([layout, label]) => (
              <button
                class="rounded-inset px-3 py-1 text-xs font-medium text-muted aria-pressed:bg-popover aria-pressed:text-fg aria-pressed:shadow-card"
                aria-pressed={toolbarLayout() === layout}
                onClick={() => setToolbarLayout(layout)}
              >
                {label}
              </button>
            )}
          </For>
        </div>
      </Row>
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
      <Row label="Series labels" description="A label on the cover with the book's place in its series.">
        <input
          type="checkbox"
          class="size-4 accent-accent"
          aria-label="Series labels on covers"
          checked={seriesBadges()}
          onChange={(e) => setSeriesBadges(e.currentTarget.checked)}
        />
      </Row>
      <InterfaceSize />
    </>
  );
}

/** "Date Read" → "date_read", as Rust makes lookup names (a preview only). */
function keyFrom(name: string): string {
  const key = name
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '');
  return /^[0-9]/.test(key) ? `f_${key}` : key;
}

/** Custom fields: listed for any library; made, renamed and deleted in Magnitide ones. */
function CustomFieldsSettings(props: { editable: boolean }) {
  const [adding, setAdding] = createSignal(false);
  const [editing, setEditing] = createSignal<number | null>(null);

  const changed = async () => {
    await loadCustomFields();
    setEditRevision((n) => n + 1);
    await refresh();
  };

  const remove = async (f: CustomField) => {
    const ok = await confirm({
      title: `Delete “${f.name}”?`,
      message: () => `Every book’s ${f.name} goes with it. This can’t be undone.`,
      confirm: 'Delete Field',
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteCustomField(f.id);
      await changed();
      toast(`Deleted the field “${f.name}”`);
    } catch (err) {
      toast(errorMessage(err), { error: true });
    }
  };

  const describe = (f: CustomField) =>
    [
      `#${f.key}`,
      FIELD_KINDS[f.kind] + (f.multiple ? ', several values' : ''),
      f.kind === 'choice' && f.choices.length ? f.choices.join(', ') : null,
    ]
      .filter(Boolean)
      .join(' · ');

  return (
    <>
      <div class="mt-6 mb-1 font-medium">Custom fields</div>
      <p class="mb-1 text-xs text-muted">
        Fields of your own, like Pages or Read. They show in the details panel and as columns in the list (pick them in
        the column menu), are edited with the rest of a book, and are searched with{' '}
        <code class="text-fg">#key:value</code> (<code class="text-fg">#pages:300</code>). calibre’s custom columns come
        across when you import a calibre library.
      </p>
      <For each={customFields()}>
        {(f) => (
          <Show
            when={editing() === f.id}
            fallback={
              <Row label={f.name} description={describe(f)}>
                <Show when={props.editable}>
                  <button class={button} onClick={() => setEditing(f.id)}>
                    {f.kind === 'choice' ? 'Edit…' : 'Rename…'}
                  </button>
                  <button class={button} onClick={() => void remove(f)}>
                    Delete
                  </button>
                </Show>
              </Row>
            }
          >
            <FieldForm field={f} onDone={() => setEditing(null)} onSaved={changed} />
          </Show>
        )}
      </For>
      <Show when={props.editable}>
        <Show
          when={adding()}
          fallback={
            <div class="mt-2">
              <button class={button} onClick={() => setAdding(true)}>
                Add Field…
              </button>
            </div>
          }
        >
          <FieldForm onDone={() => setAdding(false)} onSaved={changed} />
        </Show>
      </Show>
    </>
  );
}

/** Make a field, or (with `field`) rename it and edit its choices. Its kind is fixed once made. */
function FieldForm(props: { field?: CustomField; onDone(): void; onSaved(): Promise<void> }) {
  const [name, setName] = createSignal(props.field?.name ?? '');
  const [key, setKey] = createSignal('');
  const [kind, setKind] = createSignal<FieldKind>(props.field?.kind ?? 'text');
  const [multiple, setMultiple] = createSignal(false);
  const [choices, setChoices] = createSignal(props.field?.choices.join(', ') ?? '');
  const [error, setError] = createSignal<string | null>(null);
  const [saving, setSaving] = createSignal(false);
  const field =
    'h-7 w-full rounded-control border border-line bg-field px-2 text-xs outline-none select-text focus:border-focus';
  const choiceList = () =>
    choices()
      .split(',')
      .map((c) => c.trim())
      .filter(Boolean);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      if (props.field) {
        await api.updateCustomField(props.field.id, name(), kind() === 'choice' ? choiceList() : null);
      } else {
        await api.addCustomField({
          name: name(),
          key: key(),
          kind: kind(),
          multiple: multiple(),
          choices: choiceList(),
        });
      }
      await props.onSaved();
      props.onDone();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <form
      class="my-2 grid grid-cols-[7rem_1fr] items-center gap-x-3 gap-y-2 rounded-control border border-line p-3 text-xs"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <label for="field-name" class="text-muted">
        Name
      </label>
      <input id="field-name" class={field} value={name()} autofocus onInput={(e) => setName(e.currentTarget.value)} />
      <Show when={!props.field}>
        <label for="field-key" class="text-muted">
          Lookup name
        </label>
        <input
          id="field-key"
          class={field}
          placeholder={keyFrom(name()) || 'pages'}
          value={key()}
          onInput={(e) => setKey(e.currentTarget.value)}
        />
        <label for="field-kind" class="text-muted">
          Kind
        </label>
        <Select
          wide
          id="field-kind"
          class="h-7 pl-2 text-xs outline-none focus:border-focus"
          value={kind()}
          onChange={(e) => setKind(e.currentTarget.value as FieldKind)}
        >
          <For each={Object.entries(FIELD_KINDS)}>{([k, label]) => <option value={k}>{label}</option>}</For>
        </Select>
        <Show when={kind() === 'text'}>
          <span />
          <label class="flex items-center gap-2">
            <input
              type="checkbox"
              class="size-4 accent-accent"
              checked={multiple()}
              onChange={(e) => setMultiple(e.currentTarget.checked)}
            />
            Several values per book, like tags
          </label>
        </Show>
      </Show>
      <Show when={kind() === 'choice'}>
        <label for="field-choices" class="text-muted">
          Choices
        </label>
        <input
          id="field-choices"
          class={field}
          placeholder="To Read, Reading, Read"
          value={choices()}
          onInput={(e) => setChoices(e.currentTarget.value)}
        />
      </Show>
      <Show when={error()}>
        {(e) => (
          <p class="col-span-2 text-danger select-text" role="alert">
            {e()}
          </p>
        )}
      </Show>
      <div class="col-span-2 flex justify-end gap-2">
        <button type="button" class={button} onClick={() => props.onDone()}>
          Cancel
        </button>
        <button type="submit" class={button} disabled={saving() || !name().trim()}>
          {saving() ? 'Saving…' : props.field ? 'Save' : 'Add Field'}
        </button>
      </div>
    </form>
  );
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
      <Show when={library()}>
        <CustomFieldsSettings editable={library()?.kind === 'magnitide'} />
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
                {c().files.toLocaleString()} cached · {formatSize(c().bytes)}. Rebuilt automatically as covers are
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
    const all = tableColumns();
    const known = order.filter((id) => all.some((c) => c.id === id));
    return [...known.map((id) => all.find((c) => c.id === id)!), ...all.filter((c) => !known.includes(c.id))];
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
        A fast ebook library manager. Opens calibre libraries read-only, and imports them into Magnitide libraries
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
  const [plugins, { mutate: setPlugins }] = createResource(() =>
    api
      .pluginsStatus()
      .then((s) => s.plugins)
      .catch(() => [] as InstalledPlugin[]),
  );
  const [native, { mutate: setNative }] = createResource(() =>
    api
      .pluginsStatus()
      .then((s) => s.native)
      .catch(() => [] as NativePlugin[]),
  );
  const pluginSources = () => (plugins() ?? []).filter((p) => p.kind === 'metadataSource');
  const nativeSources = () => (native() ?? []).filter((p) => p.manifest.kind === 'metadata-source');
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
      <Show when={nativeSources().length}>
        <h4 class="mt-6 mb-1 text-[11px] font-semibold tracking-[.06em] text-muted uppercase">From plugins</h4>
        <For each={nativeSources()}>
          {(p) => (
            <Row
              label={p.manifest.name}
              description={`${sentence(p.manifest.description)} Added in Settings › Plugins.`}
            >
              <input
                type="checkbox"
                class="size-4 accent-accent"
                aria-label={`Use ${p.manifest.name}`}
                checked={p.enabled}
                onChange={(e) =>
                  void api
                    .setNativePluginEnabled(p.manifest.name, e.currentTarget.checked)
                    .then(setNative, (err) => toast(errorMessage(err), { error: true }))
                }
              />
            </Row>
          )}
        </For>
      </Show>
      <Show when={pluginSources().length}>
        <h4 class="mt-6 mb-1 text-[11px] font-semibold tracking-[.06em] text-muted uppercase">From calibre plugins</h4>
        <For each={pluginSources()}>
          {(p) => (
            <Row label={p.name} description={`${sentence(p.description)} Added in Settings › Plugins.`}>
              <input
                type="checkbox"
                class="size-4 accent-accent"
                aria-label={`Use ${p.name}`}
                checked={p.enabled}
                onChange={(e) =>
                  void api
                    .setPluginEnabled(p.name, e.currentTarget.checked)
                    .then(setPlugins, (err) => toast(errorMessage(err), { error: true }))
                }
              />
            </Row>
          )}
        </For>
      </Show>
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
            <Select
              class="h-7 pl-2 text-xs"
              aria-label={`Format to send to ${DEVICE_KIND_NAMES[kind]}`}
              value={preferredFormat(kind) ?? ''}
              onChange={(e) => setPreferredFormat(kind, e.currentTarget.value || null)}
            >
              <option value="">Ask each time</option>
              <For each={DEVICE_FORMATS[kind]}>{(f) => <option value={f}>{f}</option>}</For>
            </Select>
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

function ServerSettings() {
  const [status, { mutate }] = createResource(() => api.serverStatus().catch(() => undefined));
  const [port, setPort] = createSignal('');
  const [password, setPassword] = createSignal('');
  const [busy, setBusy] = createSignal(false);

  const update = async (enabled: boolean, newPassword: string | null = null) => {
    const current = status();
    if (!current) return;
    const n = Number(port() || current.port);
    if (!Number.isInteger(n) || n < 1024 || n > 65535) {
      toast('Pick a port from 1024 to 65535.', { error: true });
      return;
    }
    setBusy(true);
    try {
      const next: ServerStatus = await api.setServer(enabled, n, newPassword);
      mutate(next);
      setPort('');
      if (newPassword !== null) {
        setPassword('');
        toast(newPassword ? 'Password saved' : 'Password removed');
      }
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setBusy(false);
    }
  };

  const copy = (text: string) =>
    navigator.clipboard.writeText(text).then(
      () => toast('Address copied'),
      () => toast("Couldn't copy the address", { error: true }),
    );

  return (
    <>
      <Heading>Library Server</Heading>
      <p class="mb-4 text-xs text-muted">
        Shares the open library on your Wi-Fi while Magnitide is running. E-reader web browsers (Kobo's, Kindle's, a
        phone's) get a simple page to download books from, and reading apps such as KOReader can add it as an OPDS
        catalogue and sync reading positions through it. It's read-only: nothing on it can change your library.
      </p>
      <Show when={status()}>
        {(s) => (
          <>
            <Row
              label="Share the library"
              description={
                <Show when={s().error} fallback="Off until you turn it on; remembered after that.">
                  <span class="text-danger">{s().error}</span>
                </Show>
              }
            >
              <input
                type="checkbox"
                class="size-4 accent-accent"
                aria-label="Share the library on the network"
                checked={s().enabled}
                disabled={busy()}
                onChange={(e) => void update(e.currentTarget.checked)}
              />
            </Row>
            <Row label="Port" description="Change it if another app already uses this one.">
              <input
                type="number"
                min="1024"
                max="65535"
                aria-label="Port"
                class="h-7 w-24 rounded-control border border-line bg-field px-2 text-xs tabular-nums outline-none focus:border-focus"
                value={port() || String(s().port)}
                onInput={(e) => setPort(e.currentTarget.value)}
                onKeyDown={(e) => e.key === 'Enter' && port() && void update(s().enabled)}
                onBlur={() => port() && Number(port()) !== s().port && void update(s().enabled)}
              />
            </Row>
            <Row
              label="Password"
              description={
                s().hasPassword
                  ? 'Browsers and apps ask for it (any user name works).'
                  : 'Without one, anyone on your network can browse and download your books.'
              }
            >
              <input
                type="password"
                autocomplete="new-password"
                aria-label="Server password"
                placeholder={s().hasPassword ? 'Saved · type to replace' : 'None'}
                class="h-7 w-44 rounded-control border border-line bg-field px-2 text-xs outline-none focus:border-focus"
                value={password()}
                onInput={(e) => setPassword(e.currentTarget.value)}
                onKeyDown={(e) => e.key === 'Enter' && password() && void update(s().enabled, password())}
              />
              <button
                class={button}
                disabled={!password() || busy()}
                onClick={() => void update(s().enabled, password())}
              >
                Save
              </button>
              <Show when={s().hasPassword}>
                <button class={button} disabled={busy()} onClick={() => void update(s().enabled, '')}>
                  Remove
                </button>
              </Show>
            </Row>
            <Show when={s().running && s().url}>
              {(url) => (
                <div class="mt-4 flex gap-5 rounded-card border border-line bg-bg/50 p-4">
                  <Show when={s().qrSvg}>
                    {(svg) => (
                      <div
                        class="size-36 flex-none overflow-hidden rounded-inset bg-white p-1.5 [&>svg]:size-full"
                        role="img"
                        aria-label={`QR code for ${url()}`}
                        // Our own SVG, made by the qrcode crate from the address.
                        innerHTML={svg()}
                      />
                    )}
                  </Show>
                  <div class="min-w-0 space-y-2.5 text-xs">
                    <div class="flex items-center gap-2">
                      <span class="font-mono text-sm font-medium break-all select-all">{url()}</span>
                      <button class={button} onClick={() => void copy(url())}>
                        Copy
                      </button>
                    </div>
                    <Show when={s().otherUrls.length > 0}>
                      <p class="text-muted">
                        On another network: <span class="font-mono select-all">{s().otherUrls.join(', ')}</span>
                      </p>
                    </Show>
                    <p>
                      <span class="font-medium">Kobo:</span>{' '}
                      <span class="text-muted">
                        More › Beta Features › Web Browser, then go to this address. Books download straight into your
                        library, as KEPUB.
                      </span>
                    </p>
                    <p>
                      <span class="font-medium">KOReader and other reading apps:</span>{' '}
                      <span class="text-muted">
                        add an OPDS catalogue at <span class="font-mono break-all select-all">{url()}opds</span>.
                      </span>
                    </p>
                    <p>
                      <span class="font-medium">KOReader progress sync:</span>{' '}
                      <span class="text-muted">
                        in a book, Tools › Progress sync › Custom sync server:{' '}
                        <span class="font-mono break-all select-all">{url().replace(/\/$/, '')}</span>, then Log in with
                        any user name and{' '}
                        {s().hasPassword ? 'the password above' : 'any password (set one above to keep others out)'}.
                        Each user name has its own reading positions.
                      </span>
                    </p>
                    <p class="text-muted">
                      If macOS asks whether Magnitide may accept incoming connections, allow it, or devices can't
                      connect.
                    </p>
                    <Show when={s().vpn}>
                      <p class="text-muted">
                        A VPN is on. If devices can't connect, allow local network (LAN) connections in its settings, or
                        turn it off while you download.
                      </p>
                    </Show>
                  </div>
                </div>
              )}
            </Show>
          </>
        )}
      </Show>
    </>
  );
}

/** A description as a sentence (a full stop added when it has none). */
function sentence(s: string): string {
  const t = s.trim();
  return t ? t.replace(/[^.!?]$/, '$&.') : '';
}

const NATIVE_KINDS: Record<NativeKind, string> = {
  'metadata-source': 'Metadata source',
  'import-hook': 'Runs on files being added',
  'file-metadata': 'Reads and writes metadata in book files',
  'library-action': 'Library tool',
};

/** What an import-hook or file-metadata plugin handles, as Settings lines. */
function nativeHandlesLines(p: NativePlugin): string[] {
  const lines: string[] = [];
  const types = (v: string[]) => v.map((t) => (t === '*' ? 'every file' : t.toUpperCase())).join(', ');
  if (p.handles.fileTypes.length) lines.push(`Runs when adding: ${types(p.handles.fileTypes)}`);
  if (p.handles.reads.length)
    lines.push(
      `Reads metadata from: ${p.handles.reads.map((r) => r.format + (r.overrides ? ' (before Magnitide)' : '')).join(', ')}`,
    );
  if (p.handles.writes.length) lines.push(`Writes metadata into: ${p.handles.writes.join(', ')}`);
  if (p.handles.menu.length)
    lines.push(`In the book menu, under Plugins: ${p.handles.menu.map((m) => m.text).join(', ')}`);
  return lines;
}

/** What a capability lets a plugin do, as Settings shows it. */
const NATIVE_CAPABILITIES: Record<NativeCapability, string> = {
  network: 'uses the network',
  'write-library': 'edits books',
  'read-files': 'reads book files',
};

const PLUGIN_KINDS: Record<PluginKind, string> = {
  metadataSource: 'Metadata source',
  interfaceAction: 'Library tool',
  fileType: 'File type',
  metadataReader: 'Metadata reader',
  metadataWriter: 'Metadata writer',
  conversionInput: 'Conversion input',
  conversionOutput: 'Conversion output',
  catalog: 'Catalog',
  preferences: 'Settings page',
  libraryClosed: 'Runs when the library closes',
  device: 'Device driver',
  store: 'Store',
  viewer: 'Viewer',
  editBookTool: 'Book editor tool',
  other: 'Other',
};

/** What a plugin's conversions can produce (natively, or with calibre, which plugins need anyway). */
const PLUGIN_OUTPUT_FORMATS = ['EPUB', 'KEPUB', 'AZW3', 'MOBI', 'PDF', 'DOCX', 'TXT'];

/** The kinds Magnitide runs; the rest are kept, with why (matches `PluginKind::why_unsupported`). */
const SUPPORTED_PLUGIN_KINDS: PluginKind[] = [
  'metadataSource',
  'interfaceAction',
  'fileType',
  'metadataReader',
  'metadataWriter',
  'preferences',
  'libraryClosed',
];
const UNSUPPORTED_WHY: Partial<Record<PluginKind, string>> = {
  conversionInput: "needs calibre's conversion pipeline, which Magnitide doesn't run",
  conversionOutput: "needs calibre's conversion pipeline, which Magnitide doesn't run",
  catalog: 'Magnitide has no catalog generator yet',
  device: 'Magnitide has its own device support',
  store: 'Magnitide has no book store',
  viewer: 'Magnitide has no viewer',
  editBookTool: 'Magnitide has no book editor',
  other: "Magnitide doesn't know this kind of plugin",
};

/** "Runs when adding: epub, pdf" and the like, from what a plugin handles. */
function pluginHooksLine(p: InstalledPlugin): string[] {
  const h = p.hooks;
  if (!h) return [];
  const types = h.fileTypes.includes('*') ? 'every format' : h.fileTypes.join(', ');
  const lines: string[] = [];
  if (p.kind === 'metadataReader') lines.push(`Reads metadata from: ${types}`);
  else if (p.kind === 'metadataWriter') lines.push(`Writes metadata into: ${types}`);
  else {
    const when = [
      h.onImport && 'adding',
      h.onPostimport && 'after adding',
      h.onPostadd && 'after a book is added',
      h.onPostconvert && 'after converting',
      h.onPostdelete && 'after removing a format',
    ].filter(Boolean);
    if (when.length) lines.push(`Runs when ${when.join(', ')}: ${types}`);
    if (h.onPreprocess || h.onPostprocess) lines.push("Also has conversion hooks Magnitide doesn't run");
  }
  return lines;
}

function PluginSettings() {
  const [status, { mutate, refetch }] = createResource(() => api.pluginsStatus().catch(() => undefined));
  const [busy, setBusy] = createSignal(false);
  const [log, setLog] = createSignal<string[] | null>(null);
  const setPlugins = (plugins: InstalledPlugin[]) => {
    const s = status();
    if (s) mutate({ ...s, plugins });
  };

  const add = async () => {
    const path = await api.choosePluginZip();
    if (!path) return;
    setBusy(true);
    try {
      const p = await api.installPlugin(path);
      await refetch();
      void refreshGuiPlugins();
      toast(
        p.kind === 'metadataSource'
          ? `Added ${p.name}: it's now asked in Find Metadata`
          : p.kind === 'interfaceAction'
            ? `Added ${p.name}: its menu is in the book menu, under Plugins`
            : SUPPORTED_PLUGIN_KINDS.includes(p.kind)
              ? `Added ${p.name}`
              : `Added ${p.name}. Magnitide can't run this kind of plugin: ${UNSUPPORTED_WHY[p.kind] ?? 'not supported'}.`,
      );
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setBusy(false);
    }
  };

  const run = (f: () => Promise<InstalledPlugin[]>) =>
    f().then(
      (plugins) => {
        setPlugins(plugins);
        void refreshGuiPlugins();
      },
      (err) => toast(errorMessage(err), { error: true }),
    );

  const remove = async (p: InstalledPlugin) => {
    const ok = await confirm({
      title: `Remove ${p.name}?`,
      message: () => 'Its settings are kept, in case you add it again.',
      confirm: 'Remove',
      danger: true,
    });
    if (ok) void run(() => api.removePlugin(p.name));
  };

  const [configuring, setConfiguring] = createSignal<string | null>(null);
  const configure = async (p: InstalledPlugin) => {
    setConfiguring(p.name);
    try {
      const r = await api.configurePlugin(p.name);
      if (r.changed)
        toast(r.restart ? `${p.name}'s settings saved; they apply the next time it runs` : 'Settings saved');
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setConfiguring(null);
    }
  };

  return (
    <>
      <Heading>Plugins</Heading>
      <p class="mb-4 text-xs text-muted">
        Magnitide plugins are .mgp packages: small, sandboxed programs that can do only what their package declares (the
        network, say), and that Magnitide stops if they misbehave. Metadata sources join Find Metadata; import hooks run
        on files you add, and see only that file; metadata plugins read and write book formats; library tools are in the
        book menu, under Plugins, and work on the selected books. Nothing else needs to be installed. Writing one is
        described in docs/plugin-api.md.
      </p>
      <Show when={status()} fallback={<p class="text-xs text-muted">Checking the plugins…</p>}>
        {(s) => (
          <NativePlugins
            plugins={s().native}
            errors={s().nativeErrors}
            onChange={(native) => mutate({ ...s(), native })}
            onInstalled={() => void refetch()}
          />
        )}
      </Show>
      <h4 class="mt-8 mb-1 text-[11px] font-semibold tracking-[.06em] text-muted uppercase">calibre plugins</h4>
      <p class="mb-4 text-xs text-muted">
        Magnitide runs calibre plugins in its own plugin runtime: a copy of Python, Qt and the parts of calibre that
        plugins use, downloaded once. calibre itself needn't be installed. Plugins work as they do in calibre, with
        their own settings kept apart from calibre's. Metadata sources are asked in Find Metadata. Library tools
        (calibre's toolbar plugins) are in the book menu, under Plugins: they work on the selected books, their windows
        open as in calibre, and what they change comes back into the library when they're done. Other kinds of plugin
        can be added and will start working as Magnitide supports them. Plugins are programs that run on your computer:
        only add ones you trust.
      </p>
      <Show when={status()} fallback={<p class="text-xs text-muted">Checking the plugin runtime…</p>}>
        {(s) => (
          <>
            <Row
              label="Plugins run in"
              description={
                <>
                  <Switch>
                    <Match when={s().runtime.host === 'calibre' && s().runtime.hostChoice === 'downloaded'}>
                      The calibre {s().runtime.calibreDownload.version} Magnitide downloaded: plugins run on calibre's
                      own code.
                    </Match>
                    <Match when={s().runtime.host === 'calibre' && !s().runtime.calibre}>
                      The calibre {s().runtime.calibreDownload.version} Magnitide downloaded: plugins run on calibre's
                      own code.
                    </Match>
                    <Match when={s().runtime.host === 'calibre'}>
                      Your installed calibre {s().runtime.calibre?.version}: plugins run on calibre's own code.
                    </Match>
                    <Match when={s().runtime.host === 'runtime'}>
                      Magnitide's plugin runtime (a copy of Python, Qt and the parts of calibre that plugins use,
                      downloaded once); calibre needn't be installed.
                    </Match>
                    <Match when={true}>
                      <span class="text-warning">{s().runtime.hostProblem ?? 'Nothing to run plugins in.'}</span>
                    </Match>
                  </Switch>
                  <Show when={s().runtime.calibre?.problem}>
                    {(problem) => <span class="mt-0.5 block text-warning">{problem()}</span>}
                  </Show>
                </>
              }
            >
              <Select
                class="h-7 pl-2 text-xs"
                aria-label="Where plugins run"
                value={s().runtime.hostChoice}
                onChange={(e) => {
                  const choice = e.currentTarget.value as PluginHostChoice;
                  api.setPluginHost(choice).then(
                    (runtime) => {
                      mutate({ ...s(), runtime });
                      void refreshGuiPlugins();
                    },
                    (err) => toast(errorMessage(err), { error: true }),
                  );
                }}
              >
                <option value="auto">Whichever is there</option>
                <option value="calibre">Your installed calibre</option>
                <option value="downloaded">calibre downloaded by Magnitide</option>
                <option value="runtime">Magnitide's runtime</option>
              </Select>
              <button class={button} disabled={!s().runtime.host || busy()} onClick={() => void add()}>
                {busy() ? 'Adding…' : 'Add Plugin…'}
              </button>
            </Row>
            <Show when={s().runtime.hostChoice === 'auto' || s().runtime.hostChoice === 'downloaded'}>
              <Row
                label="Downloaded calibre"
                description={
                  <PluginCalibreDownload status={s().runtime} onChange={(runtime) => mutate({ ...s(), runtime })} />
                }
              />
            </Show>
            <Show when={s().runtime.hostChoice === 'auto' || s().runtime.hostChoice === 'runtime'}>
              <Row
                label="Plugin runtime"
                description={<PluginRuntime status={s().runtime} onChange={(runtime) => mutate({ ...s(), runtime })} />}
              />
            </Show>
            <Row
              label="Plugin conversions"
              description="When a plugin converts books (FanFicFare after downloading, say), they're converted to this. Plugins see it as calibre's output format."
            >
              <Select
                class="h-7 pl-2 text-xs"
                aria-label="Format plugins convert to"
                value={s().outputFormat}
                onChange={(e) => {
                  const format = e.currentTarget.value;
                  api.setPluginOutputFormat(format).then(
                    () => mutate({ ...s(), outputFormat: format }),
                    (err) => toast(errorMessage(err), { error: true }),
                  );
                }}
              >
                <For each={PLUGIN_OUTPUT_FORMATS}>{(f) => <option value={f}>{f}</option>}</For>
              </Select>
            </Row>
            <Show
              when={s().plugins.length}
              fallback={
                <p class="py-3.5 text-xs text-muted">
                  No plugins yet. calibre plugins are zip files, from calibre's plugin index (in calibre: Preferences ›
                  Plugins › Get new plugins) or their authors' pages.
                </p>
              }
            >
              <For each={s().plugins}>
                {(p) => (
                  <Row
                    label={`${p.name} ${p.version}`}
                    description={
                      <>
                        {p.description}
                        <span class="mt-0.5 block">
                          {PLUGIN_KINDS[p.kind]} · by {p.author}
                          <Show when={!SUPPORTED_PLUGIN_KINDS.includes(p.kind)}>
                            <span class="text-warning">
                              {' '}
                              · Not supported: {UNSUPPORTED_WHY[p.kind] ?? "Magnitide can't run it"}
                            </span>
                          </Show>
                        </span>
                        <For each={pluginHooksLine(p)}>{(line) => <span class="mt-0.5 block">{line}</span>}</For>
                        <Show when={p.coverage.length}>
                          <span class="mt-0.5 block text-warning">
                            Uses calibre features Magnitide doesn't provide: {p.coverage.join(', ')}
                          </span>
                        </Show>
                        <Show when={s().errors[p.name]}>
                          {(error) => <span class="mt-0.5 block text-danger select-text">Last error: {error()}</span>}
                        </Show>
                      </>
                    }
                  >
                    <input
                      type="checkbox"
                      class="size-4 accent-accent"
                      aria-label={`Use ${p.name}`}
                      checked={p.enabled}
                      disabled={!SUPPORTED_PLUGIN_KINDS.includes(p.kind)}
                      onChange={(e) => void run(() => api.setPluginEnabled(p.name, e.currentTarget.checked))}
                    />
                    <Show when={p.customizable && SUPPORTED_PLUGIN_KINDS.includes(p.kind)}>
                      <button
                        class={button}
                        disabled={!s().runtime.host || configuring() !== null}
                        onClick={() => void configure(p)}
                      >
                        {configuring() === p.name ? 'Configuring…' : 'Configure…'}
                      </button>
                    </Show>
                    <button class={button} onClick={() => void remove(p)}>
                      Remove
                    </button>
                  </Row>
                )}
              </For>
            </Show>
            <Show when={s().runtime.host && s().plugins.length}>
              <div class="mt-4">
                <button
                  class="cursor-pointer text-xs text-link hover:underline"
                  onClick={() =>
                    log()
                      ? setLog(null)
                      : void api.pluginLog().then(setLog, (err) => toast(errorMessage(err), { error: true }))
                  }
                >
                  {log() ? 'Hide the plugin log' : 'Show the plugin log'}
                </button>
                <Show when={log()}>
                  {(lines) => (
                    <pre class="mt-2 max-h-64 overflow-auto rounded-control border border-line bg-bg/50 p-2 text-[11px] whitespace-pre-wrap select-text">
                      {lines().length ? lines().join('\n') : 'Nothing logged yet.'}
                    </pre>
                  )}
                </Show>
              </div>
            </Show>
          </>
        )}
      </Show>
    </>
  );
}

/** Magnitide's own plugins: adding, turning on and off, their logs. */
function NativePlugins(props: {
  plugins: NativePlugin[];
  errors: Record<string, string>;
  onChange(plugins: NativePlugin[]): void;
  onInstalled(): void;
}) {
  const [busy, setBusy] = createSignal(false);
  const [log, setLog] = createSignal<{ name: string; lines: string[] } | null>(null);

  const add = async () => {
    const path = await api.chooseNativePlugin();
    if (!path) return;
    setBusy(true);
    try {
      const p = await api.installNativePlugin(path);
      props.onInstalled();
      void refreshGuiPlugins();
      toast(
        p.manifest.kind === 'metadata-source'
          ? `Added ${p.manifest.name}: it's now asked in Find Metadata`
          : p.manifest.kind === 'import-hook'
            ? `Added ${p.manifest.name}: it runs on files you add`
            : p.manifest.kind === 'library-action'
              ? `Added ${p.manifest.name}: its menu is in the book menu, under Plugins`
              : `Added ${p.manifest.name}: it reads and writes metadata in the formats it handles`,
      );
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setBusy(false);
    }
  };

  const run = (f: () => Promise<NativePlugin[]>) =>
    f().then(
      (plugins) => {
        props.onChange(plugins);
        void refreshGuiPlugins();
      },
      (err) => toast(errorMessage(err), { error: true }),
    );

  const remove = async (p: NativePlugin) => {
    const ok = await confirm({
      title: `Remove ${p.manifest.name}?`,
      message: () => 'The plugin is deleted from Magnitide.',
      confirm: 'Remove',
      danger: true,
    });
    if (ok) void run(() => api.removeNativePlugin(p.manifest.name));
  };

  const showLog = (p: NativePlugin) => {
    if (log()?.name === p.manifest.name) return setLog(null);
    void api.nativePluginLog(p.manifest.name).then(
      (lines) => setLog({ name: p.manifest.name, lines }),
      (err) => toast(errorMessage(err), { error: true }),
    );
  };

  const capabilities = (p: NativePlugin) =>
    p.manifest.capabilities.length
      ? p.manifest.capabilities.map((c) => NATIVE_CAPABILITIES[c]).join(', ')
      : 'works on its own, without the network';

  return (
    <>
      <Row label="Magnitide plugins" description="Add a plugin from its .mgp package.">
        <button class={button} disabled={busy()} onClick={() => void add()}>
          {busy() ? 'Adding…' : 'Add Plugin…'}
        </button>
      </Row>
      <Show when={props.plugins.length} fallback={<p class="py-3.5 text-xs text-muted">No Magnitide plugins yet.</p>}>
        <For each={props.plugins}>
          {(p) => (
            <>
              <Row
                label={`${p.manifest.name} ${p.manifest.version}`}
                description={
                  <>
                    {p.manifest.description}
                    <span class="mt-0.5 block">
                      {NATIVE_KINDS[p.manifest.kind]}
                      <Show when={p.manifest.author}> · by {p.manifest.author}</Show> · {capabilities(p)}
                    </span>
                    <For each={nativeHandlesLines(p)}>{(line) => <span class="mt-0.5 block">{line}</span>}</For>
                    <Show when={props.errors[p.manifest.name]}>
                      {(error) => <span class="mt-0.5 block text-danger select-text">Last error: {error()}</span>}
                    </Show>
                  </>
                }
              >
                <input
                  type="checkbox"
                  class="size-4 accent-accent"
                  aria-label={`Use ${p.manifest.name}`}
                  checked={p.enabled}
                  onChange={(e) => void run(() => api.setNativePluginEnabled(p.manifest.name, e.currentTarget.checked))}
                />
                <button class={button} onClick={() => showLog(p)}>
                  {log()?.name === p.manifest.name ? 'Hide log' : 'Log'}
                </button>
                <button class={button} onClick={() => void remove(p)}>
                  Remove
                </button>
              </Row>
              <Show when={log()?.name === p.manifest.name && log()}>
                {(l) => (
                  <pre class="mb-2 max-h-64 overflow-auto rounded-control border border-line bg-bg/50 p-2 text-[11px] whitespace-pre-wrap select-text">
                    {l().lines.length ? l().lines.join('\n') : 'Nothing logged yet.'}
                  </pre>
                )}
              </Show>
            </>
          )}
        </For>
      </Show>
    </>
  );
}

/** calibre's own build, downloaded by Magnitide as a plugin host. */
function PluginCalibreDownload(props: { status: RuntimeStatus; onChange(r: RuntimeStatus): void }) {
  const [working, setWorking] = createSignal<'install' | 'remove' | null>(null);
  const mb = (bytes: number | null | undefined) => (bytes == null ? '' : `${Math.max(1, Math.round(bytes / 1e6))} MB`);
  const d = () => props.status.calibreDownload;

  const install = async () => {
    setWorking('install');
    const job = startJob('calibre download', `Downloading calibre ${d().version}…`);
    const unlisten = await onPluginRuntimeProgress((p) => {
      if (p.phase === 'downloading') {
        job.progress(p.total ? `Downloading calibre: ${mb(p.done)} of ${mb(p.total)}` : 'Downloading calibre…');
      } else if (p.phase === 'verifying') job.progress('Checking the download…');
      else if (p.phase === 'extracting') job.progress('Unpacking calibre…');
    });
    try {
      props.onChange(await api.installPluginCalibre());
      job.finish(`calibre ${d().version} downloaded`);
      void refreshGuiPlugins();
    } catch (err) {
      job.finish(errorMessage(err), { error: true });
    } finally {
      unlisten();
      setWorking(null);
    }
  };

  const remove = async () => {
    const ok = await confirm({
      title: 'Remove the downloaded calibre?',
      message: () => `Plugins and their settings are kept; calibre (${mb(d().size)}) can be downloaded again.`,
      confirm: 'Remove',
      danger: true,
    });
    if (!ok) return;
    setWorking('remove');
    try {
      props.onChange(await api.removePluginCalibre());
      void refreshGuiPlugins();
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setWorking(null);
    }
  };

  return (
    <Switch>
      <Match when={d().installed}>
        <span>
          calibre {d().version}
          {d().size != null ? `, ${mb(d().size)}` : ''} at <span class="break-all">{d().path}</span>.{' '}
          <button class="cursor-pointer text-link hover:underline" disabled={!!working()} onClick={() => void remove()}>
            {working() === 'remove' ? 'Removing…' : 'Remove'}
          </button>
        </span>
      </Match>
      <Match when={d().installing || working() === 'install'}>
        <span>
          Downloading… the progress is in the status bar.{' '}
          <button
            class="cursor-pointer text-link hover:underline"
            onClick={() => void api.cancelPluginRuntimeInstall()}
          >
            Cancel
          </button>
        </span>
      </Match>
      <Match when={!d().available}>
        <span class="text-warning">calibre publishes no build for this computer.</span>
      </Match>
      <Match when={true}>
        <span>
          Not downloaded. calibre's own build ({mb(d().downloadSize)}, from its GitHub release) runs plugins on
          calibre's code and converts books too.{' '}
          <button
            class="cursor-pointer text-link hover:underline"
            disabled={!!working()}
            onClick={() => void install()}
          >
            Download
          </button>
        </span>
      </Match>
    </Switch>
  );
}

function PluginRuntime(props: { status: RuntimeStatus; onChange(r: RuntimeStatus): void }) {
  const [working, setWorking] = createSignal<'install' | 'remove' | 'check' | null>(null);
  const mb = (bytes: number | null | undefined) => (bytes == null ? '' : `${Math.max(1, Math.round(bytes / 1e6))} MB`);

  const install = async (file: string | null = null) => {
    setWorking('install');
    const job = startJob('Plugin runtime', file ? 'Installing the plugin runtime…' : 'Downloading the plugin runtime…');
    const unlisten = await onPluginRuntimeProgress((p) => {
      if (p.phase === 'downloading') {
        job.progress(
          p.total
            ? `Downloading the plugin runtime: ${mb(p.done)} of ${mb(p.total)}`
            : 'Downloading the plugin runtime…',
        );
      } else if (p.phase === 'verifying') job.progress('Checking the download…');
      else if (p.phase === 'extracting') job.progress('Unpacking the plugin runtime…');
      else if (p.phase === 'compiling') job.progress('Preparing the plugin runtime…');
    });
    try {
      const r = file ? await api.installPluginRuntimeFromFile(file) : await api.installPluginRuntime();
      props.onChange(r);
      job.finish(r.problem ? `Installed, but it can't start here: ${r.problem}` : 'Plugin runtime installed', {
        error: !!r.problem,
      });
      void refreshGuiPlugins();
    } catch (err) {
      job.finish(errorMessage(err), { error: true });
    } finally {
      unlisten();
      setWorking(null);
    }
  };

  const installFromFile = async () => {
    const path = await api.chooseRuntimeArchive();
    if (path) await install(path);
  };

  const remove = async () => {
    const ok = await confirm({
      title: 'Remove the plugin runtime?',
      message: () =>
        `Plugins and their settings are kept; the runtime (${mb(props.status.size)}) can be installed again.`,
      confirm: 'Remove',
      danger: true,
    });
    if (!ok) return;
    setWorking('remove');
    try {
      props.onChange(await api.removePluginRuntime());
      void refreshGuiPlugins();
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setWorking(null);
    }
  };

  const check = async () => {
    setWorking('check');
    try {
      props.onChange(await api.pluginRuntimeStatus());
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setWorking(null);
    }
  };

  return (
    <Switch>
      <Match when={props.status.installed}>
        <span>
          Python {props.status.python ?? '?'}, Qt {props.status.qt ?? '?'}, calibre {props.status.compat} compatibility
          {props.status.size != null ? `, ${mb(props.status.size)}` : ''} at{' '}
          <span class="break-all">{props.status.path}</span>.{' '}
          <Show when={props.status.problem}>
            {(problem) => (
              <span class="text-danger">
                It can't start here: {problem()}{' '}
                <button
                  class="cursor-pointer text-link hover:underline"
                  disabled={!!working()}
                  onClick={() => void check()}
                >
                  Check again
                </button>
                .{' '}
              </span>
            )}
          </Show>
          <button class="cursor-pointer text-link hover:underline" disabled={!!working()} onClick={() => void remove()}>
            {working() === 'remove' ? 'Removing…' : 'Remove'}
          </button>
        </span>
      </Match>
      <Match when={props.status.installing || working() === 'install'}>
        <span>
          Installing… the progress is in the status bar.{' '}
          <button
            class="cursor-pointer text-link hover:underline"
            onClick={() => void api.cancelPluginRuntimeInstall()}
          >
            Cancel
          </button>
        </span>
      </Match>
      <Match when={!props.status.available}>
        <span class="text-warning">
          There's no plugin runtime for this computer yet, so calibre plugins can't run here.
        </span>
      </Match>
      <Match when={true}>
        <span>
          Not installed. calibre plugins need it; it's a one-time download
          {props.status.downloadSize != null ? ` of ${mb(props.status.downloadSize)}` : ''}.{' '}
          <button
            class="cursor-pointer text-link hover:underline"
            disabled={!!working()}
            onClick={() => void install()}
          >
            Install
          </button>
          {' · '}
          <button
            class="cursor-pointer text-link hover:underline"
            disabled={!!working()}
            title={
              props.status.downloadFile
                ? `${props.status.downloadFile}, from ${props.status.downloadUrl}`
                : 'The published archive for this computer'
            }
            onClick={() => void installFromFile()}
          >
            Install from file…
          </button>
          <Show when={props.status.downloadFile}>
            {(file) => (
              <span class="mt-0.5 block text-faint">
                Can't download here? Get <span class="select-text">{file()}</span> from the plugin-runtime release and
                install it from the file; only that archive is accepted.
              </span>
            )}
          </Show>
        </span>
      </Match>
    </Switch>
  );
}
