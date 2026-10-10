import { For, onMount, type ParentProps, Show } from 'solid-js';

import { api, errorMessage } from '../../api';
import { addonReport, addThemeFile, loadThemeAddons, setTheme, themeAttrs, themeChoice, themes } from '../../theme';
import { toast } from '../../toast';
import { blendAllThemes, setBlendAllThemes } from '../../view';
import { AccentPicker } from '../AccentPicker';
import { button, Group, Heading, Row, Toggle } from './parts';

/** Miniature of a theme: page, a card, an accent. Scoped via data-theme. */
function ThemePreview(props: { theme: string }) {
  return (
    <div {...themeAttrs(props.theme)} class="flex h-full w-full flex-col gap-1.5 bg-bg p-2">
      <div class="h-1.5 w-1/2 rounded-full bg-fg/70" />
      <div class="flex flex-1 gap-1.5">
        <div class="flex-1 rounded bg-panel shadow-card" />
        <div class="flex-1 rounded bg-element" />
      </div>
      <div class="h-1.5 w-1/3 rounded-full bg-accent" />
    </div>
  );
}

function ThemeCard(props: ParentProps<{ selected: boolean; label: string; onSelect(): void }>) {
  return (
    <button
      class="group flex cursor-pointer flex-col gap-1.5 text-left"
      role="radio"
      aria-checked={props.selected}
      onClick={() => props.onSelect()}
    >
      <div class="h-[72px] w-full overflow-hidden rounded-card border border-line group-aria-checked:outline-2 group-aria-checked:outline-offset-2 group-aria-checked:outline-accent">
        {props.children}
      </div>
      <span class="text-xs group-aria-checked:font-semibold">{props.label}</span>
    </button>
  );
}

/** Theme files in the themes folder: add one, open the folder, reload, and what couldn't be used. */
function ThemeAddons() {
  const reload = () => loadThemeAddons().catch((err) => toast(errorMessage(err), { error: true }));
  onMount(() => void reload());
  return (
    <Group
      title="Theme add-ons"
      description={
        <>
          <p>Theme files in the themes folder show up with the themes above.</p>
          <p>
            A Catppuccin-style <code>palette.json</code> works too, one theme per flavour. The format is in{' '}
            <code>docs/themes.md</code>.
          </p>
        </>
      }
    >
      <Show when={addonReport()?.problems.length}>
        <ul class="space-y-1 border-b border-line py-3 text-xs text-danger select-text">
          <For each={addonReport()!.problems}>
            {(p) => (
              <li>
                <span class="font-medium">{p.file}</span>: {p.message}
              </li>
            )}
          </For>
        </ul>
      </Show>
      <div class="flex flex-wrap gap-2 py-3">
        <button
          class={button}
          onClick={async () => {
            const error = await addThemeFile();
            if (error) toast(error, { error: true });
          }}
        >
          Add Theme File…
        </button>
        <button
          class={button}
          onClick={() => api.openThemesFolder().catch((err) => toast(errorMessage(err), { error: true }))}
        >
          Open Themes Folder
        </button>
        <button class={button} title="After editing theme files" onClick={() => void reload()}>
          Reload
        </button>
      </div>
    </Group>
  );
}

export function ThemeSettings() {
  return (
    <>
      <Heading title="Themes">The app’s colours. The toolbar’s theme menu switches them too.</Heading>

      <section class="mb-7">
        <div
          class="mb-3 grid grid-cols-[repeat(auto-fill,minmax(120px,1fr))] gap-4"
          role="radiogroup"
          aria-label="Theme"
        >
          <ThemeCard selected={themeChoice() === 'system'} label="Match System" onSelect={() => setTheme('system')}>
            <div class="flex h-full">
              <div class="w-1/2 overflow-hidden">
                <ThemePreview theme="light" />
              </div>
              <div class="w-1/2 overflow-hidden">
                <ThemePreview theme="dark" />
              </div>
            </div>
          </ThemeCard>
          <For each={themes()}>
            {(t) => (
              <ThemeCard selected={themeChoice() === t.id} label={t.label} onSelect={() => setTheme(t.id)}>
                <ThemePreview theme={t.id} />
              </ThemeCard>
            )}
          </For>
        </div>
      </section>

      <Group>
        <Row label="Accent" description="Buttons, selection and highlights. Remembered for each theme.">
          <div class="max-w-[300px]">
            <AccentPicker />
          </div>
        </Row>
        <Row
          label="Cover colours on every theme"
          description="The details panel takes on the colours of the book’s cover. Light and Dark are made for it; turn this on for the other themes too, though their palettes may clash."
        >
          <Toggle
            label="Cover colours behind the details on every theme"
            checked={blendAllThemes()}
            onChange={setBlendAllThemes}
          />
        </Row>
      </Group>

      <ThemeAddons />
    </>
  );
}
