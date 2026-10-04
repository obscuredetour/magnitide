// Typed wrappers around the Rust commands in `crates/app/src/commands.rs`.
// The argument and result types come from `./bindings`, which ts-rs
// generates from the Rust structs (`cargo test` regenerates them).

import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';

import type { BookDetails } from './bindings/BookDetails';
import type { BookRow } from './bindings/BookRow';
import type { CacheInfo } from './bindings/CacheInfo';
import type { LibraryInfo } from './bindings/LibraryInfo';
import type { SortDir } from './bindings/SortDir';
import type { SortKey } from './bindings/SortKey';

export type { BookDetails, BookRow, CacheInfo, LibraryInfo, SortDir, SortKey };
export type { FormatInfo } from './bindings/FormatInfo';

export const api = {
  /** Open `path`, or calibre's current/default library when `path` is null. */
  openLibrary: (path: string | null) => invoke<LibraryInfo>('open_library', { path }),

  /** Books matching `query` (calibre-style search subset), in `sort` order. */
  listBooks: (query: string, sort: SortKey, dir: SortDir) => invoke<BookRow[]>('list_books', { query, sort, dir }),

  bookDetails: (id: number) => invoke<BookDetails>('book_details', { id }),

  /** Opens in the system's default app; resolves to the format that was opened. */
  openBook: (id: number, format: string | null = null) => invoke<string>('open_book', { id, format }),

  /** Show the book in the file manager with its most readable file selected. */
  revealBook: (id: number) => invoke<void>('reveal_book', { id }),

  showLibraryFolder: () => invoke<void>('show_library_folder'),

  /** Size of the cover thumbnail cache (all libraries). */
  thumbnailCacheInfo: () => invoke<CacheInfo>('thumbnail_cache_info'),

  clearThumbnailCache: () => invoke<void>('clear_thumbnail_cache'),

  /** Native folder picker; null when cancelled. */
  async chooseLibraryFolder(): Promise<string | null> {
    const picked = await open({ directory: true, multiple: false, title: 'Choose a calibre library folder' });
    return typeof picked === 'string' ? picked : null;
  },
};

export type CoverSize = 'thumb' | 'full';

let coverBase: string | undefined;

/** URL for the Rust `cover://` protocol (its form differs per platform). */
export function coverUrl(size: CoverSize, id: number): string {
  coverBase ??= convertFileSrc('', 'cover');
  return `${coverBase}${size}/${id}`;
}

/** Commands reject with the Rust error string; anything else is unexpected. */
export function errorMessage(err: unknown): string {
  return typeof err === 'string' ? err : err instanceof Error ? err.message : String(err);
}

/** The platform's file manager, for labels like "Show in Finder". */
export const fileManagerName = /Mac/.test(navigator.userAgent)
  ? 'Finder'
  : /Windows/.test(navigator.userAgent)
    ? 'File Explorer'
    : 'Files';

/**
 * The macOS menu bar's "Settings… ⌘," item. Event name matches
 * `OPEN_SETTINGS_EVENT` in `crates/app/src/menu.rs`.
 */
export function onOpenSettings(handler: () => void): Promise<UnlistenFn> {
  return listen('open-settings', handler);
}
