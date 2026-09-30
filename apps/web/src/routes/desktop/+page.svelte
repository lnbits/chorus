<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  let ready = $state(false);
  let exists = $state(false);
  let error = $state('');
  let busy = $state(false);
  let username = $state('admin');
  let adminPassword = $state('');
  let password = $state('');
  let confirmation = $state('');
  let identityMode = $state('generate');
  let nsec = $state('');
  onMount(async () => {
    try {
      exists = await invoke<boolean>('initialized');
      ready = true;
    } catch {
      error = 'This page is available inside the Chorus desktop application.';
    }
  });
  async function start(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = '';
    try {
      if (!exists) {
        if (password !== confirmation) throw new Error('Keystore passwords do not match.');
        await invoke('initialize', {
          username,
          adminPassword,
          keyPassword: password,
          nsec: identityMode === 'import' ? nsec : null
        });
        nsec = '';
        exists = true;
        adminPassword = '';
        confirmation = '';
      }
      await invoke('start_local', { keyPassword: password });
      password = '';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="bootstrap">
  <p class="eyebrow">Chorus desktop</p>
  <h1>{exists ? 'Unlock your node.' : 'Create your node.'}</h1>
  <p class="muted">
    {exists
      ? 'Unlock the encrypted keystore to start your local federation node.'
      : 'Set up your admin account and one Nostr identity for all your federations. Keep your keystore password somewhere safe.'}
  </p>
  {#if error}<div class="notice error" role="alert">{error}</div>{/if}{#if ready}<form
      onsubmit={start}
      class="panel"
    >
      <div class="panel-body">
        {#if !exists}<label
            >NOSTR IDENTITY<select bind:value={identityMode}
              ><option value="generate">Generate a new key</option><option value="import"
                >Import my nsec</option
              ></select
            ></label
          >
          {#if identityMode === 'import'}<label
              >NOSTR NSEC<input
                type="password"
                bind:value={nsec}
                autocomplete="off"
                spellcheck={false}
                placeholder="nsec1…"
                required
              />
              <p class="field-help">
                Encrypted in your local keystore. Used for signing and private messages across your
                federations.
              </p></label
            >{/if}{/if}
        {#if !exists}<label
            >ADMIN USERNAME<input bind:value={username} autocomplete="username" required /></label
          ><label
            >ADMIN PASSWORD<input
              type="password"
              bind:value={adminPassword}
              minlength="12"
              autocomplete="new-password"
              required
            /></label
          >{/if}<label
          >KEYSTORE PASSWORD<input
            type="password"
            bind:value={password}
            minlength="12"
            autocomplete={exists ? 'current-password' : 'new-password'}
            required
          /></label
        >{#if !exists}<label
            >CONFIRM KEYSTORE PASSWORD<input
              type="password"
              bind:value={confirmation}
              minlength="12"
              autocomplete="new-password"
              required
            /></label
          >{/if}<button class="primary" disabled={busy}
          >{busy ? 'Starting node…' : exists ? 'Unlock and start' : 'Create and start'}</button
        >
      </div>
    </form>{/if}
</div>

<style>
  .bootstrap {
    max-width: 500px;
    margin: 65px auto;
    padding: 20px;
  }
  .bootstrap h1 {
    font-size: 36px;
    letter-spacing: -1px;
  }
  .bootstrap p {
    font-size: 14px;
  }
  .bootstrap label {
    margin-bottom: 20px;
  }
  .bootstrap button {
    width: 100%;
  }
</style>
