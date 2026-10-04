import { createMemo, createResource, createSignal, For, type JSX, onMount, Show } from 'solid-js';
import { createStore } from 'solid-js/store';
import { Portal } from 'solid-js/web';
import Check from 'lucide-solid/icons/check';

import {
  api,
  coverUrl,
  errorMessage,
  type MatchKind,
  type MetadataMatch,
  remoteCoverUrl,
  type SearchResult,
  type SourceId,
  type SourceRecord,
} from '../api';
import { Icon } from './Icon';

/** The edit form's values, to compare against. */
export type Current = {
  title: string;
  authors: string[];
  publisher: string | null;
  published: string | null;
  languages: string[];
  tags: string[];
  identifiers: Record<string, string>;
  descriptionHtml: string | null;
  hasCover: boolean;
  isbn: string | null;
};

/** What the user chose to take; the edit form applies it. */
export type Applied = {
  title?: string;
  authors?: string[];
  publisher?: string;
  published?: string;
  languages?: string[];
  tags?: string[];
  identifiers?: Record<string, string>;
  descriptionHtml?: string;
  coverUrl?: string;
};

const SOURCE_NAME: Record<SourceId, string> = { openLibrary: 'Open Library', google: 'Google Books' };
const KIND_LABEL: Record<MatchKind, string> = {
  isbn: 'ISBN match',
  strong: 'Strong match',
  possible: 'Possible match',
};

type Value = string | string[] | Record<string, string>;

/** One row of the comparison. `from` reads a record; `text` is how a value displays and compares. */
type Field = {
  key: Exclude<keyof Applied, 'coverUrl'>;
  label: string;
  from(r: SourceRecord, c: Current): Value | null;
  current(c: Current): Value | null;
  text(v: Value): string;
};

const list = (v: Value) => (v as string[]).join(', ');
const nonEmpty = <T extends { length: number }>(v: T | null | undefined) => (v && v.length ? v : null);
const idsText = (v: Value) =>
  Object.entries(v as Record<string, string>)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([k, x]) => `${k}:${x}`)
    .join(', ');

const FIELDS: Field[] = [
  { key: 'title', label: 'Title', from: (r) => nonEmpty(r.title), current: (c) => nonEmpty(c.title), text: String },
  {
    key: 'authors',
    label: 'Authors',
    from: (r) => nonEmpty(r.authors),
    current: (c) => nonEmpty(c.authors),
    text: (v) => (v as string[]).join(' & '),
  },
  { key: 'publisher', label: 'Publisher', from: (r) => r.publisher, current: (c) => c.publisher, text: String },
  { key: 'published', label: 'Published', from: (r) => r.published, current: (c) => c.published, text: String },
  {
    key: 'languages',
    label: 'Languages',
    from: (r) => nonEmpty(r.languages),
    current: (c) => nonEmpty(c.languages),
    text: list,
  },
  { key: 'tags', label: 'Tags', from: (r) => nonEmpty(r.tags), current: (c) => nonEmpty(c.tags), text: list },
  {
    key: 'identifiers',
    label: 'Identifiers',
    // Merged into the book's own: the source's ids added, the rest kept.
    from: (r, c) => (Object.keys(r.identifiers).length ? { ...c.identifiers, ...r.identifiers } : null),
    current: (c) => (Object.keys(c.identifiers).length ? c.identifiers : null),
    text: idsText,
  },
  {
    key: 'descriptionHtml',
    label: 'Description',
    from: (r) => r.descriptionHtml,
    current: (c) => c.descriptionHtml,
    text: String,
  },
];

const same = (f: Field, a: Value | null, b: Value | null) =>
  a !== null && b !== null && f.text(a).trim().toLowerCase() === f.text(b).trim().toLowerCase();

const input =
  'h-8 min-w-0 rounded-md border border-line bg-field px-2.5 outline-none select-text focus:border-focus focus:ring-3 focus:ring-focus/20';
const smallButton =
  'cursor-pointer rounded-md border border-line bg-element px-3 py-1.5 text-sm font-medium hover:bg-hover disabled:cursor-default disabled:opacity-50';

