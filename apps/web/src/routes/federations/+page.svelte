<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import {
    api,
    session,
    short,
    thresholdLabel,
    errorMessage,
    hex,
    type Federation
  } from '$lib/api';
  import Icon from '$lib/Icon.svelte';
  import Modal from '$lib/Modal.svelte';
  let data = $state<Federation[]>([]);
  let loading = $state(true);
  let error = $state('');
  let query = $state('');
  let filter = $state('all');
  let modal = $state<'create' | 'join' | null>(null);
  let busy = $state(false);
  let formError = $state('');
  let name = $state('');
  let description = $state('');
  let isPublic = $state(true);
  let members = $state('');
  let threshold = $state(1);
  const memberCount = $derived(new Set(members.split(/[\s,]+/).filter(Boolean)).size);
  const minimumQuorum = $derived(Math.floor(memberCount / 2) + 1);
  $effect(() => {
    if (threshold < minimumQuorum) threshold = minimumQuorum;
  });
  let wasmHex = $state('');
  let wasmName = $state('');
  let relays = $state('');
  let initialState = $state('');
  let stateful = $state(false);
  let communicationKeys = $state('');
  let invitation = $state('');
  let genesisHash = $state('');
  const visible = $derived(
    data.filter(
      (f) =>
        (filter === 'all' || (filter === 'public' ? f.public : !f.public)) &&
        `${f.name} ${f.description} ${f.federation_id}`.toLowerCase().includes(query.toLowerCase())
    )
  );
  async function load() {
    loading = true;
    error = '';
    try {
      data = await api<Federation[]>('/federations');
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);
  async function openCreate() {
    modal = 'create';
    formError = '';
    try {
      const s = await api<{ member_npub: string; relays: { url: string }[] }>('/settings');
      if (!members) members = s.member_npub;
      if (!relays) relays = s.relays.map((r) => r.url).join('\n');
    } catch (e) {
      formError = errorMessage(e);
    }
  }
  async function chooseWasm(e: Event) {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (!file) return;
    if (file.size > 4 * 1024 * 1024) {
      formError = 'WASM must be 4 MB or smaller.';
      return;
    }
    wasmHex = hex(new Uint8Array(await file.arrayBuffer()));
    wasmName = file.name;
  }
  async function create() {
    busy = true;
    formError = '';
    try {
      const pairs = communicationKeys
        .split('\n')
        .map((s) => s.trim())
        .filter(Boolean)
        .map((s) => s.split(/\s+/));
      const result = await api<Federation>('/federations', {
        method: 'POST',
        body: JSON.stringify({
          name,
          description,
          public: isPublic,
          members: members.split(/[\s,]+/).filter(Boolean),
          threshold: {
            type: 'absolute',
            value: threshold
          },
          wasm_hex: wasmHex,
          relays: relays.split(/[\s,]+/).filter(Boolean),
          initial_state_hex: stateful ? hex(new TextEncoder().encode(initialState)) : null,
          communication_keys: pairs
        })
      });
      modal = null;
      await goto(`/federations/${result.federation_id}`);
    } catch (e) {
      formError = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  async function join() {
    busy = true;
    formError = '';
    try {
      const result = await api<Federation>('/federations/join', {
        method: 'POST',
        body: JSON.stringify({
          invitation: JSON.parse(invitation),
          genesis_hash: genesisHash.trim()
        })
      });
      modal = null;
      await goto(`/federations/${result.federation_id}`);
    } catch (e) {
      formError = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  async function readInvite(e: Event) {
    const f = (e.target as HTMLInputElement).files?.[0];
    if (f) {
      if (f.size > 12 * 1024 * 1024) {
        formError = 'Invitation is too large.';
        return;
      }
      invitation = await f.text();
    }
  }
</script>

<svelte:head><title>Federations — Chorus</title></svelte:head>
<div class="page-head">
  <div>
    <p class="eyebrow">Independent execution. Shared agreement.</p>
    <h1>Federations</h1>
  </div>
  <div class="actions">
    {#if $session.authenticated}<button
        onclick={() => {
          modal = 'join';
          formError = '';
        }}>Join federation</button
      ><button class="primary" onclick={openCreate}
        ><Icon name="plus" size={16} />Create federation</button
      >{:else}<a class="button" href="/login"
        >Login to manage your node <span aria-hidden="true">↗</span></a
      >{/if}
  </div>
</div>
<div class="metrics">
  <div class="metric">
    <span class="metric-label">Federations</span><strong>{loading ? '—' : data.length}</strong
    ><small>Known to this node</small>
  </div>
  <div class="metric">
    <span class="metric-label">Active members</span><strong
      >{loading ? '—' : new Set(data.flatMap((f) => f.members)).size}</strong
    ><small>Unique signing identities</small>
  </div>
  <div class="metric">
    <span class="metric-label">Finalized jobs</span><strong
      >{loading ? '—' : data.reduce((n, f) => n + f.finalized_jobs, 0)}</strong
    ><small>Threshold-attested results</small>
  </div>
  <div class="metric">
    <span class="metric-label">Protocol</span><strong>01</strong><small
      >Federated deterministic WASM</small
    >
  </div>
</div>
<div class="toolbar">
  <div class="segments" aria-label="Federation visibility">
    {#each ['all', 'public', 'private'] as f (f)}{#if f !== 'private' || $session.authenticated}<button
          class:active={filter === f}
          onclick={() => (filter = f)}
          >{f === 'all' ? 'All federations' : f[0].toUpperCase() + f.slice(1)}</button
        >{/if}{/each}
  </div>
  <label
    ><span class="sr-only">Search federations</span><input
      class="search"
      placeholder="Search name or federation ID"
      bind:value={query}
    /></label
  >
</div>
{#if error}<div role="alert" class="notice error">
    {error}<button class="small" style="margin-left:15px" onclick={load}>Retry</button>
  </div>{/if}
<div class="panel">
  {#if loading}<div class="loading">Loading federations…</div>{:else if error}<div class="empty">
      <h2>Waiting for your node.</h2>
      <p>Your federation data will appear when the connection is restored.</p>
    </div>{:else if visible.length === 0}<div class="empty">
      <div class="empty-symbol"><Icon name="federations" size={22} /></div>
      <h2>{query ? 'No matching federations.' : 'A shared result starts here.'}</h2>
      <p>
        {query
          ? 'Try a different name or federation ID.'
          : $session.authenticated
            ? 'Create a federation with your members, contract and threshold. Or join an existing federation with a verified invitation.'
            : 'No public federations are available on this node yet. Sign in to create or join a federation.'}
      </p>
      {#if !query}<div class="actions">
          {#if $session.authenticated}<button onclick={openCreate}
              >Create federation <Icon name="arrow" size={15} /></button
            >{:else}<a href="/protocol" class="button"
              >How it works <Icon name="arrow" size={15} /></a
            >{/if}
        </div>{/if}
    </div>{:else}<div class="table-wrap">
      <table>
        <thead
          ><tr
            ><th>Federation</th><th>Members / threshold</th><th>Contract</th><th>Status</th><th
              ><span class="sr-only">Open</span></th
            ></tr
          ></thead
        ><tbody
          >{#each visible as f (f.federation_id)}<tr
              ><td
                ><div class="federation-name">
                  <div class="federation-monogram">{f.name.slice(0, 1).toUpperCase()}</div>
                  <div>
                    <a href={`/federations/${f.federation_id}`}><strong>{f.name}</strong></a>
                    <p>{f.public ? 'PUBLIC' : 'PRIVATE'} · EPOCH {f.epoch}</p>
                    <p class="mono">{short(f.federation_id)}</p>
                  </div>
                </div></td
              ><td
                >{f.members.length} members
                <p>{thresholdLabel(f.threshold, f.members.length)} · {f.required} required</p></td
              ><td
                ><span class="mono">{short(f.wasm_hash)}</span>
                <p>{f.vm_version}</p></td
              ><td
                ><span class="status"
                  ><span class="dot {f.status === 'active' ? 'green' : 'red'}"
                  ></span>{f.status}</span
                >
                <p>{f.finalized_jobs} finalized</p></td
              ><td
                ><a class="button small" href={`/federations/${f.federation_id}`}
                  >Open <span aria-hidden="true">↗</span></a
                ></td
              ></tr
            >{/each}</tbody
        >
      </table>
    </div>{/if}
</div>
<div class="page-foot">
  <span>{visible.length} federations · {data.filter((f) => f.public).length} public</span><a
    href="/protocol">WASM is the contract. Nostr is the communications layer. ↗</a
  >
</div>
{#if modal === 'create'}<Modal
    title="Create federation"
    onclose={() => {
      if (!busy) modal = null;
    }}
    >{#if formError}<div class="notice error" role="alert">{formError}</div>{/if}
    <div class="fields">
      <label>NAME<input bind:value={name} placeholder="Federation name" maxlength="128" /></label
      ><label
        >VISIBILITY<select bind:value={isPublic}
          ><option value={true}>Public</option><option value={false}>Private · NIP-17</option
          ></select
        ></label
      ><label class="wide"
        >DESCRIPTION<textarea
          rows="2"
          bind:value={description}
          placeholder="What does this federation compute?"
          maxlength="4096"
        ></textarea></label
      ><label class="wide"
        >INITIAL MEMBER NPUBS<textarea rows="3" class="mono" bind:value={members}></textarea>
        <p class="field-help">
          One npub per line. Include this node’s member identity. The creator has no permanent
          special authority.
        </p></label
      ><label class="wide"
        >QUORUM · REQUIRED SIGNATURES<input
          type="number"
          min={minimumQuorum}
          max={memberCount || 1}
          step="1"
          bind:value={threshold}
        />
        <p class="field-help">
          {threshold}/{memberCount} members must agree. The quorum must exceed half the members. Future
          changes require approval under the current quorum.
        </p></label
      ><label class="wide"
        >WASM CONTRACT<input type="file" accept=".wasm" onchange={chooseWasm} />
        <p class="field-help">{wasmName || 'Chorus ABI v1 · no imports · maximum 4 MB'}</p></label
      ><label class="wide"
        >RELAY SET<textarea rows="2" bind:value={relays} placeholder="wss://relay.example.com"
        ></textarea></label
      >{#if !isPublic}<label class="wide"
          >MEMBER → COMMUNICATION KEY MAPPINGS<textarea
            rows="3"
            bind:value={communicationKeys}
            placeholder="member-npub communication-npub"
          ></textarea>
          <p class="field-help">
            One pair per line for every other member. This node’s mapping is added automatically.
          </p></label
        >{/if}<label class="check-label wide"
        ><input type="checkbox" bind:checked={stateful} />Stateful contract</label
      >{#if stateful}<label class="wide"
          >INITIAL STATE (UTF-8)<textarea bind:value={initialState} rows="2"></textarea></label
        >{/if}
    </div>
    {#snippet footer()}<button onclick={() => (modal = null)} disabled={busy}>Cancel</button><button
        class="primary"
        onclick={create}
        disabled={busy || !name || !members || !wasmHex}
        >{busy ? 'Validating and creating…' : 'Create federation'}</button
      >{/snippet}</Modal
  >{:else if modal === 'join'}<Modal
    title="Join federation"
    onclose={() => {
      if (!busy) modal = null;
    }}
    >{#if formError}<div class="notice error" role="alert">{formError}</div>{/if}
    <div class="fields">
      <label class="wide"
        >INVITATION FILE<input type="file" accept=".json" onchange={readInvite} /></label
      ><label class="wide"
        >OR PASTE INVITATION JSON<textarea
          rows="5"
          bind:value={invitation}
          class="mono"
          placeholder="Signed federation history and contract"
        ></textarea></label
      ><label class="wide"
        >TRUSTED GENESIS HASH<input
          class="mono"
          bind:value={genesisHash}
          placeholder="64-character SHA-256 hash"
        />
        <p class="field-help">
          Obtain this hash independently from a trusted federation member. Importing an invitation
          does not add your identity to the member set.
        </p></label
      >
    </div>
    {#snippet footer()}<button onclick={() => (modal = null)} disabled={busy}>Cancel</button><button
        class="primary"
        onclick={join}
        disabled={busy || !invitation || genesisHash.length !== 64}
        >{busy ? 'Verifying history…' : 'Verify and join'}</button
      >{/snippet}</Modal
  >{/if}
