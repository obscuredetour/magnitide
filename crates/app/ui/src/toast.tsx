import { createSignal, Show } from 'solid-js';

type ToastState = { message: string; error: boolean };

const [current, setCurrent] = createSignal<ToastState | null>(null);
let timer: ReturnType<typeof setTimeout> | undefined;

export function toast(message: string, { error = false } = {}): void {
  clearTimeout(timer);
  setCurrent({ message, error });
  timer = setTimeout(() => setCurrent(null), error ? 6000 : 1800);
}

export function Toast() {
  return (
    <Show when={current()}>
      {(t) => (
        <div class="toast" classList={{ error: t().error }} role="status" aria-live="polite">
          {t().message}
        </div>
      )}
    </Show>
  );
}
