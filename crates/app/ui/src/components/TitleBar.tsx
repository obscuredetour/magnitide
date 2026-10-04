import appIcon from '../../../icons/128x128.png';

/**
 * The window's title bar on macOS, where the window has no native one
 * (titleBarStyle "Overlay"): the traffic lights sit in it, and dragging or
 * double-clicking it moves or zooms the window. Elsewhere the system draws
 * the title bar, so this is hidden (and --titlebar-h is 0).
 */
export function TitleBar() {
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
