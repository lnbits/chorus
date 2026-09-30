<script lang="ts">
  import { page } from '$app/state';
  import { onMount } from 'svelte';
  import {
    api,
    session,
    short,
    date,
    bytes,
    thresholdLabel,
    errorMessage,
    hex,
    downloadJson,
    type Federation,
    type JobRow
  } from '$lib/api';
  import Modal from '$lib/Modal.svelte';
  type Detail = Omit<Federation, 'relays'> & {
    relays: { url: string; status: string }[];
    member_npubs: string[];
    contract_size: number | null;
    current_state_hash: string | null;
    epochs: { state: Federation }[];
  };
  type Proposal = {
    id: string;
    proposal: { next_state: Federation };
    approvals: { pubkey: string }[];
    accepted: boolean;
  };
  let federation = $state<Detail | null>(null);
  let jobs = $state<JobRow[]>([]);
  let proposals = $state<Proposal[]>([]);
  let error = $state('');
  let loading = $state(true);
  let tab = $state('Overview');
  let busy = $state(false);
  let modal = $state<'job' | 'governance' | null>(null);
  let selected = $state<JobRow | null>(null);
  let command = $state('');
  let commandFormat = $state('text');
  let change = $state('threshold');
  let nextThreshold = $state(1);
  const nextMemberCount = $derived(
    (federation?.members.length || 0) + (change === 'add' ? 1 : change === 'remove' ? -1 : 0)
  );
  let memberKey = $state('');
  let contractHash = $state('');
  let notice = $state('');
  const id = $derived(page.params.id);
  const tabs = ['Overview', 'Contract', 'Members', 'Jobs', 'Governance', 'Verification'];
  async function load() {
    error = '';
    try {
      federation = await api<Detail>(`/federations/${id}`);
      [jobs, proposals] = await Promise.all([
        api<JobRow[]>(`/federations/${id}/jobs`),
        api<Proposal[]>(`/federations/${id}/proposals`)
      ]);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }
  onMount(() => {
    load();
    const timer = setInterval(load, 10000);
    return () => clearInterval(timer);
  });
  async function saveJob() {
    busy = true;
    error = '';
    try {
      await api(`/federations/${id}/jobs`, {
        method: 'POST',
        body: JSON.stringify({
          command_hex:
            commandFormat === 'hex'
              ? command.replace(/\s/g, '')
              : hex(new TextEncoder().encode(command))
        })
      });
      modal = null;
      command = '';
      tab = 'Jobs';
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  async function proposal() {
    if (!federation) return;
    busy = true;
    error = '';
    try {
      const next = {
        ...federation,
        relays: federation.epochs.at(-1)?.state.relays || [],
        frost: null
      };
      next.threshold = { type: 'absolute', value: nextThreshold };
      if (change === 'add') {
        next.members = [...next.members, memberKey.trim()];
        next.communication_keys = [
          ...next.communication_keys,
          [memberKey.trim(), memberKey.trim()]
        ];
      } else if (change === 'remove') {
        next.members = next.members.filter((m) => m !== memberKey);
        next.communication_keys = next.communication_keys.filter(([m]) => m !== memberKey);
      } else if (change === 'contract') next.wasm_hash = contractHash;
      else if (change === 'identity') next.communication_keys = next.members.map((m) => [m, m]);
      await api(`/federations/${id}/proposals`, { method: 'POST', body: JSON.stringify(next) });
      modal = null;
      tab = 'Governance';
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  async function approve(pid: string) {
    busy = true;
    error = '';
    try {
      await api(`/federations/${id}/proposals/${pid}/approve`, { method: 'POST' });
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  async function upload(e: Event) {
    const f = (e.target as HTMLInputElement).files?.[0];
    if (!f) return;
    busy = true;
    try {
      const result = await api<{ wasm_hash: string }>('/contracts', {
        method: 'POST',
        body: JSON.stringify({ wasm_hex: hex(new Uint8Array(await f.arrayBuffer())) })
      });
      contractHash = result.wasm_hash;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  async function download(path: string, name: string) {
    try {
      await downloadJson(path, name);
    } catch (e) {
      error = errorMessage(e);
    }
  }
  async function rerun(job: JobRow) {
    busy = true;
    try {
      selected = await api<JobRow>(`/jobs/${job.id}/execute`, { method: 'POST' });
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      notice = 'Copied to clipboard.';
    } catch {
      notice = 'Clipboard unavailable. Select and copy the command below.';
    }
  }
  const genesis = $derived(federation?.epochs[0]?.state);
</script>

<svelte:head><title>{federation?.name || 'Federation'} — Chorus</title></svelte:head>
{#if loading}<div class="loading">Loading federation…</div>{:else if federation}
  <div class="page-head">
    <div>
      <p class="eyebrow">
        <a href="/federations">Federations</a> / {federation.public ? 'Public' : 'Private'} federation
      </p>
      <h1>{federation.name}</h1>
    </div>
    <div class="actions">
      <button onclick={() => download(`/federations/${id}/invitation`, 'invitation.json')}
        >Export invitation</button
      >{#if $session.authenticated}<button class="primary" onclick={() => (modal = 'job')}
          >Submit job <span aria-hidden="true">+</span></button
        >{/if}
    </div>
  </div>
  <p class="muted" style="font-size:14px;margin-top:-13px">{federation.description}</p>
  {#if error}<div class="notice error" role="alert">{error}</div>{/if}{#if notice}<div
      class="notice"
      role="status"
    >
      {notice}
    </div>{/if}
  <div class="metrics">
    <div class="metric">
      <span class="metric-label">Members</span><strong>{federation.members.length}</strong><small
        >Current epoch identities</small
      >
    </div>
    <div class="metric">
      <span class="metric-label">Threshold</span><strong
        >{thresholdLabel(federation.threshold, federation.members.length)}</strong
      ><small>{federation.required} signatures required</small>
    </div>
    <div class="metric">
      <span class="metric-label">Current epoch</span><strong>{federation.epoch}</strong><small
        >Signed federation state</small
      >
    </div>
    <div class="metric">
      <span class="metric-label">Finalized jobs</span><strong>{federation.finalized_jobs}</strong
      ><small
        >{federation.status === 'quarantined'
          ? 'Federation quarantined'
          : 'Independent attestations'}</small
      >
    </div>
  </div>
  <div class="tabs" aria-label="Federation sections">
    {#each tabs as name (name)}<button class:active={tab === name} onclick={() => (tab = name)}
        >{name}</button
      >{/each}
  </div>
  {#if tab === 'Overview'}<div class="detail-grid">
      <div class="panel">
        <div class="panel-head">
          <h2>Federation state</h2>
          <span class="status"
            ><span class="dot {federation.status === 'active' ? 'green' : 'red'}"
            ></span>{federation.status}</span
          >
        </div>
        <div class="panel-body">
          <div class="kv"><span>FEDERATION ID</span><code class="hash">{id}</code></div>
          <div class="kv">
            <span>STATE HASH</span><code class="hash">{federation.state_hash}</code>
          </div>
          <div class="kv">
            <span>WASM HASH</span><code class="hash">{federation.wasm_hash}</code>
          </div>
          <div class="kv"><span>VM VERSION</span><span>{federation.vm_version}</span></div>
          <div class="kv">
            <span>EXECUTION STATE</span><code class="hash"
              >{federation.current_state_hash || 'Stateless'}</code
            >
          </div>
          <div class="kv"><span>CREATED</span><span>{date(federation.created_at)}</span></div>
        </div>
      </div>
      <div>
        <div class="panel">
          <div class="panel-head">
            <h2>Nostr relays</h2>
            <span class="mono"
              >{federation.relays.filter((r) => r.status === 'connected').length}/{federation.relays
                .length}</span
            >
          </div>
          {#each federation.relays as r (r.url)}<div class="relay-row">
              <span class="dot {r.status === 'connected' ? 'green' : 'red'}"></span><span
                class="hash">{r.url}</span
              ><span class="relay-state">{r.status}</span>
            </div>{:else}<div class="panel-body muted">
              No relays configured. Add relays in Settings.
            </div>{/each}
        </div>
        <div class="panel">
          <div class="panel-body">
            <h3>Every change is governed.</h3>
            <p class="muted" style="font-size:14px">
              Members, thresholds and contract upgrades require the current epoch’s threshold
              approval.
            </p>
            <button class="small" onclick={() => (tab = 'Governance')}>View governance ↗</button>
          </div>
        </div>
      </div>
    </div>
  {:else if tab === 'Contract'}<div class="panel">
      <div class="panel-head">
        <h2>WASM contract</h2>
        <a
          class="button small"
          href={`/api/v1/contracts/${federation.wasm_hash}`}
          download="contract.wasm">Download WASM ↓</a
        >
      </div>
      <div class="panel-body">
        <div class="kv"><span>SHA-256</span><code class="hash">{federation.wasm_hash}</code></div>
        <div class="kv">
          <span>SIZE</span><span
            >{federation.contract_size === null
              ? 'Not available locally'
              : bytes(federation.contract_size)}</span
          >
        </div>
        <div class="kv"><span>VM VERSION</span><span>{federation.vm_version}</span></div>
        <div class="kv">
          <span>VALIDATION</span><span>Deterministic profile validated on import</span>
        </div>
        <div class="kv"><span>HOST IMPORTS</span><span>None permitted</span></div>
        <div class="kv">
          <span>IDENTITY</span><span
            >The SHA-256 hash identifies this contract; the filename does not.</span
          >
        </div>
      </div>
    </div>
    {#if $session.authenticated}<button
        onclick={() => {
          change = 'contract';
          nextThreshold = federation!.threshold.value;
          modal = 'governance';
        }}>Propose contract replacement</button
      >{/if}
  {:else if tab === 'Members'}<div class="panel">
      <div class="panel-head">
        <h2>Epoch {federation.epoch} · {federation.members.length} members</h2>
        {#if $session.authenticated}<button
            class="small"
            onclick={() => {
              change = 'add';
              nextThreshold = federation!.threshold.value;
              modal = 'governance';
            }}>Propose member change</button
          >{/if}
      </div>
      <div class="table-wrap">
        <table>
          <thead><tr><th>Nostr identity</th><th>Status</th><th>Observed signed jobs</th></tr></thead
          ><tbody
            >{#each federation.members as m, i (m)}<tr
                ><td
                  ><span class="mono hash">{federation.member_npubs[i]}</span>
                  <p class="mono hash">{m}</p></td
                ><td><span class="status">Member</span></td><td
                  >{jobs.filter((j) => j.consensus.groups.some((g) => g.signers.includes(m)))
                    .length}
                  <p>In locally available history</p></td
                ></tr
              >{/each}</tbody
          >
        </table>
      </div>
    </div>
    <p class="muted" style="font-size:12px">
      Membership does not imply that a node is currently online. Participation is derived from
      verified attestations.
    </p>
  {:else if tab === 'Jobs'}<div class="panel">
      <div class="panel-head">
        <h2>Execution history</h2>
        <span class="muted mono">{jobs.length} jobs</span>
      </div>
      {#if jobs.length}<div class="table-wrap">
          <table>
            <thead
              ><tr><th>Job</th><th>Status</th><th>Agreement</th><th>Submitted</th><th></th></tr
              ></thead
            ><tbody
              >{#each jobs as j (j.id)}<tr
                  ><td class="mono"
                    >{short(j.id)}
                    <p>
                      EPOCH {j.job.epoch}{j.job.sequence !== null
                        ? ` · SEQUENCE ${j.job.sequence}`
                        : ''}
                    </p></td
                  ><td
                    ><span class="status"
                      ><span
                        class="dot {j.status === 'finalized'
                          ? 'green'
                          : j.status === 'disputed'
                            ? 'red'
                            : 'amber'}"
                      ></span>{j.status}</span
                    ></td
                  ><td
                    >{j.agreeing} / {j.member_count}
                    <p>{j.required} required</p></td
                  ><td>{date(j.job.created_at)}</td><td
                    ><button class="small" onclick={() => (selected = j)}>Details</button></td
                  ></tr
                >{/each}</tbody
            >
          </table>
        </div>{:else}<div class="empty">
          <h2>No jobs yet.</h2>
          <p>Submit an input to execute the federation’s approved contract.</p>
        </div>{/if}
    </div>
  {:else if tab === 'Governance'}<div class="panel">
      <div class="panel-head">
        <h2>Proposals</h2>
        {#if $session.authenticated}<button
            class="small"
            onclick={() => {
              change = 'threshold';
              nextThreshold = federation!.threshold.value;
              modal = 'governance';
            }}>Create proposal</button
          >{/if}
      </div>
      {#each proposals as p (p.id)}<div
          class="panel-body"
          style="border-bottom:1px solid var(--border)"
        >
          <div class="actions" style="justify-content:space-between">
            <div>
              <h3>Epoch {p.proposal.next_state.epoch} proposal</h3>
              <span class="mono muted">{short(p.id, 20)}</span>
            </div>
            <span class="status">{p.accepted ? 'Accepted' : 'Pending'}</span>
          </div>
          <p class="muted">
            {p.proposal.next_state.members.length} members · {thresholdLabel(
              p.proposal.next_state.threshold,
              p.proposal.next_state.members.length
            )} threshold · WASM {short(p.proposal.next_state.wasm_hash)}
          </p>
          <p class="muted" style="font-size:12px">
            {new Set(p.approvals.map((a) => a.pubkey)).size} unique approvals
          </p>
          <details>
            <summary>Review complete proposed state</summary>
            <pre style="margin-top:15px">{JSON.stringify(p.proposal.next_state, null, 2)}</pre>
          </details>
          {#if $session.authenticated && !p.accepted}<button
              style="margin-top:18px"
              disabled={busy}
              onclick={() => approve(p.id)}>Approve this proposal</button
            >{/if}
        </div>{:else}<div class="empty">
          <h2>No pending changes.</h2>
          <p>
            Every proposal is approved under the current threshold. A member can approve only one
            successor per epoch.
          </p>
        </div>{/each}
    </div>
    <div class="notice">
      Current rule: {federation.required} of {federation.members.length} members must approve. The proposed
      threshold only applies after activation.
    </div>
  {:else}<div class="panel">
      <div class="panel-head"><h2>Independent verification</h2></div>
      <div class="panel-body">
        <h2>Obtain. Hash. Execute. Verify.</h2>
        <p class="muted">
          Download a finalized job’s complete proof, then verify it independently with the Rust CLI.
        </p>
        <div class="kv"><span>FEDERATION ID</span><code class="hash">{id}</code></div>
        <div class="kv"><span>CURRENT EPOCH</span><span>{federation.epoch}</span></div>
        <div class="kv">
          <span>MEMBERS / RULE</span><span
            >{federation.members.length} / {thresholdLabel(
              federation.threshold,
              federation.members.length
            )}</span
          >
        </div>
        <div class="kv"><span>WASM HASH</span><code class="hash">{federation.wasm_hash}</code></div>
        <div class="kv">
          <span>COMPACT PROOF</span><span
            >{federation.frost
              ? 'FROST metadata configured'
              : 'Individual member attestations'}</span
          >
        </div>
        <div class="kv">
          <span>GENESIS EPOCH</span><span
            >{genesis?.epoch ?? 0} · Confirm its hash through an independent trusted channel.</span
          >
        </div>
      </div>
    </div>
    {#each jobs.filter((j) => j.has_proof) as j (j.id)}<div class="panel">
        <div class="panel-head">
          <code>{short(j.id, 20)}</code><button
            class="small"
            onclick={() => download(`/jobs/${j.id}/proof`, 'result.json')}>Download proof ↓</button
          >
        </div>
        <div class="panel-body">
          <code class="hash"
            >fedwasm verify result.json --genesis-hash &lt;trusted-genesis-hash&gt; --rerun</code
          >
          <div style="margin-top:16px">
            <button
              class="small"
              onclick={() =>
                copy('fedwasm verify result.json --genesis-hash <trusted-genesis-hash> --rerun')}
              >Copy command</button
            >
          </div>
        </div>
      </div>{:else}<div class="notice">
        Proof downloads appear after a job reaches threshold agreement and its output is available
        locally.
      </div>{/each}
    <p class="muted" style="font-size:13px">
      A proof attests to execution of the published contract and inputs. It does not establish that
      the business logic is economically correct.
    </p>{/if}
{:else}<div class="notice error">{error || 'Federation not found.'}</div>
  <a href="/federations" class="button">Back to federations</a>{/if}
{#if modal === 'job'}<Modal title="Submit job" onclose={() => (modal = null)}
    >{#if error}<div class="notice error">{error}</div>{/if}
    <p class="muted">
      Each member independently executes this input against epoch {federation?.epoch}.
    </p>
    <div class="fields">
      <label class="wide"
        >INPUT FORMAT<select bind:value={commandFormat}
          ><option value="text">UTF-8 text</option><option value="hex">Exact bytes (hex)</option
          ></select
        ></label
      ><label class="wide"
        >COMMAND / INPUT<textarea
          rows="7"
          bind:value={command}
          class="mono"
          placeholder="Contract-specific input"
        ></textarea></label
      >
    </div>
    <p class="field-help">
      The node constructs the canonical execution envelope and pins the current state. Jobs expire
      after one hour.
    </p>
    {#snippet footer()}<button onclick={() => (modal = null)} disabled={busy}>Cancel</button><button
        class="primary"
        onclick={saveJob}
        disabled={busy}>{busy ? 'Submitting…' : 'Submit for execution'}</button
      >{/snippet}</Modal
  >{/if}
{#if modal === 'governance'}<Modal title="Propose federation change" onclose={() => (modal = null)}
    >{#if error}<div class="notice error">{error}</div>{/if}
    <div class="fields">
      <label class="wide"
        >CHANGE<select bind:value={change}
          ><option value="threshold">Change threshold</option><option value="add">Add member</option
          ><option value="remove">Remove member</option><option value="contract"
            >Replace WASM contract</option
          ><option value="identity">Use member identities for private messaging</option></select
        ></label
      >{#if change === 'identity'}<p class="field-help wide">
          This changes each private-message destination to the member’s own npub. Members should
          upgrade to the single-identity version before approving. Historical epochs stay unchanged.
        </p>{:else if change === 'add'}<label class="wide"
          >MEMBER NPUB (OR HEX PUBLIC KEY)<input class="mono" bind:value={memberKey} /></label
        >{:else if change === 'remove'}<label class="wide"
          >MEMBER<select bind:value={memberKey}
            ><option value="">Choose a member</option
            >{#each federation?.members || [] as m (m)}<option value={m}>{short(m, 22)}</option
              >{/each}</select
          ></label
        >{:else if change === 'contract'}<label class="wide"
          >REPLACEMENT CONTRACT<input type="file" accept=".wasm" onchange={upload} /></label
        ><label class="wide">VALIDATED HASH<input readonly value={contractHash} /></label>{/if}
      <label class="wide"
        >NEW QUORUM · REQUIRED SIGNATURES<input
          type="number"
          min={Math.floor(nextMemberCount / 2) + 1}
          max={nextMemberCount}
          step="1"
          bind:value={nextThreshold}
        />
        <p class="field-help">
          Proposed: {nextThreshold}/{nextMemberCount}. Must exceed half the proposed member set.
          This signed proposal needs {federation
            ? thresholdLabel(federation.threshold, federation.members.length)
            : ''} current members to approve before activation.
        </p></label
      >
    </div>
    <p class="field-help" style="margin-top:25px">
      Review before approving. The current threshold governs this change. Membership or threshold
      changes remove the old FROST configuration and require a fresh key setup.
    </p>
    {#snippet footer()}<button onclick={() => (modal = null)} disabled={busy}>Cancel</button><button
        class="primary"
        onclick={proposal}
        disabled={busy || (change === 'contract' && !contractHash)}
        >{busy ? 'Validating…' : 'Create proposal'}</button
      >{/snippet}</Modal
  >{/if}
{#if selected}<Modal title="Job verification data" onclose={() => (selected = null)}
    ><div class="kv"><span>JOB ID</span><code class="hash">{selected.id}</code></div>
    <div class="kv"><span>STATUS</span><span>{selected.status.toUpperCase()}</span></div>
    <div class="kv"><span>INPUT HASH</span><code class="hash">{selected.job.input_hash}</code></div>
    <div class="kv">
      <span>PREVIOUS STATE</span><code class="hash"
        >{selected.job.previous_state_hash || 'Stateless'}</code
      >
    </div>
    {#if selected.consensus.equivocators.length}<div class="notice error">
        Equivocation detected: {selected.consensus.equivocators.join(', ')}
      </div>{/if}{#each selected.consensus.groups as group (group.commitment.result_hash)}<div
        class="panel"
        style="margin-top:22px"
      >
        <div class="panel-head"><h3>{group.signers.length} signatures</h3></div>
        <div class="panel-body">
          <p class="eyebrow">RESULT HASH</p>
          <code class="hash">{group.commitment.result_hash}</code>
          <p class="eyebrow" style="margin-top:18px">SIGNERS</p>
          {#each group.signers as signer (signer)}<p class="mono hash">{signer}</p>{/each}
          <details>
            <summary>Complete commitment and signatures</summary>
            <pre>{JSON.stringify(group, null, 2)}</pre>
          </details>
        </div>
      </div>{:else}<p class="muted">
        Awaiting independent member attestations.
      </p>{/each}{#snippet footer()}{#if $session.authenticated}<button
          disabled={busy}
          onclick={() => selected && rerun(selected)}>Re-execute locally</button
        >{/if}{#if selected?.has_proof}<button
          class="primary"
          onclick={() => selected && download(`/jobs/${selected.id}/proof`, 'result.json')}
          >Download proof</button
        >{/if}{/snippet}</Modal
  >{/if}
