// Typed wrappers around the Rust commands in `crates/app/src/commands.rs`.
// The argument and result types come from `./bindings`, which ts-rs
// generates from the Rust structs (`cargo test` regenerates them).

import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { createSignal } from 'solid-js';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open, save } from '@tauri-apps/plugin-dialog';

import type { BookDetails } from './bindings/BookDetails';
import type { ConversionInfo } from './bindings/ConversionInfo';
import type { ConvertReport } from './bindings/ConvertReport';
import type { CoverPreview } from './bindings/CoverPreview';
import type { DeviceHit } from './bindings/DeviceHit';
import type { DeviceInfo } from './bindings/DeviceInfo';
import type { DeviceKind } from './bindings/DeviceKind';
import type { MatchedBy } from './bindings/MatchedBy';
import type { MergeOutcome } from './bindings/MergeOutcome';
import type { RemoveReport } from './bindings/RemoveReport';
import type { SendReport } from './bindings/SendReport';
import type { Lookup } from './bindings/Lookup';
import type { Match as MetadataMatch } from './bindings/Match';
import type { MatchKind } from './bindings/MatchKind';
import type { SearchResult } from './bindings/SearchResult';
import type { SourceId } from './bindings/SourceId';
import type { SourceRecord } from './bindings/SourceRecord';
import type { SourceStatus } from './bindings/SourceStatus';
import type { AddReport } from './bindings/AddReport';
import type { BookRow } from './bindings/BookRow';
import type { AuthorSort } from './bindings/AuthorSort';
import type { BookEdit } from './bindings/BookEdit';
import type { CacheInfo } from './bindings/CacheInfo';
import type { EditOutcome } from './bindings/EditOutcome';
import type { FileMetadataStatus } from './bindings/FileMetadataStatus';
import type { NameLists } from './bindings/NameLists';
import type { TidyPreview } from './bindings/TidyPreview';
import type { TidyReport } from './bindings/TidyReport';
import type { WritebackPreview } from './bindings/WritebackPreview';
import type { WriteReport } from './bindings/WriteReport';
import type { ImportProgress } from './bindings/ImportProgress';
import type { ImportReport } from './bindings/ImportReport';
import type { LibraryKind } from './bindings/LibraryKind';
import type { LibraryInfo } from './bindings/LibraryInfo';
import type { SortDir } from './bindings/SortDir';
import type { SortKey } from './bindings/SortKey';

