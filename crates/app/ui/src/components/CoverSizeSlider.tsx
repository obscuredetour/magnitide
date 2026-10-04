import { COVER_SIZE, coverSize, setCoverSize } from '../view';

function BookIcon(props: { class: string }) {
  return (
    <svg class={`flex-none fill-faint ${props.class}`} viewBox="0 0 8 12" aria-hidden="true">
      <rect width="8" height="12" rx="1.2" />
    </svg>
  );
}

export function CoverSizeSlider() {
  return (
    <label class="flex items-center gap-1.5" title="Cover size">
      <span class="sr-only">Cover size</span>
      <BookIcon class="h-2.5 w-[7px]" />
      <input
        type="range"
        class="w-24 accent-accent"
        min={COVER_SIZE.min}
        max={COVER_SIZE.max}
        step={COVER_SIZE.step}
        value={coverSize()}
        onInput={(e) => setCoverSize(Number(e.currentTarget.value))}
      />
      <BookIcon class="h-4 w-[11px]" />
    </label>
  );
}
