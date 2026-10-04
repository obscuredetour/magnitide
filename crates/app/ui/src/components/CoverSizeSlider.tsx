import RectangleVertical from 'lucide-solid/icons/rectangle-vertical';
import { COVER_SIZE, coverSize, setCoverSize } from '../view';
import { Icon } from './Icon';

function BookIcon(props: { size: 12 | 16 }) {
  return <Icon of={RectangleVertical} size={props.size} class="text-faint" />;
}

export function CoverSizeSlider() {
  return (
    <label class="flex items-center gap-1.5" title="Cover size">
      <span class="sr-only">Cover size</span>
      <BookIcon size={12} />
      <input
        type="range"
        class="w-24 accent-accent"
        min={COVER_SIZE.min}
        max={COVER_SIZE.max}
        step={COVER_SIZE.step}
        value={coverSize()}
        onInput={(e) => setCoverSize(Number(e.currentTarget.value))}
      />
      <BookIcon size={16} />
    </label>
  );
}
