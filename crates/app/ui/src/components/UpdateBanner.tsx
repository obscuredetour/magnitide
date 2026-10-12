import { Show } from 'solid-js';
import CircleArrowUp from 'lucide-solid/icons/circle-arrow-up';

import { api } from '../api';
import { smallButton } from '../buttons';
import { bannerUpdate, installUpdate, putOffUpdate } from '../updates';
import { Icon } from './Icon';

/** A newer Magnitide, found by the automatic check (or Settings › About): install it, or put it off. */
export function UpdateBanner() {
  return (
    <Show when={bannerUpdate()}>
      {(u) => (
        <div
          class="flex flex-none items-center gap-3 border-b border-line bg-highlight px-5 py-2 text-xs"
          role="status"
        >
          <Icon of={CircleArrowUp} size={16} class="text-accent" />
          <p class="min-w-0 flex-1">
            <strong>Magnitide {u().version} is available.</strong> Installing it restarts Magnitide.{' '}
            <button
              class="cursor-pointer text-link hover:underline"
              onClick={() => void api.openReleaseNotes(u().version).catch(() => {})}
            >
              What’s new
            </button>
          </p>
          <button class="flex-none cursor-pointer text-muted hover:text-fg" onClick={putOffUpdate}>
            Later
          </button>
          <button class={`${smallButton} flex-none`} onClick={() => void installUpdate()}>
            Update and Restart
          </button>
        </div>
      )}
    </Show>
  );
}
