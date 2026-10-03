import { chooseLibrary } from '../library';

export function EmptyState(props: { message: string }) {
  return (
    <div class="empty">
      <img src="/icon.png" alt="" width="88" height="88" />
      <h1>Open your calibre library</h1>
      <p>{props.message}</p>
      <p class="empty-note">
        Choose the folder that contains <code>metadata.db</code>. Magnitide only reads it and never changes your
        library.
      </p>
      <button class="primary" onClick={() => void chooseLibrary()}>
        Choose library folder…
      </button>
    </div>
  );
}
