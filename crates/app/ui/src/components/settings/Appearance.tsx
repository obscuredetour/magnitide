import { createSignal, For } from 'solid-js';

import { keys } from '../../keys';
import { setSeriesBadges, setToolbarLayout, setViewMode, seriesBadges, toolbarLayout, viewMode } from '../../view';
import { resetZoom, tryZoom, ZOOM_STEPS, zoom, zoomLabel } from '../../zoom';
import { CoverSizeSlider } from '../CoverSizeSlider';
import { button, Group, Heading, Row, Segmented, Toggle } from './parts';

/**
 * Interface size: a slider over the zoom steps (by index, so each notch is one
 * step). Dragging only previews the number; letting go tries the size, which
 * then asks to be kept (see tryZoom), so the window doesn't rescale under the
 * pointer mid-drag.
 */
function InterfaceSize() {
  const [dragging, setDragging] = createSignal<number | null>(null);
  const shown = () => dragging() ?? zoom();
  const index = () => ZOOM_STEPS.indexOf(zoom() as (typeof ZOOM_STEPS)[number]);
  return (
    <Row
      label="Interface size"
      description={`Everything in the window, text included. Also ${keys('mod++')}, ${keys('mod+-')} and ${keys('mod+0')}.`}
    >
      <input
        type="range"
        class="w-32 accent-accent"
        aria-label="Interface size"
        aria-valuetext={zoomLabel(shown())}
        min={0}
        max={ZOOM_STEPS.length - 1}
        step={1}
        list="zoom-steps"
        value={index()}
        onInput={(e) => setDragging(ZOOM_STEPS[Number(e.currentTarget.value)] ?? null)}
        onChange={(e) => {
          const z = ZOOM_STEPS[Number(e.currentTarget.value)] ?? 1;
          setDragging(null);
          const input = e.currentTarget;
          // Back to the size in use if it isn't kept.
          void tryZoom(z).then(() => (input.value = String(index())));
        }}
      />
      <datalist id="zoom-steps">
        <For each={ZOOM_STEPS}>{(_, i) => <option value={i()} />}</For>
      </datalist>
      <span class="w-10 text-right text-xs text-muted tabular-nums">{zoomLabel(shown())}</span>
      <button class={button} disabled={zoom() === 1} onClick={resetZoom}>
        Reset
      </button>
    </Row>
  );
}

export function Appearance() {
  return (
    <>
      <Heading title="Appearance">How the window is laid out. Colours are in Themes.</Heading>

      <Group title="Window">
        <InterfaceSize />
        <Row label="Toolbar" description="Two rows, or everything in one to leave more room for books.">
          <Segmented
            label="Toolbar"
            value={toolbarLayout()}
            options={[
              ['two-rows', 'Two Rows'],
              ['one-row', 'One Row'],
            ]}
            onChange={setToolbarLayout}
          />
        </Row>
      </Group>

      <Group title="Books">
        <Row label="View" description={`Covers or a list. Also ${keys('mod+1')} and ${keys('mod+2')}.`}>
          <Segmented
            label="View"
            value={viewMode()}
            options={[
              ['grid', 'Covers'],
              ['table', 'List'],
            ]}
            onChange={setViewMode}
          />
        </Row>
        <Row label="Cover size" description="How large covers are in the cover view.">
          <CoverSizeSlider />
        </Row>
        <Row label="Series labels" description="A label on each cover with the book’s place in its series.">
          <Toggle label="Series labels on covers" checked={seriesBadges()} onChange={setSeriesBadges} />
        </Row>
      </Group>
    </>
  );
}
