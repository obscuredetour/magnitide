import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from 'solid-js';
import { getCurrentWindow } from '@tauri-apps/api/window';
import ArrowLeftRight from 'lucide-solid/icons/arrow-left-right';
import Columns2 from 'lucide-solid/icons/columns-2';
import Maximize from 'lucide-solid/icons/maximize';
import Minimize from 'lucide-solid/icons/minimize';
import MoveHorizontal from 'lucide-solid/icons/move-horizontal';
import MoveVertical from 'lucide-solid/icons/move-vertical';
import Scan from 'lucide-solid/icons/scan';
import Square from 'lucide-solid/icons/square';
import X from 'lucide-solid/icons/x';

import { api, errorMessage, type OpenedComic, pageUrl } from '../api';
import { noteReading } from '../reading';
import { storage } from '../storage';
import { closeViewer } from '../viewer';
import { Icon } from './Icon';

type Fit = 'page' | 'width' | 'height';
type Spread = 'auto' | 'single' | 'double';

const FITS: { value: Fit; label: string; icon: typeof Scan }[] = [
  { value: 'page', label: 'Fit the page', icon: Scan },
  { value: 'width', label: 'Fit the width', icon: MoveHorizontal },
  { value: 'height', label: 'Fit the height', icon: MoveVertical },
];
const SPREADS: { value: Spread; label: string }[] = [
  { value: 'auto', label: 'Two pages in a wide window' },
  { value: 'single', label: 'One page' },
  { value: 'double', label: 'Two pages' },
];

/** Tells one opening from another in page URLs, so a page is never one of an earlier file. */
let openings = 0;

/**
 * Reading a comic: a full-window overlay over the library. Pages turn with the
 * arrow keys (the other way for right-to-left comics), clicks on either half,
 * Space and Page Up/Down, or the slider; a wide window shows two pages, the
 * cover and wide spreads on their own. Where you are is saved as you read.
 */
