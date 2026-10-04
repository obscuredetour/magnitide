import { createResource, For, Show } from 'solid-js';

import { api } from '../api';
import { convertBooks } from '../transfer';
import { Menu, MenuItem, MenuLabel } from './Menu';

/** "Convert…": the formats these books can be converted to. */
export function ConvertMenu(props: { ids: number[]; buttonClass: string; align?: 'start' | 'end'; above?: boolean }) {
  // Asked when opened, so a newly installed calibre shows up.
  const [targets] = createResource(
    () => props.ids,
    (ids) => api.convertTargets(ids).catch(() => [] as string[]),
  );
  const [info] = createResource(() => api.conversionInfo().catch(() => null));
  return (
    <Menu
      label="Convert"
      title="Convert to another format"
      align={props.align}
      buttonClass={props.buttonClass}
      panelClass={props.above ? 'w-60 top-auto! bottom-full mb-1.5' : 'w-60'}
      button={<>Convert…</>}
    >
      <MenuLabel>Convert to</MenuLabel>
      <For each={targets()} fallback={<p class="px-2.5 py-1.5 text-xs text-muted">Nothing to convert to.</p>}>
        {(f) => <MenuItem onSelect={() => void convertBooks(props.ids, f)}>{f}</MenuItem>}
      </For>
      <Show when={info() && !info()!.calibre}>
        <p class="px-2.5 pt-1 pb-1.5 text-[11px] text-faint">
          EPUB → KEPUB and AZW3 are built in. Install calibre for MOBI, PDF and more.
        </p>
      </Show>
    </Menu>
  );
}
