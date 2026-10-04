import { For, Show } from 'solid-js';

import { api, errorMessage, fileManagerName } from '../api';
import { chooseLibrary, items, library, openLibrary, recentLibraries } from '../library';
import { toast } from '../toast';
import { Menu, MenuItem, MenuLabel, MenuSeparator } from './Menu';

export function LibraryMenu() {
  const count = () => {
    const total = library()?.bookCount ?? 0;
    const shown = items().length;
    return shown === total
      ? `${total.toLocaleString()} books`
      : `${shown.toLocaleString()} of ${total.toLocaleString()}`;
  };
  const others = () => recentLibraries().filter((r) => r.path !== library()?.path);

  const showFolder = async () => {
    try {
      await api.showLibraryFolder();
    } catch (err) {
      toast(errorMessage(err), { error: true });
    }
  };

  return (
    <Menu
      label="Library"
      title={library()?.path ?? 'Open a library'}
      class="min-w-0 justify-self-start"
      buttonClass="-ml-2 flex max-w-full min-w-0 items-baseline gap-2 rounded-[7px] px-2 py-[5px] hover:bg-line aria-expanded:bg-line"
      panelClass="w-80"
      button={
        <>
          <span class="truncate text-sm font-[650]">{library()?.name ?? 'Magnitide'}</span>
          <Show when={library()}>
            <span class="whitespace-nowrap text-muted tabular-nums max-[1080px]:hidden">{count()}</span>
          </Show>
          <svg
            class="size-2.5 flex-none self-center fill-none stroke-muted stroke-2"
            viewBox="0 0 10 10"
            aria-hidden="true"
          >
            <path d="m2 3.5 3 3 3-3" />
          </svg>
        </>
      }
    >
      <Show when={library()}>
        {(lib) => (
          <>
            <div class="px-2.5 pt-1 pb-2">
              <div class="truncate font-semibold" title={lib().path}>
                {lib().name}
              </div>
              <div class="truncate text-xs text-muted" title={lib().path}>
                {lib().path}
              </div>
              <div class="mt-0.5 text-xs text-faint">
                {lib().bookCount.toLocaleString()} books · loaded in {lib().loadMs} ms · read-only
              </div>
            </div>
            <MenuItem onSelect={() => void showFolder()}>Show in {fileManagerName}</MenuItem>
            <MenuSeparator />
          </>
        )}
      </Show>
      <Show when={others().length}>
        <MenuLabel>Recent</MenuLabel>
        <For each={others()}>
          {(r) => (
            <MenuItem onSelect={() => void openLibrary(r.path)} title={r.path}>
              <span class="truncate">{r.name}</span>
            </MenuItem>
          )}
        </For>
        <MenuSeparator />
      </Show>
      <MenuItem onSelect={() => void chooseLibrary()}>Open Another Library…</MenuItem>
    </Menu>
  );
}
