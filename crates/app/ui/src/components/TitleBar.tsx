import { getCurrentWindow } from '@tauri-apps/api/window';
import { onCleanup, onMount } from 'solid-js';

import appIcon from '../../../icons/128x128.png';

/** Whether a pointer event is on an open dialog's backdrop, over the title bar. */
function onBackdropOverTitleBar(e: MouseEvent): boolean {
  if (!(e.target instanceof HTMLDialogElement) || !e.target.open) return false;
  // The backdrop's clicks target the dialog itself; make sure it's outside the dialog's box.
  const box = e.target.getBoundingClientRect();
  const inside = e.clientX >= box.left && e.clientX <= box.right && e.clientY >= box.top && e.clientY <= box.bottom;
  const height = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--titlebar-h')) || 0;
  return !inside && e.clientY < height;
}

/**
 * The window's title bar on macOS, where the window has no native one
 * (titleBarStyle "Overlay"): the traffic lights sit in it, and dragging or
 * double-clicking it moves or zooms the window. Elsewhere the system draws
 * the title bar, so this is hidden (and --titlebar-h is 0).
 */
export function TitleBar() {
  // A modal dialog covers the page with its backdrop, so the drag region never
  // sees presses there; move or zoom the window from the backdrop instead, and
  // keep that press from counting as a click on the backdrop (which closes
  // some dialogs).
  onMount(() => {
    const press = (e: MouseEvent) => {
      if (e.button !== 0 || !onBackdropOverTitleBar(e)) return;
      e.preventDefault();
      e.stopPropagation();
      const win = getCurrentWindow();
      void (e.detail === 2 ? win.toggleMaximize() : win.startDragging());
    };
    const click = (e: MouseEvent) => {
      if (onBackdropOverTitleBar(e)) e.stopPropagation();
    };
    addEventListener('mousedown', press, true);
    addEventListener('click', click, true);
    onCleanup(() => {
      removeEventListener('mousedown', press, true);
      removeEventListener('click', click, true);
    });
  });

  return (
    <div
      class="fixed inset-x-0 top-0 z-10 hidden h-(--titlebar-h) items-center justify-center gap-1.5 bg-bg text-xs font-semibold text-muted mac:flex"
      data-tauri-drag-region
    >
      <img src={appIcon} alt="" class="pointer-events-none size-4" draggable={false} />
      <span class="pointer-events-none">Magnitide</span>
    </div>
  );
}
