/** The book-and-wave mark as line art in the current colour, the bookmark in the accent. */
export function Mark(props: { size?: number; class?: string }) {
  return (
    <svg
      viewBox="0 0 64 64"
      width={props.size ?? 64}
      height={props.size ?? 64}
      class={props.class}
      fill="none"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <rect x="13" y="7" width="38" height="50" rx="6" />
      <path d="M20 7v50" />
      <path d="M13 36c6-6 12-6 19 0s13 6 19 0" />
      <path d="M13 45c6-6 12-6 19 0s13 6 19 0" opacity=".5" />
      <path d="M38 7h9v15l-4.5-3.5L38 22z" fill="var(--accent)" stroke="none" />
    </svg>
  );
}

/** The message in an empty view: with the mark when the library has no books, plain for a search with no matches. */
export function EmptyNote(props: { message: string; mark: boolean }) {
  return (
    <div class="absolute inset-x-0 top-[30%] flex flex-col items-center gap-3 text-center text-sm text-muted">
      {props.mark && <Mark size={48} class="text-faint" />}
      <p>{props.message}</p>
    </div>
  );
}
