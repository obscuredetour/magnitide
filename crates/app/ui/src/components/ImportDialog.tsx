import { createSignal, For, Match, onMount, Show, Switch } from 'solid-js';

import { dismissImport, importState } from '../importer';
import { library, openLibrary } from '../library';

const primary = 'cursor-pointer rounded-lg bg-accent px-4 py-1.5 font-semibold text-white hover:brightness-110';
const secondary = 'cursor-pointer rounded-lg border border-line bg-surface-2 px-4 py-1.5 font-medium hover:bg-line';

function formatBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  return `${Math.round(n / 1e3)} KB`;
}

/** Progress and results of importing a calibre library. Mount while an import is active. */
export function ImportDialog() {
  let dialog!: HTMLDialogElement;
  onMount(() => dialog.showModal());

  const running = () => {
    const s = importState();
    return s.phase === 'running' ? s : null;
  };
  const done = () => {
    const s = importState();
    return s.phase === 'done' ? s : null;
  };
  const failed = () => {
    const s = importState();
    return s.phase === 'error' ? s : null;
  };

  return (
    <dialog
      ref={dialog}
      aria-label="Import calibre library"
      class="m-auto w-[min(520px,calc(100vw-48px))] animate-menu-in rounded-2xl border border-line bg-surface p-6 text-fg shadow-lift backdrop:bg-black/40"
      onCancel={(e) => {
        e.preventDefault();
        dismissImport(); // ignored while running
      }}
    >
      <h2 class="mb-1 font-serif text-xl font-semibold">Import into a Magnitide library</h2>
      <p class="mb-5 text-xs text-muted">
        From “{library()?.name}”. The calibre library is only read and stays exactly as it is.
      </p>

      <Switch>
        <Match when={running()}>
          {(s) => {
            const pct = () => {
              const p = s().progress;
              return p && p.total ? Math.round((p.done / p.total) * 100) : 0;
            };
            return (
              <div aria-live="polite">
                <div class="mb-2 flex justify-between text-xs text-muted tabular-nums">
                  <span>
                    {s().progress
                      ? `${s().progress!.done.toLocaleString()} of ${s().progress!.total.toLocaleString()} books`
                      : 'Starting…'}
                  </span>
                  <span>{pct()}%</span>
                </div>
                <div
                  class="h-2 overflow-hidden rounded-full bg-surface-2"
                  role="progressbar"
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={pct()}
                >
                  <div
                    class="h-full rounded-full bg-accent transition-[width] duration-150"
                    style={{ width: `${pct()}%` }}
                  />
                </div>
                <p class="mt-3 truncate text-xs text-faint">{s().progress?.title || ' '}</p>
              </div>
            );
          }}
        </Match>

        <Match when={done()}>
          {(s) => {
            const r = () => s().report;
            const [showDetails, setShowDetails] = createSignal(false);
            const problems = () => [...r().failed, ...r().warnings];
            return (
              <>
                <p class="mb-3 text-sm">
                  <strong>{r().booksImported.toLocaleString()}</strong> books imported
                  {r().booksSkipped ? `, ${r().booksSkipped.toLocaleString()} already there` : ''}
                  {r().failed.length ? `, ${r().failed.length} failed` : ''}.
                </p>
                <ul class="mb-4 space-y-1 text-xs text-muted">
                  <li>
                    {r().files.toLocaleString()} book files and {r().covers.toLocaleString()} covers.{' '}
                    {r().copied === 0
                      ? 'All cloned, using no extra disk space.'
                      : `${r().cloned.toLocaleString()} cloned, ${r().copied.toLocaleString()} copied (${formatBytes(r().copiedBytes)}).`}
                  </li>
                  <Show when={r().customFields}>
                    <li>{r().customFields} custom columns brought across.</li>
                  </Show>
                  <li class="break-all">{s().dest}</li>
                </ul>
                <Show when={problems().length}>
                  <button
                    class="mb-2 cursor-pointer text-xs text-accent hover:underline"
                    onClick={() => setShowDetails(!showDetails())}
                  >
                    {showDetails() ? 'Hide' : 'Show'} {problems().length} {problems().length === 1 ? 'note' : 'notes'}
                  </button>
                  <Show when={showDetails()}>
                    <ul class="mb-4 max-h-40 space-y-1 overflow-y-auto rounded-lg bg-bg p-3 text-xs text-muted select-text">
                      <For each={problems()}>{(p) => <li>{p}</li>}</For>
                    </ul>
                  </Show>
                </Show>
                <div class="mt-2 flex justify-end gap-2">
                  <button class={secondary} onClick={() => dismissImport()}>
                    Keep Browsing calibre
                  </button>
                  <button
                    class={primary}
                    onClick={async () => {
                      const dest = s().dest;
                      dismissImport();
                      await openLibrary(dest);
                    }}
                  >
                    Open Magnitide Library
                  </button>
                </div>
              </>
            );
          }}
        </Match>

        <Match when={failed()}>
          {(s) => (
            <>
              <p class="mb-4 text-sm text-danger select-text">{s().message}</p>
              <div class="flex justify-end">
                <button class={secondary} onClick={() => dismissImport()}>
                  Close
                </button>
              </div>
            </>
          )}
        </Match>
      </Switch>
    </dialog>
  );
}
