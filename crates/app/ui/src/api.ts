// Typed wrappers around the Rust commands in `crates/app/src/commands.rs`.
// The argument and result types come from `./bindings`, which ts-rs
// generates from the Rust structs (`cargo test` regenerates them).

import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open, save } from '@tauri-apps/plugin-dialog';

import type { BookDetails } from './bindings/BookDetails';
import type { AddReport } from './bindings/AddReport';
import type { BookRow } from './bindings/BookRow';
import type { CacheInfo } from './bindings/CacheInfo';
import type { ImportProgress } from './bindings/ImportProgress';
import type { ImportReport } from './bindings/ImportReport';
import type { LibraryKind } from './bindings/LibraryKind';
import type { LibraryInfo } from './bindings/LibraryInfo';
import type { SortDir } from './bindings/SortDir';
import type { SortKey } from './bindings/SortKey';

export type {
  AddReport,
  BookDetails,
  BookRow,
  CacheInfo,
  ImportProgress,
  ImportReport,
  LibraryInfo,
  LibraryKind,
  SortDir,
  SortKey,
};
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

  /**
   * Import the open calibre library into a Magnitide library at `dest`
   * (created if new; a repeat import adds only new books). Progress arrives
   * via onImportProgress.
   */
  importCalibreLibrary: (dest: string) => invoke<ImportReport>('import_calibre_library', { dest }),

  /** Rename the open Magnitide library; resolves to the saved name. */
  renameLibrary: (name: string) => invoke<string>('rename_library', { name }),

  /**
   * Add book files and folders to the open Magnitide library (calibre libraries
   * refuse). Progress arrives via onAddProgress; the library is reloaded after.
   */
  addBooks: (paths: string[]) => invoke<AddReport>('add_books', { paths }),

  /** Pick book files to add; empty when cancelled. */
  async chooseBookFiles(): Promise<string[]> {
    const picked = await open({
      title: 'Add books',
      multiple: true,
      filters: [{ name: 'Books', extensions: BOOK_EXTENSIONS }],
    });
    return Array.isArray(picked) ? picked : picked ? [picked] : [];
  },

  /** Where to create a new Magnitide library; null when cancelled. */
  chooseNewLibraryLocation: (suggestedName: string) =>
    save({ title: 'Create a Magnitide library', defaultPath: suggestedName }),

  /** Native folder picker; null when cancelled. */
  async chooseLibraryFolder(): Promise<string | null> {
    const picked = await open({
      directory: true,
      multiple: false,
      title: 'Choose a Magnitide or calibre library folder',
    });
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

/** Import progress. Event name matches `IMPORT_PROGRESS_EVENT` in `crates/app/src/commands.rs`. */
export function onImportProgress(handler: (p: ImportProgress) => void): Promise<UnlistenFn> {
  return listen<ImportProgress>('import-progress', (e) => handler(e.payload));
}

/**
 * Book file extensions for the file picker. Mirrors `BOOK_EXTENSIONS` in
 * `crates/core/src/import/files.rs` (the backend decides; this only filters
 * the picker).
 */
export const BOOK_EXTENSIONS = [
  'epub',
  'kepub',
  'mobi',
  'azw',
  'azw3',
  'azw4',
  'kfx',
  'pdf',
  'cbz',
  'cbr',
  'cb7',
  'fb2',
  'fbz',
  'djvu',
  'txt',
  'rtf',
  'docx',
  'odt',
  'html',
  'htm',
  'lit',
  'pdb',
  'prc',
  'chm',
];

/** Progress while adding books. Event name matches `ADD_PROGRESS_EVENT` in `crates/app/src/commands.rs`. */
export function onAddProgress(handler: (p: ImportProgress) => void): Promise<UnlistenFn> {
  return listen<ImportProgress>('add-progress', (e) => handler(e.payload));
}

export type DragDrop =
  { type: 'enter'; paths: string[] } | { type: 'over' } | { type: 'drop'; paths: string[] } | { type: 'leave' };

/** Files dragged over / dropped onto the window (native paths, from Tauri). */
export function onDragDrop(handler: (e: DragDrop) => void): Promise<UnlistenFn> {
  return getCurrentWebview().onDragDropEvent((e) => handler(e.payload as DragDrop));
}
