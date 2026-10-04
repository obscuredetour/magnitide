import { createEffect, createSignal, on, Show } from 'solid-js';

import { coverUrl, type CoverSize } from '../api';

type Props = {
  id: number;
  title: string;
  authors: string;
  hasCover: boolean;
  size: CoverSize;
  /** Tiny rendering (e.g. table rows): the generated cover shows colour only. */
  compact?: boolean;
};

/**
 * A book cover filling its (positioned) parent, or a generated one when the
 * book has none or it fails to load.
 */
export function Cover(props: Props) {
  const [failed, setFailed] = createSignal(false);
  const [loaded, setLoaded] = createSignal(false);

  // The details panel reuses this component across books.
  createEffect(
    on(
      () => props.id,
      () => {
        setFailed(false);
        setLoaded(false);
      },
      { defer: true },
    ),
  );

  return (
    <Show
      when={props.hasCover && !failed()}
      fallback={
        <div
          class="absolute inset-0 flex flex-col justify-between px-[12%] py-[14%] text-white/95 cover-placeholder"
          style={{ '--hue': String((props.id * 47) % 360) }}
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
      <img
        class="absolute inset-0 size-full object-cover opacity-0 transition-opacity duration-200 data-loaded:opacity-100"
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
