// Keyboard shortcuts as people see them: macOS symbols (⌘E, ⌥←) there,
// spelled out elsewhere (Ctrl+E, Alt+←). The handlers accept ⌘ or Ctrl
// (`e.metaKey || e.ctrlKey`), so "mod" means whichever the platform uses.
// SHORTCUTS lists them all, for Settings › Keyboard Shortcuts.

export const isMac = /Mac/.test(navigator.userAgent);

const MAC: Record<string, string> = {
  mod: '⌘',
  alt: '⌥',
  shift: '⇧',
  ctrl: '⌃',
  Backspace: '⌫',
  Enter: '↩',
  Escape: 'Esc',
};
const OTHER: Record<string, string> = { mod: 'Ctrl', alt: 'Alt', shift: 'Shift', ctrl: 'Ctrl', Escape: 'Esc' };
const BOTH: Record<string, string> = { ArrowLeft: '←', ArrowRight: '→', ArrowUp: '↑', ArrowDown: '↓', '-': '−' };

/** "mod+E" → "⌘E" on macOS, "Ctrl+E" elsewhere. Parts are joined with "+"; "mod++" is ⌘+ / Ctrl++. */
export function keys(combo: string): string {
  const parts = combo.endsWith('++') ? [...combo.slice(0, -2).split('+'), '+'] : combo.split('+');
  const names = parts.map((p) => (isMac ? MAC : OTHER)[p] ?? BOTH[p] ?? p);
  // "⌘-click", as macOS writes it ("Ctrl+click" elsewhere).
  if (isMac && names.at(-1) === 'click') return `${names.slice(0, -1).join('')}-click`;
  return isMac ? names.join('') : names.join('+');
}

/** The key that moves selected books to the Trash. */
export const deleteKeys = isMac ? 'mod+Backspace' : 'Delete';

type Shortcut = { keys: string[]; action: string };

/** Every shortcut, grouped, for Settings. Click modifiers are written "mod+click". */
export const SHORTCUTS: { group: string; items: Shortcut[] }[] = [
  {
    group: 'General',
    items: [
      { keys: ['mod+,'], action: 'Settings' },
      { keys: ['mod+F', '/'], action: 'Search' },
      { keys: ['mod+1'], action: 'Covers view' },
      { keys: ['mod+2'], action: 'List view' },
      { keys: ['mod+3'], action: 'Shelf view' },
      { keys: ['mod++', 'mod+-'], action: 'Larger / smaller interface' },
      { keys: ['mod+0'], action: 'Actual size' },
    ],
  },
  {
    group: 'Books',
    items: [
      { keys: ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'], action: 'Move the selection' },
      { keys: ['Enter'], action: 'Show details' },
      { keys: ['Escape'], action: 'Back to one book, then close the details' },
      { keys: ['mod+click'], action: 'Add to or remove from the selection' },
      { keys: ['shift+click'], action: 'Select a range' },
      { keys: ['mod+A'], action: 'Select all' },
      { keys: ['mod+O'], action: 'Open in its default app' },
      { keys: ['mod+E'], action: 'Edit (several books: edit together)' },
      { keys: [deleteKeys], action: 'Move to the Trash' },
      { keys: ['shift+F10'], action: 'Book menu (also the menu key)' },
    ],
  },
  {
    group: 'Editing',
    items: [
      { keys: ['mod+S'], action: 'Save' },
      { keys: ['Escape'], action: 'Cancel' },
      { keys: ['mod+B', 'mod+I'], action: 'Bold, italic (description)' },
      { keys: ['mod+Enter'], action: 'Find Metadata: use the result' },
    ],
  },
  {
    group: 'List view',
    items: [
      { keys: ['alt+ArrowLeft', 'alt+ArrowRight'], action: 'Move the focused column' },
      { keys: ['ArrowLeft', 'ArrowRight'], action: 'Resize the details panel (its edge focused)' },
    ],
  },
];
