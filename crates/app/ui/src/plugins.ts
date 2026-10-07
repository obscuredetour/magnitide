// GUI calibre plugins: their menus (the book menu's Plugins submenu) and
// running them. A plugin runs in calibre's own code on a copy of the library;
// when it's done, what it changed is applied here and reported as a job
// (see crates/app/src/plugins.rs).

import { createSignal } from 'solid-js';

import {
  api,
  type ApplyReport,
  coverChanged,
  errorMessage,
  onPluginAsk,
  onPluginDone,
  onPluginMessage,
  onPluginSelect,
  type PluginMenu,
  type PluginMenuItem,
  type PluginMenus,
} from './api';
import { type JobHandle, recordJob, startJob } from './jobs';
import { editBooks, openEditor, untilEditorsClosed } from './editor';
import { refresh, selectBooks, setEditRevision } from './library';
import { toast } from './toast';
import { convertBooks, deleteBooks } from './transfer';

/** The GUI plugins that are on, by name; known without starting calibre. */
const [guiPlugins, setGuiPlugins] = createSignal<string[]>([]);
/** Their menus, once loaded; null until then (loading starts calibre). */
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

/** Load the plugins' menus, starting calibre the first time. */
export function loadPluginMenus(): Promise<void> {
  if (!loading) {
    loading = api
      .pluginMenus()
      .then(
        (m) => {
          setPluginMenus(m);
          for (const e of m.errors) console.warn(e);
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
 * item its toolbar button, on these books.
 */
export async function runPlugin(menu: PluginMenu, item: PluginMenuItem | null, texts: string[], ids: number[]) {
  const job = startJob(menu.plugin, `${item ? texts.join(' › ') : menu.text}…`);
  running.get(menu.plugin)?.job.finish('Still running when it was started again');
  running.set(menu.plugin, { job, messages: [], errors: [] });
  try {
    await api.runPlugin(menu.plugin, item ? item.path : [], texts, ids);
  } catch (err) {
    running.delete(menu.plugin);
    job.finish(errorMessage(err), { error: true });
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
  // calibre's Edit Metadata and Remove Books are Magnitide's: what the plugin
  // did so far is in the library already; the plugin waits for the answer.
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
