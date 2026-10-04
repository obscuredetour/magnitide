import { createSignal, Show } from 'solid-js';

import { library } from '../library';
import { storage } from '../storage';

const key = (path: string) => `networkWarningDismissed:${path}`;

/**
 * Shown for Magnitide libraries on network shares (SMB, NFS, …), where
 * SQLite and file locking can't be relied on. Dismissible per library.
 */
export function NetworkWarning() {
  const [dismissed, setDismissed] = createSignal<string | null>(null);
  const show = () => {
    const lib = library();
    return (
      lib?.kind === 'magnitide' &&
      lib.onNetworkShare &&
      dismissed() !== lib.path &&
      storage.get(key(lib.path)) !== 'yes'
    );
  };
  return (
    <Show when={show()}>
      <div
        class="flex flex-none items-start gap-3 border-b border-line bg-[color-mix(in_srgb,var(--danger)_10%,var(--bg))] px-5 py-2.5 text-xs"
        role="status"
      >
        <svg class="mt-px size-4 flex-none fill-none stroke-danger stroke-[1.8]" viewBox="0 0 20 20" aria-hidden="true">
          <path d="M10 3 2 17h16L10 3Z" stroke-linejoin="round" />
          <path d="M10 8.5v3.5M10 14.6v.1" stroke-linecap="round" />
        </svg>
        <p class="min-w-0 flex-1">
          <strong>This library is on a network share.</strong> Databases and file locks aren’t reliable over network
          shares, so open it from only one computer at a time. For shared access, Magnitide’s server mode (coming) is
          the safe way.
        </p>
        <button
          class="flex-none cursor-pointer text-muted hover:text-fg"
          onClick={() => {
            const path = library()?.path;
            if (!path) return;
            storage.set(key(path), 'yes');
            setDismissed(path);
          }}
        >
          Don’t show again
        </button>
      </div>
    </Show>
  );
}
