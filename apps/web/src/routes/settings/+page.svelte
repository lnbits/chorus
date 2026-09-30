<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { api, session, bytes, date, errorMessage } from '$lib/api';
  type Relay = { url: string; enabled: boolean; read: boolean; write: boolean; private: boolean };
  type Settings = {
    username: string;
    node_npub: string;
    member_npub: string;
    key_storage: string;
    session_ttl: number;
    sessions: { created_at: number; expires_at: number }[];
    relays: Relay[];
    relay_health: { url: string; status: string }[];
    vm: {
      version: string;
      max_memory: number;
      max_fuel: number;
      max_output: number;
      max_wasm: number;
    };
    frost: string;
  };
  let data = $state<Settings | null>(null);
  let error = $state('');
  let notice = $state('');
  let tab = $state('Security');
  let busy = $state(false);
  let relayUrl = $state('');
  let oldPassword = $state('');
  let newPassword = $state('');
  let confirmPassword = $state('');
  async function load() {
    error = '';
    try {
      data = await api<Settings>('/settings');
    } catch (e) {
      error = errorMessage(e);
    }
  }
  onMount(load);
  async function save() {
    if (!data) return;
    busy = true;
    error = '';
    notice = '';
    try {
      await api('/settings/relays', { method: 'POST', body: JSON.stringify(data.relays) });
      notice = 'Relay settings saved. Connections are updating.';
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  function add() {
    if (!data || !relayUrl.trim()) return;
    data.relays = [
      ...data.relays,
      { url: relayUrl.trim(), enabled: true, read: true, write: true, private: false }
    ];
    relayUrl = '';
  }
  async function password() {
    if (newPassword !== confirmPassword) {
      error = 'New passwords do not match.';
      return;
    }
    busy = true;
    error = '';
    try {
      await api('/settings/password', {
        method: 'POST',
        body: JSON.stringify({ current_password: oldPassword, new_password: newPassword })
      });
      oldPassword = '';
      newPassword = '';
      confirmPassword = '';
      session.set({ authenticated: false });
      await goto('/login');
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:head><title>Settings — Chorus</title></svelte:head>
<div class="page-head">
  <div>
    <p class="eyebrow">Runtime controls</p>
    <h1>Settings</h1>
  </div>
  {#if tab === 'Relays'}<button onclick={save} disabled={busy}
      >{busy ? 'Saving…' : 'Save settings'}</button
    >{:else}<button onclick={load}>Refresh</button>{/if}
</div>
{#if error}<div class="notice error" role="alert">
    {error}{#if !$session.authenticated}
      <a class="inline-link" href="/login">Login</a>{/if}
  </div>{/if}{#if notice}<div class="notice success" role="status">{notice}</div>{/if}
{#if data}<div class="panel">
    <div class="panel-body identity-banner">
      <div>
        <p class="eyebrow" style="font-size:11px;font-weight:bold">SIGNING IDENTITY</p>
        <h2>Keys stay on your node.</h2>
        <p class="muted">
          Admin access, Nostr communication and member signing use separate identities.
        </p>
      </div>
      <div class="identity-meta">
        <div><span>ADMIN</span><strong>{data.username}</strong></div>
        <div><span>KEY STORAGE</span><strong>Encrypted at rest</strong></div>
        <div><span>VM PROFILE</span><strong>{data.vm.version}</strong></div>
      </div>
    </div>
  </div>
  <div class="tabs">
    {#each ['Security', 'Nostr', 'Relays', 'Node', 'Storage', 'FedVM'] as name (name)}<button
        class:active={tab === name}
        onclick={() => (tab = name)}>{name}</button
      >{/each}
  </div>
  {#if tab === 'Security'}<div class="panel">
      <div class="panel-head"><h2>Admin security</h2></div>
      <div class="panel-body">
        <div class="fields">
          <label
            >ADMIN USERNAME<input readonly value={data.username} />
            <p class="field-help">Separate from Nostr and federation signing identities.</p></label
          ><label
            >SESSION EXPIRY<input readonly value={`${data.session_ttl / 3600} hours`} />
            <p class="field-help">
              Configured on the node. All sessions expire automatically.
            </p></label
          >
        </div>
        <div class="kv" style="margin-top:25px">
          <span>PASSWORD HASHING</span><span>Argon2id · 64 MiB · 3 iterations</span>
        </div>
        <div class="kv"><span>PASSKEYS</span><span class="muted">Not yet available</span></div>
      </div>
    </div>
    <div class="detail-grid">
      <div class="panel">
        <div class="panel-head"><h2>Change password</h2></div>
        <form
          class="panel-body"
          onsubmit={(e) => {
            e.preventDefault();
            password();
          }}
        >
          <label
            >CURRENT PASSWORD<input
              type="password"
              autocomplete="current-password"
              bind:value={oldPassword}
              required
            /></label
          ><label style="margin-top:20px"
            >NEW PASSWORD<input
              type="password"
              autocomplete="new-password"
              minlength="12"
              bind:value={newPassword}
              required
            /></label
          ><label style="margin-top:20px"
            >CONFIRM NEW PASSWORD<input
              type="password"
              autocomplete="new-password"
              bind:value={confirmPassword}
              required
            /></label
          >
          <p class="field-help">
            At least 12 characters. Changing your password signs out every session.
          </p>
          <button style="margin-top:22px" disabled={busy}>Change password</button>
        </form>
      </div>
      <div class="panel">
        <div class="panel-head">
          <h2>Active sessions</h2>
          <span class="mono">{data.sessions.length}</span>
        </div>
        {#each data.sessions as s, i (i)}<div
            class="panel-body"
            style="border-bottom:1px solid var(--border)"
          >
            <p style="margin:0;font-size:14px">Created {date(s.created_at)}</p>
            <p class="field-help">Expires {date(s.expires_at)}</p>
          </div>{/each}
      </div>
    </div>
  {:else if tab === 'Nostr'}<div class="panel">
      <div class="panel-head"><h2>Public identities</h2></div>
      <div class="panel-body">
        <div class="kv"><span>NODE NPUB</span><code class="hash">{data.node_npub}</code></div>
        <div class="kv"><span>MEMBER NPUB</span><code class="hash">{data.member_npub}</code></div>
        <div class="kv"><span>KEY STORAGE</span><span>{data.key_storage}</span></div>
        <div class="kv">
          <span>SIGNING</span><span
            >Results are signed only after independent verified execution.</span
          >
        </div>
        <div class="kv"><span>FROST</span><span>{data.frost}</span></div>
      </div>
    </div>
  {:else if tab === 'Relays'}<div class="panel">
      <div class="panel-head">
        <h2>Nostr relays</h2>
        <button class="small" disabled={busy} onclick={save}>Reconnect</button>
      </div>
      {#each data.relays as r, i (r.url)}<div
          class="panel-body"
          style="border-bottom:1px solid var(--border)"
        >
          <div class="actions" style="justify-content:space-between">
            <div class="actions">
              <span
                class="dot {data.relay_health.find((h) => h.url === r.url)?.status === 'connected'
                  ? 'green'
                  : 'red'}"
              ></span><span class="hash">{r.url}</span>
            </div>
            <button
              class="small"
              onclick={() => {
                if (data) data.relays = data.relays.filter((_, j) => j !== i);
              }}>Remove</button
            >
          </div>
          <div class="actions" style="margin-top:20px;gap:25px">
            <label class="check-label"
              ><input type="checkbox" bind:checked={r.enabled} />Enabled</label
            ><label class="check-label"><input type="checkbox" bind:checked={r.read} />Read</label
            ><label class="check-label"><input type="checkbox" bind:checked={r.write} />Write</label
            ><label class="check-label"
              ><input type="checkbox" bind:checked={r.private} />Private relay</label
            ><span class="muted" style="font-size:12px"
              >{data.relay_health.find((h) => h.url === r.url)?.status || 'not connected'}</span
            >
          </div>
        </div>{:else}<div class="empty">
          <h2>No relays configured.</h2>
          <p>Add multiple independent relays to avoid relying on a single communications path.</p>
        </div>{/each}
      <form
        class="panel-body actions"
        onsubmit={(e) => {
          e.preventDefault();
          add();
        }}
      >
        <label style="flex:1"
          ><span class="sr-only">New relay URL</span><input
            type="url"
            placeholder="wss://relay.example.com"
            bind:value={relayUrl}
            required
          /></label
        ><button>Add relay</button>
      </form>
    </div>
    <p class="field-help">
      Save settings to apply changes. Private federation payloads use encrypted NIP-17 messages,
      regardless of relay designation.
    </p>
  {:else if tab === 'FedVM'}<div class="panel">
      <div class="panel-head"><h2>Deterministic execution profile</h2></div>
      <div class="panel-body">
        <div class="fields">
          {#each [['VM VERSION', data.vm.version], ['MAXIMUM MEMORY', bytes(data.vm.max_memory)], ['EXECUTION FUEL', data.vm.max_fuel.toLocaleString()], ['MAXIMUM OUTPUT', bytes(data.vm.max_output)], ['MAXIMUM WASM', bytes(data.vm.max_wasm)], ['HOST CAPABILITIES', 'None · no WASI']] as [label, value] (label)}<label
              >{label}<input readonly {value} /></label
            >{/each}
        </div>
        <p class="field-help" style="margin-top:25px">
          These limits are part of the versioned consensus profile and cannot be changed through
          local settings.
        </p>
      </div>
    </div>
  {:else if tab === 'Storage'}<div class="panel">
      <div class="panel-head"><h2>SQLite storage</h2></div>
      <div class="panel-body">
        <div class="kv">
          <span>DURABILITY</span><span>WAL journal · synchronous FULL · foreign keys enabled</span>
        </div>
        <div class="kv">
          <span>HISTORY</span><span
            >Federation epochs, signed evidence and final results are immutable.</span
          >
        </div>
        <div class="kv">
          <span>SIGNING GUARDS</span><span>Persisted before signatures are generated.</span>
        </div>
        <div class="kv">
          <span>BACKUP</span><span
            >Use SQLite’s backup API or stop the node before copying the data directory. Keep
            encrypted vaults and their unlock credential separately.</span
          >
        </div>
      </div>
    </div>
  {:else}<div class="panel">
      <div class="panel-head"><h2>Headless federation node</h2></div>
      <div class="panel-body">
        <div class="kv"><span>PROTOCOL</span><span>chorus/1</span></div>
        <div class="kv"><span>API</span><code>/api/v1</code></div>
        <div class="kv">
          <span>HEALTH</span><a class="inline-link" href="/healthz">Open health endpoint ↗</a>
        </div>
        <div class="kv">
          <span>RUNTIME</span><span>The node runs independently of the desktop window and UI.</span>
        </div>
      </div>
    </div>{/if}
{:else if !error}<div class="loading">Loading settings…</div>{/if}

<style>
  .identity-banner {
    display: grid;
    grid-template-columns: 1fr 1.3fr;
    gap: 25px;
  }
  .identity-banner h2 {
    font-size: 23px;
    margin: 9px 0;
  }
  .identity-banner p:not(.eyebrow) {
    font-size: 13px;
    margin: 10px 0 0;
  }
  .identity-meta {
    display: grid;
    grid-template-columns: 0.8fr 1fr 1.3fr;
    border: 1px solid var(--border);
    border-radius: 3px;
  }
  .identity-meta > div {
    padding: 15px;
    border-right: 1px solid var(--border);
  }
  .identity-meta > div:last-child {
    border: 0;
  }
  .identity-meta span {
    display: block;
    font-size: 10px;
    color: var(--muted);
    margin-bottom: 12px;
  }
  .identity-meta strong {
    font-size: 14px;
    overflow-wrap: anywhere;
  }
  @media (max-width: 1100px) {
    .identity-banner {
      grid-template-columns: 1fr;
    }
  }
  @media (max-width: 760px) {
    .identity-meta > div {
      padding: 12px;
    }
    .identity-meta strong {
      font-size: 12px;
    }
  }
</style>
