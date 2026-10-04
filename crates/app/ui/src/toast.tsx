import { createSignal, Show } from 'solid-js';

type ToastState = { message: string; error: boolean };

const [current, setCurrent] = createSignal<ToastState | null>(null);
let timer: ReturnType<typeof setTimeout> | undefined;

/** Show a short message. `duration` (ms) defaults to longer for errors; Infinity keeps it up until replaced. */
export function toast(message: string, { error = false, duration }: { error?: boolean; duration?: number } = {}): void {
  clearTimeout(timer);
  setCurrent({ message, error });
  const ms = duration ?? (error ? 6000 : 1800);
  if (Number.isFinite(ms)) timer = setTimeout(() => setCurrent(null), ms);
}

export function Toast() {
  return (
    <Show when={current()}>
      {(t) => (
        <div
          class="fixed bottom-6 left-1/2 z-20 max-w-[min(560px,calc(100%-32px))] -translate-x-1/2 animate-toast-in rounded-[10px] px-4 py-[9px] text-white shadow-lift select-text"
          classList={{
            'bg-[rgb(30_30_33/0.92)]': !t().error,
            'bg-[color-mix(in_srgb,var(--danger)_88%,black)]': t().error,
          }}
          role="status"
          aria-live="polite"
        >
          {t().message}
        </div>
      )}
    </Show>
  );
}
