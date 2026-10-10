// Where calibre plugins run: calibre's own build, downloaded by Magnitide,
// and Estuary, Magnitide's compatibility layer. Each installs, shows what's there, and
// removes itself.

import { createSignal, Match, Show, Switch } from 'solid-js';

import { api, errorMessage, onPluginHostProgress, type PluginHostStatus } from '../../api';
import { confirm } from '../../confirm';
import { startJob } from '../../jobs';
import { refreshGuiPlugins } from '../../plugins';
import { toast } from '../../toast';
import { linkButton } from './parts';

/** calibre's own build, downloaded by Magnitide as a plugin host. */
export function PluginCalibreDownload(props: { status: PluginHostStatus; onChange(r: PluginHostStatus): void }) {
  const [working, setWorking] = createSignal<'install' | 'remove' | null>(null);
  const mb = (bytes: number | null | undefined) => (bytes == null ? '' : `${Math.max(1, Math.round(bytes / 1e6))} MB`);
  const d = () => props.status.calibreDownload;

  const install = async () => {
    setWorking('install');
    const job = startJob('calibre download', `Downloading calibre ${d().version}…`);
    const unlisten = await onPluginHostProgress((p) => {
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
          <button class={linkButton} disabled={!!working()} onClick={() => void remove()}>
            {working() === 'remove' ? 'Removing…' : 'Remove'}
          </button>
        </span>
      </Match>
      <Match when={d().installing || working() === 'install'}>
        <span>
          Downloading… the progress is in the status bar.{' '}
          <button class={linkButton} onClick={() => void api.cancelPluginHostInstall()}>
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
          <button class={linkButton} disabled={!!working()} onClick={() => void install()}>
            Download
          </button>
        </span>
      </Match>
    </Switch>
  );
}

export function PluginEstuary(props: { status: PluginHostStatus; onChange(r: PluginHostStatus): void }) {
  const [working, setWorking] = createSignal<'install' | 'remove' | 'check' | null>(null);
  const mb = (bytes: number | null | undefined) => (bytes == null ? '' : `${Math.max(1, Math.round(bytes / 1e6))} MB`);

  const install = async (file: string | null = null) => {
    setWorking('install');
    const job = startJob('Estuary', file ? 'Installing Estuary…' : 'Downloading Estuary…');
    const unlisten = await onPluginHostProgress((p) => {
      if (p.phase === 'downloading') {
        job.progress(p.total ? `Downloading Estuary: ${mb(p.done)} of ${mb(p.total)}` : 'Downloading Estuary…');
      } else if (p.phase === 'verifying') job.progress('Checking the download…');
      else if (p.phase === 'extracting') job.progress('Unpacking Estuary…');
      else if (p.phase === 'compiling') job.progress('Preparing Estuary…');
    });
    try {
      const r = file ? await api.installEstuaryFromFile(file) : await api.installEstuary();
      props.onChange(r);
      job.finish(r.problem ? `Installed, but it can't start here: ${r.problem}` : 'Estuary installed', {
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
    const path = await api.chooseEstuaryArchive();
    if (path) await install(path);
  };

  const remove = async () => {
    const ok = await confirm({
      title: 'Remove Estuary?',
      message: () => `Plugins and their settings are kept; Estuary (${mb(props.status.size)}) can be installed again.`,
      confirm: 'Remove',
      danger: true,
    });
    if (!ok) return;
    setWorking('remove');
    try {
      props.onChange(await api.removeEstuary());
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
      props.onChange(await api.estuaryStatus());
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
          Python {props.status.python ?? '?'} &bull; Qt {props.status.qt ?? '?'} &bull; calibre {props.status.compat}{' '}
          compatibility
          <br />
          {props.status.size != null ? `${mb(props.status.size)}` : ''} @{' '}
          <span class="break-all">{props.status.path}</span>.{' '}
          <Show when={props.status.problem}>
            {(problem) => (
              <span class="text-danger">
                It can't start here: {problem()}{' '}
                <button class={linkButton} disabled={!!working()} onClick={() => void check()}>
                  Check again
                </button>
                .{' '}
              </span>
            )}
          </Show>
          <button class={linkButton} disabled={!!working()} onClick={() => void remove()}>
            {working() === 'remove' ? 'Removing…' : 'Remove'}
          </button>
        </span>
      </Match>
      <Match when={props.status.installing || working() === 'install'}>
        <span>
          Installing… the progress is in the status bar.{' '}
          <button class={linkButton} onClick={() => void api.cancelPluginHostInstall()}>
            Cancel
          </button>
        </span>
      </Match>
      <Match when={!props.status.available}>
        <span class="text-warning">
          Estuary isn't built for this computer yet; calibre plugins need a calibre here.
        </span>
      </Match>
      <Match when={true}>
        <span>
          Not installed. It runs calibre plugins without a calibre; it's a one-time download
          {props.status.downloadSize != null ? ` of ${mb(props.status.downloadSize)}` : ''}.{' '}
          <button class={linkButton} disabled={!!working()} onClick={() => void install()}>
            Install
          </button>
          {' · '}
          <button
            class={linkButton}
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
                Can't download here? Get <span class="select-text">{file()}</span> from Estuary's release on GitHub and
                install it from the file; only that archive is accepted.
              </span>
            )}
          </Show>
        </span>
      </Match>
    </Switch>
  );
}
