// Finding names that are probably the same thing spelt differently, and
// names that hold several things, for the browse panel's tidy-up: "Sci-Fi",
// "scifi" and "SF" are one tag; "scfi" is a typo of it; "Fiction / Fantasy"
// and "Dragons;epic" are two each. Pure functions over the facet lists.

import type { FacetValue } from './api';

/** Names that look like one another, with the spelling to keep. */
export type TidyGroup = {
  members: FacetValue[];
  target: string;
  /** `variant`: the same letters, differing in case, accents or punctuation. `spelling`: a letter or two apart. */
  reason: 'variant' | 'spelling';
};

/** Letters and digits only, lower-case, accents dropped, "&" as "and". */
export function normalized(name: string): string {
  return name
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .toLowerCase()
    .replace(/&/g, 'and')
    .replace(/[^\p{L}\p{N}]+/gu, '');
}

/** Optimal string alignment distance (insert, delete, replace, swap neighbours), capped at `max + 1`. */
export function distance(a: string, b: string, max: number): number {
  if (Math.abs(a.length - b.length) > max) return max + 1;
  const rows: number[][] = [];
  for (let i = 0; i <= a.length; i++) {
    rows[i] = [i];
    for (let j = 1; j <= b.length; j++) rows[i]![j] = i === 0 ? j : 0;
  }
  for (let i = 1; i <= a.length; i++) {
    let best = Infinity;
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      let d = Math.min(rows[i - 1]![j]! + 1, rows[i]![j - 1]! + 1, rows[i - 1]![j - 1]! + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d = Math.min(d, rows[i - 2]![j - 2]! + 1);
      rows[i]![j] = d;
      best = Math.min(best, d);
    }
    if (best > max) return max + 1;
  }
  return rows[a.length]![b.length]!;
}

/** How far apart two normalized names may be and still count as one: none when short, a letter from 4, two from 8. */
const allowance = (len: number) => (len >= 8 ? 2 : len >= 4 ? 1 : 0);

/** The groups worth merging, biggest first. Each member appears in one group at most. */
export function tidyGroups(values: readonly FacetValue[]): TidyGroup[] {
  const keys = values.map((v) => normalized(v.value));
  const parent = values.map((_, i) => i);
  const find = (i: number): number => (parent[i] === i ? i : (parent[i] = find(parent[i]!)));
  const union = (a: number, b: number) => {
    parent[find(a)] = find(b);
  };
  // Same letters: one group, whatever the case or punctuation.
  const byKey = new Map<string, number>();
  keys.forEach((k, i) => {
    if (!k) return;
    const first = byKey.get(k);
    if (first === undefined) byKey.set(k, i);
    else union(i, first);
  });
  // A letter or two apart: a typo of each other, most likely. Different
  // numbers are different things ("English 8", "English 10"), never typos.
  const digits = (k: string) => k.replace(/\D/g, '');
  const distinct = [...byKey.entries()];
  for (let x = 0; x < distinct.length; x++) {
    const [ka, ia] = distinct[x]!;
    for (let y = x + 1; y < distinct.length; y++) {
      const [kb, ib] = distinct[y]!;
      const max = Math.min(allowance(ka.length), allowance(kb.length));
      if (max > 0 && digits(ka) === digits(kb) && distance(ka, kb, max) <= max) union(ia, ib);
    }
  }
  const groups = new Map<number, FacetValue[]>();
  values.forEach((v, i) => {
    const root = find(i);
    groups.set(root, [...(groups.get(root) ?? []), v]);
  });
  return [...groups.values()]
    .filter((g) => g.length > 1)
    .map((members) => {
      const sorted = [...members].sort((a, b) => b.count - a.count || score(b.value) - score(a.value));
      const reason = new Set(members.map((m) => normalized(m.value))).size === 1 ? 'variant' : 'spelling';
      return { members: sorted, target: sorted[0]!.value, reason } as TidyGroup;
    })
    .sort((a, b) => total(b) - total(a));
}

const total = (g: TidyGroup) => g.members.reduce((n, m) => n + m.count, 0);

/** A name that holds several, and its parts. */
export type Split = { from: FacetValue; into: string[] };

/**
 * Separators between values packed into one name: `;` and `|` anywhere, a
 * comma, and `/` or a dash with space on both sides ("Fiction / Fantasy",
 * "Fiction - Mystery", but not "Sci-Fi" or "1/2").
 */
const SEPARATOR = /\s*[;|,]\s*|\s+[/\-–—]\s+/u;

/** The parts a name splits into, when there are at least two. */
export function splitParts(name: string): string[] | null {
  const seen = new Set<string>();
  const parts = name
    .split(SEPARATOR)
    .map((p) => p.trim())
    .filter((p) => p && !seen.has(p.toLowerCase()) && seen.add(p.toLowerCase()));
  return parts.length > 1 ? parts : null;
}

/** The names that hold several values, most-used first. */
export function tidySplits(values: readonly FacetValue[]): Split[] {
  return values
    .map((from) => ({ from, into: splitParts(from.value) }))
    .filter((s): s is Split => s.into !== null)
    .sort((a, b) => b.from.count - a.from.count);
}
/** Between equally common spellings, prefer a capitalised one, then a shorter one. */
const score = (name: string) => (/\p{Lu}/u.test(name) ? 10 : 0) - name.length / 100;
