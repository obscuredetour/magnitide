import { createResource, For, Show } from 'solid-js';

import { api, type BookDetails } from '../api';
import { openBook, search } from '../library';
import { Cover } from './Cover';

type Props = {
  id: number;
  onClose(): void;
};

const languageNames = (() => {
  try {
    return new Intl.DisplayNames([navigator.language, 'en'], { type: 'language' });
  } catch {
    return null;
  }
})();

function languageName(code: string): string {
  try {
    return languageNames?.of(code) ?? code;
  } catch {
    return code;
  }
}

function formatSize(bytes: number): string {
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${Math.round(bytes / 1e3)} KB`;
  return `${bytes} B`;
}

const searchFor = (field: string, value: string) => search(`${field}:"${value}"`);

export function DetailsPanel(props: Props) {
  let scroller!: HTMLElement;
  // Keyed on id, so switching books never shows a stale response.
  const [book] = createResource(
    () => props.id,
    async (id) => {
      const b = await api.bookDetails(id);
      scroller.scrollTop = 0;
      return b;
    },
  );

  return (
    <aside class="details" ref={scroller} aria-label="Book details">
      <button class="details-close" title="Close (Esc)" onClick={() => props.onClose()}>
        <svg viewBox="0 0 20 20" aria-hidden="true">
          <path d="m5 5 10 10M15 5 5 15" />
        </svg>
      </button>
      <Show when={book()}>{(b) => <Details book={b()} />}</Show>
    </aside>
  );
}

function Details(props: { book: BookDetails }) {
  const b = () => props.book;
  const stars = () => {
    const s = b().stars;
    return s ? '★'.repeat(Math.floor(s)) + (s % 1 >= 0.5 ? '½' : '') : null;
  };
  const meta = (): [string, string | null][] => [
    ['Published', b().published],
    ['Publisher', b().publisher],
    ['Added', b().added],
    ['Language', b().languages.map(languageName).join(', ') || null],
    ...b().identifiers.map(([k, v]): [string, string] => [k.toUpperCase(), v]),
  ];

  return (
    <div class="details-body">
      <div class="details-cover">
        <Cover id={b().id} title={b().title} authors={b().authors.join(' & ')} hasCover={b().hasCover} size="full" />
      </div>

      <h2>{b().title}</h2>
      <p class="byline">
        <For each={b().authors}>
          {(a, i) => (
            <>
              {i() > 0 && ' & '}
              <button class="link" onClick={() => searchFor('author', a)}>
                {a}
              </button>
            </>
          )}
        </For>
      </p>
      <Show when={b().series}>{(s) => <p class="series-line">{s()}</p>}</Show>
      <Show when={stars()}>
        {(s) => (
          <div class="stars" title={`${b().stars} of 5`}>
            {s()}
          </div>
        )}
      </Show>

      <dl class="meta">
        <For each={meta().filter(([, v]) => v)}>
          {([label, value]) => (
            <>
              <dt>{label}</dt>
              <dd>{value}</dd>
            </>
          )}
        </For>
      </dl>

      <Show when={b().tags.length}>
        <div class="chips">
          <For each={b().tags}>
            {(t) => (
              <button class="chip" onClick={() => searchFor('tag', t)}>
                {t}
              </button>
            )}
          </For>
        </div>
      </Show>

      <Show when={b().formats.length}>
        <div class="section-label">Formats</div>
        <div class="chips">
          <For each={b().formats}>
            {(f) => (
              <Show
                when={f.present}
                fallback={
                  <span class="chip static missing" title="File missing from the library folder">
                    {f.format} · {formatSize(f.size)}
                  </span>
                }
              >
                <button
                  class="chip"
                  title={`Open the ${f.format} in its default app`}
                  onClick={() => openBook(b().id, f.format)}
                >
                  {f.format} · {formatSize(f.size)}
                </button>
              </Show>
            )}
          </For>
        </div>
      </Show>

      <Show when={b().commentsHtml}>
        {(html) => (
          <>
            <div class="section-label">Description</div>
            {/* Sanitized in Rust (ammonia). Links never navigate the app window. */}
            <div
              class="comments"
              innerHTML={html()}
              onClick={(e) => {
                if ((e.target as Element).closest('a')) e.preventDefault();
              }}
            />
          </>
        )}
      </Show>

      <p class="folder">{b().folder}</p>
    </div>
  );
}
