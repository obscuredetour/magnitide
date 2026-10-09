import { createMemo, createSignal, For, Show } from 'solid-js';
import Barcode from 'lucide-solid/icons/barcode';
import Building from 'lucide-solid/icons/building';
import ChevronRight from 'lucide-solid/icons/chevron-right';
import FileText from 'lucide-solid/icons/file-text';
import Languages from 'lucide-solid/icons/languages';
import Layers from 'lucide-solid/icons/layers';
import PanelRightOpen from 'lucide-solid/icons/panel-right-open';
import Star from 'lucide-solid/icons/star';
import Tag from 'lucide-solid/icons/tag';
import UserRound from 'lucide-solid/icons/user-round';

import type { FacetValue } from '../api';
import {
  anyQuery,
  CATEGORIES,
  type Category,
  expanded,
  type FacetKey,
  facets,
  isActive,
  noneQuery,
  openBrowsePanel,
  toggleExpanded,
  toggleSearch,
  valueQuery,
} from '../browse';
import { Icon } from './Icon';
import { ContextMenu, MenuItem, MenuSeparator } from './Menu';

/** The icon for each category. */
export const CATEGORY_ICONS = {
  authors: UserRound,
  series: Layers,
  tags: Tag,
  publishers: Building,
  formats: FileText,
  languages: Languages,
  ratings: Star,
  identifiers: Barcode,
} as const satisfies Record<FacetKey, unknown>;

/** Values shown under an open category before "All …"; more go to the panel. */
const SHOWN = 12;

const valueRow =
  'flex h-6 w-full cursor-default items-center gap-2 rounded-control pr-2 pl-8 text-left text-[12.5px] outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-focus aria-[current=true]:bg-highlight aria-[current=true]:text-accent';

/** The library's values by kind, each a filter: the sidebar's Browse mode. */
export function Browse() {
  const [menu, setMenu] = createSignal<{ x: number; y: number; c: Category } | null>(null);
  return (
    <div class="mt-1">
      <For each={CATEGORIES}>
        {(c) => (
          <CategoryRow
            c={c}
            values={facets()?.[c.key] ?? []}
            onMenu={(e) => {
              e.preventDefault();
              setMenu({ x: e.clientX, y: e.clientY, c });
            }}
          />
        )}
      </For>
      <Show when={menu()}>
        {(m) => (
          <ContextMenu label={m().c.label} x={m().x} y={m().y} onClose={() => setMenu(null)}>
            <MenuItem onSelect={() => toggleSearch(anyQuery(m().c))}>Books with {m().c.label.toLowerCase()}</MenuItem>
            <MenuItem onSelect={() => toggleSearch(noneQuery(m().c))}>
              Books without {m().c.label.toLowerCase()}
            </MenuItem>
            <MenuSeparator />
            <MenuItem onSelect={() => openBrowsePanel(m().c.key)}>Open in Panel</MenuItem>
          </ContextMenu>
        )}
      </Show>
    </div>
  );
}

function CategoryRow(props: { c: Category; values: FacetValue[]; onMenu(e: MouseEvent): void }) {
  const open = () => expanded().has(props.c.key);
  const [filter, setFilter] = createSignal('');
  const total = createMemo(() => props.values.reduce((n, v) => n + v.count, 0));
  const shown = createMemo(() => {
    const f = filter().trim().toLowerCase();
    const list = f ? props.values.filter((v) => props.c.show(v.value).toLowerCase().includes(f)) : props.values;
    return { list: list.slice(0, SHOWN), more: list.length - Math.min(list.length, SHOWN) };
  });
  const Ic = CATEGORY_ICONS[props.c.key];
  return (
    <div>
      <div
        class="group flex h-7 items-center gap-1 rounded-control pr-1 pl-1 text-sm hover:bg-hover"
        onContextMenu={(e) => props.onMenu(e)}
      >
        <button
          class="flex min-w-0 flex-1 cursor-default items-center gap-2 text-left outline-none"
          aria-expanded={open()}
          onClick={() => toggleExpanded(props.c.key)}
        >
          <span class="flex flex-none text-faint transition-transform duration-150" classList={{ 'rotate-90': open() }}>
            <Icon of={ChevronRight} size={12} />
          </span>
          <Icon of={Ic} class="flex-none text-muted" />
          <span class="min-w-0 flex-1 truncate">{props.c.label}</span>
        </button>
        <button
          class="hidden size-5 flex-none cursor-pointer place-items-center rounded-inset text-muted group-hover:grid hover:bg-element hover:text-fg"
          title={`Open ${props.c.label.toLowerCase()} in a panel`}
          aria-label={`Open ${props.c.label.toLowerCase()} in a panel`}
          onClick={() => openBrowsePanel(props.c.key)}
        >
          <Icon of={PanelRightOpen} size={12} />
        </button>
        {/* The count filters to books that have any; again to clear. */}
        <button
          class="min-w-6 cursor-default rounded-inset px-1 text-right text-xs text-faint tabular-nums hover:bg-element hover:text-fg aria-[current=true]:bg-highlight aria-[current=true]:text-accent"
          title={`Books with ${props.c.label.toLowerCase()}`}
          aria-current={isActive(anyQuery(props.c))}
          onClick={() => toggleSearch(anyQuery(props.c))}
        >
          {props.values.length.toLocaleString()}
        </button>
      </div>
      <Show when={open()}>
        <Show when={props.values.length > SHOWN}>
          <input
            class="mt-0.5 mb-0.5 ml-8 h-6 w-[calc(100%-2rem)] rounded-control border border-line bg-field px-2 text-xs outline-none select-text focus:border-focus"
            placeholder={`Filter ${props.values.length.toLocaleString()} ${props.c.label.toLowerCase()}`}
            aria-label={`Filter ${props.c.label.toLowerCase()}`}
            value={filter()}
            onInput={(e) => setFilter(e.currentTarget.value)}
            onKeyDown={(e) => e.stopPropagation()}
          />
        </Show>
        <For each={shown().list}>
          {(v) => (
            <button
              class={valueRow}
              aria-current={isActive(valueQuery(props.c, v.value))}
              title={`${props.c.show(v.value)} · ${v.count.toLocaleString()} of ${total().toLocaleString()}`}
              onClick={() => toggleSearch(valueQuery(props.c, v.value))}
            >
              <span class="min-w-0 flex-1 truncate">{props.c.show(v.value)}</span>
              <span class="text-[11px] text-faint tabular-nums">{v.count.toLocaleString()}</span>
            </button>
          )}
        </For>
        <Show when={shown().more > 0}>
          <button class={`${valueRow} text-muted`} onClick={() => openBrowsePanel(props.c.key)}>
            <span class="flex-1 truncate">All {props.values.length.toLocaleString()}…</span>
          </button>
        </Show>
        <Show when={!props.values.length}>
          <p class="pb-1 pl-8 text-xs text-faint">None yet.</p>
        </Show>
      </Show>
    </div>
  );
}
