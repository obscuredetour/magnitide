// GUI calibre plugins' menus, as calibre shows them on its toolbar: one
// submenu per plugin. In the book menu (for the books it was opened on) and
// behind the toolbar's Plugins button (for the selection, even if empty).

import { For, onMount, Show } from 'solid-js';
import Puzzle from 'lucide-solid/icons/puzzle';

import type { PluginMenu, PluginMenuItem } from '../api';
import { items, library, query, selectedIds } from '../library';
import { guiPlugins, loadPluginMenus, pluginMenus, runPlugin } from '../plugins';
import { Icon } from './Icon';
import { Menu, MenuItem, MenuLabel, MenuSeparator, SubMenu } from './Menu';

/** Each plugin's menu, for the books `ids` gives when an item is chosen. Loads them (starting calibre) when first shown. */
export function PluginItems(props: { ids(): number[] }) {
  onMount(() => {
    if (!pluginMenus()) void loadPluginMenus();
  });
  const run = (menu: PluginMenu, item: PluginMenuItem | null, texts: string[]) =>
    void runPlugin(menu, item, texts, props.ids(), query());
  return (
    <Show
      when={pluginMenus()}
      fallback={
        <MenuItem disabled onSelect={() => {}}>
          Starting calibre…
        </MenuItem>
      }
    >
      {(m) => (
        <>
          <For each={m().errors}>
            {(e) => (
              <MenuItem disabled title={e} onSelect={() => {}}>
                <span class="max-w-72 truncate">{e}</span>
              </MenuItem>
            )}
          </For>
          <For each={m().menus}>
            {(menu) => (
              <Show
                when={menu.items.length}
                fallback={<MenuItem onSelect={() => run(menu, null, [])}>{menu.text}</MenuItem>}
              >
                <SubMenu label={menu.text}>
                  <Show when={menu.main}>
                    <MenuItem onSelect={() => run(menu, null, [])}>{menu.text}</MenuItem>
                    <MenuSeparator />
                  </Show>
                  <PluginMenuItems items={menu.items} texts={[]} run={(item, texts) => run(menu, item, texts)} />
                </SubMenu>
              </Show>
            )}
          </For>
          <Show when={!m().menus.length && !m().errors.length}>
            <MenuLabel>No plugin menus</MenuLabel>
          </Show>
        </>
      )}
    </Show>
  );
}

function PluginMenuItems(props: {
  items: PluginMenuItem[];
  texts: string[];
  run(item: PluginMenuItem, texts: string[]): void;
}) {
  return (
    <For each={props.items}>
      {(item) => (
        <Show when={!item.separator} fallback={<MenuSeparator />}>
          <Show
            when={item.children.length}
            fallback={
              <MenuItem
                disabled={!item.enabled}
                checked={item.checkable ? item.checked : undefined}
                checkbox={item.checkable}
                onSelect={() => props.run(item, [...props.texts, item.text])}
              >
                {item.text}
              </MenuItem>
            }
          >
            <SubMenu label={item.text} disabled={!item.enabled}>
              <PluginMenuItems items={item.children} texts={[...props.texts, item.text]} run={props.run} />
            </SubMenu>
          </Show>
        </Show>
      )}
    </For>
  );
}

/** The toolbar's Plugins button: shown when a GUI plugin is on and the library is a Magnitide one. */
export function PluginsMenu(props: { onManage(): void }) {
  // The selection in list order, read when an item is chosen.
  const selection = () => {
    const chosen = selectedIds();
    return items()
      .filter((b) => chosen.has(b.id))
      .map((b) => b.id);
  };
  const count = () => selectedIds().size;
  return (
    <Show when={guiPlugins().length && library()?.kind === 'magnitide'}>
      <Menu
        label="Plugins"
        iconOnly
        align="end"
        buttonClass="grid size-8 place-items-center rounded-control hover:bg-hover aria-expanded:bg-hover"
        button={<Icon of={Puzzle} size={16} class="text-muted" />}
      >
        <MenuLabel>Plugins · {count() ? `${count().toLocaleString()} selected` : 'nothing selected'}</MenuLabel>
        <PluginItems ids={selection} />
        <MenuSeparator />
        <MenuItem onSelect={() => props.onManage()}>Manage Plugins…</MenuItem>
      </Menu>
    </Show>
  );
}
