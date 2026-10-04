import { createResource, createSignal, For, type JSX, onMount, type ParentProps, Show } from 'solid-js';
import { createStore, unwrap } from 'solid-js/store';
import X from 'lucide-solid/icons/x';

import { api, type AuthorSort, type BookEdit, type BookRow } from '../api';
import { Autocomplete } from './Autocomplete';
import { changeCover, closeEditor, type Editing, saveEdit } from '../editor';
import { items } from '../library';
import { Cover } from './Cover';
import { type Applied, MatchDialog } from './MatchDialog';
import { Icon } from './Icon';

const input =
  'h-8 w-full rounded-md border border-line bg-field px-2.5 outline-none select-text focus:border-focus focus:ring-3 focus:ring-focus/20 disabled:opacity-50';
const smallButton =
  'cursor-pointer rounded-md border border-line bg-element px-2.5 py-1 text-xs font-medium hover:bg-hover disabled:cursor-default disabled:opacity-50';

// Lists are edited as text, the way calibre does it.
const joinAuthors = (a: string[]) => a.join(' & ');
const splitAuthors = (s: string) => s.split('&').map((a) => a.trim());
const joinList = (v: string[]) => v.join(', ');
const splitList = (s: string) => s.split(',').map((t) => t.trim());
const joinIds = (ids: Record<string, string | undefined>) =>
  Object.entries(ids)
    .map(([k, v]) => `${k}:${v ?? ''}`)
    .join(', ');
const splitIds = (s: string): Record<string, string> =>
  Object.fromEntries(
    splitList(s)
      .map((pair) => pair.split(/:(.*)/s).map((x) => x.trim()))
      .filter(([k, v]) => k && v),
  );

function Field(props: ParentProps<{ label: string; hint?: JSX.Element; for?: string }>) {
  return (
    <label class="block" for={props.for}>
      <span class="mb-1 block text-xs font-medium text-muted">{props.label}</span>
      {props.children}
      <Show when={props.hint}>
        <span class="mt-1 block text-[11px] text-faint">{props.hint}</span>
      </Show>
    </label>
  );
}

