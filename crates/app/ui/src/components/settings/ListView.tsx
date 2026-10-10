import { For } from 'solid-js';

import { keys } from '../../keys';
import {
  columnOrder,
  isColumnVisible,
  resetColumnOrder,
  resetColumnSizing,
  setColumnVisible,
  setZebraRows,
  zebraRows,
} from '../../view';
import { tableColumns } from '../BookTable';
import { button, Group, Heading, Row, Toggle } from './parts';

export function ListSettings() {
  // In display order: saved ones first, the rest in their default places.
  const ordered = () => {
    const order = columnOrder();
    const all = tableColumns();
    const known = order.filter((id) => all.some((c) => c.id === id));
    return [...known.map((id) => all.find((c) => c.id === id)!), ...all.filter((c) => !known.includes(c.id))];
  };
  return (
    <>
      <Heading title="List View">The list of books, when the toolbar shows it instead of covers.</Heading>

      <Group title="Columns" description="Also in the ⋯ menu at the end of the list’s header.">
        <div class="grid grid-cols-2 gap-x-6 gap-y-2.5 py-3.5 @[640px]:grid-cols-3">
          <For each={ordered()}>
            {(c) => (
              <label class="flex items-center gap-2" classList={{ 'opacity-60': c.locked }}>
                <input
                  type="checkbox"
                  class="size-4 accent-accent"
                  checked={isColumnVisible(c.id)}
                  disabled={c.locked}
                  onChange={(e) => setColumnVisible(c.id, e.currentTarget.checked)}
                />
                {c.label}
              </label>
            )}
          </For>
        </div>
      </Group>

      <Group>
        <Row label="Alternating row colours" description="Shade every other row, to make long lists easier to follow.">
          <Toggle label="Alternating row colours" checked={zebraRows()} onChange={setZebraRows} />
        </Row>
        <Row label="Column widths" description="Drag a column’s edge to resize it; double-click the edge to reset it.">
          <button class={button} onClick={() => resetColumnSizing()}>
            Reset All
          </button>
        </Row>
        <Row
          label="Column order"
          description={`Drag a column’s header sideways to move it, or focus it and press ${keys('alt+ArrowLeft')} or ${keys('alt+ArrowRight')}.`}
        >
          <button class={button} disabled={!columnOrder().length} onClick={() => resetColumnOrder()}>
            Reset
          </button>
        </Row>
      </Group>
    </>
  );
}
