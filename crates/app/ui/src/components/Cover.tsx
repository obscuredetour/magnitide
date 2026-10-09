import { createEffect, createSignal, Show } from 'solid-js';

import { coverUrl, type CoverSize } from '../api';
import { placeholderHue } from '../coverColors';

type Props = {
  id: number;
  title: string;
  authors: string;
  hasCover: boolean;
  size: CoverSize;
  /** Tiny rendering (e.g. table rows): the generated cover shows colour only. */
  compact?: boolean;
  /**
   * `cover` (the default) fills the positioned parent, cropping to it.
   * `contain` keeps the cover's own proportions, at most --cover-h tall and
   * --cover-w wide (set by the parent), in a box that hugs it; the generated
   * cover, and the ghost before the image loads, are 2:3 of --cover-h.
   */
  fit?: 'cover' | 'contain';
};

/**
 * A book cover filling its (positioned) parent, or at its own proportions
 * (`fit`), or a generated one when the book has none or it fails to load.
 */
export function Cover(props: Props) {
  const [failed, setFailed] = createSignal(false);
  const [loaded, setLoaded] = createSignal(false);

  // The details panel reuses this component across books. Only a different
  // book resets it: the same book's details reloading (after an edit) keeps
  // the same image, which won't fire `load` again. (Tracked by hand: `on`
  // with `defer` doesn't record the first id, so it can't tell.)
  let shown = props.id;
  createEffect(() => {
    const id = props.id;
    if (id === shown) return;
    shown = id;
    setFailed(false);
    setLoaded(false);
  });

  return (
    <Show
      when={props.hasCover && !failed()}
      fallback={
        <div
          class="flex flex-col justify-between px-[12%] py-[14%] text-white/95 cover-placeholder"
          classList={{
            'absolute inset-0': props.fit !== 'contain',
            'aspect-[2/3] h-(--cover-h)': props.fit === 'contain',
          }}
          style={{ '--hue': String(placeholderHue(props.id)) }}
        >
          <Show when={!props.compact}>
            <div
              class="line-clamp-5 font-serif leading-[1.2] font-semibold [overflow-wrap:anywhere]"
              classList={{ 'text-[15px]': props.size === 'thumb', 'text-xl': props.size === 'full' }}
            >
              {props.title}
            </div>
            <div class="text-[11px] opacity-80">{props.authors}</div>
          </Show>
        </div>
      }
    >
      {/* Large covers can take a moment; the cached thumbnail stands in meanwhile. */}
      <Show when={props.size === 'full'}>
        <img class="absolute inset-0 size-full object-cover blur-[2px]" src={coverUrl('thumb', props.id)} alt="" />
      </Show>
      {/* Contained: a 2:3 ghost until the image has a size of its own (aspect-ratio: auto 2/3), then its own. */}
      <img
        class="opacity-0 transition-opacity duration-200 data-loaded:opacity-100"
        classList={{
          'absolute inset-0 size-full object-cover': props.fit !== 'contain',
          'block aspect-[auto_2/3] h-(--cover-h) max-h-(--cover-h) w-auto max-w-(--cover-w) object-contain data-loaded:h-auto':
            props.fit === 'contain',
        }}
        src={coverUrl(props.size, props.id)}
        alt=""
        decoding="async"
        data-loaded={loaded() || undefined}
        onLoad={() => setLoaded(true)}
        onError={() => setFailed(true)}
      />
    </Show>
  );
}
