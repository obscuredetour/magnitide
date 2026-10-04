import { For, Show } from 'solid-js';

import { api, errorMessage, fileManagerName } from '../api';
import { shownItems } from '../devices';
import { chooseLibrary, library, openLibrary, recentLibraries } from '../library';
import { startImport } from '../importer';
import { chooseAndAddBooks } from '../adder';
import { toast } from '../toast';
import { Menu, MenuItem, MenuLabel, MenuSeparator } from './Menu';

export function LibraryMenu() {
  const count = () => {
    const total = library()?.bookCount ?? 0;
    const shown = shownItems().length;
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
      class="min-w-0"
      buttonClass="-ml-2 flex max-w-full min-w-0 items-baseline gap-2 rounded-[7px] px-2 py-[5px] hover:bg-hover aria-expanded:bg-hover"
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
              <div class="flex items-center justify-between gap-3">
                <span class="truncate font-semibold" title={lib().path}>
                  {lib().name}
                </span>
                <span
                  class="flex-none rounded-full px-2 py-px text-[11px] font-medium"
                  classList={{
                    'bg-accent/15 text-accent': lib().kind === 'magnitide',
                    'bg-element text-muted': lib().kind === 'calibre',
                  }}
                  title={
                    lib().kind === 'magnitide'
                      ? 'A Magnitide library'
                      : 'A calibre library, opened read-only. Import it to make changes.'
                  }
                >
                  {lib().kind === 'magnitide' ? 'Magnitide' : 'calibre · read-only'}
                </span>
              </div>
              <div class="truncate text-xs text-muted" title={lib().path}>
                {lib().path}
              </div>
              <div class="mt-0.5 text-xs text-faint">
                {lib().bookCount.toLocaleString()} books · loaded in {lib().loadMs} ms
              </div>
            </div>
            <Show when={lib().kind === 'magnitide'}>
              <MenuItem onSelect={() => void chooseAndAddBooks()}>Add Books…</MenuItem>
            </Show>
            <Show when={lib().kind === 'calibre'}>
              <MenuItem onSelect={() => void startImport()}>Import into a Magnitide Library…</MenuItem>
            </Show>
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
