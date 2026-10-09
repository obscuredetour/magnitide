import { createResource, createSignal, For, Match, Show, Switch } from 'solid-js';
import SquareLibrary from 'lucide-solid/icons/square-library';
import X from 'lucide-solid/icons/x';

import { api, type BookDetails, errorMessage, fileManagerName, type MatchedBy } from '../api';
import { openEditor } from '../editor';
import { DETAILS_WIDTH, detailsWidth, setDetailsWidth } from '../view';
import { editRevision, library, openBook, search, setSelectedId } from '../library';
import { formatCustom } from '../fields';
import { identifierOrder } from '../identifiers';
import { toast } from '../toast';
import { devices, onDevice } from '../devices';
import { addFromDevice, removeFormat, removeFromDevice, sendToDevice } from '../transfer';
import { writeBookFiles } from '../writeback';
import { ConvertMenu } from './ConvertMenu';
import { Cover } from './Cover';
import { Icon } from './Icon';
import { primaryButton, secondaryButton } from '../buttons';
import { widthFlags } from '../widths';
import { keys } from '../keys';

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

/** How a device file was recognized, for its tooltip. */
function foundBy(matchedBy: MatchedBy): string {
  switch (matchedBy) {
    // The open library's book id (books imported from calibre keep calibre's, so devices still know them).
    case 'uuid':
      return library()?.kind === 'magnitide' ? 'its Magnitide id' : 'its calibre id';
    case 'isbn':
      return 'ISBN';
    case 'file':
      return 'an identical file';
    case 'title':
      return 'title and author';
  }
}

// Shared styles for repeated elements.
const chips = 'flex flex-wrap gap-1.5';
const chip = 'rounded-chip bg-element px-2.5 py-[3px] text-xs text-fg';
const chipButton = `${chip} cursor-pointer hover:bg-highlight hover:text-accent`;
/** Buttons floating over the content (Edit, Close): see-through and blurred, so they stay readable over anything; solid on hover. */
const glass = 'cursor-pointer bg-element-hover/70 shadow-card ring-1 ring-line backdrop-blur-sm hover:bg-element';
const sectionLabel = 'text-[11px] font-semibold tracking-[.06em] text-muted uppercase';
/** A section below the title block: full width in the two-column layout (from 416px, see widths.ts), label then content. */
const section = 'space-y-2 group-data-wide/details:col-span-2';

export function DetailsPanel(props: Props) {
  let scroller!: HTMLDivElement;
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
      ref={(el) => widthFlags(el, { wide: 416 })}
      class="group/details relative flex-none animate-slide-in border-l border-line bg-panel select-text max-[760px]:absolute max-[760px]:inset-y-0 max-[760px]:right-0 max-[760px]:w-[min(28rem,100%_-_2rem)]! max-[760px]:max-w-full! max-[760px]:shadow-lift"
      // The list keeps at least 320px beside it.
      style={{ width: `${detailsWidth()}px`, 'max-width': 'calc(100% - 320px)' }}
      aria-label="Book details"
    >
      <ResizeHandle />
      <div class="h-full overflow-y-auto" ref={scroller}>
        <div class="absolute top-2.5 right-2.5 z-1 flex items-center gap-1.5">
          <Show when={library()?.kind === 'magnitide'}>
            <button
              class={`h-7 rounded-chip px-3 text-xs font-medium ${glass}`}
              title={`Edit (${keys('mod+E')})`}
              onClick={() => void openEditor(props.id)}
            >
              Edit
            </button>
          </Show>
          <button
            class={`grid size-7 place-items-center rounded-full ${glass}`}
            title="Close (Esc)"
            onClick={() => props.onClose()}
          >
            <Icon of={X} class="text-muted" />
          </button>
        </div>
        <Show when={book()}>{(b) => <Details book={b()} />}</Show>
      </div>
    </aside>
  );
}

/**
 * The panel's left edge: drag to resize, double-click for the default width,
 * or focus it and use ←/→ (Shift for bigger steps). Hidden while the panel
 * overlays a narrow window.
 */
