import { createResource, createSignal, For, Show } from 'solid-js';
import X from 'lucide-solid/icons/x';

import { api, type BookDetails, errorMessage, fileManagerName, type MatchedBy } from '../api';
import { openEditor } from '../editor';
import { editRevision, library, openBook, search, setSelectedId } from '../library';
import { toast } from '../toast';
import { devices, onDevice } from '../devices';
import { removeFormat, removeFromDevice, sendToDevice } from '../transfer';
import { writeBookFiles } from '../writeback';
import { ConvertMenu } from './ConvertMenu';
import { Cover } from './Cover';
import { Icon } from './Icon';

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

const MATCHED_BY: Record<MatchedBy, string> = {
  uuid: 'its calibre id',
  isbn: 'ISBN',
  file: 'an identical file',
  title: 'title and author',
};

// Shared styles for repeated elements.
const chips = 'my-4 flex flex-wrap gap-1.5';
const chip = 'rounded-chip bg-element px-2.5 py-[3px] text-xs text-fg';
const chipButton = `${chip} cursor-pointer hover:bg-highlight hover:text-accent`;
const sectionLabel = 'mt-[22px] mb-2 text-[11px] font-semibold tracking-[.06em] text-muted uppercase';

export function DetailsPanel(props: Props) {
  let scroller!: HTMLElement;
  // Keyed on id, so switching books never shows a stale response.
  // Refetched after an edit, too (editRevision).
  const [book] = createResource(
    () => [props.id, editRevision()] as const,
    async ([id]) => {
      const b = await api.bookDetails(id);
      scroller.scrollTop = 0;
      return b;
    },
  );

  return (
    <aside
      class="relative w-[380px] flex-none animate-slide-in overflow-y-auto border-l border-line bg-panel select-text max-[760px]:absolute max-[760px]:inset-y-0 max-[760px]:right-0 max-[760px]:w-[min(380px,100%)] max-[760px]:shadow-lift"
      ref={scroller}
      aria-label="Book details"
    >
      <div class="sticky top-2.5 z-1 float-right mt-2.5 mr-2.5 flex items-center gap-1.5">
        <Show when={library()?.kind === 'magnitide'}>
          <button
            class="h-7 cursor-pointer rounded-chip bg-element px-3 text-xs font-medium hover:bg-hover"
            title="Edit (⌘E)"
            onClick={() => void openEditor(props.id)}
          >
            Edit
          </button>
        </Show>
        <button
          class="grid size-7 place-items-center rounded-full bg-element hover:bg-hover"
          title="Close (Esc)"
          onClick={() => props.onClose()}
        >
          <Icon of={X} class="text-muted" />
        </button>
      </div>
      <Show when={book()}>{(b) => <Details book={b()} />}</Show>
    </aside>
  );
}

