/**
 * The mark: four slabs under one wave (a sine cycle across them, its crest and
 * trough in the gaps), in the brand's yellow, teal, purple and coral, the same
 * on any theme, as the app icon is (brand svg/mark.svg). Each slab's corners
 * are softened by a round stroke in its own colour.
 */
const SLABS: readonly (readonly [string, string])[] = [
  [
    'M1.2 10.34 L1.94 9.44 L2.68 8.56 L3.41 7.71 L4.15 6.89 L4.89 6.1 L5.63 5.35 L6.36 4.65 L7.1 4.01 L7.84 3.42 L8.58 2.9 L9.31 2.44 L10.05 2.04 L10.05 52.8 L1.2 52.8Z',
    '#edb418',
  ],
  [
    'M15.45 1.47 L16.19 1.71 L16.93 2.03 L17.66 2.42 L18.4 2.88 L19.14 3.4 L19.88 3.99 L20.61 4.63 L21.35 5.33 L22.09 6.07 L22.82 6.86 L23.56 7.68 L24.3 8.53 L24.3 52.8 L15.45 52.8Z',
    '#00aeb6',
  ],
  [
    'M29.7 15.09 L30.44 15.95 L31.17 16.77 L31.91 17.56 L32.65 18.3 L33.39 19 L34.12 19.64 L34.86 20.23 L35.6 20.75 L36.34 21.21 L37.07 21.6 L37.81 21.91 L38.55 22.16 L38.55 52.8 L29.7 52.8Z',
    '#9b4bbb',
  ],
  [
    'M43.95 21.58 L44.69 21.19 L45.43 20.73 L46.16 20.21 L46.9 19.62 L47.64 18.97 L48.38 18.28 L49.11 17.53 L49.85 16.74 L50.59 15.92 L51.32 15.06 L52.06 14.19 L52.8 13.29 L52.8 52.8 L43.95 52.8Z',
    '#f14b4b',
  ],
];

/**
 * The mark at `size` px. In colour (`colour`), as in the app icon; else in the
 * current text colour, for quiet places like an empty library.
 */
export function Mark(props: { size?: number; class?: string; colour?: boolean }) {
  return (
    <svg
      viewBox="0 0 54 54"
      width={props.size ?? 64}
      height={props.size ?? 64}
      class={`flex-none ${props.class ?? ''}`}
      stroke-width="2.4"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {SLABS.map(([d, c]) => {
        const fill = props.colour ? c : 'currentColor';
        return <path d={d} fill={fill} stroke={fill} />;
      })}
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
