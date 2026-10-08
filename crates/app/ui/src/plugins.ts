// GUI calibre plugins: their menus (the book menu's Plugins submenu) and
// running them. A plugin runs in Magnitide's plugin runtime on a copy of the
// library; when it's done, what it changed is applied here and reported as a
// job (see crates/app/src/plugins.rs).

import { createSignal } from 'solid-js';

import {
  api,
  type ApplyReport,
  coverChanged,
  errorMessage,
  isRuntimeMissing,
  onPluginAsk,
  onPluginDone,
  onPluginJob,
  onPluginMessage,
  onPluginOpenBook,
  onPluginSearch,
  onPluginSelect,
  type PluginMenu,
  type PluginMenuItem,
  type PluginMenus,
} from './api';
import { type JobHandle, recordJob, startJob } from './jobs';
import { editBooks, openEditor, untilEditorsClosed } from './editor';
import { openBook, refresh, search, selectBooks, setEditRevision } from './library';
import { findMetadataFor, untilLookupDone } from './lookup';
import { toast } from './toast';
import { convertBooks, deleteBooks } from './transfer';
import { confirm } from './confirm';

/** The GUI plugins that are on, by name; known without starting the runtime. */
const [guiPlugins, setGuiPlugins] = createSignal<string[]>([]);
/** Their menus, once loaded; null until then (loading starts the plugin runtime). */
const [pluginMenus, setPluginMenus] = createSignal<PluginMenus | null>(null);
export { guiPlugins, pluginMenus };

let loading: Promise<void> | null = null;

/** Check which GUI plugins are on (at start, and after they change in Settings). */
export async function refreshGuiPlugins(): Promise<void> {
  try {
    setGuiPlugins(await api.guiPluginNames());
  } catch {
    setGuiPlugins([]);
  }
  setPluginMenus(null);
}

/** Load the plugins' menus, starting the plugin runtime the first time. */
export function loadPluginMenus(): Promise<void> {
  if (!loading) {
    loading = api
      .pluginMenus()
      .then(
        (m) => {
          setPluginMenus(m);
          for (const e of m.errors) {
            console.warn(e);
            toast(`A plugin couldn't be loaded. ${e}`, { error: true, duration: 9000 });
          }
        },
        (err) => {
          setPluginMenus({ menus: [], errors: [errorMessage(err)] });
        },
      )
      .finally(() => (loading = null));
  }
  return loading;
}

/** Runs waiting for their plugin to finish, by plugin, with what the plugin said meanwhile. */
const running = new Map<string, { job: JobHandle; messages: string[]; errors: string[] }>();

/**
 * Run a plugin's menu item (`item` at `texts`, its path of names), or with no
 * item its toolbar button, on these books, with the current search (plugins
 * read calibre's search box).
 */
export async function runPlugin(
  menu: PluginMenu,
  item: PluginMenuItem | null,
  texts: string[],
  ids: number[],
  query = '',
) {
  const job = startJob(menu.plugin, `${item ? texts.join(' › ') : menu.text}…`);
  running.get(menu.plugin)?.job.finish('Still running when it was started again');
  running.set(menu.plugin, { job, messages: [], errors: [] });
  try {
    await api.runPlugin(menu.plugin, item ? item.path : [], texts, ids, query);
  } catch (err) {
    running.delete(menu.plugin);
    job.finish(isRuntimeMissing(err) ? `${errorMessage(err)}` : errorMessage(err), {
      error: true,
    });
  }
}

function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

function summary(r: ApplyReport | null, said: string[]): string {
  // A plugin that changed nothing often said why ("Create a #pages column first").
  if (!r) return said.at(-1) ?? 'Done; nothing changed';
  const parts = [
    r.updated.length && `updated ${plural(r.updated.length, 'book')}`,
    r.added.length && `added ${plural(r.added.length, 'book')}`,
    r.fieldsAdded && `added ${plural(r.fieldsAdded, 'field')}`,
  ].filter(Boolean);
  const done = parts.length ? parts.join(', ') : 'nothing changed';
  const failed = r.failed.length ? `; ${plural(r.failed.length, 'book')} couldn't be changed` : '';
  return `${done[0]!.toUpperCase()}${done.slice(1)}${failed}`;
}

