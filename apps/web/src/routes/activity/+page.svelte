<script lang="ts">
  import { onMount } from 'svelte';
  import { api, date, short, errorMessage, session } from '$lib/api';
  import Icon from '$lib/Icon.svelte';
  type Activity = {
    id: number;
    kind: string;
    federation_id: string | null;
    target: string;
    message: string;
    created_at: number;
  };
  let entries = $state<Activity[]>([]);
  let error = $state('');
  let loading = $state(true);
  let filter = $state('all');
  const visible = $derived(entries.filter((e) => filter === 'all' || e.kind.includes(filter)));
  async function load() {
    loading = true;
    error = '';
    try {
      entries = await api<Activity[]>('/activity');
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);
</script>

<svelte:head><title>Activity — Chorus</title></svelte:head>
<div class="page-head">
  <div>
    <p class="eyebrow">Node event log</p>
    <h1>Activity</h1>
  </div>
  <button onclick={load} disabled={loading}>Refresh</button>
</div>
<div class="toolbar">
  <div class="segments">
    {#each [['all', 'All activity'], ['job', 'Jobs'], ['proposal', 'Governance'], ['relay', 'Relays']] as [value, label] (value)}<button
        class:active={filter === value}
        onclick={() => (filter = value)}>{label}</button
      >{/each}
  </div>
  <span class="muted" style="font-size:12px">Latest 500 events</span>
</div>
{#if error}<div class="notice error" role="alert">
    {error}{#if !$session.authenticated}
      <a class="inline-link" href="/login">Login</a>{/if}
  </div>{/if}
<div class="panel">
  {#if loading}<div class="loading">Loading activity…</div>{:else if visible.length}<div
      class="table-wrap"
    >
      <table>
        <thead><tr><th>Event</th><th>Details</th><th>Time</th><th></th></tr></thead><tbody
          >{#each visible as entry (entry.id)}<tr
              ><td
                ><span class="status"
                  ><span
                    class="dot {entry.kind.includes('failed') || entry.kind.includes('disputed')
                      ? 'red'
                      : entry.kind.includes('finalized')
                        ? 'green'
                        : ''}"
                  ></span>{entry.kind.replaceAll('_', ' ')}</span
                ></td
              ><td
                >{entry.message}{#if entry.federation_id}<p class="mono">
                    {short(entry.federation_id)}
                  </p>{/if}</td
              ><td style="white-space:nowrap">{date(entry.created_at)}</td><td
                >{#if entry.federation_id}<a
                    href={`/federations/${entry.federation_id}`}
                    class="button small">Open ↗</a
                  >{/if}</td
              ></tr
            >{/each}</tbody
        >
      </table>
    </div>{:else}<div class="empty">
      <div class="empty-symbol"><Icon name="activity" /></div>
      <h2>No activity to show.</h2>
      <p>Job results, governance changes and node events will appear here.</p>
    </div>{/if}
</div>
