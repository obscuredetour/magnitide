// Importing the open calibre library into a Magnitide library.

import { createSignal } from 'solid-js';

import { api, errorMessage, type ImportProgress, type ImportReport, onImportProgress } from './api';

export type ImportState =
  | { phase: 'idle' }
  | { phase: 'running'; dest: string; progress: ImportProgress | null }
  | { phase: 'done'; dest: string; report: ImportReport }
  | { phase: 'error'; message: string };

const [state, setState] = createSignal<ImportState>({ phase: 'idle' });
export const importState = state;

/** Ask where to create the library, then import into it. */
export async function startImport(): Promise<void> {
  if (state().phase === 'running') return;
  const dest = await api.chooseNewLibraryLocation('Magnitide Library');
  if (!dest) return;

  setState({ phase: 'running', dest, progress: null });
  const unlisten = await onImportProgress((progress) => {
    const s = state();
    if (s.phase === 'running') setState({ ...s, progress });
  });
  try {
    const report = await api.importCalibreLibrary(dest);
    setState({ phase: 'done', dest, report });
  } catch (err) {
    setState({ phase: 'error', message: errorMessage(err) });
  } finally {
    unlisten();
  }
}

/** Close the dialog. Not allowed while an import is running. */
export function dismissImport(): void {
  if (state().phase !== 'running') setState({ phase: 'idle' });
}
