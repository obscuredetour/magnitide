import { createResource, createSignal, Show } from 'solid-js';
import TriangleAlert from 'lucide-solid/icons/triangle-alert';

import { api, errorMessage, type ServerStatus } from '../../api';
import { toast } from '../../toast';
import { button, Callout, Group, Heading, help, Row, Toggle } from './parts';

const mac = /Mac/.test(navigator.userAgent);

export function ServerSettings() {
  const [status, { mutate }] = createResource(() => api.serverStatus().catch(() => undefined));
  const [port, setPort] = createSignal('');
  const [password, setPassword] = createSignal('');
  const [busy, setBusy] = createSignal(false);

  const update = async (enabled: boolean, newPassword: string | null = null) => {
    const current = status();
    if (!current) return;
    const n = Number(port() || current.port);
    if (!Number.isInteger(n) || n < 1024 || n > 65535) {
      toast('Pick a port from 1024 to 65535.', { error: true });
      return;
    }
    setBusy(true);
    try {
      const next: ServerStatus = await api.setServer(enabled, n, newPassword);
      mutate(next);
      setPort('');
      if (newPassword !== null) {
        setPassword('');
        toast(newPassword ? 'Password saved' : 'Password removed');
      }
    } catch (err) {
      toast(errorMessage(err), { error: true });
    } finally {
      setBusy(false);
    }
  };

  const copy = (text: string) =>
    navigator.clipboard.writeText(text).then(
      () => toast('Address copied'),
      () => toast("Couldn't copy the address", { error: true }),
    );

  return (
    <>
      <Heading title="Library Server">
        <p>Shares the open library on your Wi-Fi while Magnitide is running.</p>
        <p>
          E-reader web browsers (Kobo’s, Kindle’s, a phone’s) get a simple page to download books from. Reading apps
          such as KOReader can add it as an OPDS catalogue, and sync reading positions through it.
        </p>
        <p>It’s read-only: nothing on it can change your library.</p>
      </Heading>
      <Show when={status()}>
        {(s) => (
          <>
            <Group>
              <Row
                label="Share the library"
                description={
                  <Show when={s().error} fallback="Off until you turn it on; remembered after that.">
                    <span class="text-danger">{s().error}</span>
                  </Show>
                }
              >
                <Toggle
                  label="Share the library on the network"
                  checked={s().enabled}
                  disabled={busy()}
                  onChange={(on) => void update(on)}
                />
              </Row>
              <Row label="Port" description="Change it if another app already uses this one.">
                <input
                  type="number"
                  min="1024"
                  max="65535"
                  aria-label="Port"
                  class="h-7 w-24 rounded-control border border-line bg-field px-2 text-xs tabular-nums outline-none focus:border-focus"
                  value={port() || String(s().port)}
                  onInput={(e) => setPort(e.currentTarget.value)}
                  onKeyDown={(e) => e.key === 'Enter' && port() && void update(s().enabled)}
                  onBlur={() => port() && Number(port()) !== s().port && void update(s().enabled)}
                />
              </Row>
              <Row
                label="Password"
                description={
                  <>
                    {s().hasPassword
                      ? 'Browsers and apps ask for it (any user name works). '
                      : 'Without one, anyone on your network can browse and download your books. '}
                    Kept in {s().passwordStorage.place}.
                    <Show when={s().passwordStorage.fallback}>
                      <span class="mt-1 block text-danger">
                        {s().passwordStorage.fallback} It stays in the settings file, readable only by you, until a
                        keychain is running when Magnitide starts.
                      </span>
                    </Show>
                  </>
                }
              >
                <input
                  type="password"
                  autocomplete="new-password"
                  aria-label="Server password"
                  placeholder={s().hasPassword ? 'Saved · type to replace' : 'None'}
                  class="h-7 w-44 rounded-control border border-line bg-field px-2 text-xs outline-none focus:border-focus"
                  value={password()}
                  onInput={(e) => setPassword(e.currentTarget.value)}
                  onKeyDown={(e) => e.key === 'Enter' && password() && void update(s().enabled, password())}
                />
                <button
                  class={button}
                  disabled={!password() || busy()}
                  onClick={() => void update(s().enabled, password())}
                >
                  Save
                </button>
                <Show when={s().hasPassword}>
                  <button class={button} disabled={busy()} onClick={() => void update(s().enabled, '')}>
                    Remove
                  </button>
                </Show>
              </Row>
            </Group>
            <Show when={s().running && s().url}>
              {(url) => (
                <Group title="Connecting">
                  <div class="flex gap-5 py-4">
                    <Show when={s().qrSvg}>
                      {(svg) => (
                        <div
                          class="size-36 flex-none overflow-hidden rounded-inset bg-white p-1.5 [&>svg]:size-full"
                          role="img"
                          aria-label={`QR code for ${url()}`}
                          // Our own SVG, made by the qrcode crate from the address.
                          innerHTML={svg()}
                        />
                      )}
                    </Show>
                    <div class="min-w-0 flex-1 space-y-3">
                      <div>
                        <div class="flex items-center gap-2">
                          <span class="font-mono text-sm font-medium break-all select-all">{url()}</span>
                          <button class={button} onClick={() => void copy(url())}>
                            Copy
                          </button>
                        </div>
                        <Show when={s().otherUrls.length > 0}>
                          <p class={`mt-1 ${help}`}>
                            On another network: <span class="font-mono select-all">{s().otherUrls.join(', ')}</span>
                          </p>
                        </Show>
                      </div>
                      <dl class="space-y-2">
                        <div>
                          <dt class="font-medium">Kobo</dt>
                          <dd class={help}>
                            More › Beta Features › Web Browser, then go to this address. Books download straight into
                            your library, as KEPUB.
                          </dd>
                        </div>
                        <div>
                          <dt class="font-medium">KOReader and other reading apps</dt>
                          <dd class={help}>
                            Add an OPDS catalogue at <span class="font-mono break-all select-all">{url()}opds</span>.
                          </dd>
                        </div>
                        <div>
                          <dt class="font-medium">KOReader progress sync</dt>
                          <dd class={help}>
                            In a book, Tools › Progress sync › Custom sync server:{' '}
                            <span class="font-mono break-all select-all">{url().replace(/\/$/, '')}</span>. Then Log in
                            with any user name and{' '}
                            {s().hasPassword ? 'the password above' : 'any password (set one above to keep others out)'}
                            . Each user name has its own reading positions.
                          </dd>
                        </div>
                      </dl>
                    </div>
                  </div>
                </Group>
              )}
            </Show>
            <Show when={s().running && (mac || s().vpn)}>
              <Callout icon={TriangleAlert} title="If devices can’t connect" tone="warning">
                <Show when={mac}>
                  <p>If macOS asks whether Magnitide may accept incoming connections, allow it.</p>
                </Show>
                <Show when={s().vpn}>
                  <p>
                    A VPN is on: allow local network (LAN) connections in its settings, or turn it off while you
                    download.
                  </p>
                </Show>
              </Callout>
            </Show>
          </>
        )}
      </Show>
    </>
  );
}