/** Edit a book's metadata. Mount with the book being edited; unmount to close. */
export function EditDialog(props: { editing: Editing }) {
  let dialog!: HTMLDialogElement;
  let description!: HTMLDivElement;
  const id = props.editing.id;
  const [form, setForm] = createStore<BookEdit>(structuredClone(props.editing.edit));
  // Text forms of the list fields.
  const [authors, setAuthors] = createSignal(joinAuthors(form.authors));
  const [tags, setTags] = createSignal(joinList(form.tags));
  const [languages, setLanguages] = createSignal(joinList(form.languages));
  const [identifiers, setIdentifiers] = createSignal(joinIds(form.identifiers));
  // Until the user edits the title sort, it follows the title (empty = automatic).
  const [sortTouched, setSortTouched] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [saving, setSaving] = createSignal(false);

  const [matching, setMatching] = createSignal(false);

  // Author sort names: shown as one line, changed per author (it's the author's,
  // shared by all their books). Overrides by name; "" means back to automatic.
  const authorNames = () => splitAuthors(authors()).filter(Boolean);
  const [sorts] = createResource(
    () => authorNames().join('\n'),
    (key) => api.authorSorts(key ? key.split('\n') : []).catch(() => [] as AuthorSort[]),
  );
  const [sortOverrides, setSortOverrides] = createStore<Record<string, string>>({});
  const [editingSorts, setEditingSorts] = createSignal(false);
  const known = (name: string) => sorts()?.find((s) => s.name === name);
  const sortOf = (name: string) => {
    const o = sortOverrides[name];
    return o === undefined ? (known(name)?.sort ?? '') : o || (known(name)?.automatic ?? '');
  };
  const isCustom = (name: string) => {
    const k = known(name);
    return k !== undefined && sortOf(name) !== k.automatic;
  };

  const row = () => items().find((b) => b.id === id);

  /** Take what was chosen in Find Metadata into the form (saved with Save). */
  const applyFound = (a: Applied) => {
    if (a.title !== undefined) {
      setForm('title', a.title);
      if (!sortTouched()) setForm('titleSort', '');
    }
    if (a.authors) setAuthors(joinAuthors(a.authors));
    if (a.publisher !== undefined) setForm('publisher', a.publisher);
    if (a.published !== undefined) setForm('published', a.published);
    if (a.languages) setLanguages(joinList(a.languages));
    if (a.tags) setTags(joinList(a.tags));
    if (a.identifiers) setIdentifiers(joinIds(a.identifiers));
    if (a.descriptionHtml !== undefined) description.innerHTML = a.descriptionHtml;
    if (a.coverUrl) void changeCover(id, { url: a.coverUrl });
  };
  // Existing names for autocomplete; the form works without them.
  const [names] = createResource(() => api.nameLists().catch(() => null));

  // To tell whether anything changed before stepping to another book.
  let initial = '';
  onMount(() => {
    description.innerHTML = form.descriptionHtml ?? '';
    initial = JSON.stringify(build());
    dialog.showModal();
  });

  // Stepping through several books (Edit One by One).
  const queue = props.editing.queue;
  const position = queue ? queue.indexOf(id) : -1;
  const prev = queue && position > 0 ? queue[position - 1] : undefined;
  const next = queue && position >= 0 && position < queue.length - 1 ? queue[position + 1] : undefined;

  const build = (): BookEdit => {
    const html = description.innerHTML;
    return {
      ...unwrap(form),
      authors: splitAuthors(authors()),
      authorSorts: Object.fromEntries(Object.entries(sortOverrides).filter(([n]) => authorNames().includes(n))),
      tags: splitList(tags()),
      languages: splitList(languages()),
      identifiers: splitIds(identifiers()),
      descriptionHtml: description.textContent?.trim() ? html : null,
    };
  };

  /** Save (only if changed, when moving on), then close or open book `then`. */
  const save = async (then?: number) => {
    if (saving()) return;
    setSaving(true);
    const edit = build();
    const changed = then === undefined || JSON.stringify(edit) !== initial;
    setError(await saveEdit(id, changed ? edit : null, then));
    setSaving(false);
  };

  const stars = () => (form.rating ? Math.round(form.rating / 2) : 0);

  return (
    <dialog
      ref={dialog}
      aria-label="Edit book"
      class="m-auto max-h-[calc(100vh-48px)] w-[min(820px,calc(100vw-48px))] animate-menu-in overflow-hidden rounded-2xl border border-line bg-popover p-0 text-fg shadow-lift"
      onCancel={(e) => {
        e.preventDefault();
        closeEditor();
      }}
      on:keydown={(e) => {
        if ((e.metaKey || e.ctrlKey) && (e.key === 's' || e.key === 'Enter')) {
          e.preventDefault();
          void save();
        }
      }}
    >
      <form
        class="flex max-h-[calc(100vh-48px)] flex-col"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <div class="flex items-center gap-3 border-b border-line px-6 py-4">
          <h2 class="font-serif text-xl font-semibold">Edit Book</h2>
          <button
            type="button"
            class={smallButton + ' ml-1'}
            title="Look this book up online (Open Library, Google Books) and choose what to take"
            onClick={() => setMatching(true)}
          >
            Find Metadata…
          </button>
          <span class="ml-auto text-xs text-faint">⌘S to save · Esc to cancel</span>
        </div>

        <div class="flex min-h-0 flex-1 gap-6 overflow-y-auto px-6 py-5">
          {/* Cover */}
          <div class="w-40 flex-none">
            <div class="relative aspect-[2/3] w-full overflow-hidden rounded-lg bg-element shadow-card">
              <Cover id={id} title={form.title} authors={authors()} hasCover={row()?.hasCover ?? false} size="full" />
            </div>
            <div class="mt-2 flex flex-wrap gap-1.5">
              <button type="button" class={smallButton} onClick={() => void changeCover(id, 'choose')}>
                Change…
              </button>
              <Show when={row()?.hasCover}>
                <button type="button" class={smallButton} onClick={() => void changeCover(id, 'remove')}>
                  Remove
                </button>
              </Show>
            </div>
            <p class="mt-1.5 text-[11px] text-faint">Cover changes apply straight away.</p>
          </div>

          {/* Fields */}
          <div class="grid min-w-0 flex-1 grid-cols-2 content-start gap-x-4 gap-y-3.5">
            <div class="col-span-2">
              <Field label="Title" for="edit-title">
                <input
                  id="edit-title"
                  class={input}
                  required
                  value={form.title}
                  onInput={(e) => {
                    setForm('title', e.currentTarget.value);
                    if (!sortTouched()) setForm('titleSort', '');
                  }}
                />
              </Field>
            </div>
            <Field label="Title sort" hint="Leave empty to derive it from the title.">
              <input
                class={input}
                placeholder="Automatic"
                value={form.titleSort}
                onInput={(e) => {
                  setSortTouched(true);
                  setForm('titleSort', e.currentTarget.value);
                }}
              />
            </Field>
            <Field
              label="Authors"
              hint={
                <>
                  Separate several with “&”, in order.
                  <Show when={authorNames().length && sorts()}>
                    <span class="mt-0.5 flex items-baseline gap-1.5">
                      <span class="min-w-0 truncate" title="How these authors sort in lists">
                        Sorted as <span class="text-muted">{authorNames().map(sortOf).join(' & ')}</span>
                      </span>
                      <button
                        type="button"
                        class="flex-none cursor-pointer text-link hover:underline"
                        onClick={() => setEditingSorts((v) => !v)}
                      >
                        {editingSorts() ? 'Done' : 'Change'}
                      </button>
                    </span>
                  </Show>
                </>
              }
            >
              <Autocomplete
                class={input}
                separator="&"
                options={names()?.authors ?? []}
                value={authors()}
                onInput={setAuthors}
              />
            </Field>
            <Show when={editingSorts()}>
              <div class="col-span-2 -mt-1 rounded-lg bg-bg p-3">
                <div class="mb-2 text-xs text-muted">
                  Sort names decide where authors appear in lists sorted by author. Changing one applies to all of that
                  author's books.
                </div>
                <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1.3fr)_auto] items-center gap-x-3 gap-y-1.5">
                  <For each={authorNames()}>
                    {(name) => (
                      <>
                        <span class="truncate text-xs">{name}</span>
                        <input
                          class={input + ' h-7 text-xs'}
                          aria-label={`Sort name for ${name}`}
                          value={sortOf(name)}
                          onInput={(e) => setSortOverrides(name, e.currentTarget.value)}
                        />
                        <button
                          type="button"
                          class={smallButton}
                          disabled={!isCustom(name)}
                          title={`Automatic: ${known(name)?.automatic ?? ''}`}
                          onClick={() => setSortOverrides(name, '')}
                        >
                          Automatic
                        </button>
                      </>
                    )}
                  </For>
                </div>
              </div>
            </Show>
            <Field label="Series">
              <Autocomplete
                class={input}
                options={names()?.series ?? []}
                value={form.series ?? ''}
                onInput={(v) => setForm('series', v || null)}
              />
            </Field>
            <Field label={form.seriesEnd != null ? 'Books in series' : 'Number in series'}>
              <NumberRange
                start={form.seriesIndex}
                end={form.seriesEnd}
                disabled={!form.series}
                onChange={(start, end) => {
                  setForm('seriesIndex', start);
                  setForm('seriesEnd', end);
                }}
              />
            </Field>
            <Show when={form.series}>
              <div class="col-span-2 -mt-1.5 space-y-2">
                <For each={form.moreSeries}>
                  {(s, i) => (
                    <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] items-center gap-x-4">
                      <Autocomplete
                        class={input}
                        placeholder="Another series"
                        options={names()?.series ?? []}
                        value={s.name}
                        onInput={(v) => setForm('moreSeries', i(), 'name', v)}
                      />
                      <NumberRange
                        start={s.index}
                        end={s.end}
                        onChange={(start, end) => {
                          setForm('moreSeries', i(), 'index', start);
                          setForm('moreSeries', i(), 'end', end);
                        }}
                      />
                      <button
                        type="button"
                        class="grid size-7 cursor-pointer place-items-center rounded-md text-muted hover:bg-hover hover:text-fg"
                        aria-label="Remove this series"
                        onClick={() => setForm('moreSeries', (list) => list.filter((_, j) => j !== i()))}
                      >
                        <Icon of={X} />
                      </button>
                    </div>
                  )}
                </For>
                <button
                  type="button"
                  class="cursor-pointer text-xs text-link hover:underline"
                  onClick={() => setForm('moreSeries', (list) => [...list, { name: '', index: 1, end: null }])}
                >
                  + Another series
                </button>
              </div>
            </Show>
            <Show when={form.seriesEnd != null || form.collects.length}>
              <div class="col-span-2">
                <Collects
                  id={id}
                  ids={form.collects}
                  series={form.series}
                  start={form.seriesIndex}
                  end={form.seriesEnd}
                  onChange={(ids) => setForm('collects', ids)}
                />
              </div>
            </Show>
            <Field label="Publisher">
              <Autocomplete
                class={input}
                options={names()?.publishers ?? []}
                value={form.publisher ?? ''}
                onInput={(v) => setForm('publisher', v || null)}
              />
            </Field>
            <Field label="Published">
              <input
                class={input}
                type="date"
                value={form.published ?? ''}
                onInput={(e) => setForm('published', e.currentTarget.value || null)}
              />
            </Field>
            <Field label="Rating">
              <div class="flex h-8 items-center gap-0.5" role="radiogroup" aria-label="Rating">
                <For each={[1, 2, 3, 4, 5]}>
                  {(n) => (
                    <button
                      type="button"
                      role="radio"
                      aria-checked={stars() === n}
                      aria-label={`${n} star${n > 1 ? 's' : ''}`}
                      class="cursor-pointer px-0.5 text-lg leading-none"
                      classList={{ 'text-star': n <= stars(), 'text-faint hover:text-star': n > stars() }}
                      // Clicking the current rating clears it.
                      onClick={() => setForm('rating', stars() === n ? null : n * 2)}
                    >
                      ★
                    </button>
                  )}
                </For>
                <Show when={form.rating}>
                  <button
                    type="button"
                    class="ml-2 cursor-pointer text-xs text-muted hover:text-fg"
                    onClick={() => setForm('rating', null)}
                  >
                    Clear
                  </button>
                </Show>
              </div>
            </Field>
            <Field label="Languages" hint="ISO codes, comma-separated (eng, fra).">
              <input class={input} value={languages()} onInput={(e) => setLanguages(e.currentTarget.value)} />
            </Field>
            <div class="col-span-2">
              <Field label="Tags" hint="Comma-separated.">
                <Autocomplete
                  class={input}
                  separator=","
                  options={names()?.tags ?? []}
                  value={tags()}
                  onInput={setTags}
                />
              </Field>
            </div>
            <div class="col-span-2">
              <Field label="Identifiers" hint="scheme:value, comma-separated (isbn:9780…, google:…).">
                <input class={input} value={identifiers()} onInput={(e) => setIdentifiers(e.currentTarget.value)} />
              </Field>
            </div>
            <div class="col-span-2">
              <span class="mb-1 block text-xs font-medium text-muted">Description</span>
              {/* Rich text: ⌘B / ⌘I work. Sanitized when saved. */}
              <div
                ref={description}
                contentEditable
                role="textbox"
                aria-multiline="true"
                aria-label="Description"
                class="comments-html min-h-28 rounded-md border border-line bg-field px-3 py-2 font-serif text-sm/[1.6] outline-none select-text focus:border-focus focus:ring-3 focus:ring-focus/20"
              />
            </div>
          </div>
        </div>

        <div class="flex items-center justify-end gap-2 border-t border-line px-6 py-3">
          <Show when={error()}>
            {(e) => (
              <p class="mr-auto text-xs text-danger select-text" role="alert">
                {e()}
              </p>
            )}
          </Show>
          <Show when={queue}>
            {(q) => (
              <div class="mr-auto flex items-center gap-2">
                <button
                  type="button"
                  class={smallButton + ' px-3 py-1.5 text-sm'}
                  disabled={prev === undefined || saving()}
                  title="Save and edit the previous book"
                  onClick={() => void save(prev)}
                >
                  ‹ Previous
                </button>
                <button
                  type="button"
                  class={smallButton + ' px-3 py-1.5 text-sm'}
                  disabled={next === undefined || saving()}
                  title="Save and edit the next book"
                  onClick={() => void save(next)}
                >
                  Next ›
                </button>
                <span class="text-xs text-faint tabular-nums">
                  {position + 1} of {q().length} · changes are saved as you move
                </span>
              </div>
            )}
          </Show>
          <button type="button" class={smallButton + ' px-4 py-1.5 text-sm'} onClick={() => closeEditor()}>
            Cancel
          </button>
          <button
            type="submit"
            disabled={saving()}
            class="cursor-pointer rounded-lg bg-accent px-4 py-1.5 text-sm font-semibold text-on-accent hover:brightness-110 disabled:opacity-60"
          >
            {saving() ? 'Saving…' : 'Save'}
          </button>
        </div>
      </form>
      <Show when={matching()}>
        <MatchDialog
          id={id}
          current={{
            title: form.title,
            authors: splitAuthors(authors()).filter(Boolean),
            publisher: form.publisher,
            published: form.published,
            languages: splitList(languages()).filter(Boolean),
            tags: splitList(tags()).filter(Boolean),
            identifiers: splitIds(identifiers()),
            descriptionHtml: description.textContent?.trim() ? description.innerHTML : null,
            hasCover: row()?.hasCover ?? false,
            isbn: splitIds(identifiers()).isbn ?? null,
          }}
          onApply={applyFound}
          onClose={() => setMatching(false)}
        />
      </Show>
    </dialog>
  );
}

