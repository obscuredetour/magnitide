import { createResource, For, Show } from 'solid-js';

import { api, type BookDetails, errorMessage, fileManagerName } from '../api';
import { openBook, search } from '../library';
import { toast } from '../toast';
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

// Shared styles for repeated elements.
const chips = 'my-4 flex flex-wrap gap-1.5';
const chip = 'rounded-full bg-surface-2 px-2.5 py-[3px] text-xs text-fg';
const chipButton = `${chip} cursor-pointer hover:bg-accent/15 hover:text-accent`;
const sectionLabel = 'mt-[22px] mb-2 text-[11px] font-semibold tracking-[.06em] text-muted uppercase';

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
    <aside
      class="relative w-[380px] flex-none animate-slide-in overflow-y-auto border-l border-line bg-surface select-text max-[760px]:absolute max-[760px]:inset-y-0 max-[760px]:right-0 max-[760px]:w-[min(380px,100%)] max-[760px]:shadow-lift"
      ref={scroller}
      aria-label="Book details"
    >
      <button
        class="sticky top-2.5 z-1 float-right mt-2.5 mr-2.5 grid size-7 place-items-center rounded-full bg-surface-2 hover:bg-line"
        title="Close (Esc)"
        onClick={() => props.onClose()}
      >
        <svg class="size-3 stroke-muted stroke-2 [stroke-linecap:round]" viewBox="0 0 20 20" aria-hidden="true">
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
    <div class="px-7 pt-7 pb-10">
      <div class="relative mx-auto mb-6 aspect-[2/3] w-[62%] overflow-hidden rounded-lg bg-surface-2 shadow-lift">
        <Cover id={b().id} title={b().title} authors={b().authors.join(' & ')} hasCover={b().hasCover} size="full" />
      </div>

      <h2 class="mb-1.5 font-serif text-[22px]/[1.25] font-semibold text-balance">{b().title}</h2>
      <p class="mb-1 text-sm">
        <For each={b().authors}>
          {(a, i) => (
            <>
              {i() > 0 && ' & '}
              <button class="cursor-pointer text-accent hover:underline" onClick={() => searchFor('author', a)}>
                {a}
              </button>
            </>
          )}
        </For>
      </p>
      <Show when={b().series}>{(s) => <p class="mb-2.5 text-muted">{s()}</p>}</Show>
      <Show when={stars()}>
        {(s) => (
          <div class="text-sm tracking-[1px] text-star" title={`${b().stars} of 5`}>
            {s()}
          </div>
        )}
      </Show>

      <dl class="my-[18px] grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5">
        <For each={meta().filter(([, v]) => v)}>
          {([label, value]) => (
            <>
              <dt class="text-muted">{label}</dt>
              <dd class="[overflow-wrap:anywhere]">{value}</dd>
            </>
          )}
        </For>
      </dl>

      <Show when={b().tags.length}>
        <div class={chips}>
          <For each={b().tags}>
            {(t) => (
              <button class={chipButton} onClick={() => searchFor('tag', t)}>
                {t}
              </button>
            )}
          </For>
        </div>
      </Show>

      <Show when={b().formats.length}>
        <div class={sectionLabel}>Formats</div>
        <div class={chips}>
          <For each={b().formats}>
            {(f) => (
              <Show
                when={f.present}
                fallback={
                  <span class={`${chip} text-danger line-through`} title="File missing from the library folder">
                    {f.format} · {formatSize(f.size)}
                  </span>
                }
              >
                <button
                  class={chipButton}
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
            <div class={sectionLabel}>Description</div>
            {/* Sanitized in Rust (ammonia). Links never navigate the app window. */}
            <div
              class="comments-html font-serif text-sm/[1.6]"
              innerHTML={html()}
              onClick={(e) => {
                if ((e.target as Element).closest('a')) e.preventDefault();
              }}
            />
          </>
        )}
      </Show>

      <div class="mt-6">
        <button
          class="cursor-pointer text-xs text-accent hover:underline"
          onClick={() => api.revealBook(b().id).catch((err) => toast(errorMessage(err), { error: true }))}
        >
          Show in {fileManagerName}
        </button>
        <p class="mt-1 text-[11px] [overflow-wrap:anywhere] text-faint">{b().folder}</p>
      </div>
    </div>
  );
}
