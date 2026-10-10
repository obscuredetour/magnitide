import { createResource, For, Show } from 'solid-js';

import { api, type DeviceKind } from '../../api';
import { DEVICE_FORMATS, DEVICE_KIND_NAMES, preferredFormat, setPreferredFormat } from '../../devices';
import { Select } from '../Select';
import { Group, Heading, Row } from './parts';

export function DeviceSettings() {
  const kinds: DeviceKind[] = ['kobo', 'kindle', 'generic'];
  const [conversion] = createResource(() => api.conversionInfo().catch(() => ({ calibre: null })));
  return (
    <>
      <Heading title="Devices">
        <p>E-readers connected by USB show up in the toolbar, with the books on them.</p>
      </Heading>

      <Group
        title="Format to send"
        description={
          <>
            <p>
              When a book has more than one format a device reads, Magnitide asks which to send, unless you pick one
              here.
            </p>
            <p>
              A book without that format is converted on the way when it can be, or else sent in the best format it has.
              In a Magnitide library the converted file is kept with the book, so it’s only converted once.
            </p>
          </>
        }
      >
        <For each={kinds}>
          {(kind) => (
            <Row
              label={DEVICE_KIND_NAMES[kind]}
              description={`Reads ${DEVICE_FORMATS[kind].slice(0, 4).join(', ')}${kind === 'kindle' ? ' (not EPUB over USB)' : ''}…`}
            >
              <Select
                class="h-7 pl-2 text-xs"
                aria-label={`Format to send to ${DEVICE_KIND_NAMES[kind]}`}
                value={preferredFormat(kind) ?? ''}
                onChange={(e) => setPreferredFormat(kind, e.currentTarget.value || null)}
              >
                <option value="">Ask each time</option>
                <For each={DEVICE_FORMATS[kind]}>{(f) => <option value={f}>{f}</option>}</For>
              </Select>
            </Row>
          )}
        </For>
      </Group>

      <Group title="Converting">
        <Row label="Built in" description="EPUB to KEPUB, for Kobo, and EPUB to AZW3, for Kindle." />
        <Row
          label="With calibre"
          description={
            <Show when={conversion()} fallback="Checking…">
              {(c) =>
                c().calibre ? (
                  <>
                    Everything else, from and to MOBI, PDF and more, uses calibre’s converter at{' '}
                    <span class="break-all">{c().calibre}</span>.
                  </>
                ) : (
                  <>
                    Install calibre to also convert to MOBI, PDF and more, or from other formats. Magnitide finds it by
                    itself.
                  </>
                )
              }
            </Show>
          }
        />
      </Group>
    </>
  );
}