/** A source's cover, downloaded through Rust and shown from the cache. */
function RemoteCover(props: { url: string | null; class?: string; withSize?: boolean }) {
  const [preview] = createResource(
    () => props.url,
    (url) => api.coverPreview(url).catch(() => null),
  );
  return (
    <div class={`relative overflow-hidden rounded bg-element ${props.class ?? ''}`}>
      <Show when={preview()}>
        {(p) => (
          <>
            <img src={remoteCoverUrl(p().key)} alt="" class="size-full object-cover" />
            <Show when={props.withSize}>
              <SizeBadge width={p().width} height={p().height} />
            </Show>
          </>
        )}
      </Show>
    </div>
  );
}

/** Pixel dimensions over a cover, so the two can be compared at a glance. */
function SizeBadge(props: { width: number; height: number }) {
  return (
    <span class="absolute right-1 bottom-1 rounded bg-black/65 px-1.5 py-px text-[11px] font-medium text-white tabular-nums">
      {props.width}×{props.height}
    </span>
  );
}

/** The book's own cover, full size, with its dimensions once loaded. */
function CurrentCover(props: { id: number }) {
  const [size, setSize] = createSignal<{ width: number; height: number } | null>(null);
  return (
    <div class="relative aspect-[2/3] w-28 overflow-hidden rounded bg-element">
      <img
        src={coverUrl('full', props.id)}
        alt="Current cover"
        class="size-full object-cover"
        onLoad={(e) => setSize({ width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight })}
      />
      <Show when={size()}>{(s) => <SizeBadge width={s().width} height={s().height} />}</Show>
    </div>
  );
}

/** Marks a found value that's identical to the book's. */
function SameBadge() {
  return (
    <span class="mt-px inline-flex flex-none items-center gap-1 rounded-full bg-success/20 px-2 py-0.5 text-xs font-semibold text-success ring-1 ring-success/30">
      <Icon of={Check} size={12} />
      Same
    </span>
  );
}

/**
 * Look a book up online and choose, field by field, what to take. Nothing
 * is saved here: chosen values go into the edit form (the cover applies
 * straight away, like any cover change).
 */
