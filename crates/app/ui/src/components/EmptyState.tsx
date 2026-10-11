import { chooseLibrary, createLibrary } from '../library';
import { primaryButton, secondaryButton } from '../buttons';
import { Mark } from './Mark';

export function EmptyState(props: { message: string }) {
  return (
    <div class="fixed inset-x-0 top-(--topbar-h) bottom-0 flex flex-col items-center justify-center bg-bg p-6 text-center">
      <Mark size={88} class="text-muted" />
      <h1 class="mt-[18px] mb-2 font-serif text-2xl font-semibold">Open a library</h1>
      <p class="mb-2 max-w-[420px] text-sm text-muted">{props.message}</p>
      <p class="mb-[22px] max-w-[420px] text-sm text-muted">
        Create a new library and drop your books in, or open one you already have: a Magnitide library, or a calibre
        library (the folder containing <code class="text-[12.5px]">metadata.db</code>). calibre libraries are opened
        read-only and never changed.
      </p>
      <div class="flex flex-wrap justify-center gap-2">
        <button class={primaryButton} onClick={() => void createLibrary()}>
          Create New Library…
        </button>
        <button class={secondaryButton} onClick={() => void chooseLibrary()}>
          Open Library Folder…
        </button>
      </div>
    </div>
  );
}