function Details(props: { book: BookDetails }) {
  const b = () => props.book;
  // Reads the book's files, so it's fetched separately from the details.
  const [fileStatus] = createResource(
    () => [b().id, editRevision()] as const,
    ([id]) => api.fileMetadataStatus(id).catch(() => null),
  );
  const [writing, setWriting] = createSignal(false);
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
      <div class="relative mx-auto mb-6 aspect-[2/3] w-[62%] overflow-hidden rounded-lg bg-element shadow-lift">
        <Cover id={b().id} title={b().title} authors={b().authors.join(' & ')} hasCover={b().hasCover} size="full" />
      </div>

      <h2 class="mb-1.5 font-serif text-[22px]/[1.25] font-semibold text-balance">{b().title}</h2>
      <p class="mb-1 text-sm">
        <For each={b().authors}>
          {(a, i) => (
            <>
              {i() > 0 && ' & '}
              <button class="cursor-pointer text-link hover:underline" onClick={() => searchFor('author', a)}>
                {a}
              </button>
            </>
          )}
        </For>
      </p>
      <Show when={b().series.length}>
        <div class="mb-2.5 text-muted">
          <For each={b().series}>{(s) => <p>{s}</p>}</For>
        </div>
      </Show>
      <Show when={b().collectedIn.length}>
        <p class="-mt-1 mb-2.5 text-xs text-muted">
          Part of{' '}
          <For each={b().collectedIn}>
            {(o, i) => (
              <>
                {i() > 0 && ', '}
                <button class="cursor-pointer text-link hover:underline" onClick={() => setSelectedId(o.id)}>
                  {o.title}
                </button>
              </>
            )}
          </For>
        </p>
      </Show>
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

      <Show when={b().collects.length}>
        <div class={sectionLabel}>Collects</div>
        <ol class="mb-4 list-inside list-decimal space-y-0.5 text-sm">
          <For each={b().collects}>
            {(c) => (
              <li>
                <button class="cursor-pointer text-link hover:underline" onClick={() => setSelectedId(c.id)}>
                  {c.title}
                </button>
              </li>
            )}
          </For>
        </ol>
      </Show>

      <Show when={b().formats.length}>
        <div class={sectionLabel}>Formats</div>
        <div class={chips}>
          <For each={b().formats}>
            {(f) => (
              <span class="group/format relative inline-flex">
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
                <Show when={library()?.kind === 'magnitide'}>
                  <button
                    class="absolute -top-1.5 -right-1.5 grid size-[18px] cursor-pointer place-items-center rounded-full bg-popover text-xs leading-none text-muted opacity-0 shadow-sm ring-1 ring-line group-hover/format:opacity-100 hover:bg-danger hover:text-on-status hover:ring-danger focus-visible:opacity-100"
                    title={`Remove the ${f.format}`}
                    aria-label={`Remove the ${f.format}`}
                    onClick={() => void removeFormat(b(), f.format)}
                  >
                    <Icon of={X} size={12} />
                  </button>
                </Show>
              </span>
            )}
          </For>
          <Show when={library()?.kind === 'magnitide'}>
            <ConvertMenu ids={[b().id]} buttonClass={`${chip} cursor-pointer text-accent hover:bg-highlight`} />
          </Show>
        </div>
      </Show>

      <Show when={devices().length}>
        <div class="-mt-1.5 mb-4 space-y-1 text-xs">
          <For each={devices()}>
            {(d) => {
              const hit = () =>
                onDevice()
                  .get(b().id)
                  ?.find((o) => o.device.id === d.id)?.hit;
              return (
                <Show
                  when={hit()}
                  fallback={
                    <button class={chipButton} onClick={() => void sendToDevice(d, [b().id])}>
                      Send to {d.name}
                    </button>
                  }
                >
                  {(h) => (
                    <p
                      class="flex items-center gap-1.5 text-muted"
                      title={`${h().path}\nFound by ${MATCHED_BY[h().matchedBy]}`}
                    >
                      <span class="font-medium whitespace-nowrap text-success">On {d.name}</span>
                      <span class="min-w-0 truncate">{h().path}</span>
                      <button
                        class="ml-auto cursor-pointer whitespace-nowrap text-link hover:underline"
                        onClick={() => void removeFromDevice(d, [b().id])}
                      >
                        Remove
                      </button>
                    </p>
                  )}
                </Show>
              );
            }}
          </For>
        </div>
      </Show>

      <Show when={fileStatus() === 'outdated'}>
        <p class="-mt-1.5 mb-4 text-xs text-muted">
          The EPUB’s built-in metadata differs from the library.{' '}
          <button
            class="cursor-pointer text-link hover:underline disabled:opacity-60"
            disabled={writing()}
            title="Write the library’s metadata and cover into the EPUB, for e-readers and other apps"
            onClick={async () => {
              setWriting(true);
              await writeBookFiles(b().id);
              setWriting(false);
            }}
          >
            {writing() ? 'Writing…' : 'Write to file'}
          </button>
        </p>
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
          class="cursor-pointer text-xs text-link hover:underline"
          onClick={() => api.revealBook(b().id).catch((err) => toast(errorMessage(err), { error: true }))}
        >
          Show in {fileManagerName}
        </button>
        <p class="mt-1 text-[11px] [overflow-wrap:anywhere] text-faint">{b().folder}</p>
      </div>
    </div>
  );
}
