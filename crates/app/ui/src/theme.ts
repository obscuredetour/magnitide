// Theme and accent selection. Built-in themes live in themes/ (see
// themes/index.css) and are registered here; add-ons are theme files loaded
// through Rust (see docs/themes.md).

import { createSignal } from 'solid-js';

import { api, errorMessage, type ThemeAddon, type ThemeAddons } from './api';
import { storage } from './storage';

/** A palette accent: its variable name (`--mauve`) and how a theme labels it. */
export type Accent = { name: string; label: string };

/** Catppuccin's accents, in its order. */
const CATPPUCCIN_ACCENTS: Accent[] = [
  'Rosewater',
  'Flamingo',
  'Pink',
  'Mauve',
  'Red',
  'Maroon',
  'Peach',
  'Yellow',
  'Green',
  'Teal',
  'Sky',
  'Sapphire',
  'Blue',
  'Lavender',
].map((label) => ({ name: label.toLowerCase(), label }));

/** Magnitide's own themes: their coral first, then Catppuccin's. */
const MAGNITIDE_ACCENTS: Accent[] = [{ name: 'coral', label: 'Coral' }, ...CATPPUCCIN_ACCENTS];

/** Dracula's accents, by its own names (mapped onto the palette's in dracula.css). */
const DRACULA_ACCENTS: Accent[] = [
  { name: 'red', label: 'Red' },
  { name: 'peach', label: 'Orange' },
  { name: 'yellow', label: 'Yellow' },
  { name: 'green', label: 'Green' },
  { name: 'sky', label: 'Cyan' },
  { name: 'mauve', label: 'Purple' },
  { name: 'pink', label: 'Pink' },
];

/** A theme the app can switch to: built in, or an add-on. */
export type Theme = {
  id: string;
  label: string;
  appearance: 'light' | 'dark';
  /** What the accent picker offers. */
  accents: readonly Accent[];
  /** Add-ons: who made it and the file it came from. */
  author?: string | null;
  file?: string;
};

export const BUILTIN_THEMES: readonly Theme[] = [
  { id: 'light', label: 'Light', appearance: 'light', accents: MAGNITIDE_ACCENTS },
  { id: 'dark', label: 'Dark', appearance: 'dark', accents: MAGNITIDE_ACCENTS },
  { id: 'sepia', label: 'Sepia', appearance: 'light', accents: MAGNITIDE_ACCENTS },
  { id: 'catppuccin-latte', label: 'Catppuccin Latte', appearance: 'light', accents: CATPPUCCIN_ACCENTS },
  { id: 'catppuccin-frappe', label: 'Catppuccin Frappé', appearance: 'dark', accents: CATPPUCCIN_ACCENTS },
  { id: 'catppuccin-macchiato', label: 'Catppuccin Macchiato', appearance: 'dark', accents: CATPPUCCIN_ACCENTS },
  { id: 'catppuccin-mocha', label: 'Catppuccin Mocha', appearance: 'dark', accents: CATPPUCCIN_ACCENTS },
  { id: 'dracula', label: 'Dracula', appearance: 'dark', accents: DRACULA_ACCENTS },
  { id: 'alucard', label: 'Alucard', appearance: 'light', accents: DRACULA_ACCENTS },
];

// ---------- Add-ons (theme files; see docs/themes.md) ----------