export type {
  AddReport,
  AuthorSort,
  BookDetails,
  BookEdit,
  ConversionInfo,
  ConvertReport,
  CoverPreview,
  DeviceHit,
  DeviceInfo,
  DeviceKind,
  Lookup,
  MatchedBy,
  MergeOutcome,
  RemoveReport,
  SendReport,
  MatchKind,
  MetadataMatch,
  SearchResult,
  SourceId,
  SourceRecord,
  SourceStatus,
  EditOutcome,
  BookRow,
  FileMetadataStatus,
  CacheInfo,
  ImportProgress,
  ImportReport,
  LibraryInfo,
  LibraryKind,
  NameLists,
  SortDir,
  SortKey,
  TidyPreview,
  TidyReport,
  WritebackPreview,
  WriteReport,
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

  /** A book's editable fields (Magnitide libraries only). */
  bookEdit: (id: number) => invoke<BookEdit>('book_edit', { id }),

  /** How these authors sort: saved sort names, or the automatic ones for new authors. */
  authorSorts: (names: string[]) => invoke<AuthorSort[]>('author_sorts', { names }),

  /** Save an edit; renames the book's folder and files if its title or first author changed. */
  saveBookEdit: (id: number, edit: BookEdit) => invoke<EditOutcome>('save_book_edit', { id, edit }),

  /** Replace a book's cover with an image file. */
  setCoverFromFile: (id: number, path: string) => invoke<void>('set_cover_from_file', { id, path }),

  removeCover: (id: number) => invoke<void>('remove_cover', { id }),

  /** How many books "Tidy file names" would rename (changes nothing). */
  tidyPreview: () => invoke<TidyPreview>('tidy_preview'),

  /** Rename books whose folder or file names don't match their metadata. */
  tidyFileNames: () => invoke<TidyReport>('tidy_file_names'),

  /** Whether a book's files hold its current metadata; null for calibre libraries. */
  fileMetadataStatus: (id: number) => invoke<FileMetadataStatus | null>('file_metadata_status', { id }),

  /** Write a book's metadata and cover into its EPUB files (only those that differ). */
  writeMetadataToFiles: (id: number) => invoke<WriteReport>('write_metadata_to_files', { id }),

  /** How many books "Write metadata to files" would rewrite (changes nothing). */
  writebackPreview: () => invoke<WritebackPreview>('writeback_preview'),

  /** Write every book's metadata into its EPUB files that differ from the library. */
  writeAllMetadata: () => invoke<WriteReport>('write_all_metadata'),

  /** Metadata sources and whether each will be asked. */
  metadataSources: () => invoke<SourceStatus[]>('metadata_sources'),

  /** Turn a source on or off; `apiKey` replaces its key ("" clears it, null leaves it). */
  setMetadataSource: (id: SourceId, enabled: boolean, apiKey: string | null = null) =>
    invoke<SourceStatus[]>('set_metadata_source', { id, enabled, apiKey }),

  /** Search the active metadata sources (network). Descriptions come back sanitized. */
  findMetadata: (lookup: Lookup) => invoke<SearchResult>('find_metadata', { lookup }),

  /** Fetch the rest of a record (Open Library descriptions). */
  metadataDetails: (record: SourceRecord) => invoke<SourceRecord>('metadata_details', { record }),

  /** Download a source's cover for display; show it with `remoteCoverUrl(key)`. */
  coverPreview: (url: string) => invoke<CoverPreview>('cover_preview', { url }),

  /** Use a source's cover for a book (applies now). */
  setCoverFromUrl: (id: number, url: string) => invoke<void>('set_cover_from_url', { id, url }),

  /** Connected e-readers and which books of the open library are on them. */
  listDevices: () => invoke<DeviceInfo[]>('list_devices'),

  /** Scan a device again (after books were added or removed on it). */
  rescanDevice: (id: string) => invoke<void>('rescan_device', { id }),

  /** Eject (unmount) a device so it can be unplugged. */
  ejectDevice: (id: string) => invoke<void>('eject_device', { id }),

  /** Copy books to a device (best format for it; EPUBs carry the library's metadata). Progress via onTransferProgress. */
  sendToDevice: (id: string, bookIds: number[], format: string | null = null) =>
    invoke<SendReport>('send_to_device', { id, bookIds, format }),

  /** Delete books' files (and KOReader's notes for them) from a device. */
  removeFromDevice: (id: string, bookIds: number[]) => invoke<RemoveReport>('remove_from_device', { id, bookIds }),

  /** Add device files that aren't in the library (paths from `DeviceInfo.unmatched`). */
  importFromDevice: (id: string, paths: string[]) => invoke<AddReport>('import_from_device', { id, paths }),

  /** What can convert here (calibre's ebook-convert, if installed). */
  conversionInfo: () => invoke<ConversionInfo>('conversion_info'),

  /** Formats at least one of these books can be converted to. */
  convertTargets: (bookIds: number[]) => invoke<string[]>('convert_targets', { bookIds }),

  /** Convert books to `to`, adding it to each as a new format. Progress via onConvertProgress. */
  convertBooks: (bookIds: number[], to: string) => invoke<ConvertReport>('convert_books', { bookIds, to }),

  /** Merge book `other` into `keep` (formats, missing cover, tags, identifiers), removing `other`. */
  mergeBooks: (keep: number, other: number) => invoke<MergeOutcome>('merge_books', { keep, other }),

  /** Open a device in the file manager. */
  revealDevice: (id: string) => invoke<void>('reveal_device', { id }),

  /** Names already in the library (authors, tags, series, publishers), for autocomplete. */
  nameLists: () => invoke<NameLists>('name_lists'),

  /** Pick an image for a cover; null when cancelled. */
  async chooseImageFile(): Promise<string | null> {
    const picked = await open({
      title: 'Choose a cover image',
      multiple: false,
      filters: [{ name: 'Images', extensions: ['jpg', 'jpeg', 'png', 'webp', 'gif'] }],
    });
    return typeof picked === 'string' ? picked : null;
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

// Bumped when a cover changes, so the webview fetches it again instead of
// showing its cached copy. The Rust protocol ignores the query string.
const [coverVersions, setCoverVersions] = createSignal<ReadonlyMap<number, number>>(new Map());

/** Mark a book's cover as changed (its images reload everywhere). */
export function coverChanged(id: number): void {
  setCoverVersions((m) => new Map(m).set(id, (m.get(id) ?? 0) + 1));
}

/** URL for the Rust `cover://` protocol (its form differs per platform). Reactive. */
export function coverUrl(size: CoverSize, id: number): string {
  coverBase ??= convertFileSrc('', 'cover');
  const v = coverVersions().get(id);
  return `${coverBase}${size}/${id}${v ? `?v=${v}` : ''}`;
}

/** A downloaded source cover (see `api.coverPreview`), via the Rust `cover://` protocol. */
export function remoteCoverUrl(key: string): string {
  coverBase ??= convertFileSrc('', 'cover');
  return `${coverBase}remote/${key}`;
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

/** An e-reader was connected or disconnected (or rescanned). Matches `DEVICES_CHANGED_EVENT` in devices.rs. */
export function onDevicesChanged(handler: () => void): Promise<UnlistenFn> {
  return listen('devices-changed', handler);
}

/** Converting books. Event name matches `CONVERT_PROGRESS_EVENT` in commands.rs. */
export function onConvertProgress(handler: (p: ImportProgress) => void): Promise<UnlistenFn> {
  return listen<ImportProgress>('convert-progress', (e) => handler(e.payload));
}

/** Copying books to a device. Event name matches `TRANSFER_PROGRESS_EVENT` in devices.rs. */
export function onTransferProgress(handler: (p: ImportProgress) => void): Promise<UnlistenFn> {
  return listen<ImportProgress>('transfer-progress', (e) => handler(e.payload));
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
