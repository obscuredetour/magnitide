import { getVersion } from '@tauri-apps/api/app';
import { createResource, createSignal, Show } from 'solid-js';

import { api, errorMessage } from '../../api';
import { toast } from '../../toast';
import { available, checkForUpdate, installing, installUpdate } from '../../updates';
import { button, Group, linkButton, Row, Toggle } from './parts';

export function About() {
  const [version] = createResource(() => getVersion().catch(() => null));
  return (
    <>
      <div class="flex flex-col items-center pt-10 text-center">
        <img src="/icon.png" alt="" width="112" height="112" />
        <h3 class="mt-4 font-serif text-2xl font-semibold">Magnitide</h3>
        <Show when={version()}>{(v) => <div class="mt-1 text-xs text-muted">Version {v()}</div>}</Show>
        <p class="mt-4 max-w-sm text-sm text-muted">
          A fast ebook library manager. Opens calibre libraries read-only, and imports them into Magnitide libraries
          without changing them.
        </p>
        <p class="mt-4 text-xs text-faint">Licensed under the GNU GPL v3 or later.</p>
      </div>
      <Updates />
    </>
  );
}

/** Checking for and installing a newer version, where this copy can update itself. */
function Updates() {
  const [status, { mutate }] = createResource(() => api.updateStatus().catch(() => undefined));
  const [checking, setChecking] = createSignal(false);
  const [upToDate, setUpToDate] = createSignal(false);

  const setAuto = async (on: boolean) => {
    try {
      mutate(await api.setAutoUpdate(on));
    } catch (err) {
      toast(errorMessage(err), { error: true });
    }
  };

  const check = async () => {
    setChecking(true);
    setUpToDate(false);
    try {
      setUpToDate(!(await checkForUpdate()));
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setChecking(false);
    }
  };

  return (
    <Show when={status()}>
      {(s) => (
        <Group title="Updates" class="mx-auto mt-10 max-w-xl">
          <Show when={s().supported} fallback={<Row label="Updating Magnitide" description={s().unsupported} />}>
            <Row
              label="Check for updates automatically"
              description="When Magnitide starts, and once a day while it’s open. Nothing is installed until you say so."
            >
              <Toggle
                label="Check for updates automatically"
                checked={s().autoCheck}
                onChange={(on) => void setAuto(on)}
              />
            </Row>
            <Show
              when={available()}
              fallback={
                <Row
                  label="Check now"
                  description={upToDate() ? 'You have the latest version.' : 'Look for a newer version.'}
                >
                  <button class={button} disabled={checking()} onClick={() => void check()}>
                    {checking() ? 'Checking…' : 'Check for Updates'}
                  </button>
                </Row>
              }
            >
              {(u) => (
                <Row
                  label={`Magnitide ${u().version} is available`}
                  description={
                    <Show
                      when={!installing()}
                      fallback="Installing… the progress is in the status bar. Magnitide restarts when it’s done."
                    >
                      Installing it restarts Magnitide.{' '}
                      <button class={linkButton} onClick={() => void api.openReleaseNotes(u().version).catch(() => {})}>
                        What’s new
                      </button>
                    </Show>
                  }
                >
                  <button class={button} disabled={installing()} onClick={() => void installUpdate()}>
                    Update and Restart
                  </button>
                </Row>
              )}
            </Show>
          </Show>
        </Group>
      )}
    </Show>
  );
}
