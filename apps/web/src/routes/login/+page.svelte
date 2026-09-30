<script lang="ts">
  import { goto } from '$app/navigation';
  import { api, session, errorMessage, type Session } from '$lib/api';
  import Icon from '$lib/Icon.svelte';
  let username = $state('');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);
  async function submit(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = '';
    try {
      session.set(
        await api<Session>('/login', {
          method: 'POST',
          body: JSON.stringify({ username, password })
        })
      );
      password = '';
      await goto('/federations');
    } catch (e) {
      error = errorMessage(e);
      password = '';
    } finally {
      busy = false;
    }
  }
</script>

<svelte:head><title>Login — Chorus</title></svelte:head>
<div class="login-page">
  <a class="back muted" href="/">← Back to Chorus</a>
  <p class="eyebrow">Node administration</p>
  <h1>Login</h1>
  <p class="muted">Your node. Your federations.</p>
  <form onsubmit={submit}>
    <div class="panel">
      <div class="panel-body">
        {#if error}<div class="notice error" role="alert">{error}</div>{/if}<label
          >USERNAME<input autocomplete="username" required bind:value={username} /></label
        ><label
          >PASSWORD<input
            type="password"
            autocomplete="current-password"
            required
            bind:value={password}
          /></label
        ><button class="primary" disabled={busy}
          >{busy ? 'Signing in…' : 'Login'}<Icon name="arrow" size={17} /></button
        >
      </div>
    </div>
  </form>
  <div class="login-note">
    <Icon name="lock" size={15} /><span
      >Admin access is separate from your Nostr identity.<br />Signing keys stay on the node.</span
    >
  </div>
  <p class="setup-note">
    First time here? Initialize your node with <code>chorus-node init</code>, then sign in with your
    admin credentials.
  </p>
</div>

<style>
  .login-page {
    max-width: 460px;
    margin: 65px auto;
    padding: 0 20px;
  }
  .back {
    display: inline-block;
    font-size: 13px;
    margin-bottom: 45px;
  }
  .login-page h1 {
    font-size: 40px;
    letter-spacing: -1px;
    margin: 0;
  }
  .login-page > p.muted {
    font-size: 15px;
    margin: 13px 0 27px;
  }
  form label {
    margin-bottom: 24px;
  }
  form button {
    width: 100%;
    margin-top: 5px;
    justify-content: space-between;
  }
  .login-note {
    display: flex;
    gap: 12px;
    font-size: 12px;
    color: var(--muted);
    line-height: 1.6;
  }
  .setup-note {
    font-size: 12px;
    color: var(--muted);
    margin-top: 30px;
  }
</style>
