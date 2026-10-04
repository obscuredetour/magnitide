// Theme selection. Themes themselves live in themes.css.

import { createSignal } from 'solid-js';

import { storage } from './storage';

export const THEMES = [
  { id: 'light', label: 'Light' },
  { id: 'dark', label: 'Dark' },
  { id: 'sepia', label: 'Sepia' },
  { id: 'catppuccin-latte', label: 'Catppuccin Latte' },
  { id: 'catppuccin-frappe', label: 'Catppuccin Frappé' },
  { id: 'catppuccin-macchiato', label: 'Catppuccin Macchiato' },
  { id: 'catppuccin-mocha', label: 'Catppuccin Mocha' },
  { id: 'dracula', label: 'Dracula' },
  { id: 'alucard', label: 'Alucard' },
] as const;

export type ThemeName = (typeof THEMES)[number]['id'];
/** `system` follows the OS light/dark setting. */
export type ThemeChoice = 'system' | ThemeName;

const isChoice = (s: string | null): s is ThemeChoice => s === 'system' || THEMES.some((t) => t.id === s);

const systemDark = window.matchMedia('(prefers-color-scheme: dark)');
const stored = storage.get('theme');
const [choice, setChoice] = createSignal<ThemeChoice>(isChoice(stored) ? stored : 'system');

function apply(): void {
  const c = choice();
  document.documentElement.dataset.theme = c === 'system' ? (systemDark.matches ? 'dark' : 'light') : c;
}

systemDark.addEventListener('change', apply);
apply(); // at import time, before the first render

export const themeChoice = choice;

export function setTheme(c: ThemeChoice): void {
  setChoice(c);
  storage.set('theme', c);
  apply();
}