export function ComicViewer(props: { id: number }) {
  const opening = ++openings;
  const [comic, setComic] = createSignal<OpenedComic | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [page, setPage] = createSignal(0);
  const [rtl, setRtl] = createSignal(false);
  const [fit, setFit] = createSignal<Fit>((storage.get('comicFit') as Fit | null) ?? 'page');
  const [spread, setSpread] = createSignal<Spread>((storage.get('comicSpread') as Spread | null) ?? 'auto');
  const [size, setSize] = createSignal({ w: window.innerWidth, h: window.innerHeight });
  /** Pages known to be wider than tall (spreads printed across two pages): shown alone. */
  const [wide, setWide] = createSignal<ReadonlySet<number>>(new Set());
  const [controls, setControls] = createSignal(true);
  const [fullscreen, setFullscreen] = createSignal(false);
  let scroller!: HTMLDivElement;

  const total = () => comic()?.pages ?? 0;
  const twoUp = () => spread() === 'double' || (spread() === 'auto' && size().w > size().h * 1.1);
  /** The pages shown with page `p`: the cover alone, then pairs (1–2, 3–4, …), a wide one alone. */
  const spreadAt = (p: number): number[] => {
    if (!twoUp() || p === 0) return [p];
    const first = p % 2 === 1 ? p : p - 1;
    const second = first + 1;
    if (second >= total() || wide().has(first) || wide().has(second)) return [p];
    return [first, second];
  };
  const shown = createMemo(() => spreadAt(page()));
  const last = () => shown()[shown().length - 1]!;

  const go = (p: number) => setPage(Math.max(0, Math.min(total() - 1, p)));
  const next = () => go(last() + 1);
  const prev = () => go(spreadAt(Math.max(0, shown()[0]! - 1))[0]!);
  /** Towards the left or right of the screen: back or on, by reading direction. */
  const left = () => (rtl() ? next() : prev());
  const right = () => (rtl() ? prev() : next());

  // Saved as you read (once you've settled on a page), and on closing.
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  const save = () => {
    clearTimeout(saveTimer);
    const c = comic();
    if (c) {
      void api
        .saveReadingProgress(c.id, last(), c.pages)
        .then((s) => s && noteReading({ id: c.id, ...s }))
        .catch(() => {});
    }
  };
  createEffect(
    on(page, () => {
      if (!comic()) return;
      clearTimeout(saveTimer);
      saveTimer = setTimeout(save, 700);
      scroller?.scrollTo({ top: 0, left: 0 });
    }),
  );

  // The next pages (and the one before) load ahead, so turning is instant.
  createEffect(() => {
    const c = comic();
    if (!c) return;
    for (const p of [last() + 1, last() + 2, shown()[0]! - 1]) {
      if (p >= 0 && p < c.pages) new Image().src = pageUrl(c.id, p, opening);
    }
  });

  const close = async () => {
    save();
    if (fullscreen())
      await getCurrentWindow()
        .setFullscreen(false)
        .catch(() => {});
    void api.closeComic().catch(() => {});
    closeViewer();
  };

  const toggleFullscreen = async () => {
    const on = !fullscreen();
    try {
      await getCurrentWindow().setFullscreen(on);
      setFullscreen(on);
    } catch {
      // Not allowed or not supported: stay as it is.
    }
  };

  // The controls fade after a moment without the mouse moving.
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  const wake = () => {
    setControls(true);
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => setControls(false), 2500);
  };

  onMount(async () => {
    wake();
    const onKey = (e: KeyboardEvent) => {
      // Ahead of the library's own shortcuts, which would move its selection.
      e.stopPropagation();
      if (e.target instanceof HTMLInputElement && e.target.type !== 'range') return;
      const keys: Record<string, () => void> = {
        ArrowLeft: left,
        ArrowRight: right,
        ArrowDown: next,
        ArrowUp: prev,
        PageDown: next,
        PageUp: prev,
        ' ': () => (e.shiftKey ? prev() : next()),
        Home: () => go(0),
        End: () => go(total() - 1),
        f: () => void toggleFullscreen(),
        Escape: () => void close(),
      };
      const action = keys[e.key];
      if (action && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        action();
      }
    };
    const onResize = () => setSize({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('resize', onResize);
    onCleanup(() => {
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('resize', onResize);
      clearTimeout(saveTimer);
      clearTimeout(hideTimer);
    });
    try {
      const c = await api.openComic(props.id);
      setRtl(c.rightToLeft);
      setPage(c.page);
      setComic(c);
      save(); // Opening it marks it as being read.
    } catch (err) {
      setError(errorMessage(err));
    }
  });

  const setFitTo = (f: Fit) => {
    setFit(f);
    storage.set('comicFit', f);
  };
  const setSpreadTo = (s: Spread) => {
    setSpread(s);
    storage.set('comicSpread', s);
  };

  const imgClass = () => {
    const half = shown().length === 2;
    switch (fit()) {
      case 'width':
        return half ? 'w-1/2 h-auto' : 'w-full h-auto';
      case 'height':
        return 'h-full w-auto max-w-none';
      default:
        return half ? 'max-h-full max-w-[50%] object-contain' : 'max-h-full max-w-full object-contain';
    }
  };
  const button =
    'grid size-8 cursor-pointer place-items-center rounded-control text-white/80 hover:bg-white/15 hover:text-white aria-pressed:bg-white/20 aria-pressed:text-white';

  return (
    <div
      class="fixed inset-0 z-50 flex flex-col bg-black text-white select-none"
      role="dialog"
      aria-label={comic()?.title ?? 'Comic'}
      onMouseMove={wake}
      classList={{ 'cursor-none': !controls() }}
    >
      <Show
        when={comic()}
        fallback={
          <div class="m-auto max-w-md p-6 text-center text-sm text-white/70">
            <Show when={error()} fallback="Opening…">
              {(e) => (
                <>
                  <p class="mb-4 text-white select-text">{e()}</p>
                  <button class="cursor-pointer text-link hover:underline" onClick={() => void close()}>
                    Close
                  </button>
                </>
              )}
            </Show>
          </div>
        }
      >
        {(c) => (
          <>
            <div
              ref={scroller}
              class="flex min-h-0 flex-1"
              classList={{
                'items-center justify-center': fit() === 'page',
                'overflow-y-auto items-start justify-center': fit() === 'width',
                'overflow-x-auto justify-center': fit() === 'height',
              }}
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                if (e.clientX < r.left + r.width / 2) left();
                else right();
              }}
            >
              <For each={rtl() ? [...shown()].reverse() : shown()}>
                {(p) => (
                  <img
                    src={pageUrl(c().id, p, opening)}
                    alt={`Page ${p + 1}`}
                    draggable={false}
                    class={imgClass()}
                    onLoad={(e) => {
                      const img = e.currentTarget;
                      if (img.naturalWidth > img.naturalHeight * 1.1 && !wide().has(p)) {
                        setWide(new Set([...wide(), p]));
                      }
                    }}
                  />
                )}
              </For>
            </div>

            <div
              class="absolute inset-x-0 bottom-0 flex items-center gap-3 bg-gradient-to-t from-black/90 to-black/0 px-5 pt-10 pb-4 transition-opacity duration-300"
              classList={{ 'opacity-0 pointer-events-none': !controls() }}
              onMouseMove={(e) => {
                e.stopPropagation();
                setControls(true);
                clearTimeout(hideTimer);
              }}
            >
              <div class="min-w-0 shrink">
                <div class="truncate text-sm font-semibold">{c().title}</div>
                <div class="text-xs text-white/60 tabular-nums">
                  {shown().length === 2 ? `Pages ${shown()[0]! + 1}–${shown()[1]! + 1}` : `Page ${page() + 1}`} of{' '}
                  {c().pages}
                </div>
              </div>
              <input
                type="range"
                class="min-w-24 flex-1 accent-accent"
                min={0}
                max={c().pages - 1}
                value={page()}
                dir={rtl() ? 'rtl' : 'ltr'}
                aria-label="Page"
                onInput={(e) => go(Number(e.currentTarget.value))}
              />
              <div class="flex items-center gap-1">
                <For each={FITS}>
                  {(f) => (
                    <button
                      class={button}
                      title={f.label}
                      aria-label={f.label}
                      aria-pressed={fit() === f.value}
                      onClick={() => setFitTo(f.value)}
                    >
                      <Icon of={f.icon} size={16} />
                    </button>
                  )}
                </For>
                <span class="mx-1 h-5 w-px bg-white/20" />
                <button
                  class={button}
                  title={SPREADS.find((s) => s.value === spread())!.label + ' (click to change)'}
                  aria-label="Pages side by side"
                  aria-pressed={twoUp()}
                  onClick={() =>
                    setSpreadTo(SPREADS[(SPREADS.findIndex((s) => s.value === spread()) + 1) % SPREADS.length]!.value)
                  }
                >
                  <Icon of={twoUp() ? Columns2 : Square} size={16} />
                </button>
                <button
                  class={button}
                  title={rtl() ? 'Reading right to left (manga)' : 'Reading left to right'}
                  aria-label="Right to left"
                  aria-pressed={rtl()}
                  onClick={() => setRtl(!rtl())}
                >
                  <Icon of={ArrowLeftRight} size={16} />
                </button>
                <button
                  class={button}
                  title={fullscreen() ? 'Leave full screen (F)' : 'Full screen (F)'}
                  aria-label="Full screen"
                  aria-pressed={fullscreen()}
                  onClick={() => void toggleFullscreen()}
                >
                  <Icon of={fullscreen() ? Minimize : Maximize} size={16} />
                </button>
                <button class={button} title="Close (Esc)" aria-label="Close" onClick={() => void close()}>
                  <Icon of={X} size={16} />
                </button>
              </div>
            </div>
          </>
        )}
      </Show>
    </div>
  );
}