function ResizeHandle() {
  const step = (e: KeyboardEvent) => (e.shiftKey ? 64 : 16);
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the details panel"
      aria-valuenow={detailsWidth()}
      aria-valuemin={DETAILS_WIDTH.min}
      aria-valuemax={DETAILS_WIDTH.max}
      tabIndex={0}
      title="Drag to resize · double-click to reset"
      class="group/resize absolute inset-y-0 -left-1 z-2 w-2 cursor-col-resize outline-none max-[760px]:hidden"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        const handle = e.currentTarget;
        handle.setPointerCapture(e.pointerId);
        handle.dataset.dragging = '';
        const startX = e.clientX;
        const startW = detailsWidth();
        // Dragging left widens the panel.
        const move = (ev: PointerEvent) => setDetailsWidth(startW + startX - ev.clientX, false);
        const end = () => {
          delete handle.dataset.dragging;
          setDetailsWidth(detailsWidth());
          handle.removeEventListener('pointermove', move);
          handle.removeEventListener('pointerup', end);
          handle.removeEventListener('pointercancel', end);
        };
        handle.addEventListener('pointermove', move);
        handle.addEventListener('pointerup', end);
        handle.addEventListener('pointercancel', end);
      }}
      onDblClick={() => setDetailsWidth(DETAILS_WIDTH.initial)}
      onKeyDown={(e) => {
        if (e.key === 'ArrowLeft') setDetailsWidth(detailsWidth() + step(e));
        else if (e.key === 'ArrowRight') setDetailsWidth(detailsWidth() - step(e));
        else if (e.key === 'Home') setDetailsWidth(DETAILS_WIDTH.min);
        else if (e.key === 'End') setDetailsWidth(DETAILS_WIDTH.max);
        else return;
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      {/* The visible line: on hover, while dragging, and when focused from the keyboard. */}
      <div class="mx-auto h-full w-0.5 bg-accent opacity-0 transition-opacity group-hover/resize:opacity-60 group-focus-visible/resize:opacity-100 group-data-dragging/resize:opacity-100" />
    </div>
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
  const editable = () => library()?.kind === 'magnitide';
  /** Connected devices this book is on, with where. */
  const onDevices = () => onDevice().get(b().id) ?? [];
  /** Connected devices it isn't on. */
  const notOn = () => devices().filter((d) => !onDevices().some((o) => o.device.id === d.id));
  const stars = () => {
    const s = b().stars;
    return s ? '★'.repeat(Math.floor(s)) + (s % 1 >= 0.5 ? '½' : '') : null;
  };
  const meta = (): [string, string | null][] => [
    ['Published', b().published],
    ['Publisher', b().publisher],
    ['Added', b().added],
    ['Language', b().languages.map(languageName).join(', ') || null],
    ...b()
      .identifiers.slice()
      .sort(identifierOrder)
      .map(([k, v]): [string, string] => [k.toUpperCase(), v]),
  ];

  /** The description, unless it's missing or only empty markup. */
  const description = () => {
    const html = b().commentsHtml;
    return html && html.replace(/<[^>]*>|&nbsp;|\s/g, '') ? html : null;
  };

  return (
    // A full-height column: the details, then the folder pinned to the bottom.
    <div class="flex min-h-full flex-col">
      {/* Sections are spaced by the container (gap), not their own margins, so
        the one- and two-column layouts space them alike and hidden ones leave no gap. */}
      <div class="flex flex-col gap-6 px-7 pt-7 group-data-wide/details:grid group-data-wide/details:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] group-data-wide/details:grid-rows-[repeat(5,auto)] group-data-wide/details:gap-x-7">
        <div class="relative aspect-2/3 w-[62%] overflow-hidden rounded-lg bg-element shadow-lift group-data-wide/details:w-full group-data-wide/details:min-w-32">
          <Cover id={b().id} title={b().title} authors={b().authors.join(' & ')} hasCover={b().hasCover} size="full" />
        </div>

        <div class="space-y-4.5">
          <div class="space-y-1.5">
            <h2 class="font-serif text-[22px]/[1.25] font-semibold text-balance">{b().title}</h2>
            <p class="text-sm">
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
              <div class="text-muted">
                <For each={b().series}>
                  {(s) => (
                    <p>
                      {s.omnibus ? 'Books' : 'Book'} {s.number} of{' '}
                      <button
                        class="cursor-pointer text-link hover:underline"
                        onClick={() => searchFor('series', s.name)}
                      >
                        {s.name}
                      </button>
                    </p>
                  )}
                </For>
              </div>
            </Show>
            <Show when={b().collectedIn.length}>
              <p class="text-xs text-muted">
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
          </div>
          <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5">
            <For each={meta().filter(([, v]) => v)}>
              {([label, value]) => (
                <>
                  <dt class="text-muted">{label}</dt>
                  <dd class="[overflow-wrap:anywhere]">{value}</dd>
                </>
              )}
            </For>
          </dl>
          {/* Shelves first (accent-tinted, since they're the user's own), then tags. */}
          <Show when={b().shelves.length}>
            <div class={chips}>
              <For each={b().shelves}>
                {(name) => (
                  <button
                    class={`${chip} inline-flex cursor-pointer items-center gap-1 bg-highlight! text-accent! hover:underline`}
                    title={`Show the shelf “${name}”`}
                    onClick={() => searchFor('shelf', name)}
                  >
                    <Icon of={SquareLibrary} size={12} />
                    {name}
                  </button>
                )}
              </For>
            </div>
          </Show>
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
        </div>

        <Show when={b().collects.length}>
          <section class={section}>
            <div class="flex items-center gap-2">
              <h3 class={sectionLabel}>Collects</h3>
              {/* <span class={`${chip} border border-element-hover bg-element! text-muted!`}>Omnibus</span> */}
            </div>
            <ol class="list-inside list-decimal space-y-0.5 text-sm">
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
          </section>
        </Show>

        {/* Files: formats, devices, and whether the EPUB's own metadata is out of date. */}
        <Show when={b().formats.length || devices().length || fileStatus() === 'outdated'}>
          <section class={`${section} space-y-3!`}>
            <Show when={b().formats.length}>
              <div class="space-y-2">
                <h3 class={sectionLabel}>Formats</h3>
                <div class={chips}>
                  <For each={b().formats}>
                    {(f) => (
                      <span class="group/format relative inline-flex">
                        <Show
                          when={f.present}
                          fallback={
                            <span
                              class={`${chip} text-danger line-through`}
                              title="File missing from the library folder"
                            >
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
                </div>
              </div>
            </Show>
            {/* Actions on the files: send to each device it isn't on (the first is the
              panel's main action), and convert. */}
            <Show when={b().formats.length && (notOn().length || editable())}>
              <div class="flex flex-wrap gap-2">
                <For each={notOn()}>
                  {(d, i) => (
                    <button
                      class={i() === 0 ? primaryButton : secondaryButton}
                      onClick={() => void sendToDevice(d, [b().id])}
                    >
                      Send to {d.name}
                    </button>
                  )}
                </For>
                <Show when={editable()}>
                  <ConvertMenu ids={[b().id]} buttonClass={secondaryButton} />
                </Show>
              </div>
            </Show>
            <Show when={onDevices().length}>
              <div class="space-y-1 text-xs">
                <For each={onDevices()}>
                  {(o) => (
                    <p
                      class="flex items-center gap-1.5 text-muted"
                      title={`${o.hit.path}\nFound by ${foundBy(o.hit.matchedBy)}`}
                    >
                      <span class="font-medium whitespace-nowrap text-success">On {o.device.name}</span>
                      <span class="min-w-0 truncate">{o.hit.path}</span>
                      <Show when={o.hit.newFormat && editable()}>
                        <button
                          class="ml-0.5 cursor-pointer whitespace-nowrap text-link hover:underline"
                          title={`Copy the ${o.hit.format} on ${o.device.name} into the library`}
                          onClick={() => void addFromDevice(o.device, [o.hit.path])}
                        >
                          Add {o.hit.format}
                        </button>
                      </Show>
                      <button
                        class="ml-0.5 cursor-pointer whitespace-nowrap text-link hover:underline"
                        onClick={() => void removeFromDevice(o.device, [b().id])}
                      >
                        Remove
                      </button>
                    </p>
                  )}
                </For>
              </div>
            </Show>
            <Show when={fileStatus() === 'outdated'}>
              <p class="rounded-control border border-line bg-bg/50 p-2 text-xs text-muted">
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
          </section>
        </Show>

        <Show when={b().custom.length}>
          <section class={section}>
            <h3 class={sectionLabel}>Custom fields</h3>
            <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5">
              <For each={b().custom}>
                {(c) => (
                  <>
                    <dt class="text-muted">{c.name}</dt>
                    <Switch fallback={<dd class="[overflow-wrap:anywhere]">{formatCustom(c.kind, c.value)}</dd>}>
                      <Match when={c.kind === 'rating'}>
                        <dd class="tracking-[1px] text-star" title={`${Number(c.value) / 2} of 5`}>
                          {'★'.repeat(Math.floor(Number(c.value) / 2))}
                          {Number(c.value) % 2 ? '½' : ''}
                        </dd>
                      </Match>
                      <Match when={c.kind === 'longText'}>
                        {/* Sanitized in Rust, like the description. */}
                        <dd class="comments-html font-serif" innerHTML={c.value} />
                      </Match>
                    </Switch>
                  </>
                )}
              </For>
            </dl>
          </section>
        </Show>

        <section class={section}>
          <h3 class={sectionLabel}>Description</h3>
          <Show when={description()} fallback={<p class="text-sm text-faint italic">No description available.</p>}>
            {(html) => (
              // Sanitized in Rust (ammonia). Links never navigate the app window.
              <div
                class="comments-html font-serif text-sm/[1.6]"
                innerHTML={html()}
                onClick={(e) => {
                  if ((e.target as Element).closest('a')) e.preventDefault();
                }}
              />
            )}
          </Show>
        </section>
      </div>

      {/* Where the book's files are: at the bottom of the panel however short the details. */}
      <div class="mt-auto px-7 pt-6 pb-7">
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
