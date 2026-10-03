import { createEffect, createSignal, on, Show } from 'solid-js';

import { coverUrl, type CoverSize } from '../api';

type Props = {
  id: number;
  title: string;
  authors: string;
  hasCover: boolean;
  size: CoverSize;
};

/** A book cover, or a generated one when the book has none (or it fails to load). */
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
        <div class="placeholder" style={{ '--hue': String((props.id * 47) % 360) }}>
          <div class="placeholder-title">{props.title}</div>
          <div class="placeholder-author">{props.authors}</div>
        </div>
      }
    >
      {/* Large covers can take a moment; the cached thumbnail stands in meanwhile. */}
      <Show when={props.size === 'full'}>
        <img class="cover-preview" src={coverUrl('thumb', props.id)} alt="" />
      </Show>
      <img
        src={coverUrl(props.size, props.id)}
        alt=""
        decoding="async"
        classList={{ loaded: loaded() }}
        onLoad={() => setLoaded(true)}
        onError={() => setFailed(true)}
      />
    </Show>
  );
}