export function MatchDialog(props: { id: number; current: Current; onApply(applied: Applied): void; onClose(): void }) {
  let dialog!: HTMLDialogElement;
  const [title, setTitle] = createSignal(props.current.title);
  const [author, setAuthor] = createSignal(props.current.authors[0] ?? '');
  const [isbn, setIsbn] = createSignal(props.current.isbn ?? '');
  const [searching, setSearching] = createSignal(false);
  const [failed, setFailed] = createSignal<string | null>(null);
  const [result, setResult] = createStore<SearchResult>({ matches: [], errors: [] });
  const [selected, setSelected] = createSignal(0);
  // Only what the user changed; everything else follows the defaults.
  const [checked, setChecked] = createStore<Partial<Record<string, boolean>>>({});
  const [picked, setPicked] = createStore<Partial<Record<string, number>>>({});
  const [loadingDetails, setLoadingDetails] = createSignal(false);

  const match = (): MetadataMatch | undefined => result.matches[selected()];

  const search = async () => {
    setSearching(true);
    setFailed(null);
    try {
      const r = await api.findMetadata({
        title: title().trim(),
        authors: author().trim() ? [author().trim()] : [],
        isbn: isbn().trim() || null,
      });
      setResult(r);
      select(0);
    } catch (err) {
      setResult({ matches: [], errors: [] });
      setFailed(errorMessage(err));
    } finally {
      setSearching(false);
    }
  };

  const select = (i: number) => {
    setSelected(i);
    setChecked({});
    setPicked({});
    // Open Library's descriptions come separately.
    const m = result.matches[i];
    m?.records.forEach((r, j) => {
      if (!r.hasDetails) return;
      setLoadingDetails(true);
      api
        .metadataDetails(r)
        .then((full) => setResult('matches', i, 'records', j, full))
        .catch(() => setResult('matches', i, 'records', j, 'hasDetails', false))
        .finally(() => setLoadingDetails(false));
    });
  };

  /** A field's distinct values in the selected match, with the sources giving each. */
  const options = (f: Field) => {
    const out: { value: Value; sources: SourceId[] }[] = [];
    for (const r of match()?.records ?? []) {
      const v = f.from(r, props.current);
      if (v === null) continue;
      const existing = out.find((o) => same(f, o.value, v));
      if (existing) existing.sources.push(r.source!);
      else out.push({ value: v, sources: [r.source!] });
    }
    return out;
  };

  const rows = createMemo(() =>
    FIELDS.map((f) => {
      const opts = options(f);
      const current = f.current(props.current);
      // Descriptions: the longest by default; anything else: the first source's.
      const defaultPick =
        f.key === 'descriptionHtml'
          ? opts.reduce((best, o, i) => (f.text(o.value).length > f.text(opts[best]!.value).length ? i : best), 0)
          : 0;
      const pick = Math.min(picked[f.key] ?? defaultPick, Math.max(opts.length - 1, 0));
      const value = opts[pick]?.value ?? null;
      const unchanged = value === null || same(f, value, current);
      // Fill gaps by default; replacing what's there is the user's call.
      // Identifiers merge, so they're safe unless an existing one would change.
      const safe =
        f.key === 'identifiers'
          ? value !== null &&
            Object.entries(props.current.identifiers).every(([k, v]) => (value as Record<string, string>)[k] === v)
          : current === null;
      const on = !unchanged && (checked[f.key] ?? safe);
      return { f, opts, pick, value, current, unchanged, on };
    }),
  );

  const covers = () => {
    const urls = (match()?.records ?? []).flatMap((r) => (r.coverUrl ? [{ url: r.coverUrl, source: r.source! }] : []));
    return urls.filter((u, i) => urls.findIndex((x) => x.url === u.url) === i);
  };
  const coverPick = () => Math.min(picked.cover ?? 0, Math.max(covers().length - 1, 0));
  const coverOn = () => covers().length > 0 && (checked.cover ?? !props.current.hasCover);
  const chosenCount = () => rows().filter((r) => r.on).length + (coverOn() ? 1 : 0);

  const apply = () => {
    const applied: Applied = {};
    for (const r of rows()) {
      if (r.on && r.value !== null) (applied as Record<string, Value>)[r.f.key] = r.value;
    }
    if (coverOn()) applied.coverUrl = covers()[coverPick()]?.url;
    props.onApply(applied);
    props.onClose();
  };

  onMount(() => {
    dialog.showModal();
    void search();
  });

  return (
    <Portal>
      <dialog
        ref={dialog}
        aria-label="Find metadata"
        class="m-auto h-[min(680px,calc(100vh-48px))] w-[min(1000px,calc(100vw-48px))] animate-menu-in overflow-hidden rounded-2xl border border-line bg-popover p-0 text-fg shadow-lift"
        onCancel={(e) => {
          e.preventDefault();
          props.onClose();
        }}
        on:keydown={(e) => {
          const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement;
          if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
            e.preventDefault();
            if (chosenCount()) apply();
          } else if (!typing && (e.key === 'ArrowDown' || e.key === 'ArrowUp') && result.matches.length) {
            e.preventDefault();
            const n = result.matches.length;
            select((selected() + (e.key === 'ArrowDown' ? 1 : n - 1)) % n);
            dialog.querySelector(`[data-match="${selected()}"]`)?.scrollIntoView({ block: 'nearest' });
          }
        }}
      >
        <div class="flex h-full flex-col">
          <form
            class="flex items-center gap-2 border-b border-line px-5 py-3"
            onSubmit={(e) => {
              e.preventDefault();
              void search();
            }}
          >
            <h2 class="mr-2 font-serif text-lg font-semibold whitespace-nowrap">Find Metadata</h2>
            <input
              class={`${input} flex-[3]`}
              aria-label="Title"
              placeholder="Title"
              value={title()}
              onInput={(e) => setTitle(e.currentTarget.value)}
            />
            <input
              class={`${input} flex-[2]`}
              aria-label="Author"
              placeholder="Author"
              value={author()}
              onInput={(e) => setAuthor(e.currentTarget.value)}
            />
            <input
              class={`${input} w-36`}
              aria-label="ISBN"
              placeholder="ISBN"
              value={isbn()}
              onInput={(e) => setIsbn(e.currentTarget.value)}
            />
            <button type="submit" class={smallButton} disabled={searching()}>
              {searching() ? 'Searching…' : 'Search'}
            </button>
          </form>

          <div class="flex min-h-0 flex-1">
            {/* Results */}
            <ul class="w-80 flex-none overflow-y-auto border-r border-line p-2" role="listbox" aria-label="Results">
              <Show when={!searching() && !result.matches.length}>
                <li class="p-4 text-center text-sm text-muted">
                  {failed() ?? 'No matches. Try fewer words, or the ISBN alone.'}
                </li>
              </Show>
              <For each={result.matches}>
                {(m, i) => {
                  const r = () => m.records[0]!; // A match always has a record.
                  return (
                    <li
                      role="option"
                      data-match={i()}
                      aria-selected={i() === selected()}
                      class="flex cursor-default gap-2.5 rounded-lg p-2 hover:bg-hover aria-selected:bg-highlight"
                      onClick={() => select(i())}
                    >
                      <RemoteCover
                        url={m.records.find((x) => x.coverUrl)?.coverUrl ?? null}
                        class="h-[60px] w-10 flex-none"
                      />
                      <div class="min-w-0 text-xs">
                        <div class="truncate text-sm font-medium">{r().title}</div>
                        <div class="truncate text-muted">{r().authors.join(' & ')}</div>
                        <div class="truncate text-faint">
                          {[r().published?.slice(0, 4), r().publisher].filter(Boolean).join(' · ')}
                        </div>
                        <div class="mt-1 flex flex-wrap gap-1">
                          <span
                            class="rounded-full px-1.5 text-[10px] font-medium"
                            classList={{
                              'bg-accent/15 text-accent': m.kind !== 'possible',
                              'bg-element text-muted': m.kind === 'possible',
                            }}
                          >
                            {KIND_LABEL[m.kind]}
                          </span>
                          <For each={m.records}>
                            {(x) => (
                              <span class="rounded-full bg-element px-1.5 text-[10px] text-muted">
                                {SOURCE_NAME[x.source!]}
                              </span>
                            )}
                          </For>
                        </div>
                      </div>
                    </li>
                  );
                }}
              </For>
            </ul>

            {/* Comparison */}
            <div class="min-w-0 flex-1 overflow-y-auto px-5 py-3">
              <Show
                when={match()}
                fallback={<p class="pt-10 text-center text-sm text-muted">{searching() ? 'Searching…' : ''}</p>}
              >
                <div class="mb-2 grid grid-cols-[1.25rem_6.5rem_1fr_1fr] gap-x-3 text-[11px] font-semibold tracking-[.06em] text-muted uppercase">
                  <span />
                  <span />
                  <span>Now</span>
                  <span>Found</span>
                </div>
                <For each={rows()}>
                  {(row) => (
                    <FieldRow
                      label={row.f.label}
                      on={row.on}
                      disabled={row.unchanged}
                      onToggle={(on) => setChecked(row.f.key, on)}
                      current={
                        row.current === null ? null : (
                          <Text html={row.f.key === 'descriptionHtml'} value={row.f.text(row.current)} />
                        )
                      }
                      found={
                        row.value === null ? (
                          <span class="text-faint">
                            {row.f.key === 'descriptionHtml' && loadingDetails() ? 'Loading…' : '—'}
                          </span>
                        ) : (
                          <>
                            <div class="flex items-start gap-2">
                              <Show when={row.unchanged}>
                                <SameBadge />
                              </Show>
                              <div class="min-w-0" classList={{ 'text-muted': row.unchanged }}>
                                <Text html={row.f.key === 'descriptionHtml'} value={row.f.text(row.value)} />
                              </div>
                            </div>
                          </>
                        )
                      }
                      sources={
                        row.opts.length > 1
                          ? row.opts.map((o) => o.sources.map((s) => SOURCE_NAME[s]).join(' & '))
                          : null
                      }
                      pick={row.pick}
                      onPick={(i) => setPicked(row.f.key, i)}
                    />
                  )}
                </For>
                <FieldRow
                  label="Cover"
                  on={coverOn()}
                  disabled={!covers().length}
                  onToggle={(on) => setChecked('cover', on)}
                  current={props.current.hasCover ? <CurrentCover id={props.id} /> : null}
                  found={
                    covers().length ? (
                      <RemoteCover url={covers()[coverPick()]!.url} class="aspect-[2/3] w-28" withSize />
                    ) : (
                      <span class="text-faint">—</span>
                    )
                  }
                  sources={covers().length > 1 ? covers().map((c) => SOURCE_NAME[c.source]) : null}
                  pick={coverPick()}
                  onPick={(i) => setPicked('cover', i)}
                />
                <Show when={match()?.records.some((r) => r.link)}>
                  <p class="mt-3 text-[11px] text-faint">
                    From{' '}
                    {match()!
                      .records.map((r) => SOURCE_NAME[r.source!])
                      .join(' and ')}
                    . Fields that are empty now are ticked; replacing what's there is up to you.
                  </p>
                </Show>
              </Show>
            </div>
          </div>

          <div class="flex items-center justify-end gap-2 border-t border-line px-5 py-3">
            <Show when={result.errors.length}>
              <p class="mr-auto text-xs text-danger select-text" role="alert">
                {result.errors.join(' · ')}
              </p>
            </Show>
            <span class="text-xs text-faint">⌘↩ to apply · Esc to cancel</span>
            <button type="button" class={smallButton} onClick={() => props.onClose()}>
              Cancel
            </button>
            <button
              type="button"
              disabled={!chosenCount()}
              class="cursor-pointer rounded-lg bg-accent px-4 py-1.5 text-sm font-semibold text-on-accent hover:brightness-110 disabled:cursor-default disabled:opacity-50"
              onClick={apply}
            >
              {chosenCount() ? `Use ${chosenCount()} ${chosenCount() === 1 ? 'Field' : 'Fields'}` : 'Use'}
            </button>
          </div>
        </div>
      </dialog>
    </Portal>
  );
}

