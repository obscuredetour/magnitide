import { createResource, createSignal, For, type JSX, Match, type ParentProps, Show, Switch } from 'solid-js';
import type { Component } from 'solid-js';
import type { LucideProps } from 'lucide-solid';
import Blocks from 'lucide-solid/icons/blocks';
import Package from 'lucide-solid/icons/package';

import {
  api,
  errorMessage,
  type InstalledPlugin,
  type NativeCapability,
  type NativeKind,
  type NativePlugin,
  type PluginHostChoice,
  type PluginKind,
  type PluginsStatus,
  type RuntimeStatus,
} from '../../api';
import { confirm } from '../../confirm';
import { refreshGuiPlugins } from '../../plugins';
import { toast } from '../../toast';
import { Icon } from '../Icon';
import { Select } from '../Select';
import { PluginCalibreDownload, PluginRuntime } from './PluginHosts';
import { button, Heading, help, Label, linkButton, Log, Row, sentence, Toggle } from './parts';

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

/**
 * One side of the page: what kind of plugin and adding one, what's installed
 * (so the two sides line up as a snapshot), then how they work and anything
 * after (`children`).
 */
function Column(
  props: ParentProps<{
    icon: Component<LucideProps>;
    title: string;
    subtitle: string;
    add: JSX.Element;
    installed: JSX.Element;
    about: JSX.Element;
  }>,
) {
  return (
    <section class="min-w-0">
      <header class="mb-4 flex items-center gap-3">
        <span class="grid size-9 flex-none place-items-center rounded-card bg-highlight text-accent">
          <Icon of={props.icon} size={16} />
        </span>
        <div class="min-w-0 flex-1">
          <h4 class="text-[15px] font-semibold">{props.title}</h4>
          <div class={help}>{props.subtitle}</div>
        </div>
        {props.add}
      </header>
      {props.installed}
      <Label class="mt-6">How they work</Label>
      <div class={`mb-6 space-y-1.5 px-1 ${help}`}>{props.about}</div>
      {props.children}
    </section>
  );
}

/** What each kind of plugin does, as a short list. */
function Kinds(props: { items: JSX.Element[] }) {
  return (
    <ul class="list-disc space-y-0.5 pl-4 marker:text-faint">
      <For each={props.items}>{(item) => <li>{item}</li>}</For>
    </ul>
  );
}

/** A plugin: name and version, on or off, what it is and does, and its actions. */
function PluginCard(
  props: ParentProps<{
    name: string;
    version: string;
    description: string;
    enabled: boolean;
    canEnable?: boolean;
    onToggle(on: boolean): void;
    facts: JSX.Element;
    actions: JSX.Element;
  }>,
) {
  return (
    <div class="border-b border-line py-3 last:border-b-0">
      <div class="flex items-start gap-3">
        <div class="min-w-0 flex-1">
          <div class="font-medium">
            {props.name} <span class="text-xs font-normal text-faint tabular-nums">{props.version}</span>
          </div>
          <Show when={props.description}>
            <p class={`mt-0.5 ${help}`}>{sentence(props.description)}</p>
          </Show>
          <div class="mt-1.5 space-y-0.5 text-xs text-faint">{props.facts}</div>
        </div>
        <Toggle
          label={`Use ${props.name}`}
          checked={props.enabled}
          disabled={props.canEnable === false}
          onChange={(on) => props.onToggle(on)}
        />
      </div>
      <div class="mt-2.5 flex flex-wrap gap-2">{props.actions}</div>
      {props.children}
    </div>
  );
}

/** What's installed, under a label with the count, or what to do when nothing is. */
function Installed(props: ParentProps<{ count: number; empty: JSX.Element }>) {
  return (
    <>
      <Label>
        Installed
        <Show when={props.count}>
          <span class="ml-1.5 font-normal tabular-nums">{props.count}</span>
        </Show>
      </Label>
      <div class="rounded-card border border-line bg-bg/40 px-4">
        <Show when={props.count} fallback={<p class={`py-3 ${help}`}>{props.empty}</p>}>
          {props.children}
        </Show>
      </div>
    </>
  );
}