/** A series number, or with Omnibus ticked, a range ("Books 1 to 3"). */
function NumberRange(props: {
  start: number;
  end: number | null;
  disabled?: boolean;
  onChange(start: number, end: number | null): void;
}) {
  const num =
    'h-8 w-20 rounded-md border border-line bg-field px-2.5 tabular-nums outline-none focus:border-focus focus:ring-3 focus:ring-focus/20 disabled:opacity-50';
  return (
    <div class="flex items-center gap-2" classList={{ 'opacity-50': props.disabled }}>
      <input
        class={num}
        type="number"
        min="0"
        step="0.5"
        aria-label={props.end != null ? 'First book' : 'Number in series'}
        disabled={props.disabled}
        value={props.start}
        onInput={(e) => props.onChange(Number(e.currentTarget.value) || 0, props.end)}
      />
      <Show when={props.end != null}>
        <span class="text-xs text-muted">to</span>
        <input
          class={num}
          type="number"
          min="0"
          step="0.5"
          aria-label="Last book"
          disabled={props.disabled}
          value={props.end ?? ''}
          onInput={(e) => props.onChange(props.start, Number(e.currentTarget.value) || 0)}
        />
      </Show>
      <label
        class="ml-1 flex cursor-pointer items-center gap-1.5 text-xs text-muted"
        title="An omnibus or box set: number it as a range of the series' books"
      >
        <input
          type="checkbox"
          class="accent-accent"
          disabled={props.disabled}
          checked={props.end != null}
          onChange={(e) => props.onChange(props.start, e.currentTarget.checked ? props.start + 1 : null)}
        />
        Omnibus
      </label>
    </div>
  );
}

