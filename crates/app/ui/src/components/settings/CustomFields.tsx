import { createSignal, For, Show } from 'solid-js';

import { api, type CustomField, errorMessage, type FieldKind } from '../../api';
import { confirm } from '../../confirm';
import { customFields, FIELD_KINDS, loadCustomFields } from '../../fields';
import { refresh, setEditRevision } from '../../library';
import { toast } from '../../toast';
import { Select } from '../Select';
import { button, Group, help, Row } from './parts';

/** "Date Read" → "date_read", as Rust makes lookup names (a preview only). */
function keyFrom(name: string): string {
  const key = name
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '');
  return /^[0-9]/.test(key) ? `f_${key}` : key;
}

/** Custom fields: listed for any library; made, renamed and deleted in Magnitide ones. */
export function CustomFieldsSettings(props: { editable: boolean }) {
  const [adding, setAdding] = createSignal(false);
  const [editing, setEditing] = createSignal<number | null>(null);

  const changed = async () => {
    await loadCustomFields();
    setEditRevision((n) => n + 1);
    await refresh();
  };

  const remove = async (f: CustomField) => {
    const ok = await confirm({
      title: `Delete “${f.name}”?`,
      message: () => `Every book’s ${f.name} goes with it. This can’t be undone.`,
      confirm: 'Delete Field',
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteCustomField(f.id);
      await changed();
      toast(`Deleted the field “${f.name}”`);
    } catch (err) {
      toast(errorMessage(err), { error: true });
    }
  };

  const describe = (f: CustomField) =>
    [
      `#${f.key}`,
      FIELD_KINDS[f.kind] + (f.multiple ? ', several values' : ''),
      f.kind === 'choice' && f.choices.length ? f.choices.join(', ') : null,
    ]
      .filter(Boolean)
      .join(' · ');

  return (
    <Group
      title="Custom fields"
      description={
        <>
          <p>Fields of your own, like Pages or Read, edited with the rest of a book.</p>
          <p>
            They show in the details panel, and as columns in the list (pick them in its column menu). Search them with{' '}
            <code class="text-fg">#key:value</code>, as in <code class="text-fg">#pages:300</code>.
          </p>
          <p>calibre’s custom columns come across when you import a calibre library.</p>
        </>
      }
      aside={
        <Show when={props.editable && !adding()}>
          <button class={button} onClick={() => setAdding(true)}>
            Add Field…
          </button>
        </Show>
      }
    >
      <Show when={!customFields().length && !adding()}>
        <p class={`py-3 ${help}`}>No custom fields yet.</p>
      </Show>
      <For each={customFields()}>
        {(f) => (
          <Show
            when={editing() === f.id}
            fallback={
              <Row label={f.name} description={describe(f)}>
                <Show when={props.editable}>
                  <button class={button} onClick={() => setEditing(f.id)}>
                    {f.kind === 'choice' ? 'Edit…' : 'Rename…'}
                  </button>
                  <button class={button} onClick={() => void remove(f)}>
                    Delete
                  </button>
                </Show>
              </Row>
            }
          >
            <FieldForm field={f} onDone={() => setEditing(null)} onSaved={changed} />
          </Show>
        )}
      </For>
      <Show when={props.editable && adding()}>
        <FieldForm onDone={() => setAdding(false)} onSaved={changed} />
      </Show>
    </Group>
  );
}

/** Make a field, or (with `field`) rename it and edit its choices. Its kind is fixed once made. */
function FieldForm(props: { field?: CustomField; onDone(): void; onSaved(): Promise<void> }) {
  const [name, setName] = createSignal(props.field?.name ?? '');
  const [key, setKey] = createSignal('');
  const [kind, setKind] = createSignal<FieldKind>(props.field?.kind ?? 'text');
  const [multiple, setMultiple] = createSignal(false);
  const [choices, setChoices] = createSignal(props.field?.choices.join(', ') ?? '');
  const [error, setError] = createSignal<string | null>(null);
  const [saving, setSaving] = createSignal(false);
  const field =
    'h-7 w-full rounded-control border border-line bg-field px-2 text-xs outline-none select-text focus:border-focus';
  const choiceList = () =>
    choices()
      .split(',')
      .map((c) => c.trim())
      .filter(Boolean);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      if (props.field) {
        await api.updateCustomField(props.field.id, name(), kind() === 'choice' ? choiceList() : null);
      } else {
        await api.addCustomField({
          name: name(),
          key: key(),
          kind: kind(),
          multiple: multiple(),
          choices: choiceList(),
        });
      }
      await props.onSaved();
      props.onDone();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <form
      class="my-3 grid grid-cols-[7rem_1fr] items-center gap-x-3 gap-y-2 rounded-control border border-line bg-popover p-3 text-xs"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <label for="field-name" class="text-muted">
        Name
      </label>
      <input id="field-name" class={field} value={name()} autofocus onInput={(e) => setName(e.currentTarget.value)} />
      <Show when={!props.field}>
        <label for="field-key" class="text-muted">
          Lookup name
        </label>
        <input
          id="field-key"
          class={field}
          placeholder={keyFrom(name()) || 'pages'}
          value={key()}
          onInput={(e) => setKey(e.currentTarget.value)}
        />
        <label for="field-kind" class="text-muted">
          Kind
        </label>
        <Select
          wide
          id="field-kind"
          class="h-7 pl-2 text-xs outline-none focus:border-focus"
          value={kind()}
          onChange={(e) => setKind(e.currentTarget.value as FieldKind)}
        >
          <For each={Object.entries(FIELD_KINDS)}>{([k, label]) => <option value={k}>{label}</option>}</For>
        </Select>
        <Show when={kind() === 'text'}>
          <span />
          <label class="flex items-center gap-2">
            <input
              type="checkbox"
              class="size-4 accent-accent"
              checked={multiple()}
              onChange={(e) => setMultiple(e.currentTarget.checked)}
            />
            Several values per book, like tags
          </label>
        </Show>
      </Show>
      <Show when={kind() === 'choice'}>
        <label for="field-choices" class="text-muted">
          Choices
        </label>
        <input
          id="field-choices"
          class={field}
          placeholder="To Read, Reading, Read"
          value={choices()}
          onInput={(e) => setChoices(e.currentTarget.value)}
        />
      </Show>
      <Show when={error()}>
        {(e) => (
          <p class="col-span-2 text-danger select-text" role="alert">
            {e()}
          </p>
        )}
      </Show>
      <div class="col-span-2 flex justify-end gap-2">
        <button type="button" class={button} onClick={() => props.onDone()}>
          Cancel
        </button>
        <button type="submit" class={button} disabled={saving() || !name().trim()}>
          {saving() ? 'Saving…' : props.field ? 'Save' : 'Add Field'}
        </button>
      </div>
    </form>
  );
}
