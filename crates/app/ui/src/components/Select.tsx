import { type JSX, splitProps } from 'solid-js';
import ChevronDown from 'lucide-solid/icons/chevron-down';

import { Icon } from './Icon';

/**
 * A native `<select>` with the app's own arrow. WebKit draws a different one
 * in a styled select on each macOS (a ↕ stepper on macOS 15), so its native
 * look is turned off; the menu that opens is still the system's. `class` is
 * for size, text and left padding: the right padding makes room for the arrow.
 */
export function Select(props: JSX.SelectHTMLAttributes<HTMLSelectElement>) {
  const [local, rest] = splitProps(props, ['class', 'children']);
  return (
    <span class="relative inline-flex items-center">
      <select {...rest} class={`appearance-none rounded-control border border-line bg-field pr-6 ${local.class ?? ''}`}>
        {local.children}
      </select>
      <Icon of={ChevronDown} size={12} class="pointer-events-none absolute right-1.5 text-muted" />
    </span>
  );
}
