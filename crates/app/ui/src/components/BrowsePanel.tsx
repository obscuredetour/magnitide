import { createMemo, createSignal, For, Show } from 'solid-js';
import Sparkles from 'lucide-solid/icons/sparkles';
import X from 'lucide-solid/icons/x';

import { api, errorMessage, type FacetValue, type TagSplit } from '../api';
import { type Category, facets, isActive, toggleSearch, valueQuery } from '../browse';
import { primaryButton, secondaryButton, smallButton } from '../buttons';
import { confirm } from '../confirm';
import { library, refresh, setEditRevision } from '../library';
import { type Split, splitParts, tidyGroups, type TidyGroup, tidySplits } from '../tidy';
import { toast } from '../toast';
import { plural } from '../writeback';
import { CATEGORY_ICONS } from './Browse';
import { Icon } from './Icon';
import { Select } from './Select';

type Props = {
  category: Category;
  onClose(): void;
};

const glass = 'cursor-pointer bg-element-hover/70 shadow-card ring-1 ring-line backdrop-blur-sm hover:bg-element';
const card = 'rounded-card border border-line bg-bg/60 p-2.5';
const tick = 'size-3.5 accent-accent';

/** What the broom proposes: names to merge, and names to split. */
type Proposals = { groups: TidyGroup[]; splits: Split[] };

/**
 * A category opened wide beside the sidebar: every value with its count,
 * a filter, and (for names that can be changed) tidying up: pick values and
 * merge, rename, split or delete them, or let the broom find the ones that
 * are one thing spelt several ways and the ones that hold several things.
 */
