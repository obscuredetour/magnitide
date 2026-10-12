// Updates: the newer version a check found, and installing it. Checked soon
// after launch and once a day while Magnitide runs, unless that's turned off in
// Settings › About; the banner over the library and Settings › About show what
// was found, and either installs it (updates.rs does the work).

import { createSignal } from 'solid-js';

import { api, errorMessage, onUpdateProgress, type UpdateInfo } from './api';
import { startJob } from './jobs';

const DAY = 24 * 60 * 60 * 1000;
/** After launch, so the check doesn't compete with opening the library. */
const FIRST_CHECK = 10_000;

const [available, setAvailable] = createSignal<UpdateInfo | null>(null);
const [installing, setInstalling] = createSignal(false);
/** The version whose banner was put off (Later): not shown again this session. */
const [putOff, setPutOff] = createSignal<string | null>(null);
export { available, installing };

/** The update the banner offers, if any. */
export const bannerUpdate = () => {
  const u = available();
  return u && u.version !== putOff() && !installing() ? u : null;
};

export const putOffUpdate = () => setPutOff(available()?.version ?? null);

/** Look for a newer version: null when this is the latest. Throws when it can't look. */
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  const found = await api.checkForUpdate();
  setAvailable(found);
  return found;
}

/** Check soon, then once a day, whenever automatic checks are on at the time. Call once, at launch. */
export function scheduleUpdateChecks(): void {
  const check = async () => {
    try {
      const s = await api.updateStatus();
      if (s.supported && s.autoCheck && !installing()) await checkForUpdate();
    } catch {
      // Offline, or GitHub unreachable: tomorrow's check may do better.
    }
  };
  setTimeout(() => {
    void check();
    setInterval(() => void check(), DAY);
  }, FIRST_CHECK);
}

const mb = (bytes: number) => `${Math.max(1, Math.round(bytes / 1e6))} MB`;

/** Download and install the update found, then restart into it; progress is in the status bar. */
export async function installUpdate(): Promise<void> {
  const u = available();
  if (!u || installing()) return;
  setInstalling(true);
  const name = `Magnitide ${u.version}`;
  const job = startJob('Update', `Downloading ${name}…`);
  const unlisten = await onUpdateProgress((p) => {
    if (p.phase === 'downloading') {
      job.progress(p.total ? `Downloading ${name}: ${mb(p.done)} of ${mb(p.total)}` : `Downloading ${name}…`);
    } else job.progress(`Installing ${name}…`);
  });
  try {
    await api.installUpdate();
    // Magnitide restarts into the new version from here.
    job.progress(`Restarting into ${name}…`);
  } catch (err) {
    job.finish(errorMessage(err), { error: true });
    setInstalling(false);
  } finally {
    unlisten();
  }
}