/** Listen for plugins finishing, reporting and selecting. Once, at start-up. */
export function initPlugins(): void {
  void refreshGuiPlugins();
  void onPluginDone(async (d) => {
    const run = running.get(d.plugin);
    running.delete(d.plugin);
    const r = d.report;
    if (r) {
      for (const id of r.updated) coverChanged(id);
      setEditRevision((n) => n + 1);
      await refresh();
    }
    const said = run?.messages ?? [];
    const errors = run?.errors ?? [];
    const text = d.error
      ? `Its changes couldn't be applied: ${d.error}`
      : errors.length && !r
        ? `It stopped with an error: ${errors.at(-1)}`
        : summary(r, said);
    const details = [
      ...(r?.details ?? []),
      ...(r?.failed ?? []),
      ...(r?.warnings ?? []),
      ...(r ? said.map((m) => `It said: ${m}`) : said.slice(0, -1).map((m) => `It said: ${m}`)),
      ...errors.map((e) => `Error: ${e}`),
      ...d.convert.map((c) => `Asked to convert ${plural(c.ids.length, 'book')} to ${c.format}`),
    ];
    const error = !!d.error || !!r?.failed.length || (!!errors.length && !r);
    if (run) run.job.finish(text, { error, details });
    // A plugin can also change things on its own (a job it started, a window left open).
    else if (r || d.error) recordJob(d.plugin, text, { error, details });
    if (r?.removed.length) {
      toast(`${d.plugin} removed ${plural(r.removed.length, 'book')}`);
      await deleteBooks(r.removed);
    }
    // calibre's Convert Books is Magnitide's: each runs as its own job.
    for (const c of d.convert) await convertBooks(c.ids, c.format);
  });
  void onPluginMessage((m) => {
    const run = m.plugin ? running.get(m.plugin) : undefined;
    if (run) {
      // Kept for the run's history entry; shown meanwhile as its progress.
      (m.error ? run.errors : run.messages).push(m.message);
      if (!m.error) run.job.progress(m.message);
    }
    if (!run || m.error) {
      toast(m.plugin ? `${m.plugin}: ${m.message}` : m.message, { error: m.error, duration: m.error ? 7000 : 4000 });
    }
  });
  void onPluginSelect((ids) => selectBooks(ids));
  void onPluginSearch((text) => search(text));
  void onPluginOpenBook((b) => void openBook(b.id, b.format));
  // Jobs a plugin runs in the plugin runtime (calibre's job manager) show in the job bar.
  const hostJobs = new Map<number, JobHandle>();
  void onPluginJob((j) => {
    const title = j.plugin ?? 'Plugin';
    if (j.phase === 'start') {
      hostJobs.get(j.job)?.finish('Started again');
      hostJobs.set(j.job, startJob(title, `${j.description || 'Working'}…`));
    } else if (j.phase === 'progress') {
      const text = [j.description, j.message].filter(Boolean).join(': ');
      hostJobs.get(j.job)?.progress(j.percent != null && j.percent > 0 ? `${text} (${j.percent}%)` : `${text}…`);
    } else {
      const job = hostJobs.get(j.job);
      hostJobs.delete(j.job);
      const details = j.details ? j.details.split('\n').filter(Boolean).slice(-40) : [];
      const text = j.failed ? `${j.description || 'The job'} failed` : `${j.description || 'The job'} finished`;
      if (job) job.finish(text, { error: j.failed, details });
      else if (j.failed) recordJob(title, text, { error: true, details });
    }
  });
  // calibre's Edit Metadata, Remove Books, metadata download and Convert
  // Books are Magnitide's: what the plugin did so far is in the library
  // already; the plugin waits for the answer.
  void onPluginAsk(async (a) => {
    const run = running.get(a.plugin);
    try {
      setEditRevision((n) => n + 1);
      await refresh();
      selectBooks(a.ids);
      if (a.kind === 'edit') {
        run?.job.progress('Waiting for the book to be edited…');
        if (a.ids.length === 1) await openEditor(a.ids[0]!);
        else editBooks(a.ids);
        await untilEditorsClosed();
      } else if (a.kind === 'download') {
        run?.job.progress('Waiting for Find Metadata…');
        findMetadataFor(a.ids);
        await untilLookupDone();
      } else if (a.kind === 'convert') {
        run?.job.progress('Waiting for the conversion…');
        const to = await pickConversion(a.plugin, a.ids);
        if (to) await convertBooks(a.ids, to);
      } else {
        run?.job.progress('Waiting to delete…');
        await deleteBooks(a.ids);
      }
      run?.job.progress(`${a.plugin}…`);
    } finally {
      await api.pluginAnswer(a.token, a.kind, a.ids).catch((err) => toast(errorMessage(err), { error: true }));
    }
  });
}

/** calibre's conversion dialog, reduced to its one choice: the format. Null when cancelled. */
async function pickConversion(plugin: string, ids: number[]): Promise<string | null> {
  const targets = await api.convertTargets(ids).catch(() => [] as string[]);
  if (!targets.length) {
    toast(`${plugin} asked to convert ${plural(ids.length, 'book')}, but there's nothing to convert them to.`, {
      error: true,
    });
    return null;
  }
  let chosen = targets[0]!;
  const ok = await confirm({
    title: `${plugin} wants to convert ${plural(ids.length, 'book')}`,
    message: () => {
      const select = document.createElement('select');
      select.className = 'h-7 rounded-control border border-line bg-field pl-2 pr-6 text-xs text-fg';
      select.setAttribute('aria-label', 'Convert to');
      select.autofocus = true;
      for (const t of targets) select.append(new Option(t, t));
      select.onchange = () => (chosen = select.value);
      const label = document.createElement('label');
      label.className = 'flex items-center gap-2';
      label.append('Convert to', select);
      return label;
    },
    confirm: 'Convert',
  });
  return ok ? chosen : null;
}