export function BrowsePanel(props: Props) {
  const c = () => props.category;
  const values = (): FacetValue[] => facets()?.[c().key] ?? [];
  const [filter, setFilter] = createSignal('');
  const [byName, setByName] = createSignal(false);
  const [selected, setSelected] = createSignal<ReadonlySet<string>>(new Set());
  const [busy, setBusy] = createSignal(false);
  /** The broom's proposals, while reviewing them. */
  const [proposals, setProposals] = createSignal<Proposals | null>(null);
  /** Proposals unticked, by key ("m3", "s1"); and the spelling picked for a merge group. */
  const [skipped, setSkipped] = createSignal<ReadonlySet<string>>(new Set());
  const [targets, setTargets] = createSignal<ReadonlyMap<number, string>>(new Map());
  const editable = () => c().merge !== null && library()?.kind === 'magnitide';
  const isTags = () => c().key === 'tags';

  const shown = createMemo(() => {
    const f = filter().trim().toLowerCase();
    const list = f ? values().filter((v) => c().show(v.value).toLowerCase().includes(f)) : values().slice();
    if (byName())
      list.sort((a, b) => c().show(a.value).localeCompare(c().show(b.value), undefined, { sensitivity: 'base' }));
    return list;
  });
  const chosen = () => values().filter((v) => selected().has(v.value));
  /** Of the selected tags, those that hold several. */
  const splittable = () => (isTags() ? chosen().filter((v) => splitParts(v.value)) : []);

  const toggle = (value: string) => {
    const next = new Set(selected());
    if (!next.delete(value)) next.add(value);
    setSelected(next);
  };

  /** Run changes one after another, then show what happened. */
  const run = async (work: () => Promise<number>, done: (books: number) => string) => {
    setBusy(true);
    try {
      const books = await work();
      await refresh();
      setEditRevision((n) => n + 1);
      setSelected(new Set<string>());
      setProposals(null);
      toast(done(books));
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setBusy(false);
    }
  };
  const field = () => c().merge!;

  /** Merge (or rename, when it's one) the selected values under a name asked for. */
  const mergeSelected = async () => {
    const names = chosen().map((v) => v.value);
    if (!names.length) return;
    const [name, setName] = createSignal(names[0]!);
    const ok = await confirm({
      title:
        names.length === 1 ? `Rename ${c().one}` : `Merge ${plural(names.length, c().one, c().label.toLowerCase())}`,
      message: () => (
        <>
          <input
            class="h-8 w-full rounded-control border border-line bg-field px-2.5 text-fg outline-none select-text focus:border-focus focus:ring-3 focus:ring-focus/20"
            value={name()}
            aria-label="Name"
            autofocus
            onInput={(e) => setName(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                e.currentTarget.closest('dialog')?.querySelector<HTMLButtonElement>('[data-confirm]')?.click();
              }
            }}
          />
          <p class="mt-2 text-xs text-muted">
            {names.length === 1 ? 'Every book with ' : 'Every book with any of '}
            {names.map((n) => `“${n}”`).join(', ')} gets this name instead.
          </p>
        </>
      ),
      confirm: names.length === 1 ? 'Rename' : 'Merge',
    });
    if (!ok || !name().trim()) return;
    const to = name().trim();
    await run(
      async () => (await api.mergeValues(field(), names, to)).books,
      (books) => `“${to}”: ${plural(books, 'book', 'books')} changed`,
    );
  };

  /** Split the selected tags that hold several into their parts. */
  const splitSelected = async () => {
    const splits: TagSplit[] = splittable().map((v) => ({ from: v.value, into: splitParts(v.value)! }));
    if (!splits.length) return;
    const ok = await confirm({
      title: `Split ${plural(splits.length, 'tag', 'tags')}`,
      message: () => (
        <ul class="max-h-60 space-y-1.5 overflow-y-auto text-xs">
          <For each={splits}>
            {(s) => (
              <li>
                <span class="text-muted">{s.from}</span> → {s.into.join(' · ')}
              </li>
            )}
          </For>
        </ul>
      ),
      confirm: 'Split',
    });
    if (!ok) return;
    await run(
      async () => (await api.splitTags(splits)).books,
      (books) => `Split ${plural(splits.length, 'tag', 'tags')}: ${plural(books, 'book', 'books')} changed`,
    );
  };

  /** Take the selected values off every book. */
  const deleteSelected = async () => {
    const names = chosen().map((v) => v.value);
    if (!names.length) return;
    const books = chosen().reduce((n, v) => n + v.count, 0);
    const ok = await confirm({
      title: `Delete ${plural(names.length, c().one, c().label.toLowerCase())}`,
      message: () => (
        <p class="text-xs">
          {names.map((n) => `“${n}”`).join(', ')} {names.length === 1 ? 'comes' : 'come'} off{' '}
          {plural(books, 'book', 'books')}. The books stay.
        </p>
      ),
      confirm: 'Delete',
      danger: true,
    });
    if (!ok) return;
    await run(
      async () => (await api.removeValues(field(), names)).books,
      (b) => `Deleted ${plural(names.length, c().one, c().label.toLowerCase())}: ${plural(b, 'book', 'books')} changed`,
    );
  };

  const sweep = () => {
    const p = { groups: tidyGroups(values()), splits: isTags() ? tidySplits(values()) : [] };
    setSkipped(new Set<string>());
    setTargets(new Map<number, string>());
    setProposals(p);
    if (!p.groups.length && !p.splits.length) toast(`Nothing to tidy among the ${c().label.toLowerCase()}`);
  };
  const skip = (key: string, on: boolean) => {
    const next = new Set(skipped());
    if (on) next.delete(key);
    else next.add(key);
    setSkipped(next);
  };
  const chosenProposals = () => {
    const p = proposals();
    if (!p) return { merges: [] as [string[], string][], splits: [] as TagSplit[] };
    const merges = p.groups
      .map((g, i): [string[], string] | null =>
        skipped().has(`m${i}`) ? null : [g.members.map((m) => m.value), targets().get(i) ?? g.target],
      )
      .filter((m): m is [string[], string] => m !== null);
    const splits = p.splits
      .filter((_, i) => !skipped().has(`s${i}`))
      .map((s) => ({ from: s.from.value, into: s.into }));
    return { merges, splits };
  };
  const applyProposals = () => {
    const { merges, splits } = chosenProposals();
    void run(
      async () => {
        let books = 0;
        for (const [from, to] of merges) books += (await api.mergeValues(field(), from, to)).books;
        if (splits.length) books += (await api.splitTags(splits)).books;
        return books;
      },
      (books) =>
        `Tidied ${plural(merges.length + splits.length, 'group', 'groups')}: ${plural(books, 'book', 'books')} changed`,
    );
  };
  const chosenCount = () => chosenProposals().merges.length + chosenProposals().splits.length;

  return (
    <aside
      class="relative flex w-[380px] flex-none animate-slide-in flex-col border-r border-line bg-panel select-text"
      aria-label={`${c().label} panel`}
    >
      <div class="flex items-center gap-2 px-4 pt-3 pb-2">
        <Icon of={CATEGORY_ICONS[c().key]} class="text-muted" />
        <h2 class="font-serif text-lg font-semibold">{c().label}</h2>
        <span class="text-xs text-muted tabular-nums">{values().length.toLocaleString()}</span>
        <button
          class={`ml-auto grid size-7 place-items-center rounded-full ${glass}`}
          title="Close (Esc)"
          aria-label="Close"
          onClick={() => props.onClose()}
        >
          <Icon of={X} class="text-muted" />
        </button>
      </div>

      <Show
        when={proposals()}
        fallback={
          <>
            <div class="flex items-center gap-2 px-4 pb-2">
              <input
                class="h-7 min-w-0 flex-1 rounded-control border border-line bg-field px-2.5 text-sm outline-none select-text focus:border-focus"
                placeholder={`Filter ${c().label.toLowerCase()}`}
                aria-label={`Filter ${c().label.toLowerCase()}`}
                value={filter()}
                onInput={(e) => setFilter(e.currentTarget.value)}
                onKeyDown={(e) => e.stopPropagation()}
              />
              <button
                class={smallButton}
                title={
                  byName() ? 'Sorted by name; click for most common first' : 'Most common first; click to sort by name'
                }
                onClick={() => setByName(!byName())}
              >
                {byName() ? 'A–Z' : 'Count'}
              </button>
              <Show when={editable()}>
                <button
                  class={smallButton}
                  title={`Find ${c().label.toLowerCase()} that are one thing spelt several ways${isTags() ? ', and tags that hold several' : ''}`}
                  disabled={busy()}
                  onClick={sweep}
                >
                  <Icon of={Sparkles} size={12} />
                  Tidy Up
                </button>
              </Show>
            </div>
            <ul class="min-h-0 flex-1 overflow-y-auto px-2 pb-2" role="list">
              <For each={shown()}>
                {(v) => (
                  <li class="group flex h-7 items-center gap-2 rounded-control pr-2 pl-2 hover:bg-hover">
                    <Show when={editable()}>
                      <input
                        type="checkbox"
                        class={tick}
                        aria-label={`Select ${c().show(v.value)}`}
                        checked={selected().has(v.value)}
                        onChange={() => toggle(v.value)}
                      />
                    </Show>
                    <button
                      class="min-w-0 flex-1 cursor-default truncate rounded-inset text-left text-sm outline-none hover:text-accent focus-visible:ring-2 focus-visible:ring-focus aria-[current=true]:font-semibold aria-[current=true]:text-accent"
                      title={`Show books with ${c().show(v.value)}`}
                      aria-current={isActive(valueQuery(c(), v.value))}
                      onClick={() => toggleSearch(valueQuery(c(), v.value))}
                    >
                      {c().show(v.value)}
                    </button>
                    <span class="text-xs text-faint tabular-nums">{v.count.toLocaleString()}</span>
                  </li>
                )}
              </For>
              <Show when={!shown().length}>
                <p class="px-2 pt-4 text-center text-sm text-muted">
                  {values().length ? `No ${c().label.toLowerCase()} match.` : `No ${c().label.toLowerCase()} yet.`}
                </p>
              </Show>
            </ul>
            <Show when={selected().size}>
              <div class="flex flex-wrap items-center gap-2 border-t border-line px-4 py-2.5 text-xs">
                <span class="text-muted">{selected().size} selected</span>
                <button class={`${smallButton} ml-auto`} onClick={() => setSelected(new Set<string>())}>
                  Clear
                </button>
                <button
                  class={`${smallButton} text-danger hover:bg-danger/15`}
                  disabled={busy()}
                  onClick={() => void deleteSelected()}
                >
                  Delete…
                </button>
                <Show when={splittable().length}>
                  <button class={smallButton} disabled={busy()} onClick={() => void splitSelected()}>
                    Split…
                  </button>
                </Show>
                <button
                  class={`${smallButton} border-transparent bg-accent text-on-accent hover:brightness-110`}
                  disabled={busy()}
                  onClick={() => void mergeSelected()}
                >
                  {selected().size === 1 ? 'Rename…' : 'Merge…'}
                </button>
              </div>
            </Show>
            <Show when={!editable() && c().merge && library()?.kind === 'calibre'}>
              <p class="border-t border-line px-4 py-2.5 text-xs text-faint">
                Tidying up is for Magnitide libraries; calibre libraries are read-only.
              </p>
            </Show>
          </>
        }
      >
        {(p) => (
          <>
            <p class="px-4 pb-2 text-xs text-muted">
              {p().groups.length || p().splits.length
                ? 'Untick anything to leave as it is. Merges keep the spelling you pick.'
                : `Nothing to tidy among the ${c().label.toLowerCase()}.`}
            </p>
            <ul class="min-h-0 flex-1 space-y-1.5 overflow-y-auto px-3 pb-2" role="list">
              <For each={p().groups}>
                {(g, i) => (
                  <li class={card} classList={{ 'opacity-50': skipped().has(`m${i()}`) }}>
                    <div class="flex items-center gap-2">
                      <input
                        type="checkbox"
                        class={tick}
                        aria-label="Merge this group"
                        checked={!skipped().has(`m${i()}`)}
                        onChange={(e) => skip(`m${i()}`, e.currentTarget.checked)}
                      />
                      <Select
                        class="h-6 min-w-0 flex-1 pl-2 text-xs"
                        wide
                        aria-label="Spelling to keep"
                        value={targets().get(i()) ?? g.target}
                        onChange={(e) => setTargets(new Map(targets()).set(i(), e.currentTarget.value))}
                      >
                        <For each={g.members}>{(m) => <option value={m.value}>{m.value}</option>}</For>
                      </Select>
                      <span
                        class="flex-none rounded-chip bg-element px-1.5 text-[10px] text-muted"
                        title={
                          g.reason === 'variant'
                            ? 'The same letters, differing in case, accents or punctuation'
                            : 'A letter or two apart'
                        }
                      >
                        {g.reason}
                      </span>
                    </div>
                    <p class="mt-1.5 pl-5.5 text-xs text-muted">
                      <For each={g.members}>
                        {(m, j) => (
                          <>
                            {j() > 0 && ' · '}
                            <span classList={{ 'text-fg': m.value === (targets().get(i()) ?? g.target) }}>
                              {m.value} <span class="text-faint tabular-nums">{m.count}</span>
                            </span>
                          </>
                        )}
                      </For>
                    </p>
                  </li>
                )}
              </For>
              <For each={p().splits}>
                {(s, i) => (
                  <li class={card} classList={{ 'opacity-50': skipped().has(`s${i()}`) }}>
                    <div class="flex items-center gap-2">
                      <input
                        type="checkbox"
                        class={tick}
                        aria-label="Split this tag"
                        checked={!skipped().has(`s${i()}`)}
                        onChange={(e) => skip(`s${i()}`, e.currentTarget.checked)}
                      />
                      <span class="min-w-0 flex-1 truncate text-sm" title={s.from.value}>
                        {s.from.value} <span class="text-xs text-faint tabular-nums">{s.from.count}</span>
                      </span>
                      <span
                        class="flex-none rounded-chip bg-element px-1.5 text-[10px] text-muted"
                        title="Several tags in one"
                      >
                        split
                      </span>
                    </div>
                    <p class="mt-1.5 pl-5.5 text-xs text-muted">→ {s.into.join(' · ')}</p>
                  </li>
                )}
              </For>
            </ul>
            <div class="flex items-center gap-2 border-t border-line px-4 py-2.5">
              <button class={`${secondaryButton} ml-auto`} onClick={() => setProposals(null)}>
                Cancel
              </button>
              <button class={primaryButton} disabled={!chosenCount() || busy()} onClick={applyProposals}>
                {busy() ? 'Tidying…' : `Tidy ${plural(chosenCount(), 'Group', 'Groups')}`}
              </button>
            </div>
          </>
        )}
      </Show>
    </aside>
  );
}
