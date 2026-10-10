/**
 * The app icon's three stacked books, filled rather than outlined, for small
 * sizes where the icon's strokes would blur together. The top book is the
 * text colour, so it shows on dark themes too; teal and coral are the logo's.
 */
export function AppGlyph(props: { size?: number; class?: string }) {
  return (
    <svg
      viewBox="72 71 368 368"
      width={props.size ?? 32}
      height={props.size ?? 32}
      class={`flex-none ${props.class ?? ''}`}
      aria-hidden="true"
    >
      <path d="M118 124H356Q347 161 360 198H118A14 14 0 0 1 104 184V138A14 14 0 0 1 118 124Z" fill="currentColor" />
      <path d="M138 206H380A14 14 0 0 1 394 220V266A14 14 0 0 1 380 280H138Q127 243 138 206Z" fill="#126e70" />
      <path d="M100 288H420Q408 337 424 386H100A14 14 0 0 1 86 372V302A14 14 0 0 1 100 288Z" fill="#f14b4b" />
    </svg>
  );
}
