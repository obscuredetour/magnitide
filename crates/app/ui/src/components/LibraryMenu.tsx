import { For, Show } from 'solid-js';
import ChevronDown from 'lucide-solid/icons/chevron-down';

import { api, errorMessage, fileManagerName, type LibraryKind } from '../api';
import { chooseLibrary, createLibrary, library, openLibrary, recentLibraries } from '../library';
import { startImport } from '../importer';
import { chooseAndAddBooks } from '../adder';
import { toast } from '../toast';
import { Menu, MenuItem, MenuLabel, MenuSeparator } from './Menu';
import { Icon } from './Icon';

/** What kind of library it is: Magnitide's own, or a calibre one (read-only). */
function KindBadge(props: { kind: LibraryKind; class?: string }) {
  return (
    <span
      class={`flex-none rounded-chip px-2 py-px text-[11px] font-medium ${props.class ?? ''}`}
      classList={{
        'bg-accent/15 text-accent': props.kind === 'magnitide',
        'bg-element text-muted': props.kind === 'calibre',
      }}
      title={
        props.kind === 'magnitide'
          ? 'A Magnitide library'
          : 'A calibre library, opened read-only. Import it to make changes.'
      }
    >
      {props.kind === 'magnitide' ? 'Magnitide' : 'calibre · read-only'}
    </span>
  );
}

export function LibraryMenu() {
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
      class="-ml-2 min-w-0"
      buttonClass="flex max-w-full min-w-0 items-baseline gap-2 rounded-control px-2 py-[5px] hover:bg-hover aria-expanded:bg-hover"
      panelClass="w-80"
      button={
        <>
          <span class="truncate font-sans text-[15px] font-semibold">{library()?.name ?? 'Magnitide'}</span>
          <Icon of={ChevronDown} size={12} class="self-center text-muted" />
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
                <KindBadge kind={lib().kind} />
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
              <Show when={r.kind}>{(kind) => <KindBadge kind={kind()} class="ml-auto" />}</Show>
            </MenuItem>
          )}
        </For>
        <MenuSeparator />
      </Show>
      <MenuItem onSelect={() => void chooseLibrary()}>Open Another Library…</MenuItem>
      <MenuItem onSelect={() => void createLibrary()}>New Library…</MenuItem>
    </Menu>
  );
}
