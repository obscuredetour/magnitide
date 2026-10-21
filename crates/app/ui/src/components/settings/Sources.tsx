import { createResource, For, Show } from 'solid-js';
import { createStore } from 'solid-js/store';

import {
  api,
  errorMessage,
  type InstalledPlugin,
  type NativePlugin,
  type SourceId,
  type SourceStatus,
} from '../../api';
import { toast } from '../../toast';
import { button, Group, Heading, help, Row, sentence, Toggle } from './parts';

const SOURCE_NOTES: Record<SourceId, string> = {
  openLibrary: 'Free and open, run by the Internet Archive. No account needed.',
  google:
    'Needs your own free API key: in the Google Cloud console, enable the Books API, then create an API key under Credentials.',
  metron:
    'Comics: issues by series and number, with credits, story arcs and characters. Needs the API token of your free metron.cloud account; only comics are looked up there.',
};

export function SourceSettings() {
  const [sources, { mutate }] = createResource(() => api.metadataSources().catch(() => []));
  const [plugins, { mutate: setPlugins }] = createResource(() =>
    api
      .pluginsStatus()
      .then((s) => s.plugins)
      .catch(() => [] as InstalledPlugin[]),
  );
  const [native, { mutate: setNative }] = createResource(() =>
    api
      .pluginsStatus()
      .then((s) => s.native)
      .catch(() => [] as NativePlugin[]),
  );
  const pluginSources = () => (plugins() ?? []).filter((p) => p.kind === 'metadataSource');
  const nativeSources = () => (native() ?? []).filter((p) => p.manifest.kind === 'metadata-source');
  // What's typed for each source's key, until saved.
  const [drafts, setDrafts] = createStore<Partial<Record<SourceId, string>>>({});
  const keyDraft = (s: SourceStatus) => drafts[s.id] ?? '';
  const canSave = (s: SourceStatus) => keyDraft(s).trim() !== '';
  // Where keys are kept is the same for every source: the system keychain,
  // or the settings file when no keychain is reachable.
  const keyStorage = () => sources()?.find((s) => s.needsKey)?.keyStorage;

  const update = async (s: SourceStatus, enabled: boolean, apiKey: string | null = null) => {
    try {
      mutate(await api.setMetadataSource(s.id, enabled, apiKey));
      if (apiKey !== null) {
        setDrafts(s.id, undefined);
        toast(apiKey ? 'API key saved' : 'API key removed');
      }
    } catch (err) {
      toast(errorMessage(err), { error: true });
    }
  };
  const save = (s: SourceStatus) => void update(s, s.enabled, keyDraft(s));

  return (
    <>
      <Heading title="Metadata Sources">
        <p>“Find Metadata…” in the edit window asks these sources for a book’s details and cover.</p>
        <p>
          Nothing is sent until you search, and then only what you searched for: title, author, ISBN, or a comic’s
          series and issue.
        </p>
      </Heading>
      <Group
        title="Built in"
        description={
          <Show when={keyStorage()}>
            {(storage) => (
              <>
                <p>Keys are kept in {storage().place}.</p>
                <Show when={storage().fallback}>
                  <p class="text-danger">
                    {storage().fallback} They stay in the settings file, readable only by you, until a keychain (GNOME
                    Keyring, KWallet or another Secret Service) is running when Magnitide starts.
                  </p>
                </Show>
              </>
            )}
          </Show>
        }
      >
        <For each={sources()}>
          {(s) => (
            <>
              <Row
                joined={s.needsKey}
                label={s.name}
                description={
                  <>
                    {SOURCE_NOTES[s.id]}
                    <Show when={s.needsKey && s.enabled && !s.hasKey}>
                      <span class="mt-1 block text-danger">Not used until a key is saved.</span>
                    </Show>
                  </>
                }
              >
                <Toggle label={`Use ${s.name}`} checked={s.enabled} onChange={(on) => void update(s, on)} />
              </Row>
              <Show when={s.needsKey}>
                <div class="-mt-1 flex items-center gap-2 border-b border-line pb-3 last:border-b-0">
                  <input
                    type="password"
                    autocomplete="off"
                    spellcheck={false}
                    aria-label={`${s.name} API key`}
                    placeholder={
                      s.hasKey ? `Key saved in ${s.keyStorage.place} · enter a new one to replace it` : 'API key'
                    }
                    class="h-7 min-w-0 flex-1 rounded-control border border-line bg-field px-2 text-xs outline-none focus:border-focus"
                    value={keyDraft(s)}
                    onInput={(e) => setDrafts(s.id, e.currentTarget.value)}
                    onKeyDown={(e) => e.key === 'Enter' && canSave(s) && save(s)}
                  />
                  <button class={button} disabled={!canSave(s)} onClick={() => save(s)}>
                    Save
                  </button>
                  <Show when={s.hasKey}>
                    <button class={button} onClick={() => void update(s, s.enabled, '')}>
                      Remove
                    </button>
                  </Show>
                </div>
              </Show>
            </>
          )}
        </For>
      </Group>
      <Show when={nativeSources().length}>
        <Group title="From Magnitide plugins">
          <For each={nativeSources()}>
            {(p) => (
              <Row label={p.manifest.name} description={sentence(p.manifest.description)}>
                <Toggle
                  label={`Use ${p.manifest.name}`}
                  checked={p.enabled}
                  onChange={(on) =>
                    void api
                      .setNativePluginEnabled(p.manifest.name, on)
                      .then(setNative, (err) => toast(errorMessage(err), { error: true }))
                  }
                />
              </Row>
            )}
          </For>
        </Group>
      </Show>
      <Show when={pluginSources().length}>
        <Group title="From calibre plugins">
          <For each={pluginSources()}>
            {(p) => (
              <Row label={p.name} description={sentence(p.description)}>
                <Toggle
                  label={`Use ${p.name}`}
                  checked={p.enabled}
                  onChange={(on) =>
                    void api
                      .setPluginEnabled(p.name, on)
                      .then(setPlugins, (err) => toast(errorMessage(err), { error: true }))
                  }
                />
              </Row>
            )}
          </For>
        </Group>
      </Show>
      <p class={`px-1 ${help}`}>Plugins add sources of their own; they’re added and removed in Plugins.</p>
    </>
  );
}
