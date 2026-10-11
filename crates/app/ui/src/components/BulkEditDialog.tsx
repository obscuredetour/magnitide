import { createResource, createSignal, For, type JSX, onMount, type ParentProps, Show } from 'solid-js';
import { createStore } from 'solid-js/store';

import { api, type BulkEdit, errorMessage, type FieldValue, onBulkProgress, type Shared } from '../api';
import { closeBulkEditor, editOneByOne } from '../editor';
import { items, refresh, setEditRevision } from '../library';
import { startJob } from '../jobs';
import { plural, writeOnSave } from '../writeback';
import { Autocomplete } from './Autocomplete';
import { COMIC_KEY, comicFields, comicMarker, customFields, isComicFormat, ownFields } from '../fields';
import { CustomFieldInput, customFieldHint } from './CustomFieldInput';
import { primaryButton, secondaryButton, smallButton } from '../buttons';
import { keys } from '../keys';
import { backdropDismiss } from '../backdrop';

const input =
  'h-8 w-full rounded-control border border-line bg-field px-2.5 outline-none select-text placeholder:text-faint focus:border-focus focus:ring-3 focus:ring-focus/20';

const splitAuthors = (s: string) =>
  s
    .split('&')
    .map((a) => a.trim())
    .filter(Boolean);
const splitList = (s: string) =>
  s
    .split(',')
    .map((t) => t.trim())
    .filter(Boolean);

/** Edits that differ from what the books have; absent = leave each book's. */
type Draft = {
  authors?: string;
  publisher?: string;
  published?: string;
  rating?: number;
  languages?: string;
  series?: string;
};

/** A field row: label, control, and "Revert" once changed. */
function Row(
  props: ParentProps<{ label: string; changed: boolean; onRevert(): void; hint?: JSX.Element; onClear?: () => void }>,
) {
  return (
    <div class="grid grid-cols-[7.5rem_1fr] items-start gap-x-4">
      <span class="pt-2 text-xs font-medium text-muted" classList={{ 'text-accent!': props.changed }}>
        {props.label}
      </span>
      <div class="min-w-0">
        {props.children}
        <div class="mt-1 flex min-h-4 items-center gap-2 text-[11px] text-faint">
          <span class="min-w-0">{props.hint}</span>
          <span class="ml-auto flex gap-3">
            <Show when={props.onClear}>
              {(clear) => (
                <button type="button" class="cursor-pointer text-link hover:underline" onClick={() => clear()()}>
                  Clear on all
                </button>
              )}
            </Show>
            <Show when={props.changed}>
              <button type="button" class="cursor-pointer text-link hover:underline" onClick={() => props.onRevert()}>
                Revert
              </button>
            </Show>
          </span>
        </div>
      </div>
    </div>
  );
}

