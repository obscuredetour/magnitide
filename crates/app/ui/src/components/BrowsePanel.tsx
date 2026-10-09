import { createMemo, createSignal, For, Show } from 'solid-js';
import Sparkles from 'lucide-solid/icons/sparkles';
import X from 'lucide-solid/icons/x';

import { api, errorMessage, type FacetValue } from '../api';
import { type Category, facets, isActive, toggleSearch, valueQuery } from '../browse';
import { primaryButton, secondaryButton, smallButton } from '../buttons';
import { confirm } from '../confirm';
import { library, refresh, setEditRevision } from '../library';
import { tidyGroups, type TidyGroup } from '../tidy';
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

/**
 * A category opened wide beside the sidebar: every value with its count,
 * a filter, and (for names that can be merged) tidying up: pick values and
 * merge or rename them, or let the broom find the ones that are one thing
 * spelt several ways.
 */
export function BrowsePanel(props: Props) {
  const c = () => props.category;
  const values = (): FacetValue[] => facets()?.[c().key] ?? [];
  const [filter, setFilter] = createSignal('');
  const [byName, setByName] = createSignal(false);
  const [selected, setSelected] = createSignal<ReadonlySet<string>>(new Set());
  const [busy, setBusy] = createSignal(false);
  /** The broom's proposals, while reviewing them. */
  const [groups, setGroups] = createSignal<TidyGroup[] | null>(null);
  const [skipped, setSkipped] = createSignal<ReadonlySet<number>>(new Set());
  const [targets, setTargets] = createSignal<ReadonlyMap<number, string>>(new Map());
  const canMerge = () => c().merge !== null && library()?.kind === 'magnitide';

  const shown = createMemo(() => {
    const f = filter().trim().toLowerCase();
    const list = f ? values().filter((v) => c().show(v.value).toLowerCase().includes(f)) : values().slice();
    if (byName())
      list.sort((a, b) => c().show(a.value).localeCompare(c().show(b.value), undefined, { sensitivity: 'base' }));
    return list;
  });

  const toggle = (value: string) => {
    const next = new Set(selected());
    if (!next.delete(value)) next.add(value);
    setSelected(next);
  };

  /** Merge (or rename, when it's one) the selected values under a name asked for. */
  const mergeSelected = async () => {
    const names = values()
      .filter((v) => selected().has(v.value))
      .map((v) => v.value);
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
    await apply([[names, name().trim()]]);
  };

  /** Run merges one after another, then show what happened. */
  const apply = async (merges: [string[], string][]) => {
    const field = c().merge;
    if (!field) return;
    setBusy(true);
    let books = 0;
    try {
      for (const [from, to] of merges) books += (await api.mergeValues(field, from, to)).books;
      await refresh();
      setEditRevision((n) => n + 1);
      setSelected(new Set<string>());
      setGroups(null);
      toast(
        merges.length === 1
          ? `“${merges[0]![1]}”: ${plural(books, 'book', 'books')} changed`
          : `Merged ${plural(merges.length, 'group', 'groups')}: ${plural(books, 'book', 'books')} changed`,
      );
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setBusy(false);
    }
  };

  const sweep = () => {
    const g = tidyGroups(values());
    setSkipped(new Set<number>());
    setTargets(new Map<number, string>());
    setGroups(g);
    if (!g.length) toast(`Nothing to tidy: no two ${c().label.toLowerCase()} look alike`);
  };
  const applyGroups = () => {
    const merges: [string[], string][] = (groups() ?? [])
      .map((g, i) => [g, i] as const)
      .filter(([, i]) => !skipped().has(i))
      .map(([g, i]) => [g.members.map((m) => m.value), targets().get(i) ?? g.target]);
    void apply(merges);
  };
  const chosen = () => (groups()?.length ?? 0) - skipped().size;

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
        when={groups()}
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
              <Show when={canMerge()}>
                <button
                  class={smallButton}
                  title={`Find ${c().label.toLowerCase()} that are one thing spelt several ways`}
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
                    <Show when={canMerge()}>
                      <input
                        type="checkbox"
                        class="size-3.5 accent-accent"
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
              <div class="flex items-center gap-2 border-t border-line px-4 py-2.5 text-xs">
                <span class="text-muted">{plural(selected().size, 'selected', 'selected')}</span>
                <button class={`${smallButton} ml-auto`} onClick={() => setSelected(new Set<string>())}>
                  Clear
                </button>
                <button
                  class={`${smallButton} border-transparent bg-accent text-on-accent hover:brightness-110`}
                  disabled={busy()}
                  onClick={() => void mergeSelected()}
                >
                  {selected().size === 1 ? 'Rename…' : 'Merge…'}
                </button>
              </div>
            </Show>
            <Show when={!canMerge() && c().merge && library()?.kind === 'calibre'}>
              <p class="border-t border-line px-4 py-2.5 text-xs text-faint">
                Tidying up is for Magnitide libraries; calibre libraries are read-only.
              </p>
            </Show>
          </>
        }
      >
        {(gs) => (
          <>
            <p class="px-4 pb-2 text-xs text-muted">
              {gs().length
                ? `${plural(gs().length, 'group', 'groups')} of ${c().label.toLowerCase()} that look like one thing. Untick any to keep apart, and pick the spelling to keep.`
                : `No two ${c().label.toLowerCase()} look alike.`}
            </p>
            <ul class="min-h-0 flex-1 space-y-1.5 overflow-y-auto px-3 pb-2" role="list">
              <For each={gs()}>
                {(g, i) => (
                  <li
                    class="rounded-card border border-line bg-bg/60 p-2.5"
                    classList={{ 'opacity-50': skipped().has(i()) }}
                  >
                    <div class="flex items-center gap-2">
                      <input
                        type="checkbox"
                        class="size-3.5 accent-accent"
                        aria-label="Merge this group"
                        checked={!skipped().has(i())}
                        onChange={(e) => {
                          const next = new Set(skipped());
                          if (e.currentTarget.checked) next.delete(i());
                          else next.add(i());
                          setSkipped(next);
                        }}
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
                        {g.reason === 'variant' ? 'variant' : 'spelling'}
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
            </ul>
            <div class="flex items-center gap-2 border-t border-line px-4 py-2.5">
              <button class={`${secondaryButton} ml-auto`} onClick={() => setGroups(null)}>
                Cancel
              </button>
              <button class={primaryButton} disabled={!chosen() || busy()} onClick={applyGroups}>
                {busy() ? 'Merging…' : `Merge ${plural(chosen(), 'Group', 'Groups')}`}
              </button>
            </div>
          </>
        )}
      </Show>
    </aside>
  );
}
