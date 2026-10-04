// A modal "Are you sure?" for destructive actions. `confirm(...)` resolves
// to true when the user confirms; <ConfirmHost/> (mounted once in App) shows it.

import { createSignal, type JSX, onMount, Show } from 'solid-js';

type Request = {
  title: string;
  /** Rendered inside the dialog (so it can hold its own controls). */
  message: () => JSX.Element;
  /** The confirm button's label, e.g. "Remove 3 Books". */
  confirm: string;
  /** Red confirm button. */
  danger?: boolean;
  resolve(ok: boolean): void;
};

const [request, setRequest] = createSignal<Request | null>(null);

export function confirm(options: Omit<Request, 'resolve'>): Promise<boolean> {
  request()?.resolve(false);
  return new Promise((resolve) => setRequest({ ...options, resolve }));
}

export function ConfirmHost() {
  return <Show when={request()}>{(r) => <ConfirmDialog request={r()} />}</Show>;
}

function ConfirmDialog(props: { request: Request }) {
  let dialog!: HTMLDialogElement;
  let confirmButton!: HTMLButtonElement;
  const done = (ok: boolean) => {
    props.request.resolve(ok);
    setRequest(null);
  };
  onMount(() => {
    dialog.showModal();
    confirmButton.focus();
  });
  return (
    <dialog
      ref={dialog}
      aria-label={props.request.title}
      class="m-auto w-[min(440px,calc(100vw-48px))] animate-menu-in rounded-2xl border border-line bg-surface p-0 text-fg shadow-lift backdrop:bg-black/40"
      onCancel={(e) => {
        e.preventDefault();
        done(false);
      }}
    >
      <div class="px-6 pt-5 pb-4">
        <h2 class="mb-2 text-base font-semibold">{props.request.title}</h2>
        <div class="text-sm text-muted select-text">{props.request.message()}</div>
      </div>
      <div class="flex justify-end gap-2 border-t border-line px-6 py-3">
        <button
          class="cursor-pointer rounded-md border border-line bg-surface-2 px-4 py-1.5 text-sm font-medium hover:bg-line"
          onClick={() => done(false)}
        >
          Cancel
        </button>
        <button
          ref={confirmButton}
          class="cursor-pointer rounded-lg px-4 py-1.5 text-sm font-semibold text-white hover:brightness-110"
          classList={{ 'bg-danger': props.request.danger, 'bg-accent': !props.request.danger }}
          onClick={() => done(true)}
        >
          {props.request.confirm}
        </button>
      </div>
    </dialog>
  );
}
