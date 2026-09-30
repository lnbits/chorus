<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { session, connection, refreshSession, logout, errorMessage } from '$lib/api';
  import Icon from '$lib/Icon.svelte';
  let { children } = $props();
  let light = $state(false);
  let error = $state('');
  onMount(() => {
    light = localStorage.getItem('chorus-theme') === 'light';
    document.documentElement.dataset.theme = light ? 'light' : 'dark';
    refreshSession();
  });
  function theme() {
    light = !light;
    document.documentElement.dataset.theme = light ? 'light' : 'dark';
    localStorage.setItem('chorus-theme', light ? 'light' : 'dark');
  }
  async function signout() {
    try {
      await logout();
      await goto('/');
    } catch (e) {
      error = errorMessage(e);
    }
  }
  let isApp = $derived(
    page.url.pathname !== '/' &&
      page.url.pathname !== '/protocol' &&
      page.url.pathname !== '/login' &&
      page.url.pathname !== '/desktop'
  );
  let nav = $derived(
    $session.authenticated
      ? [
          { href: '/federations', label: 'Federations', icon: 'federations' },
          { href: '/activity', label: 'Activity', icon: 'activity' },
          { href: '/settings', label: 'Settings', icon: 'settings' }
        ]
      : [
          { href: '/federations', label: 'Federations', icon: 'federations' },
          { href: '/protocol', label: 'How it works', icon: 'protocol' }
        ]
  );
</script>

<svelte:head
  ><title>Chorus — Federated computation</title><meta
    name="description"
    content="Independent execution. Shared agreement. Verifiable results. A Nostr-native federation host for deterministic WASM."
  /></svelte:head
>
<a class="skip-link" href="#main">Skip to content</a>
<header class="topbar">
  <a class="brand" href="/" aria-label="Chorus home"
    ><span class="brand-mark"><i></i><i></i><i></i></span><span>CHORUS</span></a
  >
  <nav class="topnav" aria-label="Main navigation">
    {#each nav as item (item.href)}<a
        href={item.href}
        class:active={page.url.pathname.startsWith(item.href)}>{item.label}</a
      >{/each}
  </nav>
  <div class="top-actions">
    <button class="theme-button" onclick={theme}>{light ? 'Dark' : 'Light'}</button
    >{#if $session.authenticated}<span class="username">{$session.username}</span>{:else}<a
        class="login-link"
        href="/login">Login <span aria-hidden="true">↗</span></a
      >{/if}
  </div>
</header>
{#if isApp}<aside class="sidebar">
    <nav aria-label="Workspace navigation">
      {#each nav as item (item.href)}<a
          href={item.href}
          class:active={page.url.pathname.startsWith(item.href)}
          ><Icon name={item.icon} />{item.label}</a
        >{/each}
    </nav>
    <div class="sidebar-bottom">
      <div class="node-indicator">
        <span
          class="dot {$connection === 'online'
            ? 'green'
            : $connection === 'connecting'
              ? 'amber'
              : 'red'}"
        ></span><span
          >{$connection === 'online'
            ? 'Node connected'
            : $connection === 'connecting'
              ? 'Connecting'
              : 'Node unavailable'}</span
        >
      </div>
      <p>CHORUS / PROTOCOL 01</p>
      {#if $session.authenticated}<button class="text-button" onclick={signout}
          ><Icon name="logout" />Logout</button
        >{/if}
    </div>
  </aside>{/if}
<main id="main" class:workspace={isApp}>
  {#if error}<div role="alert" class="notice error">{error}</div>{/if}{@render children()}
</main>