function Text(props: { value: string; html: boolean }) {
  // Descriptions are sanitized in Rust before they get here.
  return (
    <Show when={props.html} fallback={<span class="[overflow-wrap:anywhere]">{props.value}</span>}>
      <div class="comments-html line-clamp-6 font-serif text-[13px]/[1.5]" innerHTML={props.value} />
    </Show>
  );
}

function FieldRow(props: {
  label: string;
  on: boolean;
  disabled: boolean;
  onToggle(on: boolean): void;
  current: JSX.Element | null;
  found: JSX.Element;
  sources: string[] | null;
  pick: number;
  onPick(i: number): void;
}) {
  return (
    <div class="grid grid-cols-[1.25rem_6.5rem_1fr_1fr] items-start gap-x-3 border-b border-line py-2.5 text-sm last:border-b-0">
      <input
        type="checkbox"
        class="mt-0.5 size-4 accent-accent"
        aria-label={`Use the found ${props.label.toLowerCase()}`}
        checked={props.on}
        disabled={props.disabled}
        onChange={(e) => props.onToggle(e.currentTarget.checked)}
      />
      <span class="font-medium text-muted">{props.label}</span>
      <div
        class="min-w-0"
        classList={{ 'opacity-50 line-through decoration-faint': props.on && props.current !== null }}
      >
        {props.current ?? <span class="text-faint">—</span>}
      </div>
      <div class="min-w-0" classList={{ 'opacity-60': !props.on }}>
        <Show when={props.sources}>
          {(s) => (
            <select
              class="mb-1 h-6 rounded border border-line bg-field px-1 text-[11px]"
              aria-label={`Source for ${props.label.toLowerCase()}`}
              value={props.pick}
              onChange={(e) => props.onPick(Number(e.currentTarget.value))}
            >
              <For each={s()}>{(name, i) => <option value={i()}>{name}</option>}</For>
            </select>
          )}
        </Show>
        <div>{props.found}</div>
      </div>
    </div>
  );
}
