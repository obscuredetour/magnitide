import { For } from 'solid-js';

import { setTheme, themeAttrs, themeChoice, themes } from '../theme';
import { AccentPicker } from './AccentPicker';
import { Menu, MenuItem, MenuLabel, MenuSeparator } from './Menu';

/**
 * A theme's colours, rendered by scoping `data-theme` to the swatch itself;
 * themes.css selectors match any element, not just <html>.
 */
function Swatch(props: { theme: string }) {
  return (
    <span
      {...themeAttrs(props.theme)}
      class="relative size-5 flex-none overflow-hidden rounded-full border border-line bg-bg"
      aria-hidden="true"
    >
      <span class="absolute inset-y-0 right-0 w-1/2 bg-element" />
      <span class="absolute right-[3px] bottom-[3px] size-2 rounded-full bg-accent" />
    </span>
  );
}

export function ThemeMenu() {
  return (
    <Menu
      label="Theme"
      iconOnly
      align="end"
      buttonClass="grid size-8 place-items-center rounded-lg hover:bg-hover aria-expanded:bg-hover"
      button={
        <svg class="size-[17px] fill-none stroke-muted stroke-[1.6]" viewBox="0 0 20 20" aria-hidden="true">
          <circle cx="10" cy="10" r="7" />
          <path class="fill-muted stroke-none" d="M10 3a7 7 0 0 1 0 14Z" />
        </svg>
      }
    >
      <MenuLabel>Theme</MenuLabel>
      <MenuItem checked={themeChoice() === 'system'} onSelect={() => setTheme('system')}>
        <span class="relative size-5 flex-none overflow-hidden rounded-full border border-line" aria-hidden="true">
          <span {...themeAttrs('light')} class="absolute inset-y-0 left-0 w-1/2 bg-bg" />
          <span {...themeAttrs('dark')} class="absolute inset-y-0 right-0 w-1/2 bg-bg" />
        </span>
        Match System
      </MenuItem>
      <For each={themes()}>
        {(t) => (
          <MenuItem checked={themeChoice() === t.id} onSelect={() => setTheme(t.id)}>
            <Swatch theme={t.id} />
            {t.label}
          </MenuItem>
        )}
      </For>
      <MenuSeparator />
      <MenuLabel>Accent</MenuLabel>
      <AccentPicker menu />
    </Menu>
  );
}
