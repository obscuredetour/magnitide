/**
 * The mark's solid cut (brand svg/mark-solid.svg): the three stacked books
 * filled, for small sizes where the outline's strokes would blur together.
 * Coloured by the theme, top to bottom: text, teal, coral (the logo's, in
 * themes without one).
 */
export function AppGlyph(props: { size?: number; class?: string }) {
  const book = (d: string, colour: string) => (
    <path d={d} fill={colour} stroke={colour} stroke-width="3.9" stroke-linejoin="round" />
  );
  return (
    <svg
      viewBox="2 12 96 76"
      width={props.size ?? 32}
      height={((props.size ?? 32) * 76) / 96}
      class={`flex-none ${props.class ?? ''}`}
      aria-hidden="true"
    >
      {book(
        'M75.45 34.15H15.22A4.97 4.97 0 0 1 10.25 29.18V21.72A4.97 4.97 0 0 1 15.22 16.75H75.45C72.32 19.88 72.32 31.02 75.45 34.15Z',
        'var(--text)',
      )}
      {book(
        'M15.85 56.25H78.98A4.97 4.97 0 0 0 83.95 51.28V43.12A4.97 4.97 0 0 0 78.98 38.15H15.85C19.11 41.41 19.11 52.99 15.85 56.25Z',
        'var(--teal, #0e6a6f)',
      )}
      {book(
        'M91.85 83.15H10.92A4.97 4.97 0 0 1 5.95 78.18V65.32A4.97 4.97 0 0 1 10.92 60.35H91.85C87.75 64.45 87.75 79.05 91.85 83.15Z',
        'var(--coral, #f14b4b)',
      )}
    </svg>
  );
}
