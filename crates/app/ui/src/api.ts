// Typed wrappers around the Rust commands in `crates/app/src/commands.rs`.
// The argument and result types come from `./bindings`, which ts-rs
// generates from the Rust structs (`cargo test` regenerates them).

import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

import type { BookDetails } from './bindings/BookDetails';
import type { GridItem } from './bindings/GridItem';
import type { LibraryInfo } from './bindings/LibraryInfo';
import type { SortKey } from './bindings/SortKey';

export type { BookDetails, GridItem, LibraryInfo, SortKey };
export type { FormatInfo } from './bindings/FormatInfo';

export const api = {
  /** Open `path`, or calibre's current/default library when `path` is null. */
  openLibrary: (path: string | null) => invoke<LibraryInfo>('open_library', { path }),

  /** Books matching `query` (calibre-style search subset), in `sort` order. */
  listBooks: (query: string, sort: SortKey) => invoke<GridItem[]>('list_books', { query, sort }),

  bookDetails: (id: number) => invoke<BookDetails>('book_details', { id }),

  /** Opens in the system's default app; resolves to the format that was opened. */
  openBook: (id: number, format: string | null = null) => invoke<string>('open_book', { id, format }),

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