/** Edit several books at once. Fields show what the books share ("Mixed" where they differ); only changed fields are saved. */
export function BulkEditDialog(props: { ids: number[] }) {
  let dialog!: HTMLDialogElement;
  const [values] = createResource(() => api.bulkValues(props.ids));
  const [names] = createResource(() => api.nameLists().catch(() => null));
  const [draft, setDraft] = createStore<Draft>({});
  /** Custom fields changed, by key: a value to set on every book, or null to clear it on all. */
  const [customDraft, setCustomDraft] = createStore<Record<string, FieldValue | null>>({});
  const sharedCustom = (key: string) => values()?.custom[key];
  // The comic fields show when any of these books is a comic, or they're being marked as comics.
  const showComic = () => {
    if (!comicMarker()) return false;
    const draft = customDraft[COMIC_KEY];
    if (draft !== undefined) return draft === true;
    const shared = sharedCustom(COMIC_KEY);
    if (shared?.mixed || shared?.value === true) return true;
    return props.ids.some((id) =>
      items()
        .find((b) => b.id === id)
        ?.formats.some(isComicFormat),
    );
  };
  const bulkFields = () => [
    ...ownFields(),
    ...(comicMarker() ? [comicMarker()!] : []),
    ...(showComic() ? comicFields() : []),
  ];
  const setCustom = (key: string, value: FieldValue | null) => {
    const shared = sharedCustom(key);
    // As for the other fields: emptying a mixed field keeps each book's;
    // back to the shared value is no change.
    const same = shared?.mixed ? value === null : JSON.stringify(value) === JSON.stringify(shared?.value ?? null);
    setCustomDraft(key, same ? (undefined as never) : value);
  };
  const [addTags, setAddTags] = createSignal('');
  const [removeTags, setRemoveTags] = createSignal<ReadonlySet<string>>(new Set());
  const [numbering, setNumbering] = createSignal(false);
  const [seriesStart, setSeriesStart] = createSignal(1);
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  onMount(() => dialog.showModal());

  const n = props.ids.length;
  /** Titles in the order numbering follows (the list's). */
  const order = () => props.ids.map((id) => items().find((b) => b.id === id)?.title ?? `Book ${id}`);
  /** The text a field shows: the draft once edited, else the shared value. */
  const shown = <T,>(key: keyof Draft, shared: Shared<T> | undefined, text: (v: T) => string) =>
    (draft[key] as string | undefined) ?? (shared?.value != null ? text(shared.value) : '');
  const mixedHint = (shared: Shared<unknown> | undefined, key: keyof Draft) =>
    draft[key] === ''
      ? 'Will be cleared on every book.'
      : shared?.mixed && draft[key] === undefined
        ? 'These books differ; leave it empty to keep each one’s.'
        : '';
  // Back to the books' own value: unchanged. On a mixed field, empty means
  // "keep each one's"; clearing is the explicit Clear button.
  const set = (key: keyof Draft, value: string, original: string) => {
    const mixed = values()?.[key]?.mixed ?? false;
    setDraft(key, (mixed ? value === '' : value === original) ? undefined : (value as never));
  };

  const changes = () => {
    const c: string[] = [];
    for (const [k, v] of Object.entries(draft)) if (v !== undefined) c.push(k);
    if (splitList(addTags()).length) c.push('add tags');
    if (removeTags().size) c.push('remove tags');
    if (numbering()) c.push('series numbers');
    for (const f of customFields()) if (f.key in customDraft) c.push(f.name);
    return c;
  };

  const save = async () => {
    if (saving() || !changes().length) return;
    setSaving(true);
    setError(null);
    const change: BulkEdit = {
      authors: draft.authors !== undefined ? splitAuthors(draft.authors) : null,
      publisher: draft.publisher ?? null,
      published: draft.published ?? null,
      rating: draft.rating ?? null,
      languages: draft.languages !== undefined ? splitList(draft.languages) : null,
      series: draft.series ?? null,
      seriesStart: numbering() ? seriesStart() : null,
      addTags: splitList(addTags()),
      removeTags: [...removeTags()],
      custom: { ...customDraft },
    };
    const job = startJob('Edit books', `Saving ${plural(props.ids.length, 'book', 'books')}…`);
    const unlisten = await onBulkProgress((p) => {
      if (p.total > 3 && p.done < p.total) job.progress(`Saving ${p.done + 1} of ${p.total}…`);
    });
    try {
      const r = await api.bulkEdit(props.ids, change);
      if (writeOnSave()) for (const id of props.ids) await api.writeMetadataToFiles(id).catch(() => null);
      await refresh();
      setEditRevision((x) => x + 1);
      closeBulkEditor();
      const parts = [`Saved ${plural(r.edited, 'book', 'books')}`];
      if (r.moved) parts.push('files renamed to match');
      if (r.failed.length) parts.push(`${r.failed.length} couldn’t be saved`);
      job.finish(parts.join(' · '), {
        error: r.failed.length > 0,
        details: r.failed,
        duration: r.failed.length ? 6000 : 2500,
      });
    } catch (err) {
      setError(errorMessage(err));
      job.finish(errorMessage(err), { error: true });
    } finally {
      unlisten();
      setSaving(false);
    }
  };

  const stars = () => {
    const r = draft.rating ?? values()?.rating.value ?? 0;
    return Math.round(r / 2);
  };

  return (
    <dialog
      ref={dialog}
      aria-label={`Edit ${n} books`}
      // Clicking outside closes it unless something's been changed.
      {...backdropDismiss(() => dialog, { dirty: () => changes().length > 0, close: closeBulkEditor })}
      class="m-auto max-h-[calc(100vh-48px)] w-[min(680px,calc(100vw-48px))] animate-menu-in overflow-hidden rounded-dialog border border-line bg-popover p-0 text-fg shadow-lift"
      onCancel={(e) => {
        e.preventDefault();
        closeBulkEditor();
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
          <h2 class="font-serif text-xl font-semibold">Edit {n} Books</h2>
          <button
            type="button"
            class={smallButton + ' ml-1'}
            title="Edit each book in turn, with Previous and Next"
            onClick={() => editOneByOne(props.ids)}
          >
            Edit One by One
          </button>
          <span class="ml-auto text-xs text-faint">{keys('mod+S')} to save · Esc to cancel</span>
        </div>

        <Show when={values()} fallback={<p class="px-6 py-10 text-center text-sm text-muted">Loading…</p>}>
          {(v) => (
            <div class="min-h-0 flex-1 space-y-2 overflow-y-auto px-6 py-5">
              <p class="mb-3 text-xs text-muted">
                Fields show what these books share. Only what you change is saved, to every one of them.
              </p>
              <Row
                label="Authors"
                changed={draft.authors !== undefined}
                onRevert={() => setDraft('authors', undefined)}
                hint={
                  mixedHint(v().authors, 'authors') || 'Replaces the authors of every book. Separate several with “&”.'
                }
              >
                <Autocomplete
                  class={input}
                  separator="&"
                  placeholder={v().authors.mixed ? 'Mixed' : ''}
                  options={names()?.authors ?? []}
                  value={shown('authors', v().authors, (a) => a.join(' & '))}
                  onInput={(t) => set('authors', t, v().authors.value?.join(' & ') ?? '')}
                />
              </Row>
              <Row
                label="Series"
                changed={draft.series !== undefined || numbering()}
                onClear={
                  (v().series.mixed || v().series.value != null) && draft.series !== ''
                    ? () => setDraft('series', '')
                    : undefined
                }
                onRevert={() => {
                  setDraft('series', undefined);
                  setNumbering(false);
                }}
                hint={mixedHint(v().series, 'series')}
              >
                <Autocomplete
                  class={input}
                  placeholder={v().series.mixed ? 'Mixed' : ''}
                  options={names()?.series ?? []}
                  value={shown('series', v().series, (s) => s)}
                  onInput={(t) => set('series', t, v().series.value ?? '')}
                />
                <label class="mt-2 flex min-w-0 items-center gap-2 text-xs whitespace-nowrap">
                  <input
                    type="checkbox"
                    class="accent-accent"
                    checked={numbering()}
                    onChange={(e) => setNumbering(e.currentTarget.checked)}
                  />
                  Number them in list order, starting at
                  <input
                    type="number"
                    min="0"
                    step="1"
                    class="h-6 w-16 rounded-control border border-line bg-field px-1.5 tabular-nums disabled:opacity-50"
                    disabled={!numbering()}
                    value={seriesStart()}
                    onInput={(e) => setSeriesStart(Number(e.currentTarget.value) || 0)}
                  />
                  <span class="min-w-0 truncate text-faint" title={order().join('\n')}>
                    {order()
                      .slice(0, 3)
                      .map((t, i) => `${seriesStart() + i} ${t}`)
                      .join(', ')}
                    {n > 3 ? ', …' : ''}
                  </span>
                </label>
              </Row>
              <Row
                label="Publisher"
                changed={draft.publisher !== undefined}
                onClear={
                  (v().publisher.mixed || v().publisher.value != null) && draft.publisher !== ''
                    ? () => setDraft('publisher', '')
                    : undefined
                }
                onRevert={() => setDraft('publisher', undefined)}
                hint={mixedHint(v().publisher, 'publisher')}
              >
                <Autocomplete
                  class={input}
                  placeholder={v().publisher.mixed ? 'Mixed' : ''}
                  options={names()?.publishers ?? []}
                  value={shown('publisher', v().publisher, (p) => p)}
                  onInput={(t) => set('publisher', t, v().publisher.value ?? '')}
                />
              </Row>
              <Row
                label="Published"
                changed={draft.published !== undefined}
                onClear={
                  (v().published.mixed || v().published.value != null) && draft.published !== ''
                    ? () => setDraft('published', '')
                    : undefined
                }
                onRevert={() => setDraft('published', undefined)}
                hint={mixedHint(v().published, 'published')}
              >
                <input
                  class={input + ' w-48'}
                  type="date"
                  value={shown('published', v().published, (d) => d)}
                  onInput={(e) => set('published', e.currentTarget.value, v().published.value ?? '')}
                />
              </Row>
              <Row
                label="Rating"
                changed={draft.rating !== undefined}
                onRevert={() => setDraft('rating', undefined)}
                hint={v().rating.mixed && draft.rating === undefined ? 'These books differ.' : ''}
              >
                <div class="flex h-8 items-center gap-0.5" role="radiogroup" aria-label="Rating">
                  <For each={[1, 2, 3, 4, 5]}>
                    {(s) => (
                      <button
                        type="button"
                        role="radio"
                        aria-checked={stars() === s}
                        aria-label={`${s} star${s > 1 ? 's' : ''}`}
                        class="cursor-pointer px-0.5 text-lg leading-none"
                        classList={{ 'text-star': s <= stars(), 'text-faint hover:text-star': s > stars() }}
                        onClick={() => setDraft('rating', s * 2)}
                      >
                        ★
                      </button>
                    )}
                  </For>
                  <button
                    type="button"
                    class="ml-2 cursor-pointer text-xs text-muted hover:text-fg"
                    onClick={() => setDraft('rating', 0)}
                  >
                    No rating
                  </button>
                </div>
              </Row>
              <Row
                label="Languages"
                changed={draft.languages !== undefined}
                onClear={
                  (v().languages.mixed || v().languages.value != null) && draft.languages !== ''
                    ? () => setDraft('languages', '')
                    : undefined
                }
                onRevert={() => setDraft('languages', undefined)}
                hint={mixedHint(v().languages, 'languages') || 'ISO codes, comma-separated (eng, fra).'}
              >
                <input
                  class={input}
                  placeholder={v().languages.mixed ? 'Mixed' : ''}
                  value={shown('languages', v().languages, (l) => l.join(', '))}
                  onInput={(e) => set('languages', e.currentTarget.value, v().languages.value?.join(', ') ?? '')}
                />
              </Row>
              <Row
                label="Add tags"
                changed={splitList(addTags()).length > 0}
                onRevert={() => setAddTags('')}
                hint="Added to every book; their other tags stay."
              >
                <Autocomplete
                  class={input}
                  separator=","
                  options={names()?.tags ?? []}
                  value={addTags()}
                  onInput={setAddTags}
                />
              </Row>
              <Show when={v().tags.length}>
                <Row
                  label="Remove tags"
                  changed={removeTags().size > 0}
                  onRevert={() => setRemoveTags(new Set<string>())}
                  hint="Click a tag to remove it from every book that has it."
                >
                  <div class="flex flex-wrap gap-1.5 pt-1">
                    <For each={v().tags}>
                      {(t) => {
                        const on = () => removeTags().has(t.name);
                        return (
                          <button
                            type="button"
                            aria-pressed={on()}
                            class="cursor-pointer rounded-chip bg-element px-2.5 py-[3px] text-xs hover:bg-hover aria-pressed:bg-danger/15 aria-pressed:text-danger aria-pressed:line-through"
                            onClick={() => {
                              const next = new Set(removeTags());
                              if (!next.delete(t.name)) next.add(t.name);
                              setRemoveTags(next);
                            }}
                          >
                            {t.name}
                            <span class="ml-1 text-faint tabular-nums">
                              {t.books === n ? 'all' : `${t.books}/${n}`}
                            </span>
                          </button>
                        );
                      }}
                    </For>
                  </div>
                </Row>
              </Show>
              <For each={bulkFields()}>
                {(f) => {
                  const changed = () => f.key in customDraft;
                  const shared = () => sharedCustom(f.key);
                  return (
                    <Row
                      label={f.name}
                      changed={changed()}
                      onRevert={() => setCustomDraft(f.key, undefined as never)}
                      onClear={
                        shared()?.value != null || shared()?.mixed ? () => setCustomDraft(f.key, null) : undefined
                      }
                      hint={
                        changed() && customDraft[f.key] === null
                          ? 'Will be cleared on every book.'
                          : shared()?.mixed && !changed()
                            ? 'These books differ; leave it to keep each one’s.'
                            : customFieldHint(f)
                      }
                    >
                      <CustomFieldInput
                        field={f}
                        inputClass={input}
                        value={changed() ? customDraft[f.key]! : (shared()?.value ?? null)}
                        mixed={shared()?.mixed && !changed()}
                        onChange={(v) => setCustom(f.key, v)}
                      />
                    </Row>
                  );
                }}
              </For>
            </div>
          )}
        </Show>

        <div class="flex items-center justify-end gap-2 border-t border-line px-6 py-3">
          <p
            class="mr-auto text-xs select-text"
            classList={{ 'text-danger': !!error(), 'text-faint': !error() }}
            role="status"
          >
            {error() ??
              (changes().length
                ? `Changes ${changes().join(', ')} on ${plural(n, 'book', 'books')}.`
                : 'Nothing changed yet.')}
          </p>
          <button type="button" class={secondaryButton} onClick={() => closeBulkEditor()}>
            Cancel
          </button>
          <button type="submit" disabled={saving() || !changes().length} class={primaryButton}>
            {saving() ? 'Saving…' : `Save ${n} Books`}
          </button>
        </div>
      </form>
    </dialog>
  );
}