const SAFE_NAME = /^[a-z][a-z0-9-]*$/;
/** Rust has already checked every value; this is only a second line of defence. */
const SAFE_VALUE = /^[a-z0-9#%().,\s-]+$/i;

function addonTheme(a: ThemeAddon): Theme {
  return {
    id: a.id,
    label: a.name,
    appearance: a.appearance === 'light' ? 'light' : 'dark',
    accents: a.accents,
    author: a.author,
    file: a.file,
  };
}

/**
 * One rule per add-on, setting its palette and role overrides. Matched on
 * both attributes so it outranks the shared role defaults wherever this
 * stylesheet lands in the page.
 */
function addonCss(list: readonly ThemeAddon[]): string {
  return list
    .filter((a) => SAFE_NAME.test(a.id))
    .map((a) => {
      const vars = a.variables
        .filter(([k, v]) => SAFE_NAME.test(k) && SAFE_VALUE.test(v))
        .map(([k, v]) => `  --${k}: ${v};`)
        .join('\n');
      return `[data-theme='${a.id}'][data-appearance] {\n${vars}\n}`;
    })
    .join('\n');
}

function cachedAddons(): ThemeAddon[] {
  try {
    const parsed: unknown = JSON.parse(storage.get('themeAddons') ?? '[]');
    return Array.isArray(parsed) ? (parsed as ThemeAddon[]) : [];
  } catch {
    return [];
  }
}

const [addons, setAddons] = createSignal<readonly ThemeAddon[]>(cachedAddons());

/** Every theme: built in, then add-ons. */
export const themes = (): readonly Theme[] => [...BUILTIN_THEMES, ...addons().map(addonTheme)];

/** Install add-ons: their stylesheet, the registry, and a cached copy so they show at the next start before Rust answers. */
function installAddons(list: readonly ThemeAddon[]): void {
  let style = document.getElementById('theme-addons');
  if (!style) {
    style = document.createElement('style');
    style.id = 'theme-addons';
    document.head.append(style);
  }
  style.textContent = addonCss(list);
  setAddons(list);
  storage.set('themeAddons', JSON.stringify(list));
  apply();
}

// ---------- Choosing ----------

/** `system` follows the OS light/dark setting. */
export type ThemeChoice = string;

/** The attributes that scope an element to a theme (for swatches and previews). */
export function themeAttrs(id: string): { 'data-theme': string; 'data-appearance': string } {
  const t = themes().find((x) => x.id === id) ?? BUILTIN_THEMES[0]!;
  return { 'data-theme': t.id, 'data-appearance': t.appearance };
}

const systemDark = window.matchMedia('(prefers-color-scheme: dark)');
const [choice, setChoice] = createSignal<ThemeChoice>(storage.get('theme') ?? 'system');
const [systemIsDark, setSystemIsDark] = createSignal(systemDark.matches);

/** Each theme's chosen accent (a palette name); a theme without one uses its own. */
function loadAccents(): Record<string, string> {
  try {
    const parsed: unknown = JSON.parse(storage.get('accents') ?? '{}');
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, string>) : {};
  } catch {
    return {};
  }
}
const [accents, setAccents] = createSignal<Record<string, string>>(loadAccents());

/** The theme in use: `system` resolved, and a removed add-on falls back to it. */
export const currentTheme = (): string => {
  const c = choice();
  if (c !== 'system' && themes().some((t) => t.id === c)) return c;
  return systemIsDark() ? 'dark' : 'light';
};

/** The accents the theme in use offers. */
export const currentAccents = (): readonly Accent[] => themes().find((t) => t.id === currentTheme())?.accents ?? [];

/** The accent chosen for the theme in use, or null for the theme's own. */
export const accentChoice = (): string | null => {
  const a = accents()[currentTheme()];
  return a && currentAccents().some((x) => x.name === a) ? a : null;
};

function apply(): void {
  const root = document.documentElement;
  Object.assign(root.dataset, { theme: currentTheme(), appearance: themeAttrs(currentTheme())['data-appearance'] });
  // The chosen accent, on <html> alone: elements scoped to a theme (swatches) keep their own.
  const a = accentChoice();
  if (a && SAFE_NAME.test(a)) {
    root.dataset.accent = a;
    root.style.setProperty('--accent', `var(--${a})`);
  } else {
    delete root.dataset.accent;
    root.style.removeProperty('--accent');
  }
}

systemDark.addEventListener('change', () => {
  setSystemIsDark(systemDark.matches);
  apply();
});
installAddons(addons()); // at import time, before the first render (from the cache)

/** What the user chose ('system' or a theme id). */
export const themeChoice = (): ThemeChoice => {
  const c = choice();
  return c === 'system' || themes().some((t) => t.id === c) ? c : 'system';
};

export function setTheme(c: ThemeChoice): void {
  setChoice(c);
  storage.set('theme', c);
  apply();
}

/** Choose the theme in use's accent (a palette name), or null for its own. */
export function setAccent(name: string | null): void {
  const next = { ...accents() };
  if (name) next[currentTheme()] = name;
  else delete next[currentTheme()];
  setAccents(next);
  storage.set('accents', JSON.stringify(next));
  apply();
}

/** The latest add-on load: problems with theme files, and the folder. */
const [addonReport, setAddonReport] = createSignal<ThemeAddons | null>(null);
export { addonReport };

/** (Re)load add-ons from the themes folder. */
export async function loadThemeAddons(): Promise<void> {
  const r = await api.themeAddons();
  setAddonReport(r);
  installAddons(r.themes);
}

/** Pick a theme file and add it; returns an error message, if any. */
export async function addThemeFile(): Promise<string | null> {
  const path = await api.chooseThemeFile();
  if (!path) return null;
  try {
    const r = await api.addThemeFile(path);
    setAddonReport(r);
    installAddons(r.themes);
    return null;
  } catch (err) {
    return errorMessage(err);
  }
}
