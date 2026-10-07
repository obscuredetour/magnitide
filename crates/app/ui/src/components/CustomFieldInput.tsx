// One custom field's value, edited the way its kind needs (Edit Book and
// Edit Books). Null is no value.

import { For, onMount, Show } from 'solid-js';

import type { CustomField, FieldValue } from '../api';
import { Select } from './Select';

/** Selects are drawn by `Select`, which brings the border and arrow. */
const SELECT = 'h-8 pl-2.5 outline-none focus:border-focus focus:ring-3 focus:ring-focus/20';

const asText = (v: FieldValue | null): string =>
  typeof v === 'string' ? v : typeof v === 'number' ? String(v) : Array.isArray(v) ? v.join(', ') : '';
const asNumber = (v: FieldValue | null): number | null => (typeof v === 'number' ? v : null);
const orNull = (s: string): string | null => (s.trim() ? s : null);

export function CustomFieldInput(props: {
  field: CustomField;
  value: FieldValue | null;
  onChange(value: FieldValue | null): void;
  /** Classes for text inputs and selects. */
  inputClass: string;
  /** Bulk edit: the books differ, and nothing has been entered. */
  mixed?: boolean;
}) {
  const f = () => props.field;
  const placeholder = () => (props.mixed ? 'Mixed — left as each book has it' : '');
  const id = () => `custom-${f().key}`;

  switch (props.field.kind) {
    case 'longText': {
      let box!: HTMLDivElement;
      onMount(() => {
        box.innerHTML = typeof props.value === 'string' ? props.value : '';
      });
      return (
        <div
          ref={box}
          id={id()}
          contentEditable
          role="textbox"
          aria-multiline="true"
          aria-label={f().name}
          data-placeholder={placeholder()}
          class="comments-html min-h-16 overflow-y-auto rounded-control border border-line bg-field px-3 py-2 font-serif text-sm/[1.6] outline-none select-text empty:before:text-faint empty:before:content-[attr(data-placeholder)] focus:border-focus focus:ring-3 focus:ring-focus/20"
          onInput={() => props.onChange(box.textContent?.trim() ? box.innerHTML : null)}
        />
      );
    }
    case 'integer':
    case 'number':
      return (
        <input
          id={id()}
          class={props.inputClass + ' tabular-nums'}
          type="number"
          step={f().kind === 'integer' ? 1 : 'any'}
          placeholder={placeholder()}
          value={asNumber(props.value) ?? ''}
          onInput={(e) => {
            const n = e.currentTarget.valueAsNumber;
            props.onChange(e.currentTarget.value === '' || Number.isNaN(n) ? null : n);
          }}
        />
      );
    case 'bool':
      return (
        <Select
          wide
          id={id()}
          class={SELECT}
          value={props.value === true ? 'yes' : props.value === false ? 'no' : ''}
          onChange={(e) => {
            const v = e.currentTarget.value;
            props.onChange(v === 'yes' ? true : v === 'no' ? false : null);
          }}
        >
          <option value="">{props.mixed ? 'Mixed' : '—'}</option>
          <option value="yes">Yes</option>
          <option value="no">No</option>
        </Select>
      );
    case 'date':
      return (
        <input
          id={id()}
          class={props.inputClass}
          type="date"
          value={typeof props.value === 'string' ? props.value : ''}
          onInput={(e) => props.onChange(orNull(e.currentTarget.value))}
        />
      );
    case 'rating': {
      const stars = () => Math.round((asNumber(props.value) ?? 0) / 2);
      return (
        <div class="flex h-8 items-center gap-0.5" role="radiogroup" aria-label={f().name}>
          <For each={[1, 2, 3, 4, 5]}>
            {(n) => (
              <button
                type="button"
                role="radio"
                aria-checked={stars() === n}
                aria-label={`${n} star${n > 1 ? 's' : ''}`}
                class="cursor-pointer px-0.5 text-lg leading-none"
                classList={{ 'text-star': n <= stars(), 'text-faint hover:text-star': n > stars() }}
                onClick={() => props.onChange(stars() === n ? null : n * 2)}
              >
                ★
              </button>
            )}
          </For>
          <Show when={props.mixed && props.value === null}>
            <span class="ml-2 text-xs text-faint">Mixed</span>
          </Show>
          <Show when={props.value !== null}>
            <button
              type="button"
              class="ml-2 cursor-pointer text-xs text-muted hover:text-fg"
              onClick={() => props.onChange(null)}
            >
              Clear
            </button>
          </Show>
        </div>
      );
    }
    case 'choice': {
      // A value no longer among the choices (they were changed) stays selectable.
      const options = () => {
        const v = asText(props.value);
        return v && !f().choices.includes(v) ? [...f().choices, v] : f().choices;
      };
      return (
        <Select
          wide
          id={id()}
          class={SELECT}
          value={asText(props.value)}
          onChange={(e) => props.onChange(orNull(e.currentTarget.value))}
        >
          <option value="">{props.mixed ? 'Mixed' : '—'}</option>
          <For each={options()}>{(c) => <option value={c}>{c}</option>}</For>
        </Select>
      );
    }
    case 'series': {
      const series = () =>
        props.value && typeof props.value === 'object' && !Array.isArray(props.value)
          ? props.value
          : { name: '', index: 1 };
      return (
        <div class="flex gap-2">
          <input
            id={id()}
            class={props.inputClass}
            placeholder={placeholder()}
            value={series().name}
            onInput={(e) =>
              props.onChange(
                e.currentTarget.value.trim() ? { name: e.currentTarget.value, index: series().index } : null,
              )
            }
          />
          <input
            class={props.inputClass + ' w-20 flex-none tabular-nums'}
            type="number"
            min="0"
            step="any"
            aria-label={`${f().name} number`}
            value={series().index}
            disabled={!series().name.trim()}
            onInput={(e) => {
              const n = e.currentTarget.valueAsNumber;
              if (!Number.isNaN(n)) props.onChange({ name: series().name, index: n });
            }}
          />
        </div>
      );
    }
    default:
      // Text. Several values are edited comma-separated, like tags, and sent as
      // typed: they're split when saved.
      return (
        <input
          id={id()}
          class={props.inputClass}
          placeholder={placeholder()}
          value={asText(props.value)}
          onInput={(e) => props.onChange(orNull(e.currentTarget.value))}
        />
      );
  }
}

/** Hint under a field's input, where its kind needs one. */
export function customFieldHint(field: CustomField): string | undefined {
  if (field.kind === 'text' && field.multiple) return 'Comma-separated.';
  return undefined;
}