/** The books an omnibus contains: linked by search, or all at once from its series range. */
function Collects(props: {
  id: number;
  ids: number[];
  series: string | null;
  start: number;
  end: number | null;
  onChange(ids: number[]): void;
}) {
  const [titles, setTitles] = createStore<Record<number, string>>({});
  const [query, setQuery] = createSignal('');
  const remember = (rows: BookRow[]) => rows.forEach((r) => setTitles(r.id, r.title));
  void api
    .bookRows(props.ids)
    .then(remember)
    .catch(() => {});

  const [results] = createResource(
    () => query().trim(),
    async (q) => {
      if (q.length < 2) return [];
      const rows = await api.listBooks(q, 'title', 'asc').catch(() => [] as BookRow[]);
      return rows.filter((r) => r.id !== props.id && !props.ids.includes(r.id)).slice(0, 6);
    },
  );
  // Books of the same series numbered inside the range, not yet linked.
  const [inRange] = createResource(
    () => (props.series && props.end != null ? ([props.series, props.start, props.end] as const) : null),
    async ([series, start, end]) => {
      const rows = await api.listBooks(`series:"${series}"`, 'series', 'asc').catch(() => [] as BookRow[]);
      return rows.filter((r) => {
        const n = Number(r.seriesIndex);
        return r.id !== props.id && r.series === series && Number.isFinite(n) && n >= start && n <= end;
      });
    },
  );
  const suggested = () => (inRange() ?? []).filter((r) => !props.ids.includes(r.id));
  const add = (rows: BookRow[]) => {
    remember(rows);
    props.onChange([...props.ids, ...rows.map((r) => r.id).filter((i) => !props.ids.includes(i))]);
  };

  return (
    <div class="rounded-lg bg-bg p-3">
      <div class="mb-1 text-xs font-medium text-muted">Collects</div>
      <div class="mb-2 text-[11px] text-faint">The books this omnibus contains, if you have them separately.</div>
      <div class="flex flex-wrap gap-1.5">
        <For each={props.ids}>
          {(bookId) => (
            <span class="flex items-center gap-1 rounded-full bg-element py-[3px] pr-1 pl-2.5 text-xs">
              {titles[bookId] ?? `Book ${bookId}`}
              <button
                type="button"
                class="grid size-4 cursor-pointer place-items-center rounded-full text-muted hover:bg-hover hover:text-fg"
                aria-label={`Unlink ${titles[bookId] ?? 'book'}`}
                onClick={() => props.onChange(props.ids.filter((i) => i !== bookId))}
              >
                <Icon of={X} size={12} />
              </button>
            </span>
          )}
        </For>
        <Show when={suggested().length}>
          <button type="button" class={smallButton} onClick={() => add(suggested())}>
            Add {suggested().length === 1 ? '1 book' : `${suggested().length} books`} numbered {props.start}–{props.end}{' '}
            in {props.series}
          </button>
        </Show>
      </div>
      <div class="relative mt-2">
        <input
          class={input + ' h-7 text-xs'}
          placeholder="Find a book to link…"
          aria-label="Find a book to link"
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
        />
        <Show when={results()?.length}>
          <ul class="mt-1 rounded-md border border-line bg-popover p-1">
            <For each={results()}>
              {(r) => (
                <li>
                  <button
                    type="button"
                    class="w-full cursor-pointer truncate rounded px-2 py-1 text-left text-xs hover:bg-highlight"
                    onClick={() => {
                      add([r]);
                      setQuery('');
                    }}
                  >
                    {r.title} <span class="text-muted">· {r.authors}</span>
                    <Show when={r.series}>
                      <span class="text-faint">
                        {' '}
                        · {r.series} #{r.seriesIndex}
                      </span>
                    </Show>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </div>
  );
}
