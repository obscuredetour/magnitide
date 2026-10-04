import {
  createContext,
  createEffect,
  createSignal,
  type JSX,
  onCleanup,
  type ParentProps,
  Show,
  useContext,
} from 'solid-js';

const MenuContext = createContext<{ close(): void }>();

type MenuProps = {
  /** Accessible name for the menu (and the button, when `iconOnly`). */
  label: string;
  /** The button shows only an icon, so it needs `label` as its name and tooltip. */
  iconOnly?: boolean;
  /** Tooltip for a button with visible text. */
  title?: string;
  /** Classes for the wrapper, e.g. grid placement. */
  class?: string;
  /** Contents of the trigger button. */
  button: JSX.Element;
  buttonClass?: string;
  /** Which edge of the button the menu lines up with. */
  align?: 'start' | 'end';
  /** Extra classes for the panel, e.g. a width. */
  panelClass?: string;
  children: JSX.Element;
};

const ITEMS = '[role^="menuitem"]:not([disabled])';

/**
 * A button with a dropdown menu. Arrow keys move between items, Esc or a
 * click outside closes it, and keys pressed inside don't reach the app's
 * global shortcuts.
 */
export function Menu(props: MenuProps) {
  const [open, setOpen] = createSignal(false);
  let root!: HTMLDivElement;
  let button!: HTMLButtonElement;
  let panel: HTMLDivElement | undefined;

  const items = () => (panel ? [...panel.querySelectorAll<HTMLElement>(ITEMS)] : []);
  const close = (refocus = true) => {
    setOpen(false);
    if (refocus) button.focus();
  };

  createEffect(() => {
    if (!open()) return;
    const onPointerDown = (e: PointerEvent) => {
      if (!root.contains(e.target as Node)) close(false);
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    onCleanup(() => document.removeEventListener('pointerdown', onPointerDown, true));
    // Focus the checked item if there is one, else the first.
    queueMicrotask(() => {
      const all = items();
      (all.find((el) => el.getAttribute('aria-checked') === 'true') ?? all[0])?.focus();
    });
  });

  const onKeyDown = (e: KeyboardEvent) => {
    e.stopPropagation();
    const all = items();
    const i = all.indexOf(document.activeElement as HTMLElement);
    const focusAt = (n: number) => all[(n + all.length) % all.length]?.focus();
    switch (e.key) {
      case 'Escape':
        e.preventDefault();
        close();
        break;
      case 'Tab':
        close(false);
        break;
      case 'ArrowDown':
        e.preventDefault();
        focusAt(i + 1);
        break;
      case 'ArrowUp':
        e.preventDefault();
        focusAt(i < 0 ? -1 : i - 1);
        break;
      case 'Home':
        e.preventDefault();
        focusAt(0);
        break;
      case 'End':
        e.preventDefault();
        focusAt(-1);
        break;
    }
  };

  return (
    <div ref={root} class={`relative ${props.class ?? ''}`}>
      <button
        ref={button}
        class={props.buttonClass}
        aria-haspopup="menu"
        aria-expanded={open()}
        aria-label={props.iconOnly ? props.label : undefined}
        title={props.iconOnly ? props.label : props.title}
        onClick={() => setOpen(!open())}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown' && !open()) {
            e.preventDefault();
            setOpen(true);
          }
        }}
      >
        {props.button}
      </button>
      <Show when={open()}>
        <MenuContext.Provider value={{ close }}>
          <div
            ref={panel}
            role="menu"
            aria-label={props.label}
            class={`absolute top-full z-30 mt-1.5 min-w-56 animate-menu-in rounded-xl border border-line bg-surface p-1.5 text-fg shadow-lift ${props.panelClass ?? ''}`}
            classList={{ 'right-0': props.align === 'end', 'left-0': props.align !== 'end' }}
            // Native (not delegated) so stopPropagation keeps keys from the app's
            // document-level shortcuts.
            on:keydown={onKeyDown}
          >
            {props.children}
          </div>
        </MenuContext.Provider>
      </Show>
    </div>
  );
}

type ItemProps = ParentProps<{
  onSelect(): void;
  /** Makes this a radio item (with a check mark when true). */
  checked?: boolean;
  disabled?: boolean;
  title?: string;
}>;

export function MenuItem(props: ItemProps) {
  const menu = useContext(MenuContext);
  const isRadio = () => props.checked !== undefined;
  return (
    <button
      role={isRadio() ? 'menuitemradio' : 'menuitem'}
      aria-checked={isRadio() ? props.checked : undefined}
      disabled={props.disabled}
      title={props.title}
      tabIndex={-1}
      class="flex w-full cursor-default items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left outline-none hover:bg-accent/15 focus:bg-accent/15 disabled:opacity-45 disabled:hover:bg-transparent"
      onClick={() => {
        menu?.close();
        props.onSelect();
      }}
    >
      {props.children}
      <Show when={isRadio()}>
        <svg
          class="ml-auto size-3.5 flex-none fill-none stroke-accent stroke-2 [stroke-linecap:round] [stroke-linejoin:round]"
          classList={{ invisible: !props.checked }}
          viewBox="0 0 16 16"
          aria-hidden="true"
        >
          <path d="m3 8.5 3.5 3.5L13 4.5" />
        </svg>
      </Show>
    </button>
  );
}

export function MenuSeparator() {
  return <div role="separator" class="mx-1 my-1.5 h-px bg-line" />;
}

/** Non-interactive heading text inside a menu. */
export function MenuLabel(props: ParentProps) {
  return (
    <div class="px-2.5 pt-1 pb-1.5 text-[11px] font-semibold tracking-[.06em] text-muted uppercase">
      {props.children}
    </div>
  );
}