export function PluginSettings() {
  const [status, { mutate, refetch }] = createResource(() => api.pluginsStatus().catch(() => undefined));
  return (
    <>
      <Heading title="Plugins">
        <p>
          Plugins add metadata sources to Find Metadata, tools for the selected books (in the book menu, under Plugins),
          and steps that run on books as they’re added.
        </p>
        <p>Magnitide runs its own plugins, and calibre’s.</p>
      </Heading>
      <Show when={status()} fallback={<p class={help}>Checking the plugins…</p>}>
        {(s) => (
          <div class="grid items-start gap-x-8 gap-y-10 @[700px]:grid-cols-2">
            <NativePlugins
              plugins={s().native}
              errors={s().nativeErrors}
              onChange={(native) => mutate({ ...s(), native })}
              onInstalled={() => void refetch()}
            />
            <CalibrePlugins status={s()} onChange={mutate} onInstalled={() => void refetch()} />
          </div>
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
    <Column
      icon={Package}
      title="Magnitide plugins"
      subtitle=".mgp packages"
      add={
        <button class={button} disabled={busy()} onClick={() => void add()}>
          {busy() ? 'Adding…' : 'Add Plugin…'}
        </button>
      }
      about={
        <>
          <p>
            Small, sandboxed programs that can do only what their package declares (use the network, say). Magnitide
            stops any that misbehave, and nothing else needs installing.
          </p>
          <Kinds
            items={[
              'Metadata sources join Find Metadata.',
              'Import hooks run on files you add, and see only that file.',
              'Metadata plugins read and write metadata in book files.',
              'Library tools are in the book menu, under Plugins, and work on the selected books.',
            ]}
          />
          <p>
            Writing one is described in <code>docs/plugin-api.md</code>.
          </p>
        </>
      }
      installed={
        <Installed count={props.plugins.length} empty="No Magnitide plugins yet. Add one from its .mgp package.">
          <For each={props.plugins}>
            {(p) => (
              <PluginCard
                name={p.manifest.name}
                version={p.manifest.version}
                description={p.manifest.description}
                enabled={p.enabled}
                onToggle={(on) => void run(() => api.setNativePluginEnabled(p.manifest.name, on))}
                facts={
                  <>
                    <p>
                      {NATIVE_KINDS[p.manifest.kind]}
                      <Show when={p.manifest.author}> · by {p.manifest.author}</Show> · {capabilities(p)}
                    </p>
                    <For each={nativeHandlesLines(p)}>{(line) => <p>{line}</p>}</For>
                    <Show when={props.errors[p.manifest.name]}>
                      {(error) => <p class="text-danger select-text">Last error: {error()}</p>}
                    </Show>
                  </>
                }
                actions={
                  <>
                    <button class={button} onClick={() => showLog(p)}>
                      {log()?.name === p.manifest.name ? 'Hide Log' : 'Log'}
                    </button>
                    <button class={button} onClick={() => void remove(p)}>
                      Remove
                    </button>
                  </>
                }
              >
                <Show when={log()?.name === p.manifest.name && log()}>
                  {(l) => <Log lines={l().lines} class="mt-2.5" />}
                </Show>
              </PluginCard>
            )}
          </For>
        </Installed>
      }
    />
  );
}

/** calibre plugins: where they run, adding them, turning on and off, configuring, the log. */
function CalibrePlugins(props: { status: PluginsStatus; onChange(s: PluginsStatus): void; onInstalled(): void }) {
  const s = () => props.status;
  const [busy, setBusy] = createSignal(false);
  const [log, setLog] = createSignal<string[] | null>(null);
  const setRuntime = (runtime: RuntimeStatus) => props.onChange({ ...s(), runtime });

  const add = async () => {
    const path = await api.choosePluginZip();
    if (!path) return;
    setBusy(true);
    try {
      const p = await api.installPlugin(path);
      props.onInstalled();
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
        props.onChange({ ...s(), plugins });
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
    <Column
      icon={Blocks}
      title="calibre plugins"
      subtitle=".zip files"
      add={
        <button
          class={button}
          disabled={!s().runtime.host || busy()}
          title={s().runtime.host ? undefined : 'Set up where they run first'}
          onClick={() => void add()}
        >
          {busy() ? 'Adding…' : 'Add Plugin…'}
        </button>
      }
      installed={
        <Installed
          count={s().plugins.length}
          empty={
            <>
              No calibre plugins yet. They’re zip files, from calibre’s plugin index (in calibre: Preferences › Plugins
              › Get new plugins) or their authors’ pages.
            </>
          }
        >
          <For each={s().plugins}>
            {(p) => (
              <PluginCard
                name={p.name}
                version={p.version}
                description={p.description}
                enabled={p.enabled}
                canEnable={SUPPORTED_PLUGIN_KINDS.includes(p.kind)}
                onToggle={(on) => void run(() => api.setPluginEnabled(p.name, on))}
                facts={
                  <>
                    <p>
                      {PLUGIN_KINDS[p.kind]} · by {p.author}
                    </p>
                    <Show when={!SUPPORTED_PLUGIN_KINDS.includes(p.kind)}>
                      <p class="text-warning">Not supported: {UNSUPPORTED_WHY[p.kind] ?? "Magnitide can't run it"}</p>
                    </Show>
                    <For each={pluginHooksLine(p)}>{(line) => <p>{line}</p>}</For>
                    <Show when={p.coverage.length}>
                      <p class="text-warning">
                        Uses calibre features Magnitide doesn’t provide: {p.coverage.join(', ')}
                      </p>
                    </Show>
                    <Show when={s().errors[p.name]}>
                      {(error) => <p class="text-danger select-text">Last error: {error()}</p>}
                    </Show>
                  </>
                }
                actions={
                  <>
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
                  </>
                }
              />
            )}
          </For>
        </Installed>
      }
      about={
        <>
          <p>
            They work as they do in calibre, with their own settings kept apart from calibre’s. calibre itself needn’t
            be installed.
          </p>
          <Kinds
            items={[
              'Metadata sources are asked in Find Metadata.',
              'Library tools (calibre’s toolbar plugins) are in the book menu, under Plugins. Their windows open as in calibre, and what they change comes back into the library.',
              'Other kinds can be added, and start working as Magnitide supports them.',
            ]}
          />
          <p class="text-warning">Plugins are programs that run on your computer: only add ones you trust.</p>
        </>
      }
    >
      <Label>Where they run</Label>
      <div class="mb-5 rounded-card border border-line bg-bg/40 px-4">
        <Row
          stacked
          label="Plugins run in"
          description={
            <>
              <Switch>
                <Match when={s().runtime.host === 'calibre' && s().runtime.hostChoice === 'downloaded'}>
                  The calibre {s().runtime.calibreDownload.version} Magnitide downloaded: plugins run on calibre’s own
                  code.
                </Match>
                <Match when={s().runtime.host === 'calibre' && !s().runtime.calibre}>
                  The calibre {s().runtime.calibreDownload.version} Magnitide downloaded: plugins run on calibre’s own
                  code.
                </Match>
                <Match when={s().runtime.host === 'calibre'}>
                  Your installed calibre {s().runtime.calibre?.version}: plugins run on calibre’s own code.
                </Match>
                <Match when={s().runtime.host === 'runtime'}>
                  Magnitide’s plugin runtime: a copy of Python, Qt and the parts of calibre that plugins use, downloaded
                  once.
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
                  setRuntime(runtime);
                  void refreshGuiPlugins();
                },
                (err) => toast(errorMessage(err), { error: true }),
              );
            }}
          >
            <option value="auto">Whichever is there</option>
            <option value="calibre">Your installed calibre</option>
            <option value="downloaded">calibre downloaded by Magnitide</option>
            <option value="runtime">Magnitide’s runtime</option>
          </Select>
        </Row>
        <Show when={s().runtime.hostChoice === 'auto' || s().runtime.hostChoice === 'downloaded'}>
          <Row
            stacked
            label="Downloaded calibre"
            description={<PluginCalibreDownload status={s().runtime} onChange={setRuntime} />}
          />
        </Show>
        <Show when={s().runtime.hostChoice === 'auto' || s().runtime.hostChoice === 'runtime'}>
          <Row
            stacked
            label="Plugin runtime"
            description={<PluginRuntime status={s().runtime} onChange={setRuntime} />}
          />
        </Show>
        <Row
          stacked
          label="Conversions"
          description="When a plugin converts books (FanFicFare after downloading, say), they’re converted to this. Plugins see it as calibre’s output format."
        >
          <Select
            class="h-7 pl-2 text-xs"
            aria-label="Format plugins convert to"
            value={s().outputFormat}
            onChange={(e) => {
              const format = e.currentTarget.value;
              api.setPluginOutputFormat(format).then(
                () => props.onChange({ ...s(), outputFormat: format }),
                (err) => toast(errorMessage(err), { error: true }),
              );
            }}
          >
            <For each={PLUGIN_OUTPUT_FORMATS}>{(f) => <option value={f}>{f}</option>}</For>
          </Select>
        </Row>
      </div>

      <Show when={s().runtime.host && s().plugins.length}>
        <div class="mt-3 px-1">
          <button
            class={`${linkButton} text-xs`}
            onClick={() =>
              log()
                ? setLog(null)
                : void api.pluginLog().then(setLog, (err) => toast(errorMessage(err), { error: true }))
            }
          >
            {log() ? 'Hide the plugin log' : 'Show the plugin log'}
          </button>
          <Show when={log()}>{(lines) => <Log lines={lines()} class="mt-2" />}</Show>
        </div>
      </Show>
    </Column>
  );
}
